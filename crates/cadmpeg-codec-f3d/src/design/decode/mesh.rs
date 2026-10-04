// SPDX-License-Identifier: Apache-2.0
//! Join mesh-geometry containers to their bodies through the Design segment.
//!
//! A mesh body's geometry lives in a `.paramesh` container ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)),
//! and a typed Design graph joins the container, mesh body, owning feature,
//! optional texture resources, and Scene state ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).

use crate::bytes::{lp_ascii_strict, lp_utf16_bounded_charged};
use crate::container::ContainerScan;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::image::neutral_asset_id_charged;
use crate::design::decode::meta::{
    metadata_for_bulk_stream, typed_primary_frames, TypedPrimaryFrame,
};
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::decode::sketch::{
    indexed_record_header_at, native_scope_charged, native_scope_scoped, IndexedRecordOffsets,
};
use crate::design::decode::text::rsplit_once_ascii;
use cadmpeg_core::container::{ContainerEntry, ContainerRole};
use cadmpeg_core::decode::{bounded_len, DecodeContext, ScopedReservation, View};

use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::paramesh_body_wrapper as body_wrapper;
use crate::layout::paramesh_collection_owner_backlink_prefix as collection_owner;
use crate::layout::paramesh_collection_owner_v17 as collection_owner_v17;
use crate::layout::paramesh_entry_name_prefix as entry_name_prefix;
use crate::layout::paramesh_feature_scope_base as feature_scope_base;
use crate::layout::paramesh_feature_scope_prefix as feature_scope;
use crate::layout::paramesh_guid_join_prefix as guid_join;
use crate::layout::paramesh_mesh_body_join_prefix as mesh_body;
use crate::layout::paramesh_mesh_collection_base_prefix as mesh_collection_base;
use crate::layout::paramesh_mesh_collection_prefix as mesh_collection;
use crate::layout::paramesh_scene_node as scene_node;
use crate::layout::paramesh_scene_node_placed as placed_scene_node;
use crate::layout::paramesh_scene_state as scene_state;
use crate::layout::paramesh_texture_filename_prefix as texture_filename;
use crate::layout::paramesh_texture_table_prefix as texture_table;
use crate::paramesh::{decode_mesh_container, MeshContainer};
use crate::records::entity_header::SegmentTypeData;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::mesh::{
    DesignGuidText, DesignMeshBody, DesignMeshCollection, DesignMeshCollectionOwner,
    DesignMeshEntryName, DesignMeshFeature, DesignMeshFixedRecord, DesignMeshGuid,
    DesignMeshPlacement, DesignMeshRecordIdentity, DesignMeshSceneBounds, DesignMeshSceneNode,
    DesignMeshSceneState, DesignMeshScope, DesignMeshTextureFile, DesignMeshTextureResource,
    DesignMeshTextureTable, DesignMeshUuid, MeshAffineTransform,
};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::units::UnitVector3;
use std::collections::{HashMap, HashSet};

const PARAMESH_MODULE: &str = "ParaMesh";
const COMMON_DATA_MODULE: &str = "CommonData";
const DATA_MODEL_MODULE: &str = "DataModel";
const FUSION_MODULE: &str = "Fusion";
const SCENE_MODULE: &str = "Scene";
const MESH_ENTRY_NAME_TYPE_GUID: &str = "A1BAA3F6-4B67-4A0D-BACC-75F38A2230F3";
const MESH_ENTRY_NAME_BASE_TYPE_GUID: &str = "130A0711-4E92-4FCD-AADE-B9C82238BB27";
const MESH_ENTRY_NAME_TYPE_VERSION: u32 = 0;
const MESH_GUID_TYPE_GUID: &str = "A8338A26-5436-433C-8BAC-C3CF024AD595";
const MESH_GUID_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_GUID_TYPE_VERSION: u32 = 1;
const MESH_BODY_TYPE_GUID: &str = "EA90DA22-556C-4C61-89BB-20C2681B7A9D";
const MESH_BODY_BASE_TYPE_GUID: &str = "CB844AB6-240D-4FC9-9C9F-3679DC896D6F";
const MESH_BODY_TYPE_VERSION: u32 = 7;
const MESH_COLLECTION_TYPE_GUID: &str = "443807AD-8025-41A3-8A50-5157579C3D78";
const MESH_COLLECTION_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const MESH_COLLECTION_TYPE_VERSION: u32 = 0;
const MESH_COLLECTION_BASE_BASE_TYPE_GUID: &str = "834C9DEF-4C39-4587-A08D-A5BD1267B7B4";
const MESH_COLLECTION_BASE_TYPE_VERSION: u32 = 0;
const MESH_TEXTURE_TABLE_TYPE_GUID: &str = "6FC173DC-C7E3-402C-A8C0-891A26DADF8D";
const MESH_TEXTURE_TABLE_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_TEXTURE_TABLE_TYPE_VERSION: u32 = 0;
const MESH_WRAPPER_TYPE_GUID: &str = "E5B3F49A-D8D0-4EEF-BC2B-FCDDAEF9745E";
const MESH_WRAPPER_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_WRAPPER_TYPE_VERSION: u32 = 0;
const MESH_FEATURE_SCOPE_TYPE_GUID: &str = "99F6967E-ED35-4222-B906-5CCF0AC70B53";
const MESH_FEATURE_SCOPE_BASE_TYPE_GUID: &str = "2FCB0587-233E-449B-9724-9AAE5AA23647";
const MESH_FEATURE_SCOPE_TYPE_VERSION: u32 = 0;
const MESH_SCOPE_BASE_RECORD_TYPE_GUID: &str = "CB844AB6-240D-4FC9-9C9F-3679DC896D6F";
const MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_SCOPE_BASE_RECORD_TYPE_VERSION: u32 = 1;
const MESH_SCENE_STATE_TYPE_GUID: &str = "F85F2E62-7627-4922-A16D-53E1275D2AAC";
const MESH_SCENE_STATE_BASE_TYPE_GUID: &str = "D0EDEF7C-5879-45A4-9651-900659CC4FDD";
const MESH_SCENE_STATE_TYPE_VERSION: u32 = 0;
const SCENE_NODE_TYPE_GUID: &str = "702B9CD2-537C-429E-8CC4-22BEEEB98C37";
const SCENE_NODE_BASE_TYPE_GUID: &str = "EB7847AF-E60D-4AB0-A736-4AC00C1F1D21";
const SCENE_NODE_TYPE_VERSION: u32 = 1;
const SCENE_AUXILIARY_TYPE_GUID: &str = "2343B7AB-A2E0-4C66-8B74-99A05E4C670B";
const SCENE_AUXILIARY_BASE_TYPE_GUID: &str = "74D7FEF9-44E9-494D-A25D-81EC33D2841E";
const SCENE_AUXILIARY_TYPE_VERSION: u32 = 1;
const MESH_TEXTURE_FILENAME_TYPE_GUID: &str = "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06573";
const MESH_TEXTURE_FILENAME_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_TEXTURE_FILENAME_TYPE_VERSION: u32 = 0;
const MESH_COLLECTION_OWNER_TYPE_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
const MESH_COLLECTION_OWNER_BASE_TYPE_GUID: &str = "42054630-20A0-40E1-B969-CFE9E742F5C9";
const MESH_COLLECTION_OWNER_TYPE_VERSIONS: [u32; 3] = [15, 17, 23];
const MESH_BODY_OWNER_TYPE_GUID: &str = "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA";
const MESH_BODY_OWNER_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const MESH_BODY_OWNER_TYPE_VERSION: u32 = 4;

/// Row-major 4x4 f64 matrix byte length.
#[cfg(test)]
const MATRIX_BYTES: usize = 128;
const SAME_SEGMENT_REFERENCE_BYTES: usize = indexed_header::LEN;
const SCENE_FOOTER_BYTES: usize = scene_state::LEN - scene_state::FOOTER_MARKER;
/// Six `f64` bound coordinates and a trailing marker byte.
const SCENE_BOUNDS_PAYLOAD_BYTES: usize = 49;
/// The bounds payload of a Scene record without bounds: an empty box whose
/// maximum corner is `f64::MIN` and whose minimum corner is `f64::MAX`.
const SCENE_BOUNDS_ABSENT: [u8; SCENE_BOUNDS_PAYLOAD_BYTES] = {
    let maximum = f64::MIN.to_le_bytes();
    let minimum = f64::MAX.to_le_bytes();
    let mut payload = [1; SCENE_BOUNDS_PAYLOAD_BYTES];
    let mut at = 0;
    while at < 48 {
        payload[at] = if at < 24 {
            maximum[at % 8]
        } else {
            minimum[at % 8]
        };
        at += 1;
    }
    payload
};
/// A flag-map entry: a counted 36-byte GUID and a `u32` flag word.
const TEXTURE_FLAG_ENTRY_BYTES: usize = 44;
/// A filename-map entry: a counted 36-byte GUID and a local reference.
const TEXTURE_FILENAME_ENTRY_BYTES: usize = 51;

/// One mesh body's geometry, in model millimetres.
pub(crate) struct MeshBody {
    /// Deterministic native identifier, keyed by the mesh-body record.
    pub(crate) id: String,
    /// Vertex positions in model millimetres.
    pub(crate) vertices: Vec<FinitePoint3>,
    /// Triangle corner indices into `vertices`.
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Source-classified feature edges as ascending vertex-index pairs.
    pub(crate) feature_edges: Vec<[u32; 2]>,
    /// One transformed unit normal per flattened triangle corner.
    pub(crate) corner_normals: Option<Vec<UnitVector3>>,
    /// Source face groups as an ordered partition of triangle ordinals.
    pub(crate) triangle_groups: Vec<crate::paramesh::MeshTriangleGroup>,
    /// One texture-table selector per triangle, when authored.
    pub(crate) texture_ids: Option<Vec<u32>>,
    /// The attribute channels the container's registry declares.
    pub(crate) attributes: Vec<crate::paramesh::MeshAttribute>,
}

impl MeshAffineTransform {
    fn parse(bytes: &[u8], at: usize) -> Option<Self> {
        let mut cells = [0.0; 16];
        for (index, cell) in cells.iter_mut().enumerate() {
            *cell = View::f64_le_at(bytes, at.checked_add(index.checked_mul(8)?)?)?;
        }
        Self::new(cells).ok()
    }

    fn transform_point(self, point: FinitePoint3) -> Result<FinitePoint3, CodecError> {
        let point = point.get();
        let (x, y, z) = (point.x, point.y, point.z);
        let cells = self.cells();
        let point = cadmpeg_ir::math::Point3::new(
            (cells[0] * x + cells[1] * y + cells[2] * z + cells[3])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[4] * x + cells[5] * y + cells[6] * z + cells[7])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[8] * x + cells[9] * y + cells[10] * z + cells[11])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
        );
        FinitePoint3::new(point).ok_or_else(|| {
            CodecError::Malformed("F3D mesh placement produces a non-finite vertex".into())
        })
    }

    /// Transform an oriented surface normal with the cofactor of the linear
    /// map. The determinant sign keeps the normal aligned with the unchanged
    /// triangle tuple under a reflection.
    fn transform_normal(self, normal: UnitVector3) -> Result<UnitVector3, CodecError> {
        let transform = self.transform();
        transform
            .apply_normal(*normal.as_raw())
            .zip(transform.orientation())
            .map(|(normal, orientation)| {
                if orientation.is_sign_negative() {
                    normal.reversed()
                } else {
                    normal
                }
            })
            .ok_or_else(|| CodecError::malformed("F3D mesh placement produces a degenerate normal"))
    }
}

/// The two equal affine maps stored by a mesh-body class record.
fn mesh_body_transform(payload: &[u8]) -> Option<MeshAffineTransform> {
    let first = MeshAffineTransform::parse(payload, mesh_body::FIRST_TRANSFORM)?;
    let second = MeshAffineTransform::parse(payload, mesh_body::SECOND_TRANSFORM)?;
    (first == second).then_some(first)
}

/// Result of decoding and joining one `.paramesh` entry.
pub(crate) enum MeshContainerOutcome {
    /// Geometry and its Design body record were decoded and joined.
    Joined(MeshBody),
    /// The container decoded, but no complete Design join named it.
    Unjoined {
        /// Native archive entry name.
        entry_name: String,
    },
    /// Reading or parsing the container failed independently of other entries.
    Failed {
        /// Native archive entry name.
        entry_name: String,
        /// Exact read or parse failure.
        error: CodecError,
    },
    /// A complete Design mesh body names no archive entry.
    Missing {
        /// Entry basename stored by the Design record.
        entry_name: String,
    },
}

/// Complete mesh decode: per-container outcomes plus typed Design features.
pub(crate) struct MeshDecode {
    pub(crate) outcomes: Vec<MeshContainerOutcome>,
    pub(crate) features: Vec<DesignMeshFeature>,
}

/// A decoded container's geometry, apart from the two UUIDs that join it.
struct MeshGeometry {
    vertices: Vec<FinitePoint3>,
    triangles: Vec<[u32; 3]>,
    feature_edges: Vec<[u32; 2]>,
    corner_normals: Option<Vec<UnitVector3>>,
    triangle_groups: Vec<crate::paramesh::MeshTriangleGroup>,
    texture_ids: Option<Vec<u32>>,
    attributes: Vec<crate::paramesh::MeshAttribute>,
}

/// Split a container into its Fusion UUID, its mesh UUID and its geometry.
fn split_container(container: MeshContainer) -> (String, DesignMeshUuid, MeshGeometry) {
    let MeshContainer {
        fusion_uuid,
        mesh_uuid,
        vertices,
        triangles,
        feature_edges,
        corner_normals,
        triangle_groups,
        texture_ids,
        attributes,
    } = container;
    (
        fusion_uuid,
        mesh_uuid,
        MeshGeometry {
            vertices,
            triangles,
            feature_edges,
            corner_normals,
            triangle_groups,
            texture_ids,
            attributes,
        },
    )
}

/// One same-segment local reference: marker `1`, a little-endian `u64`
/// target, and two zero bytes.
type LocalReference = [u8; SAME_SEGMENT_REFERENCE_BYTES];

/// A counted run of local references, each validated, borrowed from its
/// record. Each target has one byte encoding, so two runs name the same
/// record indices in the same order exactly when their bytes are equal.
#[derive(Clone, Copy, Debug)]
struct LocalReferenceRun<'a>(&'a [LocalReference]);

impl<'a> LocalReferenceRun<'a> {
    fn len(self) -> usize {
        self.0.len()
    }

    fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    fn first(self) -> Option<u32> {
        self.0.first().and_then(local_reference_target)
    }

    fn as_bytes(self) -> &'a [u8] {
        self.0.as_flattened()
    }

    /// Every target in order, after admitting the traversal.
    fn targets(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<impl Iterator<Item = u32> + 'a, CodecError> {
        Ok(ctx
            .admit_iter(self.0, operation)?
            .filter_map(local_reference_target))
    }
}

#[derive(Debug)]
struct MeshEntryNameRecord {
    entry: DesignMeshEntryName,
    guid_record_index: u32,
}

#[derive(Debug)]
struct MeshGuidRecord {
    guid: DesignMeshGuid,
    entry_name_record_index: u32,
}

#[derive(Debug)]
struct MeshBodyRecord {
    placement: DesignMeshPlacement,
    guid_record_index: u32,
    scope_record_index: u32,
    wrapper_record_index: u32,
    owner_record_index: u32,
    scene_node_record_index: u32,
    collection_record_index: u32,
}

#[derive(Debug)]
struct MeshCollectionRecord<'a> {
    collection: DesignMeshCollection,
    texture_table_record_index: u32,
    body_records: LocalReferenceRun<'a>,
    owner_record_index: u32,
}

/// The filename-map entry joined to one flag-map entry.
#[derive(Clone, Copy, Debug)]
struct MeshTextureFilenameEntry {
    ordinal: u32,
    record_index: u32,
}

/// One flag-map entry and, once the filename map is read, the filename-map
/// entry with the same resource GUID.
#[derive(Debug)]
struct MeshTextureEntry {
    ordinal: u32,
    resource_guid: DesignGuidText,
    flags: u32,
    filename: Option<MeshTextureFilenameEntry>,
}

