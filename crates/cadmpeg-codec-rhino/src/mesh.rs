// SPDX-License-Identifier: Apache-2.0
//! Bounded `ON_Mesh` decoding.
//!
//! Mesh channel kinds are codec-owned and their payloads are little-endian:
//! [`CHANNEL_UV`] is two `f32`, [`CHANNEL_COLOR`] is four direct `ON_Color`
//! bytes in memory order, [`CHANNEL_SURFACE_PARAMETERS`] is two `f64`,
//! [`CHANNEL_CURVATURE`] is two `f64`. Channel data is never unit-scaled.

use crate::loss::Diagnostics;
use std::borrow::Cow;
use std::ops::Range;

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteBinary32;
use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};
use sha1::{Digest, Sha1};

use crate::chunks::{
    chunk_at, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
};
use crate::curves::{error, GeometryError};
use crate::decode::session_ceiling;
use crate::objects::{ClassUserdata, UserdataDescriptor};
use crate::settings::MillimeterScale;
use crate::subd::MeshProxyFingerprint;
use crate::wire::{uuid, Uuid};

const EPS_MESH_SYNCHRONIZATION_OK_E6: f64 = 1.0e-6;

/// Decode context and root view used for mesh expansion.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshExpand<'a> {
    ctx: &'a DecodeContext<'a>,
    root: View<'a>,
}

impl<'a> MeshExpand<'a> {
    pub(crate) fn new(ctx: &'a DecodeContext<'a>, root: View<'a>) -> Self {
        Self { ctx, root }
    }

    pub(crate) fn data(self) -> &'a [u8] {
        self.root.window()
    }

    pub(crate) fn root(self) -> View<'a> {
        self.root
    }

    pub(crate) fn ctx(self) -> &'a DecodeContext<'a> {
        self.ctx
    }
}

/// Maps an expansion refusal to the mesh decoder error type.
fn expansion_refused(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    offset: usize,
    refusal: CodecError,
) -> Result<GeometryError, cadmpeg_core::CodecError> {
    Ok(match refusal {
        resource @ CodecError::ResourceLimit(_) => GeometryError::Codec(resource),
        other => error(
            offset,
            ctx.format_retained(
                format_args!("mesh buffer expansion refused: {other}"),
                "Rhino expansion_refused text",
            )?,
        ),
    })
}

/// `ON_Mesh` class UUID.
pub(crate) const ON_MESH: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xe4, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
/// V5 class-userdata UUID for the mesh double-precision vertex array.
pub(crate) const V5_MESH_DOUBLE_VERTICES: Uuid = Uuid::from_canonical([
    0x17, 0xf2, 0x4e, 0x75, 0x21, 0xbe, 0x4a, 0x7b, 0x9f, 0x3d, 0x7f, 0x85, 0x22, 0x52, 0x47, 0xe3,
]);
/// V4/V5 class-userdata UUID for the legacy mesh n-gon list.
pub(crate) const V4V5_MESH_NGON_USERDATA: Uuid = Uuid::from_canonical([
    0x31, 0xf5, 0x5a, 0xa3, 0x71, 0xfb, 0x49, 0xf5, 0xa9, 0x75, 0x75, 0x75, 0x84, 0xd9, 0x37, 0xff,
]);
/// `CTtMappingMeshInfoUserData` class and item UUID.
pub(crate) const TT_MAPPING_MESH_INFO_USERDATA: Uuid = Uuid::from_canonical([
    0x17, 0x06, 0xad, 0xc5, 0x52, 0xbf, 0x4b, 0xe2, 0x84, 0x02, 0x45, 0x01, 0xeb, 0x2a, 0xe6, 0x75,
]);
/// `CTtRenderMeshInfoUserData` class and item UUID.
pub(crate) const TT_RENDER_MESH_INFO_USERDATA: Uuid = Uuid::from_canonical([
    0x49, 0x60, 0xa0, 0x46, 0x82, 0x01, 0x4f, 0x0f, 0x8f, 0x22, 0xfc, 0xb6, 0xf9, 0x1c, 0x76, 0x5d,
]);
/// Anonymous userdata payload chunk.
const ANONYMOUS: u32 = 0x4000_8000;
/// `ON_opennurbs4_id`, the legacy mesh n-gon userdata application UUID.
const OPENNURBS4: Uuid = Uuid::from_canonical([
    0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe, 0x52, 0x0d,
]);
/// Codec-owned UV channel kind.
const CHANNEL_UV: u32 = 0x5248_0001;
/// Codec-owned color channel kind.
const CHANNEL_COLOR: u32 = 0x5248_0002;
/// Codec-owned surface-parameter channel kind.
const CHANNEL_SURFACE_PARAMETERS: u32 = 0x5248_0003;
/// Codec-owned curvature channel kind.
const CHANNEL_CURVATURE: u32 = 0x5248_0004;
/// Maximum vertex count declared by one mesh.
const MAX_MESH_VERTICES: usize = 1 << 24;
/// Maximum face count declared by one mesh.
const MAX_MESH_FACES: usize = 1 << 24;
/// Maximum number of legacy n-gon records accepted by the bounded reader.
const MAX_MESH_NGONS: usize = 1 << 20;
/// Maximum corners in one legacy n-gon record.
const MAX_MESH_NGON_CORNERS: usize = 100_000;
/// Maximum mapping-mesh face-source IDs accepted in one correspondence carrier.
const MAX_MESH_FACE_SOURCE_IDS: usize = 1 << 24;
/// Maximum decompressed size of one mesh buffer.
const MAX_BUFFER_OUTPUT: usize = 256 * 1024 * 1024;
/// Maximum admitted mesh-buffer bytes per document.
const MAX_DOCUMENT_BUFFER_OUTPUT: usize = 256 * 1024 * 1024;

/// Monotonic document-wide count of admitted mesh-buffer bytes.
#[derive(Debug, Clone)]
pub(crate) struct MeshBudget {
    used: usize,
    limit: usize,
}

impl MeshBudget {
    /// Creates an empty production document budget.
    pub(crate) fn new() -> Self {
        Self {
            used: 0,
            limit: MAX_DOCUMENT_BUFFER_OUTPUT,
        }
    }

    /// Caps admitted mesh-buffer bytes with the session retained-byte ceiling.
    pub(crate) fn from_session(ctx: &DecodeContext<'_>) -> Self {
        Self {
            used: 0,
            limit: session_ceiling(
                ctx.policy().limits.max_retained_bytes,
                MAX_DOCUMENT_BUFFER_OUTPUT,
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self { used: 0, limit }
    }

    /// Returns the admitted-byte count, for cross-module tests.
    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.used
    }

    /// Proves the document-local buffer-byte ceiling before allocation.
    fn admit(&self, ctx: &DecodeContext<'_>, bytes: usize) -> Result<usize, CodecError> {
        let total = self.used.checked_add(bytes).ok_or_else(|| {
            ctx.refuse_codec_limit("Rhino document mesh buffer bytes", u64::MAX, u64::MAX)
        })?;
        if total > self.limit {
            return Err(ctx.refuse_codec_limit(
                "Rhino document mesh buffer bytes",
                u64_from_index(self.limit),
                u64_from_index(total),
            ));
        }
        Ok(total)
    }
}

fn buffer_output_limit(expand: MeshExpand<'_>) -> usize {
    session_ceiling(
        expand.ctx.policy().limits.max_decompressed_bytes_per_expand,
        MAX_BUFFER_OUTPUT,
    )
}

/// A decoded mesh and non-fatal channel warnings.
#[derive(Debug, Clone)]
pub(crate) struct DecodedMesh {
    /// Typed IR tessellation.
    pub(crate) tessellation: Tessellation,
    /// Per-object warnings.
    pub(crate) warnings: Diagnostics,
    /// Typed losses raised while selecting writer-version-dependent fields.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Whether source coordinates were converted to millimeters.
    pub(crate) scaled: bool,
    /// Number of stored n-gon group records not represented in the IR.
    pub(crate) ngon_count: usize,
    /// Number of stored quadrilateral faces converted to neutral triangles.
    pub(crate) quad_count: usize,
    /// Native mesh arrays used to validate an attached `SubD` proxy.
    pub(crate) proxy_fingerprint: Option<MeshProxyFingerprint>,
}

/// Caller-owned identity and archive metadata for one mesh decode.
pub(crate) struct MeshDecodeOptions<'a> {
    /// Source writer version used by version-gated fields.
    pub(crate) writer_version: Option<i64>,
    /// Source-object association assigned to the tessellation.
    pub(crate) association: Option<cadmpeg_ir::SourceObjectAssociation>,
    /// Deterministic tessellation ID.
    pub(crate) id: MeshId,
    /// Native-unit to millimeter scale.
    pub(crate) scale: MillimeterScale,
    /// Class userdata attached to the owning mesh object.
    pub(crate) userdata: &'a [UserdataDescriptor],
}

