// SPDX-License-Identifier: Apache-2.0
//! `DisplayLists` descriptor tables.

use cadmpeg_ir::math::planar::{point_segment_distance, segments_intersect};
use cadmpeg_ir::units::FinitePoint2;

use crate::brep::feature_source::FeatureSourceId;
use crate::brep::PersistentFaceIdentity;
use crate::container::{ContainerScan, Section};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::tessellation::{TessellationChannel, TessellationMesh};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::units::OrthonormalFrame3;
use std::collections::HashMap;

use crate::layout::display_lists_compact_face_header as compact_face;
use crate::layout::display_lists_extended_face_header as extended_face;
use crate::layout::display_lists_scene_source_binding as scene_src;

const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];
const SCENE_SOURCE_MARKER: &[u8] = &scene_src::MARKER_VALUE;
const EPS_DISPLAY_QUANTIZATION: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_CYLINDER_ANGLE: f64 = 1.0e-12;
const DISPLAY_QUANTIZATION_ULPS: f64 = 8.0;
const MAX_PLANAR_TRIM_ARC_SEGMENTS: usize = 4096;
const MIN_TESSELLATION_NORMAL_ALIGNMENT: f64 = 1.0 - 1.0e-4;

#[cfg(test)]
thread_local! {
    static DISPLAY_PARSE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_display_parse_calls() {
    DISPLAY_PARSE_CALLS.with(|calls| calls.set(0));
}

#[cfg(test)]
pub(crate) fn display_parse_calls() -> usize {
    DISPLAY_PARSE_CALLS.with(std::cell::Cell::get)
}

/// The evaluation tolerance of one face in the display lane.
///
/// The display lane quantizes coordinates to `f32`, so a tolerance below
/// [`EPS_DISPLAY_QUANTIZATION`] states a resolution the lane does not carry.
/// [`FaceEvaluationTolerance::of`] is the only constructor: a stated tolerance
/// under that bound is refused, so no stated value is floored, and a face that
/// states no tolerance evaluates at the bound.
#[derive(Debug, Clone, Copy)]
struct FaceEvaluationTolerance(f64);

impl FaceEvaluationTolerance {
    /// The evaluation tolerance of `face`, or `None` when its stated
    /// tolerance is finer than the display lane resolves. Callers validate
    /// every face before evaluating any trim, so this `None` cannot silently
    /// discard a source face during assignment.
    fn of(face: &cadmpeg_ir::topology::Face) -> Option<Self> {
        let Some(stated) = face.tolerance else {
            return Some(Self(EPS_DISPLAY_QUANTIZATION));
        };
        (stated.get() >= EPS_DISPLAY_QUANTIZATION).then_some(Self(stated.get()))
    }

    /// The tolerance value.
    fn get(self) -> f64 {
        self.0
    }
}

const FACE_TESSELLATION_CLASS: &[u8] = b"uoTempFaceTessData_c";

macro_rules! require_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Summary {
    pub(crate) vertices: usize,
    pub(crate) triangles: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Mesh {
    mesh: TessellationMesh,
    channels: Vec<TessellationChannel>,
    /// Vertices the strips span, counted when the lanes were paired.
    vertices: usize,
    /// Strips in the mesh, counted when the lanes were paired.
    strips: usize,
}

impl Default for Mesh {
    fn default() -> Self {
        Self {
            mesh: TessellationMesh::List {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            channels: Vec::new(),
            vertices: 0,
            strips: 0,
        }
    }
}

impl Mesh {
    /// Number of vertices the strips span.
    fn vertex_count(&self) -> usize {
        self.vertices
    }

    /// Number of triangles the strips expand to: a strip of `n` vertices
    /// states `n - 2` triangles, and every strip spans at least three.
    fn triangle_count(&self) -> usize {
        self.vertices - 2 * self.strips
    }

    /// Number of strips in the mesh.
    fn strip_count(&self) -> usize {
        self.strips
    }

    pub(crate) fn into_tessellation(
        self,
        id: cadmpeg_ir::tessellation::TessellationId,
    ) -> Result<cadmpeg_ir::tessellation::Tessellation, cadmpeg_ir::tessellation::TessellationError>
    {
        cadmpeg_ir::tessellation::Tessellation::new(id, self.mesh, self.channels)
    }
}

/// A nonempty-or-empty byte interval whose end never precedes its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ByteRange {
    start: usize,
    end: usize,
}

impl ByteRange {
    /// The interval `start..end`, when it is ordered.
    fn new(start: usize, end: usize) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    /// First byte offset of the interval.
    pub(crate) fn start(self) -> usize {
        self.start
    }

    /// One past the last byte offset of the interval.
    pub(crate) fn end(self) -> usize {
        self.end
    }

    /// This interval truncated at `end`, empty when `end` precedes its start.
    fn truncated(self, end: usize) -> Self {
        Self {
            start: self.start,
            end: end.min(self.end).max(self.start),
        }
    }
}

/// One decoded `uoTempFaceTessData_c` descriptor table.
#[derive(Debug, Clone)]
pub(crate) struct DisplayFace {
    pub(crate) mesh: Mesh,
    pub(crate) table: ByteRange,
    pub(crate) metadata: ByteRange,
    pub(crate) surface_references: Vec<PersistentSurfaceReference>,
}

/// One framed persistent-surface reference in a display-face metadata slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PersistentSurfaceReference {
    /// A complete identity whose optional tail is entirely numeric.
    Complete(PersistentFaceIdentity),
    /// A source-level reference whose trailing fields are opaque.
    SourceOnly {
        feature_source_id: FeatureSourceId,
        local_surface_id: u32,
    },
}

impl PersistentSurfaceReference {
    pub(crate) fn feature_source_id(&self) -> FeatureSourceId {
        match self {
            Self::Complete(identity) => identity.feature_source_id,
            Self::SourceOnly {
                feature_source_id, ..
            } => *feature_source_id,
        }
    }

    fn complete_identity(&self) -> Option<&PersistentFaceIdentity> {
        match self {
            Self::Complete(identity) => Some(identity),
            Self::SourceOnly { .. } => None,
        }
    }
}

/// One `DisplayLists` table whose persistent surface identity can bind a B-rep face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistentFaceBinding {
    pub(crate) tessellation: String,
    pub(crate) identity: PersistentFaceIdentity,
}

impl DisplayFace {
    /// Return the source ID only when all duplicated references agree.
    pub(crate) fn feature_source_id(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<FeatureSourceId>, cadmpeg_core::CodecError> {
        let Some((first, rest)) = self.surface_references.split_first() else {
            return Ok(None);
        };
        let source = first.feature_source_id();
        Ok(ctx
            .all_by(
                rest,
                |candidate| Ok(candidate.feature_source_id() == source),
                "scan SLDPRT display feature source references",
            )?
            .then_some(source))
    }

    /// Return the complete identity only when every duplicate reference agrees.
    pub(crate) fn persistent_surface_identity(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<&PersistentFaceIdentity>, cadmpeg_core::CodecError> {
        const OPERATION: &str = "scan SLDPRT display persistent surface references";
        let Some((first, rest)) = self.surface_references.split_first() else {
            return Ok(None);
        };
        let Some(first) = first.complete_identity() else {
            return Ok(None);
        };
        Ok(ctx
            .all_by(
                rest,
                |reference| match reference.complete_identity() {
                    Some(identity) => ctx.equal(identity, first, OPERATION),
                    None => Ok(false),
                },
                OPERATION,
            )?
            .then_some(first))
    }
}

/// One class declaration in a display payload and the bytes it governs, up
/// to the next declaration.
#[derive(Debug, Clone)]
pub(crate) struct ClassInterval<'a> {
    pub(crate) name: &'a str,
    pub(crate) class_offset: usize,
    pub(crate) content: ByteRange,
}

/// Every class declaration in `payload`, in payload order.
pub(crate) fn class_intervals<'a>(
    ctx: &DecodeContext<'_>,
    payload: &'a [u8],
) -> Result<Vec<ClassInterval<'a>>, cadmpeg_core::CodecError> {
    const NAME: usize = 6;
    let mut intervals: Vec<ClassInterval<'a>> = Vec::new();
    for offset in ctx.find_bytes_iter(
        payload,
        CLASS_MARKER,
        "scan SLDPRT display class declarations",
    )? {
        let Some(length) = View::u16_le_at(payload, offset + 4).map(usize::from) else {
            continue;
        };
        if !(1..=128).contains(&length) {
            continue;
        }
        // At most 128 name bytes, so the name test is fixed work.
        let Some(name) = payload
            .get(offset + NAME..offset + NAME + length)
            .filter(|bytes| {
                bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            })
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        else {
            continue;
        };
        if let Some(previous) = intervals.last_mut() {
            previous.content = previous.content.truncated(offset);
        }
        let Some(content) = ByteRange::new(offset + NAME + length, payload.len()) else {
            continue;
        };
        ctx.push_vec(
            &mut intervals,
            ClassInterval {
                name,
                class_offset: offset,
                content,
            },
            "collect SLDPRT display class intervals",
        )?;
    }
    Ok(intervals)
}

/// Whether a native class names a scene light.
fn is_scene_light(name: &str) -> bool {
    matches!(
        crate::classification::native_object_class(name).tree_node(),
        Some(
            cadmpeg_ir::features::FeatureTreeNodeRole::AmbientLight
                | cadmpeg_ir::features::FeatureTreeNodeRole::DirectionalLight
                | cadmpeg_ir::features::FeatureTreeNodeRole::PointLight
                | cadmpeg_ir::features::FeatureTreeNodeRole::SpotLight
        )
    )
}

/// The light class each scene source names, when every declaration that
/// names the source agrees on its class.
pub(crate) fn scene_feature_classes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<HashMap<u32, String>, cadmpeg_core::CodecError> {
    const INDEX: &str = "index SLDPRT scene class sources";
    let mut storage = ctx.reserve_scoped(0, INDEX)?;
    let mut candidates = std::collections::BTreeMap::<u32, Option<&str>>::new();
    for section in scan.sections(ctx)? {
        let payload = section.payload();
        for class in class_intervals(ctx, payload)? {
            if !is_scene_light(class.name) {
                continue;
            }
            let Some(records) = payload.get(class.content.start()..class.content.end()) else {
                continue;
            };
            for window in ctx
                .admit_iter(records, "scan SLDPRT display class sources")?
                .windows(const { crate::nonzero(scene_src::LEN) })
            {
                let Some(source) = window
                    .starts_with(SCENE_SOURCE_MARKER)
                    .then(|| View::u32_le_at(window, scene_src::SOURCE_ID))
                    .flatten()
                    .filter(|source| *source != 0)
                else {
                    continue;
                };
                // Class names are at most 128 bytes, so the comparison is fixed work.
                match ctx.get_mut_btree_map(&mut candidates, &source, INDEX)? {
                    Some(existing) => {
                        if *existing != Some(class.name) {
                            *existing = None;
                        }
                    }
                    None => {
                        storage.with_storage(|| {
                            ctx.insert_btree_map(&mut candidates, source, Some(class.name), INDEX)
                        })?;
                    }
                }
            }
        }
    }
    let mut resolved = HashMap::new();
    for (source, class) in ctx.admit_iter(candidates, "collect SLDPRT scene feature classes")? {
        if let Some(class) = class {
            let class = ctx.copy_retained_text(class, "retain SLDPRT scene class name")?;
            ctx.insert_hash_map(
                &mut resolved,
                source,
                class,
                "collect SLDPRT scene feature classes",
            )?;
        }
    }
    Ok(resolved)
}

/// One display-list channel descriptor and the data bytes it states.
#[derive(Debug, Clone, Copy)]
struct Descriptor<'a> {
    item_size: u32,
    kind: u32,
    flags: u32,
    count: u32,
    data: &'a [u8],
}

impl<'a> Descriptor<'a> {
    /// The descriptor at `at` and the offset after its data, when both lie
    /// inside `bytes`.
    fn read(bytes: &'a [u8], at: usize) -> Option<(Self, usize)> {
        let item_size = View::u32_le_at(bytes, at)?;
        let kind = View::u32_le_at(bytes, at.checked_add(4)?)?;
        let flags = View::u32_le_at(bytes, at.checked_add(8)?)?;
        let count = View::u32_le_at(bytes, at.checked_add(12)?)?;
        let start = at.checked_add(16)?;
        let end = cadmpeg_core::decode::index_from_u32(item_size)
            .checked_mul(cadmpeg_core::decode::index_from_u32(count))
            .and_then(|size| start.checked_add(size))?;
        let data = bytes.get(start..end)?;
        Some((
            Self {
                item_size,
                kind,
                flags,
                count,
                data,
            },
            end,
        ))
    }

    fn shape(self) -> (u32, u32, u32) {
        (self.item_size, self.kind, self.flags)
    }

    fn count(self) -> usize {
        cadmpeg_core::decode::index_from_u32(self.count)
    }
}

/// Whether a mesh's auxiliary channels agree with its strip lengths, as
/// [`auxiliary_lanes_agree`] tests them; any number of channels but three
/// disagrees.
pub(crate) fn auxiliary_channels_are_consistent(
    ctx: &DecodeContext<'_>,
    strips: &[usize],
    channels: &[TessellationChannel],
) -> Result<bool, cadmpeg_core::CodecError> {
    let [b, c, d] = channels else {
        return Ok(false);
    };
    fn lane(channel: &TessellationChannel) -> Descriptor<'_> {
        Descriptor {
            item_size: channel.item_size(),
            kind: channel.kind(),
            flags: channel.flags(),
            count: channel.count(),
            data: channel.data(),
        }
    }
    auxiliary_lanes_agree(ctx, strips, |length| length, [lane(b), lane(c), lane(d)])
}

/// Whether the three auxiliary channels agree with the strip spans: two
/// per-endpoint lanes that are empty or cover every strip endpoint, and one
/// per-strip lane that states each strip's endpoint count.
fn auxiliary_lanes_agree<S: Copy>(
    ctx: &DecodeContext<'_>,
    spans: &[S],
    length: impl Fn(S) -> usize,
    [b, c, d]: [Descriptor<'_>; 3],
) -> Result<bool, cadmpeg_core::CodecError> {
    let endpoints = |span: S| length(span).checked_mul(2)?.checked_sub(2);
    if b.shape() != (4, 8, 2)
        || c.shape() != (4, 8, 2)
        || d.shape() != (1, 8, 2)
        || c.count() != spans.len()
    {
        return Ok(false);
    }
    let Some(endpoint_count) = ctx
        .admit_iter(spans, "sum SLDPRT auxiliary channel endpoints")?
        .try_fold(0usize, |total, span| total.checked_add(endpoints(*span)?))
    else {
        return Ok(false);
    };
    let counts = (b.count(), d.count());
    if counts != (0, 0) && counts != (endpoint_count, endpoint_count) {
        return Ok(false);
    }
    Ok(ctx.all_by(
        c.data.chunks_exact(4).zip(spans),
        |(bytes, span)| {
            Ok(endpoints(*span)
                == View::u32_le_at(bytes, 0).map(cadmpeg_core::decode::index_from_u32))
        },
        "scan SLDPRT auxiliary endpoint bytes",
    )? && ctx.all_by(
        b.data.chunks_exact(4),
        |bytes| Ok(View::f32_le_at(bytes, 0).is_some_and(f32::is_finite)),
        "scan SLDPRT auxiliary channel scalars",
    )?)
}

/// The finite `f32` triples of a 12-byte lane, each mapped to a row; `None`
/// when a coordinate is not finite.
fn read_triples<T>(
    ctx: &DecodeContext<'_>,
    lane: Descriptor<'_>,
    row: impl Fn(f64, f64, f64) -> T,
    operation: &'static str,
) -> Result<Option<Vec<T>>, cadmpeg_core::CodecError> {
    let mut rows = Vec::new();
    ctx.reserve_capacity(&mut rows, lane.count(), operation)?;
    for triple in ctx
        .admit_iter(lane.data, operation)?
        .chunks(const { crate::nonzero(12) })
    {
        let read = |at| {
            View::f32_le_at(triple, at)
                .map(f64::from)
                .filter(|value| value.is_finite())
        };
        let (Some(x), Some(y), Some(z)) = (read(0), read(4), read(8)) else {
            return Ok(None);
        };
        ctx.push_vec(&mut rows, row(x, y, z), operation)?;
    }
    Ok(Some(rows))
}

/// The lanes of one display-list table. The strip spans are scratch under
/// `scratch`; the vertex and normal lanes are scoped under `lanes` until the
/// mesh that keeps them is accepted.
struct ProbedTable<'ctx> {
    spans: Vec<u32>,
    vertices: Vec<Point3>,
    normals: Vec<Vector3>,
    channels: Vec<TessellationChannel>,
    scratch: ScopedReservation<'ctx>,
    lanes: ScopedReservation<'ctx>,
}