#[derive(Debug)]
struct MeshTextureTableRecord {
    identity: DesignMeshRecordIdentity,
    /// Resources in flag-map order.
    textures: Vec<MeshTextureEntry>,
}

#[derive(Debug)]
struct MeshWrapperRecord {
    identity: DesignMeshFixedRecord<{ u64_from_index(body_wrapper::LEN) }>,
    body_record_index: u32,
}

#[derive(Debug)]
struct MeshSceneNodeRecord {
    node: DesignMeshSceneNode,
    state_record_index: u32,
    auxiliary_record_index: u32,
}

#[derive(Debug)]
struct MeshScopeRecord<'a> {
    scope: DesignMeshScope,
    body_records: LocalReferenceRun<'a>,
}

#[derive(Debug)]
struct MeshCollectionOwnerRecord {
    owner: DesignMeshCollectionOwner,
    collection_record_index: u32,
}

impl MeshBody {
    /// Place one container's geometry through its joined Design body record.
    fn from_geometry(
        ctx: &DecodeContext<'_>,
        entry_name: &str,
        body_byte_offset: u64,
        transform: MeshAffineTransform,
        geometry: MeshGeometry,
    ) -> Result<Self, CodecError> {
        let MeshGeometry {
            vertices,
            triangles,
            feature_edges,
            corner_normals,
            triangle_groups,
            texture_ids,
            attributes,
        } = geometry;
        let mut id = native_scope_charged(ctx, entry_name)?;
        ctx.append_formatted_retained(
            &mut id,
            format_args!(":mesh-body#{body_byte_offset}"),
            "f3d mesh body identifier",
        )?;
        let mut placed_vertices = ctx.vector_storage(vertices.len(), "f3d placed mesh vertices")?;
        for point in ctx.admit_iter(&vertices, "place F3D mesh vertices")? {
            ctx.push_vec(
                &mut placed_vertices,
                transform.transform_point(*point)?,
                "f3d placed mesh vertices",
            )?;
        }
        let placed_normals = if let Some(normals) = corner_normals {
            let mut placed = ctx.vector_storage(normals.len(), "f3d placed mesh normals")?;
            for normal in ctx.admit_iter(&normals, "place F3D mesh corner normals")? {
                ctx.push_vec(
                    &mut placed,
                    transform.transform_normal(*normal)?,
                    "f3d placed mesh normals",
                )?;
            }
            Some(placed)
        } else {
            None
        };
        Ok(Self {
            id,
            vertices: placed_vertices,
            // Placement does not change indexing. Triangle tuples, feature
            // edges, and corner selectors remain in serialized order.
            triangles,
            feature_edges,
            corner_normals: placed_normals,
            triangle_groups,
            texture_ids,
            attributes,
        })
    }
}

fn validate_mesh_registration(
    ctx: &DecodeContext<'_>,
    frame: TypedPrimaryFrame<'_>,
    expected_version: u32,
    expected_base_type_guid: &str,
    expected_module: &str,
    record_kind: &str,
) -> Result<(), CodecError> {
    if frame.design_type.version != expected_version {
        return Err(CodecError::NotImplemented(ctx.format_retained(
            format_args!(
                "F3D Design {record_kind} record version {} is unsupported",
                frame.design_type.version
            ),
            "f3d Design unsupported diagnostic",
        )?));
    }
    if !registered_module_and_base(
        ctx,
        frame.design_type,
        expected_base_type_guid,
        expected_module,
    )? {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!(
                "F3D Design {record_kind} entity {} has incompatible registration metadata",
                frame.entity_id
            ),
        ));
    }
    Ok(())
}

/// Whether a type entry is registered by `module` and derives from
/// `base_type_guid`, compared without case.
fn registered_module_and_base(
    ctx: &DecodeContext<'_>,
    design_type: &SegmentTypeData,
    base_type_guid: &str,
    module: &str,
) -> Result<bool, CodecError> {
    if !ctx.equal_bytes(
        design_type.module.as_bytes(),
        module.as_bytes(),
        "match F3D mesh type module",
    )? {
        return Ok(false);
    }
    let Some(base) = design_type.base_type_guid.value() else {
        return Ok(false);
    };
    ctx.eq_ignore_ascii_case(
        base.as_str(),
        base_type_guid,
        "match F3D mesh base type GUID",
    )
}

fn exact_record_index(
    ctx: &DecodeContext<'_>,
    record: &[u8],
    frame: TypedPrimaryFrame<'_>,
    record_kind: &str,
) -> Result<u32, CodecError> {
    View::u32_le_at(record, 7)
        .filter(|record_index| u64::from(*record_index) == frame.entity_id)
        .ok_or_else(|| {
            crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design {record_kind} entity {} has an invalid record index",
                    frame.entity_id
                ),
            )
        })
}

fn malformed_frame(ctx: &DecodeContext<'_>, record_kind: &str, entity_id: u64) -> CodecError {
    crate::design::text::malformed_design(
        ctx,
        format_args!("F3D Design {record_kind} entity {entity_id} has an invalid primary frame"),
    )
}

fn source_offset(frame_start: usize, relative: usize) -> Option<u64> {
    u64::try_from(frame_start.checked_add(relative)?).ok()
}

fn record_identity(
    ctx: &DecodeContext<'_>,
    record: &[u8],
    frame: TypedPrimaryFrame<'_>,
    record_kind: &str,
) -> Result<DesignMeshRecordIdentity, CodecError> {
    let record_index = exact_record_index(ctx, record, frame, record_kind)?;
    let malformed = || malformed_frame(ctx, record_kind, frame.entity_id);
    let header = indexed_record_header_at(record, 0).ok_or_else(malformed)?;
    let byte_offset = u64::try_from(frame.start).map_err(|_| malformed())?;
    let frame_length = frame
        .end
        .checked_sub(frame.start)
        .and_then(|length| u64::try_from(length).ok())
        .ok_or_else(malformed)?;
    let class_tag = header.retain_class_tag(ctx, "copy F3D mesh indexed class tag")?;
    DesignMeshRecordIdentity::new(class_tag, record_index, byte_offset, frame_length)
        .map_err(|_| malformed())
}

#[derive(Clone, Copy)]
struct MeshRecordType<'a> {
    type_guid: &'a str,
    base_type_guid: &'a str,
    version: u32,
    module: &'a str,
}

/// Whether a type entry is the registration `expected` names.
fn registered_type(
    ctx: &DecodeContext<'_>,
    design_type: &SegmentTypeData,
    expected: MeshRecordType<'_>,
) -> Result<bool, CodecError> {
    Ok(design_type.version == expected.version
        && ctx.eq_ignore_ascii_case(
            design_type.type_guid.as_str(),
            expected.type_guid,
            "match F3D mesh type GUID",
        )?
        && registered_module_and_base(ctx, design_type, expected.base_type_guid, expected.module)?)
}

#[derive(Clone, Copy)]
struct NestedMeshRecordFrame {
    frame_start: usize,
    at: usize,
    end: usize,
    record_index: u32,
}

/// The identity of a record nested at `frame.at` that repeats the outer
/// record index and whose class tag selects the `expected` registration.
fn nested_record_identity(
    ctx: &DecodeContext<'_>,
    record: &[u8],
    frame: NestedMeshRecordFrame,
    meta: &crate::metastream::MetaStream,
    expected: MeshRecordType<'_>,
) -> Result<Option<DesignMeshRecordIdentity>, CodecError> {
    let Some(header) = indexed_record_header_at(record, frame.at) else {
        return Ok(None);
    };
    if header.record_index != frame.record_index {
        return Ok(None);
    }
    let Some(design_type) = header
        .class_code
        .checked_sub(256)
        .and_then(|ordinal| usize::try_from(ordinal).ok())
        .and_then(|ordinal| meta.types.get(ordinal))
    else {
        return Ok(None);
    };
    if !registered_type(ctx, design_type, expected)? {
        return Ok(None);
    }
    let (Some(byte_offset), Some(frame_length)) = (
        source_offset(frame.frame_start, frame.at),
        frame
            .end
            .checked_sub(frame.at)
            .and_then(|length| u64::try_from(length).ok()),
    ) else {
        return Ok(None);
    };
    let class_tag = header.retain_class_tag(ctx, "copy F3D mesh indexed class tag")?;
    Ok(
        DesignMeshRecordIdentity::new(class_tag, frame.record_index, byte_offset, frame_length)
            .ok(),
    )
}

/// The nonzero record index a local reference targets.
fn local_reference_target(reference: &LocalReference) -> Option<u32> {
    if reference[0] != 1 || reference[9] != 0 || reference[10] != 0 {
        return None;
    }
    u32::try_from(View::u64_le_at(reference, 1)?)
        .ok()
        .filter(|target| *target != 0)
}

/// The target of the local reference at `at`.
fn exact_local_record_index(record: &[u8], at: usize) -> Option<u32> {
    local_reference_target(bytes_at::<SAME_SEGMENT_REFERENCE_BYTES>(record, at)?)
}

/// The `u32` entry count at `count_at`, when the record holds that many
/// entries of `entry_bytes` after it, and the offset of the first entry.
fn counted_entries(record: &[u8], count_at: usize, entry_bytes: usize) -> Option<(usize, usize)> {
    let raw_count = View::u32_le_at(record, count_at)?;
    let start = count_at.checked_add(4)?;
    let count = bounded_len(
        u64::from(raw_count),
        entry_bytes,
        record.len().checked_sub(start)?,
    )?;
    Some((count, start))
}

/// The counted local-reference run at `count_at` and the offset after it.
/// The validation stops at the first reference that is not local.
fn counted_local_references<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a [u8],
    count_at: usize,
) -> Result<Option<(LocalReferenceRun<'a>, usize)>, CodecError> {
    let Some((count, start)) = counted_entries(record, count_at, SAME_SEGMENT_REFERENCE_BYTES)
    else {
        return Ok(None);
    };
    let Some(references) = record.get(start..).and_then(|rest| {
        rest.as_chunks::<SAME_SEGMENT_REFERENCE_BYTES>()
            .0
            .get(..count)
    }) else {
        return Ok(None);
    };
    if !ctx.all_by(
        references,
        |reference| Ok(local_reference_target(reference).is_some()),
        "validate F3D mesh local record references",
    )? {
        return Ok(None);
    }
    let run = LocalReferenceRun(references);
    let Some(end) = start.checked_add(run.as_bytes().len()) else {
        return Ok(None);
    };
    Ok(Some((run, end)))
}

fn parse_mesh_entry_name_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshEntryNameRecord, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_ENTRY_NAME_TYPE_VERSION,
        MESH_ENTRY_NAME_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-entry-name",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(ctx, record, frame, "mesh-entry-name")?;
    let malformed = || malformed_frame(ctx, "mesh-entry-name", frame.entity_id);
    if !zeros_at::<10>(record, entry_name_prefix::ZERO_RUN_10) {
        return Err(malformed());
    }
    let guid_record_index =
        exact_local_record_index(record, entry_name_prefix::GUID_RECORD_REFERENCE)
            .ok_or_else(malformed)?;
    let (entry_name, _) = lp_utf16_bounded_charged(
        ctx,
        record,
        entry_name_prefix::LEN,
        1..=1024,
        "f3d Design UTF-16 text",
    )?
    .ok_or_else(malformed)?;
    Ok(MeshEntryNameRecord {
        entry: DesignMeshEntryName::new(identity, entry_name).map_err(|_| malformed())?,
        guid_record_index,
    })
}

/// The Fusion UUID and entry-name backlink of a mesh-GUID record.
fn mesh_guid_fields(record: &[u8]) -> Option<(&str, u32)> {
    if !zeros_at::<21>(record, guid_join::ZERO_RUN_21) {
        return None;
    }
    let (fusion_uuid, end) = lp_ascii_strict(record, guid_join::FUSION_UUID, 36..=36)?;
    if end != guid_join::ENTRY_NAME_BACKLINK || !crate::bytes::is_guid_hyphenated(fusion_uuid) {
        return None;
    }
    let entry_name_record_index = exact_local_record_index(record, guid_join::ENTRY_NAME_BACKLINK)?;
    Some((fusion_uuid, entry_name_record_index))
}

fn parse_mesh_guid_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshGuidRecord, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_GUID_TYPE_VERSION,
        MESH_GUID_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-GUID",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(ctx, record, frame, "mesh-GUID")?;
    let malformed = || malformed_frame(ctx, "mesh-GUID", frame.entity_id);
    let (guid, entry_name_record_index) = mesh_guid_fields(record).ok_or_else(malformed)?;
    let guid = DesignGuidText::try_from(ctx.copy_retained_text(guid, "retain F3D mesh GUID")?)
        .map_err(|_| malformed())?;
    Ok(MeshGuidRecord {
        guid: DesignMeshGuid::new(identity, guid).map_err(|_| malformed())?,
        entry_name_record_index,
    })
}

/// The placement and graph references of a mesh-body record.
fn mesh_body_fields(record: &[u8], identity: DesignMeshRecordIdentity) -> Option<MeshBodyRecord> {
    if !zeros_at::<10>(record, mesh_body::ZERO_RUN_10) {
        return None;
    }
    let placement = DesignMeshPlacement::new(identity, mesh_body_transform(record)?).ok()?;
    let collection_reference_at = record.len().checked_sub(SAME_SEGMENT_REFERENCE_BYTES)?;
    Some(MeshBodyRecord {
        placement,
        scope_record_index: exact_local_record_index(record, mesh_body::FEATURE_SCOPE_REFERENCE)?,
        wrapper_record_index: exact_local_record_index(record, mesh_body::WRAPPER_REFERENCE)?,
        owner_record_index: exact_local_record_index(record, mesh_body::BODY_OWNER_REFERENCE)?,
        guid_record_index: exact_local_record_index(record, mesh_body::CONTAINER_GUID_REFERENCE)?,
        scene_node_record_index: exact_local_record_index(record, mesh_body::SCENE_NODE_REFERENCE)?,
        collection_record_index: exact_local_record_index(record, collection_reference_at)?,
    })
}

fn parse_mesh_body_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshBodyRecord, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_BODY_TYPE_VERSION,
        MESH_BODY_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-body",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(ctx, record, frame, "mesh-body")?;
    mesh_body_fields(record, identity)
        .ok_or_else(|| malformed_frame(ctx, "mesh-body", frame.entity_id))
}

fn parse_mesh_collection_record<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    meta: &crate::metastream::MetaStream,
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshCollectionRecord<'a>, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_COLLECTION_TYPE_VERSION,
        MESH_COLLECTION_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-collection",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(ctx, record, frame, "mesh-collection")?;
    let malformed = || malformed_frame(ctx, "mesh-collection", frame.entity_id);
    let base_at = mesh_collection::LEN;
    let base_count_at = base_at + mesh_collection_base::BODY_COUNT;
    let first_count = View::u32_le_at(record, mesh_collection::BODY_COUNT);
    if !zeros_at::<10>(record, mesh_collection::ZERO_RUN_10)
        || bytes_at::<2>(record, mesh_collection::CONSTANT_01_01) != Some(&[1, 1])
        || !zeros_at::<9>(record, base_at + mesh_collection_base::ZERO_RUN_9)
        || first_count.is_none()
        || first_count != View::u32_le_at(record, base_count_at)
    {
        return Err(malformed());
    }
    let texture_table_record_index =
        exact_local_record_index(record, mesh_collection::TEXTURE_TABLE_REFERENCE)
            .ok_or_else(malformed)?;
    let base_record = nested_record_identity(
        ctx,
        record,
        NestedMeshRecordFrame {
            frame_start: frame.start,
            at: base_at,
            end: record.len(),
            record_index: identity.record_index(),
        },
        meta,
        MeshRecordType {
            type_guid: MESH_COLLECTION_BASE_TYPE_GUID,
            base_type_guid: MESH_COLLECTION_BASE_BASE_TYPE_GUID,
            version: MESH_COLLECTION_BASE_TYPE_VERSION,
            module: COMMON_DATA_MODULE,
        },
    )?
    .ok_or_else(malformed)?;
    let (body_records, owner_at) =
        counted_local_references(ctx, record, base_count_at)?.ok_or_else(malformed)?;
    let owner_record_index = exact_local_record_index(record, owner_at).ok_or_else(malformed)?;
    if owner_at.checked_add(SAME_SEGMENT_REFERENCE_BYTES) != Some(record.len()) {
        return Err(malformed());
    }
    Ok(MeshCollectionRecord {
        collection: DesignMeshCollection::new(identity, base_record).map_err(|_| malformed())?,
        texture_table_record_index,
        body_records,
        owner_record_index,
    })
}

