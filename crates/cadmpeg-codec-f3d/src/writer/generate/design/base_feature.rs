// SPDX-License-Identifier: Apache-2.0
//! One `Base Feature` that stores the current bodies, the timeline that lists
//! it, and the records around them (f3d.md §3.1, "Records of a Base Feature
//! design", and the records of "Records of a sketch-and-extrude design" that
//! it names). Every reference is a same-segment reference in the plain form.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::codec::write::record_residual_unavailable;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, ShellId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Shell, ShellMember};

use super::frames::DesignRecords;
use super::history::{HEAD_STATE, ROOT_STATE};
use super::parameters::ParameterPlan;
use super::roster::values::Timestamps;
use super::roster::{self, Context, RosterBody};
use super::types;
use crate::native::F3dNative;
use crate::records::references::DesignClassTag;
use crate::records::sketch_links::{
    DesignPersistentIdText, PersistentDesignLink, PersistentSubentityTag,
};
use crate::writer::generate::presentation::{
    deterministic_guid, BrowserNodeTail, GeneratedDesignRegistry, TypeRow,
};

/// The persistent identity of the one `Base Feature`: the first value of the
/// identity counter.
const PERSISTENT_ID: u64 = 300;
/// The feature-family name of the scope.
const FAMILY: &str = "Base Feature";
/// The GUID the scope frame stores before its scope-reference count.
const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
/// The counter the timeline context keeps for the `Base Feature` family.
const FAMILY_COUNTER: &str = "DcBaseFeatureMetaType";
/// The flag of a dependency-table edge to the component record.
const COMPONENT_EDGE: u32 = 0x1000;
/// The flag of every other dependency-table edge.
const EDGE: u32 = 1;
/// The evaluation epoch of the item tail: `250`, below the `252` Fusion
/// writes, until the decoder reads an epoch-252 tail beside record indices
/// 209 to 212 (interim, F3D-EPOCH).
const EPOCH: u32 = 250;
/// The evaluation duration of the item tail, the median of Fusion's Base
/// Feature items (DR-913).
const DURATION: f64 = 0.000_631;

/// The bodies the `Base Feature` stores, in body order, each with its faces
/// in the order of its lump, shell and face chains.
pub(super) struct BaseFeaturePlan<'a> {
    bodies: Vec<PlannedBody<'a>>,
}

struct PlannedBody<'a> {
    id: &'a BodyId,
    faces: Vec<&'a FaceId>,
    edges: usize,
}

/// The bodies of `target`'s B-rep stream as one `Base Feature` stores them.
pub(super) fn plan(target: &CadIr) -> BaseFeaturePlan<'_> {
    let geometry = &target.geometry;
    let children = geometry.child_lists();
    let shells = geometry
        .shells
        .iter()
        .map(|shell| (&shell.id, shell))
        .collect::<HashMap<&ShellId, &Shell>>();
    let mut bodies = Vec::new();
    let mut body_of_face = HashMap::new();
    for (ordinal, body) in geometry.bodies.iter().enumerate() {
        let mut faces = Vec::new();
        let mut edges = BTreeSet::<&EdgeId>::new();
        for region in children.body_regions(&body.id) {
            for shell in &region.shells {
                faces.extend(children.shell_faces(shell).iter().map(|face| &face.id));
                edges.extend(shells.get(shell).into_iter().flat_map(|shell| {
                    shell.members.iter().filter_map(|member| match member {
                        ShellMember::WireEdge { id } => Some(id),
                        ShellMember::FreeVertex { .. } => None,
                    })
                }));
            }
        }
        for face in &faces {
            body_of_face.insert(*face, ordinal);
        }
        bodies.push((body, faces, edges));
    }
    let face_of_loop = geometry
        .loops
        .iter()
        .map(|face_loop| (&face_loop.id, &face_loop.face))
        .collect::<HashMap<_, _>>();
    for coedge in &geometry.coedges {
        let body = face_of_loop
            .get(&coedge.owner_loop)
            .and_then(|face| body_of_face.get(*face));
        if let Some(&body) = body {
            bodies[body].2.insert(&coedge.edge);
        }
    }
    BaseFeaturePlan {
        bodies: bodies
            .into_iter()
            .map(|(body, faces, edges)| PlannedBody {
                id: &body.id,
                faces,
                edges: edges.len(),
            })
            .collect(),
    }
}

/// The record indices of one result body.
struct BodyRecords {
    design_reference: u32,
    parameter_reference: u32,
    node: u32,
    tracking: u32,
    node_guid: String,
    parameter_guid: String,
}