/// Read the six descriptors at `at` as display-list lanes.
///
/// `None` is a probe miss: the descriptors at this byte position do not state
/// the display-list grammar. The descriptor shapes and counts are tested
/// before any lane is read.
fn probe_table<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
) -> Result<Option<(ProbedTable<'ctx>, usize)>, cadmpeg_core::CodecError> {
    let mut end = at;
    let mut next = || {
        let (descriptor, after) = Descriptor::read(bytes, end)?;
        end = after;
        Some(descriptor)
    };
    let (Some(strips), Some(positions), Some(normals), Some(b), Some(c), Some(d)) =
        (next(), next(), next(), next(), next(), next())
    else {
        return Ok(None);
    };
    if strips.shape() != (4, 8, 2)
        || positions.shape() != (12, 100, 2)
        || normals.shape() != (12, 100, 2)
        || strips.count == 0
        || positions.count == 0
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "decode display-list strips")?;
    let spans = scratch.with_storage(|| {
        let mut spans = ctx.collection_vec(strips.count(), "decode display-list strips")?;
        for length in ctx
            .admit_iter(strips.data, "decode display-list strips")?
            .chunks(const { crate::nonzero(4) })
        {
            spans.extend(View::u32_le_at(length, 0));
        }
        Ok::<_, cadmpeg_core::CodecError>(spans)
    })?;
    if !auxiliary_lanes_agree(ctx, &spans, cadmpeg_core::decode::index_from_u32, [b, c, d])? {
        return Ok(None);
    }
    let mut lanes = ctx.reserve_scoped(0, "decode display-list vertices")?;
    let Some(vertices) = lanes.with_storage(|| {
        read_triples(
            ctx,
            positions,
            |x, y, z| Point3::new(x * 1000.0, y * 1000.0, z * 1000.0),
            "decode display-list vertices",
        )
    })?
    else {
        return Ok(None);
    };
    let Some(normal_rows) = lanes
        .with_storage(|| read_triples(ctx, normals, Vector3::new, "decode display-list normals"))?
    else {
        return Ok(None);
    };
    let mut channels = Vec::new();
    ctx.reserve_vec(&mut channels, 6, "decode display-list channels")?;
    for descriptor in [strips, positions, normals, b, c, d] {
        let Ok(channel) = TessellationChannel::new(
            cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
            descriptor.item_size,
            descriptor.kind,
            descriptor.flags,
            ctx.copy_retained(descriptor.data, "copy display-list channel bytes")?,
        ) else {
            return Ok(None);
        };
        channels.push(channel);
    }
    Ok(Some((
        ProbedTable {
            spans,
            vertices,
            normals: normal_rows,
            channels,
            scratch,
            lanes,
        },
        end,
    )))
}

/// Pair one display-list table's lanes into mesh rows.
///
/// `Ok(None)` is a probe miss and nothing else: the descriptors at this byte
/// position do not state the display-list grammar. The descriptor is the record
/// boundary, so once `probe_table` answers, the region is a table and every
/// lane disagreement inside it is a `CodecError` naming the table's byte
/// offset — a normal lane that does not cover the vertices, and strip spans
/// that do not cut the vertex lane, alike. The display-list grammar states a
/// normal descriptor for every table, so the lane is always present and `None`
/// is never the reading of an empty one.
fn parse_table(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
) -> Result<Option<(Mesh, usize)>, cadmpeg_core::CodecError> {
    let Some((
        ProbedTable {
            spans,
            vertices,
            normals,
            channels,
            scratch,
            lanes,
        },
        end,
    )) = probe_table(ctx, bytes, at)?
    else {
        return Ok(None);
    };
    let vertex_count = vertices.len();
    if normals.len() == vertex_count {
        // `from_strip_lanes` pairs every vertex with its normal and then
        // partitions the paired rows into strips.
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(vertex_count),
            "pair display-list shaded vertices",
        )?;
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(vertex_count),
            "partition display-list strip vertices",
        )?;
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(spans.len()),
            "partition display-list strips",
        )?;
    }
    let mesh =
        TessellationMesh::from_strip_lanes(vertices, Some(normals), &spans).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "sldprt display-list table at byte {at}: {error}"
            ))
        })?;
    lanes.commit()?;
    drop(scratch);
    Ok(Some((
        Mesh {
            mesh,
            channels,
            vertices: vertex_count,
            strips: spans.len(),
        },
        end,
    )))
}

pub(crate) fn section_display_faces(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
) -> Result<Vec<DisplayFace>, cadmpeg_core::CodecError> {
    #[cfg(test)]
    DISPLAY_PARSE_CALLS.with(|calls| calls.set(calls.get() + 1));
    let payload = section.payload();
    let mut markers = ctx
        .find_bytes_iter(
            payload,
            FACE_TESSELLATION_CLASS,
            "scan SLDPRT display face declarations",
        )?
        .peekable();
    let mut faces = Vec::new();
    while let Some(marker) = markers.next() {
        let header = marker + FACE_TESSELLATION_CLASS.len();
        let (Some(triangle_count), Some(strip_count)) = (
            View::u32_le_at(payload, header + compact_face::TRIANGLE_COUNT),
            View::u32_le_at(payload, header + compact_face::STRIP_COUNT),
        ) else {
            continue;
        };
        // `uoBodyPropInfo_c` declarations separate body-property groups, but
        // face descriptor tables continue after them without repeating the
        // `uoTempFaceTessData_c` declaration. Only another face declaration
        // starts a new sequence.
        let limit = markers.peek().copied().unwrap_or(payload.len());
        let start = header + descriptor_table_offset(payload, header);
        let Some(tables) = parse_table_sequence(ctx, payload, start, limit)? else {
            continue;
        };
        // The face header states the mesh of its first table when it states a
        // mesh at all. Two zero counts state none: the primary writes that
        // header over a face whose descriptor table still holds a mesh, and
        // the marker then states no display face -- the disposition this
        // decoder has always given it. A stated, positive count that disagrees
        // with the table below it is a different thing: a contradiction inside
        // bytes that are present, which takes the same disposition as a lane
        // disagreement inside the same table.
        let [(table_start, _, first_mesh), ..] = tables.as_slice() else {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "sldprt display-face header at byte {header} states no table"
            )));
        };
        let Some((triangle_count, strip_count)) =
            (triangle_count != 0 || strip_count != 0).then_some((triangle_count, strip_count))
        else {
            // The header states no mesh, so the tables below it are not the
            // ones it describes and this marker states no display face.
            continue;
        };
        let parsed_triangles = first_mesh.triangle_count();
        let parsed_strips = first_mesh.strip_count();
        if usize::try_from(triangle_count).ok() != Some(parsed_triangles)
            || usize::try_from(strip_count).ok() != Some(parsed_strips)
        {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "sldprt display-face table at byte {table_start}: header states \
                 {triangle_count} triangle(s) and {strip_count} strip(s); the parsed mesh has \
                 {parsed_triangles} triangle(s) and {parsed_strips} strip(s)"
            )));
        }
        for (start, end, mesh) in tables {
            let (Some(table), Some(metadata)) =
                (ByteRange::new(start, end), ByteRange::new(end, limit))
            else {
                continue;
            };
            ctx.reserve_vec(&mut faces, 1, "collect display-list faces")?;
            faces.push(DisplayFace {
                mesh,
                table,
                metadata,
                surface_references: Vec::new(),
            });
        }
    }
    // Faces are in table order: a marker's tables ascend and end before the
    // next marker, whose tables follow it.
    for index in ctx.admit_iter(0..faces.len(), "bound SLDPRT display face metadata")? {
        let metadata_end = faces
            .get(index + 1)
            .map_or(faces[index].metadata.end(), |next| next.table.start());
        faces[index].metadata = faces[index].metadata.truncated(metadata_end);
        faces[index].surface_references =
            persistent_surface_references(ctx, payload, faces[index].metadata)?;
    }
    Ok(faces)
}

/// Offset of the first descriptor after a face-tessellation class name.
///
/// Both forms begin with two u32 cells. The extended form then carries a
/// fixed 32-byte extension. A compact table begins with item
/// size 4 at the same position, so it cannot satisfy the extension grammar.
fn descriptor_table_offset(payload: &[u8], at: usize) -> usize {
    let extended = View::u32_le_at(payload, at + extended_face::FORM) == Some(1)
        && View::u32_le_at(payload, at + extended_face::ZERO_AT_12) == Some(0)
        && View::u32_le_at(payload, at + extended_face::ZERO_AT_16) == Some(0)
        && View::u32_le_at(payload, at + extended_face::FORM_TOKEN).is_some_and(|token| token != 0)
        && payload
            .get(at + extended_face::ZERO_TAIL..at + extended_face::LEN)
            .is_some_and(|tail| tail.iter().all(|byte| *byte == 0));
    if extended {
        extended_face::LEN
    } else {
        compact_face::LEN
    }
}

/// One display-list table: the byte range it spans and the mesh it states.
type TableSpan = (usize, usize, Mesh);

fn parse_table_sequence(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    at: usize,
    limit: usize,
) -> Result<Option<Vec<TableSpan>>, cadmpeg_core::CodecError> {
    let first_start = at;
    let Some((mesh, mut at)) = parse_table(ctx, payload, at)? else {
        return Ok(None);
    };
    if at > limit {
        return Ok(None);
    }
    if mesh.vertex_count() == 0 {
        return Ok(None);
    }
    let mut meshes = Vec::new();
    ctx.reserve_vec(&mut meshes, 1, "collect display-list tables")?;
    meshes.push((first_start, at, mesh));
    while at + 16 <= limit {
        let Some(relative) = ctx.position_by(
            payload[at..limit].windows(4),
            |window| Ok(window == [4, 0, 0, 0]),
            "scan SLDPRT subsequent display table",
        )?
        else {
            break;
        };
        at += relative;
        let start = at;
        if let Some((next, end)) = parse_table(ctx, payload, at)? {
            if end <= limit && next.vertex_count() > 0 {
                ctx.reserve_vec(&mut meshes, 1, "collect display-list tables")?;
                meshes.push((start, end, next));
                at = end;
            } else {
                at += 4;
            }
        } else {
            at += 4;
        }
    }
    Ok(Some(meshes))
}

/// The fields of a comma-separated text, as `str::split(',')` yields them,
/// over comma positions found by one admitted search.
struct CommaFields<'t, I> {
    text: &'t str,
    commas: I,
    start: Option<usize>,
}

impl<'t, I: Iterator<Item = usize>> Iterator for CommaFields<'t, I> {
    type Item = &'t str;

    fn next(&mut self) -> Option<&'t str> {
        let start = self.start?;
        match self.commas.next() {
            Some(comma) => {
                self.start = Some(comma + 1);
                Some(&self.text[start..comma])
            }
            None => {
                self.start = None;
                Some(&self.text[start..])
            }
        }
    }
}

fn persistent_surface_references(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    range: ByteRange,
) -> Result<Vec<PersistentSurfaceReference>, cadmpeg_core::CodecError> {
    const MARKER: &[u8] = &[0xff, 0xfe, 0xff];
    const PARSE: &str = "parse display-list reference text";
    let mut references = Vec::new();
    let mut at = range.start();
    while at + 4 <= range.end() && at + 4 <= payload.len() {
        ctx.charge_work(1, "scan display-list reference names")?;
        if payload.get(at..at + MARKER.len()) != Some(MARKER) {
            at += 1;
            continue;
        }
        let count = usize::from(payload[at + 3]);
        let start = at + 4;
        let Some(end) = count
            .checked_mul(2)
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= range.end())
        else {
            at += 1;
            continue;
        };
        let Some(raw) = payload.get(start..end) else {
            at += 1;
            continue;
        };
        at = end;
        let (text, _text_reservation) = match ctx.utf16le_scoped_text(
            raw,
            count,
            false,
            "decode display-list reference text",
        ) {
            Ok(text) => text,
            Err(cadmpeg_core::CodecError::Malformed(_)) => continue,
            Err(error) => return Err(error),
        };
        let text = ctx.trim_text(&text, PARSE)?;
        let mut fields = CommaFields {
            text,
            commas: ctx.find_bytes_iter(text.as_bytes(), b",", PARSE)?,
            start: Some(0),
        }
        .peekable();
        // The class name is compared with literals, so its test is fixed work.
        if !fields
            .next()
            .is_some_and(|name| name.starts_with("mo") && name.ends_with("SurfIdRep_c"))
        {
            continue;
        }
        let Some(feature_source_id) = fields
            .next()
            .map(|field| ctx.parse_text::<u32>(field, PARSE))
            .transpose()?
            .and_then(Result::ok)
            .and_then(|source| FeatureSourceId::try_from(source).ok())
        else {
            continue;
        };
        let Some(local_surface_id) = fields
            .next()
            .map(|field| ctx.parse_text::<u32>(field, PARSE))
            .transpose()?
            .and_then(Result::ok)
        else {
            continue;
        };
        // The numeric tail is kept only when every trailing field is an
        // `i32`; a blank last field is not a field.
        let mut tail = ctx.reserve_scoped(0, "decode display-list reference fields")?;
        let mut trailing_fields = Some(Vec::new());
        while let Some(field) = fields.next() {
            if fields.peek().is_none() && ctx.trim_text(field, PARSE)?.is_empty() {
                break;
            }
            let Ok(value) = ctx.parse_text::<i32>(field, PARSE)? else {
                trailing_fields = None;
                break;
            };
            if let Some(values) = &mut trailing_fields {
                ctx.push_scoped_vec(
                    &mut tail,
                    values,
                    u32::from_ne_bytes(value.to_ne_bytes()),
                    "decode display-list reference fields",
                )?;
            }
        }
        ctx.reserve_vec(&mut references, 1, "collect display-list references")?;
        references.push(match trailing_fields {
            Some(trailing_fields) => {
                tail.commit()?;
                PersistentSurfaceReference::Complete(PersistentFaceIdentity {
                    feature_source_id,
                    local_id: local_surface_id,
                    trailing_fields,
                })
            }
            None => PersistentSurfaceReference::SourceOnly {
                feature_source_id,
                local_surface_id,
            },
        });
    }
    Ok(references)
}

pub(crate) fn summary_for_faces(
    ctx: &DecodeContext<'_>,
    faces: &[DisplayFace],
) -> Result<Summary, cadmpeg_core::CodecError> {
    ctx.admit_iter(faces, "sum SLDPRT display faces")?
        .try_fold(Summary::default(), |total, face| {
            Some(Summary {
                vertices: total.vertices.checked_add(face.mesh.vertex_count())?,
                triangles: total.triangles.checked_add(face.mesh.triangle_count())?,
            })
        })
        .ok_or_else(|| ctx.refuse_codec_limit("sum SLDPRT display faces", u64::MAX - 1, u64::MAX))
}

struct SurfaceCandidate<'a> {
    face: &'a FaceId,
    body: &'a cadmpeg_ir::ids::BodyId,
    surface: &'a SurfaceGeometry,
    tolerance: f64,
    inverse: cadmpeg_ir::transform::Transform,
    trim: Option<AnalyticTrim>,
}

/// An admitted tessellation mesh: finite positions and normals.
type AdmittedMesh = TessellationMesh<FinitePoint3, cadmpeg_ir::features::FiniteVector3>;