/// The hyphenated resource GUID at `at`, its uppercase key and the offset
/// after it.
fn texture_guid_at(record: &[u8], at: usize) -> Option<(&str, [u8; 36], usize)> {
    let (guid, end) = lp_ascii_strict(record, at, 36..=36)?;
    if !crate::bytes::is_guid_hyphenated(guid) {
        return None;
    }
    let key: [u8; 36] = *guid.as_bytes().first_chunk()?;
    Some((guid, key.map(|byte| byte.to_ascii_uppercase()), end))
}

fn parse_mesh_texture_table_record(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshTextureTableRecord, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_TEXTURE_TABLE_TYPE_VERSION,
        MESH_TEXTURE_TABLE_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-texture-table",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(ctx, record, frame, "mesh-texture-table")?;
    let textures = mesh_texture_entries(ctx, storage, record)?
        .ok_or_else(|| malformed_frame(ctx, "mesh-texture-table", frame.entity_id))?;
    Ok(MeshTextureTableRecord { identity, textures })
}

/// The flag map joined to the filename map by resource GUID, compared
/// without case. Each GUID occurs once in each map, and both maps hold the
/// same GUIDs. The texture storage lives in `storage`.
fn mesh_texture_entries(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    record: &[u8],
) -> Result<Option<Vec<MeshTextureEntry>>, CodecError> {
    if !zeros_at::<10>(record, texture_table::ZERO_RUN_10) {
        return Ok(None);
    }
    let Some((flags_count, mut at)) = counted_entries(
        record,
        texture_table::FLAGS_MAP_COUNT,
        TEXTURE_FLAG_ENTRY_BYTES,
    ) else {
        return Ok(None);
    };
    let mut textures = Vec::new();
    storage.with_storage(|| {
        ctx.reserve_capacity(&mut textures, flags_count, "f3d mesh texture entries")
    })?;
    let mut keys = HashMap::new();
    let mut keys_storage = ctx.reserve_scoped(0, "f3d mesh texture keys")?;
    for ordinal in ctx.admit_iter(&(0..flags_count), "read F3D mesh texture flags")? {
        let Some((resource_guid, key, end)) = texture_guid_at(record, at) else {
            return Ok(None);
        };
        let (Ok(ordinal), Some(flags), Some(next_at)) = (
            u32::try_from(ordinal),
            View::u32_le_at(record, end),
            end.checked_add(4),
        ) else {
            return Ok(None);
        };
        at = next_at;
        let index = textures.len();
        if keys_storage
            .with_storage(|| ctx.insert_hash_map(&mut keys, key, index, "f3d mesh texture keys"))?
            .is_some()
        {
            return Ok(None);
        }
        let resource_guid = DesignGuidText::try_from(
            ctx.copy_retained_text(resource_guid, "retain F3D mesh texture GUID")?,
        )
        .map_err(CodecError::malformed)?;
        ctx.push_vec(
            &mut textures,
            MeshTextureEntry {
                ordinal,
                resource_guid,
                flags,
                filename: None,
            },
            "f3d mesh texture entries",
        )?;
    }
    let Some((filename_count, mut at)) = counted_entries(record, at, TEXTURE_FILENAME_ENTRY_BYTES)
    else {
        return Ok(None);
    };
    // Each filename entry joins a distinct flag entry, so equal counts make
    // the join one-to-one.
    if filename_count != flags_count {
        return Ok(None);
    }
    for ordinal in ctx.admit_iter(&(0..filename_count), "read F3D mesh texture filenames")? {
        let Some((_, key, end)) = texture_guid_at(record, at) else {
            return Ok(None);
        };
        let (Ok(ordinal), Some(record_index), Some(next_at)) = (
            u32::try_from(ordinal),
            exact_local_record_index(record, end),
            end.checked_add(SAME_SEGMENT_REFERENCE_BYTES),
        ) else {
            return Ok(None);
        };
        at = next_at;
        let Some(texture) = ctx
            .get_hash_map(&keys, &key, "f3d mesh texture keys")?
            .and_then(|index| textures.get_mut(*index))
            .filter(|texture| texture.filename.is_none())
        else {
            return Ok(None);
        };
        texture.filename = Some(MeshTextureFilenameEntry {
            ordinal,
            record_index,
        });
    }
    if at != record.len() {
        return Ok(None);
    }
    Ok(Some(textures))
}

fn parse_mesh_wrapper_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshWrapperRecord, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_WRAPPER_TYPE_VERSION,
        MESH_WRAPPER_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-wrapper",
    )?;
    let record = &bytes[frame.start..frame.end];
    let malformed = || malformed_frame(ctx, "mesh-wrapper", frame.entity_id);
    let identity =
        DesignMeshFixedRecord::try_from(record_identity(ctx, record, frame, "mesh-wrapper")?)
            .map_err(|_| malformed())?;
    if !zeros_at::<10>(record, body_wrapper::ZERO_RUN_10)
        || !zeros_at::<8>(record, body_wrapper::ZERO_TAIL_8)
    {
        return Err(malformed());
    }
    let body_record_index =
        exact_local_record_index(record, body_wrapper::BODY_REFERENCE).ok_or_else(malformed)?;
    Ok(MeshWrapperRecord {
        identity,
        body_record_index,
    })
}

enum SceneBoundsPayload {
    Invalid,
    Absent,
    Present(DesignMeshSceneBounds),
}

fn parse_scene_footer(record: &[u8], at: usize) -> SceneBoundsPayload {
    if at.checked_add(SCENE_FOOTER_BYTES) != Some(record.len()) || record.get(at) != Some(&1) {
        return SceneBoundsPayload::Invalid;
    }
    let Some(payload_at) = at.checked_add(1) else {
        return SceneBoundsPayload::Invalid;
    };
    parse_scene_bounds_payload(record, payload_at)
}

/// The bounds payload that ends `record` at `payload_at`: six `f64`
/// coordinates and the marker `1`.
fn parse_scene_bounds_payload(record: &[u8], payload_at: usize) -> SceneBoundsPayload {
    if payload_at.checked_add(SCENE_BOUNDS_PAYLOAD_BYTES) != Some(record.len()) {
        return SceneBoundsPayload::Invalid;
    }
    let Some(payload) = bytes_at::<SCENE_BOUNDS_PAYLOAD_BYTES>(record, payload_at) else {
        return SceneBoundsPayload::Invalid;
    };
    if *payload == SCENE_BOUNDS_ABSENT {
        return SceneBoundsPayload::Absent;
    }
    if payload[48] != 1 {
        return SceneBoundsPayload::Invalid;
    }
    let mut values = [0.0; 6];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, ordinal * 8) else {
            return SceneBoundsPayload::Invalid;
        };
        *value = parsed;
    }
    match DesignMeshSceneBounds::new(
        [values[0], values[1], values[2]],
        [values[3], values[4], values[5]],
    ) {
        Ok(bounds) => SceneBoundsPayload::Present(bounds),
        Err(_) => SceneBoundsPayload::Invalid,
    }
}

fn parse_mesh_scene_state_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<DesignMeshSceneState, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_SCENE_STATE_TYPE_VERSION,
        MESH_SCENE_STATE_BASE_TYPE_GUID,
        SCENE_MODULE,
        "mesh-scene-state",
    )?;
    let record = &bytes[frame.start..frame.end];
    let malformed = || malformed_frame(ctx, "mesh-scene-state", frame.entity_id);
    let identity =
        DesignMeshFixedRecord::try_from(record_identity(ctx, record, frame, "mesh-scene-state")?)
            .map_err(|_| malformed())?;
    if !zeros_at::<34>(record, scene_state::ZERO_RUN_34) {
        return Err(malformed());
    }
    let bounds = match parse_scene_footer(record, scene_state::FOOTER_MARKER) {
        SceneBoundsPayload::Invalid => return Err(malformed()),
        SceneBoundsPayload::Absent => None,
        SceneBoundsPayload::Present(bounds) => Some(bounds),
    };
    Ok(DesignMeshSceneState::new(identity, bounds))
}

/// The bounds, placement and references of a compact or placed Scene node.
fn scene_node_fields(
    record: &[u8],
    identity: DesignMeshRecordIdentity,
) -> Option<MeshSceneNodeRecord> {
    if !zeros_at::<14>(record, scene_node::ZERO_RUN_14)
        || View::u32_le_at(record, scene_node::CONSTANT_TWO_A) != Some(2)
        || View::u32_le_at(record, scene_node::CONSTANT_TWO_B) != Some(2)
        || View::u32_le_at(record, scene_node::CONSTANT_THREE) != Some(3)
    {
        return None;
    }
    let (bounds, transform) =
        if record.len() == scene_node::LEN && zeros_at::<24>(record, scene_node::ZERO_RUN_24) {
            (parse_scene_footer(record, scene_node::FOOTER_MARKER), None)
        } else if record.len() == placed_scene_node::LEN
            && zeros_at::<25>(record, placed_scene_node::ZERO_RUN_25)
        {
            (
                parse_scene_bounds_payload(record, placed_scene_node::FOOTER_MASK),
                Some(MeshAffineTransform::parse(
                    record,
                    placed_scene_node::TRANSFORM,
                )?),
            )
        } else {
            return None;
        };
    let bounds = match bounds {
        SceneBoundsPayload::Invalid => return None,
        SceneBoundsPayload::Absent => None,
        SceneBoundsPayload::Present(bounds) => Some(bounds),
    };
    Some(MeshSceneNodeRecord {
        node: DesignMeshSceneNode::new(identity, bounds, transform).ok()?,
        state_record_index: exact_local_record_index(record, scene_node::SCENE_STATE_REFERENCE)?,
        auxiliary_record_index: exact_local_record_index(
            record,
            scene_node::AUXILIARY_RECORD_REFERENCE,
        )?,
    })
}

fn parse_scene_node_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshSceneNodeRecord, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        SCENE_NODE_TYPE_VERSION,
        SCENE_NODE_BASE_TYPE_GUID,
        SCENE_MODULE,
        "mesh-scene-node",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(ctx, record, frame, "mesh-scene-node")?;
    scene_node_fields(record, identity)
        .ok_or_else(|| malformed_frame(ctx, "mesh-scene-node", frame.entity_id))
}

fn parse_typed_identity(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
    expected_version: u32,
    expected_base_type_guid: &str,
    expected_module: &str,
    record_kind: &str,
) -> Result<DesignMeshRecordIdentity, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        expected_version,
        expected_base_type_guid,
        expected_module,
        record_kind,
    )?;
    record_identity(ctx, &bytes[frame.start..frame.end], frame, record_kind)
}

fn parse_mesh_scope_record<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    meta: &crate::metastream::MetaStream,
    records: &IndexedRecordOffsets,
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshScopeRecord<'a>, CodecError> {
    validate_mesh_registration(
        ctx,
        frame,
        MESH_FEATURE_SCOPE_TYPE_VERSION,
        MESH_FEATURE_SCOPE_BASE_TYPE_GUID,
        FUSION_MODULE,
        "mesh-feature-scope",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(ctx, record, frame, "mesh-feature-scope")?;
    let malformed = || malformed_frame(ctx, "mesh-feature-scope", frame.entity_id);
    if !zeros_at::<10>(record, feature_scope::ZERO_RUN_10) {
        return Err(malformed());
    }
    let (body_records, body_list_end) =
        counted_local_references(ctx, record, feature_scope::BODY_COUNT)?.ok_or_else(malformed)?;
    let frame_offset = u64::try_from(frame.start).map_err(|_| malformed())?;
    let scope = parse_parameter_scope(
        ctx,
        bytes,
        records,
        identity.record_index(),
        identity.class_tag(),
        frame_offset,
    )?
    .ok_or_else(malformed)?;
    if !matches!(scope.payload(), DesignScopePayload::BaseMeshFeature)
        || scope.byte_offset() != frame_offset
    {
        return Err(malformed());
    }
    let paired_relative = usize::try_from(scope.paired_byte_offset())
        .ok()
        .and_then(|paired_at| paired_at.checked_sub(frame.start))
        .ok_or_else(malformed)?;
    if body_list_end > paired_relative
        || paired_relative.checked_add(feature_scope_base::LEN) != Some(record.len())
    {
        return Err(malformed());
    }
    let base_record = nested_record_identity(
        ctx,
        record,
        NestedMeshRecordFrame {
            frame_start: frame.start,
            at: paired_relative,
            end: record.len(),
            record_index: identity.record_index(),
        },
        meta,
        MeshRecordType {
            type_guid: MESH_SCOPE_BASE_RECORD_TYPE_GUID,
            base_type_guid: MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID,
            version: MESH_SCOPE_BASE_RECORD_TYPE_VERSION,
            module: DATA_MODEL_MODULE,
        },
    )?
    .ok_or_else(malformed)?;
    // The base record ends the frame, so these offsets lie inside it.
    if !zeros_at::<8>(record, paired_relative + feature_scope_base::ZERO_RUN_8) {
        return Err(malformed());
    }
    let owner_record_index = exact_local_record_index(
        record,
        paired_relative + feature_scope_base::SCOPE_OWNER_REFERENCE,
    )
    .ok_or_else(malformed)?;
    Ok(MeshScopeRecord {
        scope: DesignMeshScope::new(identity, base_record, owner_record_index)
            .map_err(|_| malformed())?,
        body_records,
    })
}

fn parse_mesh_collection_owner_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<Option<MeshCollectionOwnerRecord>, CodecError> {
    let admitted_version =
        if MESH_COLLECTION_OWNER_TYPE_VERSIONS.contains(&frame.design_type.version) {
            frame.design_type.version
        } else {
            MESH_COLLECTION_OWNER_TYPE_VERSIONS[2]
        };
    let identity = parse_typed_identity(
        ctx,
        bytes,
        frame,
        admitted_version,
        MESH_COLLECTION_OWNER_BASE_TYPE_GUID,
        FUSION_MODULE,
        "mesh-collection-owner",
    )?;
    let record = &bytes[frame.start..frame.end];
    let Some(backlink_at) = (match frame.design_type.version {
        15 => record.len().checked_sub(11),
        17 if record.len() >= collection_owner_v17::LEN => {
            Some(collection_owner_v17::COLLECTION_BACKLINK)
        }
        23 => Some(collection_owner::COLLECTION_BACKLINK),
        _ => None,
    }) else {
        return Ok(None);
    };
    let Some(collection_record_index) = exact_local_record_index(record, backlink_at) else {
        return Ok(None);
    };
    Ok(Some(MeshCollectionOwnerRecord {
        owner: DesignMeshCollectionOwner::new(
            identity,
            source_offset(frame.start, backlink_at)
                .ok_or_else(|| malformed_frame(ctx, "mesh-collection-owner", frame.entity_id))?,
        )
        .map_err(|_| malformed_frame(ctx, "mesh-collection-owner", frame.entity_id))?,
        collection_record_index,
    }))
}

fn parse_mesh_texture_filename_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<(DesignMeshRecordIdentity, String), CodecError> {
    let identity = parse_typed_identity(
        ctx,
        bytes,
        frame,
        MESH_TEXTURE_FILENAME_TYPE_VERSION,
        MESH_TEXTURE_FILENAME_BASE_TYPE_GUID,
        "",
        "mesh-texture-filename",
    )?;
    let record = &bytes[frame.start..frame.end];
    let malformed = || malformed_frame(ctx, "mesh-texture-filename", frame.entity_id);
    if !zeros_at::<10>(record, texture_filename::ZERO_RUN_10) {
        return Err(malformed());
    }
    let (filename, end) = lp_utf16_bounded_charged(
        ctx,
        record,
        texture_filename::BASENAME_CODE_UNIT_COUNT,
        1..=1024,
        "f3d Design UTF-16 text",
    )?
    .ok_or_else(malformed)?;
    if end != record.len() {
        return Err(malformed());
    }
    Ok((identity, filename))
}