pub(crate) enum MeshId {
    Ready(cadmpeg_ir::tessellation::TessellationId),
    ExtrusionCache(usize),
    V5ExtrusionCache(usize),
}

impl MeshId {
    fn into_tessellation_id(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<cadmpeg_ir::tessellation::TessellationId, CodecError> {
        match self {
            Self::Ready(id) => Ok(id),
            Self::ExtrusionCache(index) => {
                cadmpeg_ir::tessellation::TessellationId::mint(ctx.format_retained(
                    format_args!("rhino:extrusion:mesh-cache#{index}"),
                    "Rhino extrusion mesh-cache ID",
                )?)
                .or_else(|error| {
                    Err(CodecError::Malformed(ctx.format_retained(
                        format_args!("{error}"),
                        "Rhino into_tessellation_id text",
                    )?))
                })
            }
            Self::V5ExtrusionCache(index) => {
                cadmpeg_ir::tessellation::TessellationId::mint(ctx.format_retained(
                    format_args!("rhino:extrusion:v5-mesh-cache#{index}"),
                    "Rhino V5 extrusion mesh-cache ID",
                )?)
                .or_else(|error| {
                    Err(CodecError::Malformed(ctx.format_retained(
                        format_args!("{error}"),
                        "Rhino into_tessellation_id text",
                    )?))
                })
            }
        }
    }
}

#[derive(Default)]
struct MeshChannels {
    vertices: Vec<[FiniteBinary32; 3]>,
    /// The normal lane, absent when the archive carries no normal channel.
    normals: Option<Vec<FiniteVector3>>,
    channels: Vec<TessellationChannel>,
    warnings: Diagnostics,
    losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Returns whether a UUID is `ON_Mesh`.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    uuid == ON_MESH
}

/// Decodes one bounded `ON_Mesh` class-data payload.
pub(crate) fn decode(
    expand: MeshExpand<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    options: MeshDecodeOptions<'_>,
    document_budget: &mut MeshBudget,
) -> Result<DecodedMesh, GeometryError> {
    let MeshDecodeOptions {
        writer_version,
        association,
        id,
        scale,
        userdata,
    } = options;
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let version = reader.u8()?;
    let major = version >> 4;
    let minor = version & 0x0f;
    if major == 2 || major == 0 || major > 3 {
        return Err(GeometryError::unsupported(
            reader.position() - 1,
            "unsupported ON_Mesh major",
        ));
    }
    if major == 3 && archive == ArchiveVersion::V5 && minor > 5 {
        return Err(GeometryError::unsupported(
            reader.position() - 1,
            "mesh minor is newer than the V5 writer band",
        ));
    }
    let vertex_count = count(&mut reader, MAX_MESH_VERTICES)?;
    let face_count = count(&mut reader, MAX_MESH_FACES)?;
    for _ in 0..4 {
        interval(&mut reader)?;
    }
    reader.f64()?;
    reader.f64()?;
    for _ in 0..16 {
        reader.f32()?;
    }
    reader.i32()?;
    let parameters_present = reader.u8()?;
    if parameters_present > 1 {
        return Err(error(
            reader.position() - 1,
            "invalid mesh-parameters presence",
        ));
    }
    if parameters_present != 0 {
        consume_optional_chunk(&mut reader, archive)?;
    }
    for _ in 0..4 {
        let present = reader.u8()?;
        if present > 1 {
            return Err(error(
                reader.position() - 1,
                "invalid curvature-stat presence",
            ));
        }
        if present != 0 {
            consume_optional_chunk(&mut reader, archive)?;
        }
    }
    let mut vertex_storage = expand
        .ctx()
        .reserve_scoped(0, "Rhino mesh source vertex scratch")?;
    let mut double_storage = None;
    let mut face_storage = expand.ctx().reserve_scoped(0, "Rhino mesh face scratch")?;
    let mut decoded = MeshChannels::default();
    let faces = face_storage
        .with_storage(|| read_faces(expand.ctx(), &mut reader, vertex_count, face_count))?;
    let mut ngon_count = 0;
    if major == 1 {
        read_raw_channels(
            expand.ctx(),
            &mut vertex_storage,
            &mut reader,
            vertex_count,
            &mut decoded,
        )?;
    } else {
        read_compressed_channels(
            expand,
            &mut vertex_storage,
            &mut reader,
            vertex_count,
            &mut decoded,
            document_budget,
            archive,
        )?;
    }
    if minor >= 2 {
        reader.i32()?;
    }
    if major == 3 && minor >= 3 {
        let _mapping_id = uuid(&mut reader)?;
        let mut surface_storage = expand
            .ctx()
            .reserve_scoped(0, "Rhino mesh surface buffer scratch")?;
        let surface = read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: vertex_count * 16,
                name: "surface parameters",
            },
            &mut decoded.warnings,
            document_budget,
            archive,
            Some(&mut surface_storage),
        )?;
        if let Some(bytes) = surface {
            expand
                .ctx()
                .reserve_vec(&mut decoded.channels, 1, "Rhino mesh channel entries")?;
            decoded.channels.push(channel(
                expand.ctx(),
                CHANNEL_SURFACE_PARAMETERS,
                16,
                match bytes {
                    Cow::Owned(bytes) => surface_storage.commit_value(bytes)?,
                    Cow::Borrowed(bytes) => {
                        expand.ctx().copy_retained(bytes, "rhino_mesh_buffer")?
                    }
                },
            )?);
        }
    }
    let post_2006_fields =
        major == 3 && minor >= 4 && writer_version.is_some_and(|version| version >= 200_606_010);
    if post_2006_fields {
        read_mapping_tag(expand.ctx(), &mut reader, archive, &mut decoded.warnings)?;
        if minor >= 5 {
            for _ in 0..3 {
                let value = reader.u8()?;
                if value > 2 {
                    decoded.warnings.push_admitted(
                        expand.ctx(),
                        format_args!("invalid mesh tri-state flag retained"),
                    )?;
                }
            }
        }
        if minor >= 6 && reader.bool_with_writer_version(writer_version)? {
            ngon_count = read_ngons(
                expand.ctx(),
                &mut reader,
                archive,
                vertex_count,
                face_count,
                &mut decoded.warnings,
            )?;
        }
    }
    let mut double_vertices = None;
    if post_2006_fields {
        if minor >= 7 && reader.bool_with_writer_version(writer_version)? {
            let mut buffer_storage = expand
                .ctx()
                .reserve_scoped(0, "Rhino mesh double buffer scratch")?;
            let (count, bytes) = read_double_chunk(
                expand,
                &mut reader,
                archive,
                &mut decoded.warnings,
                vertex_count,
                document_budget,
                &mut buffer_storage,
            )?;
            if count == vertex_count {
                if let Some(bytes) = bytes {
                    let mut numeric_storage = expand
                        .ctx()
                        .reserve_scoped(0, "Rhino mesh raw double scratch")?;
                    let mut candidate_storage = expand
                        .ctx()
                        .reserve_scoped(0, "Rhino mesh finite double scratch")?;
                    let values =
                        numeric_storage.with_storage(|| parse_f64_points(expand.ctx(), &bytes))?;
                    let mut finite = candidate_storage.with_storage(|| {
                        expand
                            .ctx()
                            .collection_vec(values.len(), "Rhino mesh admitted double vertices")
                    })?;
                    let valid = expand.ctx().all_by(
                        &values,
                        |point| {
                            if let Some(point) =
                                FinitePoint3::new(Point3::new(point[0], point[1], point[2]))
                            {
                                finite.push(point);
                                Ok(true)
                            } else {
                                Ok(false)
                            }
                        },
                        "Rhino mesh double finite validation",
                    )?;
                    if valid && synchronization_ok(expand.ctx(), &values, &decoded.vertices)? {
                        double_vertices = Some(finite);
                        double_storage = Some(candidate_storage);
                    } else {
                        decoded.warnings.push_admitted(
                            expand.ctx(),
                            format_args!("double vertices rejected; using float vertices"),
                        )?;
                    }
                }
            } else {
                decoded.warnings.push_coded_admitted(
                    expand.ctx(),
                    crate::loss::RhinoLossCode::RedundantFieldRepaired,
                    format_args!(
                        "redundant mesh double vertex count mismatch; using float vertices"
                    ),
                )?;
            }
        }
        if minor >= 8 {
            for _ in 0..6 {
                reader.f64()?;
            }
        }
    }
    if ngon_count == 0 {
        if let Some(extra) = expand.ctx().find_map(
            userdata,
            |raw| {
                let Some(value) = UserdataDescriptor::known(raw) else {
                    return Ok(None);
                };
                Ok((value.class_uuid == V4V5_MESH_NGON_USERDATA
                    && value.item_uuid == V4V5_MESH_NGON_USERDATA
                    && (value.application_uuid.is_none()
                        || value.application_uuid == Some(OPENNURBS4)))
                .then_some(value))
            },
            "Rhino decode traversal",
        )? {
            match read_v4v5_ngon_userdata(
                expand.ctx(),
                data,
                extra,
                archive,
                vertex_count,
                face_count,
            ) {
                Ok(Some(count)) => ngon_count = count,
                Ok(None) => decoded.warnings.push_admitted(
                    expand.ctx(),
                    format_args!(
                        "V4/V5 mesh n-gon userdata at offset {} was rejected; grouping omitted",
                        extra.range.start
                    ),
                )?,
                Err(error @ GeometryError::Codec(_)) => return Err(error),
                Err(error) => decoded.warnings.push_admitted(
                    expand.ctx(),
                    format_args!(
                        "V4/V5 mesh n-gon userdata at offset {} was dropped: {error}",
                        extra.range.start
                    ),
                )?,
            }
        }
    }
    if major == 3 && minor >= 4 && !post_2006_fields {
        let dropped = reader.skip_remaining()?;
        if dropped != 0 && writer_version.is_none() {
            expand
                .ctx()
                .reserve_vec(&mut decoded.losses, 1, "Rhino mesh losses")?;
            decoded.losses.push(crate::wire::admitted_loss(
                expand.ctx(),
                crate::loss::RhinoLossCode::SourceWriterStampUnverified,
                format_args!("ON_Mesh dropped {dropped} bytes of post-2006 fields (mapping tag, n-gons, double-precision vertices) because the archive has no writer-version stamp"),
                "Rhino mesh loss text",
            )?);
        }
    }
    let skipped = reader.skip_remaining()?;
    if skipped != 0 {
        decoded.warnings.push_admitted(
            expand.ctx(),
            format_args!("ON_Mesh skipped {skipped} trailing bytes"),
        )?;
    }
    if double_vertices.is_none() {
        if let Some(extra) = expand.ctx().find_map(
            userdata,
            |raw| {
                let Some(value) = UserdataDescriptor::known(raw) else {
                    return Ok(None);
                };
                Ok((value.class_uuid == V5_MESH_DOUBLE_VERTICES
                    && value.item_uuid == V5_MESH_DOUBLE_VERTICES)
                    .then_some(value))
            },
            "Rhino decode traversal",
        )? {
            let mut candidate_storage = expand
                .ctx()
                .reserve_scoped(0, "Rhino V5 mesh double scratch")?;
            match candidate_storage.with_storage(|| read_v5_double_vertices(expand.ctx(), data, extra, archive, &decoded.vertices)) {
                Ok(Some(values)) => { double_vertices = Some(values); double_storage = Some(candidate_storage); },
                Ok(None) => decoded.warnings.push_coded_admitted(expand.ctx(), crate::loss::RhinoLossCode::RedundantFieldRepaired, format_args!(
                    "redundant V5 mesh double-precision userdata at offset {} was rejected; using float vertices",
                    extra.range.start
                ))?,
                Err(error @ GeometryError::Codec(_)) => return Err(error),
                Err(error) => decoded.warnings.push_coded_admitted(expand.ctx(), crate::loss::RhinoLossCode::RedundantFieldRepaired, format_args!(
                    "redundant V5 mesh double-precision userdata at offset {} was dropped: {error}",
                    extra.range.start
                ))?,
            }
        }
    }
    for (class, label, mapping) in [
        (
            TT_MAPPING_MESH_INFO_USERDATA,
            "CTtMappingMeshInfoUserData",
            true,
        ),
        (
            TT_RENDER_MESH_INFO_USERDATA,
            "CTtRenderMeshInfoUserData",
            false,
        ),
    ] {
        expand.ctx().fold(
            userdata,
            (),
            |(), raw| {
                let Some(extra) = UserdataDescriptor::known(raw) else {
                    return Ok(());
                };
                if extra.class_uuid != class || extra.item_uuid != class {
                    return Ok(());
                }
                if let Err(error) =
                    parse_mesh_correspondence_userdata(data, extra.payload_range.clone(), mapping)
                {
                    decoded.warnings.push_admitted(
                        expand.ctx(),
                        format_args!(
                            "{label} userdata at offset {} could not be transferred: {error}",
                            extra.range.start
                        ),
                    )?;
                }
                Ok(())
            },
            "Rhino decode traversal",
        )?;
    }
    let proxy_fingerprint = if expand.ctx().any_by(
        userdata,
        |raw| {
            let Some(extra) = UserdataDescriptor::known(raw) else {
                return Ok(false);
            };
            Ok(extra.class_uuid == crate::subd::SUBD_MESH_PROXY_USERDATA
                && extra.item_uuid == crate::subd::SUBD_MESH_PROXY_USERDATA)
        },
        "Rhino mesh proxy userdata scan",
    )? {
        Some(native_proxy_fingerprint(
            &faces,
            &decoded.vertices,
            expand.ctx(),
        )?)
    } else {
        None
    };
    let mut vertices = expand
        .ctx()
        .collection_vec(decoded.vertices.len(), "Rhino mesh scaled vertices")
        .map_err(crate::curves::GeometryError::from)?;
    let mut append = |point: [f64; 3]| -> Result<(), GeometryError> {
        let [Some(x), Some(y), Some(z)] =
            point.map(|coordinate| crate::wire::scaled_coordinate(coordinate, scale))
        else {
            return Err(error(reader.position(), "scaled mesh vertex is invalid"));
        };
        vertices.push(FinitePoint3::from_coordinates(x, y, z));
        Ok(())
    };
    if let Some(double_vertices) = double_vertices {
        let mut points = double_vertices.into_iter();
        for _ in 0..points.len() {
            let point = expand
                .ctx()
                .next_charged(&mut points, "Rhino double mesh vertex scaling")?
                .ok_or_else(|| {
                    GeometryError::unpositioned("mesh double vertex source ended early")
                })?
                .get();
            append([point.x, point.y, point.z])?;
        }
        drop(points);
    } else {
        let mut points = std::mem::take(&mut decoded.vertices).into_iter();
        for _ in 0..points.len() {
            let point = expand
                .ctx()
                .next_charged(&mut points, "Rhino float mesh vertex scaling")?
                .ok_or_else(|| {
                    GeometryError::unpositioned("mesh float vertex source ended early")
                })?;
            append(point.map(|value| f64::from(value.get())))?;
        }
        drop(points);
    }
    drop(decoded.vertices);
    drop(vertex_storage);
    drop(double_storage);
    let quad_count = quad_face_count(expand.ctx(), &faces)?;
    let triangles = triangulate_faces(expand.ctx(), &faces, &vertices, FinitePoint3::get)?;
    let id = id.into_tessellation_id(expand.ctx())?;
    Ok(DecodedMesh {
        tessellation: Tessellation::from_parts(
            id,
            cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
                vertices,
                triangles,
                decoded.normals,
            )
            .or_else(|lanes| {
                Err(error(
                    reader.position(),
                    expand
                        .ctx()
                        .format_retained(format_args!("{lanes}"), "Rhino decode text")?,
                ))
            })?,
            decoded.channels,
        )
        .or_else(|err| {
            Err(error(
                reader.position(),
                expand
                    .ctx()
                    .format_retained(format_args!("{err}"), "Rhino decode text")?,
            ))
        })?
        .with_source_object(association),
        warnings: decoded.warnings,
        losses: decoded.losses,
        scaled: scale != MillimeterScale::IDENTITY,
        ngon_count,
        quad_count,
        proxy_fingerprint,
    })
}