/// The record indices of the design.
struct Indices {
    timeline: u32,
    context: u32,
    /// The scope, the result owner and its parameter companion, the
    /// tracked-results table, the item-tail companion and the scope's
    /// record-timestamp table, where the design has result bodies.
    item: Option<ItemIndices>,
    owner_collection: u32,
    component: u32,
    component_timestamps: u32,
    component_link: u32,
    context_12d00ac0: u32,
    context_016d8c93: u32,
    context_members: u32,
    dependencies: u32,
    persistent_ids: u32,
    origin: u32,
    origin_point: u32,
    origin_axes: u32,
    origin_planes: u32,
    collection: u32,
    collection_companion: u32,
    aggregate: u32,
}

struct ItemIndices {
    scope: u32,
    result_owner: u32,
    result_companion: u32,
    tracked_results: u32,
    item_companion: u32,
    scope_timestamps: u32,
}

/// The class tags of the written types.
struct Classes {
    timeline: DesignClassTag,
    context: DesignClassTag,
    scope: DesignClassTag,
    result_owner: DesignClassTag,
    tracking: DesignClassTag,
    tracked_results: DesignClassTag,
    item_companion: DesignClassTag,
    timestamps: DesignClassTag,
    owner_collection: DesignClassTag,
    component: DesignClassTag,
    component_link: DesignClassTag,
    context_12d00ac0: DesignClassTag,
    context_016d8c93: DesignClassTag,
    context_members: DesignClassTag,
    dependencies: DesignClassTag,
    persistent_ids: DesignClassTag,
    origin: DesignClassTag,
    origin_point: DesignClassTag,
    origin_axis: DesignClassTag,
    origin_plane: DesignClassTag,
    collection: DesignClassTag,
    collection_companion: DesignClassTag,
    aggregate: DesignClassTag,
    body_reference: DesignClassTag,
    object: DesignClassTag,
    tracking_entry: DesignClassTag,
    collection_base: DesignClassTag,
}