/// Index the records that `parse` reads from `frames` by record index; a
/// frame it returns `None` for is not part of the graph. Each record index
/// occurs once. The map storage lives in `storage`.
fn record_map<'a, T>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    frames: &[TypedPrimaryFrame<'a>],
    record_kind: &str,
    mut parse: impl FnMut(TypedPrimaryFrame<'a>) -> Result<Option<T>, CodecError>,
) -> Result<HashMap<u32, T>, CodecError> {
    let mut out = HashMap::new();
    for frame in ctx.admit_iter(frames, "index F3D mesh primary frames")? {
        let Some(record) = parse(*frame)? else {
            continue;
        };
        let index = u32::try_from(frame.entity_id).map_err(|_| {
            crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design {record_kind} entity {} exceeds the indexed-record domain",
                    frame.entity_id
                ),
            )
        })?;
        if storage
            .with_storage(|| ctx.insert_hash_map(&mut out, index, record, "f3d mesh record map"))?
            .is_some()
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!("F3D Design {record_kind} record index {index} is not unique"),
            ));
        }
    }
    Ok(out)
}

fn malformed_mesh_graph(ctx: &DecodeContext<'_>, stream: &str, invariant: &str) -> CodecError {
    crate::design::text::malformed_design(
        ctx,
        format_args!("F3D Design mesh feature graph violates `{invariant}` in {stream}"),
    )
}

fn mesh_graph_diagnostic(
    ctx: &DecodeContext<'_>,
    message: std::fmt::Arguments<'_>,
) -> Result<CodecError, CodecError> {
    Ok(CodecError::Malformed(
        ctx.format_retained(message, "f3d mesh graph diagnostic")?,
    ))
}

/// The record indices of a reference run, in temporary storage.
fn diagnostic_targets<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    run: LocalReferenceRun<'_>,
    operation: &'static str,
) -> Result<(Vec<u32>, ScopedReservation<'ctx>), CodecError> {
    let (mut targets, reservation) = ctx.scoped_vector_storage(run.len(), operation)?;
    for target in run.targets(ctx, operation)? {
        ctx.push_vec(&mut targets, target, operation)?;
    }
    Ok((targets, reservation))
}

fn mesh_collection_indices(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    collections: &[MeshCollectionRecord<'_>],
) -> Result<HashSet<u32>, CodecError> {
    let mut indices = HashSet::new();
    for collection in ctx.admit_iter(collections, "index F3D mesh collections")? {
        let index = collection.collection.record().record_index();
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut indices, index, "f3d mesh collection indices")
        })?;
    }
    Ok(indices)
}

/// The typed records of one Design stream's mesh feature graph, keyed by
/// record index. Joining a collection removes each record it uses.
struct MeshGraph<'a> {
    stream: String,
    entry_names: HashMap<u32, MeshEntryNameRecord>,
    guids: HashMap<u32, MeshGuidRecord>,
    bodies: HashMap<u32, MeshBodyRecord>,
    texture_tables: HashMap<u32, MeshTextureTableRecord>,
    wrappers: HashMap<u32, MeshWrapperRecord>,
    scopes: HashMap<u32, MeshScopeRecord<'a>>,
    states: HashMap<u32, DesignMeshSceneState>,
    scene_nodes: HashMap<u32, MeshSceneNodeRecord>,
    scene_auxiliary_frames: HashMap<u32, TypedPrimaryFrame<'a>>,
    filename_frames: HashMap<u32, TypedPrimaryFrame<'a>>,
    collection_owners: HashMap<u32, MeshCollectionOwnerRecord>,
    body_owner_frames: HashMap<u32, TypedPrimaryFrame<'a>>,
}

const GRAPH_LOOKUP: &str = "join F3D mesh graph records";

impl<'a> MeshGraph<'a> {
    fn error(&self, ctx: &DecodeContext<'_>, invariant: &str) -> CodecError {
        malformed_mesh_graph(ctx, &self.stream, invariant)
    }

    /// The one scope with the collection's ordered body list whose bodies
    /// all link back to that scope and to the collection.
    fn collection_scope(
        &self,
        ctx: &DecodeContext<'_>,
        collection: &MeshCollectionRecord<'a>,
    ) -> Result<u32, CodecError> {
        let collection_index = collection.collection.record().record_index();
        // A matching scope is the scope each of its bodies names, so the
        // first body selects the only candidate.
        let candidate = match collection.body_records.first() {
            Some(first) => ctx
                .get_hash_map(&self.bodies, &first, GRAPH_LOOKUP)?
                .map(|body| body.scope_record_index),
            None => None,
        };
        if let Some(scope_index) = candidate {
            if let Some(scope) = ctx.get_hash_map(&self.scopes, &scope_index, GRAPH_LOOKUP)? {
                if ctx.equal_bytes(
                    scope.body_records.as_bytes(),
                    collection.body_records.as_bytes(),
                    "match F3D mesh scope body lists",
                )? && ctx.all_by(
                    collection.body_records.0,
                    |reference| {
                        let Some(body_index) = local_reference_target(reference) else {
                            return Ok(false);
                        };
                        Ok(ctx
                            .get_hash_map(&self.bodies, &body_index, GRAPH_LOOKUP)?
                            .is_some_and(|body| {
                                body.scope_record_index == scope_index
                                    && body.collection_record_index == collection_index
                            }))
                    },
                    "match F3D mesh collection body links",
                )? {
                    return Ok(scope_index);
                }
            }
        }
        Err(self.scope_diagnostic(ctx, collection, candidate)?)
    }

    fn scope_diagnostic(
        &self,
        ctx: &DecodeContext<'_>,
        collection: &MeshCollectionRecord<'a>,
        candidate: Option<u32>,
    ) -> Result<CodecError, CodecError> {
        let (collection_bodies, _collection_storage) = diagnostic_targets(
            ctx,
            collection.body_records,
            "f3d mesh diagnostic collection bodies",
        )?;
        let candidate_scope = match candidate {
            Some(scope_index) => ctx.get_hash_map(&self.scopes, &scope_index, GRAPH_LOOKUP)?,
            None => None,
        };
        let (scope_bodies, _scope_storage) = match candidate_scope {
            Some(scope) => {
                let (bodies, storage) = diagnostic_targets(
                    ctx,
                    scope.body_records,
                    "f3d mesh diagnostic scope bodies",
                )?;
                (Some(bodies), Some(storage))
            }
            None => (None, None),
        };
        let (mut body_links, _links_storage) = ctx.scoped_vector_storage(
            collection.body_records.len(),
            "f3d mesh diagnostic body links",
        )?;
        for body_index in collection
            .body_records
            .targets(ctx, "f3d mesh diagnostic body links")?
        {
            if let Some(body) = ctx.get_hash_map(&self.bodies, &body_index, GRAPH_LOOKUP)? {
                ctx.push_vec(
                    &mut body_links,
                    (
                        body_index,
                        body.scope_record_index,
                        body.collection_record_index,
                    ),
                    "f3d mesh diagnostic body links",
                )?;
            }
        }
        mesh_graph_diagnostic(ctx, format_args!(
            "F3D Design mesh feature graph violates `each mesh collection has exactly one scope with the same ordered body list` in {}: collection {} bodies {:?}, scope {:?} bodies {:?}, body links {:?}",
            self.stream,
            collection.collection.record().record_index(),
            collection_bodies,
            candidate,
            scope_bodies,
            body_links,
        ))
    }

    /// Remove one collection's records from the graph and join them into a
    /// mesh feature.
    fn join_collection<F>(
        &mut self,
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        collection: MeshCollectionRecord<'a>,
        asset_for_filename: &mut F,
    ) -> Result<DesignMeshFeature, CodecError>
    where
        F: FnMut(&str) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError>,
    {
        let collection_index = collection.collection.record().record_index();
        let scope_record_index = self.collection_scope(ctx, &collection)?;
        let scope = ctx
            .remove_hash_map(&mut self.scopes, &scope_record_index, GRAPH_LOOKUP)?
            .ok_or_else(|| {
                self.error(
                    ctx,
                    "a mesh feature scope belongs to exactly one mesh collection",
                )
            })?;
        let texture_table = ctx
            .remove_hash_map(
                &mut self.texture_tables,
                &collection.texture_table_record_index,
                GRAPH_LOOKUP,
            )?
            .ok_or_else(|| {
                self.error(
                    ctx,
                    "a mesh texture table belongs to exactly one mesh collection",
                )
            })?;
        let collection_owner = ctx
            .remove_hash_map(
                &mut self.collection_owners,
                &collection.owner_record_index,
                GRAPH_LOOKUP,
            )?
            .filter(|owner| owner.collection_record_index == collection_index)
            .ok_or_else(|| {
                self.error(
                    ctx,
                    "each mesh collection has one unused owner with a reciprocal backlink",
                )
            })?;

        let MeshTextureTableRecord {
            identity: texture_table_identity,
            textures: entries,
        } = texture_table;
        let textures = ctx.try_collect_vec(
            entries
                .into_iter()
                .map(|entry| self.texture_resource(ctx, bytes, entry, &mut *asset_for_filename)),
            "f3d mesh texture resources",
        )?;

        let mut feature_bodies =
            ctx.vector_storage(collection.body_records.len(), "f3d mesh feature bodies")?;
        for body_record_index in collection
            .body_records
            .targets(ctx, "project F3D mesh collection bodies")?
        {
            let body = self.join_body(ctx, bytes, body_record_index)?;
            ctx.push_vec(&mut feature_bodies, body, "f3d mesh feature bodies")?;
        }
        let scope_offset = usize::try_from(scope.scope.record().byte_offset()).map_err(|_| {
            self.error(
                ctx,
                "mesh feature scope byte offsets fit the platform address domain",
            )
        })?;
        DesignMeshFeature::new(
            mesh_feature_id_charged(ctx, &self.stream, scope_offset)?,
            scope.scope,
            collection.collection,
            DesignMeshTextureTable::new_charged(ctx, texture_table_identity, textures).map_err(
                |error| match error {
                    CodecError::Malformed(message) => self.error(ctx, &message),
                    other => other,
                },
            )?,
            collection_owner.owner,
            feature_bodies,
        )
        .map_err(|message| self.error(ctx, &message))
    }

    fn texture_resource<F>(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        entry: MeshTextureEntry,
        asset_for_filename: &mut F,
    ) -> Result<DesignMeshTextureResource, CodecError>
    where
        F: FnMut(&str) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError>,
    {
        let filename_entry = entry.filename.ok_or_else(|| {
            self.error(
                ctx,
                "texture flag and filename maps have identical GUID keys",
            )
        })?;
        let filename_frame = ctx
            .get_hash_map(
                &self.filename_frames,
                &filename_entry.record_index,
                GRAPH_LOOKUP,
            )?
            .copied()
            .ok_or_else(|| {
                self.error(
                    ctx,
                    "each texture filename reference targets a filename record",
                )
            })?;
        let (filename_record, filename) =
            parse_mesh_texture_filename_record(ctx, bytes, filename_frame)?;
        let (archive_entry_name, asset) = asset_for_filename(&filename)?;
        Ok(DesignMeshTextureResource {
            ordinal: entry.ordinal,
            resource_guid: entry.resource_guid,
            flags: entry.flags,
            filename_ordinal: filename_entry.ordinal,
            file: DesignMeshTextureFile::new(filename_record, &filename, archive_entry_name)
                .map_err(|message| self.error(ctx, &message))?,
            asset,
        })
    }

    /// Remove one mesh body and the records only it uses from the graph.
    fn join_body(
        &mut self,
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        body_record_index: u32,
    ) -> Result<DesignMeshBody, CodecError> {
        let body = ctx
            .remove_hash_map(&mut self.bodies, &body_record_index, GRAPH_LOOKUP)?
            .ok_or_else(|| {
                self.error(
                    ctx,
                    "each collection body reference targets one unused mesh body",
                )
            })?;
        let wrapper = ctx
            .remove_hash_map(&mut self.wrappers, &body.wrapper_record_index, GRAPH_LOOKUP)?
            .filter(|wrapper| wrapper.body_record_index == body.placement.record().record_index())
            .ok_or_else(|| self.error(ctx, "each mesh body has one unused reciprocal wrapper"))?;
        let guid = ctx
            .remove_hash_map(&mut self.guids, &body.guid_record_index, GRAPH_LOOKUP)?
            .ok_or_else(|| self.error(ctx, "each mesh body has one unused GUID record"))?;
        let entry_name = ctx
            .remove_hash_map(
                &mut self.entry_names,
                &guid.entry_name_record_index,
                GRAPH_LOOKUP,
            )?
            .filter(|entry| entry.guid_record_index == guid.guid.record().record_index())
            .ok_or_else(|| {
                self.error(
                    ctx,
                    "each mesh GUID has one unused reciprocal entry-name record",
                )
            })?;
        let scene_node = ctx
            .remove_hash_map(
                &mut self.scene_nodes,
                &body.scene_node_record_index,
                GRAPH_LOOKUP,
            )?
            .ok_or_else(|| self.error(ctx, "each mesh body has one unused Scene node"))?;
        let scene_state = ctx
            .remove_hash_map(
                &mut self.states,
                &scene_node.state_record_index,
                GRAPH_LOOKUP,
            )?
            .ok_or_else(|| self.error(ctx, "each Scene node has one unused Scene state"))?;
        let scene_auxiliary_frame = ctx
            .remove_hash_map(
                &mut self.scene_auxiliary_frames,
                &scene_node.auxiliary_record_index,
                GRAPH_LOOKUP,
            )?
            .ok_or_else(|| {
                self.error(ctx, "each Scene node has one unused Scene auxiliary record")
            })?;
        let scene_auxiliary = parse_typed_identity(
            ctx,
            bytes,
            scene_auxiliary_frame,
            SCENE_AUXILIARY_TYPE_VERSION,
            SCENE_AUXILIARY_BASE_TYPE_GUID,
            SCENE_MODULE,
            "mesh-scene-auxiliary",
        )?;
        let body_owner_frame = ctx
            .get_hash_map(
                &self.body_owner_frames,
                &body.owner_record_index,
                GRAPH_LOOKUP,
            )?
            .copied()
            .ok_or_else(|| self.error(ctx, "each mesh body references a typed Body owner"))?;
        let body_owner = parse_typed_identity(
            ctx,
            bytes,
            body_owner_frame,
            MESH_BODY_OWNER_TYPE_VERSION,
            MESH_BODY_OWNER_BASE_TYPE_GUID,
            "Body",
            "mesh-body-owner",
        )?;
        Ok(DesignMeshBody {
            placement: body.placement,
            entry: entry_name.entry,
            guid: guid.guid,
            wrapper_record: wrapper.identity,
            scene_state,
            scene_node: scene_node.node,
            scene_auxiliary_record: scene_auxiliary,
            owner_record: body_owner,
            container_mesh_uuid: None,
            tessellation_id: None,
        })
    }

    /// Whether a record that must belong to a feature is left unjoined.
    fn has_unjoined_records(&self) -> bool {
        !self.entry_names.is_empty()
            || !self.guids.is_empty()
            || !self.bodies.is_empty()
            || !self.texture_tables.is_empty()
            || !self.wrappers.is_empty()
            || !self.scopes.is_empty()
    }
}

