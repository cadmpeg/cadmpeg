// SPDX-License-Identifier: Apache-2.0
//! Author `Base Mesh Feature` operations, their Design mesh graph and their
//! `.paramesh` geometry containers from the neutral model
//! ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers),
//! [§3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).

mod container;

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{ChannelAddressing, Tessellation, TessellationId};

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};
use crate::design::decode::mesh as kinds;
use crate::layout::{
    paramesh_collection_owner_backlink_prefix as collection_owner,
    paramesh_mesh_body_join_prefix as mesh_body,
};
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::entity_header::SegmentType;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::mesh::{DesignMeshBody, DesignMeshFeature, DesignMeshSceneBounds};
use container::{Channel, ChannelDomain, Container, Layout};

/// Family name of the scope that owns mesh bodies (spec §3.1).
const MESH_FEATURE_KIND: &str = "Base Mesh Feature";
/// Row-major cells of the body placement of an untransformed millimetre
/// mesh: the f64 nearest `0.1` on the diagonal maps millimetres to the
/// centimetres of the model (spec §3.1).
const MILLIMETRE_PLACEMENT: [f64; 16] = [
    0.1, 0.0, 0.0, 0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 1.0,
];
/// Byte length of the fixed tail that follows the feature-family name of a
/// 78-byte-tail parameter scope (spec §3.1, parameter scopes).
const SCOPE_TAIL_LEN: usize = 78;
/// Tail offset of the preceding history-state identity in a 78-byte tail.
const SCOPE_TAIL_PREVIOUS_STATE: usize = 31;
/// Zero bytes between the scope body list and its reference table.
const SCOPE_BODY_GAP: usize = 24;
/// Null history-state identity.
const NULL_STATE: u32 = u32::MAX;

/// Native scope, mesh graph and member records of one mesh import. Synthesis
/// re-emits them when they still project to the neutral feature
/// (#209 §2.3 W2).
struct NativeMesh<'a> {
    scope: &'a DesignParameterScope,
    mesh: &'a DesignMeshFeature,
    members: Vec<(u32, &'a SegmentType)>,
}

/// One neutral mesh import the writer authors.
pub(in crate::writer) struct MeshImport<'a> {
    feature: FeatureId,
    feature_ordinal: u32,
    native: Option<NativeMesh<'a>>,
    tessellations: Vec<&'a Tessellation>,
}