/// Reads one current `CTt` mesh-correspondence carrier without admitting its
/// recomputable cache state to the neutral model.
fn parse_mesh_correspondence_userdata(
    data: &[u8],
    payload_range: Range<usize>,
    mapping: bool,
) -> Result<(), GeometryError> {
    let mut reader = BoundedReader::new(data, payload_range.start, payload_range.end)?;
    let version = reader.i32()?;
    if version != 1 {
        return Err(GeometryError::unsupported(
            payload_range.start,
            "mesh correspondence userdata version is unsupported",
        ));
    }
    reader.i32()?;
    for _ in 0..30 {
        reader.f64()?;
    }
    if mapping {
        let count_offset = reader.position();
        let raw_count = reader.i32()?;
        let count = usize::try_from(raw_count).map_err(|_| {
            error(
                count_offset,
                "mesh correspondence face-source count is negative",
            )
        })?;
        if count > MAX_MESH_FACE_SOURCE_IDS {
            return Err(error(
                count_offset,
                "mesh correspondence face-source count exceeds cap",
            ));
        }
        let byte_count = count.checked_mul(4).ok_or_else(|| {
            error(
                count_offset,
                "mesh correspondence face-source size overflow",
            )
        })?;
        reader.take(byte_count)?;
    } else {
        reader.i32()?;
    }
    reader.skip_remaining()?;
    Ok(())
}