impl BaseFeaturePlan<'_> {
    /// Whether the design has a `Base Feature` item: the B-rep stream writes
    /// a body.
    pub(super) fn has_item(&self) -> bool {
        !self.bodies.is_empty()
    }

    /// Name each result body and each face it stores in the B-rep stream
    /// (f3d.md §3.1, "Names in the B-rep of a Base Feature"): the body group
    /// `(3, "300", j, 0, 0)` and the face group `(digits(t), t, [-300])`.
    /// Returns the losses of the face numbering of later bodies.
    pub(super) fn stamp(&self, native: &mut F3dNative) -> Result<Vec<LossNote>, CodecError> {
        let design_id = DesignPersistentIdText::try_from(PERSISTENT_ID.to_string())
            .map_err(CodecError::Malformed)?;
        let reference = i64::try_from(PERSISTENT_ID)
            .map_err(|_| CodecError::Malformed("Base Feature identity".into()))?;
        let mut first = 0u64;
        for (ordinal, body) in self.bodies.iter().enumerate() {
            let body_number = i64::try_from(ordinal + 1)
                .map_err(|_| CodecError::NotImplemented("Base Feature body count".into()))?;
            native.persistent_design_links.push(PersistentDesignLink {
                id: format!("f3d:generated:base-feature-body#{ordinal}"),
                target: AttributeTarget::Body(body.id.clone()),
                design_id: design_id.clone(),
                design_reference: body_number,
                ordinal: 0,
            });
            for (token, face) in (first..).zip(&body.faces) {
                let text = token.to_string();
                let selector = i64::try_from(text.len())
                    .map_err(|_| CodecError::NotImplemented("face token".into()))?;
                native
                    .persistent_subentity_tags
                    .push(PersistentSubentityTag {
                        id: format!("f3d:generated:base-feature-face#{token}"),
                        target: AttributeTarget::Face((*face).clone()),
                        selector,
                        token: cadmpeg_core::text::NonBlankString::new(text)
                            .ok_or_else(|| CodecError::Malformed("face token".into()))?,
                        token_integer: 0,
                        design_references: vec![-reference],
                        ordinal: 0,
                    });
            }
            // The next body's first token follows the faces and edges of
            // this body and one more (DR-887, the one multi-body import).
            let used = u64::try_from(body.faces.len() + body.edges + 1)
                .map_err(|_| CodecError::NotImplemented("face token".into()))?;
            first = first.checked_add(used).ok_or_else(|| {
                CodecError::NotImplemented("Base Feature face tokens overflow".into())
            })?;
        }
        Ok(record_residual_unavailable(
            "F3D",
            "numbers the faces of these later Base Feature bodies by the multi-body rule that f3d-open-items.md DR-887 leaves open",
            self.bodies.iter().skip(1).map(|body| body.id.to_string()),
        )
        .into_iter()
        .collect())
    }

    /// The design's records, the parameter records whose tails name its
    /// parameter collection, and the losses of the byte positions whose
    /// values the specification does not state.
    pub(super) fn encode(
        &self,
        parameters: &ParameterPlan,
        registry: &mut GeneratedDesignRegistry,
    ) -> Result<(Vec<u8>, DesignRecords, Vec<LossNote>), CodecError> {
        let bodies = self.body_records(registry)?;
        let parameter_indices = parameters.allocate(registry)?;
        let indices = Indices::allocate(registry, self.has_item())?;
        let classes = Classes::register(registry, &indices, &bodies)?;
        if let (Some(item), Some(nodes)) = (&indices.item, registry.browser_nodes.as_mut()) {
            nodes.tail = Some(BrowserNodeTail {
                scope: item.scope,
                paired_class: classes.object.clone(),
                context: indices.context,
            });
        }
        let parameter_bytes =
            parameters.encode(registry, &parameter_indices, Some(indices.collection))?;
        let mut out = DesignRecords::default();
        let writer = Writer {
            indices: &indices,
            classes: &classes,
            bodies: &bodies,
            stamps: Timestamps::FIXED,
            act_component: None,
        };
        writer.timeline(&mut out);
        writer.context(&mut out)?;
        if let Some(item) = &indices.item {
            writer.item(&mut out, item)?;
        }
        writer.components(&mut out)?;
        writer.origin(&mut out);
        writer.parameter_collection(&mut out, &parameter_indices)?;
        out.record(&classes.aggregate, indices.aggregate);
        writer.body_references(&mut out)?;
        Ok((parameter_bytes, out, residual_losses(&indices, &bodies)))
    }

    /// Each result body as the roster writes it.
    pub(super) fn roster_bodies<'a>(&self, target: &'a CadIr) -> Vec<RosterBody<'a>> {
        self.bodies
            .iter()
            .filter_map(|planned| {
                let body = target
                    .geometry
                    .bodies
                    .iter()
                    .find(|body| &body.id == planned.id)?;
                Some(RosterBody {
                    id: &body.id,
                    entities: planned.faces.len() + planned.edges,
                    sheet: body.kind == cadmpeg_ir::topology::BodyKind::Sheet,
                    node_guid: deterministic_guid("browser-node", body.id.as_str())
                        .to_ascii_lowercase(),
                    parameter_guid: deterministic_guid("body-parameter", body.id.as_str())
                        .to_ascii_lowercase(),
                })
            })
            .collect()
    }

    /// The timeline, its context and the records they name, the origin
    /// container, the parameter collection and its companion, the body
    /// tracking entries, and the user parameters, at the roster's indices.
    pub(super) fn roster_records(
        ctx: &Context<'_>,
        parameters: &ParameterPlan,
        out: &mut DesignRecords,
    ) -> Result<(), CodecError> {
        let indices = Indices::roster();
        let classes = Classes::of(ctx)?;
        let bodies = ctx
            .roster
            .bodies
            .iter()
            .zip(&ctx.bodies)
            .map(|(records, body)| BodyRecords {
                design_reference: records.design,
                parameter_reference: records.parameter,
                node: records.node,
                tracking: records.tracking,
                node_guid: body.node_guid.clone(),
                parameter_guid: body.parameter_guid.clone(),
            })
            .collect::<Vec<_>>();
        let writer = Writer {
            indices: &indices,
            classes: &classes,
            bodies: &bodies,
            stamps: ctx.values.stamps,
            act_component: Some(roster::ACT_COMPONENT),
        };
        writer.timeline(out);
        writer.context(out)?;
        if let Some(item) = &indices.item {
            writer.item(out, item)?;
        }
        writer.components(out)?;
        writer.origin_container(out);
        writer.parameter_collection(out, &ctx.roster.parameters)?;
        if ctx.roster.parameters.is_empty() {
            return Ok(());
        }
        parameters.roster_records(
            &ctx.class(types::PARAMETER.guid)?,
            &ctx.roster.parameters,
            indices.collection,
            out,
        )
    }

    /// The record indices of each result body. Its design body-reference
    /// record is the body-map entity suffix of the body.
    fn body_records(
        &self,
        registry: &mut GeneratedDesignRegistry,
    ) -> Result<Vec<BodyRecords>, CodecError> {
        let nodes = registry
            .browser_nodes
            .as_ref()
            .map(|nodes| {
                nodes
                    .nodes
                    .iter()
                    .map(|node| (&node.body, node))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let mut joined = Vec::new();
        for body in &self.bodies {
            let node = nodes.get(body.id).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "Base Feature body {} has no browser node",
                    body.id
                ))
            })?;
            let design_reference = u32::try_from(node.entity_suffix).map_err(|_| {
                CodecError::NotImplemented("Design body entity suffix exceeds u32".into())
            })?;
            joined.push((
                design_reference,
                node.record_index,
                node.node_guid.clone(),
                deterministic_guid("body-parameter", body.id.as_str()),
            ));
        }
        joined
            .into_iter()
            .map(|(design_reference, node, node_guid, parameter_guid)| {
                Ok(BodyRecords {
                    design_reference,
                    parameter_reference: registry.allocate()?,
                    node,
                    tracking: registry.allocate()?,
                    node_guid,
                    parameter_guid,
                })
            })
            .collect()
    }
}