/// Neutral mesh imports the writer can author, with the reason for each it
/// cannot.
pub(in crate::writer) fn select<'a>(target: &'a CadIr, native: &'a F3dNative) -> Vec<MeshImport<'a>> {
    let mut features: Vec<_> = target
        .model
        .features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::MeshImport { tessellations }) => {
                Some((feature, tessellations))
            }
            _ => None,
        })
        .collect();
    features.sort_by_key(|(feature, _)| feature.ordinal);
    let mut selected = Vec::new();
    let mut rank = 0u32;
    for (feature, ids) in features {
        rank = rank.saturating_add(1);
        if feature.suppressed == Some(true)
            || feature.product_definition.is_some()
            || !feature.dependencies.is_empty()
            || !feature.evaluation.outputs().is_empty()
            || feature.source_text.is_some()
            || !feature.source_content.is_empty()
        {
            continue;
        }
        let Some(tessellations) = ids
            .iter()
            .map(|id| {
                target
                    .model
                    .tessellations
                    .iter()
                    .find(|tessellation| tessellation.id == *id)
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !tessellations.iter().all(|tessellation| storable(tessellation)) {
            continue;
        }
        let native = native_mesh(feature, &tessellations, native);
        let feature_ordinal = native
            .as_ref()
            .map_or(rank, |native| native.scope.feature_ordinal.get());
        selected.push(MeshImport {
            feature: feature.id.clone(),
            feature_ordinal,
            native,
            tessellations,
        });
    }
    selected
}

/// Tessellations the selected mesh imports author.
pub(in crate::writer) fn authored_tessellations(
    target: &CadIr,
    native: &F3dNative,
) -> std::collections::BTreeSet<TessellationId> {
    select(target, native)
        .into_iter()
        .flat_map(|import| import.tessellations)
        .map(|tessellation| tessellation.id.clone())
        .collect()
}

/// Whether a container can store the mesh: its corners reach the first and
/// last vertex, so the implicit first corner is determined (spec §1.1.2).
fn storable(tessellation: &Tessellation) -> bool {
    let triangles = tessellation.triangles();
    let Some(last) = tessellation.vertex_count().checked_sub(1) else {
        return false;
    };
    let corners = triangles.iter().flatten();
    corners.clone().min() == Some(&0)
        && corners.max().and_then(|max| usize::try_from(*max).ok()) == Some(last)
}

fn native_mesh<'a>(
    feature: &cadmpeg_ir::features::Feature,
    tessellations: &[&Tessellation],
    native: &'a F3dNative,
) -> Option<NativeMesh<'a>> {
    let scope = native
        .design_parameter_scopes
        .iter()
        .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
    if scope.kind() != DesignFeatureKind::BaseMeshFeature
        || feature.name.as_deref()
            != Some(format!("{} {}", scope.source_kind_name(), scope.feature_ordinal).as_str())
        || feature.source_tag.as_deref() != Some(scope.source_kind_name())
    {
        return None;
    }
    let stream = crate::ids::native_stream(&scope.id)?;
    let mesh = native.design_mesh_features.iter().find(|mesh| {
        crate::ids::native_stream(&mesh.id) == Some(stream)
            && mesh.scope().record().record_index() == scope.record_index
    })?;
    if mesh.bodies().len() != tessellations.len()
        || tessellations
            .iter()
            .any(|tessellation| tessellation.native_ref.as_deref() != Some(mesh.id.as_str()))
        || !mesh.texture_table.resources().is_empty()
    {
        return None;
    }
    let members: Vec<u32> = scope.reference_members().values().copied().collect();
    if feature.source_properties.len() != members.len()
        || members.iter().enumerate().any(|(ordinal, member)| {
            feature
                .source_properties
                .get(format!("reference:{ordinal}").as_str())
                != Some(&member.to_string())
        })
    {
        return None;
    }
    let segment = crate::ids::design_segment(&scope.id)?;
    let members = members
        .into_iter()
        .map(|member| {
            native.design_record_headers.iter().find(|header| {
                header.record_index == member
                    && crate::ids::design_segment(&header.id) == Some(segment)
            })?;
            let kind = native.design_types.iter().find(|kind| {
                crate::ids::design_segment(kind.id()) == Some(segment)
                    && kind.entities.values().any(|entity| *entity == u64::from(member))
            })?;
            Some((member, kind))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(NativeMesh {
        scope,
        mesh,
        members,
    })
}

/// One authored Design segment, its mesh containers and what it wrote.
pub(super) struct EncodedMesh {
    pub(super) bulk: Vec<u8>,
    pub(super) meta: Vec<u8>,
    /// Archive basename and bytes of each `.paramesh` container.
    pub(super) containers: Vec<(String, Vec<u8>)>,
    pub(super) feature: FeatureId,
    pub(super) tessellations: Vec<TessellationId>,
    pub(super) losses: Vec<LossNote>,
    /// Source Design segment and record indices of the native records this
    /// segment re-emits, which no other segment writes again.
    pub(super) native_records: Option<(String, std::collections::BTreeSet<u32>)>,
}

/// The native records without the Design records the mesh segments re-emit.
pub(super) fn without_reemitted(native: &F3dNative, meshes: &[EncodedMesh]) -> Option<F3dNative> {
    let owned: Vec<_> = meshes
        .iter()
        .filter_map(|mesh| mesh.native_records.as_ref())
        .collect();
    if owned.is_empty() {
        return None;
    }
    let owns = |segment: Option<&str>, index: u64| {
        owned.iter().any(|(owner, indices)| {
            segment == Some(owner.as_str())
                && u32::try_from(index).is_ok_and(|index| indices.contains(&index))
        })
    };
    let mut native = native.clone();
    native
        .design_record_headers
        .retain(|header| !owns(crate::ids::design_segment(&header.id), u64::from(header.record_index)));
    for kind in &mut native.design_types {
        let segment = crate::ids::design_segment(kind.id()).map(str::to_owned);
        let entities: Vec<u64> = kind
            .entities
            .values()
            .copied()
            .filter(|entity| !owns(segment.as_deref(), *entity))
            .collect();
        if entities.len() != kind.entities.values().count() {
            kind.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
        }
    }
    Some(native)
}

/// Record indices of one mesh body graph.
struct BodyIndices {
    body: u32,
    entry: u32,
    guid: u32,
    wrapper: u32,
    state: u32,
    node: u32,
    auxiliary: u32,
    owner: u32,
}

/// Record indices of one mesh import graph.
struct GraphIndices {
    scope: u32,
    scope_owner: u32,
    collection: u32,
    texture_table: u32,
    collection_owner: u32,
    bodies: Vec<BodyIndices>,
    history_state: u32,
    previous_history_state: u32,
}

fn native_indices(native: &NativeMesh<'_>) -> GraphIndices {
    let state = |value: Option<i64>| value.and_then(|value| u32::try_from(value).ok());
    GraphIndices {
        scope: native.scope.record_index,
        scope_owner: native.mesh.scope().owner_record_index(),
        collection: native.mesh.collection().record().record_index(),
        texture_table: native.mesh.texture_table.record().record_index(),
        collection_owner: native.mesh.collection_owner.record().record_index(),
        bodies: native
            .mesh
            .bodies()
            .iter()
            .map(|body| BodyIndices {
                body: body.placement.record().record_index(),
                entry: body.entry.record().record_index(),
                guid: body.guid.record().record_index(),
                wrapper: body.wrapper_record.record_index(),
                state: body.scene_state.record().record_index(),
                node: body.scene_node.record_index(),
                auxiliary: body.scene_auxiliary_record.record_index(),
                owner: body.owner_record.record_index(),
            })
            .collect(),
        history_state: state(native.scope.history_state_id()).unwrap_or(NULL_STATE),
        previous_history_state: state(native.scope.previous_history_state_id())
            .unwrap_or(NULL_STATE),
    }
}

fn fresh_indices(bodies: usize) -> Result<GraphIndices, CodecError> {
    let mut next = 0u32;
    let mut take = || {
        next = next
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("F3D mesh record index exceeds u32".into()))?;
        Ok::<_, CodecError>(next)
    };
    let scope = take()?;
    let scope_owner = take()?;
    let collection = take()?;
    let texture_table = take()?;
    let collection_owner = take()?;
    let owner = take()?;
    let mut body_indices = Vec::new();
    for _ in 0..bodies {
        body_indices.push(BodyIndices {
            body: take()?,
            entry: take()?,
            guid: take()?,
            wrapper: take()?,
            state: take()?,
            node: take()?,
            auxiliary: take()?,
            owner,
        });
    }
    Ok(GraphIndices {
        scope,
        scope_owner,
        collection,
        texture_table,
        collection_owner,
        bodies: body_indices,
        history_state: NULL_STATE,
        previous_history_state: NULL_STATE,
    })
}

/// A version-4 GUID text derived from `seed`, so a re-encode of the same
/// document is byte-identical.
fn derived_guid(seed: &str, uppercase: bool) -> String {
    let digest = cadmpeg_ir::hash::digest::Sha256Digest::digest(seed.as_bytes());
    let hex: Vec<char> = digest.as_str().chars().take(32).collect();
    let mut text = String::with_capacity(36);
    for (index, character) in hex.into_iter().enumerate() {
        if matches!(index, 8 | 12 | 16 | 20) {
            text.push('-');
        }
        let character = match index {
            12 => '4',
            16 => '8',
            _ => character,
        };
        text.push(if uppercase {
            character.to_ascii_uppercase()
        } else {
            character
        });
    }
    text
}