fn native_proxy_fingerprint(
    faces: &[[u32; 4]],
    vertices: &[[FiniteBinary32; 3]],
    ctx: &DecodeContext<'_>,
) -> Result<MeshProxyFingerprint, CodecError> {
    let mut face_digest = Sha1::new();
    for face in ctx.admit_iter(faces, "Rhino mesh proxy SHA-1")? {
        for index in face {
            face_digest.update(index.to_ne_bytes());
        }
    }
    let mut vertex_digest = Sha1::new();
    for vertex in ctx.admit_iter(vertices, "Rhino mesh proxy SHA-1")? {
        for coordinate in vertex {
            vertex_digest.update(coordinate.get().to_ne_bytes());
        }
    }
    Ok(MeshProxyFingerprint {
        face_count: faces.len(),
        vertex_count: vertices.len(),
        face_sha1: face_digest.finalize().into(),
        vertex_sha1: vertex_digest.finalize().into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaceIndexWidth {
    One,
    Two,
    Four,
}

impl FaceIndexWidth {
    pub(crate) fn bytes(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
        }
    }

    fn from_value(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            4 => Some(Self::Four),
            _ => None,
        }
    }
}

fn read_faces(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    vertices: usize,
    faces: usize,
) -> Result<Vec<[u32; 4]>, GeometryError> {
    let width = FaceIndexWidth::from_value(reader.i32()?)
        .ok_or_else(|| error(reader.position() - 4, "invalid mesh face index width"))?;
    let bytes = faces
        .checked_mul(4)
        .and_then(|value| value.checked_mul(width.bytes()))
        .ok_or_else(|| error(reader.position(), "mesh face byte count overflow"))?;
    let raw = reader.take(bytes)?;
    let mut result = ctx
        .collection_vec(faces, "Rhino mesh faces")
        .map_err(crate::curves::GeometryError::from)?;
    for face in 0..faces {
        ctx.charge_work(1, "Rhino mesh read_faces records")?;
        let mut indices = [0_u32; 4];
        for (slot, index) in indices.iter_mut().enumerate() {
            let offset = (face * 4 + slot) * width.bytes();
            let Some(value) = face_index(raw, offset, width) else {
                return Err(error(
                    reader.position(),
                    format!("mesh face index payload is truncated at offset {offset}"),
                ));
            };
            *index = value;
            if (usize::try_from(*index).map_err(|_| {
                GeometryError::unpositioned("mesh count or index exceeds address space")
            })?) >= vertices
            {
                return Err(error(reader.position(), "mesh face index out of range"));
            }
        }
        result.push(indices);
    }
    Ok(result)
}

pub(crate) fn triangulate_faces<P: Copy>(
    ctx: &DecodeContext<'_>,
    faces: &[[u32; 4]],
    vertices: &[P],
    point: impl Fn(P) -> Point3,
) -> Result<Vec<[u32; 3]>, GeometryError> {
    let triangle_count = ctx
        .admit_iter(faces, "Rhino triangulate faces traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .try_fold(0_usize, |count, face| {
            count.checked_add(match unique_face_vertices(face) {
                3 => 1,
                4 => 2,
                _ => 0,
            })
        });
    let triangle_count = triangle_count
        .ok_or_else(|| GeometryError::unpositioned("mesh triangle count overflow"))?;
    let mut triangles = ctx
        .collection_vec(triangle_count, "Rhino mesh triangles")
        .map_err(crate::curves::GeometryError::from)?;
    for face in ctx
        .admit_iter(faces, "Rhino triangulate faces traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let unique_count = unique_face_vertices(face);
        if unique_count == 3 {
            let mut unique = [0_u32; 3];
            let mut count = 0;
            for index in face {
                if !unique[..count].contains(index) {
                    unique[count] = *index;
                    count += 1;
                }
            }
            triangles.push([unique[0], unique[1], unique[2]]);
        } else if unique_count == 4 {
            let diagonal_02 = point(
                vertices[usize::try_from(face[0]).map_err(|_| {
                    GeometryError::unpositioned("mesh count or index exceeds address space")
                })?],
            )
            .distance(point(
                vertices[usize::try_from(face[2]).map_err(|_| {
                    GeometryError::unpositioned("mesh count or index exceeds address space")
                })?],
            ));
            let diagonal_13 = point(
                vertices[usize::try_from(face[1]).map_err(|_| {
                    GeometryError::unpositioned("mesh count or index exceeds address space")
                })?],
            )
            .distance(point(
                vertices[usize::try_from(face[3]).map_err(|_| {
                    GeometryError::unpositioned("mesh count or index exceeds address space")
                })?],
            ));
            if diagonal_02 <= diagonal_13 {
                triangles.extend([[face[0], face[1], face[2]], [face[0], face[2], face[3]]]);
            } else {
                triangles.extend([[face[0], face[1], face[3]], [face[1], face[2], face[3]]]);
            }
        }
    }
    Ok(triangles)
}

