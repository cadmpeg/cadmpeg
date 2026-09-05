// SPDX-License-Identifier: Apache-2.0
//! Join mesh-geometry containers to their bodies through the Design segment.
//!
//! A mesh body's geometry lives in a `.paramesh` container ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)),
//! and a typed Design graph joins the container, mesh body, owning feature,
//! optional texture resources, and Scene state ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).

use cadmpeg_core::container::ContainerRole;

use crate::bytes::{is_guid_hyphenated, lp_ascii_strict, lp_utf16_bounded, take_reference};
use crate::container::ContainerScan;
use crate::design::decode::meta::{
    metadata_for_bulk_stream, typed_primary_frames, TypedPrimaryFrame,
};
use crate::design::decode::scopes::parse_parameter_scope;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids;
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
use crate::records::{
    DesignMeshBody, DesignMeshFeature, DesignMeshRecordIdentity, DesignMeshSceneBounds,
    DesignMeshTextureResource, DesignRecordHeader,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
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

/// One mesh body's geometry, in model millimetres.
pub(crate) struct MeshBody {
    /// Deterministic native identifier, keyed by the mesh-body record.
    pub(crate) id: String,
    /// Vertex positions in model millimetres.
    pub(crate) vertices: Vec<cadmpeg_ir::math::Point3>,
    /// Triangle corner indices into `vertices`.
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Source-classified feature edges as ascending vertex-index pairs.
    pub(crate) feature_edges: Vec<[u32; 2]>,
    /// One transformed unit normal per flattened triangle corner.
    pub(crate) corner_normals: Vec<cadmpeg_ir::math::Vector3>,
    /// Source face groups as an ordered partition of triangle ordinals.
    pub(crate) triangle_groups: Vec<crate::paramesh::MeshTriangleGroup>,
    /// One texture-table selector per triangle, when authored.
    pub(crate) texture_ids: Option<Vec<u32>>,
    /// The attribute channels the container's registry declares.
    pub(crate) attributes: Vec<crate::paramesh::MeshAttribute>,
}

/// A finite, nonsingular row-major affine map.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MeshAffineTransform([f64; 16]);

impl MeshAffineTransform {
    fn parse(bytes: &[u8], at: usize) -> Option<Self> {
        let mut cells = [0.0; 16];
        for (index, cell) in cells.iter_mut().enumerate() {
            *cell = View::f64_le_at(bytes, at.checked_add(index.checked_mul(8)?)?)?;
        }
        let transform = Self(cells);
        valid_mesh_transform(transform.rows()).then_some(transform)
    }

    fn transform_point(self, point: [f64; 3]) -> Result<cadmpeg_ir::math::Point3, CodecError> {
        let [x, y, z] = point;
        let cells = self.0;
        let point = cadmpeg_ir::math::Point3::new(
            (cells[0] * x + cells[1] * y + cells[2] * z + cells[3])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[4] * x + cells[5] * y + cells[6] * z + cells[7])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[8] * x + cells[9] * y + cells[10] * z + cells[11])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
        );
        if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
            return Err(CodecError::Malformed(
                "F3D mesh placement produces a non-finite vertex".into(),
            ));
        }
        Ok(point)
    }

    /// Transform an oriented surface normal with the cofactor of the linear
    /// map. The determinant sign keeps the normal aligned with the unchanged
    /// triangle tuple under a reflection.
    fn transform_normal(self, normal: [f64; 3]) -> Result<cadmpeg_ir::math::Vector3, CodecError> {
        let cells = self.0;
        let [x, y, z] = normal;
        let transformed = [
            (cells[5] * cells[10] - cells[6] * cells[9]) * x
                + (cells[6] * cells[8] - cells[4] * cells[10]) * y
                + (cells[4] * cells[9] - cells[5] * cells[8]) * z,
            (cells[2] * cells[9] - cells[1] * cells[10]) * x
                + (cells[0] * cells[10] - cells[2] * cells[8]) * y
                + (cells[1] * cells[8] - cells[0] * cells[9]) * z,
            (cells[1] * cells[6] - cells[2] * cells[5]) * x
                + (cells[2] * cells[4] - cells[0] * cells[6]) * y
                + (cells[0] * cells[5] - cells[1] * cells[4]) * z,
        ];
        let scale = transformed
            .iter()
            .map(|component| component.abs())
            .fold(0.0f64, f64::max);
        if !scale.is_finite() || scale == 0.0 {
            return Err(CodecError::Malformed(
                "F3D mesh placement produces a degenerate normal".into(),
            ));
        }
        let scaled = transformed.map(|component| component / scale);
        let length = (scaled[0] * scaled[0] + scaled[1] * scaled[1] + scaled[2] * scaled[2]).sqrt();
        if !length.is_finite() || length <= f64::EPSILON {
            return Err(CodecError::Malformed(
                "F3D mesh placement produces a degenerate normal".into(),
            ));
        }
        Ok(cadmpeg_ir::math::Vector3::new(
            scaled[0] / length,
            scaled[1] / length,
            scaled[2] / length,
        ))
    }

    fn rows(self) -> [[f64; 4]; 4] {
        let cells = self.0;
        [
            [cells[0], cells[1], cells[2], cells[3]],
            [cells[4], cells[5], cells[6], cells[7]],
            [cells[8], cells[9], cells[10], cells[11]],
            [cells[12], cells[13], cells[14], cells[15]],
        ]
    }

    fn from_rows(rows: [[f64; 4]; 4]) -> Self {
        Self([
            rows[0][0], rows[0][1], rows[0][2], rows[0][3], rows[1][0], rows[1][1], rows[1][2],
            rows[1][3], rows[2][0], rows[2][1], rows[2][2], rows[2][3], rows[3][0], rows[3][1],
            rows[3][2], rows[3][3],
        ])
    }
}