impl Indices {
    /// The indices of the one-body roster.
    fn roster() -> Self {
        Self {
            timeline: 207,
            context: roster::CONTEXT,
            item: Some(ItemIndices {
                scope: roster::SCOPE,
                result_owner: 213,
                result_companion: 214,
                tracked_results: 212,
                item_companion: 211,
                scope_timestamps: 210,
            }),
            owner_collection: 199,
            component: 205,
            component_timestamps: 206,
            component_link: 208,
            context_12d00ac0: 202,
            context_016d8c93: 204,
            context_members: 200,
            dependencies: 201,
            persistent_ids: 203,
            origin: roster::ORIGIN,
            origin_point: 25,
            origin_axes: 26,
            origin_planes: 29,
            collection: roster::PARAMETER_COLLECTION,
            collection_companion: 18,
            aggregate: roster::AGGREGATE,
        }
    }

    fn allocate(registry: &mut GeneratedDesignRegistry, item: bool) -> Result<Self, CodecError> {
        let timeline = registry.allocate()?;
        let context = registry.allocate()?;
        let item = item
            .then(|| {
                Ok::<_, CodecError>(ItemIndices {
                    scope: registry.allocate()?,
                    result_owner: registry.allocate()?,
                    result_companion: registry.allocate()?,
                    tracked_results: registry.allocate()?,
                    item_companion: registry.allocate()?,
                    scope_timestamps: registry.allocate()?,
                })
            })
            .transpose()?;
        Ok(Self {
            timeline,
            context,
            item,
            owner_collection: registry.allocate()?,
            component: registry.allocate()?,
            component_timestamps: registry.allocate()?,
            component_link: registry.allocate()?,
            context_12d00ac0: registry.allocate()?,
            context_016d8c93: registry.allocate()?,
            context_members: registry.allocate()?,
            dependencies: registry.allocate()?,
            persistent_ids: registry.allocate()?,
            origin: registry.allocate()?,
            origin_point: registry.allocate()?,
            origin_axes: registry.allocate_run(3)?,
            origin_planes: registry.allocate_run(3)?,
            collection: registry.allocate()?,
            collection_companion: registry.allocate()?,
            aggregate: registry.allocate()?,
        })
    }

    /// The members of the owner collection and of the `B65B5682` record: the
    /// component record, then the scope and the result owner.
    fn members(&self) -> Vec<u32> {
        let mut members = vec![self.component];
        if let Some(item) = &self.item {
            members.extend([item.scope, item.result_owner]);
        }
        members
    }
}