fn quad_face_count(ctx: &DecodeContext<'_>, faces: &[[u32; 4]]) -> Result<usize, CodecError> {
    Ok(ctx
        .admit_iter(faces, "Rhino mesh quad count")?
        .filter(|face| unique_face_vertices(face) == 4)
        .count())
}

fn unique_face_vertices(face: &[u32; 4]) -> usize {
    let mut unique = [0_u32; 4];
    let mut count = 0;
    for index in face {
        if !unique[..count].contains(index) {
            unique[count] = *index;
            count += 1;
        }
    }
    count
}

fn face_index(raw: &[u8], offset: usize, width: FaceIndexWidth) -> Option<u32> {
    match width {
        FaceIndexWidth::One => raw.get(offset).copied().map(u32::from),
        FaceIndexWidth::Two => View::u16_le_at(raw, offset).map(u32::from),
        FaceIndexWidth::Four => View::u32_le_at(raw, offset),
    }
}

fn read_raw_channels(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    reader: &mut BoundedReader<'_>,
    vertices: usize,
    decoded: &mut MeshChannels,
) -> Result<(), GeometryError> {
    let MeshChannels {
        vertices: points,
        normals,
        channels,
        warnings,
        ..
    } = decoded;
    let vertex_bytes = read_counted_raw(ctx, reader, vertices, 12, "vertices", warnings)?;
    if let Some(bytes) = vertex_bytes {
        *points = storage.with_storage(|| parse_f32_points(ctx, bytes))?;
    }
    let normal_bytes = read_counted_raw(ctx, reader, vertices, 12, "normals", warnings)?;
    if let Some(bytes) = normal_bytes {
        match parse_f32_vectors(ctx, bytes) {
            Ok(value) => *normals = Some(value),
            Err(error @ GeometryError::Codec(_)) => return Err(error),
            Err(_) => warnings.push_admitted(
                ctx,
                format_args!("normals channel contains nonfinite values"),
            )?,
        }
    }
    let uv = read_counted_raw(ctx, reader, vertices, 8, "UV", warnings)?;
    if let Some(bytes) = uv {
        ctx.reserve_vec(channels, 1, "Rhino mesh channel entries")?;
        channels.push(channel(
            ctx,
            CHANNEL_UV,
            8,
            ctx.copy_retained(bytes, "Rhino mesh raw UV channel")?,
        )?);
    }
    let curvature = read_counted_raw(ctx, reader, vertices, 16, "curvature", warnings)?;
    if let Some(bytes) = curvature {
        ctx.reserve_vec(channels, 1, "Rhino mesh channel entries")?;
        channels.push(channel(
            ctx,
            CHANNEL_CURVATURE,
            16,
            ctx.copy_retained(bytes, "Rhino mesh raw curvature channel")?,
        )?);
    }
    let colors = read_counted_raw(ctx, reader, vertices, 4, "colors", warnings)?;
    if let Some(bytes) = colors {
        ctx.reserve_vec(channels, 1, "Rhino mesh channel entries")?;
        channels.push(channel(
            ctx,
            CHANNEL_COLOR,
            4,
            ctx.copy_retained(bytes, "Rhino mesh raw color channel")?,
        )?);
    }
    if points.len() != vertices {
        return Err(error(reader.position(), "mesh vertex channel is required"));
    }
    Ok(())
}

/// What one compressed mesh vertex channel decodes into.
#[derive(Clone, Copy)]
enum MeshChannelAction {
    Vertices,
    Normals,
    Raw(u32),
}

/// One compressed mesh vertex channel: its name, per-vertex size and decode action.
#[derive(Clone, Copy)]
struct MeshChannelSpec {
    name: &'static str,
    item_size: u32,
    action: MeshChannelAction,
}

impl MeshChannelSpec {
    const ALL: [Self; 5] = [
        Self {
            name: "vertices",
            item_size: 12,
            action: MeshChannelAction::Vertices,
        },
        Self {
            name: "normals",
            item_size: 12,
            action: MeshChannelAction::Normals,
        },
        Self {
            name: "UV",
            item_size: 8,
            action: MeshChannelAction::Raw(CHANNEL_UV),
        },
        Self {
            name: "curvature",
            item_size: 16,
            action: MeshChannelAction::Raw(CHANNEL_CURVATURE),
        },
        Self {
            name: "colors",
            item_size: 4,
            action: MeshChannelAction::Raw(CHANNEL_COLOR),
        },
    ];
}

fn read_compressed_channels(
    expand: MeshExpand<'_>,
    vertex_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    reader: &mut BoundedReader<'_>,
    vertices: usize,
    decoded: &mut MeshChannels,
    document_budget: &mut MeshBudget,
    archive: ArchiveVersion,
) -> Result<(), GeometryError> {
    for spec in MeshChannelSpec::ALL {
        let mut buffer_storage = expand
            .ctx()
            .reserve_scoped(0, "Rhino mesh numeric buffer scratch")?;
        let bytes = read_buffer(
            expand,
            reader,
            MeshBufferSpec {
                expected: vertices
                    * usize::try_from(spec.item_size).map_err(|_| {
                        GeometryError::unpositioned("mesh count or index exceeds address space")
                    })?,
                name: spec.name,
            },
            &mut decoded.warnings,
            document_budget,
            archive,
            Some(&mut buffer_storage),
        )?;
        let Some(bytes) = bytes else { continue };
        match spec.action {
            MeshChannelAction::Vertices => {
                decoded.vertices =
                    vertex_storage.with_storage(|| parse_f32_points(expand.ctx(), &bytes))?;
            }
            MeshChannelAction::Normals => match parse_f32_vectors(expand.ctx(), &bytes) {
                Ok(value) => decoded.normals = Some(value),
                Err(error @ GeometryError::Codec(_)) => return Err(error),
                Err(_) => decoded.warnings.push_admitted(
                    expand.ctx(),
                    format_args!("normals channel contains nonfinite values"),
                )?,
            },
            MeshChannelAction::Raw(kind) => {
                expand
                    .ctx()
                    .reserve_vec(&mut decoded.channels, 1, "Rhino mesh channel entries")?;
                decoded.channels.push(channel(
                    expand.ctx(),
                    kind,
                    spec.item_size,
                    match bytes {
                        Cow::Owned(bytes) => buffer_storage.commit_value(bytes)?,
                        Cow::Borrowed(bytes) => {
                            expand.ctx().copy_retained(bytes, "rhino_mesh_buffer")?
                        }
                    },
                )?);
            }
        }
    }
    if decoded.vertices.len() != vertices {
        return Err(error(reader.position(), "mesh vertex channel is required"));
    }
    Ok(())
}