/// Whether a row-major mesh placement is finite, affine, and nonsingular.
pub(crate) fn valid_mesh_transform(transform: [[f64; 4]; 4]) -> bool {
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return false;
    }
    let determinant = transform[0][0]
        * (transform[1][1] * transform[2][2] - transform[1][2] * transform[2][1])
        - transform[0][1] * (transform[1][0] * transform[2][2] - transform[1][2] * transform[2][0])
        + transform[0][2] * (transform[1][0] * transform[2][1] - transform[1][1] * transform[2][0]);
    determinant.is_finite() && determinant != 0.0
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshEntryNameRecord {
    identity: DesignMeshRecordIdentity,
    guid_record_index: u32,
    entry_name: String,
    guid_reference_offset: u64,
    entry_name_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshGuidRecord {
    identity: DesignMeshRecordIdentity,
    entry_name_record_index: u32,
    fusion_uuid: String,
    fusion_uuid_offset: u64,
    entry_reference_offset: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct MeshBodyRecord {
    identity: DesignMeshRecordIdentity,
    guid_record_index: u32,
    scope_record_index: u32,
    wrapper_record_index: u32,
    owner_record_index: u32,
    scene_node_record_index: u32,
    collection_record_index: u32,
    transform: MeshAffineTransform,
    scope_reference_offset: u64,
    wrapper_reference_offset: u64,
    owner_reference_offset: u64,
    guid_reference_offset: u64,
    scene_node_reference_offset: u64,
    collection_reference_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshCollectionRecord {
    identity: DesignMeshRecordIdentity,
    base_record: DesignMeshRecordIdentity,
    texture_table_record_index: u32,
    body_record_indices: Vec<u32>,
    count_offsets: [u64; 2],
    body_reference_offsets: Vec<u64>,
    texture_reference_offset: u64,
    owner_record_index: u32,
    owner_reference_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshTextureMapEntry {
    ordinal: u32,
    resource_guid: String,
    guid_offset: u64,
    value: u32,
    value_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshTextureFilenameEntry {
    ordinal: u32,
    resource_guid: String,
    guid_offset: u64,
    filename_record_index: u32,
    reference_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshTextureTableRecord {
    identity: DesignMeshRecordIdentity,
    flags_count_offset: u64,
    filename_count_offset: u64,
    flags: Vec<MeshTextureMapEntry>,
    filenames: Vec<MeshTextureFilenameEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshWrapperRecord {
    identity: DesignMeshRecordIdentity,
    body_record_index: u32,
    body_reference_offset: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct MeshSceneNodeRecord {
    identity: DesignMeshRecordIdentity,
    bounds: Option<DesignMeshSceneBounds>,
    transform: Option<crate::records::Located<MeshAffineTransform>>,
    state_record_index: u32,
    state_reference_offset: u64,
    auxiliary_record_index: u32,
    auxiliary_reference_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshScopeRecord {
    identity: DesignMeshRecordIdentity,
    base_record: DesignMeshRecordIdentity,
    body_record_indices: Vec<u32>,
    body_count_offset: u64,
    body_reference_offsets: Vec<u64>,
    owner_record_index: u32,
    owner_reference_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshCollectionOwnerRecord {
    identity: DesignMeshRecordIdentity,
    collection_record_index: u32,
    collection_reference_offset: u64,
}

impl MeshBody {
    /// Project one decoded container through its joined Design body record.
    fn from_container(
        entry_name: &str,
        body_byte_offset: u64,
        transform: MeshAffineTransform,
        container: MeshContainer,
    ) -> Result<Self, CodecError> {
        let MeshContainer {
            fusion_uuid: _,
            mesh_uuid: _,
            vertices,
            triangles,
            feature_edges,
            corner_normals,
            triangle_groups,
            texture_ids,
            attributes,
        } = container;
        Ok(Self {
            id: ids::native_mesh_body_id(entry_name, body_byte_offset),
            vertices: vertices
                .into_iter()
                .map(|point| transform.transform_point(point))
                .collect::<Result<_, _>>()?,
            // Placement does not change indexing. Triangle tuples, feature
            // edges, and corner selectors remain in serialized order.
            triangles,
            feature_edges,
            corner_normals: corner_normals
                .into_iter()
                .map(|normal| transform.transform_normal(normal))
                .collect::<Result<_, _>>()?,
            triangle_groups,
            texture_ids,
            attributes,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct MeshDesignRecords {
    features: Vec<DesignMeshFeature>,
}

fn validate_mesh_registration(
    frame: TypedPrimaryFrame<'_>,
    expected_version: u32,
    expected_base_type_guid: &str,
    expected_module: &str,
    record_kind: &str,
) -> Result<(), CodecError> {
    if frame.design_type.version != expected_version {
        return Err(CodecError::NotImplemented(format!(
            "F3D Design {record_kind} record version {} is unsupported",
            frame.design_type.version
        )));
    }
    if frame.design_type.module != expected_module
        || !frame
            .design_type
            .base_type_guid.as_ref().map(|field| field.value.as_str())
            .is_some_and(|base| base.eq_ignore_ascii_case(expected_base_type_guid))
    {
        return Err(CodecError::malformed(format_args!(
            "F3D Design {record_kind} entity {} has incompatible registration metadata",
            frame.entity_id
        )));
    }
    Ok(())
}

fn exact_record_index(
    record: &[u8],
    frame: TypedPrimaryFrame<'_>,
    record_kind: &str,
) -> Result<u32, CodecError> {
    View::u32_le_at(record, 7)
        .filter(|record_index| u64::from(*record_index) == frame.entity_id)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D Design {record_kind} entity {} has an invalid record index",
                frame.entity_id
            ))
        })
}

fn malformed_frame(record_kind: &str, entity_id: u64) -> CodecError {
    CodecError::malformed(format_args!(
        "F3D Design {record_kind} entity {entity_id} has an invalid primary frame"
    ))
}

fn source_offset(frame_start: usize, relative: usize) -> Option<u64> {
    u64::try_from(frame_start.checked_add(relative)?).ok()
}

fn indexed_class_tag(record: &[u8], at: usize) -> Option<String> {
    (View::u32_le_at(record, at) == Some(3)).then_some(())?;
    let tag = std::str::from_utf8(record.get(at.checked_add(4)?..at.checked_add(7)?)?).ok()?;
    (tag.len() == 3 && tag.bytes().all(|byte| byte.is_ascii_digit())).then(|| tag.to_owned())
}

fn record_identity(
    record: &[u8],
    frame: TypedPrimaryFrame<'_>,
    record_kind: &str,
) -> Result<DesignMeshRecordIdentity, CodecError> {
    let record_index = exact_record_index(record, frame, record_kind)?;
    let class_tag = indexed_class_tag(record, 0)
        .ok_or_else(|| malformed_frame(record_kind, frame.entity_id))?;
    Ok(DesignMeshRecordIdentity {
        class_tag,
        record_index,
        byte_offset: u64::try_from(frame.start)
            .map_err(|_| malformed_frame(record_kind, frame.entity_id))?,
        frame_length: u64::try_from(frame.end.saturating_sub(frame.start))
            .map_err(|_| malformed_frame(record_kind, frame.entity_id))?,
    })
}

fn validate_design_type(
    design_type: &crate::records::SegmentType,
    expected_type_guid: &str,
    expected_base_type_guid: &str,
    expected_version: u32,
    expected_module: &str,
) -> bool {
    design_type
        .type_guid
        .eq_ignore_ascii_case(expected_type_guid)
        && design_type.version == expected_version
        && design_type.module == expected_module
        && design_type
            .base_type_guid.as_ref().map(|field| field.value.as_str())
            .is_some_and(|base| base.eq_ignore_ascii_case(expected_base_type_guid))
}

#[allow(clippy::too_many_arguments)]
fn nested_record_identity(
    record: &[u8],
    frame_start: usize,
    at: usize,
    end: usize,
    record_index: u32,
    meta: &crate::metastream::MetaStream,
    expected_type_guid: &str,
    expected_base_type_guid: &str,
    expected_version: u32,
    expected_module: &str,
) -> Option<DesignMeshRecordIdentity> {
    let class_tag = indexed_class_tag(record, at)?;
    (View::u32_le_at(record, at.checked_add(indexed_header::RECORD_INDEX)?) == Some(record_index))
        .then_some(())?;
    let tag = class_tag.parse::<u32>().ok()?;
    let ordinal = usize::try_from(tag.checked_sub(256)?).ok()?;
    validate_design_type(
        meta.types.get(ordinal)?,
        expected_type_guid,
        expected_base_type_guid,
        expected_version,
        expected_module,
    )
    .then_some(())?;
    Some(DesignMeshRecordIdentity {
        class_tag,
        record_index,
        byte_offset: source_offset(frame_start, at)?,
        frame_length: u64::try_from(end.checked_sub(at)?).ok()?,
    })
}

fn exact_local_record_index(record: &[u8], at: usize) -> Option<u32> {
    let mut cursor = at;
    let reference = take_reference(record, &mut cursor)?;
    if cursor != at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)?
        || reference.segment.is_some()
        || reference.link_name.is_some()
    {
        return None;
    }
    u32::try_from(reference.target?)
        .ok()
        .filter(|target| *target != 0)
}

fn counted_local_record_indices(
    record: &[u8],
    count_at: usize,
) -> Option<(Vec<u32>, Vec<usize>, usize)> {
    let count = usize::try_from(View::u32_le_at(record, count_at)?).ok()?;
    let mut at = count_at.checked_add(4)?;
    (count <= record.len().saturating_sub(at) / SAME_SEGMENT_REFERENCE_BYTES).then_some(())?;
    let mut indices = Vec::with_capacity(count);
    let mut offsets = Vec::with_capacity(count);
    for _ in 0..count {
        indices.push(exact_local_record_index(record, at)?);
        offsets.push(at);
        at = at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)?;
    }
    Some((indices, offsets, at))
}

fn absolute_offsets(frame_start: usize, offsets: Vec<usize>) -> Option<Vec<u64>> {
    offsets
        .into_iter()
        .map(|offset| source_offset(frame_start, offset))
        .collect()
}

fn parse_mesh_entry_name_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshEntryNameRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_ENTRY_NAME_TYPE_VERSION,
        MESH_ENTRY_NAME_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-entry-name",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-entry-name")?;
    let parsed = (|| {
        (record.get(entry_name_prefix::ZERO_RUN_10..entry_name_prefix::GUID_RECORD_REFERENCE)
            == Some(&[0; 10]))
        .then_some(())?;
        let guid_record_index =
            exact_local_record_index(record, entry_name_prefix::GUID_RECORD_REFERENCE)?;
        let (entry_name, end) = lp_utf16_bounded(record, entry_name_prefix::LEN, 1..=1024)?;
        (end == record.len()).then_some(MeshEntryNameRecord {
            identity,
            guid_record_index,
            entry_name,
            guid_reference_offset: source_offset(
                frame.start,
                entry_name_prefix::GUID_RECORD_REFERENCE,
            )?,
            entry_name_offset: source_offset(frame.start, entry_name_prefix::LEN.checked_add(4)?)?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-entry-name", frame.entity_id))
}

fn parse_mesh_guid_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshGuidRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_GUID_TYPE_VERSION,
        MESH_GUID_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-GUID",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-GUID")?;
    let parsed = (|| {
        (record.get(guid_join::ZERO_RUN_21..guid_join::FUSION_UUID) == Some(&[0; 21]))
            .then_some(())?;
        let (fusion_uuid, end) = lp_ascii_strict(record, guid_join::FUSION_UUID, 36..=36)?;
        (end == guid_join::ENTRY_NAME_BACKLINK && is_guid_hyphenated(&fusion_uuid)).then_some(())?;
        let entry_name_record_index =
            exact_local_record_index(record, guid_join::ENTRY_NAME_BACKLINK)?;
        Some(MeshGuidRecord {
            identity,
            entry_name_record_index,
            fusion_uuid,
            fusion_uuid_offset: source_offset(frame.start, guid_join::FUSION_UUID.checked_add(4)?)?,
            entry_reference_offset: source_offset(frame.start, guid_join::ENTRY_NAME_BACKLINK)?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-GUID", frame.entity_id))
}

fn parse_mesh_body_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshBodyRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_BODY_TYPE_VERSION,
        MESH_BODY_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-body",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-body")?;
    let parsed = (|| {
        (record.get(mesh_body::ZERO_RUN_10..mesh_body::ZERO_RUN_10 + 10) == Some(&[0; 10]))
            .then_some(())?;
        let scope_record_index =
            exact_local_record_index(record, mesh_body::FEATURE_SCOPE_REFERENCE)?;
        let wrapper_record_index = exact_local_record_index(record, mesh_body::WRAPPER_REFERENCE)?;
        let owner_record_index = exact_local_record_index(record, mesh_body::BODY_OWNER_REFERENCE)?;
        let guid_record_index =
            exact_local_record_index(record, mesh_body::CONTAINER_GUID_REFERENCE)?;
        let scene_node_record_index =
            exact_local_record_index(record, mesh_body::SCENE_NODE_REFERENCE)?;
        let collection_reference_at = record.len().checked_sub(SAME_SEGMENT_REFERENCE_BYTES)?;
        (collection_reference_at
            >= mesh_body::SCENE_NODE_REFERENCE.checked_add(SAME_SEGMENT_REFERENCE_BYTES)?)
        .then_some(())?;
        let collection_record_index = exact_local_record_index(record, collection_reference_at)?;
        let transform = mesh_body_transform(record)?;
        Some(MeshBodyRecord {
            identity,
            guid_record_index,
            scope_record_index,
            wrapper_record_index,
            owner_record_index,
            scene_node_record_index,
            collection_record_index,
            transform,
            scope_reference_offset: source_offset(frame.start, mesh_body::FEATURE_SCOPE_REFERENCE)?,
            wrapper_reference_offset: source_offset(frame.start, mesh_body::WRAPPER_REFERENCE)?,
            owner_reference_offset: source_offset(frame.start, mesh_body::BODY_OWNER_REFERENCE)?,
            guid_reference_offset: source_offset(frame.start, mesh_body::CONTAINER_GUID_REFERENCE)?,
            scene_node_reference_offset: source_offset(
                frame.start,
                mesh_body::SCENE_NODE_REFERENCE,
            )?,
            collection_reference_offset: source_offset(frame.start, collection_reference_at)?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-body", frame.entity_id))
}

fn parse_mesh_collection_record(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshCollectionRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_COLLECTION_TYPE_VERSION,
        MESH_COLLECTION_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-collection",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-collection")?;
    let parsed = (|| {
        (record.get(mesh_collection::ZERO_RUN_10..mesh_collection::BODY_COUNT) == Some(&[0; 10]))
            .then_some(())?;
        (record.get(mesh_collection::CONSTANT_01_01..mesh_collection::TEXTURE_TABLE_REFERENCE)
            == Some(&[1, 1]))
        .then_some(())?;
        let first_count =
            usize::try_from(View::u32_le_at(record, mesh_collection::BODY_COUNT)?).ok()?;
        let texture_table_record_index =
            exact_local_record_index(record, mesh_collection::TEXTURE_TABLE_REFERENCE)?;
        let base_record = nested_record_identity(
            record,
            frame.start,
            mesh_collection::LEN,
            record.len(),
            identity.record_index,
            meta,
            MESH_COLLECTION_BASE_TYPE_GUID,
            MESH_COLLECTION_BASE_BASE_TYPE_GUID,
            MESH_COLLECTION_BASE_TYPE_VERSION,
            COMMON_DATA_MODULE,
        )?;
        (record.get(
            mesh_collection::LEN + mesh_collection_base::ZERO_RUN_9
                ..mesh_collection::LEN + mesh_collection_base::BODY_COUNT,
        ) == Some(&[0; 9]))
        .then_some(())?;
        let (body_record_indices, body_reference_offsets, owner_at) = counted_local_record_indices(
            record,
            mesh_collection::LEN + mesh_collection_base::BODY_COUNT,
        )?;
        (first_count == body_record_indices.len()).then_some(())?;
        let owner_record_index = exact_local_record_index(record, owner_at)?;
        (owner_at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)? == record.len()).then_some(())?;
        Some(MeshCollectionRecord {
            identity,
            base_record,
            texture_table_record_index,
            body_record_indices,
            count_offsets: [
                source_offset(frame.start, mesh_collection::BODY_COUNT)?,
                source_offset(
                    frame.start,
                    mesh_collection::LEN + mesh_collection_base::BODY_COUNT,
                )?,
            ],
            body_reference_offsets: absolute_offsets(frame.start, body_reference_offsets)?,
            texture_reference_offset: source_offset(
                frame.start,
                mesh_collection::TEXTURE_TABLE_REFERENCE,
            )?,
            owner_record_index,
            owner_reference_offset: source_offset(frame.start, owner_at)?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-collection", frame.entity_id))
}

fn parse_mesh_texture_table_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshTextureTableRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_TEXTURE_TABLE_TYPE_VERSION,
        MESH_TEXTURE_TABLE_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-texture-table",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-texture-table")?;
    let parsed = (|| {
        (record.get(texture_table::ZERO_RUN_10..texture_table::FLAGS_MAP_COUNT) == Some(&[0; 10]))
            .then_some(())?;
        let flags_count =
            usize::try_from(View::u32_le_at(record, texture_table::FLAGS_MAP_COUNT)?).ok()?;
        let mut at = texture_table::FLAGS_MAP_COUNT.checked_add(4)?;
        let mut flags = Vec::with_capacity(flags_count);
        let mut flag_keys = HashSet::with_capacity(flags_count);
        for ordinal in 0..flags_count {
            let guid_at = at;
            let (resource_guid, end) = lp_ascii_strict(record, at, 36..=36)?;
            (is_guid_hyphenated(&resource_guid)
                && flag_keys.insert(resource_guid.to_ascii_uppercase()))
            .then_some(())?;
            at = end;
            let value_offset = at;
            let value = View::u32_le_at(record, at)?;
            at = at.checked_add(4)?;
            flags.push(MeshTextureMapEntry {
                ordinal: u32::try_from(ordinal).ok()?,
                resource_guid,
                guid_offset: source_offset(frame.start, guid_at.checked_add(4)?)?,
                value,
                value_offset: source_offset(frame.start, value_offset)?,
            });
        }
        let filename_count_at = at;
        let filename_count = usize::try_from(View::u32_le_at(record, at)?).ok()?;
        at = at.checked_add(4)?;
        let mut filenames = Vec::with_capacity(filename_count);
        let mut filename_keys = HashSet::with_capacity(filename_count);
        for ordinal in 0..filename_count {
            let guid_at = at;
            let (resource_guid, end) = lp_ascii_strict(record, at, 36..=36)?;
            (is_guid_hyphenated(&resource_guid)
                && filename_keys.insert(resource_guid.to_ascii_uppercase()))
            .then_some(())?;
            at = end;
            let reference_at = at;
            let filename_record_index = exact_local_record_index(record, at)?;
            at = at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)?;
            filenames.push(MeshTextureFilenameEntry {
                ordinal: u32::try_from(ordinal).ok()?,
                resource_guid,
                guid_offset: source_offset(frame.start, guid_at.checked_add(4)?)?,
                filename_record_index,
                reference_offset: source_offset(frame.start, reference_at)?,
            });
        }
        (at == record.len() && flag_keys == filename_keys).then_some(())?;
        Some(MeshTextureTableRecord {
            identity,
            flags_count_offset: source_offset(frame.start, texture_table::FLAGS_MAP_COUNT)?,
            filename_count_offset: source_offset(frame.start, filename_count_at)?,
            flags,
            filenames,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-texture-table", frame.entity_id))
}

fn parse_mesh_wrapper_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshWrapperRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_WRAPPER_TYPE_VERSION,
        MESH_WRAPPER_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-wrapper",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-wrapper")?;
    let parsed = (|| {
        (record.len() == body_wrapper::LEN
            && record.get(body_wrapper::ZERO_RUN_10..body_wrapper::BODY_REFERENCE)
                == Some(&[0; 10]))
        .then_some(())?;
        let body_record_index = exact_local_record_index(record, body_wrapper::BODY_REFERENCE)?;
        (record.get(body_wrapper::ZERO_TAIL_8..body_wrapper::LEN) == Some(&[0; 8])).then_some(())?;
        Some(MeshWrapperRecord {
            identity,
            body_record_index,
            body_reference_offset: source_offset(frame.start, body_wrapper::BODY_REFERENCE)?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-wrapper", frame.entity_id))
}

fn scene_state_mask_is_exact(mask: &[u8]) -> bool {
    mask.len() == 49
        && mask.iter().enumerate().all(|(index, byte)| {
            *byte
                == match index {
                    6 | 14 | 22 | 30 | 38 | 46 => 0xef,
                    31 | 39 | 47 => 0x7f,
                    48 => 0x01,
                    _ => 0xff,
                }
        })
}

#[allow(clippy::option_option)] // Distinguish an invalid footer from a valid footer without bounds.
fn parse_scene_footer(
    record: &[u8],
    at: usize,
    frame_start: usize,
) -> Option<Option<DesignMeshSceneBounds>> {
    (at.checked_add(SCENE_FOOTER_BYTES) == Some(record.len()) && record.get(at) == Some(&1))
        .then_some(())?;
    parse_scene_bounds_payload(record, at.checked_add(1)?, frame_start)
}

#[allow(clippy::option_option)] // Distinguish an invalid payload from the exact no-bounds state mask.
fn parse_scene_bounds_payload(
    record: &[u8],
    payload_at: usize,
    frame_start: usize,
) -> Option<Option<DesignMeshSceneBounds>> {
    let payload = record.get(payload_at..)?;
    if scene_state_mask_is_exact(payload) {
        return Some(None);
    }
    (payload.len() == 49 && payload[48] == 1).then_some(())?;
    let mut values = [0.0; 6];
    for (ordinal, value) in values.iter_mut().enumerate() {
        *value = View::f64_le_at(record, payload_at.checked_add(ordinal.checked_mul(8)?)?)?;
    }
    let maximum = [values[0], values[1], values[2]];
    let minimum = [values[3], values[4], values[5]];
    (values.iter().all(|value| value.is_finite())
        && minimum
            .iter()
            .zip(maximum)
            .all(|(minimum, maximum)| *minimum <= maximum))
    .then_some(())?;
    Some(Some(DesignMeshSceneBounds {
        maximum,
        minimum,
        offsets: [
            source_offset(frame_start, payload_at)?,
            source_offset(frame_start, payload_at.checked_add(24)?)?,
        ],
    }))
}

fn parse_mesh_scene_state_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<(DesignMeshRecordIdentity, Option<DesignMeshSceneBounds>), CodecError> {
    validate_mesh_registration(
        frame,
        MESH_SCENE_STATE_TYPE_VERSION,
        MESH_SCENE_STATE_BASE_TYPE_GUID,
        SCENE_MODULE,
        "mesh-scene-state",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-scene-state")?;
    let bounds = (record.len() == scene_state::LEN
        && record.get(scene_state::ZERO_RUN_34..scene_state::FOOTER_MARKER) == Some(&[0; 34]))
    .then(|| parse_scene_footer(record, scene_state::FOOTER_MARKER, frame.start))
    .flatten()
    .ok_or_else(|| malformed_frame("mesh-scene-state", frame.entity_id))?;
    Ok((identity, bounds))
}

fn parse_scene_node_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshSceneNodeRecord, CodecError> {
    validate_mesh_registration(
        frame,
        SCENE_NODE_TYPE_VERSION,
        SCENE_NODE_BASE_TYPE_GUID,
        SCENE_MODULE,
        "mesh-scene-node",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-scene-node")?;
    let parsed = (|| {
        (record.get(scene_node::ZERO_RUN_14..scene_node::CONSTANT_TWO_A) == Some(&[0; 14])
            && View::u32_le_at(record, scene_node::CONSTANT_TWO_A) == Some(2)
            && View::u32_le_at(record, scene_node::CONSTANT_TWO_B) == Some(2)
            && View::u32_le_at(record, scene_node::CONSTANT_THREE) == Some(3))
        .then_some(())?;
        let (bounds, transform) = if record.len() == scene_node::LEN
            && record.get(scene_node::ZERO_RUN_24..scene_node::FOOTER_MARKER) == Some(&[0; 24])
        {
            (
                parse_scene_footer(record, scene_node::FOOTER_MARKER, frame.start)?,
                None,
            )
        } else if record.len() == placed_scene_node::LEN
            && record.get(placed_scene_node::ZERO_RUN_25..placed_scene_node::TRANSFORM)
                == Some(&[0; 25])
        {
            (
                parse_scene_bounds_payload(record, placed_scene_node::FOOTER_MASK, frame.start)?,
                Some(crate::records::Located {
                    value: MeshAffineTransform::parse(record, placed_scene_node::TRANSFORM)?,
                    offset: source_offset(frame.start, placed_scene_node::TRANSFORM)?,
                }),
            )
        } else {
            return None;
        };
        Some(MeshSceneNodeRecord {
            identity,
            bounds,
            transform,
            state_record_index: exact_local_record_index(
                record,
                scene_node::SCENE_STATE_REFERENCE,
            )?,
            state_reference_offset: source_offset(frame.start, scene_node::SCENE_STATE_REFERENCE)?,
            auxiliary_record_index: exact_local_record_index(
                record,
                scene_node::AUXILIARY_RECORD_REFERENCE,
            )?,
            auxiliary_reference_offset: source_offset(
                frame.start,
                scene_node::AUXILIARY_RECORD_REFERENCE,
            )?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-scene-node", frame.entity_id))
}

fn parse_typed_identity(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
    expected_version: u32,
    expected_base_type_guid: &str,
    expected_module: &str,
    record_kind: &str,
) -> Result<DesignMeshRecordIdentity, CodecError> {
    validate_mesh_registration(
        frame,
        expected_version,
        expected_base_type_guid,
        expected_module,
        record_kind,
    )?;
    record_identity(&bytes[frame.start..frame.end], frame, record_kind)
}

fn parse_mesh_scope_record(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    records: &IndexedRecordOffsets,
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshScopeRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_FEATURE_SCOPE_TYPE_VERSION,
        MESH_FEATURE_SCOPE_BASE_TYPE_GUID,
        FUSION_MODULE,
        "mesh-feature-scope",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-feature-scope")?;
    let parsed = (|| {
        (record.get(feature_scope::ZERO_RUN_10..feature_scope::BODY_COUNT) == Some(&[0; 10]))
            .then_some(())?;
        let (body_record_indices, body_reference_offsets, body_list_end) =
            counted_local_record_indices(record, feature_scope::BODY_COUNT)?;
        let header = DesignRecordHeader {
            id: String::new(),
            record_index: identity.record_index,
            class_tag: identity.class_tag.clone(),
            byte_offset: u64::try_from(frame.start).ok()?,
        };
        let scope = parse_parameter_scope(bytes, records, &header)?;
        (scope.kind() == crate::records::DesignFeatureKind::BaseMeshFeature
            && scope.byte_offset == u64::try_from(frame.start).ok()?)
        .then_some(())?;
        let paired_at = usize::try_from(scope.paired_byte_offset).ok()?;
        let paired_relative = paired_at.checked_sub(frame.start)?;
        (body_list_end <= paired_relative
            && paired_relative.checked_add(feature_scope_base::LEN) == Some(record.len()))
        .then_some(())?;
        let base_record = nested_record_identity(
            record,
            frame.start,
            paired_relative,
            record.len(),
            identity.record_index,
            meta,
            MESH_SCOPE_BASE_RECORD_TYPE_GUID,
            MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID,
            MESH_SCOPE_BASE_RECORD_TYPE_VERSION,
            DATA_MODEL_MODULE,
        )?;
        (record.get(
            paired_relative + feature_scope_base::ZERO_RUN_8
                ..paired_relative + feature_scope_base::SCOPE_OWNER_REFERENCE,
        ) == Some(&[0; 8]))
        .then_some(())?;
        let owner_at = paired_relative.checked_add(feature_scope_base::SCOPE_OWNER_REFERENCE)?;
        let owner_record_index = exact_local_record_index(record, owner_at)?;
        Some(MeshScopeRecord {
            identity,
            base_record,
            body_record_indices,
            body_count_offset: source_offset(frame.start, feature_scope::BODY_COUNT)?,
            body_reference_offsets: absolute_offsets(frame.start, body_reference_offsets)?,
            owner_record_index,
            owner_reference_offset: source_offset(frame.start, owner_at)?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-feature-scope", frame.entity_id))
}

fn parse_mesh_collection_owner_record(
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
        identity,
        collection_record_index,
        collection_reference_offset: source_offset(frame.start, backlink_at)
            .ok_or_else(|| malformed_frame("mesh-collection-owner", frame.entity_id))?,
    }))
}

fn parse_mesh_texture_filename_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<(DesignMeshRecordIdentity, String, u64), CodecError> {
    let identity = parse_typed_identity(
        bytes,
        frame,
        MESH_TEXTURE_FILENAME_TYPE_VERSION,
        MESH_TEXTURE_FILENAME_BASE_TYPE_GUID,
        "",
        "mesh-texture-filename",
    )?;
    let record = &bytes[frame.start..frame.end];
    let parsed = (|| {
        (record.get(texture_filename::ZERO_RUN_10..texture_filename::BASENAME_CODE_UNIT_COUNT)
            == Some(&[0; 10]))
        .then_some(())?;
        let (filename, end) =
            lp_utf16_bounded(record, texture_filename::BASENAME_CODE_UNIT_COUNT, 1..=1024)?;
        (end == record.len()).then_some((
            identity,
            filename,
            source_offset(frame.start, texture_filename::LEN)?,
        ))
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-texture-filename", frame.entity_id))
}

fn unique_record_map<T>(
    records: Vec<T>,
    record_index: impl Fn(&T) -> u32,
    record_kind: &str,
) -> Result<HashMap<u32, T>, CodecError> {
    let mut out = HashMap::with_capacity(records.len());
    for record in records {
        let index = record_index(&record);
        if out.insert(index, record).is_some() {
            return Err(CodecError::malformed(format_args!(
                "F3D Design {record_kind} record index {index} is not unique"
            )));
        }
    }
    Ok(out)
}

fn typed_frame_map<'a>(
    frames: Vec<TypedPrimaryFrame<'a>>,
    record_kind: &str,
) -> Result<HashMap<u32, TypedPrimaryFrame<'a>>, CodecError> {
    let mut out = HashMap::with_capacity(frames.len());
    for frame in frames {
        let index = u32::try_from(frame.entity_id).map_err(|_| {
            CodecError::malformed(format_args!(
                "F3D Design {record_kind} entity {} exceeds the indexed-record domain",
                frame.entity_id
            ))
        })?;
        if out.insert(index, frame).is_some() {
            return Err(CodecError::malformed(format_args!(
                "F3D Design {record_kind} record index {index} is not unique"
            )));
        }
    }
    Ok(out)
}

fn malformed_mesh_graph(stream: &str, invariant: &str) -> CodecError {
    CodecError::malformed(format_args!(
        "F3D Design mesh feature graph violates `{invariant}` in {stream}"
    ))
}

fn parse_mesh_design_records<F>(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    source_entry_name: &str,
    asset_for_filename: &mut F,
) -> Result<MeshDesignRecords, CodecError>
where
    F: FnMut(&str) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError>,
{
    let stream = ids::native_scope(source_entry_name);
    let records = IndexedRecordOffsets::build(bytes);
    let collection_frames =
        typed_primary_frames(bytes, meta, MESH_COLLECTION_TYPE_GUID, "mesh-collection")?;
    if collection_frames.is_empty() {
        return Ok(MeshDesignRecords {
            features: Vec::new(),
        });
    }
    let collections = collection_frames
        .into_iter()
        .map(|frame| parse_mesh_collection_record(bytes, meta, frame))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|collection| !collection.body_record_indices.is_empty())
        .collect::<Vec<_>>();
    if collections.is_empty() {
        return Ok(MeshDesignRecords {
            features: Vec::new(),
        });
    }
    let mut entry_names = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_ENTRY_NAME_TYPE_GUID, "mesh-entry-name")?
            .into_iter()
            .map(|frame| parse_mesh_entry_name_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index,
        "mesh-entry-name",
    )?;
    let mut guids = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_GUID_TYPE_GUID, "mesh-GUID")?
            .into_iter()
            .map(|frame| parse_mesh_guid_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index,
        "mesh-GUID",
    )?;
    let mut bodies = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_BODY_TYPE_GUID, "mesh-body")?
            .into_iter()
            .map(|frame| parse_mesh_body_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index,
        "mesh-body",
    )?;
    let collection_record_indices = collections
        .iter()
        .map(|collection| collection.identity.record_index)
        .collect::<HashSet<_>>();
    let mut texture_tables = unique_record_map(
        typed_primary_frames(
            bytes,
            meta,
            MESH_TEXTURE_TABLE_TYPE_GUID,
            "mesh-texture-table",
        )?
        .into_iter()
        .map(|frame| parse_mesh_texture_table_record(bytes, frame))
        .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index,
        "mesh-texture-table",
    )?;
    let mut wrappers = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_WRAPPER_TYPE_GUID, "mesh-wrapper")?
            .into_iter()
            .map(|frame| parse_mesh_wrapper_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index,
        "mesh-wrapper",
    )?;
    let mut scopes = unique_record_map(
        typed_primary_frames(
            bytes,
            meta,
            MESH_FEATURE_SCOPE_TYPE_GUID,
            "mesh-feature-scope",
        )?
        .into_iter()
        .map(|frame| parse_mesh_scope_record(bytes, meta, &records, frame))
        .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index,
        "mesh-feature-scope",
    )?;
    let mut states = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_SCENE_STATE_TYPE_GUID, "mesh-scene-state")?
            .into_iter()
            .map(|frame| parse_mesh_scene_state_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.0.record_index,
        "mesh-scene-state",
    )?;
    let mut scene_nodes = unique_record_map(
        typed_primary_frames(bytes, meta, SCENE_NODE_TYPE_GUID, "mesh-scene-node")?
            .into_iter()
            .map(|frame| parse_scene_node_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index,
        "mesh-scene-node",
    )?;
    let scene_auxiliary_frames = typed_frame_map(
        typed_primary_frames(
            bytes,
            meta,
            SCENE_AUXILIARY_TYPE_GUID,
            "mesh-scene-auxiliary",
        )?,
        "mesh-scene-auxiliary",
    )?;
    let filename_frames = typed_frame_map(
        typed_primary_frames(
            bytes,
            meta,
            MESH_TEXTURE_FILENAME_TYPE_GUID,
            "mesh-texture-filename",
        )?,
        "mesh-texture-filename",
    )?;
    let collection_owners = unique_record_map(
        typed_primary_frames(
            bytes,
            meta,
            MESH_COLLECTION_OWNER_TYPE_GUID,
            "mesh-collection-owner",
        )?
        .into_iter()
        .map(|frame| parse_mesh_collection_owner_record(bytes, frame))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .filter(|owner| collection_record_indices.contains(&owner.collection_record_index))
        .collect(),
        |record| record.identity.record_index,
        "mesh-collection-owner",
    )?;
    let body_owner_frames = typed_frame_map(
        typed_primary_frames(bytes, meta, MESH_BODY_OWNER_TYPE_GUID, "mesh-body-owner")?,
        "mesh-body-owner",
    )?;

    let mut features = Vec::with_capacity(collections.len());
    let mut used_collection_owners = HashSet::new();
    let mut used_scene_auxiliaries = HashSet::new();
    for collection in collections {
        let stream_error = |invariant| malformed_mesh_graph(&stream, invariant);
        let candidate_scopes = scopes
            .iter()
            .filter(|(_, scope)| scope.body_record_indices == collection.body_record_indices)
            .filter(|(scope_index, _)| {
                collection.body_record_indices.iter().all(|body_index| {
                    bodies.get(body_index).is_some_and(|body| {
                        body.scope_record_index == **scope_index
                            && body.collection_record_index == collection.identity.record_index
                    })
                })
            })
            .map(|(record_index, _)| *record_index)
            .collect::<Vec<_>>();
        let [scope_record_index] = candidate_scopes.as_slice() else {
            let scope_lists = scopes
                .iter()
                .map(|(index, scope)| (*index, scope.body_record_indices.clone()))
                .collect::<Vec<_>>();
            let body_links = collection
                .body_record_indices
                .iter()
                .filter_map(|index| {
                    bodies.get(index).map(|body| {
                        (
                            *index,
                            body.scope_record_index,
                            body.collection_record_index,
                        )
                    })
                })
                .collect::<Vec<_>>();
            return Err(CodecError::malformed(format_args!(
                "F3D Design mesh feature graph violates `each mesh collection has exactly one scope with the same ordered body list` in {stream}: collection {} bodies {:?}, scope lists {:?}, body links {:?}",
                collection.identity.record_index,
                collection.body_record_indices,
                scope_lists,
                body_links,
            )));
        };
        let scope = scopes.remove(scope_record_index).ok_or_else(|| {
            stream_error("a mesh feature scope belongs to exactly one mesh collection")
        })?;
        let texture_table = texture_tables
            .remove(&collection.texture_table_record_index)
            .ok_or_else(|| {
                stream_error("a mesh texture table belongs to exactly one mesh collection")
            })?;
        let collection_owner = collection_owners
            .get(&collection.owner_record_index)
            .filter(|owner| {
                owner.collection_record_index == collection.identity.record_index
                    && used_collection_owners.insert(owner.identity.record_index)
            })
            .ok_or_else(|| {
                stream_error("each mesh collection has one unused owner with a reciprocal backlink")
            })?;

        let mut filename_entries = texture_table
            .filenames
            .iter()
            .map(|entry| (entry.resource_guid.to_ascii_uppercase(), entry))
            .collect::<HashMap<_, _>>();
        let mut textures = Vec::with_capacity(texture_table.flags.len());
        for flag in &texture_table.flags {
            let filename_entry = filename_entries
                .remove(&flag.resource_guid.to_ascii_uppercase())
                .ok_or_else(|| {
                    stream_error("texture flag and filename maps have identical GUID keys")
                })?;
            let filename_frame = filename_frames
                .get(&filename_entry.filename_record_index)
                .copied()
                .ok_or_else(|| {
                    stream_error("each texture filename reference targets a filename record")
                })?;
            let (filename_record, filename, filename_offset) =
                parse_mesh_texture_filename_record(bytes, filename_frame)?;
            let (archive_entry_name, asset) = asset_for_filename(&filename)?;
            textures.push(DesignMeshTextureResource {
                ordinal: flag.ordinal,
                resource_guid: flag.resource_guid.clone(),
                flags_guid_offset: flag.guid_offset,
                flags: flag.value,
                flags_offset: flag.value_offset,
                filename_ordinal: filename_entry.ordinal,
                filename_guid_offset: filename_entry.guid_offset,
                filename_record,
                filename_record_reference_offset: filename_entry.reference_offset,
                filename,
                filename_offset,
                archive_entry_name,
                asset,
            });
        }
        if !filename_entries.is_empty() {
            return Err(stream_error(
                "texture flag and filename maps have identical GUID keys",
            ));
        }

        let mut feature_bodies = Vec::with_capacity(collection.body_record_indices.len());
        for body_record_index in &collection.body_record_indices {
            let body = bodies.remove(body_record_index).ok_or_else(|| {
                stream_error("each collection body reference targets one unused mesh body")
            })?;
            let wrapper = wrappers
                .remove(&body.wrapper_record_index)
                .filter(|wrapper| wrapper.body_record_index == body.identity.record_index)
                .ok_or_else(|| stream_error("each mesh body has one unused reciprocal wrapper"))?;
            let guid = guids
                .remove(&body.guid_record_index)
                .ok_or_else(|| stream_error("each mesh body has one unused GUID record"))?;
            let entry_name = entry_names
                .remove(&guid.entry_name_record_index)
                .filter(|entry| entry.guid_record_index == guid.identity.record_index)
                .ok_or_else(|| {
                    stream_error("each mesh GUID has one unused reciprocal entry-name record")
                })?;
            let scene_node = scene_nodes
                .remove(&body.scene_node_record_index)
                .ok_or_else(|| stream_error("each mesh body has one unused Scene node"))?;
            let scene_state = states
                .remove(&scene_node.state_record_index)
                .ok_or_else(|| stream_error("each Scene node has one unused Scene state"))?;
            let scene_auxiliary_frame = scene_auxiliary_frames
                .get(&scene_node.auxiliary_record_index)
                .copied()
                .filter(|_| used_scene_auxiliaries.insert(scene_node.auxiliary_record_index))
                .ok_or_else(|| {
                    stream_error("each Scene node has one unused Scene auxiliary record")
                })?;
            let scene_auxiliary = parse_typed_identity(
                bytes,
                scene_auxiliary_frame,
                SCENE_AUXILIARY_TYPE_VERSION,
                SCENE_AUXILIARY_BASE_TYPE_GUID,
                SCENE_MODULE,
                "mesh-scene-auxiliary",
            )?;
            let body_owner_frame = body_owner_frames
                .get(&body.owner_record_index)
                .copied()
                .ok_or_else(|| stream_error("each mesh body references a typed Body owner"))?;
            let body_owner = parse_typed_identity(
                bytes,
                body_owner_frame,
                MESH_BODY_OWNER_TYPE_VERSION,
                MESH_BODY_OWNER_BASE_TYPE_GUID,
                "Body",
                "mesh-body-owner",
            )?;
            let body_byte_offset = usize::try_from(body.identity.byte_offset).map_err(|_| {
                stream_error("mesh body byte offsets fit the platform address domain")
            })?;
            feature_bodies.push(DesignMeshBody {
                body_record: body.identity,
                entry_name_record: entry_name.identity,
                guid_record: guid.identity,
                wrapper_record: wrapper.identity,
                scene_state_record: scene_state.0,
                scene_state_bounds: scene_state.1,
                scene_node_record: scene_node.identity,
                scene_node_bounds: scene_node.bounds,
                scene_node_transform: scene_node.transform.map(|located| crate::records::Located { value: located.value.rows(), offset: located.offset }),
                scene_auxiliary_record: scene_auxiliary,
                owner_record: body_owner,
                entry_name: entry_name.entry_name,
                entry_name_offset: entry_name.entry_name_offset,
                fusion_uuid: guid.fusion_uuid,
                container_mesh_uuid: None,
                fusion_uuid_offset: guid.fusion_uuid_offset,
                transform: body.transform.rows(),
                transform_offsets: [
                    source_offset(body_byte_offset, mesh_body::FIRST_TRANSFORM).ok_or_else(
                        || stream_error("the first mesh transform has an addressable offset"),
                    )?,
                    source_offset(body_byte_offset, mesh_body::SECOND_TRANSFORM).ok_or_else(
                        || stream_error("the second mesh transform has an addressable offset"),
                    )?,
                ],
                scope_reference_offset: body.scope_reference_offset,
                wrapper_reference_offset: body.wrapper_reference_offset,
                owner_reference_offset: body.owner_reference_offset,
                guid_reference_offset: body.guid_reference_offset,
                scene_node_reference_offset: body.scene_node_reference_offset,
                collection_reference_offset: body.collection_reference_offset,
                wrapper_body_reference_offset: wrapper.body_reference_offset,
                entry_guid_reference_offset: entry_name.guid_reference_offset,
                guid_entry_reference_offset: guid.entry_reference_offset,
                scene_state_reference_offset: scene_node.state_reference_offset,
                scene_auxiliary_reference_offset: scene_node.auxiliary_reference_offset,
                tessellation_id: None,
            });
        }
        let scope_offset = usize::try_from(scope.identity.byte_offset).map_err(|_| {
            stream_error("mesh feature scope byte offsets fit the platform address domain")
        })?;
        features.push(DesignMeshFeature {
            id: ids::native_design_mesh_feature_id(source_entry_name, scope_offset),
            scope_record: scope.identity,
            scope_base_record: scope.base_record,
            collection_record: collection.identity,
            collection_base_record: collection.base_record,
            texture_table_record: texture_table.identity,
            body_count_offsets: [
                scope.body_count_offset,
                collection.count_offsets[0],
                collection.count_offsets[1],
            ],
            body_record_indices: collection.body_record_indices,
            scope_body_reference_offsets: scope.body_reference_offsets,
            collection_body_reference_offsets: collection.body_reference_offsets,
            texture_table_reference_offset: collection.texture_reference_offset,
            collection_owner_record: collection_owner.identity.clone(),
            collection_owner_reference_offset: collection.owner_reference_offset,
            collection_owner_backlink_offset: collection_owner.collection_reference_offset,
            scope_owner_record_index: scope.owner_record_index,
            scope_owner_reference_offset: scope.owner_reference_offset,
            texture_flags_count_offset: texture_table.flags_count_offset,
            texture_filename_count_offset: texture_table.filename_count_offset,
            bodies: feature_bodies,
            textures,
        });
    }
    if !entry_names.is_empty()
        || !guids.is_empty()
        || !bodies.is_empty()
        || !texture_tables.is_empty()
        || !wrappers.is_empty()
        || !scopes.is_empty()
    {
        return Err(malformed_mesh_graph(
            &stream,
            "all typed mesh graph records belong to exactly one feature",
        ));
    }
    Ok(MeshDesignRecords { features })
}

fn decode_mesh_design_records(scan: &ContainerScan) -> Result<Vec<MeshDesignRecords>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let mut asset_for_filename = |filename: &str| {
            let mut matches = scan.entries.iter().filter(|candidate| {
                scan.is_design_asset_entry(candidate, ContainerRole::Image)
                    && candidate.name.rsplit('/').next() == Some(filename)
            });
            let (Some(asset), None) = (matches.next(), matches.next()) else {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design mesh texture `{filename}` does not resolve to one embedded image"
                )));
            };
            Ok((
                asset.name.clone(),
                crate::ids::neutral_asset_id(&asset.name),
            ))
        };
        let records = parse_mesh_design_records(
            scan.entry_bytes(&entry.name)?,
            &meta,
            &entry.name,
            &mut asset_for_filename,
        )?;
        if !records.features.is_empty() {
            out.push(records);
        }
    }
    Ok(out)
}

fn resolve_mesh_body(
    records: &[MeshDesignRecords],
    entry_name: &str,
    fusion_uuid: &str,
) -> Option<(usize, usize, usize)> {
    let mut matches = Vec::new();
    for (design_ordinal, design) in records.iter().enumerate() {
        for (feature_ordinal, feature) in design.features.iter().enumerate() {
            for (body_ordinal, body) in feature.bodies.iter().enumerate() {
                if body.tessellation_id.is_none()
                    && body.entry_name == entry_name
                    && body.fusion_uuid.eq_ignore_ascii_case(fusion_uuid)
                {
                    matches.push((design_ordinal, feature_ordinal, body_ordinal));
                }
            }
        }
    }
    let [joined] = matches.as_slice() else {
        return None;
    };
    Some(*joined)
}

/// Decode every mesh body: one per `.paramesh` container joined to the
/// mesh-body record that names its GUID record.
pub(crate) fn decode_mesh_bodies(scan: &ContainerScan) -> Result<MeshDecode, CodecError> {
    let mut design_records = decode_mesh_design_records(scan)?;
    let mut outcomes = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::Paramesh))
    {
        let container = match scan
            .entry_bytes(&entry.name)
            .and_then(decode_mesh_container)
        {
            Ok(container) => container,
            Err(error) => {
                outcomes.push(MeshContainerOutcome::Failed {
                    entry_name: entry.name.clone(),
                    error,
                });
                continue;
            }
        };
        let name = &entry.name;
        let base = name.rsplit('/').next().unwrap_or(name);
        let Some((design_ordinal, feature_ordinal, body_ordinal)) =
            resolve_mesh_body(&design_records, base, &container.fusion_uuid)
        else {
            outcomes.push(MeshContainerOutcome::Unjoined {
                entry_name: entry.name.clone(),
            });
            continue;
        };
        design_records[design_ordinal].features[feature_ordinal].bodies[body_ordinal]
            .container_mesh_uuid = Some(container.mesh_uuid.clone());
        let body = &design_records[design_ordinal].features[feature_ordinal].bodies[body_ordinal];
        let projected = match MeshBody::from_container(
            &entry.name,
            body.body_record.byte_offset,
            MeshAffineTransform::from_rows(body.transform),
            container,
        ) {
            Ok(projected) => projected,
            Err(error) => {
                outcomes.push(MeshContainerOutcome::Failed {
                    entry_name: entry.name.clone(),
                    error,
                });
                continue;
            }
        };
        design_records[design_ordinal].features[feature_ordinal].bodies[body_ordinal]
            .tessellation_id = Some(projected.id.clone());
        outcomes.push(MeshContainerOutcome::Joined(projected));
    }
    for body in design_records
        .iter()
        .flat_map(|design| &design.features)
        .flat_map(|feature| &feature.bodies)
        .filter(|body| body.tessellation_id.is_none())
    {
        outcomes.push(MeshContainerOutcome::Missing {
            entry_name: body.entry_name.clone(),
        });
    }
    Ok(MeshDecode {
        outcomes,
        features: design_records
            .into_iter()
            .flat_map(|design| design.features)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(cells: [f64; 16]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(MATRIX_BYTES);
        for value in cells {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn mesh_body_payload(cells: [f64; 16]) -> Vec<u8> {
        let mut payload = vec![0; mesh_body::FIRST_TRANSFORM];
        payload.extend_from_slice(&matrix(cells));
        payload.push(0);
        payload.extend_from_slice(&matrix(cells));
        payload
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

    fn push_ascii(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(
            &u32::try_from(value.len())
                .expect("test ASCII length")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        let encoded = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(
            &u32::try_from(encoded.len())
                .expect("test UTF-16 length")
                .to_le_bytes(),
        );
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
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
        push_utf16(&mut bytes, entry_name);
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
        push_ascii(&mut bytes, fusion_uuid);
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

    fn design_type(
        type_guid: &str,
        base_type_guid: Option<&str>,
        version: u32,
        module: &str,
        entity_ids: Vec<u64>,
    ) -> crate::records::SegmentType {
        crate::records::SegmentType {
            id: String::new(),
            byte_offset: 0,
            type_guid: type_guid.into(),
            type_guid_offset: 0,
            base_type_guid: base_type_guid.map(|value| crate::records::RecordedValue { value: value.to_owned(), offset: Some(0) }),
            version,
            version_offset: 0,
            module: module.into(),
            entity_ids,
            entity_id_offsets: Vec::new(),
        }
    }

    fn primary_record(entity_id: u64, bulk_offset: usize) -> crate::metastream::RecordIndexEntry {
        crate::metastream::RecordIndexEntry {
            entity_id,
            bulk_offset: u64::try_from(bulk_offset).expect("test bulk offset"),
        }
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
        bytes.extend_from_slice(&(body_record_indices.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        push_reference(&mut bytes, texture_table_record_index);
        push_indexed_header(&mut bytes, base_class_tag, record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.extend_from_slice(&(body_record_indices.len() as u32).to_le_bytes());
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
        bytes.extend_from_slice(&(flags.len() as u32).to_le_bytes());
        for (guid, value) in flags {
            push_ascii(&mut bytes, guid);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&(filenames.len() as u32).to_le_bytes());
        for (guid, target) in filenames {
            push_ascii(&mut bytes, guid);
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
        bytes.extend_from_slice(&(body_record_indices.len() as u32).to_le_bytes());
        for body in body_record_indices {
            push_reference(&mut bytes, *body);
        }
        bytes.extend_from_slice(&[0; 24]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_reference(&mut bytes, member_record_index);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        push_utf16(&mut bytes, "Base Mesh Feature");
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
        push_utf16(&mut bytes, filename);
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
        let [feature] = design.features.as_slice() else {
            panic!("one mesh feature");
        };
        assert_eq!(feature.scope_record.record_index, 109);
        assert_eq!(feature.collection_record.record_index, 100);
        assert_eq!(feature.texture_table_record.record_index, 101);
        assert_eq!(feature.body_record_indices, [104]);
        assert!(feature.textures.is_empty());
        let [body] = feature.bodies.as_slice() else {
            panic!("one mesh body");
        };
        assert_eq!(body.entry_name, ENTRY_NAME);
        assert_eq!(body.fusion_uuid, FUSION_UUID);
        assert_eq!(body.wrapper_record.record_index, 108);
        assert_eq!(body.scene_state_record.record_index, 105);
        assert_eq!(body.scene_node_record.record_index, 107);
        assert_eq!(body.scene_auxiliary_record.record_index, 106);
        assert_eq!(body.owner_record.record_index, 111);
        assert_eq!(
            resolve_mesh_body(&[design], ENTRY_NAME, FUSION_UUID),
            Some((0, 0, 0))
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
        assert!(design.features.is_empty());
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
                    .eq_ignore_ascii_case(MESH_COLLECTION_TYPE_GUID)
            })
            .expect("mesh-collection type");
        let [collection_entity] = collection_type.entity_ids.as_slice() else {
            panic!("one mesh-collection entity");
        };
        let collection_entity = *collection_entity;
        collection_type.entity_ids.clear();
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
        assert!(design.features.is_empty());
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
        let [feature] = design.features.as_slice() else {
            panic!("one mesh feature");
        };
        let [first, second] = feature.bodies.as_slice() else {
            panic!("two mesh bodies");
        };

        assert_eq!(feature.body_record_indices, [104, 117]);
        assert_eq!(first.owner_record, second.owner_record);
        assert_ne!(first.body_record, second.body_record);
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
        let textures = &design.features[0].textures;
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].flags, 2);
        assert_eq!(textures[0].filename, "mesh-a.png");
        assert_eq!(textures[0].filename_ordinal, 1);
        assert_eq!(textures[1].flags, 258);
        assert_eq!(textures[1].filename, "mesh-b.jpg");
        assert_eq!(textures[1].filename_ordinal, 0);
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
        let table =
            parse_mesh_texture_table_record(&graph.bytes, *frame).expect("original texture table");
        let [first, second] = table.filenames.as_slice() else {
            panic!("two texture resources");
        };
        let second_reference =
            usize::try_from(second.reference_offset).expect("test reference offset");
        put_reference(
            &mut graph.bytes,
            second_reference,
            first.filename_record_index,
        );
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
        let textures = &design.features[0].textures;
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].filename_record, textures[1].filename_record);
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
            parse_mesh_texture_table_record(&bytes, frame),
            Err(CodecError::Malformed(_))
        ));
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
            parse_mesh_wrapper_record(&bytes, frame),
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
            parse_mesh_scene_state_record(&graph.bytes, frame),
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
            parse_scene_node_record(&graph.bytes, frame),
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

        let parsed = parse_scene_node_record(&graph.bytes, frame).expect("finite Scene bounds");
        let bounds = parsed.bounds.expect("present bounds");
        assert_eq!(bounds.maximum, [1.0, 2.0, 3.0]);
        assert_eq!(bounds.minimum, [-4.0, -5.0, -6.0]);
        assert_eq!(
            bounds.offsets,
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
                        .eq_ignore_ascii_case(SCENE_NODE_TYPE_GUID)
                })
                .expect("Scene-node type"),
        };

        let parsed = parse_scene_node_record(&bytes, frame).expect("placed Scene node");
        assert_eq!(parsed.transform.expect("placed transform").value.0, transform);
        assert_eq!(
            parsed.transform.map(|located| located.offset),
            Some(placed_scene_node::TRANSFORM as u64)
        );
        let bounds = parsed.bounds.expect("placed bounds");
        assert_eq!(bounds.maximum, [4.0, 5.0, 6.0]);
        assert_eq!(bounds.minimum, [1.0, 2.0, 3.0]);
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

        let owner = parse_mesh_collection_owner_record(&bytes, frame)
            .expect("valid legacy owner frame")
            .expect("legacy collection owner");
        assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
        assert_eq!(owner.collection_reference_offset, 859);
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

        let owner = parse_mesh_collection_owner_record(&bytes, frame)
            .expect("valid version-17 owner frame")
            .expect("version-17 collection owner");
        assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
        assert_eq!(
            owner.collection_reference_offset,
            collection_owner_v17::COLLECTION_BACKLINK as u64
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

        assert!(parse_mesh_collection_owner_record(&bytes, frame)
            .expect("valid generic owner frame")
            .is_none());
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
        assert!(resolve_mesh_body(
            &[design.clone(), design],
            "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh",
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE"
        )
        .is_none());
    }

    #[test]
    fn mesh_body_transform_applies_nonuniform_scale_and_translation() {
        let cells = [
            0.175, 0.0, 0.0, 0.4, 0.0, 0.06, 0.0, 0.7, 0.0, 0.0, 0.125, 0.3, 0.0, 0.0, 0.0, 1.0,
        ];
        let transform = mesh_body_transform(&mesh_body_payload(cells)).expect("affine map");
        assert_eq!(
            transform
                .transform_point([2.0, 5.0, 8.0])
                .expect("transformed point"),
            cadmpeg_ir::math::Point3::new(7.5, 10.0, 13.0)
        );
    }

    #[test]
    fn reflected_mesh_placement_preserves_triangle_and_corner_order() {
        let cells = [
            -0.5, 0.0, 0.0, 1.0, 0.0, 0.25, 0.0, -2.0, 0.0, 0.0, 2.0, 0.5, 0.0, 0.0, 0.0, 1.0,
        ];
        let transform = mesh_body_transform(&mesh_body_payload(cells)).expect("reflected map");
        let container = MeshContainer {
            fusion_uuid: "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into(),
            mesh_uuid: "11111111-2222-4333-8444-555555555555".into(),
            vertices: vec![[2.0, 8.0, 3.0], [0.0, 0.0, 0.0], [4.0, 4.0, -1.0]],
            triangles: vec![[2, 0, 1]],
            feature_edges: vec![[0, 2]],
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_ids: None,
            attributes: vec![crate::paramesh::MeshAttribute {
                role: 4,
                resource_guid: None,
                authored_name: None,
                groups: Vec::new(),
                element_code: 4,
                domain: crate::paramesh::MeshAttributeDomain::Corner,
                item_size: Some(16),
                values: (0..80).collect(),
                indices: Some(vec![0, 2]),
                triangle_values: None,
            }],
        };
        let body = MeshBody::from_container("mesh.paramesh", 100, transform, container)
            .expect("projected mesh");

        assert_eq!(
            body.vertices,
            [
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 65.0),
                cadmpeg_ir::math::Point3::new(10.0, -20.0, 5.0),
                cadmpeg_ir::math::Point3::new(-10.0, -10.0, -15.0),
            ]
        );
        assert_eq!(body.triangles, [[2, 0, 1]]);
        assert_eq!(body.feature_edges, [[0, 2]]);
        assert_eq!(body.attributes[0].indices, Some(vec![0, 2]));
    }

    #[test]
    fn mesh_placement_transforms_corner_normals_with_oriented_cofactors() {
        let cells = [
            -2.0, 0.5, 0.2, 1.0, 0.1, 3.0, 0.25, -2.0, 0.3, -0.2, 4.0, 0.5, 0.0, 0.0, 0.0, 1.0,
        ];
        let transform = mesh_body_transform(&mesh_body_payload(cells)).expect("affine map");
        let container = MeshContainer {
            fusion_uuid: "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into(),
            mesh_uuid: "11111111-2222-4333-8444-555555555555".into(),
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            feature_edges: vec![[0, 1]],
            corner_normals: vec![[0.0, 0.0, 1.0]; 3],
            triangle_groups: Vec::new(),
            texture_ids: None,
            attributes: Vec::new(),
        };
        let body = MeshBody::from_container("mesh.paramesh", 100, transform, container)
            .expect("projected mesh");
        let geometric_normal = body.vertices[1]
            .vector_from(body.vertices[0])
            .cross(body.vertices[2].vector_from(body.vertices[0]))
            .unit()
            .expect("triangle normal");
        assert_eq!(body.corner_normals.len(), 3);
        for normal in body.corner_normals {
            assert!((normal.dot(geometric_normal) - 1.0).abs() < 1.0e-12);
        }
    }

    #[test]
    fn mesh_body_transform_refuses_mismatched_projective_and_singular_pairs() {
        let identity = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut mismatched = mesh_body_payload(identity);
        mismatched[mesh_body::SECOND_TRANSFORM..mesh_body::SECOND_TRANSFORM + 8]
            .copy_from_slice(&2.0f64.to_le_bytes());
        assert!(mesh_body_transform(&mismatched).is_none());

        let mut projective = identity;
        projective[12] = 1.0;
        assert!(mesh_body_transform(&mesh_body_payload(projective)).is_none());

        let mut singular = identity;
        singular[0] = 0.0;
        assert!(mesh_body_transform(&mesh_body_payload(singular)).is_none());
    }
}