impl Classes {
    /// The class tags of the roster's type table.
    fn of(ctx: &Context<'_>) -> Result<Self, CodecError> {
        let class = |row: TypeRow| ctx.class(row.guid);
        Ok(Self {
            timeline: class(types::TIMELINE)?,
            context: class(types::TIMELINE_CONTEXT)?,
            scope: class(types::BASE_FEATURE)?,
            result_owner: class(types::RESULT_OWNER)?,
            tracking: class(types::BODY_TRACKING)?,
            tracked_results: class(types::TRACKED_RESULTS)?,
            item_companion: class(types::ITEM_TAIL_COMPANION)?,
            timestamps: class(types::RECORD_TIMESTAMPS)?,
            owner_collection: class(types::OWNER_COLLECTION)?,
            component: class(types::COMPONENT_RECORD)?,
            component_link: class(types::CONTEXT_COMPONENT_LINK)?,
            context_12d00ac0: class(types::CONTEXT_12D00AC0)?,
            context_016d8c93: class(types::CONTEXT_016D8C93)?,
            context_members: class(types::CONTEXT_MEMBERS)?,
            dependencies: class(types::DEPENDENCIES)?,
            persistent_ids: class(types::PERSISTENT_IDS)?,
            origin: class(types::ORIGIN)?,
            origin_point: class(types::ORIGIN_POINT)?,
            origin_axis: class(types::ORIGIN_AXIS)?,
            origin_plane: class(types::ORIGIN_PLANE)?,
            collection: class(types::PARAMETER_COLLECTION)?,
            collection_companion: class(types::COLLECTION_COMPANION)?,
            aggregate: class(types::COMPONENT_AGGREGATE)?,
            body_reference: class(types::BODY_REFERENCE)?,
            object: class(types::DATA_MODEL_OBJECT)?,
            tracking_entry: class(types::TRACKING_ENTRY)?,
            collection_base: class(types::COLLECTION)?,
        })
    }

    fn register(
        registry: &mut GeneratedDesignRegistry,
        indices: &Indices,
        bodies: &[BodyRecords],
    ) -> Result<Self, CodecError> {
        let item = indices.item.as_ref();
        let item_ids =
            |select: fn(&ItemIndices) -> u32| item.map(select).into_iter().collect::<Vec<_>>();
        let mut register = |row: TypeRow, ids: &[u32]| registry.register(row, ids);
        let mut timestamps = item_ids(|item| item.result_companion);
        timestamps.extend(item_ids(|item| item.scope_timestamps));
        timestamps.push(indices.component_timestamps);
        let body_references = bodies
            .iter()
            .flat_map(|body| [body.design_reference, body.parameter_reference])
            .collect::<Vec<_>>();
        let tracking = bodies.iter().map(|body| body.tracking).collect::<Vec<_>>();
        let origin_axes = (0..3)
            .map(|axis| indices.origin_axes + axis)
            .collect::<Vec<_>>();
        let origin_planes = (0..3)
            .map(|plane| indices.origin_planes + plane)
            .collect::<Vec<_>>();
        for ancestor in types::ANCESTORS {
            register(ancestor, &[])?;
        }
        let classes = Self {
            object: register(types::DATA_MODEL_OBJECT, &[])?,
            tracking_entry: register(types::TRACKING_ENTRY, &[])?,
            collection_base: register(types::COLLECTION, &[])?,
            timeline: register(types::TIMELINE, &[indices.timeline])?,
            context: register(types::TIMELINE_CONTEXT, &[indices.context])?,
            scope: register(types::BASE_FEATURE, &item_ids(|item| item.scope))?,
            result_owner: register(types::RESULT_OWNER, &item_ids(|item| item.result_owner))?,
            tracking: register(types::BODY_TRACKING, &tracking)?,
            tracked_results: register(
                types::TRACKED_RESULTS,
                &item_ids(|item| item.tracked_results),
            )?,
            item_companion: register(
                types::ITEM_TAIL_COMPANION,
                &item_ids(|item| item.item_companion),
            )?,
            timestamps: register(types::RECORD_TIMESTAMPS, &timestamps)?,
            owner_collection: register(types::OWNER_COLLECTION, &[indices.owner_collection])?,
            component: register(types::COMPONENT_RECORD, &[indices.component])?,
            component_link: register(types::CONTEXT_COMPONENT_LINK, &[indices.component_link])?,
            context_12d00ac0: register(types::CONTEXT_12D00AC0, &[indices.context_12d00ac0])?,
            context_016d8c93: register(types::CONTEXT_016D8C93, &[indices.context_016d8c93])?,
            context_members: register(types::CONTEXT_MEMBERS, &[indices.context_members])?,
            dependencies: register(types::DEPENDENCIES, &[indices.dependencies])?,
            persistent_ids: register(types::PERSISTENT_IDS, &[indices.persistent_ids])?,
            origin: register(types::ORIGIN, &[indices.origin])?,
            origin_point: register(types::ORIGIN_POINT, &[indices.origin_point])?,
            origin_axis: register(types::ORIGIN_AXIS, &origin_axes)?,
            origin_plane: register(types::ORIGIN_PLANE, &origin_planes)?,
            collection: register(types::PARAMETER_COLLECTION, &[indices.collection])?,
            collection_companion: register(
                types::COLLECTION_COMPANION,
                &[indices.collection_companion],
            )?,
            aggregate: register(types::COMPONENT_AGGREGATE, &[indices.aggregate])?,
            body_reference: register(types::BODY_REFERENCE, &body_references)?,
        };
        register(types::AGGREGATE, &[])?;
        Ok(classes)
    }
}