fn read_counted_raw<'a>(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'a>,
    vertices: usize,
    item_size: usize,
    name: &str,
    warnings: &mut Diagnostics,
) -> Result<Option<&'a [u8]>, GeometryError> {
    let count = reader.i32()?;
    if count < 0 {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::RedundantFieldRepaired,
            format_args!("redundant mesh {name} channel has a negative count; channel dropped"),
        )?;
        return Ok(None);
    }
    if count == 0 {
        return Ok(None);
    }
    let bytes = (usize::try_from(count)
        .map_err(|_| GeometryError::unpositioned("mesh count or index exceeds address space"))?)
    .checked_mul(item_size)
    .ok_or_else(|| error(reader.position(), "mesh channel byte count overflow"))?;
    let data = reader.take(bytes)?;
    if usize::try_from(count)
        .map_err(|_| GeometryError::unpositioned("mesh count or index exceeds address space"))?
        != vertices
    {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::RedundantFieldRepaired,
            format_args!("redundant mesh {name} channel count mismatch; channel dropped"),
        )?;
        return Ok(None);
    }
    Ok(Some(data))
}

#[derive(Clone, Copy)]
struct MeshBufferSpec<'a> {
    expected: usize,
    name: &'a str,
}

fn read_buffer<'a>(
    expand: MeshExpand<'a>,
    reader: &mut BoundedReader<'_>,
    spec: MeshBufferSpec<'_>,
    warnings: &mut Diagnostics,
    document_budget: &mut MeshBudget,
    archive: ArchiveVersion,
    storage: Option<&mut cadmpeg_core::decode::ScopedReservation<'_>>,
) -> Result<Option<Cow<'a, [u8]>>, GeometryError> {
    let MeshBufferSpec { expected, name } = spec;
    let declared = usize::try_from(reader.u32()?)
        .map_err(|_| GeometryError::unpositioned("mesh count or index exceeds address space"))?;
    if declared == 0 {
        return Ok(None);
    }
    let buffer_limit = buffer_output_limit(expand);
    if declared > buffer_limit {
        return Err(expand
            .ctx()
            .refuse_codec_limit(
                "Rhino mesh buffer output bytes",
                u64_from_index(buffer_limit),
                u64_from_index(declared),
            )
            .into());
    }
    let admitted_document_bytes = document_budget.admit(expand.ctx(), declared)?;
    let crc = reader.u32()?;
    let method = reader.u8()?;
    let (bytes, expanded, consumed): (&[u8], Option<View<'a>>, usize) = match method {
        0 => {
            let mut input = reader.unread()?;
            let bytes = input.take(declared)?;
            document_budget.used = admitted_document_bytes;
            (bytes, None, declared)
        }
        1 => {
            let chunk = chunk_at(
                reader.backing_bytes(),
                reader.position(),
                reader.end(),
                archive,
                false,
            )?;
            if chunk.typecode != 0x4000_8000 || chunk.short() {
                return Err(error(
                    reader.position(),
                    "compressed buffer is not anonymous",
                ));
            }
            let source = expand
                .root
                .child(chunk.body().start, chunk.body().end)
                .ok_or_else(|| {
                    error(
                        chunk.body().start,
                        "compressed buffer body escapes the root view",
                    )
                })?;
            let body = reader
                .backing_bytes()
                .get(chunk.body().start..chunk.body().end)
                .ok_or_else(|| {
                    error(
                        chunk.body().start,
                        "compressed buffer body escapes the archive bytes",
                    )
                })?;
            if !std::ptr::eq(source.window(), body)
                && !expand.ctx().equal_bytes(
                    source.window(),
                    body,
                    "Rhino compressed mesh source equality",
                )?
            {
                return Err(error(
                    chunk.body().start,
                    format!(
                        "expansion source window of {} bytes does not alias \
                         the {} byte compressed chunk body",
                        source.window().len(),
                        body.len()
                    ),
                ));
            }
            let (view, compressed) = inflate(expand, source, declared)?;
            document_budget.used = admitted_document_bytes;
            if compressed != chunk.body().len() {
                return Err(error(
                    chunk.body().start + compressed,
                    "zlib chunk has trailing bytes",
                ));
            }
            if matches!(
                verify_checksum(expand.ctx(), reader.backing_bytes(), &chunk)?,
                ChecksumStatus::Mismatch { .. }
            ) {
                warnings.push_coded_admitted(
                    expand.ctx(),
                    crate::loss::RhinoLossCode::IntegrityFailure,
                    format_args!("{name} compressed chunk CRC mismatch"),
                )?;
            }
            (
                view.window(),
                Some(view),
                chunk.next_offset() - reader.position(),
            )
        }
        _ => {
            return Err(error(
                reader.position() - 1,
                "unknown compressed-buffer method",
            ))
        }
    };
    reader.skip(consumed)?;
    if bytes.len() != expected {
        warnings.push_coded_admitted(
            expand.ctx(),
            crate::loss::RhinoLossCode::RedundantFieldRepaired,
            format_args!("redundant mesh {name} compressed-buffer size mismatch; channel dropped"),
        )?;
        return Ok(None);
    }
    expand.ctx().charge_work(
        u64_from_index(bytes.len()),
        "Rhino mesh buffer checksum bytes",
    )?;
    if crc32fast::hash(bytes) != crc {
        warnings.push_coded_admitted(
            expand.ctx(),
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!("{name} compressed-buffer CRC mismatch"),
        )?;
        return Ok(None);
    }
    let output = match expanded {
        Some(view) => Cow::Borrowed(view.window()),
        None => Cow::Owned(match storage {
            Some(storage) => {
                storage.with_storage(|| expand.ctx().copy_retained(bytes, "rhino_mesh_buffer"))?
            }
            None => expand.ctx().copy_retained(bytes, "rhino_mesh_buffer")?,
        }),
    };
    Ok(Some(output))
}

pub(crate) fn fuzz_buffer(data: &[u8]) {
    let Some(expected) = View::u16_le_at(data, 0).map(usize::from) else {
        return;
    };
    let Ok(mut reader) = BoundedReader::new(data, 2, data.len()) else {
        return;
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &policy) else {
        return;
    };
    let expand = MeshExpand::new(&ctx, root);
    let mut warnings = Diagnostics::new();
    let mut document_budget = MeshBudget::new();
    let _probe = read_buffer(
        expand,
        &mut reader,
        MeshBufferSpec {
            expected,
            name: "fuzz",
        },
        &mut warnings,
        &mut document_budget,
        ArchiveVersion::V8,
        None,
    );
}

/// Inflates one anonymous zlib mesh buffer to exactly `expected` bytes.
fn inflate<'a>(
    expand: MeshExpand<'a>,
    source: View<'_>,
    expected: usize,
) -> Result<(View<'a>, usize), GeometryError> {
    let base = source.start();
    cadmpeg_container::compression::inflate_zlib_member(
        expand.ctx(),
        source,
        ExpandSpec::Exact(cadmpeg_core::decode::u64_from_index(expected)),
    )
    .or_else(|refusal| Err(expansion_refused(expand.ctx(), base, refusal)?))
}