/// The strip rows of a strip mesh, borrowed in mesh order.
type StripRows<'m, V> = std::iter::FlatMap<
    std::slice::Iter<'m, cadmpeg_ir::tessellation::Strip<V>>,
    &'m [V],
    fn(&'m cadmpeg_ir::tessellation::Strip<V>) -> &'m [V],
>;

/// The vertex rows of a mesh in mesh order, borrowed: each position and, when
/// the mesh carries per-vertex normals, its normal. Each step is fixed work.
enum MeshRows<'m> {
    Positions(std::slice::Iter<'m, FinitePoint3>),
    Shaded(std::slice::Iter<'m, ShadedRow>),
    PositionStrips(StripRows<'m, FinitePoint3>),
    ShadedStrips(StripRows<'m, ShadedRow>),
}

type ShadedRow =
    cadmpeg_ir::tessellation::ShadedVertex<FinitePoint3, cadmpeg_ir::features::FiniteVector3>;

impl<'m> MeshRows<'m> {
    fn new(mesh: &'m AdmittedMesh) -> Self {
        match mesh {
            TessellationMesh::List { vertices, .. }
            | TessellationMesh::CornerShadedList { vertices, .. } => {
                Self::Positions(vertices.iter())
            }
            TessellationMesh::ShadedList { vertices, .. } => Self::Shaded(vertices.iter()),
            TessellationMesh::Strips { strips } => {
                Self::PositionStrips(strips.as_slice().iter().flat_map(
                    cadmpeg_ir::tessellation::Strip::vertices
                        as fn(
                            &'m cadmpeg_ir::tessellation::Strip<FinitePoint3>,
                        ) -> &'m [FinitePoint3],
                ))
            }
            TessellationMesh::ShadedStrips { strips } => {
                Self::ShadedStrips(strips.as_slice().iter().flat_map(
                    cadmpeg_ir::tessellation::Strip::vertices
                        as fn(&'m cadmpeg_ir::tessellation::Strip<ShadedRow>) -> &'m [ShadedRow],
                ))
            }
        }
    }

    /// Whether every vertex row carries a shading normal and there is at
    /// least one row.
    fn shaded(mesh: &'m AdmittedMesh) -> bool {
        let mut rows = Self::new(mesh);
        matches!(rows, Self::Shaded(_) | Self::ShadedStrips(_)) && rows.next().is_some()
    }
}

impl Iterator for MeshRows<'_> {
    type Item = (FinitePoint3, Option<cadmpeg_ir::features::FiniteVector3>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Positions(rows) => rows.next().map(|position| (*position, None)),
            Self::PositionStrips(rows) => rows.next().map(|position| (*position, None)),
            Self::Shaded(rows) => rows.next().map(|row| (row.position, Some(row.normal))),
            Self::ShadedStrips(rows) => rows.next().map(|row| (row.position, Some(row.normal))),
        }
    }
}

/// The triangles of a strip mesh, expanded strip by strip with alternating
/// winding, without materializing them.
struct StripTriangles<'m, V> {
    strips: std::slice::Iter<'m, cadmpeg_ir::tessellation::Strip<V>>,
    /// First vertex index of the current strip and of the strip after it.
    base: u32,
    next_base: u32,
    /// Next triangle in the current strip and the strip's triangle count.
    index: u32,
    count: u32,
}

impl<V> Iterator for StripTriangles<'_, V> {
    type Item = [u32; 3];

    fn next(&mut self) -> Option<[u32; 3]> {
        while self.index == self.count {
            let strip = self.strips.next()?;
            self.base = self.next_base;
            self.next_base = self
                .base
                .checked_add(u32::try_from(strip.vertices().len()).ok()?)?;
            self.index = 0;
            self.count = u32::try_from(strip.triangle_count()).ok()?;
        }
        let a = self.base.checked_add(self.index)?;
        let (b, c) = (a.checked_add(1)?, a.checked_add(2)?);
        let triangle = if self.index.is_multiple_of(2) {
            [a, b, c]
        } else {
            [a, c, b]
        };
        self.index += 1;
        Some(triangle)
    }
}

/// The triangle corner indices of a mesh, borrowed or expanded from its
/// strips. Each step is fixed work.
enum MeshTriangles<'m> {
    Listed(std::slice::Iter<'m, [u32; 3]>),
    Cornered(
        std::slice::Iter<
            'm,
            cadmpeg_ir::tessellation::ShadedTriangle<cadmpeg_ir::features::FiniteVector3>,
        >,
    ),
    PositionStrips(StripTriangles<'m, FinitePoint3>),
    ShadedStrips(StripTriangles<'m, ShadedRow>),
}

impl<'m> MeshTriangles<'m> {
    fn new(mesh: &'m AdmittedMesh) -> Self {
        fn strips<V>(strips: &cadmpeg_ir::tessellation::Strips<V>) -> StripTriangles<'_, V> {
            StripTriangles {
                strips: strips.as_slice().iter(),
                base: 0,
                next_base: 0,
                index: 0,
                count: 0,
            }
        }
        match mesh {
            TessellationMesh::List { triangles, .. }
            | TessellationMesh::ShadedList { triangles, .. } => Self::Listed(triangles.iter()),
            TessellationMesh::CornerShadedList { triangles, .. } => {
                Self::Cornered(triangles.iter())
            }
            TessellationMesh::Strips { strips: rows } => Self::PositionStrips(strips(rows)),
            TessellationMesh::ShadedStrips { strips: rows } => Self::ShadedStrips(strips(rows)),
        }
    }
}

impl Iterator for MeshTriangles<'_> {
    type Item = [u32; 3];

    fn next(&mut self) -> Option<[u32; 3]> {
        match self {
            Self::Listed(triangles) => triangles.next().copied(),
            Self::Cornered(triangles) => triangles.next().map(|triangle| triangle.corners),
            Self::PositionStrips(triangles) => triangles.next(),
            Self::ShadedStrips(triangles) => triangles.next(),
        }
    }
}

/// The body of each shell, through the region that owns the shell.
fn shell_bodies<'a>(
    ctx: &DecodeContext<'_>,
    regions: &'a [cadmpeg_ir::topology::Region],
    shells: &'a [cadmpeg_ir::topology::Shell],
) -> Result<
    HashMap<&'a cadmpeg_ir::ids::ShellId, &'a cadmpeg_ir::ids::BodyId>,
    cadmpeg_core::CodecError,
> {
    let regions = ctx.collect_hash_map(
        regions.iter().map(|region| (&region.id, &region.body)),
        "index SLDPRT tessellation regions",
    )?;
    let mut bodies = HashMap::new();
    for shell in ctx.admit_iter(shells, "index SLDPRT tessellation shells")? {
        if let Some(body) = ctx.get_hash_map(&regions, &shell.region, "look up SLDPRT hash key")? {
            ctx.insert_hash_map(
                &mut bodies,
                &shell.id,
                *body,
                "index SLDPRT tessellation shells",
            )?;
        }
    }
    Ok(bodies)
}

/// Bind a face-tessellation table when its vertices select one surface face.
///
/// Display coordinates are stored as f32, while the B-rep carriers are f64.
/// The relative tolerance below covers that quantization. Complete analytic
/// trims can distinguish faces on a shared analytic carrier. A NURBS support
/// must provide a forward-evaluated parameter witness within the face or
/// display quantization tolerance; an unconstrained nearest-support fit is
/// not an ownership witness.
pub(crate) fn assign_unique_surface_owners(
    ctx: &DecodeContext<'_>,
    model: &mut cadmpeg_ir::document::Model,
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    for face in ctx.admit_iter(&model.faces, "validate SLDPRT face evaluation tolerances")? {
        if let Some(stated) = face.tolerance {
            if stated.get() < EPS_DISPLAY_QUANTIZATION {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "sldprt face {} evaluation tolerance {} is below display quantization floor {}",
                    face.id,
                    stated.get(),
                    EPS_DISPLAY_QUANTIZATION
                )));
            }
        }
    }
    // The indexes and candidates are scratch for this assignment.
    let mut indexes = ctx.reserve_scoped(0, "index SLDPRT tessellation owners")?;
    let surfaces = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model
                .surfaces
                .iter()
                .map(|surface| (&surface.id, &surface.geometry)),
            "index SLDPRT tessellation surfaces",
        )
    })?;
    let shell_bodies = indexes.with_storage(|| shell_bodies(ctx, &model.regions, &model.shells))?;
    let body_transforms = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model.bodies.iter().map(|body| (&body.id, body.transform)),
            "index SLDPRT tessellation body transforms",
        )
    })?;
    let loops = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model.loops.iter().map(|loop_| (&loop_.id, loop_)),
            "index SLDPRT tessellation loops",
        )
    })?;
    let coedges = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model.coedges.iter().map(|coedge| (&coedge.id, coedge)),
            "index SLDPRT tessellation coedges",
        )
    })?;
    let edges = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model.edges.iter().map(|edge| (&edge.id, edge)),
            "index SLDPRT tessellation edges",
        )
    })?;
    let vertices = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model.vertices.iter().map(|vertex| (&vertex.id, vertex)),
            "index SLDPRT tessellation vertices",
        )
    })?;
    let points = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model
                .points
                .iter()
                .map(|point| (&point.id, point.position().get())),
            "index SLDPRT tessellation points",
        )
    })?;
    let curves = indexes.with_storage(|| {
        ctx.collect_hash_map(
            model
                .curves
                .iter()
                .map(|curve| (&curve.id, &curve.geometry)),
            "index SLDPRT tessellation curves",
        )
    })?;
    let coordinate_scale = ctx
        .admit_iter(&model.points, "measure SLDPRT tessellation coordinates")?
        .map(|point| point.position().get())
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(1.0_f64, f64::max);
    let topology = TrimTopology {
        loops: &loops,
        coedges: &coedges,
        edges: &edges,
        vertices: &vertices,
        points: &points,
        curves: &curves,
        coordinate_scale,
    };
    let mut candidates = Vec::new();
    for face in ctx.admit_iter(&model.faces, "select SLDPRT tessellation face candidates")? {
        let Some(body) = ctx.get_hash_map(&shell_bodies, &face.shell, "look up SLDPRT hash key")?
        else {
            continue;
        };
        let inverse = match ctx
            .get_hash_map(&body_transforms, *body, "look up SLDPRT hash key")?
            .copied()
            .flatten()
        {
            Some(transform) if transform.is_proper_rigid() => {
                match transform.try_inverse_affine().ok() {
                    Some(value) => value,
                    None => continue,
                }
            }
            Some(_) => continue,
            None => cadmpeg_ir::transform::Transform::identity(),
        };
        let Some(surface) =
            ctx.get_hash_map(&surfaces, &face.surface, "look up SLDPRT hash key")?
        else {
            continue;
        };
        let trim = indexes.with_storage(|| analytic_trim(ctx, face, surface, &topology))?;
        indexes.with_storage(|| {
            ctx.push_vec(
                &mut candidates,
                SurfaceCandidate {
                    face: &face.id,
                    body,
                    surface,
                    tolerance: face
                        .tolerance
                        .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
                    inverse,
                    trim,
                },
                "collect SLDPRT tessellation face candidates",
            )
        })?;
    }

    let mut assigned = Vec::new();
    for mesh in ctx.admit_iter(&mut model.tessellations, "scan SLDPRT tessellations")? {
        if mesh.body.is_some()
            || !mesh.faces.is_empty()
            || MeshRows::new(mesh.mesh()).next().is_none()
        {
            continue;
        }
        let (owner, _owner_scratch) = ctx
            .with_scoped_storage("select SLDPRT tessellation owner", || {
                select_surface_owner(ctx, mesh.mesh(), &candidates)
            })?;
        let Some((index, chordal_deflection)) = owner else {
            continue;
        };
        let owner = &candidates[index];
        ctx.reserve_vec(
            &mut mesh.faces,
            1,
            "assign SLDPRT geometric tessellation face",
        )?;
        let face = owner
            .face
            .try_clone_for_decode(ctx, "retain SLDPRT tessellation face ID")?;
        let body = owner
            .body
            .try_clone_for_decode(ctx, "retain SLDPRT tessellation body ID")?;
        let assigned_id =
            ctx.copy_retained_text(mesh.id.as_str(), "retain SLDPRT assigned tessellation ID")?;
        ctx.reserve_vec(&mut assigned, 1, "collect SLDPRT assigned tessellations")?;
        mesh.faces.push(face);
        mesh.body = Some(body);
        if let Some(deflection) = chordal_deflection {
            mesh.set_chordal_deflection(Some(deflection))
                .map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "invalid tessellation deflection: {error}"
                    ))
                })?;
        }
        assigned.push(assigned_id);
    }
    Ok(assigned)
}