/// Writes the records of one design.
struct Writer<'a> {
    indices: &'a Indices,
    classes: &'a Classes,
    bodies: &'a [BodyRecords],
    stamps: Timestamps,
    /// The ACT component record the `549FBB80` record names, where the
    /// archive holds an ACT segment.
    act_component: Option<u32>,
}

impl Writer<'_> {
    /// The feature timeline: ten zero bytes, the timeline context, and the
    /// item references.
    fn timeline(&self, out: &mut DesignRecords) {
        let indices = self.indices;
        out.record(&self.classes.timeline, indices.timeline)
            .zeros(10)
            .reference(indices.context);
        match &indices.item {
            Some(item) => {
                out.u32(1).reference(item.scope);
            }
            None => {
                out.u32(0);
            }
        }
    }

    /// The timeline context: the browser nodes of the result bodies, the
    /// family counter, context value `0`, the component link, the seven
    /// context records, and the component aggregate after the paired header.
    fn context(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let indices = self.indices;
        out.record(&self.classes.context, indices.context)
            .zeros(10)
            .u32(count(self.bodies.len())?);
        for body in self.bodies {
            out.reference(body.node);
        }
        if indices.item.is_some() {
            out.u32(1).lp_ascii(FAMILY_COUNTER)?.u64(1);
        } else {
            out.u32(0);
        }
        out.u64(0).u32(1).reference(indices.component_link);
        for record in [
            indices.context_12d00ac0,
            indices.context_members,
            indices.timeline,
            indices.dependencies,
            indices.persistent_ids,
            indices.owner_collection,
            indices.context_016d8c93,
        ] {
            out.reference(record);
        }
        out.header(&self.classes.object, indices.context)
            .zeros(8)
            .reference(indices.aggregate);
        Ok(())
    }

    /// The `Base Feature` scope and the records its item tail and result
    /// runs name.
    fn item(&self, out: &mut DesignRecords, item: &ItemIndices) -> Result<(), CodecError> {
        let (indices, classes) = (self.indices, self.classes);
        let bodies = count(self.bodies.len())?;
        out.record(&classes.scope, item.scope).zeros(8).byte(1).u32(
            bodies
                .checked_mul(2)
                .ok_or_else(|| CodecError::NotImplemented("Base Feature body count".into()))?,
        );
        for body in self.bodies {
            out.member(body.design_reference, 0);
        }
        for body in self.bodies {
            out.member(body.parameter_reference, 0);
        }
        out.reference(0).u32(bodies);
        for body in self.bodies {
            out.reference(body.parameter_reference);
        }
        out.byte(0).member(item.result_owner, 0).u32(bodies);
        for body in self.bodies {
            out.reference(body.node);
        }
        out.zeros(8).lp_utf16(NULL_GUID)?;
        out.zeros(3)
            .u32(1)
            .reference(item.result_owner)
            .u32(HEAD_STATE)
            .lp_utf16(FAMILY)?;
        // The 78-byte item tail: family ordinal 1, no label, state bytes
        // `00 00 00`, the preceding state, the two evaluation epochs around
        // the duration, `s0 = s1 = 0`.
        out.u32(1)
            .u32(0)
            .u32(0)
            .u32(0)
            .zeros(3)
            .reference(item.tracked_results)
            .byte(0)
            .u32(ROOT_STATE)
            .u32(EPOCH)
            .f64(DURATION)
            .u32(EPOCH)
            .reference(item.item_companion)
            .byte(1)
            .byte(0)
            .byte(0)
            .reference(item.scope_timestamps)
            .zeros(2);
        out.header(&classes.object, item.scope)
            .zeros(8)
            .reference(indices.owner_collection);

        out.record(&classes.result_owner, item.result_owner)
            .zeros(22)
            .reference(item.scope)
            .zeros(20)
            .byte(1)
            .zeros(2)
            .reference(item.result_companion)
            .byte(0)
            .reference(item.scope)
            .header(&classes.object, item.result_owner)
            .zeros(8)
            .reference(indices.owner_collection);
        timestamp_table(
            out,
            &classes.timestamps,
            item.result_companion,
            false,
            &[],
            (item.result_owner, self.stamps.result),
        )?;

        out.record(&classes.tracked_results, item.tracked_results)
            .zeros(10)
            .u32(bodies);
        for (key, body) in (1u64..).zip(self.bodies) {
            out.reference(body.tracking).u64(key);
        }
        out.u64(u64::from(bodies) + 1)
            .header(&classes.object, item.tracked_results)
            .zeros(9);
        for body in self.bodies {
            out.record(&classes.tracking, body.tracking).zeros(14);
            out.lp_utf16(&body.parameter_guid)?
                .lp_utf16(&body.node_guid)?
                .header(&classes.tracking_entry, body.tracking)
                .zeros(9);
        }
        out.record(&classes.item_companion, item.item_companion)
            .zeros(9)
            .byte(1)
            .zeros(8)
            .u32(0);
        timestamp_table(
            out,
            &classes.timestamps,
            item.scope_timestamps,
            true,
            &[
                (indices.component, self.stamps.component),
                (item.scope, self.stamps.scope),
            ],
            (item.scope, self.stamps.scope),
        )?;
        Ok(())
    }

    /// The owner collection, the component record and the records the
    /// timeline context names.
    fn components(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (indices, classes) = (self.indices, self.classes);
        let members = indices.members();
        out.record(&classes.owner_collection, indices.owner_collection)
            .zeros(10)
            .header(&classes.collection_base, indices.owner_collection)
            .zeros(8)
            .u32(count(members.len())?);
        for member in &members {
            out.reference(*member);
        }
        out.reference(indices.context);

        out.record(&classes.component, indices.component)
            .zeros(10)
            .byte(1)
            .reference(indices.aggregate)
            .byte(1)
            .zeros(2)
            .reference(indices.component_timestamps)
            .zeros(2)
            .header(&classes.object, indices.component)
            .zeros(8)
            .reference(indices.owner_collection);
        timestamp_table(
            out,
            &classes.timestamps,
            indices.component_timestamps,
            false,
            &[],
            (indices.component, self.stamps.component),
        )?;

        let (act_target, act_segment) = self.act_component.map_or((0, 0), |record| {
            (record, roster::segment::SegmentKind::Act.id())
        });
        out.record(&classes.component_link, indices.component_link)
            .zeros(22)
            .u32(3)
            .byte(0)
            .cross_segment_reference(u64::from(act_target), act_segment)
            .reference(indices.aggregate)
            .u32(u32::MAX)
            .u32(0);
        out.record(&classes.context_12d00ac0, indices.context_12d00ac0)
            .zeros(18)
            .reference(indices.context);
        out.record(&classes.context_016d8c93, indices.context_016d8c93)
            .zeros(14)
            .reference(indices.context);
        out.record(&classes.context_members, indices.context_members)
            .zeros(10)
            .reference(indices.context)
            .byte(1)
            .u32(count(members.len())?);
        for member in &members {
            out.reference(*member);
        }

        // The dependency table: the input map, the context, and the
        // dependent map of the same edges by the record read.
        out.record(&classes.dependencies, indices.dependencies)
            .zeros(10);
        match &indices.item {
            Some(item) => {
                out.byte(1).u32(2);
                dependency(out, item.scope, indices.component, COMPONENT_EDGE);
                dependency(out, item.result_owner, item.scope, EDGE);
                out.reference(indices.context).u32(2);
                dependency(out, indices.component, item.scope, COMPONENT_EDGE);
                dependency(out, item.scope, item.result_owner, EDGE);
            }
            None => {
                out.byte(0).u32(0).reference(indices.context).u32(0);
            }
        }

        // The persistent-id table in the forward form: `Base Feature -> 300`.
        out.record(&classes.persistent_ids, indices.persistent_ids)
            .zeros(10);
        match &indices.item {
            Some(item) => {
                out.u32(1)
                    .u64(u64::from(item.scope))
                    .u32(1)
                    .u64(PERSISTENT_ID)
                    .u32(1)
                    .u64(PERSISTENT_ID)
                    .u64(u64::from(item.scope));
            }
            None => {
                out.u32(0).u32(0);
            }
        }
        out.u32(0).reference(indices.context);
        Ok(())
    }

    /// The origin container and the header of each record it names.
    fn origin(&self, out: &mut DesignRecords) {
        let (indices, classes) = (self.indices, self.classes);
        self.origin_container(out);
        out.record(&classes.origin_point, indices.origin_point);
        for axis in 0..3 {
            out.record(&classes.origin_axis, indices.origin_axes + axis);
        }
        for plane in 0..3 {
            out.record(&classes.origin_plane, indices.origin_planes + plane);
        }
    }

    /// The origin container: the origin point, axes and planes, and the
    /// component aggregate.
    fn origin_container(&self, out: &mut DesignRecords) {
        let (indices, classes) = (self.indices, self.classes);
        out.record(&classes.origin, indices.origin)
            .zeros(12)
            .u32(7)
            .reference(indices.origin_point);
        for axis in 0..3 {
            out.reference(indices.origin_axes + axis);
        }
        for plane in 0..3 {
            out.reference(indices.origin_planes + plane);
        }
        out.reference(indices.aggregate);
    }

    /// The parameter collection and its companion.
    fn parameter_collection(
        &self,
        out: &mut DesignRecords,
        parameters: &[u32],
    ) -> Result<(), CodecError> {
        let (indices, classes) = (self.indices, self.classes);
        let parameter_count = count(parameters.len())?;
        out.record(&classes.collection, indices.collection)
            .zeros(19)
            .u32(parameter_count)
            .reference(indices.collection_companion)
            .byte(0)
            .u32(parameter_count);
        for parameter in parameters {
            out.reference(*parameter);
        }
        out.reference(indices.aggregate);
        out.record(&classes.collection_companion, indices.collection_companion)
            .zeros(8)
            .byte(1)
            .u32(1)
            .reference(indices.collection)
            .zeros(9);
        Ok(())
    }

    /// Each result body's design and parameter body-reference records, each
    /// naming the other.
    fn body_references(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        for body in self.bodies {
            for (record, property, other) in [
                (
                    body.design_reference,
                    "BASEFEAT_DM_BODY_REF",
                    body.parameter_reference,
                ),
                (
                    body.parameter_reference,
                    "BASEFEAT_PM_BODY_REF",
                    body.design_reference,
                ),
            ] {
                out.record(&self.classes.body_reference, record)
                    .zeros(9)
                    .byte(1)
                    .u32(1)
                    .lp_ascii(property)?
                    .lp_ascii("PassiveRefMetaType")?;
                out.reference(other).lp_utf16(&format!("0_{record}"))?;
            }
        }
        Ok(())
    }
}