fn parse_mesh_design_records<F>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    source_entry_name: &str,
    asset_for_filename: &mut F,
) -> Result<Vec<DesignMeshFeature>, CodecError>
where
    F: FnMut(&str) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError>,
{
    let mut storage = ctx.reserve_scoped(0, "f3d mesh graph records")?;
    let collection_frames = typed_primary_frames(
        ctx,
        bytes,
        meta,
        MESH_COLLECTION_TYPE_GUID,
        "mesh-collection",
    )?;
    let mut collections = Vec::new();
    for frame in ctx.admit_iter(&collection_frames, "parse F3D mesh collections")? {
        let collection = parse_mesh_collection_record(ctx, bytes, meta, *frame)?;
        if !collection.body_records.is_empty() {
            storage.with_storage(|| {
                ctx.push_vec(&mut collections, collection, "f3d mesh collection records")
            })?;
        }
    }
    if collections.is_empty() {
        return Ok(Vec::new());
    }
    let (_stream_storage, stream) = native_scope_scoped(ctx, source_entry_name)?;
    let collection_indices = mesh_collection_indices(ctx, &mut storage, &collections)?;
    let frames = |type_guid: &str, record_kind: &str| {
        typed_primary_frames(ctx, bytes, meta, type_guid, record_kind)
    };
    let entry_names = record_map(
        ctx,
        &mut storage,
        &frames(MESH_ENTRY_NAME_TYPE_GUID, "mesh-entry-name")?,
        "mesh-entry-name",
        |frame| parse_mesh_entry_name_record(ctx, bytes, frame).map(Some),
    )?;
    let guids = record_map(
        ctx,
        &mut storage,
        &frames(MESH_GUID_TYPE_GUID, "mesh-GUID")?,
        "mesh-GUID",
        |frame| parse_mesh_guid_record(ctx, bytes, frame).map(Some),
    )?;
    let bodies = record_map(
        ctx,
        &mut storage,
        &frames(MESH_BODY_TYPE_GUID, "mesh-body")?,
        "mesh-body",
        |frame| parse_mesh_body_record(ctx, bytes, frame).map(Some),
    )?;
    let mut texture_storage = ctx.reserve_scoped(0, "f3d mesh texture tables")?;
    let texture_tables = record_map(
        ctx,
        &mut storage,
        &frames(MESH_TEXTURE_TABLE_TYPE_GUID, "mesh-texture-table")?,
        "mesh-texture-table",
        |frame| parse_mesh_texture_table_record(ctx, &mut texture_storage, bytes, frame).map(Some),
    )?;
    let wrappers = record_map(
        ctx,
        &mut storage,
        &frames(MESH_WRAPPER_TYPE_GUID, "mesh-wrapper")?,
        "mesh-wrapper",
        |frame| parse_mesh_wrapper_record(ctx, bytes, frame).map(Some),
    )?;
    let records = IndexedRecordOffsets::build(ctx, bytes)?;
    let scopes = record_map(
        ctx,
        &mut storage,
        &frames(MESH_FEATURE_SCOPE_TYPE_GUID, "mesh-feature-scope")?,
        "mesh-feature-scope",
        |frame| parse_mesh_scope_record(ctx, bytes, meta, &records, frame).map(Some),
    )?;
    let states = record_map(
        ctx,
        &mut storage,
        &frames(MESH_SCENE_STATE_TYPE_GUID, "mesh-scene-state")?,
        "mesh-scene-state",
        |frame| parse_mesh_scene_state_record(ctx, bytes, frame).map(Some),
    )?;
    let scene_nodes = record_map(
        ctx,
        &mut storage,
        &frames(SCENE_NODE_TYPE_GUID, "mesh-scene-node")?,
        "mesh-scene-node",
        |frame| parse_scene_node_record(ctx, bytes, frame).map(Some),
    )?;
    let scene_auxiliary_frames = record_map(
        ctx,
        &mut storage,
        &frames(SCENE_AUXILIARY_TYPE_GUID, "mesh-scene-auxiliary")?,
        "mesh-scene-auxiliary",
        |frame| Ok(Some(frame)),
    )?;
    let filename_frames = record_map(
        ctx,
        &mut storage,
        &frames(MESH_TEXTURE_FILENAME_TYPE_GUID, "mesh-texture-filename")?,
        "mesh-texture-filename",
        |frame| Ok(Some(frame)),
    )?;
    let collection_owners = record_map(
        ctx,
        &mut storage,
        &frames(MESH_COLLECTION_OWNER_TYPE_GUID, "mesh-collection-owner")?,
        "mesh-collection-owner",
        |frame| {
            let Some(owner) = parse_mesh_collection_owner_record(ctx, bytes, frame)? else {
                return Ok(None);
            };
            Ok(ctx
                .contains_hash_set(
                    &collection_indices,
                    &owner.collection_record_index,
                    "match F3D mesh collection owners",
                )?
                .then_some(owner))
        },
    )?;
    let body_owner_frames = record_map(
        ctx,
        &mut storage,
        &frames(MESH_BODY_OWNER_TYPE_GUID, "mesh-body-owner")?,
        "mesh-body-owner",
        |frame| Ok(Some(frame)),
    )?;
    let mut graph = MeshGraph {
        stream,
        entry_names,
        guids,
        bodies,
        texture_tables,
        wrappers,
        scopes,
        states,
        scene_nodes,
        scene_auxiliary_frames,
        filename_frames,
        collection_owners,
        body_owner_frames,
    };
    let features = ctx.try_collect_vec(
        collections.into_iter().map(|collection| {
            graph.join_collection(ctx, bytes, collection, &mut *asset_for_filename)
        }),
        "f3d mesh graph features",
    )?;
    if graph.has_unjoined_records() {
        return Err(graph.error(
            ctx,
            "all typed mesh graph records belong to exactly one feature",
        ));
    }
    Ok(features)
}

/// Every mesh feature of every Design stream, in stream order.
fn decode_mesh_design_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignMeshFeature>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D mesh design streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let Some(meta) = metadata_for_bulk_stream(ctx, scan, &entry.name)? else {
            continue;
        };
        let mut asset_for_filename = |filename: &str| mesh_image_asset(ctx, scan, filename);
        let mut features = parse_mesh_design_records(
            ctx,
            scan.entry_bytes(&entry.name)?,
            &meta,
            &entry.name,
            &mut asset_for_filename,
        )?;
        ctx.append_vec(&mut out, &mut features, "f3d mesh decoded features")?;
    }
    Ok(out)
}

/// The text after the last `/` of an archive entry name.
fn entry_basename<'name>(
    ctx: &DecodeContext<'_>,
    name: &'name str,
) -> Result<&'name str, CodecError> {
    Ok(
        rsplit_once_ascii(ctx, name, b'/', "split F3D mesh entry basename")?
            .map_or(name, |(_, basename)| basename),
    )
}

/// The one embedded Design image whose basename is `filename`.
fn mesh_image_asset(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    filename: &str,
) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError> {
    const OPERATION: &str = "find F3D mesh texture image";
    let is_image = |candidate: &ContainerEntry| -> Result<bool, CodecError> {
        if !scan.is_design_asset_entry(candidate, ContainerRole::Image) {
            return Ok(false);
        }
        let basename = entry_basename(ctx, &candidate.name)?;
        ctx.equal_bytes(basename.as_bytes(), filename.as_bytes(), OPERATION)
    };
    let entries = scan.entries.as_slice();
    let asset = match ctx.position_by(entries, &is_image, OPERATION)? {
        Some(index) => {
            let later = entries.get(index + 1..).unwrap_or_default();
            if ctx.any_by(later, &is_image, OPERATION)? {
                None
            } else {
                entries.get(index)
            }
        }
        None => None,
    };
    let Some(asset) = asset else {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!(
                "F3D Design mesh texture `{filename}` does not resolve to one embedded image"
            ),
        ));
    };
    Ok((
        ctx.copy_retained_text(&asset.name, "f3d mesh image entry name")?,
        neutral_asset_id_charged(ctx, &asset.name)?,
    ))
}

fn mesh_feature_id_charged(
    ctx: &DecodeContext<'_>,
    stream: &str,
    offset: usize,
) -> Result<String, CodecError> {
    let mut id = ctx.copy_retained_text(stream, "f3d mesh feature ID prefix")?;
    ctx.append_formatted_retained(
        &mut id,
        format_args!(":design-mesh-feature#{offset}"),
        "f3d mesh feature ID suffix",
    )?;
    Ok(id)
}

/// Whether an unjoined mesh body names the archive entry and Fusion UUID.
fn names_container(
    ctx: &DecodeContext<'_>,
    body: &DesignMeshBody,
    entry_name: &str,
    fusion_uuid: &str,
) -> Result<bool, CodecError> {
    Ok(body.tessellation_id.is_none()
        && ctx.equal_bytes(
            body.entry.name().as_bytes(),
            entry_name.as_bytes(),
            "match F3D mesh entry name",
        )?
        && ctx.eq_ignore_ascii_case(
            body.guid.value(),
            fusion_uuid,
            "match F3D mesh Fusion UUID",
        )?)
}

/// The feature and body ordinals of the one unjoined mesh body that names
/// the container. The search stops at a second match.
fn resolve_mesh_body(
    ctx: &DecodeContext<'_>,
    features: &[DesignMeshFeature],
    entry_name: &str,
    fusion_uuid: &str,
) -> Result<Option<(usize, usize)>, CodecError> {
    const OPERATION: &str = "join F3D mesh body records";
    let mut joined = None;
    let mut feature_ordinal = 0;
    let ambiguous = ctx.any_by(
        features,
        |feature| {
            let current_feature = feature_ordinal;
            feature_ordinal += 1;
            let mut body_ordinal = 0;
            ctx.any_by(
                feature.bodies(),
                |body| {
                    let current_body = body_ordinal;
                    body_ordinal += 1;
                    if !names_container(ctx, body, entry_name, fusion_uuid)? {
                        return Ok(false);
                    }
                    if joined.is_some() {
                        return Ok(true);
                    }
                    joined = Some((current_feature, current_body));
                    Ok(false)
                },
                OPERATION,
            )
        },
        OPERATION,
    )?;
    Ok(if ambiguous { None } else { joined })
}