fn read_ngons(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    vertices: usize,
    faces: usize,
    warnings: &mut Diagnostics,
) -> Result<usize, GeometryError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    crate::chunks::warn_checksum(ctx, reader.backing_bytes(), &chunk, "mesh ngon", warnings)?;
    let mut child =
        BoundedReader::new(reader.backing_bytes(), chunk.body().start, chunk.body().end)?;
    let major = child.i32()?;
    let minor = child.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::unsupported(
            child.position() - 8,
            "unsupported ngon version",
        ));
    }
    let count = checked_u32(&mut child, 1 << 20)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino current mesh ngon records")?;
        let boundary = checked_u32(&mut child, vertices)?;
        if boundary == 0 {
            continue;
        }
        let face_count = checked_u32(&mut child, faces)?;
        for _ in 0..boundary {
            ctx.charge_work(1, "Rhino current mesh ngon indices")?;
            checked_u32(&mut child, vertices)?;
        }
        for _ in 0..face_count {
            ctx.charge_work(1, "Rhino current mesh ngon indices")?;
            checked_u32(&mut child, faces)?;
        }
    }
    child.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(count)
}

fn read_mapping_tag(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<(), GeometryError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    crate::chunks::warn_checksum(
        ctx,
        reader.backing_bytes(),
        &chunk,
        "mesh mapping tag",
        warnings,
    )?;
    let mut child =
        BoundedReader::new(reader.backing_bytes(), chunk.body().start, chunk.body().end)?;
    let major = child.i32()?;
    let minor = child.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::unsupported(
            child.position() - 8,
            "unsupported mapping-tag version",
        ));
    }
    uuid(&mut child)?;
    child.i32()?;
    for _ in 0..16 {
        let value = child.f64()?;
        if !value.is_finite() {
            return Err(error(
                child.position() - 8,
                "mapping transform is not finite",
            ));
        }
    }
    if minor >= 1 {
        child.u32()?;
    }
    child.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(())
}

/// A decoded double-vertex chunk: the declared vertex count, and the buffer
/// bytes when it survived its size and CRC checks (an arena view for a
/// compressed buffer, owned for a stored one). The count is returned even when
/// the bytes are absent so the caller can distinguish a count mismatch from a
/// dropped buffer.
type DoubleVertexChunk<'a> = (usize, Option<Cow<'a, [u8]>>);

fn read_double_chunk<'a>(
    expand: MeshExpand<'a>,
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
    vertex_count: usize,
    document_budget: &mut MeshBudget,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<DoubleVertexChunk<'a>, GeometryError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    let mut child =
        BoundedReader::new(reader.backing_bytes(), chunk.body().start, chunk.body().end)?;
    let major = child.i32()?;
    let minor = child.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::unsupported(
            child.position() - 8,
            "unsupported double-vertex version",
        ));
    }
    let count = checked_u32(&mut child, MAX_MESH_VERTICES)?;
    let expected = count
        .checked_mul(24)
        .ok_or_else(|| error(child.position(), "double-vertex size overflow"))?;
    let buffer_start = child.position();
    let nested_buffer = (reader.backing_bytes().get(buffer_start + 8).copied() == Some(1))
        .then(|| {
            chunk_at(
                reader.backing_bytes(),
                buffer_start + 9,
                chunk.body().end,
                archive,
                false,
            )
            .map(|child| child.range())
        })
        .transpose()?;
    let bytes = read_buffer(
        expand,
        &mut child,
        MeshBufferSpec {
            expected,
            name: "double vertices",
        },
        warnings,
        document_budget,
        archive,
        Some(storage),
    )?;
    child.skip_remaining()?;
    let mut range_storage = expand
        .ctx()
        .reserve_scoped(0, "Rhino mesh double checksum ranges")?;
    let direct = range_storage.with_storage(|| {
        crate::chunks::direct_checksum_ranges(expand.ctx(), &chunk.body(), nested_buffer.as_slice())
    })?;
    if matches!(
        crate::chunks::verify_checksum_ranges(
            expand.ctx(),
            reader.backing_bytes(),
            &chunk,
            &direct
        )?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push_coded_admitted(
            expand.ctx(),
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!(
                "mesh double vertices CRC mismatch at offset {}",
                chunk.header_start
            ),
        )?;
    }
    reader.skip(chunk.next_offset() - reader.position())?;
    if count != vertex_count {
        return Ok((count, None));
    }
    Ok((count, bytes))
}

/// Reads the obsolete V5 class-userdata double-precision vertex array.
///
/// openNURBS reads the array count from the serialized array itself. The two
/// counts and CRCs are producer-side validity fields; `DeleteAfterRead()` only
/// adopts the array when its actual count matches the owner mesh and its f64
/// values cast exactly to the owner's f32 vertices.
fn read_v5_double_vertices(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    extra: &ClassUserdata,
    archive: ArchiveVersion,
    float_vertices: &[[FiniteBinary32; 3]],
) -> Result<Option<Vec<FinitePoint3>>, GeometryError> {
    let chunk = chunk_at(
        data,
        extra.payload_range.start,
        extra.payload_range.end,
        archive,
        false,
    )?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(error(
            chunk.header_start,
            "V5 mesh double-precision userdata is not anonymous",
        ));
    }
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let major = reader.i32()?;
    let _minor = reader.i32()?;
    if major != 1 {
        return Err(error(
            reader.position() - 8,
            "unsupported V5 mesh double-precision userdata version",
        ));
    }
    let _float_count = reader.i32()?;
    let _double_count = reader.i32()?;
    let _float_crc = reader.u32()?;
    let _double_crc = reader.u32()?;
    let array_count = checked_u32(&mut reader, MAX_MESH_VERTICES)?;
    let coordinate_bytes = array_count
        .checked_mul(24)
        .ok_or_else(|| error(reader.position(), "V5 double-vertex byte count overflow"))?;
    if coordinate_bytes > reader.remaining() {
        return Err(FramingError::Truncated {
            offset: reader.position(),
            needed: coordinate_bytes,
        }
        .into());
    }
    if array_count != float_vertices.len() {
        return Ok(None);
    }
    let mut finite = ctx.collection_vec(array_count, "Rhino V5 mesh admitted double vertices")?;
    let mut floats = float_vertices.iter();
    while let Some(float) =
        ctx.next_charged(&mut floats, "Rhino mesh read_v5_double_vertices records")?
    {
        let point = [reader.f64()?, reader.f64()?, reader.f64()?];
        let Some(admitted) = FinitePoint3::new(Point3::new(point[0], point[1], point[2])) else {
            return Ok(None);
        };
        if !point.iter().zip(float).all(|(double, float)| {
            cadmpeg_core::convert::f32_from_f64(*double) == Some(float.get())
        }) {
            return Ok(None);
        }
        finite.push(admitted);
    }
    reader.skip_remaining()?;
    Ok(Some(finite))
}