/// A record-timestamp table: fourteen zero bytes, `01 01` for the scope's
/// table and `00 00` for the others, the `(record, timestamp)` entries, the
/// owner entry and eight zero bytes.
fn timestamp_table(
    out: &mut DesignRecords,
    class: &DesignClassTag,
    record: u32,
    scope: bool,
    entries: &[(u32, u64)],
    owner: (u32, u64),
) -> Result<(), CodecError> {
    let prefix = u8::from(scope);
    out.record(class, record)
        .zeros(14)
        .byte(prefix)
        .byte(prefix)
        .u32(count(entries.len())?);
    for (entry, timestamp) in entries.iter().chain([&owner]) {
        out.reference(*entry).u64(*timestamp);
    }
    out.zeros(8);
    Ok(())
}

/// One dependency-table entry with one edge.
fn dependency(out: &mut DesignRecords, record: u32, edge: u32, flag: u32) {
    out.reference(record).u32(1).reference(edge).u32(flag);
}

fn count(length: usize) -> Result<u32, CodecError> {
    u32::try_from(length).map_err(|_| CodecError::NotImplemented("Design run exceeds u32".into()))
}

/// The records whose bytes the specification does not state in full.
fn residual_losses(indices: &Indices, bodies: &[BodyRecords]) -> Vec<LossNote> {
    let mut records = vec![
        format!(
            "record {} (component aggregate E03784ED: its references and runs)",
            indices.aggregate
        ),
        format!(
            "record {} (E03784ED link 549FBB80: the target of its cross-segment reference)",
            indices.component_link
        ),
        format!(
            "record {} (origin point 0E18EEF2: its payload)",
            indices.origin_point
        ),
    ];
    records.extend((0..3).map(|axis| {
        format!(
            "record {} (origin axis 19031004: its payload)",
            indices.origin_axes + axis
        )
    }));
    records.extend((0..3).map(|plane| {
        format!(
            "record {} (origin plane 8D424E29: its payload and name)",
            indices.origin_planes + plane
        )
    }));
    if indices.item.is_none() {
        records.extend(
            [indices.owner_collection, indices.context_members].map(|record| {
                format!("record {record} (member list of a design with no Base Feature)")
            }),
        );
    }
    records.extend(bodies.iter().flat_map(|body| {
        [body.design_reference, body.parameter_reference].map(|record| {
            format!("record {record} (body-reference record D3937028: its GUID and presentation members)")
        })
    }));
    record_residual_unavailable(
        "F3D",
        "writes these Design records with zero bytes where the specification states no value",
        records,
    )
    .into_iter()
    .collect()
}