fn header(out: &mut Vec<u8>, tag: u32, index: u32) -> Result<(), CodecError> {
    native_lp_ascii(out, &tag.to_string())?;
    out.extend_from_slice(&index.to_le_bytes());
    Ok(())
}

fn reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(index).to_le_bytes());
    out.extend_from_slice(&[0; 2]);
}

fn put_reference(out: &mut [u8], at: usize, index: u32) {
    out[at] = 1;
    out[at + 1..at + 9].copy_from_slice(&u64::from(index).to_le_bytes());
    out[at + 9..at + 11].copy_from_slice(&[0; 2]);
}

fn put_matrix(out: &mut [u8], at: usize, cells: [f64; 16]) {
    for (ordinal, cell) in cells.into_iter().enumerate() {
        let start = at + ordinal * 8;
        out[start..start + 8].copy_from_slice(&cell.to_le_bytes());
    }
}

/// Scene footer: marker `01` and either the exact absent-bounds mask or the
/// six bound values and `01` (spec §3.1).
fn scene_bounds(out: &mut Vec<u8>, bounds: Option<&DesignMeshSceneBounds>) {
    match bounds {
        Some(bounds) => {
            for value in bounds.maximum().into_iter().chain(bounds.minimum()) {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.push(1);
        }
        None => {
            for index in 0..49 {
                out.push(match index {
                    6 | 14 | 22 | 30 | 38 | 46 => 0xef,
                    31 | 39 | 47 => 0x7f,
                    48 => 0x01,
                    _ => 0xff,
                });
            }
        }
    }
}

/// Type-table positions of the mesh graph classes in an authored segment.
#[derive(Clone, Copy)]
enum Class {
    EntryName,
    Guid,
    Body,
    Collection,
    CollectionBase,
    TextureTable,
    Wrapper,
    Scope,
    ScopeBase,
    SceneState,
    SceneNode,
    SceneAuxiliary,
    CollectionOwner,
    BodyOwner,
}

impl Class {
    const ALL: [Self; 14] = [
        Self::EntryName,
        Self::Guid,
        Self::Body,
        Self::Collection,
        Self::CollectionBase,
        Self::TextureTable,
        Self::Wrapper,
        Self::Scope,
        Self::ScopeBase,
        Self::SceneState,
        Self::SceneNode,
        Self::SceneAuxiliary,
        Self::CollectionOwner,
        Self::BodyOwner,
    ];

    fn tag(self) -> u32 {
        let position = Self::ALL
            .iter()
            .position(|class| std::mem::discriminant(class) == std::mem::discriminant(&self))
            .and_then(|position| u32::try_from(position).ok())
            .unwrap_or_default();
        256 + position
    }

    /// Type GUID, base-type GUID, version and add-in module (spec §3.1).
    fn registration(self) -> (&'static str, &'static str, u32, &'static str) {
        match self {
            Self::EntryName => (
                kinds::MESH_ENTRY_NAME_TYPE_GUID,
                kinds::MESH_ENTRY_NAME_BASE_TYPE_GUID,
                kinds::MESH_ENTRY_NAME_TYPE_VERSION,
                kinds::PARAMESH_MODULE,
            ),
            Self::Guid => (
                kinds::MESH_GUID_TYPE_GUID,
                kinds::MESH_GUID_BASE_TYPE_GUID,
                kinds::MESH_GUID_TYPE_VERSION,
                kinds::PARAMESH_MODULE,
            ),
            Self::Body => (
                kinds::MESH_BODY_TYPE_GUID,
                kinds::MESH_BODY_BASE_TYPE_GUID,
                kinds::MESH_BODY_TYPE_VERSION,
                kinds::PARAMESH_MODULE,
            ),
            Self::Collection => (
                kinds::MESH_COLLECTION_TYPE_GUID,
                kinds::MESH_COLLECTION_BASE_TYPE_GUID,
                kinds::MESH_COLLECTION_TYPE_VERSION,
                kinds::PARAMESH_MODULE,
            ),
            Self::CollectionBase => (
                kinds::MESH_COLLECTION_BASE_TYPE_GUID,
                kinds::MESH_COLLECTION_BASE_BASE_TYPE_GUID,
                kinds::MESH_COLLECTION_BASE_TYPE_VERSION,
                kinds::COMMON_DATA_MODULE,
            ),
            Self::TextureTable => (
                kinds::MESH_TEXTURE_TABLE_TYPE_GUID,
                kinds::MESH_TEXTURE_TABLE_BASE_TYPE_GUID,
                kinds::MESH_TEXTURE_TABLE_TYPE_VERSION,
                kinds::PARAMESH_MODULE,
            ),
            Self::Wrapper => (
                kinds::MESH_WRAPPER_TYPE_GUID,
                kinds::MESH_WRAPPER_BASE_TYPE_GUID,
                kinds::MESH_WRAPPER_TYPE_VERSION,
                kinds::PARAMESH_MODULE,
            ),
            Self::Scope => (
                kinds::MESH_FEATURE_SCOPE_TYPE_GUID,
                kinds::MESH_FEATURE_SCOPE_BASE_TYPE_GUID,
                kinds::MESH_FEATURE_SCOPE_TYPE_VERSION,
                kinds::FUSION_MODULE,
            ),
            Self::ScopeBase => (
                kinds::MESH_SCOPE_BASE_RECORD_TYPE_GUID,
                kinds::MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID,
                kinds::MESH_SCOPE_BASE_RECORD_TYPE_VERSION,
                kinds::DATA_MODEL_MODULE,
            ),
            Self::SceneState => (
                kinds::MESH_SCENE_STATE_TYPE_GUID,
                kinds::MESH_SCENE_STATE_BASE_TYPE_GUID,
                kinds::MESH_SCENE_STATE_TYPE_VERSION,
                kinds::SCENE_MODULE,
            ),
            Self::SceneNode => (
                kinds::SCENE_NODE_TYPE_GUID,
                kinds::SCENE_NODE_BASE_TYPE_GUID,
                kinds::SCENE_NODE_TYPE_VERSION,
                kinds::SCENE_MODULE,
            ),
            Self::SceneAuxiliary => (
                kinds::SCENE_AUXILIARY_TYPE_GUID,
                kinds::SCENE_AUXILIARY_BASE_TYPE_GUID,
                kinds::SCENE_AUXILIARY_TYPE_VERSION,
                kinds::SCENE_MODULE,
            ),
            Self::CollectionOwner => (
                kinds::MESH_COLLECTION_OWNER_TYPE_GUID,
                kinds::MESH_COLLECTION_OWNER_BASE_TYPE_GUID,
                kinds::MESH_COLLECTION_OWNER_TYPE_VERSIONS[2],
                kinds::FUSION_MODULE,
            ),
            Self::BodyOwner => (
                kinds::MESH_BODY_OWNER_TYPE_GUID,
                kinds::MESH_BODY_OWNER_BASE_TYPE_GUID,
                kinds::MESH_BODY_OWNER_TYPE_VERSION,
                "Body",
            ),
        }
    }
}

/// Bulk records and their primary and secondary index entries.
#[derive(Default)]
struct Segment {
    bulk: Vec<u8>,
    primary: Vec<crate::metastream::RecordIndexEntry>,
    secondary: Vec<crate::metastream::RecordIndexEntry>,
    entities: BTreeMap<u32, Vec<u64>>,
}

impl Segment {
    fn record(&mut self, tag: u32, index: u32) -> Result<(), CodecError> {
        self.primary.push(crate::metastream::RecordIndexEntry {
            entity_id: u64::from(index),
            bulk_offset: cadmpeg_core::decode::u64_from_index(self.bulk.len()),
        });
        self.entities.entry(tag).or_default().push(u64::from(index));
        header(&mut self.bulk, tag, index)
    }
}

/// Author every selected mesh import. Each import is one Design segment.
pub(super) fn encode(
    imports: Vec<MeshImport<'_>>,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Vec<EncodedMesh>, CodecError> {
    let mut encoded = Vec::new();
    for import in imports {
        if let Some(mesh) = encode_import(&import, ctx)? {
            encoded.push(mesh);
        }
    }
    Ok(encoded)
}

fn encode_import(
    import: &MeshImport<'_>,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<EncodedMesh>, CodecError> {
    let indices = match &import.native {
        Some(native) => native_indices(native),
        None => fresh_indices(import.tessellations.len())?,
    };
    let native_bodies: Vec<Option<&DesignMeshBody>> = match &import.native {
        Some(native) => native.mesh.bodies().iter().map(Some).collect(),
        None => vec![None; import.tessellations.len()],
    };
    let mut losses = Vec::new();
    let mut containers = Vec::new();
    let mut entry_names = Vec::new();
    let mut fusion_uuids = Vec::new();
    let mut placements = Vec::new();
    for (tessellation, body) in import.tessellations.iter().zip(&native_bodies) {
        let placement = body.map_or(MILLIMETRE_PLACEMENT, |body| {
            body.placement.transform().cells()
        });
        let fusion_uuid = body.map_or_else(
            || derived_guid(&format!("fusion:{}", tessellation.id), true),
            |body| body.guid.value().to_owned(),
        );
        let mesh_uuid = body
            .and_then(|body| body.container_mesh_uuid.as_ref())
            .map_or_else(
                || derived_guid(&format!("mesh:{}", tessellation.id), false),
                |uuid| uuid.as_str().to_owned(),
            );
        let entry_name = body.map_or_else(
            || {
                format!(
                    "ParaMeshGeometry.{}.paramesh",
                    derived_guid(&format!("entry:{}", tessellation.id), true)
                )
            },
            |body| body.entry.name().to_owned(),
        );
        let Some(vertices) = container_vertices(tessellation, placement) else {
            losses.push(F3dLossCode::WriterTessellationOmitted.note(format!(
                "tessellation {} has a vertex outside the f32 container range; feature {} is omitted",
                tessellation.id, import.feature
            )));
            return Ok(None);
        };
        let triangles = tessellation.triangles();
        let channels = channels(tessellation, placement, &mut losses)?;
        let Some(bytes) = container::encode(&Container {
            fusion_uuid: &fusion_uuid,
            mesh_uuid: &mesh_uuid,
            vertices: &vertices,
            triangles: &triangles,
            feature_edges: tessellation.feature_edges(),
            channels,
        })?
        else {
            return Ok(None);
        };
        report_unrepresented(tessellation, &mut losses);
        ctx.charge_entities(1, "F3D mesh container")?;
        containers.push((entry_name.clone(), bytes));
        entry_names.push(entry_name);
        fusion_uuids.push(fusion_uuid);
        placements.push(placement);
    }
    let indices_scope_owner = indices.scope_owner;
    let mut segment = Segment::default();
    let body_count = u32::try_from(indices.bodies.len())
        .map_err(|_| CodecError::InvalidInput("F3D mesh body count exceeds u32".into()))?;
    ctx.charge_entities(
        6 + 7 * u64::from(body_count),
        "F3D Design mesh graph records",
    )?;
    // Collection, its nested base and the texture table.
    segment.record(Class::Collection.tag(), indices.collection)?;
    segment.bulk.extend_from_slice(&[0; 10]);
    segment.bulk.extend_from_slice(&body_count.to_le_bytes());
    segment.bulk.extend_from_slice(&[1, 1]);
    reference(&mut segment.bulk, indices.texture_table);
    header(
        &mut segment.bulk,
        Class::CollectionBase.tag(),
        indices.collection,
    )?;
    segment.bulk.extend_from_slice(&[0; 9]);
    segment.bulk.extend_from_slice(&body_count.to_le_bytes());
    for body in &indices.bodies {
        reference(&mut segment.bulk, body.body);
    }
    reference(&mut segment.bulk, indices.collection_owner);
    segment.record(Class::TextureTable.tag(), indices.texture_table)?;
    segment.bulk.extend_from_slice(&[0; 10]);
    segment.bulk.extend_from_slice(&0_u32.to_le_bytes());
    segment.bulk.extend_from_slice(&0_u32.to_le_bytes());
    let mut owners = std::collections::BTreeSet::new();
    for (ordinal, body) in indices.bodies.iter().enumerate() {
        let native = native_bodies[ordinal];
        segment.record(Class::EntryName.tag(), body.entry)?;
        segment.bulk.extend_from_slice(&[0; 10]);
        reference(&mut segment.bulk, body.guid);
        native_lp_utf16(&mut segment.bulk, &entry_names[ordinal])?;
        segment.record(Class::Guid.tag(), body.guid)?;
        segment.bulk.extend_from_slice(&[0; 21]);
        native_lp_ascii(&mut segment.bulk, &fusion_uuids[ordinal])?;
        reference(&mut segment.bulk, body.entry);
        segment.bulk.extend_from_slice(&[0; 4]);
        segment.record(Class::SceneState.tag(), body.state)?;
        segment.bulk.extend_from_slice(&[0; 34]);
        segment.bulk.push(1);
        scene_bounds(
            &mut segment.bulk,
            native.and_then(|native| native.scene_state.bounds()),
        );
        segment.record(Class::SceneAuxiliary.tag(), body.auxiliary)?;
        segment.record(Class::SceneNode.tag(), body.node)?;
        segment.bulk.extend_from_slice(&[0; 14]);
        segment.bulk.extend_from_slice(&2_u32.to_le_bytes());
        segment.bulk.extend_from_slice(&2_u32.to_le_bytes());
        reference(&mut segment.bulk, body.state);
        segment.bulk.extend_from_slice(&3_u32.to_le_bytes());
        reference(&mut segment.bulk, body.auxiliary);
        let node_bounds = native.and_then(|native| native.scene_node.bounds());
        match native.and_then(|native| native.scene_node.transform()) {
            Some(transform) => {
                segment.bulk.extend_from_slice(&[0; 25]);
                for cell in transform.value.cells() {
                    segment.bulk.extend_from_slice(&cell.to_le_bytes());
                }
                scene_bounds(&mut segment.bulk, node_bounds);
            }
            None => {
                segment.bulk.extend_from_slice(&[0; 24]);
                segment.bulk.push(1);
                scene_bounds(&mut segment.bulk, node_bounds);
            }
        }
        segment.record(Class::Wrapper.tag(), body.wrapper)?;
        segment.bulk.extend_from_slice(&[0; 10]);
        reference(&mut segment.bulk, body.body);
        segment.bulk.extend_from_slice(&[0; 8]);
        let start = segment.bulk.len();
        segment.record(Class::Body.tag(), body.body)?;
        segment
            .bulk
            .resize(start + mesh_body::LEN + kinds::SAME_SEGMENT_REFERENCE_BYTES, 0);
        let record = &mut segment.bulk[start..];
        put_matrix(record, mesh_body::FIRST_TRANSFORM, placements[ordinal]);
        put_matrix(record, mesh_body::SECOND_TRANSFORM, placements[ordinal]);
        put_reference(record, mesh_body::FEATURE_SCOPE_REFERENCE, indices.scope);
        put_reference(record, mesh_body::WRAPPER_REFERENCE, body.wrapper);
        put_reference(record, mesh_body::BODY_OWNER_REFERENCE, body.owner);
        put_reference(record, mesh_body::CONTAINER_GUID_REFERENCE, body.guid);
        put_reference(record, mesh_body::SCENE_NODE_REFERENCE, body.node);
        put_reference(record, mesh_body::LEN, indices.collection);
        owners.insert(body.owner);
    }
    // The feature scope: body list, reference table, history state, family
    // name, fixed tail and the closing same-index record.
    segment.record(Class::Scope.tag(), indices.scope)?;
    segment.bulk.extend_from_slice(&[0; 10]);
    segment.bulk.extend_from_slice(&body_count.to_le_bytes());
    for body in &indices.bodies {
        reference(&mut segment.bulk, body.body);
    }
    segment.bulk.extend_from_slice(&[0; SCOPE_BODY_GAP]);
    let members: Vec<u32> = match &import.native {
        Some(native) => native.members.iter().map(|(index, _)| *index).collect(),
        None => vec![indices.collection_owner],
    };
    segment.bulk.extend_from_slice(
        &u32::try_from(members.len())
            .map_err(|_| CodecError::InvalidInput("F3D mesh scope member count".into()))?
            .to_le_bytes(),
    );
    for member in &members {
        reference(&mut segment.bulk, *member);
    }
    segment
        .bulk
        .extend_from_slice(&indices.history_state.to_le_bytes());
    native_lp_utf16(
        &mut segment.bulk,
        import
            .native
            .as_ref()
            .map_or(MESH_FEATURE_KIND, |native| native.scope.source_kind_name()),
    )?;
    let mut tail = [0_u8; SCOPE_TAIL_LEN];
    tail[..4].copy_from_slice(&import.feature_ordinal.to_le_bytes());
    tail[SCOPE_TAIL_PREVIOUS_STATE..SCOPE_TAIL_PREVIOUS_STATE + 4]
        .copy_from_slice(&indices.previous_history_state.to_le_bytes());
    segment.bulk.extend_from_slice(&tail);
    segment.secondary.push(crate::metastream::RecordIndexEntry {
        entity_id: u64::from(indices.scope),
        bulk_offset: cadmpeg_core::decode::u64_from_index(segment.bulk.len()),
    });
    header(&mut segment.bulk, Class::ScopeBase.tag(), indices.scope)?;
    segment.bulk.extend_from_slice(&[0; 8]);
    reference(&mut segment.bulk, indices.scope_owner);
    let start = segment.bulk.len();
    segment.record(Class::CollectionOwner.tag(), indices.collection_owner)?;
    segment.bulk.resize(start + collection_owner::LEN, 0);
    put_reference(
        &mut segment.bulk[start..],
        collection_owner::COLLECTION_BACKLINK,
        indices.collection,
    );
    for owner in owners {
        segment.record(Class::BodyOwner.tag(), owner)?;
    }
    let mut types: Vec<GeneratedDesignType> = Class::ALL
        .iter()
        .map(|class| {
            let (guid, base, version, module) = class.registration();
            Ok(GeneratedDesignType {
                type_guid: guid.to_owned().try_into().map_err(CodecError::Malformed)?,
                base_type_guid: Some(base.to_owned().try_into().map_err(CodecError::Malformed)?),
                version,
                module: module.to_owned(),
                entity_ids: segment.entities.remove(&class.tag()).unwrap_or_default(),
            })
        })
        .collect::<Result<_, CodecError>>()?;
    if let Some(native) = &import.native {
        for (member, kind) in &native.members {
            let tag = 256 + u32::try_from(types.len())
                .map_err(|_| CodecError::InvalidInput("F3D mesh type count".into()))?;
            ctx.charge_entities(1, "F3D Design mesh member record")?;
            segment.record(tag, *member)?;
            let mut generated = GeneratedDesignType::from(*kind);
            generated.entity_ids = vec![u64::from(*member)];
            types.push(generated);
        }
    }
    let registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let meta = encode_design_metastream(&registry, &segment.primary, &segment.secondary)?
        .ok_or_else(|| CodecError::malformed("F3D mesh segment has no type table"))?;
    let native_records = import
        .native
        .as_ref()
        .and_then(|native| crate::ids::design_segment(&native.scope.id))
        .map(|source| {
            let mut indices: std::collections::BTreeSet<u32> = segment
                .primary
                .iter()
                .filter_map(|record| u32::try_from(record.entity_id).ok())
                .collect();
            indices.insert(indices_scope_owner);
            (source.to_owned(), indices)
        });
    Ok(Some(EncodedMesh {
        native_records,
        bulk: segment.bulk,
        meta,
        containers,
        feature: import.feature.clone(),
        tessellations: import
            .tessellations
            .iter()
            .map(|tessellation| tessellation.id.clone())
            .collect(),
        losses,
    }))
}

/// The linear block of a row-major placement.
fn linear(cells: [f64; 16]) -> [[f64; 3]; 3] {
    [
        [cells[0], cells[1], cells[2]],
        [cells[4], cells[5], cells[6]],
        [cells[8], cells[9], cells[10]],
    ]
}

/// Container coordinates whose placement reproduces each model vertex.
fn container_vertices(tessellation: &Tessellation, placement: [f64; 16]) -> Option<Vec<[f32; 3]>> {
    let vertices = tessellation.vertices();
    if placement == MILLIMETRE_PLACEMENT {
        return vertices
            .iter()
            .map(|vertex| {
                let point = vertex.get();
                narrow([point.x, point.y, point.z])
            })
            .collect();
    }
    let m = linear(placement);
    let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if !determinant.is_finite() || determinant == 0.0 {
        return None;
    }
    let cofactor = |row: usize, column: usize| {
        let rows: Vec<usize> = (0..3).filter(|index| *index != row).collect();
        let columns: Vec<usize> = (0..3).filter(|index| *index != column).collect();
        let minor = m[rows[0]][columns[0]] * m[rows[1]][columns[1]]
            - m[rows[0]][columns[1]] * m[rows[1]][columns[0]];
        if (row + column) % 2 == 0 {
            minor
        } else {
            -minor
        }
    };
    let inverse: [[f64; 3]; 3] =
        std::array::from_fn(|row| std::array::from_fn(|column| cofactor(column, row) / determinant));
    let translation = [placement[3], placement[7], placement[11]];
    vertices
        .iter()
        .map(|vertex| {
            let point = vertex.get();
            let model = [
                point.x / cadmpeg_asm::nurbs::reader::LEN_TO_MM - translation[0],
                point.y / cadmpeg_asm::nurbs::reader::LEN_TO_MM - translation[1],
                point.z / cadmpeg_asm::nurbs::reader::LEN_TO_MM - translation[2],
            ];
            narrow(std::array::from_fn(|row| {
                inverse[row][0] * model[0] + inverse[row][1] * model[1] + inverse[row][2] * model[2]
            }))
        })
        .collect()
}

fn narrow(values: [f64; 3]) -> Option<[f32; 3]> {
    Some([
        cadmpeg_core::convert::f32_from_f64(values[0])?,
        cadmpeg_core::convert::f32_from_f64(values[1])?,
        cadmpeg_core::convert::f32_from_f64(values[2])?,
    ])
}

/// Octahedral `(u, v)` of a unit direction (spec §1.1.2, code 5); both lie in
/// `[-1, 1]`.
fn octahedral(direction: [f64; 3]) -> Option<[f32; 2]> {
    let length = direction[0].abs() + direction[1].abs() + direction[2].abs();
    let (mut u, mut v) = (direction[0] / length, direction[1] / length);
    if direction[2] < 0.0 {
        let sign = |value: f64| if value < 0.0 { -1.0 } else { 1.0 };
        let (old_u, old_v) = (u, v);
        u = (1.0 - old_v.abs()) * sign(old_u);
        v = (1.0 - old_u.abs()) * sign(old_v);
    }
    Some([
        cadmpeg_core::convert::f32_from_f64(u)?,
        cadmpeg_core::convert::f32_from_f64(v)?,
    ])
}

/// Container-space packed normal of each model-space corner normal. The
/// placement maps a container normal with its cofactor, so the container
/// normal is the transposed linear block applied to the model normal.
fn corner_normal_bytes(tessellation: &Tessellation, placement: [f64; 16]) -> Vec<Vec<u8>> {
    let m = linear(placement);
    tessellation
        .per_corner_normals()
        .into_iter()
        .map(|normal| {
            let n = normal.get();
            let n = [n.x, n.y, n.z];
            let container: [f64; 3] =
                std::array::from_fn(|column| m[0][column] * n[0] + m[1][column] * n[1] + m[2][column] * n[2]);
            octahedral(container)
                .unwrap_or([0.0, 0.0])
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect()
        })
        .collect()
}

/// A corner channel from one element per corner: each vertex defaults to the
/// element of its first corner, and every corner that differs from its
/// vertex default is an override (spec §1.1.2, attribute channels).
fn corner_channel(
    role: u32,
    element_code: u32,
    corners: &[Vec<u8>],
    flat: &[u32],
    vertex_count: usize,
    item_size: usize,
) -> Channel {
    let mut defaults: Vec<Option<&[u8]>> = vec![None; vertex_count];
    for (element, vertex) in corners.iter().zip(flat) {
        if let Some(slot) = usize::try_from(*vertex)
            .ok()
            .and_then(|vertex| defaults.get_mut(vertex))
        {
            slot.get_or_insert(element.as_slice());
        }
    }
    let zero = vec![0; item_size];
    let mut values: Vec<u8> = defaults
        .iter()
        .flat_map(|element| element.unwrap_or(&zero).iter().copied())
        .collect();
    let mut positions = Vec::new();
    for (position, (element, vertex)) in corners.iter().zip(flat).enumerate() {
        let default = usize::try_from(*vertex)
            .ok()
            .and_then(|vertex| defaults.get(vertex).copied().flatten());
        if default != Some(element.as_slice()) {
            positions.push(u32::try_from(position).unwrap_or(u32::MAX));
            values.extend_from_slice(element);
        }
    }
    Channel {
        domain: ChannelDomain::Corner,
        role,
        element_code,
        values,
        positions,
        groups: Vec::new(),
    }
}

/// The override positions of a decoded corner channel whose table is in the
/// stored form: per-vertex defaults followed by the overrides in corner
/// order.
fn stored_positions(selectors: &[u32], flat: &[u32], vertex_count: u32) -> Option<Vec<u32>> {
    let mut positions = Vec::new();
    for (position, (selector, vertex)) in selectors.iter().zip(flat).enumerate() {
        if selector == vertex {
            continue;
        }
        let expected = vertex_count.checked_add(u32::try_from(positions.len()).ok()?)?;
        if *selector != expected {
            return None;
        }
        positions.push(u32::try_from(position).ok()?);
    }
    Some(positions)
}

/// The registry channels of one tessellation. Neutral corner normals and
/// triangle groups regenerate their channels; every other channel keeps its
/// stored elements.
fn channels(
    tessellation: &Tessellation,
    placement: [f64; 16],
    losses: &mut Vec<LossNote>,
) -> Result<Vec<Channel>, CodecError> {
    let triangles = tessellation.triangles();
    let flat: Vec<u32> = triangles.iter().flatten().copied().collect();
    let vertex_count = tessellation.vertex_count();
    let vertex_count_u32 = u32::try_from(vertex_count)
        .map_err(|_| CodecError::InvalidInput("F3D mesh vertex count exceeds u32".into()))?;
    let normals = corner_normal_bytes(tessellation, placement);
    let mut written_normals = normals.is_empty();
    let mut written_groups = tessellation.triangle_groups().is_empty();
    let mut channels = Vec::new();
    for channel in tessellation.channels() {
        let (role, code) = (channel.kind(), channel.flags());
        let Ok(item_size) = usize::try_from(channel.item_size()) else {
            continue;
        };
        if Layout::for_element(code).is_none() || item_size == 0 {
            losses.push(F3dLossCode::WriterTessellationOmitted.note(format!(
                "tessellation {}: channel with element code {code} has no container layout and is omitted",
                tessellation.id
            )));
            continue;
        }
        match (role, code, channel.addressing()) {
            (0, crate::paramesh::ELEMENT_PACKED_DIRECTION_CODE, ChannelAddressing::Corner { indices })
                if !written_normals =>
            {
                written_normals = true;
                channels.push(normal_channel(
                    channel.data(),
                    indices,
                    &normals,
                    &flat,
                    vertex_count_u32,
                    item_size,
                ));
            }
            (0, crate::paramesh::ELEMENT_TRIANGLE_DELTA_CODE, ChannelAddressing::Triangle { .. })
                if !written_groups =>
            {
                written_groups = true;
                channels.push(group_channel(tessellation, Some(channel.data()))?);
            }
            (1, crate::paramesh::ELEMENT_TRIANGLE_DELTA_CODE, ChannelAddressing::Triangle { .. }) => {}
            (_, _, ChannelAddressing::Vertex {}) => channels.push(Channel {
                domain: ChannelDomain::Vertex,
                role,
                element_code: code,
                values: channel.data().to_vec(),
                positions: Vec::new(),
                groups: Vec::new(),
            }),
            (_, _, ChannelAddressing::Corner { indices }) => {
                let elements: Vec<Vec<u8>> = indices
                    .iter()
                    .map(|selector| {
                        let start = usize::try_from(*selector).unwrap_or(usize::MAX).saturating_mul(item_size);
                        channel
                            .data()
                            .get(start..start.saturating_add(item_size))
                            .map_or_else(|| vec![0; item_size], <[u8]>::to_vec)
                    })
                    .collect();
                match stored_positions(indices, &flat, vertex_count_u32) {
                    Some(positions) => channels.push(Channel {
                        domain: ChannelDomain::Corner,
                        role,
                        element_code: code,
                        values: channel.data().to_vec(),
                        positions,
                        groups: Vec::new(),
                    }),
                    None => channels.push(corner_channel(
                        role,
                        code,
                        &elements,
                        &flat,
                        vertex_count,
                        item_size,
                    )),
                }
            }
            (_, _, ChannelAddressing::Triangle { .. }) => channels.push(Channel {
                domain: ChannelDomain::Triangle,
                role,
                element_code: code,
                values: channel.data().to_vec(),
                positions: Vec::new(),
                groups: Vec::new(),
            }),
        }
    }
    if !written_normals {
        channels.push(corner_channel(
            0,
            crate::paramesh::ELEMENT_PACKED_DIRECTION_CODE,
            &normals,
            &flat,
            vertex_count,
            8,
        ));
    }
    if !written_groups {
        channels.push(group_channel(tessellation, None)?);
    }
    Ok(channels)
}

/// The corner-normal channel. A stored table keeps its layout when every
/// corner that selects one element still carries one normal; otherwise the
/// channel is rebuilt from the corner normals.
fn normal_channel(
    stored: &[u8],
    selectors: &[u32],
    normals: &[Vec<u8>],
    flat: &[u32],
    vertex_count: u32,
    item_size: usize,
) -> Channel {
    let rebuilt = || {
        corner_channel(
            0,
            crate::paramesh::ELEMENT_PACKED_DIRECTION_CODE,
            normals,
            flat,
            usize::try_from(vertex_count).unwrap_or_default(),
            item_size,
        )
    };
    let Some(positions) = stored_positions(selectors, flat, vertex_count) else {
        return rebuilt();
    };
    let mut values = stored.to_vec();
    let mut assigned = vec![false; stored.len() / item_size.max(1)];
    for (selector, normal) in selectors.iter().zip(normals) {
        let Ok(entry) = usize::try_from(*selector) else {
            return rebuilt();
        };
        let start = entry * item_size;
        let Some(slot) = values.get_mut(start..start + item_size) else {
            return rebuilt();
        };
        if assigned[entry] && slot != normal.as_slice() {
            return rebuilt();
        }
        slot.copy_from_slice(normal);
        assigned[entry] = true;
    }
    Channel {
        domain: ChannelDomain::Corner,
        role: 0,
        element_code: crate::paramesh::ELEMENT_PACKED_DIRECTION_CODE,
        values,
        positions,
        groups: Vec::new(),
    }
}

/// The role-0 face-group partition. A stored value stream whose keys still
/// partition the triangles as the neutral groups do keeps its keys; otherwise
/// each group takes its ordinal as key.
fn group_channel(tessellation: &Tessellation, stored: Option<&[u8]>) -> Result<Channel, CodecError> {
    let groups = tessellation.triangle_groups();
    let guid = |ordinal: usize, source: Option<&String>| {
        source
            .filter(|guid| crate::bytes::is_guid_hyphenated(guid))
            .cloned()
            .unwrap_or_else(|| derived_guid(&format!("group:{}:{ordinal}", tessellation.id), true))
    };
    if let Some(keys) = stored.and_then(container::terminal_delta_values) {
        let mut records = Vec::new();
        let mut consistent = keys.len() == tessellation.triangle_count();
        for (ordinal, group) in groups.iter().enumerate() {
            let key = group
                .triangles
                .first()
                .and_then(|triangle| keys.get(usize::try_from(*triangle).ok()?))
                .copied();
            let Some(key) = key else {
                consistent = false;
                break;
            };
            consistent &= group
                .triangles
                .iter()
                .all(|triangle| usize::try_from(*triangle).ok().and_then(|index| keys.get(index)) == Some(&key))
                && records.iter().all(|(other, _)| *other != key);
            records.push((key, guid(ordinal, group.source_id.as_ref())));
        }
        if consistent {
            return Ok(Channel {
                domain: ChannelDomain::Triangle,
                role: 0,
                element_code: crate::paramesh::ELEMENT_TRIANGLE_DELTA_CODE,
                values: stored.unwrap_or_default().to_vec(),
                positions: Vec::new(),
                groups: records,
            });
        }
    }
    let mut keys = vec![0_u32; tessellation.triangle_count()];
    let mut records = Vec::new();
    for (ordinal, group) in groups.iter().enumerate() {
        let key = u32::try_from(ordinal)
            .map_err(|_| CodecError::InvalidInput("F3D mesh group count exceeds u32".into()))?;
        for triangle in &group.triangles {
            if let Some(slot) = usize::try_from(*triangle)
                .ok()
                .and_then(|index| keys.get_mut(index))
            {
                *slot = key;
            }
        }
        records.push((key, guid(ordinal, group.source_id.as_ref())));
    }
    let values = container::terminal_delta(&keys).ok_or_else(|| {
        CodecError::NotImplemented("F3D mesh group keys leave the i32 range".into())
    })?;
    Ok(Channel {
        domain: ChannelDomain::Triangle,
        role: 0,
        element_code: crate::paramesh::ELEMENT_TRIANGLE_DELTA_CODE,
        values,
        positions: Vec::new(),
        groups: records,
    })
}

/// Charge the tessellation fields a mesh container does not store.
fn report_unrepresented(tessellation: &Tessellation, losses: &mut Vec<LossNote>) {
    let mut fields = Vec::new();
    if !tessellation.texture_assignments().is_empty() {
        fields.push("texture assignments");
    }
    if tessellation.body.is_some() {
        fields.push("body");
    }
    if !tessellation.faces.is_empty() {
        fields.push("faces");
    }
    if tessellation.chordal_deflection().is_some() {
        fields.push("chordal deflection");
    }
    if tessellation.source_object.is_some() {
        fields.push("source object");
    }
    if !tessellation.texture_coordinates().is_empty() {
        fields.push("texture coordinates");
    }
    if !tessellation.vertex_normals().is_empty() {
        fields.push("vertex normals");
    }
    if !fields.is_empty() {
        losses.push(F3dLossCode::WriterTessellationOmitted.note(format!(
            "tessellation {}: {} are not encoded",
            tessellation.id,
            fields.join(", ")
        )));
    }
}