/// Reads the V4/V5 legacy mesh n-gon userdata list.
///
/// `ON_V4V5_MeshNgonUserData::Read` stores each positive-`N` record as two
/// signed index arrays. `ON_ValidateMeshNgonUserData` admits nonzero matching
/// mesh counts without rechecking those arrays; the older zero-count form is
/// checked for in-range vertices and face indices with a `-1` suffix.
fn read_v4v5_ngon_userdata(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    extra: &ClassUserdata,
    archive: ArchiveVersion,
    vertex_count: usize,
    face_count: usize,
) -> Result<Option<usize>, GeometryError> {
    let chunk = chunk_at(
        data,
        extra.payload_range.start,
        extra.payload_range.end,
        archive,
        false,
    )?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(error(
            chunk.header_start,
            "V4/V5 mesh n-gon userdata is not anonymous",
        ));
    }
    if matches!(
        verify_checksum(ctx, data, &chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        return Ok(None);
    }
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::unsupported(
            reader.position() - 8,
            "unsupported V4/V5 mesh n-gon userdata version",
        ));
    }
    let raw_count = reader.i32()?;
    if raw_count <= 0 {
        reader.skip_remaining()?;
        return Ok(Some(0));
    }
    let count = usize::try_from(raw_count)
        .ok()
        .filter(|count| *count <= MAX_MESH_NGONS)
        .ok_or_else(|| error(reader.position() - 4, "mesh n-gon count exceeds cap"))?;
    let mesh_face_count = i32::try_from(face_count)
        .map_err(|_| error(reader.position(), "mesh face count exceeds i32"))?;
    let mesh_vertex_count = i32::try_from(vertex_count)
        .map_err(|_| error(reader.position(), "mesh vertex count exceeds i32"))?;
    let mut record_count = 0;
    let mut valid_indices = true;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino V4V5 mesh ngon records")?;
        let raw_corner_count = reader.i32()?;
        if raw_corner_count <= 0 {
            continue;
        }
        if raw_corner_count < 3 {
            return Ok(None);
        }
        let Some(corner_count) = usize::try_from(raw_corner_count)
            .ok()
            .filter(|count| *count <= MAX_MESH_NGON_CORNERS)
        else {
            return Ok(None);
        };
        for _ in 0..corner_count {
            ctx.charge_work(1, "Rhino V4V5 mesh ngon indices")?;
            let vertex = reader.i32()?;
            valid_indices &= vertex >= 0 && vertex < mesh_vertex_count;
        }
        let mut unused_faces = false;
        for _ in 0..corner_count {
            ctx.charge_work(1, "Rhino V4V5 mesh ngon indices")?;
            let face = reader.i32()?;
            if face == -1 {
                unused_faces = true;
            } else if unused_faces || face < 0 || face >= mesh_face_count {
                valid_indices = false;
            }
        }
        record_count += 1;
    }
    let stored_face_count = if minor >= 1 { reader.i32()? } else { 0 };
    let stored_vertex_count = if minor >= 1 { reader.i32()? } else { 0 };
    reader.skip_remaining()?;

    let valid = if stored_face_count == 0 && stored_vertex_count == 0 {
        valid_indices
    } else {
        stored_face_count == mesh_face_count && stored_vertex_count == mesh_vertex_count
    };
    if !valid {
        return Ok(None);
    }
    Ok(Some(record_count))
}

fn consume_optional_chunk(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(), GeometryError> {
    let bytes = reader.backing_bytes();
    let chunk = chunk_at(bytes, reader.position(), reader.end(), archive, false)?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(())
}

fn parse_f32_points(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<[FiniteBinary32; 3]>, GeometryError> {
    parse_f32_records(ctx, bytes, "Rhino mesh f32 points", |point| point)
}

fn parse_f32_vectors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<FiniteVector3>, GeometryError> {
    let mut storage = ctx.reserve_scoped(0, "Rhino mesh normal scratch")?;
    let values = storage.with_storage(|| {
        parse_f32_records(ctx, bytes, "Rhino mesh f32 normals", |point| {
            FiniteVector3::from_components(point[0].into(), point[1].into(), point[2].into())
        })
    })?;
    storage.commit_value(values).map_err(GeometryError::from)
}

fn parse_f32_records<T>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
    project: impl Fn([FiniteBinary32; 3]) -> T,
) -> Result<Vec<T>, GeometryError> {
    if !bytes.len().is_multiple_of(12) {
        return Err(GeometryError::unpositioned(
            "invalid f32 point channel length",
        ));
    }
    let mut view = View::over_retained(bytes);
    let mut values = ctx.collection_vec(bytes.len() / 12, operation)?;
    for _ in 0..bytes.len() / 12 {
        ctx.charge_work(1, "Rhino mesh f32 records")?;
        let [Some(x), Some(y), Some(z)] = [view.f32_le(), view.f32_le(), view.f32_le()] else {
            return Err(GeometryError::unpositioned(
                "invalid f32 point channel length",
            ));
        };
        let [Some(x), Some(y), Some(z)] = [x, y, z].map(FiniteBinary32::new) else {
            return Err(GeometryError::unpositioned(
                "f32 point channel contains nonfinite values",
            ));
        };
        values.push(project([x, y, z]));
    }
    Ok(values)
}

fn parse_f64_points(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<[f64; 3]>, GeometryError> {
    if !bytes.len().is_multiple_of(24) {
        return Err(GeometryError::unpositioned(
            "invalid f64 point channel length",
        ));
    }
    let mut view = View::over_retained(bytes);
    let count = bytes.len() / 24;
    let mut points = ctx
        .collection_vec(count, "Rhino mesh f64 points")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino mesh parse_f64_points records")?;
        let point = [view.f64_le(), view.f64_le(), view.f64_le()];
        let [Some(x), Some(y), Some(z)] = point else {
            return Err(GeometryError::unpositioned(
                "invalid f64 point channel length",
            ));
        };
        points.push([x, y, z]);
    }
    Ok(points)
}

fn synchronization_ok(
    ctx: &DecodeContext<'_>,
    double: &[[f64; 3]],
    float: &[[FiniteBinary32; 3]],
) -> Result<bool, CodecError> {
    ctx.all_by(
        double.iter().zip(float),
        |(a, b)| {
            let scale = f64::from(
                b.iter()
                    .map(|value| value.get().abs())
                    .fold(0.0_f32, f32::max),
            );
            Ok(a.iter().zip(b).all(|(left, right)| {
                (*left - f64::from(right.get())).abs() <= scale * EPS_MESH_SYNCHRONIZATION_OK_E6
            }))
        },
        "Rhino mesh synchronized double vertices",
    )
}

fn channel(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: u32,
    item_size: u32,
    data: Vec<u8>,
) -> Result<TessellationChannel, GeometryError> {
    TessellationChannel::new(
        cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
        item_size,
        kind,
        0,
        data,
    )
    .or_else(|error| {
        Err(GeometryError::unpositioned(ctx.format_retained(
            format_args!("invalid mesh channel: {error}"),
            "Rhino channel text",
        )?))
    })
}

fn interval(reader: &mut BoundedReader<'_>) -> Result<(), FramingError> {
    let lo = reader.f64()?;
    let hi = reader.f64()?;
    if !lo.is_finite() || !hi.is_finite() || lo > hi {
        return Err(FramingError::Structural {
            offset: reader.position() - 16,
            message: "invalid mesh interval".to_string(),
        });
    }
    Ok(())
}

/// Reads a mesh element count bounded by the codec-local `cap`.
///
/// Omits the `checked_count_bytes` remaining-bytes floor: `vertex_count` may
/// address zlib-compressed data downstream, and `face_count` is floored at
/// consumption by `reader.take(bytes)` in `read_faces`.
fn count(reader: &mut BoundedReader<'_>, cap: usize) -> Result<usize, GeometryError> {
    let value = reader.i32()?;
    if value < 0
        || usize::try_from(value)
            .map_err(|_| GeometryError::unpositioned("mesh count or index exceeds address space"))?
            > cap
    {
        return Err(error(reader.position() - 4, "mesh count exceeds cap"));
    }
    usize::try_from(value)
        .map_err(|_| GeometryError::unpositioned("mesh count or index exceeds address space"))
}

/// Reads an unsigned mesh count bounded by `cap`, without a remaining-bytes
/// floor (see [`count`]). Callers must admit counted traversal and backing
/// allocation before consuming the count.
fn checked_u32(reader: &mut BoundedReader<'_>, cap: usize) -> Result<usize, GeometryError> {
    let value = usize::try_from(reader.u32()?)
        .map_err(|_| GeometryError::unpositioned("mesh count or index exceeds address space"))?;
    if value > cap {
        return Err(error(reader.position() - 4, "mesh count exceeds cap"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests;