/// The candidate that owns `mesh`, and the chordal deflection measured when
/// the owner is chosen by an approximate fit.
fn select_surface_owner(
    ctx: &DecodeContext<'_>,
    mesh: &AdmittedMesh,
    candidates: &[SurfaceCandidate<'_>],
) -> Result<Option<(usize, Option<f64>)>, cadmpeg_core::CodecError> {
    let mut coordinate_scale = 1.0_f64;
    let mut rows = MeshRows::new(mesh);
    while let Some((point, _)) =
        ctx.next_charged(&mut rows, "measure SLDPRT tessellation vertices")?
    {
        let point = point.get();
        coordinate_scale = coordinate_scale
            .max(point.x.abs())
            .max(point.y.abs())
            .max(point.z.abs());
    }
    let quantization_tolerance =
        coordinate_scale * f64::from(f32::EPSILON) * DISPLAY_QUANTIZATION_ULPS
            + EPS_DISPLAY_QUANTIZATION;
    let mut owners = Vec::new();
    for (index, candidate) in ctx
        .admit_iter(candidates, "test SLDPRT tessellation face candidate")?
        .enumerate()
    {
        let tolerance = candidate.tolerance.max(quantization_tolerance);
        let Some(surface) = candidate.surface.solved() else {
            continue;
        };
        let fits = ctx.all_by(
            MeshRows::new(mesh),
            |(point, _)| {
                let Some(local) = candidate.inverse.apply_point(point.get()) else {
                    return Ok(false);
                };
                Ok(surface_measure(ctx, surface, local.get(), Some(tolerance))?
                    .is_some_and(|measure| measure.residual <= tolerance))
            },
            "test SLDPRT tessellation vertex",
        )?;
        if fits
            && match candidate.trim.as_ref() {
                Some(trim) => trim.contains_mesh(ctx, mesh, candidate.inverse, tolerance)?,
                None => true,
            }
        {
            ctx.push_vec(&mut owners, index, "collect SLDPRT tessellation owners")?;
        }
    }
    if let [owner] = owners.as_slice() {
        return Ok(Some((*owner, None)));
    }
    if ctx.any_by(
        &owners,
        |index| {
            Ok(candidates[*index]
                .surface
                .solved()
                .is_some_and(contains_nurbs_surface))
        },
        "scan SLDPRT tessellation owners",
    )? {
        return Ok(None);
    }
    Ok(
        approximate_surface_owner(ctx, mesh, candidates, quantization_tolerance)?
            .map(|(index, deflection)| (index, Some(deflection))),
    )
}

fn approximate_surface_owner(
    ctx: &DecodeContext<'_>,
    mesh: &AdmittedMesh,
    candidates: &[SurfaceCandidate<'_>],
    quantization_tolerance: f64,
) -> Result<Option<(usize, f64)>, cadmpeg_core::CodecError> {
    if !MeshRows::shaded(mesh) {
        return Ok(None);
    }
    let mut fits = Vec::new();
    for (index, candidate) in ctx
        .admit_iter(candidates, "scan SLDPRT surface fit candidates")?
        .enumerate()
    {
        let Some(surface) = candidate.surface.solved() else {
            continue;
        };
        let mut max_residual = 0.0_f64;
        let aligned = ctx.all_by(
            MeshRows::new(mesh),
            |(point, normal)| {
                let (Some(normal), Some(local_point)) =
                    (normal, candidate.inverse.apply_point(point.get()))
                else {
                    return Ok(false);
                };
                let Some(measure) = surface_measure(ctx, surface, local_point.get(), None)? else {
                    return Ok(false);
                };
                let (Some(surface_normal), Some(mesh_normal)) = (
                    measure.normal,
                    candidate
                        .inverse
                        .apply_vector(normal.get())
                        .and_then(|normal| normal.unit()),
                ) else {
                    return Ok(false);
                };
                if surface_normal.dot(mesh_normal).abs() < MIN_TESSELLATION_NORMAL_ALIGNMENT {
                    return Ok(false);
                }
                max_residual = max_residual.max(measure.residual);
                Ok(true)
            },
            "scan SLDPRT surface fit vertices",
        )?;
        if !aligned || is_planar_surface(surface) && max_residual > quantization_tolerance {
            continue;
        }
        ctx.push_vec(
            &mut fits,
            (index, max_residual),
            "collect SLDPRT tessellation surface fits",
        )?;
    }
    if fits.is_empty() {
        return approximate_trimmed_surface_owner(ctx, mesh, candidates, quantization_tolerance);
    }
    ctx.stable_sort_by(
        &mut fits,
        |value| &value.1,
        f64::total_cmp,
        "sort SLDPRT tessellation surface fits",
    )?;
    // The fits within quantization of the best are a prefix of the sorted fits.
    let best_deflection = fits[0].1;
    let near_best = ctx.partition_point(
        &fits,
        |(_, deflection)| Ok(*deflection <= best_deflection + quantization_tolerance),
        "bound SLDPRT tessellation surface fits",
    )?;
    let mut trimmed = Vec::new();
    for fit @ (index, _) in ctx
        .admit_iter(&fits[..near_best], "scan SLDPRT fits values")?
        .copied()
    {
        let keep = match candidates[index].trim.as_ref() {
            Some(trim) => {
                trim.contains_mesh(ctx, mesh, candidates[index].inverse, quantization_tolerance)?
            }
            None => true,
        };
        if keep {
            ctx.push_vec(&mut trimmed, fit, "collect SLDPRT trimmed surface fits")?;
        }
    }
    let [(index, deflection), rest @ ..] = trimmed.as_slice() else {
        return Ok(None);
    };
    if rest
        .first()
        .is_some_and(|(_, next, ..)| *next <= *deflection + quantization_tolerance)
    {
        return Ok(None);
    }
    Ok(Some((*index, *deflection)))
}

/// Use a unique analytic trim as an ownership witness when stored display
/// normals are absent or inconsistent. This path requires a bounded trim, so
/// geometric coincidence on an unbounded analytic carrier cannot fabricate an
/// owner without the normal agreement required above.
fn approximate_trimmed_surface_owner(
    ctx: &DecodeContext<'_>,
    mesh: &AdmittedMesh,
    candidates: &[SurfaceCandidate<'_>],
    quantization_tolerance: f64,
) -> Result<Option<(usize, f64)>, cadmpeg_core::CodecError> {
    let mut fits = Vec::new();
    for (index, candidate) in ctx
        .admit_iter(candidates, "scan SLDPRT surface fit candidates")?
        .enumerate()
    {
        let (Some(trim), Some(surface)) = (candidate.trim.as_ref(), candidate.surface.solved())
        else {
            continue;
        };
        let mut max_residual = 0.0_f64;
        let measured = ctx.all_by(
            MeshRows::new(mesh),
            |(point, _)| {
                let Some(local_point) = candidate.inverse.apply_point(point.get()) else {
                    return Ok(false);
                };
                let Some(measure) = surface_measure(ctx, surface, local_point.get(), None)? else {
                    return Ok(false);
                };
                max_residual = max_residual.max(measure.residual);
                Ok(true)
            },
            "scan SLDPRT trimmed fit vertices",
        )?;
        if !measured || is_planar_surface(surface) && max_residual > quantization_tolerance {
            continue;
        }
        if trim.contains_mesh(ctx, mesh, candidate.inverse, quantization_tolerance)? {
            ctx.push_vec(
                &mut fits,
                (index, max_residual),
                "collect SLDPRT tessellation trimmed fits",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut fits,
        |value| &value.1,
        f64::total_cmp,
        "sort SLDPRT tessellation surface fits",
    )?;
    // The best fit owns the mesh when no other fit is within quantization of it.
    let [(index, deflection), rest @ ..] = fits.as_slice() else {
        return Ok(None);
    };
    if rest
        .first()
        .is_some_and(|(_, next)| *next <= *deflection + quantization_tolerance)
    {
        return Ok(None);
    }
    Ok(Some((*index, *deflection)))
}

fn is_planar_surface(surface: &SolvedSurfaceGeometry) -> bool {
    match surface {
        SolvedSurfaceGeometry::Plane(_) => true,
        SolvedSurfaceGeometry::Transformed(placed) => is_planar_surface(placed.basis()),
        _ => false,
    }
}

fn contains_nurbs_surface(surface: &SolvedSurfaceGeometry) -> bool {
    match surface {
        SolvedSurfaceGeometry::Nurbs(_) => true,
        SolvedSurfaceGeometry::Transformed(placed) => contains_nurbs_surface(placed.basis()),
        _ => false,
    }
}

/// Bind `DisplayLists` tables to faces through their complete persistent identity.
///
/// The identity is a source-declared face key, so it is stronger than a
/// geometric coincidence test. A repeated key with different B-rep targets or
/// repeated table IDs with different identities is rejected as ambiguous.
pub(crate) fn assign_persistent_owners(
    ctx: &DecodeContext<'_>,
    model: &mut cadmpeg_ir::document::Model,
    face_identities: &[(FaceId, PersistentFaceIdentity)],
    bindings: &[PersistentFaceBinding],
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    const IDENTITIES: &str = "index SLDPRT persistent face identities";
    const BINDINGS: &str = "index SLDPRT persistent tessellation bindings";
    // The indexes are scratch for this assignment.
    let mut indexes = ctx.reserve_scoped(0, "index SLDPRT persistent tessellation owners")?;
    let faces_by_identity = indexes.with_storage(|| {
        let mut faces = HashMap::<&PersistentFaceIdentity, Option<&FaceId>>::new();
        for (target, identity) in ctx.admit_iter(face_identities, IDENTITIES)? {
            match ctx.get_mut_hash_map(&mut faces, identity, IDENTITIES)? {
                Some(existing) => {
                    if let Some(face) = *existing {
                        if !ctx.equal(face, target, IDENTITIES)? {
                            *existing = None;
                        }
                    }
                }
                None => {
                    ctx.insert_hash_map(&mut faces, identity, Some(target), IDENTITIES)?;
                }
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(faces)
    })?;
    let face_bodies = indexes.with_storage(|| {
        let shell_bodies = shell_bodies(ctx, &model.regions, &model.shells)?;
        let mut face_bodies = HashMap::new();
        for face in ctx.admit_iter(&model.faces, "index SLDPRT tessellation faces")? {
            if let Some(body) =
                ctx.get_hash_map(&shell_bodies, &face.shell, "look up SLDPRT hash key")?
            {
                ctx.insert_hash_map(
                    &mut face_bodies,
                    &face.id,
                    *body,
                    "index SLDPRT tessellation faces",
                )?;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(face_bodies)
    })?;
    let bindings_by_mesh = indexes.with_storage(|| {
        let mut meshes = HashMap::<&str, Option<&PersistentFaceIdentity>>::new();
        for binding in ctx.admit_iter(bindings, BINDINGS)? {
            let key = binding.tessellation.as_str();
            let identity = &binding.identity;
            match ctx.get_mut_hash_map(&mut meshes, key, BINDINGS)? {
                Some(existing) => {
                    if let Some(stated) = *existing {
                        if !ctx.equal(stated, identity, BINDINGS)? {
                            *existing = None;
                        }
                    }
                }
                None => {
                    ctx.insert_hash_map(&mut meshes, key, Some(identity), BINDINGS)?;
                }
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(meshes)
    })?;

    let mut assigned = Vec::new();
    for mesh in ctx.admit_iter(&mut model.tessellations, "scan SLDPRT tessellations")? {
        if mesh.body.is_some() || !mesh.faces.is_empty() {
            continue;
        }
        let Some(Some(identity)) = ctx.get_hash_map(
            &bindings_by_mesh,
            mesh.id.as_str(),
            "look up SLDPRT hash key",
        )?
        else {
            continue;
        };
        let Some(Some(face)) =
            ctx.get_hash_map(&faces_by_identity, identity, "look up SLDPRT hash key")?
        else {
            continue;
        };
        let Some(body) = ctx.get_hash_map(&face_bodies, face, "look up SLDPRT hash key")? else {
            continue;
        };
        ctx.reserve_vec(
            &mut mesh.faces,
            1,
            "assign SLDPRT persistent tessellation face",
        )?;
        let face = face.try_clone_for_decode(ctx, "retain SLDPRT tessellation face ID")?;
        let body = body.try_clone_for_decode(ctx, "retain SLDPRT tessellation body ID")?;
        let assigned_id =
            ctx.copy_retained_text(mesh.id.as_str(), "retain SLDPRT assigned tessellation ID")?;
        ctx.reserve_vec(&mut assigned, 1, "collect SLDPRT assigned tessellations")?;
        mesh.faces.push(face);
        mesh.body = Some(body);
        assigned.push(assigned_id);
    }
    Ok(assigned)
}

#[derive(Debug, Clone, Copy)]
struct PlaneFrame {
    origin: FinitePoint3,
    normal: Vector3,
    u_axis: Vector3,
}

impl PlaneFrame {
    fn new(origin: FinitePoint3, normal: Vector3, u_axis: Vector3) -> Option<Self> {
        let normal = normal.unit()?;
        let u_axis = (u_axis - normal.scale(u_axis.dot(normal))).unit()?;
        Some(Self {
            origin,
            normal,
            u_axis,
        })
    }

    fn v_axis(self) -> Vector3 {
        let axis = self.normal.cross(self.u_axis);
        let length = axis.norm();
        Vector3::new(axis.x / length, axis.y / length, axis.z / length)
    }

    fn project(self, point: Point3) -> Point2 {
        let delta = point.vector_from(self.origin.get());
        Point2::new(delta.dot(self.u_axis), delta.dot(self.v_axis()))
    }
}

#[derive(Debug, Clone, Copy)]
struct CircularHole {
    center: Point2,
    radius: f64,
}

#[derive(Debug, Clone)]
enum AnalyticTrim {
    Planar(PlanarTrim),
    Cylindrical(CylindricalTrim),
    Conical(ConicalTrim),
}

impl AnalyticTrim {
    fn contains_mesh(
        &self,
        ctx: &DecodeContext<'_>,
        mesh: &AdmittedMesh,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        match self {
            Self::Planar(trim) => trim.contains_mesh(ctx, mesh, inverse_body, tolerance),
            Self::Cylindrical(trim) => trim.contains_mesh(ctx, mesh, inverse_body, tolerance),
            Self::Conical(trim) => trim.contains_mesh(ctx, mesh, inverse_body, tolerance),
        }
    }
}

#[derive(Debug, Clone)]
enum PlanarOuter {
    Polygon(Vec<Point2>),
    Circle(CircularHole),
}

#[derive(Debug, Clone)]
enum PlanarHole {
    Polygon {
        boundary: Vec<Point2>,
        triangles: Vec<[Point2; 3]>,
    },
    Circle(CircularHole),
}

impl PlanarHole {
    /// A hole bounded by a simple polygon whose doubled signed area is `area`.
    fn polygon(
        ctx: &DecodeContext<'_>,
        boundary: Vec<Point2>,
        area: cadmpeg_ir::scalar::FiniteReal,
        tolerance: f64,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let triangles = require_some!(triangulate_polygon(
            ctx,
            &boundary,
            area.get().signum(),
            tolerance
        )?);
        Ok(Some(Self::Polygon {
            boundary,
            triangles,
        }))
    }
}

#[derive(Debug, Clone)]
struct PlanarTrim {
    frame: PlaneFrame,
    outer: Option<PlanarOuter>,
    holes: Vec<PlanarHole>,
    boundary_tolerance: f64,
}

enum HoleConstraint<'a> {
    Polygon {
        boundary: &'a [Point2],
        triangles: &'a [[Point2; 3]],
    },
    Circle {
        exclusion: CircularHole,
        boundary: CircularHole,
    },
}

#[derive(Debug, Clone, Copy)]
struct CylindricalTrim {
    origin: Point3,
    frame: OrthonormalFrame3,
    radius: f64,
    min_axial: f64,
    max_axial: f64,
    angular_start: f64,
    angular_span: f64,
}

#[derive(Debug, Clone, Copy)]
struct ConicalTrim {
    origin: Point3,
    frame: OrthonormalFrame3,
    radius: f64,
    ratio: PositiveReal,
    slope: f64,
    min_axial: f64,
    max_axial: f64,
    angular_start: f64,
    angular_span: f64,
}

impl CylindricalTrim {
    fn contains_mesh(
        self,
        ctx: &DecodeContext<'_>,
        mesh: &AdmittedMesh,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        ctx.all_by(
            MeshRows::new(mesh),
            |(point, _)| {
                let Some(point) = inverse_body.apply_point(point.get()) else {
                    return Ok(false);
                };
                let point = point.get();
                let axial = point
                    .vector_from(self.origin)
                    .dot(*self.frame.axis().as_raw());
                let angular = cylinder_angle(point, self.origin, &self.frame);
                Ok(axial.is_finite()
                    && axial >= self.min_axial - tolerance
                    && axial <= self.max_axial + tolerance
                    && angular.is_some_and(|angular| {
                        self.angular_span >= std::f64::consts::TAU - EPS_CYLINDER_ANGLE
                            || circular_interval_contains(
                                self.angular_start,
                                self.angular_span,
                                angular,
                                tolerance / self.radius,
                            )
                    }))
            },
            "scan SLDPRT CylindricalTrim mesh vertices",
        )
    }
}

impl ConicalTrim {
    fn contains_mesh(
        self,
        ctx: &DecodeContext<'_>,
        mesh: &AdmittedMesh,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        ctx.all_by(
            MeshRows::new(mesh),
            |(point, _)| {
                let Some(point) = inverse_body.apply_point(point.get()) else {
                    return Ok(false);
                };
                let point = point.get();
                let axial = point
                    .vector_from(self.origin)
                    .dot(*self.frame.axis().as_raw());
                let local_radius = self.radius + axial * self.slope;
                let angular = cone_angle(point, self.origin, &self.frame, self.ratio);
                Ok(axial.is_finite()
                    && local_radius.is_finite()
                    && axial >= self.min_axial - tolerance
                    && axial <= self.max_axial + tolerance
                    && angular.is_some_and(|angular| {
                        self.angular_span >= std::f64::consts::TAU - EPS_CYLINDER_ANGLE
                            || circular_interval_contains(
                                self.angular_start,
                                self.angular_span,
                                angular,
                                tolerance
                                    / (local_radius.abs() * self.ratio.get().min(1.0))
                                        .max(EPS_DISPLAY_QUANTIZATION),
                            )
                    }))
            },
            "scan SLDPRT ConicalTrim mesh vertices",
        )
    }
}

impl PlanarTrim {
    fn contains_mesh(
        &self,
        ctx: &DecodeContext<'_>,
        mesh: &AdmittedMesh,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let tolerance = tolerance + self.boundary_tolerance;
        let mut scratch = ctx.reserve_scoped(0, "collect SLDPRT planar trim projections")?;
        let mut projected = Vec::new();
        let mut rows = MeshRows::new(mesh);
        while let Some((point, _)) =
            ctx.next_charged(&mut rows, "project SLDPRT planar trim mesh vertex")?
        {
            let Some(point) = inverse_body.apply_point(point.get()) else {
                return Ok(false);
            };
            ctx.push_scoped_vec(
                &mut scratch,
                &mut projected,
                self.frame.project(point.get()),
                "collect SLDPRT planar trim projections",
            )?;
        }
        let mut holes = Vec::new();
        for hole in ctx.admit_iter(&self.holes, "test SLDPRT planar trim hole")? {
            let constraint = match hole {
                PlanarHole::Polygon {
                    boundary,
                    triangles,
                } => HoleConstraint::Polygon {
                    boundary,
                    triangles,
                },
                PlanarHole::Circle(hole) => {
                    let Some((exclusion, boundary)) =
                        chordal_hole_constraint(ctx, *hole, &projected, tolerance)?
                    else {
                        return Ok(false);
                    };
                    HoleConstraint::Circle {
                        exclusion,
                        boundary,
                    }
                }
            };
            ctx.push_scoped_vec(
                &mut scratch,
                &mut holes,
                constraint,
                "collect SLDPRT planar trim constraints",
            )?;
        }
        if ctx.any_by(
            &projected,
            |point| {
                let outside = match self.outer.as_ref() {
                    Some(PlanarOuter::Polygon(outer)) => {
                        !polygon_contains(ctx, outer, *point, tolerance)?
                    }
                    Some(PlanarOuter::Circle(outer)) => {
                        !(point_distance(*point, outer.center) <= outer.radius + tolerance)
                    }
                    None => false,
                };
                Ok(outside
                    || ctx.any_by(
                        &holes,
                        |hole| hole.contains_interior(ctx, *point, tolerance),
                        "scan SLDPRT interior planar trim holes",
                    )?)
            },
            "test SLDPRT planar trim points",
        )? {
            return Ok(false);
        }
        // The outer boundary's simplicity at this tolerance, decided once,
        // when the first triangle needs it.
        let mut outer_is_simple = None;
        let mut triangles = MeshTriangles::new(mesh);
        while let Some(triangle) =
            ctx.next_charged(&mut triangles, "scan SLDPRT topology members")?
        {
            let [Some(a), Some(b), Some(c)] = triangle.map(|index| {
                projected
                    .get(cadmpeg_core::decode::index_from_u32(index))
                    .copied()
            }) else {
                return Ok(false);
            };
            if let Some(PlanarOuter::Polygon(boundary)) = &self.outer {
                let simple = match outer_is_simple {
                    Some(simple) => simple,
                    None => *outer_is_simple
                        .insert(simple_polygon_area_twice(ctx, boundary, tolerance)?.is_some()),
                };
                if !simple || !polygon_contains_triangle(ctx, boundary, [a, b, c], tolerance)? {
                    return Ok(false);
                }
            }
            if ctx.any_by(
                &holes,
                |hole| hole.crosses_triangle(ctx, [a, b, c], tolerance),
                "test SLDPRT planar trim holes",
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl HoleConstraint<'_> {
    fn contains_interior(
        &self,
        ctx: &DecodeContext<'_>,
        point: Point2,
        tolerance: f64,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Polygon { boundary, .. } => {
                polygon_strictly_contains(ctx, boundary, point, tolerance)?
            }
            Self::Circle { exclusion, .. } => {
                point_distance(point, exclusion.center) < exclusion.radius - tolerance
            }
        })
    }

    fn crosses_triangle(
        &self,
        ctx: &DecodeContext<'_>,
        triangle: [Point2; 3],
        tolerance: f64,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Polygon { triangles, .. } => ctx.any_by(
                *triangles,
                |hole_triangle| {
                    Ok(triangles_have_positive_overlap(
                        triangle,
                        *hole_triangle,
                        tolerance,
                    ))
                },
                "test SLDPRT planar trim triangles",
            )?,
            Self::Circle {
                exclusion,
                boundary,
            } => triangle_crosses_hole(triangle, *exclusion, *boundary, tolerance),
        })
    }
}

/// The indexed topology and curve maps that every trim grammar reads.
#[derive(Clone, Copy)]
struct TrimTopology<'a> {
    loops: &'a HashMap<&'a cadmpeg_ir::ids::LoopId, &'a cadmpeg_ir::topology::Loop>,
    coedges: &'a HashMap<&'a cadmpeg_ir::ids::CoedgeId, &'a cadmpeg_ir::topology::Coedge>,
    edges: &'a HashMap<&'a cadmpeg_ir::ids::EdgeId, &'a cadmpeg_ir::topology::Edge>,
    vertices: &'a HashMap<&'a cadmpeg_ir::ids::VertexId, &'a cadmpeg_ir::topology::Vertex>,
    points: &'a HashMap<&'a cadmpeg_ir::ids::PointId, Point3>,
    curves: &'a HashMap<&'a cadmpeg_ir::ids::CurveId, &'a CurveGeometry>,
    /// The largest point coordinate magnitude in the model, at least one.
    coordinate_scale: f64,
}

fn closed_planar_circle(
    ctx: &DecodeContext<'_>,
    loop_: &cadmpeg_ir::topology::Loop,
    surface: &SurfaceGeometry,
    frame: PlaneFrame,
    tolerance: f64,
    topology: &TrimTopology<'_>,
) -> Result<Option<CircularHole>, cadmpeg_core::CodecError> {
    let TrimTopology {
        coedges,
        edges,
        vertices,
        points,
        curves,
        ..
    } = *topology;
    let coedge = *require_some!(ctx.get_hash_map(
        coedges,
        &loop_.coedges()[0],
        "look up SLDPRT hash key"
    )?);
    let edge = *require_some!(ctx.get_hash_map(edges, &coedge.edge, "look up SLDPRT hash key")?);
    if loop_.coedges().len() != 1
        || !ctx.equal(
            &coedge.owner_loop,
            &loop_.id,
            "compare SLDPRT trim topology identities",
        )?
    {
        return Ok(None);
    }
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = *require_some!(ctx
        .get_hash_map(
            curves,
            require_some!(edge.curve()),
            "look up SLDPRT hash key"
        )?)
    else {
        return Ok(None);
    };
    let center = circle_curve.center().get();
    let axis = *circle_curve.frame().axis().as_raw();
    let radius = circle_curve.radius().get();
    let boundary_point = *require_some!(ctx.get_hash_map(
        points,
        &require_some!(ctx.get_hash_map(vertices, &edge.start, "look up SLDPRT hash key")?).point,
        "look up SLDPRT hash key"
    )?);
    let end_point = *require_some!(ctx.get_hash_map(
        points,
        &require_some!(ctx.get_hash_map(vertices, &edge.end, "look up SLDPRT hash key")?).point,
        "look up SLDPRT hash key"
    )?);
    if radius <= tolerance
        || axis.dot(frame.normal).abs() < 1.0 - EPS_AXIS_ALIGNMENT
        || require_some!(analytic_surface_residual(
            require_some!(surface.solved()),
            center
        )) > tolerance
        || require_some!(analytic_surface_residual(
            require_some!(surface.solved()),
            boundary_point
        )) > tolerance
        || boundary_point.distance(end_point) > tolerance
        || (boundary_point.distance(center) - radius).abs() > tolerance
    {
        return Ok(None);
    }
    Ok(Some(CircularHole {
        center: frame.project(center),
        radius,
    }))
}

#[derive(Clone, Copy)]
struct BoundaryTolerance {
    tolerance: f64,
    sampling_tolerance: f64,
}

fn planar_boundary_samples(
    ctx: &DecodeContext<'_>,
    curve: &CurveGeometry,
    start: Point3,
    end: Point3,
    surface: &SurfaceGeometry,
    frame: PlaneFrame,
    limits: BoundaryTolerance,
) -> Result<Option<(Vec<Point2>, f64)>, cadmpeg_core::CodecError> {
    let BoundaryTolerance {
        tolerance,
        sampling_tolerance,
    } = limits;
    match curve {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => {
            Ok(Some((vec![frame.project(start)], 0.0)))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            let axis = *circle_curve.frame().axis().as_raw();
            let (reference, transverse) = perpendicular_basis(circle_curve.frame());
            let radius = circle_curve.radius().get();
            if axis.dot(frame.normal).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                || radius <= tolerance
                || require_some!(analytic_surface_residual(
                    require_some!(surface.solved()),
                    center
                )) > tolerance
            {
                return Ok(None);
            }
            let endpoint_tolerance = tolerance.max(sampling_tolerance);
            let start_parameter = ellipse_parameter(
                start,
                center,
                reference,
                transverse,
                radius,
                radius,
                endpoint_tolerance,
            );
            let start_parameter = require_some!(start_parameter);
            let end_parameter = ellipse_parameter(
                end,
                center,
                reference,
                transverse,
                radius,
                radius,
                endpoint_tolerance,
            );
            let end_parameter = require_some!(end_parameter);
            let span = require_some!(shortest_arc_span(start_parameter, end_parameter));
            PlanarArc {
                center,
                first_direction: reference,
                second_direction: transverse,
                first_radius: radius,
                second_radius: radius,
            }
            .samples(
                ctx,
                start_parameter,
                span,
                surface,
                frame,
                BoundaryTolerance {
                    tolerance,
                    sampling_tolerance,
                },
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            let axis = *ellipse_curve.frame().axis().as_raw();
            let major_direction = *ellipse_curve.frame().reference().as_raw();
            let minor_direction = *ellipse_curve.frame().binormal().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            if axis.dot(frame.normal).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                || major_radius <= tolerance
                || minor_radius <= tolerance
                || require_some!(analytic_surface_residual(
                    require_some!(surface.solved()),
                    center
                )) > tolerance
            {
                return Ok(None);
            }
            let endpoint_tolerance = tolerance.max(sampling_tolerance);
            let start_parameter = ellipse_parameter(
                start,
                center,
                major_direction,
                minor_direction,
                major_radius,
                minor_radius,
                endpoint_tolerance,
            );
            let start_parameter = require_some!(start_parameter);
            let end_parameter = ellipse_parameter(
                end,
                center,
                major_direction,
                minor_direction,
                major_radius,
                minor_radius,
                endpoint_tolerance,
            );
            let end_parameter = require_some!(end_parameter);
            let span = require_some!(shortest_arc_span(start_parameter, end_parameter));
            PlanarArc {
                center,
                first_direction: major_direction,
                second_direction: minor_direction,
                first_radius: major_radius,
                second_radius: minor_radius,
            }
            .samples(
                ctx,
                start_parameter,
                span,
                surface,
                frame,
                BoundaryTolerance {
                    tolerance,
                    sampling_tolerance,
                },
            )
        }
        _ => Ok(None),
    }
}

#[derive(Clone, Copy)]
struct PlanarArc {
    center: Point3,
    first_direction: Vector3,
    second_direction: Vector3,
    first_radius: f64,
    second_radius: f64,
}

impl PlanarArc {
    fn samples(
        self,
        ctx: &DecodeContext<'_>,
        start_parameter: f64,
        span: f64,
        surface: &SurfaceGeometry,
        frame: PlaneFrame,
        limits: BoundaryTolerance,
    ) -> Result<Option<(Vec<Point2>, f64)>, cadmpeg_core::CodecError> {
        let BoundaryTolerance {
            tolerance,
            sampling_tolerance,
        } = limits;
        let radius = self.first_radius.max(self.second_radius);
        let Some((segments, boundary_tolerance)) =
            planar_arc_segments(span, radius, sampling_tolerance)
        else {
            return Ok(None);
        };
        let solved_surface = require_some!(surface.solved());
        let mut points = Vec::new();
        for index in 0..segments {
            ctx.charge_work(1, "sample SLDPRT planar trim arc")?;
            let (Some(index_value), Some(segment_value)) = (
                cadmpeg_core::convert::f64_from_index(index),
                cadmpeg_core::convert::f64_from_index(segments),
            ) else {
                return Ok(None);
            };
            let parameter = start_parameter + span * index_value / segment_value;
            let point = self
                .center
                .translated(self.first_direction, self.first_radius * parameter.cos())
                .translated(self.second_direction, self.second_radius * parameter.sin());
            if analytic_surface_residual(solved_surface, point)
                .is_none_or(|residual| residual > tolerance)
            {
                return Ok(None);
            }
            ctx.reserve_vec(&mut points, 1, "collect SLDPRT planar trim arc samples")?;
            points.push(frame.project(point));
        }
        Ok(Some((points, boundary_tolerance)))
    }
}

fn ellipse_parameter(
    point: Point3,
    center: Point3,
    major_direction: Vector3,
    minor_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
    tolerance: f64,
) -> Option<f64> {
    let delta = point.vector_from(center);
    let cosine = delta.dot(major_direction) / major_radius;
    let sine = delta.dot(minor_direction) / minor_radius;
    let normalization = cosine.hypot(sine);
    (normalization.is_finite()
        && (normalization - 1.0).abs() <= tolerance / major_radius.min(minor_radius))
    .then_some(sine.atan2(cosine).rem_euclid(std::f64::consts::TAU))
}

fn shortest_arc_span(start: f64, end: f64) -> Option<f64> {
    let forward = (end - start).rem_euclid(std::f64::consts::TAU);
    let span = if forward <= std::f64::consts::PI {
        forward
    } else {
        forward - std::f64::consts::TAU
    };
    (span.abs() > EPS_CYLINDER_ANGLE && span.abs() < std::f64::consts::PI - EPS_CYLINDER_ANGLE)
        .then_some(span)
}

/// Answer the segment count and the sagitta the chords leave.
///
/// A chord whose sagitta is `tolerance` subtends `2 * asin(half_chord)` with
/// `half_chord = sqrt(tolerance / (2 * radius))`, the sine of the quarter
/// span. `radius` is a positive IR radius and `tolerance` is a positive
/// sampling tolerance, so the ratio is positive. It reaches one at a tolerance
/// of twice the radius, which is not an error: the sagitta of the complete
/// circle is the diameter, so a tolerance at or beyond it admits the whole
/// circle in one segment, and the sine of the quarter span stays at one.
///
/// `None` when the span is not finite or the segment count has no exact
/// floating-point value.
fn planar_arc_segments(span: f64, radius: f64, tolerance: f64) -> Option<(usize, f64)> {
    if !span.is_finite() {
        return None;
    }
    let half_chord = (0.5 * (tolerance / radius)).min(1.0).sqrt();
    let maximum_span = 4.0 * half_chord.asin();
    let requested = if maximum_span.is_finite() && maximum_span > EPS_CYLINDER_ANGLE {
        match cadmpeg_core::convert::truncate_f64_to_usize((span.abs() / maximum_span).ceil()) {
            Some(count) => count,
            None => MAX_PLANAR_TRIM_ARC_SEGMENTS,
        }
    } else {
        MAX_PLANAR_TRIM_ARC_SEGMENTS
    };
    let segments = requested.clamp(1, MAX_PLANAR_TRIM_ARC_SEGMENTS);
    let actual_span = span.abs() / cadmpeg_core::convert::f64_from_index(segments)?;
    let sine = (actual_span / 4.0).sin();
    Some((segments, (radius * sine) * (2.0 * sine)))
}

fn planar_trim(
    ctx: &DecodeContext<'_>,
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    topology: &TrimTopology<'_>,
) -> Result<Option<PlanarTrim>, cadmpeg_core::CodecError> {
    let TrimTopology {
        loops,
        coedges,
        edges,
        vertices,
        points,
        curves,
        coordinate_scale,
    } = *topology;
    let frame = require_some!(plane_frame(require_some!(surface.solved())));
    let tolerance = require_some!(FaceEvaluationTolerance::of(face)).get();
    let sampling_tolerance = tolerance.max(
        coordinate_scale * f64::from(f32::EPSILON) * DISPLAY_QUANTIZATION_ULPS
            + EPS_DISPLAY_QUANTIZATION,
    );
    let mut polygons = Vec::new();
    let mut circles = Vec::new();
    let mut boundary_tolerance = 0.0_f64;
    let (outer_loop, inner_loops) = match &face.loops {
        cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => (&[][..], loops.as_slice()),
        cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
            (std::slice::from_ref(outer), inner.as_slice())
        }
    };
    let mut loop_ids = outer_loop.iter().chain(inner_loops);
    while let Some(loop_id) = ctx.next_charged(&mut loop_ids, "scan SLDPRT planar trim loop")? {
        let loop_ =
            *require_some!(ctx.get_hash_map(&(loops), loop_id, "look up SLDPRT hash key")?);
        if loop_.coedges().is_empty()
            || loop_.vertices().next().is_some()
            || !ctx.equal(
                &loop_.face,
                &face.id,
                "compare SLDPRT trim topology identities",
            )?
        {
            return Ok(None);
        }
        if loop_.coedges().len() == 1 {
            let circle = require_some!(closed_planar_circle(
                ctx, loop_, surface, frame, tolerance, topology,
            )?);
            ctx.reserve_vec(&mut circles, 1, "collect SLDPRT planar trim circles")?;
            circles.push(circle);
            continue;
        }

        let mut polygon = Vec::new();
        let mut first_start = None;
        let mut previous_end = None;
        for coedge_id in loop_.coedges() {
            ctx.charge_work(1, "scan SLDPRT planar trim coedge")?;
            let coedge = *require_some!(ctx.get_hash_map(
                &(coedges),
                coedge_id,
                "look up SLDPRT hash key"
            )?);
            let edge = *require_some!(ctx.get_hash_map(
                &(edges),
                &coedge.edge,
                "look up SLDPRT hash key"
            )?);
            if !ctx.equal(
                &coedge.owner_loop,
                &loop_.id,
                "compare SLDPRT trim topology identities",
            )? {
                return Ok(None);
            }
            let (start, end) = match coedge.sense {
                Sense::Forward => (&edge.start, &edge.end),
                Sense::Reversed => (&edge.end, &edge.start),
            };
            let start = *require_some!(ctx.get_hash_map(
                &(points),
                &require_some!(ctx.get_hash_map(&(vertices), start, "look up SLDPRT hash key")?)
                    .point,
                "look up SLDPRT hash key"
            )?);
            let end = *require_some!(ctx.get_hash_map(
                &(points),
                &require_some!(ctx.get_hash_map(&(vertices), end, "look up SLDPRT hash key")?)
                    .point,
                "look up SLDPRT hash key"
            )?);
            if require_some!(analytic_surface_residual(
                require_some!(surface.solved()),
                start
            )) > tolerance
                || require_some!(analytic_surface_residual(
                    require_some!(surface.solved()),
                    end
                )) > tolerance
                || previous_end.is_some_and(|previous: Point3| previous.distance(start) > tolerance)
            {
                return Ok(None);
            }
            let (samples, sample_tolerance) = require_some!(planar_boundary_samples(
                ctx,
                require_some!(ctx.get_hash_map(
                    &(curves),
                    require_some!(edge.curve()),
                    "look up SLDPRT hash key"
                )?),
                start,
                end,
                surface,
                frame,
                crate::tessellation::BoundaryTolerance {
                    tolerance,
                    sampling_tolerance
                }
            )?);
            ctx.reserve_vec(
                &mut polygon,
                samples.len(),
                "collect SLDPRT planar trim polygon",
            )?;
            polygon.extend(samples);
            boundary_tolerance = boundary_tolerance.max(sample_tolerance);
            first_start.get_or_insert(start);
            previous_end = Some(end);
        }
        if require_some!(previous_end).distance(require_some!(first_start)) > tolerance {
            return Ok(None);
        }
        ctx.reserve_vec(&mut polygons, 1, "collect SLDPRT planar trim polygons")?;
        polygons.push(polygon);
    }
    let (outer, holes) = if polygons.is_empty() {
        if circles.is_empty() {
            return Ok(None);
        }
        let (outer, holes) = require_some!(circular_outer_and_holes(ctx, &circles, tolerance)?);
        let mut planar_holes = Vec::new();
        for hole in ctx.admit_iter(&holes, "scan SLDPRT holes values")?.copied() {
            ctx.reserve_vec(&mut planar_holes, 1, "collect SLDPRT circular planar holes")?;
            planar_holes.push(PlanarHole::Circle(hole));
        }
        (PlanarOuter::Circle(outer), planar_holes)
    } else {
        let (outer, holes) = require_some!(polygon_outer_and_holes(
            ctx,
            polygons,
            &circles,
            sampling_tolerance
        )?);
        (PlanarOuter::Polygon(outer), holes)
    };
    Ok(Some(PlanarTrim {
        frame,
        outer: Some(outer),
        holes,
        boundary_tolerance,
    }))
}

/// Split a planar face's polygon loops and circular loops into its outer
/// polygon and its holes.
///
/// The outer polygon is the one polygon that encloses every other polygon and
/// every circle; the holes must not overlap. Every polygon's simplicity is
/// decided once, when there are several polygons to compare.
fn polygon_outer_and_holes(
    ctx: &DecodeContext<'_>,
    polygons: Vec<Vec<Point2>>,
    circles: &[CircularHole],
    tolerance: f64,
) -> Result<Option<(Vec<Point2>, Vec<PlanarHole>)>, cadmpeg_core::CodecError> {
    const COMPARE: &str = "compare SLDPRT planar trim boundaries";
    let mut scratch = ctx.reserve_scoped(0, COMPARE)?;
    let mut areas = Vec::new();
    if polygons.len() > 1 {
        for polygon in ctx.admit_iter(&polygons, "test SLDPRT planar trim polygon simplicity")? {
            let area = simple_polygon_area_twice(ctx, polygon, tolerance)?;
            ctx.push_scoped_vec(&mut scratch, &mut areas, area, COMPARE)?;
        }
    }
    let simple = |index: usize| areas.get(index).is_some_and(Option::is_some);
    let mut outer_index = None;
    for (index, outer) in ctx
        .admit_iter(&polygons, "scan SLDPRT planar trim outer candidates")?
        .enumerate()
    {
        if ctx.all_by(
            polygons
                .iter()
                .enumerate()
                .filter(|(inner_index, _)| *inner_index != index),
            |(inner_index, inner)| {
                Ok(simple(inner_index)
                    && simple(index)
                    && polygon_inside_polygon(ctx, inner, outer, tolerance)?)
            },
            COMPARE,
        )? && ctx.all_by(
            circles,
            |circle| circle_inside_polygon(ctx, outer, *circle, tolerance),
            "compare SLDPRT planar trim circles",
        )? && outer_index.replace(index).is_some()
        {
            return Ok(None);
        }
    }
    let outer_index = require_some!(outer_index);
    // The outer polygon encloses every other polygon, so each other polygon
    // is a hole inside it; the holes must not overlap one another.
    let hole_count = polygons.len() - 1;
    let hole = |position: usize| {
        let index = if position < outer_index {
            position
        } else {
            position + 1
        };
        polygons[index].as_slice()
    };
    if ctx.any_by(
        0..hole_count,
        |left| {
            ctx.any_by(
                left + 1..hole_count,
                |right| polygons_overlap(ctx, hole(left), hole(right), tolerance),
                "scan SLDPRT subsequent polygon holes",
            )
        },
        "scan SLDPRT polygon hole pairs",
    )? || ctx.any_by(
        0..hole_count,
        |position| {
            ctx.any_by(
                circles,
                |circle| circle_overlaps_polygon(ctx, *circle, hole(position), tolerance),
                "scan SLDPRT circles against polygon holes",
            )
        },
        "scan SLDPRT polygon circular holes",
    )? || ctx.any_by(
        0..circles.len(),
        |left| {
            ctx.any_by(
                &circles[left + 1..],
                |right| {
                    Ok(point_distance(circles[left].center, right.center)
                        < circles[left].radius + right.radius - tolerance)
                },
                "scan SLDPRT subsequent circular holes",
            )
        },
        "scan SLDPRT circular hole pairs",
    )? {
        return Ok(None);
    }
    let mut holes = Vec::new();
    ctx.reserve_vec(
        &mut holes,
        circles.len() + hole_count,
        "collect SLDPRT planar trim holes",
    )?;
    for circle in ctx.admit_iter(circles, "scan SLDPRT circles values")? {
        holes.push(PlanarHole::Circle(*circle));
    }
    let mut outer = Vec::new();
    for (index, polygon) in ctx
        .admit_iter(polygons, "collect SLDPRT planar trim polygons")?
        .enumerate()
    {
        if index == outer_index {
            outer = polygon;
            continue;
        }
        let area = require_some!(areas.get(index).copied().flatten());
        holes.push(require_some!(PlanarHole::polygon(
            ctx, polygon, area, tolerance
        )?));
    }
    Ok(Some((outer, holes)))
}

fn planar_hole_trim(
    ctx: &DecodeContext<'_>,
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    topology: &TrimTopology<'_>,
) -> Result<Option<PlanarTrim>, cadmpeg_core::CodecError> {
    let loops = topology.loops;
    let frame = require_some!(plane_frame(require_some!(surface.solved())));
    let tolerance = require_some!(FaceEvaluationTolerance::of(face)).get();
    let mut has_polygon_loop = false;
    let (outer_loop, inner_loops) = match &face.loops {
        cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => (&[][..], loops.as_slice()),
        cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
            (std::slice::from_ref(outer), inner.as_slice())
        }
    };
    let mut loop_ids = outer_loop.iter().chain(inner_loops);
    while let Some(loop_id) = ctx.next_charged(&mut loop_ids, "scan SLDPRT planar hole loop")? {
        let loop_ =
            require_some!(ctx.get_hash_map(&(loops), loop_id, "look up SLDPRT hash key")?);
        has_polygon_loop |= loop_.coedges().len() > 1;
    }
    if !has_polygon_loop {
        return Ok(None);
    }
    let mut holes = Vec::new();
    for loop_id in ctx
        .admit_iter(outer_loop, "test SLDPRT planar hole loop")?
        .chain(ctx.admit_iter(inner_loops, "test SLDPRT planar hole loop")?)
    {
        let loop_ =
            require_some!(ctx.get_hash_map(&(loops), loop_id, "look up SLDPRT hash key")?);
        if loop_.coedges().len() == 1
            && loop_.vertices().next().is_none()
            && ctx.equal(
                &loop_.face,
                &face.id,
                "compare SLDPRT trim topology identities",
            )?
        {
            if let Some(circle) =
                closed_planar_circle(ctx, loop_, surface, frame, tolerance, topology)?
            {
                ctx.reserve_vec(&mut holes, 1, "collect SLDPRT planar hole trim")?;
                holes.push(PlanarHole::Circle(circle));
            }
        }
    }
    Ok((!holes.is_empty()).then_some(PlanarTrim {
        frame,
        outer: None,
        holes,
        boundary_tolerance: 0.0,
    }))
}

fn cylindrical_trim(
    ctx: &DecodeContext<'_>,
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    topology: &TrimTopology<'_>,
) -> Result<Option<CylindricalTrim>, cadmpeg_core::CodecError> {
    let TrimTopology {
        loops,
        coedges,
        edges,
        vertices,
        points,
        curves,
        ..
    } = *topology;
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface else {
        return Ok(None);
    };
    let origin = cylinder_surface.origin().get();
    let frame = *cylinder_surface.frame();
    let axis = *frame.axis().as_raw();
    let radius = cylinder_surface.radius().get();
    if radius <= EPS_DISPLAY_QUANTIZATION {
        return Ok(None);
    }
    if face.loops.len() != 1 {
        return Ok(None);
    }
    let loop_id = require_some!(face.loops.iter().next());
    let loop_ = *require_some!(ctx.get_hash_map(&(loops), loop_id, "look up SLDPRT hash key")?);
    if loop_.coedges().is_empty()
        || loop_.vertices().next().is_some()
        || !ctx.equal(
            &loop_.face,
            &face.id,
            "compare SLDPRT trim topology identities",
        )?
    {
        return Ok(None);
    }
    let tolerance = require_some!(FaceEvaluationTolerance::of(face)).get();
    let mut axial_bounds = None::<(f64, f64)>;
    let mut angles = Vec::new();
    for coedge_id in loop_.coedges() {
        ctx.charge_work(1, "scan SLDPRT cylindrical trim coedges")?;
        let coedge =
            *require_some!(ctx.get_hash_map(&(coedges), coedge_id, "look up SLDPRT hash key")?);
        if !ctx.equal(
            &coedge.owner_loop,
            &loop_.id,
            "compare SLDPRT trim topology identities",
        )? {
            return Ok(None);
        }
        let edge =
            *require_some!(ctx.get_hash_map(&(edges), &coedge.edge, "look up SLDPRT hash key")?);
        let curve = require_some!(ctx.get_hash_map(
            &(curves),
            require_some!(edge.curve()),
            "look up SLDPRT hash key"
        )?);
        match curve {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let direction = *line_curve.direction().as_raw();
                if direction.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT {
                    return Ok(None);
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let curve_axis = circle_curve.frame().axis().as_raw();
                let curve_radius = circle_curve.radius().get();
                if curve_axis.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || (curve_radius - radius).abs() > tolerance
                {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
        for vertex_id in [&edge.start, &edge.end] {
            ctx.charge_work(1, "scan SLDPRT cylindrical trim endpoints")?;
            let vertex = require_some!(ctx.get_hash_map(
                &(vertices),
                vertex_id,
                "look up SLDPRT hash key"
            )?);
            let point = *require_some!(ctx.get_hash_map(
                &(points),
                &vertex.point,
                "look up SLDPRT hash key"
            )?);
            if require_some!(analytic_surface_residual(
                require_some!(surface.solved()),
                point
            )) > tolerance
            {
                return Ok(None);
            }
            let axial = point.vector_from(origin).dot(axis);
            if !axial.is_finite() {
                return Ok(None);
            }
            axial_bounds = Some(match axial_bounds {
                Some((min_axial, max_axial)) => (min_axial.min(axial), max_axial.max(axial)),
                None => (axial, axial),
            });
            let angle = require_some!(cylinder_angle(point, origin, &frame));
            ctx.push_vec(&mut angles, angle, "collect SLDPRT cylindrical trim angles")?;
        }
    }
    let (min_axial, max_axial) = require_some!(axial_bounds);
    let (angular_start, angular_span) = require_some!(circular_interval(ctx, &mut angles)?);
    Ok(
        (max_axial - min_axial > tolerance).then_some(CylindricalTrim {
            origin,
            frame,
            radius,
            min_axial,
            max_axial,
            angular_start,
            angular_span,
        }),
    )
}

fn conical_trim(
    ctx: &DecodeContext<'_>,
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    topology: &TrimTopology<'_>,
) -> Result<Option<ConicalTrim>, cadmpeg_core::CodecError> {
    let TrimTopology {
        loops,
        coedges,
        edges,
        vertices,
        points,
        curves,
        ..
    } = *topology;
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) = surface else {
        return Ok(None);
    };
    let origin = cone_surface.origin().get();
    let frame = *cone_surface.frame();
    let axis = *frame.axis().as_raw();
    let radius = cone_surface.radius().get();
    let ratio = cone_surface.ratio();
    let half_angle = cone_surface.half_angle().get();
    let slope = half_angle.tan();
    if radius <= EPS_DISPLAY_QUANTIZATION || !slope.is_finite() {
        return Ok(None);
    }
    if face.loops.len() != 1 {
        return Ok(None);
    }
    let loop_id = require_some!(face.loops.iter().next());
    let loop_ = *require_some!(ctx.get_hash_map(&(loops), loop_id, "look up SLDPRT hash key")?);
    if loop_.coedges().is_empty()
        || loop_.vertices().next().is_some()
        || !ctx.equal(
            &loop_.face,
            &face.id,
            "compare SLDPRT trim topology identities",
        )?
    {
        return Ok(None);
    }
    let tolerance = require_some!(FaceEvaluationTolerance::of(face)).get();
    let mut axial_bounds = None::<(f64, f64)>;
    let mut angles = Vec::new();
    for coedge_id in loop_.coedges() {
        ctx.charge_work(1, "scan SLDPRT conical trim coedges")?;
        let coedge =
            *require_some!(ctx.get_hash_map(&(coedges), coedge_id, "look up SLDPRT hash key")?);
        if !ctx.equal(
            &coedge.owner_loop,
            &loop_.id,
            "compare SLDPRT trim topology identities",
        )? {
            return Ok(None);
        }
        let edge =
            *require_some!(ctx.get_hash_map(&(edges), &coedge.edge, "look up SLDPRT hash key")?);
        let curve = require_some!(ctx.get_hash_map(
            &(curves),
            require_some!(edge.curve()),
            "look up SLDPRT hash key"
        )?);
        match curve {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let direction = *line_curve.direction().as_raw();
                if direction.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT {
                    return Ok(None);
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                // Two poles, so the pole test is fixed work.
                if nurbs.degree() != 1
                    || nurbs.periodic()
                    || nurbs.pole_rows().count() != 2
                    || nurbs.control_points().iter().any(|point| {
                        surface.solved().is_none_or(|surface| {
                            analytic_surface_residual(surface, point.get())
                                .is_none_or(|residual| residual > tolerance)
                        })
                    })
                {
                    return Ok(None);
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center().get();
                let curve_axis = ellipse_curve.frame().axis().as_raw();
                let major_direction = *ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                let (reference, transverse) = perpendicular_basis(&frame);
                let center_delta = center.vector_from(origin);
                let center_axial = center_delta.dot(axis);
                let center_radial = center_delta - axis.scale(center_axial);
                let expected_radius = (radius + center_axial * slope).abs();
                let reference_aligned = major_direction.dot(reference).abs();
                let transverse_aligned = major_direction.dot(transverse).abs();
                let aligned_radii = if reference_aligned >= 1.0 - EPS_AXIS_ALIGNMENT {
                    (major_radius - expected_radius).abs() <= tolerance
                        && (minor_radius - expected_radius * ratio.get()).abs() <= tolerance
                } else if transverse_aligned >= 1.0 - EPS_AXIS_ALIGNMENT {
                    (major_radius - expected_radius * ratio.get()).abs() <= tolerance
                        && (minor_radius - expected_radius).abs() <= tolerance
                } else {
                    false
                };
                if curve_axis.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || major_radius <= tolerance
                    || minor_radius <= tolerance
                    || center_radial.norm() > tolerance
                    || !aligned_radii
                {
                    return Ok(None);
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let curve_axis = circle_curve.frame().axis().as_raw();
                let curve_radius = circle_curve.radius().get();
                if (ratio.get() - 1.0).abs() > EPS_AXIS_ALIGNMENT
                    || curve_axis.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                {
                    return Ok(None);
                }
                let center_delta = center.vector_from(origin);
                let center_axial = center_delta.dot(axis);
                let center_radial = center_delta - axis.scale(center_axial);
                let expected_radius = (radius + center_axial * slope).abs();
                if center_radial.norm() > tolerance
                    || (curve_radius - expected_radius).abs() > tolerance
                {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
        for vertex_id in [&edge.start, &edge.end] {
            ctx.charge_work(1, "scan SLDPRT conical trim endpoints")?;
            let vertex = require_some!(ctx.get_hash_map(
                &(vertices),
                vertex_id,
                "look up SLDPRT hash key"
            )?);
            let point = *require_some!(ctx.get_hash_map(
                &(points),
                &vertex.point,
                "look up SLDPRT hash key"
            )?);
            if require_some!(analytic_surface_residual(
                require_some!(surface.solved()),
                point
            )) > tolerance
            {
                return Ok(None);
            }
            let axial = point.vector_from(origin).dot(axis);
            let angle = require_some!(cone_angle(point, origin, &frame, ratio));
            if !axial.is_finite() || !angle.is_finite() {
                return Ok(None);
            }
            axial_bounds = Some(match axial_bounds {
                Some((min_axial, max_axial)) => (min_axial.min(axial), max_axial.max(axial)),
                None => (axial, axial),
            });
            ctx.reserve_vec(&mut angles, 1, "collect SLDPRT conical trim angles")?;
            angles.push(angle);
        }
    }
    let (min_axial, max_axial) = require_some!(axial_bounds);
    let (angular_start, angular_span) = require_some!(circular_interval(ctx, &mut angles)?);
    Ok((max_axial - min_axial > tolerance).then_some(ConicalTrim {
        origin,
        frame,
        radius,
        ratio,
        slope,
        min_axial,
        max_axial,
        angular_start,
        angular_span,
    }))
}

/// The reference of `frame` minus its component along the axis, and the axis
/// cross that projection, each divided by its length.
///
/// The frame holds unit directions perpendicular within `1e-9`, so each length
/// is within about `2e-9` of one, and the two results are unit length and
/// perpendicular to the axis and to each other to rounding.
fn perpendicular_basis(frame: &OrthonormalFrame3) -> (Vector3, Vector3) {
    let divided = |vector: Vector3| {
        let length = vector.norm();
        Vector3::new(vector.x / length, vector.y / length, vector.z / length)
    };
    let axis = *frame.axis().as_raw();
    let reference = *frame.reference().as_raw();
    let reference = divided(reference - axis.scale(reference.dot(axis)));
    (reference, divided(axis.cross(reference)))
}

fn cylinder_angle(point: Point3, origin: Point3, frame: &OrthonormalFrame3) -> Option<f64> {
    let (reference, transverse) = perpendicular_basis(frame);
    let delta = point.vector_from(origin);
    let angle = delta.dot(transverse).atan2(delta.dot(reference));
    angle
        .is_finite()
        .then_some(angle.rem_euclid(std::f64::consts::TAU))
}

fn cone_angle(
    point: Point3,
    origin: Point3,
    frame: &OrthonormalFrame3,
    ratio: PositiveReal,
) -> Option<f64> {
    let (reference, transverse) = perpendicular_basis(frame);
    let delta = point.vector_from(origin);
    let major = delta.dot(reference);
    let minor = delta.dot(transverse);
    let angle = (minor / ratio.get()).atan2(major);
    angle
        .is_finite()
        .then_some(angle.rem_euclid(std::f64::consts::TAU))
}

fn circular_interval(
    ctx: &DecodeContext<'_>,
    angles: &mut Vec<f64>,
) -> Result<Option<(f64, f64)>, cadmpeg_core::CodecError> {
    ctx.retain_vec(
        angles,
        |angle| Ok(angle.is_finite()),
        "select SLDPRT circular trim angles",
    )?;
    if angles.is_empty() {
        return Ok(None);
    }
    ctx.stable_sort_by(
        angles,
        |value| value,
        f64::total_cmp,
        "sort SLDPRT circular trim angles",
    )?;
    ctx.dedup_by(
        angles,
        |left, right| Ok((*left - *right).abs() <= EPS_CYLINDER_ANGLE),
        "merge SLDPRT circular trim angles",
    )?;
    if angles.len() == 1 {
        return Ok(Some((angles[0], std::f64::consts::TAU)));
    }
    let Some((largest_gap_index, largest_gap)) = ctx
        .admit_iter(0..angles.len(), "measure SLDPRT circular trim gaps")?
        .map(|index| {
            let next = angles[(index + 1) % angles.len()];
            let gap = if index + 1 == angles.len() {
                next + std::f64::consts::TAU - angles[index]
            } else {
                next - angles[index]
            };
            (index, gap)
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
    else {
        return Ok(None);
    };
    let start = angles[(largest_gap_index + 1) % angles.len()];
    Ok(Some((start, std::f64::consts::TAU - largest_gap)))
}

fn circular_interval_contains(start: f64, span: f64, angle: f64, tolerance: f64) -> bool {
    let distance = (angle - start).rem_euclid(std::f64::consts::TAU);
    // Quantized display-list angles can fall just before the start boundary.
    // The wrapped distance is then near TAU, rather than near zero.
    distance <= span + tolerance || distance + tolerance >= std::f64::consts::TAU
}

// The trim grammars share the same indexed topology maps.
fn analytic_trim(
    ctx: &DecodeContext<'_>,
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    topology: &TrimTopology<'_>,
) -> Result<Option<AnalyticTrim>, cadmpeg_core::CodecError> {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
            let trim = planar_trim(ctx, face, surface, topology)?;
            let trim = match trim {
                Some(trim) => Some(trim),
                None => planar_hole_trim(ctx, face, surface, topology)?,
            };
            Ok(trim.map(AnalyticTrim::Planar))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
            cylindrical_trim(ctx, face, surface, topology)
                .map(|trim| trim.map(AnalyticTrim::Cylindrical))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
            conical_trim(ctx, face, surface, topology).map(|trim| trim.map(AnalyticTrim::Conical))
        }
        _ => Ok(None),
    }
}

fn plane_frame(surface: &SolvedSurfaceGeometry) -> Option<PlaneFrame> {
    let (origin, normal, u_axis) = match surface {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            (origin, *normal, *u_axis)
        }
        SolvedSurfaceGeometry::Transformed(placed) if placed.transform().is_proper_rigid() => {
            let basis = plane_frame(placed.basis())?;
            let transform = placed.transform();
            (
                transform.apply_point(basis.origin.get())?,
                transform.apply_vector(basis.normal)?.get(),
                transform.apply_vector(basis.u_axis)?.get(),
            )
        }
        _ => return None,
    };
    PlaneFrame::new(origin, normal, u_axis)
}

fn point_distance(left: Point2, right: Point2) -> f64 {
    (left.u - right.u).hypot(left.v - right.v)
}

fn signed_area_twice(left: Point2, middle: Point2, right: Point2) -> f64 {
    (middle.u - left.u) * (right.v - middle.v) - (middle.v - left.v) * (right.u - middle.u)
}

/// Twice the signed area of a simple polygon: at least three vertices, an
/// area above the squared tolerance, and no two non-adjacent edges that
/// meet. The pair search charges each pair it tests.
fn simple_polygon_area_twice(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    tolerance: f64,
) -> Result<Option<cadmpeg_ir::scalar::FiniteReal>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "test SLDPRT polygon simplicity";
    let count = polygon.len();
    if count < 3 {
        return Ok(None);
    }
    // The area sum visits each vertex once.
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(count), OPERATION)?;
    let Some(area) = cadmpeg_ir::math::planar::polygon_area_twice(polygon) else {
        return Ok(None);
    };
    if area.get().abs() <= tolerance * tolerance {
        return Ok(None);
    }
    let edge = |index: usize| (polygon[index], polygon[(index + 1) % count]);
    let crossed = ctx.any_by(
        0..count,
        |left| {
            ctx.any_by(
                left + 1..count,
                |right| {
                    if right == left + 1 || (left == 0 && right + 1 == count) {
                        return Ok(false);
                    }
                    let ((a, b), (c, d)) = (edge(left), edge(right));
                    Ok(segments_intersect(a, b, c, d, tolerance))
                },
                OPERATION,
            )
        },
        OPERATION,
    )?;
    Ok((!crossed).then_some(area))
}

/// Ear-clip a simple polygon whose doubled signed area has sign
/// `orientation`.
fn triangulate_polygon(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    orientation: f64,
    tolerance: f64,
) -> Result<Option<Vec<[Point2; 3]>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "triangulate SLDPRT planar polygon";
    let mut scratch = ctx.reserve_scoped(0, "collect SLDPRT planar polygon vertices")?;
    let mut remaining = scratch.with_storage(|| {
        ctx.collection_vec(polygon.len(), "collect SLDPRT planar polygon vertices")
    })?;
    remaining.extend(ctx.admit_iter(0..polygon.len(), "collect SLDPRT planar polygon vertices")?);
    let mut triangles = Vec::new();
    while remaining.len() > 3 {
        let count = remaining.len();
        let position = require_some!(ctx.position_by(
            0..count,
            |position| is_ear(ctx, polygon, &remaining, position, orientation, tolerance),
            OPERATION,
        )?);
        let previous_position = (position + count - 1) % count;
        let next_position = (position + 1) % count;
        ctx.push_vec(
            &mut triangles,
            [
                polygon[remaining[previous_position]],
                polygon[remaining[position]],
                polygon[remaining[next_position]],
            ],
            "collect SLDPRT planar polygon triangles",
        )?;
        ctx.copy_within(
            &mut remaining[position..],
            1..count - position,
            0,
            OPERATION,
        )?;
        ctx.truncate_vec(&mut remaining, count - 1, OPERATION)?;
    }
    let [first, second, third] = remaining.as_slice() else {
        return Ok(None);
    };
    let [first, second, third] = [polygon[*first], polygon[*second], polygon[*third]];
    let scale = point_distance(first, second)
        .max(point_distance(second, third))
        .max(point_distance(third, first));
    if signed_area_twice(first, second, third)
        .abs()
        .partial_cmp(&(tolerance * scale))
        != Some(std::cmp::Ordering::Greater)
    {
        return Ok(None);
    }
    ctx.push_vec(
        &mut triangles,
        [first, second, third],
        "collect SLDPRT planar polygon triangles",
    )?;
    Ok(Some(triangles))
}

/// Whether the vertex at `position` of the remaining polygon is an ear: a
/// convex corner whose diagonal crosses no other edge and whose triangle holds
/// no other remaining vertex.
fn is_ear(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    remaining: &[usize],
    position: usize,
    orientation: f64,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    let count = remaining.len();
    let previous_position = (position + count - 1) % count;
    let next_position = (position + 1) % count;
    let previous = polygon[remaining[previous_position]];
    let current = polygon[remaining[position]];
    let next = polygon[remaining[next_position]];
    let scale = point_distance(previous, current).max(point_distance(current, next));
    let cross = signed_area_twice(previous, current, next);
    if !cross.is_finite() || orientation * cross <= tolerance * scale {
        return Ok(false);
    }
    let corner =
        |other: usize| other == previous_position || other == position || other == next_position;
    if ctx.any_by(
        0..count,
        |edge_position| {
            let edge_next_position = (edge_position + 1) % count;
            Ok(!corner(edge_position)
                && !corner(edge_next_position)
                && segments_intersect(
                    previous,
                    next,
                    polygon[remaining[edge_position]],
                    polygon[remaining[edge_next_position]],
                    tolerance,
                ))
        },
        "test SLDPRT polygon ear diagonal",
    )? {
        return Ok(false);
    }
    let triangle = [previous, current, next];
    Ok(!ctx.any_by(
        remaining.iter().enumerate(),
        |(other_position, index)| {
            Ok(!corner(other_position)
                && triangle_strictly_contains(&triangle, polygon[*index], tolerance))
        },
        "test SLDPRT polygon ear interior",
    )?)
}

/// Whether the edge from vertex `index` to the next one passes within
/// `tolerance` of `point`. Fixed work.
fn edge_near(polygon: &[Point2], index: usize, point: Point2, tolerance: f64) -> bool {
    // A point or edge end that is not finite has no distance to measure.
    match [point, polygon[index], polygon[(index + 1) % polygon.len()]].map(FinitePoint2::new) {
        [Some(point), Some(start), Some(end)] => {
            point_segment_distance(point, start, end) <= tolerance
        }
        _ => false,
    }
}

/// Whether the edge from vertex `index` to the next one crosses the ray from
/// `point` toward increasing `u`. Fixed work.
fn edge_crosses_ray(polygon: &[Point2], index: usize, point: Point2) -> bool {
    let start = polygon[index];
    let end = polygon[(index + 1) % polygon.len()];
    (start.v > point.v) != (end.v > point.v)
        && start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v) > point.u
}

/// Whether `point` lies inside `polygon` by the crossing rule.
fn crossing_parity(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    point: Point2,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(0..polygon.len(), "scan SLDPRT planar polygon boundary")?
        .filter(|index| edge_crosses_ray(polygon, *index, point))
        .count()
        % 2
        == 1)
}

/// Whether some edge of `polygon` passes within `tolerance` of `point`.
fn near_boundary(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    point: Point2,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.any_by(
        0..polygon.len(),
        |index| Ok(edge_near(polygon, index, point, tolerance)),
        "scan SLDPRT planar polygon boundary",
    )
}

fn polygon_contains(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    point: Point2,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(near_boundary(ctx, polygon, point, tolerance)? || crossing_parity(ctx, polygon, point)?)
}

fn polygon_strictly_contains(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    point: Point2,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(!near_boundary(ctx, polygon, point, tolerance)? && crossing_parity(ctx, polygon, point)?)
}

/// `polygon_strictly_contains` for a triangle, which is fixed work.
fn triangle_strictly_contains(triangle: &[Point2; 3], point: Point2, tolerance: f64) -> bool {
    !(0..3).any(|index| edge_near(triangle, index, point, tolerance))
        && (0..3)
            .filter(|index| edge_crosses_ray(triangle, *index, point))
            .count()
            % 2
            == 1
}

/// Checks each triangle edge interval between polygon boundary intersections.
/// A simple polygon that contains the complete triangle boundary contains its
/// interior; `boundary` is simple.
fn polygon_contains_triangle(
    ctx: &DecodeContext<'_>,
    boundary: &[Point2],
    triangle: [Point2; 3],
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    if triangle
        .iter()
        .any(|point| FinitePoint2::new(*point).is_none())
    {
        return Ok(false);
    }
    let capacity = boundary
        .len()
        .checked_mul(2)
        .and_then(|count| count.checked_add(2))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("test SLDPRT planar outer triangle", u64::MAX, u64::MAX)
        })?;
    for edge in 0..3 {
        let start = triangle[edge];
        let end = triangle[(edge + 1) % 3];
        let du = end.u - start.u;
        let dv = end.v - start.v;
        let length_squared = du * du + dv * dv;
        if !length_squared.is_finite() {
            return Ok(false);
        }
        if length_squared == 0.0 {
            continue;
        }
        let (mut cuts, mut reservation) =
            ctx.scoped_vector_storage(capacity, "collect SLDPRT triangle boundary cuts")?;
        ctx.charge_collection_items(2, "collect SLDPRT triangle boundary cuts")?;
        cuts.extend([0.0_f64, 1.0]);
        for (index, point) in ctx
            .admit_iter(boundary, "intersect SLDPRT planar outer triangle")?
            .enumerate()
        {
            let next = boundary[(index + 1) % boundary.len()];
            let pu = point.u - start.u;
            let pv = point.v - start.v;
            let projected = (pu * du + pv * dv) / length_squared;
            if !projected.is_finite() {
                return Ok(false);
            }
            if (0.0..=1.0).contains(&projected) {
                reservation.with_storage(|| {
                    ctx.push_vec(
                        &mut cuts,
                        projected,
                        "collect SLDPRT triangle boundary cuts",
                    )
                })?;
            }
            let eu = next.u - point.u;
            let ev = next.v - point.v;
            let denominator = du * ev - dv * eu;
            if !denominator.is_finite() {
                return Ok(false);
            }
            if denominator == 0.0 {
                continue;
            }
            let along_triangle = (pu * ev - pv * eu) / denominator;
            let along_boundary = (pu * dv - pv * du) / denominator;
            if !along_triangle.is_finite() || !along_boundary.is_finite() {
                return Ok(false);
            }
            if (0.0..=1.0).contains(&along_triangle) && (0.0..=1.0).contains(&along_boundary) {
                reservation.with_storage(|| {
                    ctx.push_vec(
                        &mut cuts,
                        along_triangle,
                        "collect SLDPRT triangle boundary cuts",
                    )
                })?;
            }
        }
        ctx.stable_sort_by(
            &mut cuts,
            |value| value,
            f64::total_cmp,
            "sort SLDPRT triangle boundary cuts",
        )?;
        if !ctx.all_by(
            cuts.windows(2),
            |interval| {
                let parameter = interval[0] + (interval[1] - interval[0]) / 2.0;
                polygon_contains(
                    ctx,
                    boundary,
                    Point2::new(start.u + parameter * du, start.v + parameter * dv),
                    tolerance,
                )
            },
            "scan SLDPRT triangle boundary cuts",
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether simple polygon `inner` lies inside simple polygon `outer`: every
/// inner vertex inside or on `outer`, and no edge pair meets.
fn polygon_inside_polygon(
    ctx: &DecodeContext<'_>,
    inner: &[Point2],
    outer: &[Point2],
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(ctx.all_by(
        inner,
        |point| polygon_contains(ctx, outer, *point, tolerance),
        "scan SLDPRT planar inner boundary",
    )? && !polygons_cross(ctx, inner, outer, tolerance)?)
}

/// Whether some edge of `first` meets some edge of `second`.
fn polygons_cross(
    ctx: &DecodeContext<'_>,
    first: &[Point2],
    second: &[Point2],
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.any_by(
        0..first.len(),
        |left| {
            ctx.any_by(
                0..second.len(),
                |right| {
                    Ok(segments_intersect(
                        first[left],
                        first[(left + 1) % first.len()],
                        second[right],
                        second[(right + 1) % second.len()],
                        tolerance,
                    ))
                },
                "scan SLDPRT planar second boundary",
            )
        },
        "scan SLDPRT planar first boundary",
    )
}

fn polygons_overlap(
    ctx: &DecodeContext<'_>,
    first: &[Point2],
    second: &[Point2],
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(polygons_cross(ctx, first, second, tolerance)?
        || ctx.any_by(
            first,
            |point| polygon_strictly_contains(ctx, second, *point, tolerance),
            "scan SLDPRT planar first boundary",
        )?
        || ctx.any_by(
            second,
            |point| polygon_strictly_contains(ctx, first, *point, tolerance),
            "scan SLDPRT planar second boundary",
        )?)
}

fn triangles_have_positive_overlap(
    first: [Point2; 3],
    second: [Point2; 3],
    tolerance: f64,
) -> bool {
    for triangle in [first, second] {
        for index in 0..triangle.len() {
            let start = triangle[index];
            let end = triangle[(index + 1) % triangle.len()];
            let length = point_distance(start, end);
            if !length.is_finite() || length <= f64::EPSILON {
                return false;
            }
            let axis = Point2::new((start.v - end.v) / length, (end.u - start.u) / length);
            let first_projection = triangle_projection(first, axis);
            let second_projection = triangle_projection(second, axis);
            let overlap = first_projection.1.min(second_projection.1)
                - first_projection.0.max(second_projection.0);
            if !overlap.is_finite() || overlap <= tolerance {
                return false;
            }
        }
    }
    true
}

fn triangle_projection(triangle: [Point2; 3], axis: Point2) -> (f64, f64) {
    triangle
        .into_iter()
        .map(|point| point.u * axis.u + point.v * axis.v)
        .fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
        )
}

fn convex_polygon_contains(polygon: &[Point2], point: Point2, tolerance: f64) -> bool {
    let mut sign = 0.0_f64;
    for index in 0..polygon.len() {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        let cross = signed_area_twice(start, end, point);
        if edge_near(polygon, index, point, tolerance) {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn circle_inside_polygon(
    ctx: &DecodeContext<'_>,
    polygon: &[Point2],
    hole: CircularHole,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(polygon_contains(ctx, polygon, hole.center, tolerance)?
        && ctx.all_by(
            0..polygon.len(),
            |index| {
                Ok(
                    match [
                        hole.center,
                        polygon[index],
                        polygon[(index + 1) % polygon.len()],
                    ]
                    .map(FinitePoint2::new)
                    {
                        [Some(point), Some(start), Some(end)] => {
                            point_segment_distance(point, start, end) >= hole.radius - tolerance
                        }
                        _ => false,
                    },
                )
            },
            "scan SLDPRT planar polygon boundary",
        )?)
}

fn circle_inside_circle(outer: CircularHole, inner: CircularHole, tolerance: f64) -> bool {
    point_distance(outer.center, inner.center) + inner.radius <= outer.radius + tolerance
}

fn circular_outer_and_holes(
    ctx: &DecodeContext<'_>,
    circles: &[CircularHole],
    tolerance: f64,
) -> Result<Option<(CircularHole, Vec<CircularHole>)>, cadmpeg_core::CodecError> {
    let mut outer_index = None;
    for (index, outer) in ctx
        .admit_iter(circles, "compare SLDPRT circular trim boundaries")?
        .enumerate()
    {
        if ctx.all_by(
            circles.iter().enumerate(),
            |(inner_index, inner)| {
                Ok(index == inner_index || circle_inside_circle(*outer, *inner, tolerance))
            },
            "scan SLDPRT contained circular holes",
        )? && outer_index.replace(index).is_some()
        {
            return Ok(None);
        }
    }
    let Some(outer_index) = outer_index else {
        return Ok(None);
    };
    let mut holes = Vec::new();
    for (index, hole) in ctx
        .admit_iter(circles, "compare SLDPRT circular trim boundaries")?
        .enumerate()
    {
        if index != outer_index {
            ctx.push_vec(&mut holes, *hole, "collect SLDPRT circular trim holes")?;
        }
    }
    if ctx.any_by(
        0..holes.len(),
        |left| {
            ctx.any_by(
                &holes[left + 1..],
                |right| {
                    Ok(point_distance(holes[left].center, right.center)
                        < holes[left].radius + right.radius - tolerance)
                },
                "scan SLDPRT subsequent circular trim holes",
            )
        },
        "scan SLDPRT circular trim hole pairs",
    )? {
        return Ok(None);
    }
    Ok(Some((circles[outer_index], holes)))
}

fn chordal_hole_constraint(
    ctx: &DecodeContext<'_>,
    hole: CircularHole,
    points: &[Point2],
    tolerance: f64,
) -> Result<Option<(CircularHole, CircularHole)>, cadmpeg_core::CodecError> {
    let mut minimum = None::<f64>;
    for point in ctx.admit_iter(points, "measure SLDPRT circular trim point")? {
        let distance = point_distance(*point, hole.center);
        minimum = Some(minimum.map_or(distance, |current| current.min(distance)));
    }
    let Some(minimum) = minimum else {
        return Ok(None);
    };
    if minimum >= hole.radius - tolerance {
        return Ok(Some((hole, hole)));
    }

    let mut scratch = ctx.reserve_scoped(0, "collect SLDPRT circular trim angles")?;
    let mut boundary_angles = Vec::new();
    for point in ctx.admit_iter(points, "scan SLDPRT circular trim boundary")? {
        let distance = point_distance(*point, hole.center);
        if (distance - hole.radius).abs() <= tolerance {
            ctx.push_scoped_vec(
                &mut scratch,
                &mut boundary_angles,
                (point.v - hole.center.v).atan2(point.u - hole.center.u),
                "collect SLDPRT circular trim angles",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut boundary_angles,
        |value| value,
        f64::total_cmp,
        "sort SLDPRT circular trim angles",
    )?;
    ctx.dedup_by(
        &mut boundary_angles,
        |left, right| Ok((*left - *right).abs() <= tolerance / hole.radius),
        "merge SLDPRT circular trim angles",
    )?;
    if boundary_angles.len() < 3 {
        return Ok(None);
    }
    let Some(last) = boundary_angles.last() else {
        return Ok(None);
    };
    let wrap_gap = boundary_angles[0] + std::f64::consts::TAU - *last;
    let maximum_gap = ctx
        .admit_iter(
            &boundary_angles,
            "scan SLDPRT adjacent circular trim angles",
        )?
        .windows(const { crate::nonzero(2) })
        .map(|pair| pair[1] - pair[0])
        .chain(std::iter::once(wrap_gap))
        .reduce(f64::max);
    let Some(maximum_gap) = maximum_gap else {
        return Ok(None);
    };
    if maximum_gap > std::f64::consts::PI {
        return Ok(None);
    }
    let maximum_sagitta = hole.radius * (1.0 - (maximum_gap / 2.0).cos());
    let inward_deflection = hole.radius - minimum;
    if inward_deflection > maximum_sagitta + tolerance {
        return Ok(None);
    }
    Ok(Some((
        CircularHole {
            radius: hole.radius - maximum_sagitta,
            ..hole
        },
        hole,
    )))
}

fn triangle_crosses_hole(
    triangle: [Point2; 3],
    exclusion: CircularHole,
    boundary: CircularHole,
    tolerance: f64,
) -> bool {
    if convex_polygon_contains(&triangle, exclusion.center, tolerance) {
        return true;
    }
    (0..3).any(|index| {
        let start = triangle[index];
        let end = triangle[(index + 1) % 3];
        let near = match [exclusion.center, start, end].map(FinitePoint2::new) {
            [Some(point), Some(start), Some(end)] => {
                point_segment_distance(point, start, end) < exclusion.radius - tolerance
            }
            _ => false,
        };
        near && ![start, end].iter().all(|point| {
            (point_distance(*point, boundary.center) - boundary.radius).abs() <= tolerance
        })
    })
}

fn circle_overlaps_polygon(
    ctx: &DecodeContext<'_>,
    circle: CircularHole,
    polygon: &[Point2],
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(
        polygon_strictly_contains(ctx, polygon, circle.center, tolerance)?
            || ctx.any_by(
                polygon,
                |point| Ok(point_distance(*point, circle.center) < circle.radius + tolerance),
                "scan SLDPRT planar polygon boundary",
            )?
            || ctx.any_by(
                0..polygon.len(),
                |index| {
                    Ok(
                        match [
                            circle.center,
                            polygon[index],
                            polygon[(index + 1) % polygon.len()],
                        ]
                        .map(FinitePoint2::new)
                        {
                            [Some(point), Some(start), Some(end)] => {
                                point_segment_distance(point, start, end)
                                    < circle.radius + tolerance
                            }
                            _ => false,
                        },
                    )
                },
                "scan SLDPRT planar polygon boundary",
            )?,
    )
}

fn analytic_surface_normal(surface: &SolvedSurfaceGeometry, point: Point3) -> Option<Vector3> {
    let subtract = |left: Point3, right: Point3| {
        Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
    };
    match surface {
        SolvedSurfaceGeometry::Plane(plane_surface) => Some(*plane_surface.frame().axis().as_raw()),
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = *cylinder_surface.frame().axis().as_raw();
            let delta = subtract(point, origin);
            let radial = delta - axis.scale(delta.dot(axis));
            radial.unit()
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            subtract(point, center).unit()
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = *torus_surface.frame().axis().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let delta = subtract(point, center);
            let axial = delta.dot(axis);
            let radial = delta - axis.scale(axial);
            let radial_unit = radial.unit()?;
            (radial_unit.scale(radial.norm() - major_radius) + axis.scale(axial)).unit()
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = *cone_surface.frame().axis().as_raw();
            let (reference, transverse) = perpendicular_basis(cone_surface.frame());
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let slope = half_angle.tan();
            if !slope.is_finite() {
                return None;
            }
            let delta = point.vector_from(origin);
            let axial = delta.dot(axis);
            let major = delta.dot(reference);
            let minor = delta.dot(transverse);
            let elliptical_radius = major.hypot(minor / ratio);
            if elliptical_radius <= f64::EPSILON {
                return None;
            }
            let local_radius = radius + axial * slope;
            (reference.scale(major / elliptical_radius)
                + transverse.scale(minor / (ratio * ratio * elliptical_radius))
                - axis.scale(slope * local_radius.signum()))
            .unit()
        }
        SolvedSurfaceGeometry::Transformed(placed) if placed.transform().is_proper_rigid() => {
            let transform = placed.transform();
            transform
                .apply_vector(analytic_surface_normal(
                    placed.basis(),
                    transform
                        .try_inverse_affine()
                        .ok()?
                        .apply_point(point)?
                        .get(),
                )?)?
                .unit()
        }
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

fn analytic_surface_residual(surface: &SolvedSurfaceGeometry, point: Point3) -> Option<f64> {
    let subtract = |left: Point3, right: Point3| {
        Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
    };
    match surface {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            Some(subtract(point, origin).dot(*normal).abs())
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = *cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            let delta = subtract(point, origin);
            let radial = delta - axis.scale(delta.dot(axis));
            Some((radial.norm() - radius).abs())
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            let radius = sphere_surface.radius().get();
            Some((subtract(point, center).norm() - radius).abs())
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = *torus_surface.frame().axis().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            let delta = subtract(point, center);
            let axial = delta.dot(axis);
            let radial = delta - axis.scale(axial);
            Some(
                (((radial.norm() - major_radius).powi(2) + axial.powi(2)).sqrt() - minor_radius)
                    .abs(),
            )
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = *cone_surface.frame().axis().as_raw();
            let (reference, transverse) = perpendicular_basis(cone_surface.frame());
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let slope = half_angle.tan();
            if !slope.is_finite() {
                return None;
            }
            let delta = subtract(point, origin);
            let axial = delta.dot(axis);
            let major = delta.dot(reference);
            let minor = delta.dot(transverse);
            let local_radius = radius + axial * slope;
            let elliptical_radius = major.hypot(minor / ratio);
            Some((elliptical_radius - local_radius.abs()).abs())
        }
        SolvedSurfaceGeometry::Transformed(placed) if placed.transform().is_proper_rigid() => {
            analytic_surface_residual(
                placed.basis(),
                placed
                    .transform()
                    .try_inverse_affine()
                    .ok()?
                    .apply_point(point)?
                    .get(),
            )
        }
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
    .filter(|residual| residual.is_finite())
}

#[derive(Debug, Clone, Copy)]
struct SurfaceMeasure {
    residual: f64,
    normal: Option<Vector3>,
}

fn surface_measure(
    ctx: &DecodeContext<'_>,
    surface: &SolvedSurfaceGeometry,
    point: Point3,
    fit_tolerance: Option<f64>,
) -> Result<Option<SurfaceMeasure>, cadmpeg_core::CodecError> {
    if let SolvedSurfaceGeometry::Nurbs(nurbs) = surface {
        let Some(tolerance) = fit_tolerance else {
            return Ok(None);
        };
        let Some(parameters) =
            crate::brep::evaluation::nurbs_surface_parameter_near_point(ctx, nurbs, point, None)?
        else {
            return Ok(None);
        };
        let Some(partials) = crate::brep::evaluation::nurbs_surface_partials(
            ctx,
            nurbs,
            parameters.u,
            parameters.v,
        )?
        else {
            return Ok(None);
        };
        let residual = point.distance(partials.point.get());
        if residual > tolerance {
            return Ok(None);
        }
        return Ok(Some(SurfaceMeasure {
            residual,
            normal: partials.du.cross(partials.dv.get()).unit(),
        })
        .filter(|measure| measure.residual.is_finite()));
    }
    if let SolvedSurfaceGeometry::Transformed(placed) = surface {
        let _depth = ctx.enter_nested("measure SLDPRT placed surface")?;
        let transform = placed.transform();
        if transform.is_proper_rigid() {
            let Some(inverse) = transform.try_inverse_affine().ok() else {
                return Ok(None);
            };
            let Some(local_point) = inverse.apply_point(point) else {
                return Ok(None);
            };
            let Some(mut measure) =
                surface_measure(ctx, placed.basis(), local_point.get(), fit_tolerance)?
            else {
                return Ok(None);
            };
            measure.normal = measure
                .normal
                .and_then(|normal| transform.apply_vector(normal)?.unit());
            return Ok(Some(measure));
        }
    }
    let Some(residual) = analytic_surface_residual(surface, point) else {
        return Ok(None);
    };
    Ok(Some(SurfaceMeasure {
        residual,
        normal: analytic_surface_normal(surface, point),
    }))
}

#[cfg(test)]
mod tests;