/// Decode every mesh body: one per `.paramesh` container joined to the
/// mesh-body record that names its GUID record.
pub(crate) fn decode_mesh_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<MeshDecode, CodecError> {
    let mut features = decode_mesh_design_records(ctx, scan)?;
    let mut outcomes = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D ParaMesh assets")?
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::Paramesh))
    {
        let container = match scan
            .entry_bytes(&entry.name)
            .and_then(|bytes| decode_mesh_container(ctx, bytes))
        {
            Ok(container) => container,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                ctx.push_vec(
                    &mut outcomes,
                    MeshContainerOutcome::Failed {
                        entry_name: ctx
                            .copy_retained_text(&entry.name, "f3d failed mesh entry name")?,
                        error,
                    },
                    "f3d mesh container outcomes",
                )?;
                continue;
            }
        };
        let (fusion_uuid, mesh_uuid, geometry) = split_container(container);
        let base = entry_basename(ctx, &entry.name)?;
        let Some((feature_ordinal, body_ordinal)) =
            resolve_mesh_body(ctx, &features, base, &fusion_uuid)?
        else {
            ctx.push_vec(
                &mut outcomes,
                MeshContainerOutcome::Unjoined {
                    entry_name: ctx
                        .copy_retained_text(&entry.name, "f3d unjoined mesh entry name")?,
                },
                "f3d mesh container outcomes",
            )?;
            continue;
        };
        let body = &mut features[feature_ordinal].bodies_mut()[body_ordinal];
        body.container_mesh_uuid = Some(mesh_uuid);
        let projected = match MeshBody::from_geometry(
            ctx,
            &entry.name,
            body.placement.record().byte_offset(),
            body.placement.transform(),
            geometry,
        ) {
            Ok(projected) => projected,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                ctx.push_vec(
                    &mut outcomes,
                    MeshContainerOutcome::Failed {
                        entry_name: ctx
                            .copy_retained_text(&entry.name, "f3d failed mesh entry name")?,
                        error,
                    },
                    "f3d mesh container outcomes",
                )?;
                continue;
            }
        };
        body.tessellation_id =
            Some(ctx.copy_retained_text(&projected.id, "f3d mesh tessellation reference")?);
        ctx.push_vec(
            &mut outcomes,
            MeshContainerOutcome::Joined(projected),
            "f3d mesh container outcomes",
        )?;
    }
    for feature in ctx.admit_iter(&features, "scan F3D mesh features for missing bodies")? {
        for body in ctx.admit_iter(
            feature.bodies(),
            "scan F3D mesh feature bodies for missing bodies",
        )? {
            if body.tessellation_id.is_some() {
                continue;
            }
            ctx.push_vec(
                &mut outcomes,
                MeshContainerOutcome::Missing {
                    entry_name: ctx
                        .copy_retained_text(body.entry.name(), "f3d missing mesh entry name")?,
                },
                "f3d mesh container outcomes",
            )?;
        }
    }
    Ok(MeshDecode { outcomes, features })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    mod diagnostic_scope_copy;
    mod identity;
    mod placement;
    mod push_vec;
    mod text;
    use super::{
        parse_mesh_collection_owner_record, parse_mesh_scene_state_record,
        parse_mesh_texture_table_record, parse_mesh_wrapper_record, parse_scene_node_record,
        resolve_mesh_body, MeshBody, COMMON_DATA_MODULE, DATA_MODEL_MODULE, FUSION_MODULE,
        MATRIX_BYTES, MESH_BODY_BASE_TYPE_GUID, MESH_BODY_OWNER_BASE_TYPE_GUID,
        MESH_BODY_OWNER_TYPE_GUID, MESH_BODY_OWNER_TYPE_VERSION, MESH_BODY_TYPE_GUID,
        MESH_BODY_TYPE_VERSION, MESH_COLLECTION_BASE_BASE_TYPE_GUID,
        MESH_COLLECTION_BASE_TYPE_GUID, MESH_COLLECTION_BASE_TYPE_VERSION,
        MESH_COLLECTION_OWNER_BASE_TYPE_GUID, MESH_COLLECTION_OWNER_TYPE_GUID,
        MESH_COLLECTION_OWNER_TYPE_VERSIONS, MESH_COLLECTION_TYPE_GUID,
        MESH_COLLECTION_TYPE_VERSION, MESH_ENTRY_NAME_BASE_TYPE_GUID, MESH_ENTRY_NAME_TYPE_GUID,
        MESH_ENTRY_NAME_TYPE_VERSION, MESH_FEATURE_SCOPE_BASE_TYPE_GUID,
        MESH_FEATURE_SCOPE_TYPE_GUID, MESH_FEATURE_SCOPE_TYPE_VERSION, MESH_GUID_BASE_TYPE_GUID,
        MESH_GUID_TYPE_GUID, MESH_GUID_TYPE_VERSION, MESH_SCENE_STATE_BASE_TYPE_GUID,
        MESH_SCENE_STATE_TYPE_GUID, MESH_SCENE_STATE_TYPE_VERSION,
        MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID, MESH_SCOPE_BASE_RECORD_TYPE_GUID,
        MESH_SCOPE_BASE_RECORD_TYPE_VERSION, MESH_TEXTURE_FILENAME_BASE_TYPE_GUID,
        MESH_TEXTURE_FILENAME_TYPE_GUID, MESH_TEXTURE_FILENAME_TYPE_VERSION,
        MESH_TEXTURE_TABLE_BASE_TYPE_GUID, MESH_TEXTURE_TABLE_TYPE_GUID,
        MESH_TEXTURE_TABLE_TYPE_VERSION, MESH_WRAPPER_BASE_TYPE_GUID, MESH_WRAPPER_TYPE_GUID,
        MESH_WRAPPER_TYPE_VERSION, PARAMESH_MODULE, SAME_SEGMENT_REFERENCE_BYTES,
        SCENE_AUXILIARY_BASE_TYPE_GUID, SCENE_AUXILIARY_TYPE_GUID, SCENE_AUXILIARY_TYPE_VERSION,
        SCENE_MODULE, SCENE_NODE_BASE_TYPE_GUID, SCENE_NODE_TYPE_GUID, SCENE_NODE_TYPE_VERSION,
    };
    use crate::design::decode::meta::{
        typed_primary_frames as typed_primary_frames_with_context, TypedPrimaryFrame,
    };
    use crate::design::test_support::{design_type, primary_record};
    use crate::layout::{
        paramesh_collection_owner_backlink_prefix as collection_owner,
        paramesh_collection_owner_v17 as collection_owner_v17,
        paramesh_mesh_body_join_prefix as mesh_body,
        paramesh_mesh_collection_base_prefix as mesh_collection_base,
        paramesh_mesh_collection_prefix as mesh_collection, paramesh_scene_node as scene_node,
        paramesh_scene_node_placed as placed_scene_node,
        paramesh_texture_table_prefix as texture_table,
    };
    use crate::paramesh::MeshContainer;
    use crate::test_support::{lp_ascii, lp_utf16};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::units::UnitVector3;

    fn typed_primary_frames<'a>(
        bytes: &[u8],
        meta: &'a crate::metastream::MetaStream,
        type_guid: &str,
        record_kind: &str,
    ) -> Result<Vec<TypedPrimaryFrame<'a>>, CodecError> {
        crate::design::test_support::with_test_decode_context(|ctx| {
            typed_primary_frames_with_context(ctx, bytes, meta, type_guid, record_kind)
        })
    }

    fn parse_texture_table(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &[u8],
        frame: TypedPrimaryFrame<'_>,
    ) -> Result<super::MeshTextureTableRecord, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "test texture storage")?;
        parse_mesh_texture_table_record(ctx, &mut storage, bytes, frame)
    }

    fn parse_mesh_design_records<F>(
        bytes: &[u8],
        meta: &crate::metastream::MetaStream,
        source_entry_name: &str,
        asset_for_filename: &mut F,
    ) -> Result<Vec<crate::records::mesh::DesignMeshFeature>, CodecError>
    where
        F: FnMut(&str) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError>,
    {
        crate::design::test_support::with_test_decode_context(|ctx| {
            super::parse_mesh_design_records(
                ctx,
                bytes,
                meta,
                source_entry_name,
                asset_for_filename,
            )
        })
    }

    #[test]
    fn mesh_record_map_refuses_collection_limit() {
        let graph = synthetic_mesh_graph(false);
        let frames = typed_primary_frames(
            &graph.bytes,
            &graph.meta,
            super::MESH_COLLECTION_TYPE_GUID,
            "mesh-collection",
        )
        .unwrap();
        assert!(!frames.is_empty());
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test record map").unwrap();
        assert!(matches!(
            super::record_map(&ctx, &mut storage, &frames, "mesh-collection", |frame| Ok(Some(frame))),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && limit.operation == "f3d mesh record map"
        ));
    }

    #[test]
    fn mesh_local_reference_accepts_only_the_eleven_byte_local_form() {
        let local = |target: u64| {
            let mut bytes = vec![1];
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.extend_from_slice(&[0, 0]);
            bytes
        };
        assert_eq!(super::exact_local_record_index(&local(104), 0), Some(104));
        assert_eq!(super::exact_local_record_index(&local(0), 0), None);
        assert_eq!(
            super::exact_local_record_index(&local(u64::from(u32::MAX) + 1), 0),
            None
        );
        let mut null = local(104);
        null[0] = 0;
        assert_eq!(super::exact_local_record_index(&null, 0), None);
        let mut cross_segment = local(104);
        cross_segment[10] = 1;
        assert_eq!(super::exact_local_record_index(&cross_segment, 0), None);
        let mut inline_typed = local(104);
        inline_typed.truncate(9);
        lp_ascii(&mut inline_typed, "10000000-0000-4000-8000-000000000001");
        inline_typed.extend_from_slice(&[0, 0]);
        assert_eq!(super::exact_local_record_index(&inline_typed, 0), None);
        assert_eq!(super::exact_local_record_index(&local(104)[..10], 0), None);
    }

    #[test]
    fn anisotropic_mesh_normal_preserves_orientation_without_cofactor_overflow() {
        for sign in [-1.0, 1.0] {
            let transform = crate::records::mesh::MeshAffineTransform::new([
                sign * 1e200,
                0.0,
                0.0,
                0.0,
                0.0,
                1e200,
                0.0,
                0.0,
                0.0,
                0.0,
                1e-200,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ])
            .unwrap();
            assert_eq!(
                *transform
                    .transform_normal(UnitVector3::Z_AXIS)
                    .unwrap()
                    .as_raw(),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, sign)
            );
        }
    }

    fn matrix(cells: [f64; 16]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(MATRIX_BYTES);
        for value in cells {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn push_indexed_header(bytes: &mut Vec<u8>, class_tag: u32, record_index: u32) {
        let class_tag = class_tag.to_string();
        assert_eq!(class_tag.len(), 3);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag.as_bytes());
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn put_reference(bytes: &mut [u8], at: usize, target: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 9].copy_from_slice(&u64::from(target).to_le_bytes());
        bytes[at + 9..at + 11].copy_from_slice(&[0, 0]);
    }

    fn push_reference(bytes: &mut Vec<u8>, target: u32) {
        let at = bytes.len();
        bytes.resize(at + SAME_SEGMENT_REFERENCE_BYTES, 0);
        put_reference(bytes, at, target);
    }

    fn mesh_entry_record(
        class_tag: u32,
        record_index: u32,
        guid_record_index: u32,
        entry_name: &str,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        push_reference(&mut bytes, guid_record_index);
        lp_utf16(&mut bytes, entry_name);
        bytes
    }

    fn mesh_guid_record(
        class_tag: u32,
        record_index: u32,
        entry_name_record_index: u32,
        fusion_uuid: &str,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 21]);
        lp_ascii(&mut bytes, fusion_uuid);
        push_reference(&mut bytes, entry_name_record_index);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    #[allow(clippy::too_many_arguments)] // One argument per serialized body-graph reference.
    fn mesh_body_record(
        class_tag: u32,
        record_index: u32,
        guid_record_index: u32,
        scope_record_index: u32,
        wrapper_record_index: u32,
        owner_record_index: u32,
        scene_node_record_index: u32,
        collection_record_index: u32,
    ) -> Vec<u8> {
        let identity = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.resize(600, 0);
        bytes[mesh_body::FIRST_TRANSFORM..mesh_body::FIRST_TRANSFORM + MATRIX_BYTES]
            .copy_from_slice(&matrix(identity));
        bytes[mesh_body::SECOND_TRANSFORM..mesh_body::SECOND_TRANSFORM + MATRIX_BYTES]
            .copy_from_slice(&matrix(identity));
        put_reference(
            &mut bytes,
            mesh_body::FEATURE_SCOPE_REFERENCE,
            scope_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::WRAPPER_REFERENCE,
            wrapper_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::BODY_OWNER_REFERENCE,
            owner_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::CONTAINER_GUID_REFERENCE,
            guid_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::SCENE_NODE_REFERENCE,
            scene_node_record_index,
        );
        let tail_at = bytes.len() - SAME_SEGMENT_REFERENCE_BYTES;
        put_reference(&mut bytes, tail_at, collection_record_index);
        bytes
    }

    fn push_scene_footer(bytes: &mut Vec<u8>) {
        bytes.push(1);
        for index in 0..49 {
            bytes.push(match index {
                6 | 14 | 22 | 30 | 38 | 46 => 0xef,
                31 | 39 | 47 => 0x7f,
                48 => 0x01,
                _ => 0xff,
            });
        }
    }

    fn mesh_collection_record(
        class_tag: u32,
        base_class_tag: u32,
        record_index: u32,
        texture_table_record_index: u32,
        body_record_indices: &[u32],
        owner_record_index: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(
            &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&[1, 1]);
        push_reference(&mut bytes, texture_table_record_index);
        push_indexed_header(&mut bytes, base_class_tag, record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.extend_from_slice(
            &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        for body in body_record_indices {
            push_reference(&mut bytes, *body);
        }
        push_reference(&mut bytes, owner_record_index);
        bytes
    }

    fn mesh_texture_table_record(
        class_tag: u32,
        record_index: u32,
        flags: &[(&str, u32)],
        filenames: &[(&str, u32)],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(
            &(u32::try_from(flags.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (guid, value) in flags {
            lp_ascii(&mut bytes, guid);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(
            &(u32::try_from(filenames.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (guid, target) in filenames {
            lp_ascii(&mut bytes, guid);
            push_reference(&mut bytes, *target);
        }
        bytes
    }

    fn mesh_wrapper_record(class_tag: u32, record_index: u32, body_record_index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        push_reference(&mut bytes, body_record_index);
        bytes.extend_from_slice(&[0; 8]);
        bytes
    }

    fn mesh_scene_state_record(class_tag: u32, record_index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 34]);
        push_scene_footer(&mut bytes);
        bytes
    }

    fn mesh_scene_node_record(
        class_tag: u32,
        record_index: u32,
        state_record_index: u32,
        auxiliary_record_index: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 14]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        push_reference(&mut bytes, state_record_index);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        push_reference(&mut bytes, auxiliary_record_index);
        bytes.extend_from_slice(&[0; 24]);
        push_scene_footer(&mut bytes);
        bytes
    }

    fn placed_mesh_scene_node_record(
        class_tag: u32,
        record_index: u32,
        state_record_index: u32,
        auxiliary_record_index: u32,
        transform: [f64; 16],
        bounds: [f64; 6],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 14]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        push_reference(&mut bytes, state_record_index);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        push_reference(&mut bytes, auxiliary_record_index);
        bytes.extend_from_slice(&[0; 25]);
        for value in transform {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in bounds {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(1);
        bytes
    }

    fn mesh_scope_record(
        class_tag: u32,
        base_class_tag: u32,
        record_index: u32,
        body_record_indices: &[u32],
        member_record_index: u32,
        owner_record_index: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(
            &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        for body in body_record_indices {
            push_reference(&mut bytes, *body);
        }
        bytes.extend_from_slice(&[0; 24]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_reference(&mut bytes, member_record_index);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, "Base Mesh Feature");
        let mut tail = [0; 78];
        tail[..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        push_indexed_header(&mut bytes, base_class_tag, record_index);
        bytes.extend_from_slice(&[0; 8]);
        push_reference(&mut bytes, owner_record_index);
        bytes
    }

    fn collection_owner_record(
        class_tag: u32,
        record_index: u32,
        collection_record_index: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.resize(collection_owner::COLLECTION_BACKLINK, 0);
        push_reference(&mut bytes, collection_record_index);
        bytes
    }

    fn identity_record(class_tag: u32, record_index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes
    }

    fn texture_filename_record(class_tag: u32, record_index: u32, filename: &str) -> Vec<u8> {
        let mut bytes = identity_record(class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, filename);
        bytes
    }

    fn push_primary_frame(
        bytes: &mut Vec<u8>,
        records: &mut Vec<crate::metastream::RecordIndexEntry>,
        entity_id: u32,
        frame: Vec<u8>,
    ) {
        records.push(primary_record(u64::from(entity_id), bytes.len()));
        bytes.extend(frame);
    }

    struct SyntheticMeshGraph {
        bytes: Vec<u8>,
        meta: crate::metastream::MetaStream,
    }

    fn no_texture_asset(
        _filename: &str,
    ) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError> {
        panic!("empty texture table must not resolve an asset")
    }

    fn synthetic_mesh_graph_with_body_count(
        with_textures: bool,
        body_count: usize,
    ) -> SyntheticMeshGraph {
        const ENTRY: u32 = 102;
        const GUID: u32 = 103;
        const BODY: u32 = 104;
        const STATE: u32 = 105;
        const AUXILIARY: u32 = 106;
        const NODE: u32 = 107;
        const WRAPPER: u32 = 108;
        const SCOPE: u32 = 109;
        const COLLECTION_OWNER: u32 = 110;
        const BODY_OWNER: u32 = 111;
        const SCOPE_OWNER: u32 = 112;
        const FILENAME_A: u32 = 113;
        const FILENAME_B: u32 = 114;
        const SECOND_ENTRY: u32 = 115;
        const SECOND_GUID: u32 = 116;
        const SECOND_BODY: u32 = 117;
        const SECOND_STATE: u32 = 118;
        const SECOND_AUXILIARY: u32 = 119;
        const SECOND_NODE: u32 = 120;
        const SECOND_WRAPPER: u32 = 121;
        const COLLECTION: u32 = 100;
        const TEXTURES: u32 = 101;
        const ENTRIES: [u32; 2] = [ENTRY, SECOND_ENTRY];
        const GUIDS: [u32; 2] = [GUID, SECOND_GUID];
        const BODIES: [u32; 2] = [BODY, SECOND_BODY];
        const STATES: [u32; 2] = [STATE, SECOND_STATE];
        const AUXILIARIES: [u32; 2] = [AUXILIARY, SECOND_AUXILIARY];
        const NODES: [u32; 2] = [NODE, SECOND_NODE];
        const WRAPPERS: [u32; 2] = [WRAPPER, SECOND_WRAPPER];
        const ENTRY_NAMES: [&str; 2] = [
            "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh",
            "ParaMeshGeometry.66666666-7777-4888-8999-AAAAAAAAAAAA.paramesh",
        ];
        const FUSION_UUIDS: [&str; 2] = [
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
            "BBBBBBBB-CCCC-4DDD-8EEE-FFFFFFFFFFFF",
        ];
        const RESOURCE_A: &str = "10000000-0000-4000-8000-000000000001";
        const RESOURCE_B: &str = "20000000-0000-4000-8000-000000000002";

        const TAG_ENTRY: u32 = 256;
        const TAG_GUID: u32 = 257;
        const TAG_BODY: u32 = 258;
        const TAG_COLLECTION: u32 = 259;
        const TAG_COLLECTION_BASE: u32 = 260;
        const TAG_TEXTURES: u32 = 261;
        const TAG_WRAPPER: u32 = 262;
        const TAG_SCOPE: u32 = 263;
        const TAG_SCOPE_BASE: u32 = 264;
        const TAG_STATE: u32 = 265;
        const TAG_NODE: u32 = 266;
        const TAG_AUXILIARY: u32 = 267;
        const TAG_COLLECTION_OWNER: u32 = 268;
        const TAG_BODY_OWNER: u32 = 269;
        const TAG_FILENAME: u32 = 270;

        assert!(body_count <= 2);
        let entity_ids = |indices: &[u32]| {
            indices[..body_count]
                .iter()
                .copied()
                .map(u64::from)
                .collect()
        };

        let filename_ids = if with_textures {
            vec![u64::from(FILENAME_A), u64::from(FILENAME_B)]
        } else {
            Vec::new()
        };
        let types = vec![
            design_type(
                MESH_ENTRY_NAME_TYPE_GUID,
                Some(MESH_ENTRY_NAME_BASE_TYPE_GUID),
                MESH_ENTRY_NAME_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&ENTRIES),
            ),
            design_type(
                MESH_GUID_TYPE_GUID,
                Some(MESH_GUID_BASE_TYPE_GUID),
                MESH_GUID_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&GUIDS),
            ),
            design_type(
                MESH_BODY_TYPE_GUID,
                Some(MESH_BODY_BASE_TYPE_GUID),
                MESH_BODY_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&BODIES),
            ),
            design_type(
                MESH_COLLECTION_TYPE_GUID,
                Some(MESH_COLLECTION_BASE_TYPE_GUID),
                MESH_COLLECTION_TYPE_VERSION,
                PARAMESH_MODULE,
                vec![u64::from(COLLECTION)],
            ),
            design_type(
                MESH_COLLECTION_BASE_TYPE_GUID,
                Some(MESH_COLLECTION_BASE_BASE_TYPE_GUID),
                MESH_COLLECTION_BASE_TYPE_VERSION,
                COMMON_DATA_MODULE,
                Vec::new(),
            ),
            design_type(
                MESH_TEXTURE_TABLE_TYPE_GUID,
                Some(MESH_TEXTURE_TABLE_BASE_TYPE_GUID),
                MESH_TEXTURE_TABLE_TYPE_VERSION,
                PARAMESH_MODULE,
                vec![u64::from(TEXTURES)],
            ),
            design_type(
                MESH_WRAPPER_TYPE_GUID,
                Some(MESH_WRAPPER_BASE_TYPE_GUID),
                MESH_WRAPPER_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&WRAPPERS),
            ),
            design_type(
                MESH_FEATURE_SCOPE_TYPE_GUID,
                Some(MESH_FEATURE_SCOPE_BASE_TYPE_GUID),
                MESH_FEATURE_SCOPE_TYPE_VERSION,
                FUSION_MODULE,
                vec![u64::from(SCOPE)],
            ),
            design_type(
                MESH_SCOPE_BASE_RECORD_TYPE_GUID,
                Some(MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID),
                MESH_SCOPE_BASE_RECORD_TYPE_VERSION,
                DATA_MODEL_MODULE,
                Vec::new(),
            ),
            design_type(
                MESH_SCENE_STATE_TYPE_GUID,
                Some(MESH_SCENE_STATE_BASE_TYPE_GUID),
                MESH_SCENE_STATE_TYPE_VERSION,
                SCENE_MODULE,
                entity_ids(&STATES),
            ),
            design_type(
                SCENE_NODE_TYPE_GUID,
                Some(SCENE_NODE_BASE_TYPE_GUID),
                SCENE_NODE_TYPE_VERSION,
                SCENE_MODULE,
                entity_ids(&NODES),
            ),
            design_type(
                SCENE_AUXILIARY_TYPE_GUID,
                Some(SCENE_AUXILIARY_BASE_TYPE_GUID),
                SCENE_AUXILIARY_TYPE_VERSION,
                SCENE_MODULE,
                entity_ids(&AUXILIARIES),
            ),
            design_type(
                MESH_COLLECTION_OWNER_TYPE_GUID,
                Some(MESH_COLLECTION_OWNER_BASE_TYPE_GUID),
                MESH_COLLECTION_OWNER_TYPE_VERSIONS[2],
                FUSION_MODULE,
                vec![u64::from(COLLECTION_OWNER)],
            ),
            design_type(
                MESH_BODY_OWNER_TYPE_GUID,
                Some(MESH_BODY_OWNER_BASE_TYPE_GUID),
                MESH_BODY_OWNER_TYPE_VERSION,
                "Body",
                vec![u64::from(BODY_OWNER)],
            ),
            design_type(
                MESH_TEXTURE_FILENAME_TYPE_GUID,
                Some(MESH_TEXTURE_FILENAME_BASE_TYPE_GUID),
                MESH_TEXTURE_FILENAME_TYPE_VERSION,
                "",
                filename_ids,
            ),
        ];
        assert_eq!(types.len(), 15);

        let mut bytes = Vec::new();
        let mut records = Vec::new();
        push_primary_frame(
            &mut bytes,
            &mut records,
            COLLECTION,
            mesh_collection_record(
                TAG_COLLECTION,
                TAG_COLLECTION_BASE,
                COLLECTION,
                TEXTURES,
                &BODIES[..body_count],
                COLLECTION_OWNER,
            ),
        );
        let flags = if with_textures {
            vec![(RESOURCE_A, 2), (RESOURCE_B, 258)]
        } else {
            Vec::new()
        };
        let filenames = if with_textures {
            vec![(RESOURCE_B, FILENAME_B), (RESOURCE_A, FILENAME_A)]
        } else {
            Vec::new()
        };
        push_primary_frame(
            &mut bytes,
            &mut records,
            TEXTURES,
            mesh_texture_table_record(TAG_TEXTURES, TEXTURES, &flags, &filenames),
        );
        if with_textures {
            push_primary_frame(
                &mut bytes,
                &mut records,
                FILENAME_A,
                texture_filename_record(TAG_FILENAME, FILENAME_A, "mesh-a.png"),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                FILENAME_B,
                texture_filename_record(TAG_FILENAME, FILENAME_B, "mesh-b.jpg"),
            );
        }
        for ordinal in 0..body_count {
            push_primary_frame(
                &mut bytes,
                &mut records,
                ENTRIES[ordinal],
                mesh_entry_record(
                    TAG_ENTRY,
                    ENTRIES[ordinal],
                    GUIDS[ordinal],
                    ENTRY_NAMES[ordinal],
                ),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                GUIDS[ordinal],
                mesh_guid_record(
                    TAG_GUID,
                    GUIDS[ordinal],
                    ENTRIES[ordinal],
                    FUSION_UUIDS[ordinal],
                ),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                STATES[ordinal],
                mesh_scene_state_record(TAG_STATE, STATES[ordinal]),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                AUXILIARIES[ordinal],
                identity_record(TAG_AUXILIARY, AUXILIARIES[ordinal]),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                NODES[ordinal],
                mesh_scene_node_record(
                    TAG_NODE,
                    NODES[ordinal],
                    STATES[ordinal],
                    AUXILIARIES[ordinal],
                ),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                WRAPPERS[ordinal],
                mesh_wrapper_record(TAG_WRAPPER, WRAPPERS[ordinal], BODIES[ordinal]),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                BODIES[ordinal],
                mesh_body_record(
                    TAG_BODY,
                    BODIES[ordinal],
                    GUIDS[ordinal],
                    SCOPE,
                    WRAPPERS[ordinal],
                    BODY_OWNER,
                    NODES[ordinal],
                    COLLECTION,
                ),
            );
        }
        push_primary_frame(
            &mut bytes,
            &mut records,
            SCOPE,
            mesh_scope_record(
                TAG_SCOPE,
                TAG_SCOPE_BASE,
                SCOPE,
                &BODIES[..body_count],
                140,
                SCOPE_OWNER,
            ),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            COLLECTION_OWNER,
            collection_owner_record(TAG_COLLECTION_OWNER, COLLECTION_OWNER, COLLECTION),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            BODY_OWNER,
            identity_record(TAG_BODY_OWNER, BODY_OWNER),
        );
        SyntheticMeshGraph {
            bytes,
            meta: crate::metastream::MetaStream {
                types,
                records,
                secondary_records: Vec::new(),
            },
        }
    }

    fn synthetic_mesh_graph(with_textures: bool) -> SyntheticMeshGraph {
        synthetic_mesh_graph_with_body_count(with_textures, 1)
    }

    fn sole_typed_frame<'a>(
        graph: &'a SyntheticMeshGraph,
        type_guid: &str,
    ) -> TypedPrimaryFrame<'a> {
        let frames = typed_primary_frames(&graph.bytes, &graph.meta, type_guid, "test record")
            .expect("typed test frame");
        let [frame] = frames.as_slice() else {
            panic!("one typed test frame");
        };
        *frame
    }

    #[test]
    fn typed_mesh_feature_graph_closes_every_body_join() {
        const ENTRY_NAME: &str = "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh";
        const FUSION_UUID: &str = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
        let graph = synthetic_mesh_graph(false);
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("complete typed mesh graph");
        let [feature] = design.as_slice() else {
            panic!("one mesh feature");
        };
        assert_eq!(feature.scope().record().record_index(), 109);
        assert_eq!(feature.collection().record().record_index(), 100);
        assert_eq!(feature.texture_table.record().record_index(), 101);
        assert_eq!(
            feature
                .bodies()
                .iter()
                .map(|body| body.placement.record().record_index())
                .collect::<Vec<_>>(),
            [104]
        );
        assert!(feature.texture_table.resources().is_empty());
        let [body] = feature.bodies() else {
            panic!("one mesh body");
        };
        assert_eq!(body.entry.name(), ENTRY_NAME);
        assert_eq!(body.guid.value(), FUSION_UUID);
        assert_eq!(body.wrapper_record.record_index(), 108);
        assert_eq!(body.scene_state.record().record_index(), 105);
        assert_eq!(body.scene_node.record_index(), 107);
        assert_eq!(body.scene_auxiliary_record.record_index(), 106);
        assert_eq!(body.owner_record.record_index(), 111);
        assert_eq!(
            crate::design::test_support::with_test_decode_context(|ctx| {
                resolve_mesh_body(ctx, &design, ENTRY_NAME, FUSION_UUID)
            })
            .expect("mesh body join"),
            Some((0, 0))
        );
    }

    #[test]
    fn empty_mesh_collection_does_not_enter_feature_graph() {
        let graph = synthetic_mesh_graph_with_body_count(false, 0);
        let mut no_asset = no_texture_asset;

        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("empty mesh registry");
        assert!(design.is_empty());
    }

    #[test]
    fn mesh_registrations_without_a_collection_do_not_form_a_graph() {
        let mut graph = synthetic_mesh_graph(false);
        let collection_type = graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_TYPE_GUID)
            })
            .expect("mesh-collection type");
        let entities = collection_type
            .entities
            .values()
            .copied()
            .collect::<Vec<_>>();
        let [collection_entity] = entities.as_slice() else {
            panic!("one mesh-collection entity");
        };
        let collection_entity = *collection_entity;
        collection_type.entities = crate::records::identity::ReferenceRun::unlocated(Vec::new());
        graph
            .meta
            .records
            .retain(|record| record.entity_id != collection_entity);
        let mut no_asset = no_texture_asset;

        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("no mesh collection");
        assert!(design.is_empty());
    }

    #[test]
    fn typed_mesh_feature_allows_bodies_to_share_one_body_owner() {
        let graph = synthetic_mesh_graph_with_body_count(false, 2);
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("two-body mesh feature graph");
        let [feature] = design.as_slice() else {
            panic!("one mesh feature");
        };
        let [first, second] = feature.bodies() else {
            panic!("two mesh bodies");
        };

        assert_eq!(
            feature
                .bodies()
                .iter()
                .map(|body| body.placement.record().record_index())
                .collect::<Vec<_>>(),
            [104, 117]
        );
        assert_eq!(first.owner_record, second.owner_record);
        assert_ne!(first.placement.record(), second.placement.record());
    }

    #[test]
    fn mesh_texture_maps_join_by_guid_independently_of_map_order() {
        let graph = synthetic_mesh_graph(true);
        let mut asset = |filename: &str| {
            let entry = format!("Synthetic/Textures/{filename}");
            Ok((entry.clone(), crate::ids::neutral_asset_id(&entry)))
        };
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut asset,
        )
        .expect("textured mesh graph");
        let textures = design[0].texture_table.resources();
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].flags, 2);
        assert_eq!(textures[0].file.filename(), "mesh-a.png");
        assert_eq!(textures[0].filename_ordinal, 1);
        assert_eq!(textures[1].flags, 258);
        assert_eq!(textures[1].file.filename(), "mesh-b.jpg");
        assert_eq!(textures[1].filename_ordinal, 0);
    }

    #[test]
    fn mesh_entry_and_texture_filename_refuse_retained_text_limits() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        const ENTRY_NAME: &str = "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh";
        let graph = synthetic_mesh_graph(false);
        let frame = sole_typed_frame(&graph, MESH_ENTRY_NAME_TYPE_GUID);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(ENTRY_NAME.len() - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_mesh_entry_name_record(&ctx, &graph.bytes, frame)
            .err()
            .unwrap();
        assert!(matches!(error, CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "f3d Design UTF-16 text"));

        let graph = synthetic_mesh_graph(true);
        let frames = typed_primary_frames(
            &graph.bytes,
            &graph.meta,
            MESH_TEXTURE_FILENAME_TYPE_GUID,
            "mesh-texture-filename",
        )
        .unwrap();
        let frame = frames[0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from("mesh-a.png".len() - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_mesh_texture_filename_record(&ctx, &graph.bytes, frame)
            .err()
            .unwrap();
        assert!(matches!(error, CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "f3d Design UTF-16 text"));
    }

    #[test]
    fn mesh_image_asset_identifier_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"PNG").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let expected = crate::ids::neutral_asset_id(ENTRY);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes =
                u64::try_from(ENTRY.len() + expected.as_str().len() - 1).unwrap();
            let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let refusal = super::mesh_image_asset(&limited, scan, "mark.png");
            assert!(matches!(
                refusal,
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == "f3d asset identifier"
            ));
            let admitted = super::mesh_image_asset(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                "mark.png",
            )
            .unwrap();
            assert_eq!(admitted.0, ENTRY);
            assert_eq!(admitted.1, expected);
        });
    }

    #[test]
    fn mesh_texture_resources_can_share_one_filename_record() {
        let mut graph = synthetic_mesh_graph(true);
        let frames = typed_primary_frames(
            &graph.bytes,
            &graph.meta,
            MESH_TEXTURE_TABLE_TYPE_GUID,
            "mesh-texture-table",
        )
        .expect("texture-table frame");
        let [frame] = frames.as_slice() else {
            panic!("one texture-table frame");
        };
        let table = crate::design::test_support::with_test_decode_context(|ctx| {
            parse_texture_table(ctx, &graph.bytes, *frame)
        })
        .expect("original texture table");
        let filename = |ordinal| {
            table
                .textures
                .iter()
                .filter_map(|texture| texture.filename)
                .find(|filename| filename.ordinal == ordinal)
                .expect("filename-map entry")
        };
        let (first, second) = (filename(0), filename(1));
        let second_reference = usize::try_from(
            table.identity.byte_offset()
                + u64_from_index(texture_table::LEN)
                + u64_from_index(table.textures.len()) * 44
                + 4
                + u64::from(second.ordinal) * 51
                + 40,
        )
        .expect("test reference offset");
        put_reference(&mut graph.bytes, second_reference, first.record_index);
        let mut asset = |filename: &str| {
            let entry = format!("Synthetic/Textures/{filename}");
            Ok((entry.clone(), crate::ids::neutral_asset_id(&entry)))
        };

        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut asset,
        )
        .expect("shared texture filename record");
        let textures = design[0].texture_table.resources();
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].file.record(), textures[1].file.record());
        assert_eq!(textures[0].asset, textures[1].asset);
    }

    #[test]
    fn mesh_graph_rejects_body_count_disagreement() {
        let mut graph = synthetic_mesh_graph(false);
        graph.bytes[mesh_collection::LEN + mesh_collection_base::BODY_COUNT
            ..mesh_collection::LEN + mesh_collection_base::BODY_COUNT + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        let mut no_asset = no_texture_asset;
        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_texture_table_rejects_duplicate_resource_keys() {
        const RESOURCE: &str = "10000000-0000-4000-8000-000000000001";
        let graph = synthetic_mesh_graph(false);
        let bytes = mesh_texture_table_record(
            261,
            101,
            &[(RESOURCE, 2), (RESOURCE, 258)],
            &[(RESOURCE, 113)],
        );
        let frame = TypedPrimaryFrame {
            entity_id: 101,
            start: 0,
            end: bytes.len(),
            design_type: &graph.meta.types[5],
        };
        assert!(matches!(
            crate::design::test_support::with_test_decode_context(|ctx| {
                parse_texture_table(ctx, &bytes, frame)
            }),
            Err(CodecError::Malformed(_))
        ));
    }

    fn texture_table_frame<'a>(
        graph: &'a SyntheticMeshGraph,
        bytes: &[u8],
    ) -> TypedPrimaryFrame<'a> {
        TypedPrimaryFrame {
            entity_id: 101,
            start: 0,
            end: bytes.len(),
            design_type: &graph.meta.types[5],
        }
    }

    #[test]
    fn mesh_texture_table_joins_maps_by_guid_without_case() {
        const LOWER: &str = "1000000a-0000-4000-8000-00000000000b";
        const UPPER: &str = "1000000A-0000-4000-8000-00000000000B";
        const OTHER: &str = "20000000-0000-4000-8000-000000000002";
        let graph = synthetic_mesh_graph(false);
        let joined = mesh_texture_table_record(261, 101, &[(LOWER, 2)], &[(UPPER, 113)]);
        let table = crate::design::test_support::with_test_decode_context(|ctx| {
            parse_texture_table(ctx, &joined, texture_table_frame(&graph, &joined))
        })
        .expect("GUID keys equal without case");
        let [texture] = table.textures.as_slice() else {
            panic!("one texture");
        };
        assert_eq!(texture.resource_guid.as_str(), LOWER);
        assert_eq!(texture.flags, 2);
        assert!(matches!(
            texture.filename,
            Some(super::MeshTextureFilenameEntry {
                ordinal: 0,
                record_index: 113
            })
        ));
        for (flags, filenames) in [
            (&[(LOWER, 2)][..], &[(OTHER, 113)][..]),
            (
                &[(LOWER, 2), (OTHER, 3)][..],
                &[(UPPER, 113), (UPPER, 114)][..],
            ),
            (&[(LOWER, 2), (OTHER, 3)][..], &[(UPPER, 113)][..]),
        ] {
            let bytes = mesh_texture_table_record(261, 101, flags, filenames);
            assert!(matches!(
                crate::design::test_support::with_test_decode_context(|ctx| {
                    parse_texture_table(ctx, &bytes, texture_table_frame(&graph, &bytes))
                }),
                Err(CodecError::Malformed(_))
            ));
        }
    }

    #[test]
    fn mesh_texture_table_charges_each_input_sized_collection() {
        const RESOURCE: &str = "10000000-0000-4000-8000-000000000001";
        let graph = synthetic_mesh_graph(false);
        let bytes = mesh_texture_table_record(261, 101, &[(RESOURCE, 2)], &[(RESOURCE, 113)]);
        let frame = texture_table_frame(&graph, &bytes);
        for operation in ["f3d mesh texture entries", "f3d mesh texture keys"] {
            let refusal = crate::test_support::resource_refusal_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                operation,
                0,
                |ctx| parse_texture_table(ctx, &bytes, frame).map(|_| ()),
            );
            assert!(matches!(
                refusal,
                CodecError::ResourceLimit(limit)
                    if limit.operation == operation && limit.additional == 1
            ));
        }
        crate::design::test_support::with_test_decode_context(|ctx| {
            let table = parse_texture_table(ctx, &bytes, frame).unwrap();
            assert_eq!(table.textures.len(), 1);
        });
    }

    #[test]
    fn mesh_collection_indices_refuse_collection_limit() {
        let graph = synthetic_mesh_graph(true);
        let collection = crate::design::test_support::with_test_decode_context(|ctx| {
            super::parse_mesh_collection_record(
                ctx,
                &graph.bytes,
                &graph.meta,
                sole_typed_frame(&graph, MESH_COLLECTION_TYPE_GUID),
            )
            .unwrap()
        });
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (indices_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = indices_ctx.reserve_scoped(0, "test indices").unwrap();
        assert!(matches!(
            super::mesh_collection_indices(&indices_ctx, &mut storage, &[collection]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && limit.operation == "f3d mesh collection indices"
        ));
    }

    #[test]
    fn mesh_graph_diagnostic_text_refuses_retained_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;

        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            super::mesh_graph_diagnostic(&ctx, format_args!("mesh links {:?}", [1, 2])),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && limit.operation == "f3d mesh graph diagnostic"
        ));
        crate::design::test_support::with_test_decode_context(|ctx| {
            let error =
                super::mesh_graph_diagnostic(ctx, format_args!("mesh links {:?}", [1, 2])).unwrap();
            assert!(
                matches!(error, CodecError::Malformed(message) if message == "mesh links [1, 2]")
            );
        });
    }

    #[test]
    fn mesh_feature_identifier_refuses_prefix_and_suffix_limits() {
        let stream = crate::ids::native_scope("Synthetic/BulkStream.dat");
        for (limit, operation) in [
            (0, "f3d mesh feature ID prefix"),
            (u64_from_index(stream.len()), "f3d mesh feature ID suffix"),
        ] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::default();
            policy.limits.max_retained_bytes = limit;

            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                super::mesh_feature_id_charged(&ctx, &stream, 100),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                        && failure.operation == operation
            ));
        }
        crate::design::test_support::with_test_decode_context(|ctx| {
            let id = super::mesh_feature_id_charged(ctx, &stream, 100).unwrap();
            assert_eq!(
                id,
                crate::ids::native_design_mesh_feature_id("Synthetic/BulkStream.dat", 100)
            );
        });
    }

    #[test]
    fn mesh_wrapper_rejects_a_truncated_exact_frame() {
        let graph = synthetic_mesh_graph(false);
        let bytes = mesh_wrapper_record(262, 108, 104);
        let frame = TypedPrimaryFrame {
            entity_id: 108,
            start: 0,
            end: bytes.len() - 1,
            design_type: &graph.meta.types[6],
        };
        assert!(matches!(
            crate::design::test_support::with_test_decode_context(|ctx| parse_mesh_wrapper_record(
                ctx, &bytes, frame
            )),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_scene_state_rejects_a_mutated_footer_mask() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, MESH_SCENE_STATE_TYPE_GUID).start;
        graph.bytes[start + 52] ^= 1;
        let frame = sole_typed_frame(&graph, MESH_SCENE_STATE_TYPE_GUID);

        assert!(matches!(
            crate::design::test_support::with_test_decode_context(|ctx| {
                parse_mesh_scene_state_record(ctx, &graph.bytes, frame)
            }),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_scene_node_rejects_a_mutated_fixed_lane() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID).start;
        graph.bytes[start + 44..start + 48].copy_from_slice(&4u32.to_le_bytes());
        let frame = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID);

        assert!(matches!(
            crate::design::test_support::with_test_decode_context(|ctx| parse_scene_node_record(
                ctx,
                &graph.bytes,
                frame
            )),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_scene_node_transfers_finite_footer_bounds() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID).start;
        let payload_at = start + scene_node::FOOTER_MARKER + 1;
        let values: [f64; 6] = [1.0, 2.0, 3.0, -4.0, -5.0, -6.0];
        for (ordinal, value) in values.iter().enumerate() {
            let at = payload_at + ordinal * 8;
            graph.bytes[at..at + 8].copy_from_slice(&(*value).to_le_bytes());
        }
        graph.bytes[payload_at + 48] = 1;
        let frame = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID);

        let parsed = crate::design::test_support::with_test_decode_context(|ctx| {
            parse_scene_node_record(ctx, &graph.bytes, frame)
        })
        .expect("finite Scene bounds");
        let bounds = parsed.node.bounds().expect("present bounds");
        assert_eq!(bounds.maximum(), [1.0, 2.0, 3.0]);
        assert_eq!(bounds.minimum(), [-4.0, -5.0, -6.0]);
        assert_eq!(
            parsed.node.bounds_offsets(),
            [
                u64::try_from(payload_at).unwrap(),
                u64::try_from(payload_at + 24).unwrap()
            ]
        );
    }

    #[test]
    fn mesh_scene_node_transfers_placed_transform_and_bounds() {
        let graph = synthetic_mesh_graph(false);
        let transform = [
            -1.0, 0.0, 0.0, 4.0, 0.0, -1.0, 0.0, 5.0, 0.0, 0.0, 1.0, 6.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let bytes = placed_mesh_scene_node_record(
            267,
            107,
            105,
            106,
            transform,
            [4.0, 5.0, 6.0, 1.0, 2.0, 3.0],
        );
        let frame = TypedPrimaryFrame {
            entity_id: 107,
            start: 0,
            end: bytes.len(),
            design_type: graph
                .meta
                .types
                .iter()
                .find(|design_type| {
                    design_type
                        .type_guid
                        .as_str()
                        .eq_ignore_ascii_case(SCENE_NODE_TYPE_GUID)
                })
                .expect("Scene-node type"),
        };

        let parsed = crate::design::test_support::with_test_decode_context(|ctx| {
            parse_scene_node_record(ctx, &bytes, frame)
        })
        .expect("placed Scene node");
        assert_eq!(
            parsed
                .node
                .transform()
                .expect("placed transform")
                .value
                .cells(),
            transform
        );
        assert_eq!(
            parsed.node.transform().map(|located| located.offset),
            Some(u64_from_index(placed_scene_node::TRANSFORM))
        );
        let bounds = parsed.node.bounds().expect("placed bounds");
        assert_eq!(bounds.maximum(), [4.0, 5.0, 6.0]);
        assert_eq!(bounds.minimum(), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn mesh_graph_rejects_a_nonreciprocal_collection_owner() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, MESH_COLLECTION_OWNER_TYPE_GUID).start;
        put_reference(
            &mut graph.bytes,
            start + collection_owner::COLLECTION_BACKLINK,
            101,
        );
        let mut no_asset = no_texture_asset;

        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_graph_rejects_reused_scene_auxiliary() {
        let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
        let frames = typed_primary_frames(
            &graph.bytes,
            &graph.meta,
            SCENE_NODE_TYPE_GUID,
            "mesh-scene-node",
        )
        .unwrap();
        let second = frames.iter().find(|frame| frame.entity_id == 120).unwrap();
        put_reference(
            &mut graph.bytes,
            second.start + scene_node::AUXILIARY_RECORD_REFERENCE,
            106,
        );
        let mut no_asset = no_texture_asset;
        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_collection_owner_accepts_legacy_version_fifteen() {
        const EXPECTED_COLLECTION: u32 = 100;
        let mut graph = synthetic_mesh_graph(false);
        graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type")
            .version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[0];
        let design_type = graph
            .meta
            .types
            .iter()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type");
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, 270, 110);
        bytes.resize(859, 0);
        push_reference(&mut bytes, EXPECTED_COLLECTION);
        let frame = TypedPrimaryFrame {
            entity_id: 110,
            start: 0,
            end: bytes.len(),
            design_type,
        };

        let owner = crate::design::test_support::with_test_decode_context(|ctx| {
            parse_mesh_collection_owner_record(ctx, &bytes, frame)
        })
        .expect("valid legacy owner frame")
        .expect("legacy collection owner");
        assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
        assert_eq!(owner.owner.backlink_offset(), 859);
    }

    #[test]
    fn mesh_collection_owner_accepts_version_seventeen_frame() {
        const EXPECTED_COLLECTION: u32 = 100;
        let mut graph = synthetic_mesh_graph(false);
        let design_type = graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type");
        design_type.version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[1];
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, 270, 110);
        bytes.resize(collection_owner_v17::COLLECTION_BACKLINK, 0);
        push_reference(&mut bytes, EXPECTED_COLLECTION);
        bytes.extend_from_slice(&[0; 64]);
        let frame = TypedPrimaryFrame {
            entity_id: 110,
            start: 0,
            end: bytes.len(),
            design_type,
        };

        let owner = crate::design::test_support::with_test_decode_context(|ctx| {
            parse_mesh_collection_owner_record(ctx, &bytes, frame)
        })
        .expect("valid version-17 owner frame")
        .expect("version-17 collection owner");
        assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
        assert_eq!(
            owner.owner.backlink_offset(),
            u64_from_index(collection_owner_v17::COLLECTION_BACKLINK)
        );
    }

    #[test]
    fn mesh_collection_owner_ignores_non_owner_class_members() {
        let mut graph = synthetic_mesh_graph(false);
        let design_type = graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type");
        design_type.version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[1];
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, 270, 110);
        bytes.resize(collection_owner_v17::LEN, 0);
        let frame = TypedPrimaryFrame {
            entity_id: 110,
            start: 0,
            end: bytes.len(),
            design_type,
        };

        assert!(
            crate::design::test_support::with_test_decode_context(|ctx| {
                parse_mesh_collection_owner_record(ctx, &bytes, frame)
            })
            .expect("valid generic owner frame")
            .is_none()
        );
    }

    #[test]
    fn mesh_graph_rejects_disagreeing_ordered_body_lists() {
        let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
        let start = sole_typed_frame(&graph, MESH_FEATURE_SCOPE_TYPE_GUID).start;
        put_reference(&mut graph.bytes, start + 36, 104);
        let mut no_asset = no_texture_asset;

        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_join_rejects_multiple_typed_body_candidates() {
        let graph = synthetic_mesh_graph(false);
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("mesh graph");
        let joined = crate::design::test_support::with_test_decode_context(|ctx| {
            resolve_mesh_body(
                ctx,
                &[design.clone(), design].concat(),
                "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh",
                "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
            )
        })
        .expect("ambiguous mesh join result");
        assert!(joined.is_none());
    }

    #[test]
    fn mesh_body_projection_refuses_identifier_and_collection_limits() {
        let transform = crate::records::mesh::MeshAffineTransform::new([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
        .unwrap();
        let container = || MeshContainer {
            fusion_uuid: "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into(),
            mesh_uuid: crate::records::mesh::DesignMeshUuid::try_from(
                "11111111-2222-4333-8444-555555555555".to_owned(),
            )
            .unwrap(),
            vertices: vec![FinitePoint3::ZERO],
            triangles: Vec::new(),
            feature_edges: Vec::new(),
            corner_normals: Some(vec![UnitVector3::Z_AXIS]),
            triangle_groups: Vec::new(),
            texture_ids: None,
            attributes: Vec::new(),
        };
        let geometry = || super::split_container(container()).2;
        let native_scope_bytes = u64_from_index(crate::ids::native_scope("mesh.paramesh").len());
        for (collection_limit, retained_limit, dimension, operation) in [
            (
                0,
                u64::MAX,
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                "f3d placed mesh vertices",
            ),
            (
                1,
                u64::MAX,
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                "f3d placed mesh normals",
            ),
            (
                2,
                0,
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                "f3d native stream key",
            ),
            (
                2,
                native_scope_bytes,
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                "f3d mesh body identifier",
            ),
        ] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::default();
            policy.limits.max_collection_items = collection_limit;
            policy.limits.max_retained_bytes = retained_limit;

            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                MeshBody::from_geometry(&ctx, "mesh.paramesh", 100, transform, geometry()),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == dimension && failure.operation == operation
            ));
        }
        crate::design::test_support::with_test_decode_context(|ctx| {
            let body =
                MeshBody::from_geometry(ctx, "mesh.paramesh", 100, transform, geometry()).unwrap();
            assert_eq!(
                body.id,
                crate::ids::native_mesh_body_id("mesh.paramesh", 100)
            );
            assert_eq!(body.vertices.len(), 1);
            assert_eq!(body.corner_normals.unwrap().len(), 1);
        });
    }

    #[test]
    fn mesh_outcome_and_text_copy_refuse_caller_limits() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut collection_policy = cadmpeg_core::decode::DecodePolicy::default();
        collection_policy.limits.max_collection_items = 0;
        let (collection_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &collection_policy)
                .unwrap();
        assert!(matches!(
            collection_ctx.push_vec(&mut Vec::new(), super::MeshContainerOutcome::Missing { entry_name: String::new() }, "f3d mesh container outcomes"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && limit.operation == "f3d mesh container outcomes"
        ));
        let mut retained_policy = cadmpeg_core::decode::DecodePolicy::default();
        retained_policy.limits.max_retained_bytes = 0;
        let (retained_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &retained_policy)
                .unwrap();
        assert!(matches!(
            retained_ctx.copy_retained_text("mesh.paramesh", "f3d test mesh text"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && limit.operation == "f3d test mesh text"
        ));
    }

    #[test]
    fn mesh_frame_diagnostic_refuses_retained_limit() {
        let expected = "F3D Design mesh-wrapper entity 108 has an invalid primary frame";
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::malformed_frame(&ctx, "mesh-wrapper", 108),
            CodecError::ResourceLimit(failure) if failure.operation == "f3d Design diagnostic"));
    }

    #[test]
    fn mesh_graph_invariant_diagnostic_refuses_retained_limit() {
        let expected =
            "F3D Design mesh feature graph violates `member` in Synthetic/BulkStream.dat";
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(super::malformed_mesh_graph(&ctx, "Synthetic/BulkStream.dat", "member"),
            CodecError::ResourceLimit(failure) if failure.operation == "f3d Design diagnostic")
        );
    }

    #[test]
    fn mesh_invalid_record_index_diagnostic_refuses_retained_limit() {
        let graph = synthetic_mesh_graph(false);
        let frame = sole_typed_frame(&graph, MESH_WRAPPER_TYPE_GUID);
        let expected = format!(
            "F3D Design mesh-wrapper entity {} has an invalid record index",
            frame.entity_id
        );
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();

        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(super::exact_record_index(&ctx, &[], frame, "mesh-wrapper"),
            Err(CodecError::ResourceLimit(failure)) if failure.operation == "f3d Design diagnostic")
        );
    }

    #[test]
    fn mesh_unsupported_version_diagnostic_refuses_retained_limit() {
        let graph = synthetic_mesh_graph(false);
        let frame = sole_typed_frame(&graph, MESH_WRAPPER_TYPE_GUID);
        let expected = format!(
            "F3D Design mesh-wrapper record version {} is unsupported",
            frame.design_type.version
        );
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();

        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(super::validate_mesh_registration(&ctx, frame, frame.design_type.version + 1,
            MESH_WRAPPER_BASE_TYPE_GUID, PARAMESH_MODULE, "mesh-wrapper"),
            Err(CodecError::ResourceLimit(failure)) if failure.operation == "f3d Design unsupported diagnostic")
        );
    }
}
