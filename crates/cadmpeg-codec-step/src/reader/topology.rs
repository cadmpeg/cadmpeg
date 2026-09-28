// SPDX-License-Identifier: Apache-2.0
//! STEP boundary-representation ownership and orientation decoding.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::rc::Rc;

use super::geometry::curve_carrier_record;
use super::{source_numeric_id, RecordExt, ValueExt};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{CommitSession, DraftError, ModelDraft};
use cadmpeg_ir::eval::{
    model_curve_parameter_near_point_in_index_with_tolerance, model_curve_point_by_id,
    model_surface_partials_by_id, model_surface_point_by_id, nurbs_curve_parameter_domain,
    nurbs_pcurve_parameter_domain, pcurve_tangent, pcurve_uv,
};
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, IdentityKeyTail, LoopId, PcurveId,
    PointId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, PcurveUse, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3, COINCIDENCE_TOLERANCE};

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use self::admissions::{pcurve_admission_note, PcurveAdmission};
use super::geometry::surface_periodic_domains;
use super::index::CarrierIndex;
use super::StageOutcome;

const EPS_TOPOLOGY_READ_DEGENERATE: f64 = 1.0e-10;
const EPS_TOPOLOGY_READ_EXACT_GEOMETRY: f64 = 1.0e-12;

fn push_topology_vec<T>(
    values: &mut Vec<T>,
    value: T,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values.try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    values.push(value);
    Ok(())
}

fn one_topology_vec<T>(
    value: T,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut values = Vec::new();
    push_topology_vec(&mut values, value, ctx, operation)?;
    Ok(values)
}

fn append_topology_vec<T>(
    target: &mut Vec<T>,
    source: &mut Vec<T>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let count = source.len();
    ctx.charge_collection_items(u64_from_index(count), operation)?;
    target.try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64_from_index(count)))?;
    target.append(source);
    Ok(())
}

fn push_topology_group<K: Ord, V>(
    values: &mut BTreeMap<K, Vec<V>>,
    key: K,
    value: V,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    member_operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains_key(&key) {
        ctx.charge_collection_items(1, group_operation)?;
    }
    push_topology_vec(values.entry(key).or_default(), value, ctx, member_operation)
}

fn insert_topology_set<T: Ord>(
    values: &mut BTreeSet<T>,
    value: T,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains(&value) {
        ctx.charge_collection_items(1, operation)?;
    }
    values.insert(value);
    Ok(())
}

fn insert_topology_hash_set<T: Eq + Hash>(
    values: &mut HashSet<T>,
    value: T,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains(&value) {
        ctx.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    values.insert(value);
    Ok(())
}

fn insert_topology_map<K: Ord, V>(
    values: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains_key(&key) {
        ctx.charge_collection_items(1, operation)?;
    }
    values.insert(key, value);
    Ok(())
}

fn copy_topology_id<T: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>>(
    identity: &str,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<T, CodecError> {
    let bytes = ctx.copy_retained(identity.as_bytes(), operation)?;
    let text = String::from_utf8(bytes).map_err(CodecError::malformed)?;
    T::try_from(text).map_err(CodecError::malformed)
}

fn copy_topology_body_id(
    body: &BodyId,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<BodyId, CodecError> {
    copy_topology_id(body.as_str(), ctx, operation)
}

fn copy_topology_body_ids(
    bodies: &[BodyId],
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Vec<BodyId>, CodecError> {
    let mut copies = Vec::new();
    for body in bodies {
        ctx.charge_collection_items(1, operation)?;
        copies
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        copies.push(copy_topology_body_id(body, ctx, operation)?);
    }
    Ok(copies)
}

fn push_topology_body_group(
    groups: &mut BTreeMap<u64, Vec<BodyId>>,
    key: u64,
    body: &BodyId,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    member_operation: &'static str,
) -> Result<(), CodecError> {
    if !groups.contains_key(&key) {
        ctx.charge_collection_items(1, group_operation)?;
    }
    let copy = copy_topology_body_id(body, ctx, member_operation)?;
    push_topology_vec(groups.entry(key).or_default(), copy, ctx, member_operation)
}

fn insert_topology_body_group(
    groups: &mut BTreeMap<u64, BTreeSet<BodyId>>,
    key: u64,
    body: &BodyId,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    member_operation: &'static str,
) -> Result<(), CodecError> {
    if groups.get(&key).is_some_and(|bodies| bodies.contains(body)) {
        return Ok(());
    }
    if !groups.contains_key(&key) {
        ctx.charge_collection_items(1, group_operation)?;
    }
    ctx.charge_collection_items(1, member_operation)?;
    let copy = copy_topology_body_id(body, ctx, member_operation)?;
    groups.entry(key).or_default().insert(copy);
    Ok(())
}

fn push_topology_id_group<T: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>>(
    groups: &mut BTreeMap<u64, Vec<T>>,
    key: u64,
    identity: &str,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    member_operation: &'static str,
) -> Result<(), CodecError> {
    if !groups.contains_key(&key) {
        ctx.charge_collection_items(1, group_operation)?;
    }
    let copy = copy_topology_id(identity, ctx, member_operation)?;
    push_topology_vec(groups.entry(key).or_default(), copy, ctx, member_operation)
}

mod admissions;

pub(super) struct TopologyData {
    pub(super) body_by_root: BTreeMap<u64, Vec<BodyId>>,
    shape_representation_relationships: BTreeMap<u64, Vec<u64>>,
    pub(super) body_by_shell: BTreeMap<u64, BTreeSet<BodyId>>,
    pub(super) faces_by_source: BTreeMap<u64, Vec<FaceId>>,
    pub(super) edges_by_source: BTreeMap<u64, Vec<EdgeId>>,
    pub(super) vertices_by_source: BTreeMap<u64, Vec<VertexId>>,
}

fn topology_commit_error(
    context: &str,
    error: &DraftError,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    match error {
        DraftError::IdentityCollision(identity) => crate::decode_alloc::charged_format(
            ctx, "step_topology_commit_error_text",
            format_args!("{context} conflicts with decoded topology: identity collision at '{identity}': {error}"),
        ),
        DraftError::UnresolvedReference { .. }
        | DraftError::ReferenceWalk { .. }
        | DraftError::FeatureParents { .. } => {
            crate::decode_alloc::charged_format(
                ctx, "step_topology_commit_error_text",
                format_args!("{context} conflicts with decoded topology: {error}"),
            )
        }
    }
}

/// Body identifiers held with their temporary decode reservation.
#[derive(Debug)]
pub(super) struct AdmittedRepresentationBodies<'a> {
    values: Vec<BodyId>,
    reservation: Option<ScopedReservation<'a>>,
}

impl<'a> AdmittedRepresentationBodies<'a> {
    pub(super) fn into_parts(self) -> (Vec<BodyId>, Option<ScopedReservation<'a>>) {
        (self.values, self.reservation)
    }
}

impl std::ops::Deref for AdmittedRepresentationBodies<'_> {
    type Target = [BodyId];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

fn admitted_body_clone<'a>(
    bodies: &[BodyId],
    ctx: Option<&'a DecodeContext<'_>>,
    operation: &'static str,
) -> Result<AdmittedRepresentationBodies<'a>, cadmpeg_core::CodecError> {
    let bytes = if let Some(ctx) = ctx {
        ctx.charge_collection_items(u64_from_index(bodies.len()), operation)?;
        let bytes = bodies.iter().try_fold(
            u64_from_index(bodies.len())
                .checked_mul(u64_from_index(std::mem::size_of::<BodyId>()))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
            |total, body| {
                total
                    .checked_add(u64_from_index(body.as_str().len()))
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
            },
        )?;
        Some(ctx.reserve_scoped(bytes, operation)?)
    } else {
        None
    };
    let mut values = Vec::new();
    values.try_reserve_exact(bodies.len()).map_err(|_| {
        let requested = u64_from_index(bodies.len());
        match ctx {
            Some(ctx) => ctx.refuse_codec_limit(operation, 0, requested),
            None => cadmpeg_core::decode::refuse_local_limit(operation, 0, requested),
        }
    })?;
    values.extend_from_slice(bodies);
    Ok(AdmittedRepresentationBodies {
        values,
        reservation: bytes,
    })
}

fn cache_representation_bodies<'a>(
    cache: &mut BTreeMap<u64, AdmittedRepresentationBodies<'a>>,
    representation: u64,
    bodies: &[BodyId],
    ctx: Option<&'a DecodeContext<'_>>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut admitted = admitted_body_clone(bodies, ctx, "step_representation_body_cache_values")?;
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, "step_representation_body_cache_entries")?;
        if let Some(bytes) = admitted.reservation.as_mut() {
            bytes.grow(u64_from_index(std::mem::size_of::<(
                u64,
                AdmittedRepresentationBodies<'_>,
            )>()))?;
        }
    }
    cache.insert(representation, admitted);
    Ok(())
}

fn insert_body_id(
    bodies: &mut BTreeSet<BodyId>,
    body: &BodyId,
    ctx: Option<&DecodeContext<'_>>,
    bytes: &mut Option<ScopedReservation<'_>>,
) -> Result<(), cadmpeg_core::CodecError> {
    if bodies.contains(body) {
        return Ok(());
    }
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, "step_representation_body_set")?;
        if let Some(bytes) = bytes.as_mut() {
            let amount = u64_from_index(std::mem::size_of::<BodyId>())
                .checked_add(u64_from_index(body.as_str().len()))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("step_representation_body_set", u64::MAX - 1, u64::MAX)
                })?;
            bytes.grow(amount)?;
        }
    }
    bodies.insert(body.clone());
    Ok(())
}

/// Resolve the bodies represented by a representation item graph.
///
/// A representation can contain a body root directly or contain mapped items
/// whose representation map points at another representation. Keep this
/// traversal shared by topology classification and product placement so both
/// consumers apply the same graph and cycle rules.
pub(super) fn representation_bodies<'a>(
    representation: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    cache: &mut BTreeMap<u64, AdmittedRepresentationBodies<'a>>,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: Option<&'a DecodeContext<'_>>,
) -> Result<AdmittedRepresentationBodies<'a>, cadmpeg_core::CodecError> {
    if let Some(bodies) = cache.get(&representation) {
        return admitted_body_clone(bodies, ctx, "step_representation_body_cache_copy");
    }
    if ctx.is_none() && depth >= super::record_graph_limit(None) {
        return admitted_body_clone(&[], ctx, "step_representation_body_empty");
    }
    let _depth_guard = ctx
        .map(|ctx| ctx.enter_nested("step_representation_body_walk"))
        .transpose()?;
    if let Some(bodies) = topology.body_by_root.get(&representation) {
        let bodies = admitted_body_clone(bodies, ctx, "step_representation_body_root_copy")?;
        cache_representation_bodies(cache, representation, &bodies, ctx)?;
        return Ok(bodies);
    }
    if active.contains(&representation) {
        return admitted_body_clone(&[], ctx, "step_representation_body_empty");
    }
    let active_bytes = if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, "step_representation_body_active")?;
        Some(ctx.reserve_scoped(
            u64_from_index(std::mem::size_of::<u64>()),
            "step_representation_body_active",
        )?)
    } else {
        None
    };
    active.insert(representation);
    let mut body_ids = BTreeSet::new();
    let mut body_ids_bytes = ctx
        .map(|ctx| ctx.reserve_scoped(0, "step_representation_body_set"))
        .transpose()?;
    if let Some(items) = exchange
        .records()
        .get(&representation)
        .and_then(representation_item_values)
    {
        for item in items.iter().filter_map(ValueExt::reference) {
            let Some(record) = exchange.records().get(&item) else {
                continue;
            };
            if let Some(bodies) = topology.body_by_root.get(&item) {
                for body in bodies {
                    insert_body_id(&mut body_ids, body, ctx, &mut body_ids_bytes)?;
                }
                continue;
            }
            if record.partial("MAPPED_ITEM").is_none() {
                continue;
            }
            let Some(mapped_representation) = mapped_representation(record, exchange) else {
                continue;
            };
            let nested = representation_bodies(
                mapped_representation,
                exchange,
                topology,
                cache,
                active,
                depth + 1,
                ctx,
            )?;
            for body in nested.iter() {
                insert_body_id(&mut body_ids, body, ctx, &mut body_ids_bytes)?;
            }
        }
    }
    for related in topology
        .shape_representation_relationships
        .get(&representation)
        .into_iter()
        .flatten()
        .copied()
    {
        let nested =
            representation_bodies(related, exchange, topology, cache, active, depth + 1, ctx)?;
        for body in nested.iter() {
            insert_body_id(&mut body_ids, body, ctx, &mut body_ids_bytes)?;
        }
    }
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(
            u64_from_index(body_ids.len()),
            "step_representation_body_output",
        )?;
        if let Some(bytes) = body_ids_bytes.as_mut() {
            bytes.grow(
                u64_from_index(body_ids.len())
                    .checked_mul(u64_from_index(std::mem::size_of::<BodyId>()))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "step_representation_body_output",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?,
            )?;
        }
    }
    let bodies = body_ids.into_iter().collect::<Vec<_>>();
    active.remove(&representation);
    drop(active_bytes);
    cache_representation_bodies(cache, representation, &bodies, ctx)?;
    Ok(AdmittedRepresentationBodies {
        values: bodies,
        reservation: body_ids_bytes,
    })
}

/// A product shape can use a placement-only `SHAPE_REPRESENTATION` and link
/// it to the body-producing representation with `SHAPE_REPRESENTATION_RELATIONSHIP`.
/// The relationship is undirected for body reachability; retain both endpoints
/// in one indexed graph so resolution does not rescan the exchange per call.
fn shape_representation_relationships(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Vec<u64>>, CodecError> {
    let mut related = BTreeMap::<u64, Vec<u64>>::new();
    for record in exchange.records().values() {
        let Some(relationship) = record.partial("SHAPE_REPRESENTATION_RELATIONSHIP") else {
            continue;
        };
        let mut references = relationship
            .parameters
            .iter()
            .filter_map(ValueExt::reference);
        let (first, second) = match (references.next(), references.next()) {
            (Some(first), Some(second)) => (first, second),
            _ => {
                let Some(base) = record.partial("REPRESENTATION_RELATIONSHIP") else {
                    continue;
                };
                let mut references = base.parameters.iter().filter_map(ValueExt::reference);
                let Some(first) = references.next() else {
                    continue;
                };
                let Some(second) = references.next() else {
                    continue;
                };
                (first, second)
            }
        };
        push_topology_group(&mut related, first, second, ctx,
            "step_shape_relationship_groups", "step_shape_relationship_members")?;
        push_topology_group(&mut related, second, first, ctx,
            "step_shape_relationship_groups", "step_shape_relationship_members")?;
    }
    for representations in related.values_mut() {
        representations.sort_unstable();
        representations.dedup();
    }
    Ok(related)
}

fn representation_items(record: &RawRecord) -> Option<Vec<u64>> {
    named_refs(record, "REPRESENTATION", 1).or_else(|| {
        record
            .simple_name()
            .and_then(|name| named_refs(record, name, 1))
    })
}

fn representation_item_values(record: &RawRecord) -> Option<&[Value]> {
    if record.partials.len() == 1 {
        return entity_parameter(record, "REPRESENTATION", 1)
            .and_then(reference_values)
            .or_else(|| {
                record
                    .simple_name()
                    .and_then(|name| entity_parameter(record, name, 1))
                    .and_then(reference_values)
            });
    }
    record
        .partial("REPRESENTATION")?
        .parameters
        .iter()
        .find_map(reference_values)
}

fn named_reference_values<'a>(
    record: &'a RawRecord,
    name: &str,
    simple_index: usize,
) -> Option<&'a [Value]> {
    if record.partials.len() == 1 {
        return entity_parameter(record, name, simple_index).and_then(reference_values);
    }
    record
        .partial(name)?
        .parameters
        .iter()
        .find_map(reference_values)
}

fn reference_values(value: &Value) -> Option<&[Value]> {
    value
        .list()
        .filter(|items| items.iter().all(|item| item.reference().is_some()))
}

fn mapped_representation(record: &RawRecord, exchange: &Exchange) -> Option<u64> {
    let map = named_reference(record, "MAPPED_ITEM", 1, 0)?;
    exchange
        .records()
        .get(&map)
        .and_then(|map| named_reference(map, "REPRESENTATION_MAP", 1, 1))
}

pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    carrier_index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<TopologyData>, CodecError> {
    let mut commit_session = CommitSession::new(ir);
    let mut result = StageOutcome {
        value: TopologyData {
            body_by_root: BTreeMap::new(),
            shape_representation_relationships: shape_representation_relationships(exchange, ctx)?,
            body_by_shell: BTreeMap::new(),
            faces_by_source: BTreeMap::new(),
            edges_by_source: BTreeMap::new(),
            vertices_by_source: BTreeMap::new(),
        },
        claims: HashSet::new(),
        losses: Vec::new(),
        notes: Vec::new(),
    };
    let mut losses: Vec<LossNote> = Vec::new();
    for (&id, record) in exchange.records() {
        let Some(name) = most_specific(record, &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL"])
        else {
            continue;
        };
        if record.partials.len() != 1 || matches!(record.parameter(1), Some(Value::Derived)) {
            continue;
        }
        push_topology_vec(&mut result.losses,
            StepLossCode::OrientedShellOmitsCfsFaces
                .note(format!(
                    "{name} #{id} omits the derived `cfs_faces` slot required by ISO 10303-21; \
                 read the shell element from positional slot 1"
                ))
                .with_provenance(
                    cadmpeg_ir::SourceProvenance::root(
                        crate::dialect::FORMAT,
                        record.span.start as u64,
                    )
                    .with_tag("oriented_shell"),
                ),
        ctx, "step_topology_losses")?;
    }
    let vertices = vertex_defs(exchange, ctx)?;
    let edges = edge_defs(exchange, ctx)?;
    let oriented = oriented_defs(exchange, ctx)?;
    let shells = shell_defs(exchange, ctx)?;
    let point_positions = carrier_index;
    for (vertex_id, vertex) in exchange.entities("VERTEX_POINT") {
        let Some(point_id) = named_reference(vertex, "VERTEX_POINT", 1, 0) else {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "VERTEX_POINT #{vertex_id} has no resolvable point carrier"
            )), ctx, "step_topology_losses")?;
            continue;
        };
        if !carrier_index.points.contains_key(&point_id) {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "VERTEX_POINT #{vertex_id} has unresolved point carrier #{point_id}"
            )), ctx, "step_topology_losses")?;
        }
    }
    let mut built_wire_models = BTreeSet::new();
    for (&representation, record) in exchange.records() {
        let Some(items) = representation_item_values(record) else {
            continue;
        };
        for model in items.iter().filter_map(Value::reference) {
            if !exchange.records().get(&model).is_some_and(|record| {
                record.partial("EDGE_BASED_WIREFRAME_MODEL").is_some()
            }) {
                continue;
            }
        if built_wire_models.contains(&model) {
            insert_topology_hash_set(&mut result.claims, representation, ctx, "step_topology_claims")?;
            if let Some(body_ids) = result.body_by_root.get(&model) {
                let copies = copy_topology_body_ids(body_ids, ctx, "step_topology_root_bodies")?;
                insert_topology_map(&mut result.body_by_root, representation, copies, ctx, "step_topology_root_groups")?;
            }
            continue;
        }
        let outcome = build_wire(
            model,
            exchange,
            &vertices,
            &edges,
            point_positions,
            &mut losses,
            ctx,
        )?;
        let (built, failures) = outcome.into_parts();
        let mut committed = 0;
        for mut built in built {
            if let Err(error) = commit_session.commit_model(built.draft) {
                push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(topology_commit_error(
                    &format!("EDGE_BASED_WIREFRAME_MODEL #{model}"),
                    &error,
                ctx,
                )?), ctx, "step_topology_losses")?;
            } else {
                committed += 1;
                insert_topology_set(&mut built_wire_models, model, ctx, "step_built_wire_models")?;
                insert_topology_hash_set(&mut built.typed, representation, ctx, "step_wire_typed")?;
                push_topology_body_group(
                    &mut result.body_by_root, model, &built.body_id, ctx,
                    "step_topology_root_groups", "step_topology_root_bodies",
                )?;
                for typed in std::mem::take(&mut built.typed) {
                    insert_topology_hash_set(&mut result.claims, typed, ctx, "step_topology_claims")?;
                }
            }
        }
        if committed == 0 {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "EDGE_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
            )), ctx, "step_topology_losses")?;
        } else if let Some(failures) = failures {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "EDGE_BASED_WIREFRAME_MODEL #{model} omitted {} unresolved connected edge set(s)",
                failures.count
            )), ctx, "step_topology_losses")?;
        }
        }
    }
    for (model, record) in exchange.entities("SHELL_BASED_WIREFRAME_MODEL") {
        let scope_root = named_reference_values(record, "SHELL_BASED_WIREFRAME_MODEL", 1)
            .into_iter()
            .flatten()
            .filter_map(Value::reference)
            .any(|shell| result.body_by_shell.contains_key(&shell));
        let outcome = build_shell_wire(
            model,
            exchange,
            &vertices,
            &edges,
            point_positions,
            scope_root,
            &mut losses,
            ctx,
        )?;
        let (built, failures) = outcome.into_parts();
        let mut committed = 0;
        for mut built in built {
            if let Err(error) = commit_session.commit_model(built.draft) {
                push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(topology_commit_error(
                    &format!("SHELL_BASED_WIREFRAME_MODEL #{model}"),
                    &error,
                ctx,
                )?), ctx, "step_topology_losses")?;
            } else {
                committed += 1;
                for shell in &built.shell_sources {
                    insert_topology_body_group(
                        &mut result.body_by_shell, *shell, &built.body_id, ctx,
                        "step_topology_shell_groups", "step_topology_shell_bodies",
                    )?;
                }
                push_topology_body_group(
                    &mut result.body_by_root, model, &built.body_id, ctx,
                    "step_topology_root_groups", "step_topology_root_bodies",
                )?;
                for typed in std::mem::take(&mut built.typed) {
                    insert_topology_hash_set(&mut result.claims, typed, ctx, "step_topology_claims")?;
                }
            }
        }
        if committed == 0 {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "SHELL_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
            )), ctx, "step_topology_losses")?;
        } else if let Some(failures) = failures {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "SHELL_BASED_WIREFRAME_MODEL #{model} omitted {} unresolved wire shell(s)",
                failures.count
            )), ctx, "step_topology_losses")?;
        }
    }
    let mut decoded_pcurves = BTreeSet::new();
    for pcurve in &commit_session.document().model.pcurves {
        if let Some(id) = source_numeric_id(pcurve.id.as_str(), "pcurve") {
            insert_topology_set(&mut decoded_pcurves, id, ctx, "step_decoded_topology_pcurves")?;
        }
    }
    let topology_root_types = [
        "SHELL_BASED_SURFACE_MODEL",
        "FACE_BASED_SURFACE_MODEL",
        "FACETED_BREP",
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
    ];
    let mut distinct_roots = BTreeSet::new();
    for (_, record) in exchange.entities_any(&topology_root_types) {
        if let Some(key) = root_key(record, exchange, &shells, ctx)? {
            insert_topology_set(&mut distinct_roots, key, ctx, "step_distinct_topology_roots")?;
        }
    }
    let distinct_root_count = distinct_roots.len();
    let scope_distinct_roots = distinct_root_count > 1;
    let mut built_roots = BTreeMap::<RootKey, RootBuilt>::new();
    let mut representation_cache = BTreeMap::new();
    let mut admissions: Vec<PcurveAdmission> = Vec::new();
    for (id, record) in exchange.entities_any(&topology_root_types) {
        let Some(key) = root_key(record, exchange, &shells, ctx)? else {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "STEP topology root #{id} does not resolve to a complete connected topology graph",
            )), ctx, "step_topology_losses")?;
            continue;
        };
        if let Some(root_built) = built_roots.get(&key) {
            insert_topology_hash_set(&mut result.claims, id, ctx, "step_topology_claims")?;
            let copies = copy_topology_body_ids(&root_built.body_ids, ctx, "step_topology_root_bodies")?;
            insert_topology_map(&mut result.body_by_root, id, copies, ctx, "step_topology_root_groups")?;
            for (&shell, body_ids) in &root_built.body_by_shell {
                for body in body_ids {
                    insert_topology_body_group(
                        &mut result.body_by_shell, shell, body, ctx,
                        "step_topology_shell_groups", "step_topology_shell_bodies",
                    )?;
                }
            }
            continue;
        }
        // A STEP file can define independent topology roots that reuse a
        // global edge or vertex without reusing the shell record. CADIR
        // identities are global, so every distinct root receives an owner
        // scope when more than one root is present. This preserves each root
        // without making the result depend on source record order.
        let scope_root = scope_distinct_roots;
        let outcome = build(
            id,
            record,
            exchange,
            commit_session.document(),
            &vertices,
            &edges,
            &oriented,
            &shells,
            &decoded_pcurves,
            point_positions,
            scope_root,
            &mut losses,
            ctx,
        )?;
        let (built, failures) = outcome.into_parts();
        let failure_message = failures
            .as_ref()
            .and_then(|failures| failures.first.as_ref())
            .map(BuildFailure::message);
        let mut body_ids = Vec::new();
        let mut body_by_shell = BTreeMap::<u64, BTreeSet<BodyId>>::new();
        for mut built in built {
            drop_committed_surfaces(&mut built.draft, &mut commit_session);
            if let Err(error) = commit_session.commit_model(built.draft) {
                push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(topology_commit_error(
                    &format!("STEP topology root #{id}"),
                    &error,
                ctx,
                )?), ctx, "step_topology_losses")?;
            } else {
                for shell in &built.shell_sources {
                    insert_topology_body_group(
                        &mut result.body_by_shell, *shell, &built.body_id, ctx,
                        "step_topology_shell_groups", "step_topology_shell_bodies",
                    )?;
                    insert_topology_body_group(
                        &mut body_by_shell, *shell, &built.body_id, ctx,
                        "step_topology_built_shell_groups", "step_topology_built_shell_bodies",
                    )?;
                }
                push_topology_vec(
                    &mut body_ids,
                    copy_topology_body_id(&built.body_id, ctx, "step_topology_built_bodies")?,
                    ctx,
                    "step_topology_built_bodies",
                )?;
                for typed in std::mem::take(&mut built.typed) {
                    insert_topology_hash_set(&mut result.claims, typed, ctx, "step_topology_claims")?;
                }
                // A rejected draft transfers no relation, so only a committed
                // body contributes its admitted relations to the document.
                append_topology_vec(
                    &mut admissions, &mut built.pcurve_admissions, ctx,
                    "step_topology_admissions",
                )?;
            }
        }
        if body_ids.is_empty() {
            if let Some(message) = failure_message {
                push_topology_vec(&mut result.losses,
                    StepLossCode::TopologyRootRejected
                        .note(format!("STEP topology root #{id} rejected: {message}")),
                ctx, "step_topology_losses")?;
            } else {
                push_topology_vec(&mut result.losses, StepLossCode::TopologyRootIncomplete.note(format!(
                        "STEP topology root #{id} does not resolve to a complete connected topology graph",
                    )), ctx, "step_topology_losses")?;
            }
        } else {
            let copies = copy_topology_body_ids(&body_ids, ctx, "step_topology_root_bodies")?;
            insert_topology_map(&mut result.body_by_root, id, copies, ctx, "step_topology_root_groups")?;
            insert_topology_map(&mut built_roots,
                key,
                RootBuilt {
                    body_ids,
                    body_by_shell,
                },
                ctx,
                "step_topology_built_roots",
            )?;
            if let Some(failures) = failures {
                let detail = failure_message
                    .as_deref()
                    .map_or_else(String::new, |message| format!(": {message}"));
                push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                    "STEP topology root #{id} omitted {} unresolved shell(s){detail}",
                    failures.count,
                )), ctx, "step_topology_losses")?;
            }
        }
    }
    // Every admitted relation shares one class of unproved invariant, so the
    // document reports the class once with its count and named examples.
    if let Some(note) = pcurve_admission_note(&admissions) {
        push_topology_vec(&mut result.losses, note, ctx, "step_topology_losses")?;
    }
    for (id, record) in exchange.entities("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION") {
        let omitted = geometric_set_omissions(record, exchange, carrier_index, ctx)?;
        if !omitted.is_empty() {
            let note = geometric_set_omission_message(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION", id, &omitted, ctx,
            )?;
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(note), ctx, "step_topology_losses")?;
        }
        let Some(mut built) = build_geometric_set(id, record, exchange, carrier_index, &mut losses)
        else {
            if mark_standalone_geometric_set(
                id,
                record,
                exchange,
                carrier_index,
                &mut result.claims,
                ctx,
            )? {
                continue;
            }
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no decoded bounded surfaces"
            )), ctx, "step_topology_losses")?;
            continue;
        };
        if let Err(error) = commit_session.commit_model(built.draft) {
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(topology_commit_error(
                &format!("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id}"),
                &error,
            ctx,
            )?), ctx, "step_topology_losses")?;
        } else {
            push_topology_body_group(
                &mut result.body_by_root, id, &built.body_id, ctx,
                "step_topology_root_groups", "step_topology_root_bodies",
            )?;
            for typed in std::mem::take(&mut built.typed) {
                insert_topology_hash_set(&mut result.claims, typed, ctx, "step_topology_claims")?;
            }
        }
    }
    for (id, record) in exchange.entities_any(&[
        "SHAPE_REPRESENTATION",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION",
    ]) {
        let Some(representation_type) = most_specific(
            record,
            &[
                "ADVANCED_BREP_SHAPE_REPRESENTATION",
                "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION",
                "SHAPE_REPRESENTATION",
            ],
        ) else {
            continue;
        };
        let omitted = geometric_set_omissions(record, exchange, carrier_index, ctx)?;
        if !omitted.is_empty() {
            let note = geometric_set_omission_message(representation_type, id, &omitted, ctx)?;
            push_topology_vec(&mut losses, StepLossCode::DecodeWarning.note(note), ctx, "step_topology_losses")?;
        }
        mark_standalone_geometric_set(id, record, exchange, carrier_index, &mut result.claims, ctx)?;
    }
    for (id, record) in exchange.entities_any(&[
        "MANIFOLD_SURFACE_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_REPRESENTATION",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "SHAPE_REPRESENTATION",
    ]) {
        if most_specific(
            record,
            &[
                "MANIFOLD_SURFACE_SHAPE_REPRESENTATION",
                "ADVANCED_BREP_REPRESENTATION",
                "ADVANCED_BREP_SHAPE_REPRESENTATION",
                "SHAPE_REPRESENTATION",
            ],
        )
        .is_none()
        {
            continue;
        }
        let has_body = !representation_bodies(
            id,
            exchange,
            &result,
            &mut representation_cache,
            &mut BTreeSet::new(),
            0,
            Some(ctx),
        )?
        .is_empty();
        if has_body {
            insert_topology_hash_set(&mut result.claims, id, ctx, "step_topology_claims")?;
        }
    }
    for face in &commit_session.document().model.faces {
        if let Some(source) = source_numeric_id(face.id.as_str(), "face") {
            push_topology_id_group(
                &mut result.faces_by_source, source, face.id.as_str(), ctx,
                "step_topology_source_face_groups", "step_topology_source_faces",
            )?;
        }
    }
    for edge in &commit_session.document().model.edges {
        if let Some(source) = source_numeric_id(edge.id.as_str(), "edge") {
            push_topology_id_group(
                &mut result.edges_by_source, source, edge.id.as_str(), ctx,
                "step_topology_source_edge_groups", "step_topology_source_edges",
            )?;
        }
    }
    for vertex in &commit_session.document().model.vertices {
        if let Some(source) = source_numeric_id(vertex.id.as_str(), "vertex") {
            push_topology_id_group(
                &mut result.vertices_by_source, source, vertex.id.as_str(), ctx,
                "step_topology_source_vertex_groups", "step_topology_source_vertices",
            )?;
        }
    }
    append_topology_vec(&mut result.losses, &mut losses, ctx, "step_topology_loss_merge")?;
    Ok(result)
}

fn geometric_set_omissions(
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<u64>, CodecError> {
    let Some(set_ids) = representation_item_values(representation) else {
        return Ok(Vec::new());
    };
    let mut omitted = Vec::new();
    for set_id in set_ids.iter().filter_map(Value::reference) {
        let Some(set) = exchange.records().get(&set_id) else {
            continue;
        };
        let Some(set_type) = most_specific(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            continue;
        };
        let Some(members) = named_reference_values(set, set_type, 1) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            if !carrier_index.points.contains_key(&member)
                && !carrier_index.curves.contains_key(&member)
                && !carrier_index.surfaces.contains_key(&member)
            {
                push_topology_vec(&mut omitted, member, ctx, "step_geometric_set_omissions")?;
            }
        }
    }
    Ok(omitted)
}

struct OmittedMembers<'a>(&'a [u64]);

impl std::fmt::Display for OmittedMembers<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, member) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "#{member}")?;
        }
        Ok(())
    }
}

fn geometric_set_omission_message(
    representation_type: &str,
    id: u64,
    omitted: &[u64],
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    crate::decode_alloc::charged_format(
        ctx,
        "step_geometric_set_omission_text",
        format_args!(
            "{representation_type} #{id} omitted unsupported or unresolved member(s): {}",
            OmittedMembers(omitted)
        ),
    )
}

enum BuildOutcome {
    Built(Vec<Built>),
    Partial {
        built: Vec<Built>,
        failures: BuildFailures,
    },
}

struct BuildFailures {
    count: NonZeroUsize,
    first: Option<BuildFailure>,
}

impl BuildOutcome {
    fn push(&mut self, value: Built, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        match self {
            Self::Built(built) | Self::Partial { built, .. } => {
                push_topology_vec(built, value, ctx, "step_topology_built_outcome")
            }
        }
    }

    fn fail(&mut self, failure: Option<BuildFailure>) {
        match self {
            Self::Built(built) => {
                *self = Self::Partial {
                    built: std::mem::take(built),
                    failures: BuildFailures {
                        count: NonZeroUsize::MIN,
                        first: failure,
                    },
                };
            }
            Self::Partial { failures, .. } => {
                failures.count = failures.count.saturating_add(1);
                if failures.first.is_none() {
                    failures.first = failure;
                }
            }
        }
    }

    fn into_parts(self) -> (Vec<Built>, Option<BuildFailures>) {
        match self {
            Self::Built(built) => (built, None),
            Self::Partial { built, failures } => (built, Some(failures)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CarrierKind {
    BoundLoopReference,
    BoundOrientation,
    Coedge,
    CoedgeEdge,
    ConnectedFaceSet,
    ConnectedFaceSetMemberList,
    ConnectedOuterShell,
    EdgeDefinition,
    EdgeLoopCarrier,
    EdgeLoopContinuity,
    EdgeLoopMember,
    EdgeLoopMemberList,
    FaceAttributes,
    FaceBound,
    FaceBoundAttributes,
    FaceBoundCarrier,
    FaceCarrier,
    FaceRecord,
    FaceWithMultipleOuterBounds,
    ImplicitFacePlane,
    LoopRecord,
    OrientedEdgeDefinition,
    PolyLoopPointCarrier,
    PolyLoopPointList,
    PolyVertexPoint,
    ShellCarrier,
    ShellFaceList,
    ShellRecord,
    ShellType,
    TopologyDraft,
    TopologyRootCarrier,
    VertexDefinition,
    VertexLoopReference,
    VertexPoint,
}

impl std::fmt::Display for CarrierKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BoundLoopReference => "bound loop reference",
            Self::BoundOrientation => "bound orientation",
            Self::Coedge => "coedge",
            Self::CoedgeEdge => "coedge edge",
            Self::ConnectedFaceSet => "connected face set",
            Self::ConnectedFaceSetMemberList => "connected face set member list",
            Self::ConnectedOuterShell => "connected outer shell",
            Self::EdgeDefinition => "edge definition",
            Self::EdgeLoopCarrier => "edge loop carrier",
            Self::EdgeLoopContinuity => "edge loop continuity",
            Self::EdgeLoopMember => "edge loop member",
            Self::EdgeLoopMemberList => "edge loop member list",
            Self::FaceAttributes => "face attributes",
            Self::FaceBound => "face bound",
            Self::FaceBoundAttributes => "face bound attributes",
            Self::FaceBoundCarrier => "face bound carrier",
            Self::FaceCarrier => "face carrier",
            Self::FaceRecord => "face record",
            Self::FaceWithMultipleOuterBounds => "face with multiple outer bounds",
            Self::ImplicitFacePlane => "implicit face plane",
            Self::LoopRecord => "loop record",
            Self::OrientedEdgeDefinition => "oriented edge definition",
            Self::PolyLoopPointCarrier => "poly loop point carrier",
            Self::PolyLoopPointList => "poly loop point list",
            Self::PolyVertexPoint => "poly vertex point",
            Self::ShellCarrier => "shell carrier",
            Self::ShellFaceList => "shell face list",
            Self::ShellRecord => "shell record",
            Self::ShellType => "shell type",
            Self::TopologyDraft => "topology draft",
            Self::TopologyRootCarrier => "topology root carrier",
            Self::VertexDefinition => "vertex definition",
            Self::VertexLoopReference => "vertex loop reference",
            Self::VertexPoint => "vertex point",
        })
    }
}

#[derive(Clone, Debug)]
struct BuildFailure {
    record_id: u64,
    carrier_kind: CarrierKind,
}

impl BuildFailure {
    fn message(&self) -> String {
        format!(
            "{} #{} missing or unresolved",
            self.carrier_kind, self.record_id
        )
    }
}

fn require_carrier<T>(
    value: Option<T>,
    failure: &mut Option<BuildFailure>,
    record_id: u64,
    carrier_kind: CarrierKind,
) -> Option<T> {
    if value.is_none() {
        failure.get_or_insert(BuildFailure {
            record_id,
            carrier_kind,
        });
    }
    value
}

fn note_failure(failure: &mut Option<BuildFailure>, record_id: u64, carrier_kind: CarrierKind) {
    failure.get_or_insert(BuildFailure {
        record_id,
        carrier_kind,
    });
}

fn build_wire(
    id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, Rc<EdgeDef>>,
    point_positions: &CarrierIndex,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<BuildOutcome, CodecError> {
    let Some(model) = exchange.records().get(&id) else {
        return Ok(BuildOutcome::Partial {
            built: Vec::new(),
            failures: BuildFailures {
                count: NonZeroUsize::MIN,
                first: None,
            },
        });
    };
    let Some(sets) = named_reference_values(model, "EDGE_BASED_WIREFRAME_MODEL", 1) else {
        return Ok(BuildOutcome::Partial {
            built: Vec::new(),
            failures: BuildFailures {
                count: NonZeroUsize::MIN,
                first: None,
            },
        });
    };
    let scoped = sets.len() > 1;
    let mut outcome = BuildOutcome::Built(Vec::new());
    for set_id in sets.iter().filter_map(Value::reference) {
        match build_wire_set(
            id,
            set_id,
            exchange,
            vdefs,
            edefs,
            point_positions,
            scoped,
            losses,
            ctx,
        ) {
            Ok(Some(value)) => outcome.push(value, ctx)?,
            Ok(None) => outcome.fail(None),
            Err(error) => return Err(error),
        }
    }
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
fn build_wire_set(
    id: u64,
    set_id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, Rc<EdgeDef>>,
    point_positions: &CarrierIndex,
    scoped: bool,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Built>, CodecError> {
    let Some(set) = exchange.records().get(&set_id) else {
        return Ok(None);
    };
    let Some(set_type) = most_specific(set, &["CONNECTED_EDGE_SUB_SET", "CONNECTED_EDGE_SET"])
    else {
        return Ok(None);
    };
    let Some(used_edges) = connected_set_members(set, set_type) else {
        return Ok(None);
    };
    let suffix = if scoped {
        IdentityKeyTail::empty().dash(key_word!("set")).dash(set_id)
    } else {
        IdentityKeyTail::empty()
    };
    let mut typed = HashSet::new();
    insert_topology_hash_set(&mut typed, id, ctx, "step_wire_typed")?;
    insert_topology_hash_set(&mut typed, set_id, ctx, "step_wire_typed")?;
    if set_type == "CONNECTED_EDGE_SUB_SET"
        && !validate_subset_parent(set_id, set, set_type, exchange, losses, ctx)?
    {
        typed.remove(&set_id);
    }
    let mut used_vertices = BTreeSet::new();
    let mut wire_edges = Vec::new();
    let mut built_edges = Vec::new();
    for edge_id in used_edges.iter().filter_map(Value::reference) {
        let Some(edge) = edefs.get(&edge_id) else {
            return Ok(None);
        };
        let (start, end) = edge.curve_vertices();
        let edge_suffix = IdentityKeyTail::empty()
            .dash(key_word!("wire"))
            .dash(id)
            .dash(key_word!("set"))
            .dash(set_id);
        let ir_id = EdgeId::from(ids::data(
            kind!("edge"),
            IdentityKey::from(edge_id).with_tail(&edge_suffix),
        ));
        let vertex_suffix = IdentityKeyTail::empty()
            .dash(key_word!("wire"))
            .dash(id)
            .dash(key_word!("set"))
            .dash(set_id);
        push_topology_vec(
            &mut wire_edges,
            copy_topology_id(ir_id.as_str(), ctx, "step_wire_edge_ids")?,
            ctx,
            "step_wire_edge_ids",
        )?;
        push_topology_vec(&mut built_edges, Edge {
            id: ir_id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(edge_curve_id_reported(
                edge_id, edge, exchange, losses, ctx,
            )?),
            start: VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(start).with_tail(&vertex_suffix),
            )),
            end: VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(end).with_tail(&vertex_suffix),
            )),
            tolerance: None,
        }, ctx, "step_wire_edges")?;
        insert_topology_set(&mut used_vertices, start, ctx, "step_wire_used_vertices")?;
        insert_topology_set(&mut used_vertices, end, ctx, "step_wire_used_vertices")?;
        insert_topology_hash_set(&mut typed, edge_id, ctx, "step_wire_typed")?;
        if let Some(parent) = edge.parent() {
            insert_topology_hash_set(&mut typed, parent, ctx, "step_wire_typed")?;
        }
    }
    let vertex_suffix = IdentityKeyTail::empty()
        .dash(key_word!("wire"))
        .dash(id)
        .dash(key_word!("set"))
        .dash(set_id);
    let mut built_vertices = Vec::new();
    for vertex_id in used_vertices {
        let Some(vertex) = vdefs.get(&vertex_id) else {
            return Ok(None);
        };
        if point_positions.get(vertex.point).is_none() {
            return Ok(None);
        }
        push_topology_vec(&mut built_vertices, Vertex {
            id: VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(vertex_id).with_tail(&vertex_suffix),
            )),
            point: PointId::from(ids::data(kind!("point"), vertex.point)),
            tolerance: None,
        }, ctx, "step_wire_vertices")?;
        insert_topology_hash_set(&mut typed, vertex_id, ctx, "step_wire_typed")?;
    }
    let body = BodyId::from(ids::data(
        kind!("body"),
        IdentityKey::from(id).with_tail(&suffix),
    ));
    let region = RegionId::from(ids::data(
        kind!("region"),
        IdentityKey::from(id).with_tail(&suffix),
    ));
    let shell = ShellId::from(ids::data(
        kind!("shell"),
        IdentityKey::from(id).with_tail(&suffix),
    ));
    let shell_value = match Shell::new(
        copy_topology_id(shell.as_str(), ctx, "step_wire_shell_id_copy")?,
        copy_topology_id(region.as_str(), ctx, "step_wire_region_id_copy")?,
        Vec::new(),
        wire_edges,
        Vec::new(),
    ) {
        Ok(shell) => shell,
        Err(error) => {
            push_topology_vec(losses,
                StepLossCode::DecodeWarning
                    .note(format!("CONNECTED_EDGE_SET #{set_id}: {error}")),
                ctx, "step_topology_losses")?;
            return Ok(None);
        }
    };
    let staged = staged_topology(
        typed,
        built_vertices,
        built_edges,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        one_topology_vec(shell_value, ctx, "step_wire_shells")?,
        Region {
            id: copy_topology_id(region.as_str(), ctx, "step_wire_region_id_copy")?,
            body: copy_topology_body_id(&body, ctx, "step_wire_body_id_copy")?,
            shells: one_topology_vec(shell, ctx, "step_wire_region_shells")?,
        },
        Body {
            id: copy_topology_body_id(&body, ctx, "step_wire_body_id_copy")?,
            kind: BodyKind::Wire,
            regions: one_topology_vec(region, ctx, "step_wire_body_regions")?,
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    );
    let mut built = match staged {
        Ok(built) => built,
        Err(error) => {
            push_topology_vec(losses,
                StepLossCode::DecodeWarning.note(format!("CONNECTED_EDGE_SET #{set_id}: {error}")),
                ctx, "step_topology_losses")?;
            return Ok(None);
        }
    };
    insert_topology_set(&mut built.shell_sources, set_id, ctx, "step_wire_shell_sources")?;
    Ok(Some(built))
}

fn build_shell_wire(
    id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, Rc<EdgeDef>>,
    point_positions: &CarrierIndex,
    scope_root: bool,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<BuildOutcome, CodecError> {
    let Some(model) = exchange.records().get(&id) else {
        return Ok(BuildOutcome::Partial {
            built: Vec::new(),
            failures: BuildFailures {
                count: NonZeroUsize::MIN,
                first: None,
            },
        });
    };
    let Some(shell_ids) = named_reference_values(model, "SHELL_BASED_WIREFRAME_MODEL", 1) else {
        return Ok(BuildOutcome::Partial {
            built: Vec::new(),
            failures: BuildFailures {
                count: NonZeroUsize::MIN,
                first: None,
            },
        });
    };
    let scoped = shell_ids.len() > 1;
    let mut outcome = BuildOutcome::Built(Vec::new());
    for shell_id in shell_ids.iter().filter_map(Value::reference) {
        match build_shell_wire_set(
            id,
            shell_id,
            exchange,
            vdefs,
            edefs,
            point_positions,
            scoped,
            scope_root,
            losses,
            ctx,
        ) {
            Ok(Some(value)) => outcome.push(value, ctx)?,
            Ok(None) => outcome.fail(None),
            Err(error) => return Err(error),
        }
    }
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
fn build_shell_wire_set(
    id: u64,
    shell_id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, Rc<EdgeDef>>,
    point_positions: &CarrierIndex,
    scoped: bool,
    scope_root: bool,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Built>, CodecError> {
    let Some(shell_record) = exchange.records().get(&shell_id) else {
        return Ok(None);
    };
    let mut typed = HashSet::new();
    insert_topology_hash_set(&mut typed, id, ctx, "step_wire_typed")?;
    insert_topology_hash_set(&mut typed, shell_id, ctx, "step_wire_typed")?;
    let mut edge_uses = Vec::new();
    let mut used_vertices = BTreeSet::new();
    let mut free_vertices = BTreeSet::new();
    if shell_record.partial("WIRE_SHELL").is_some() {
        let Some(loop_ids) = named_reference_values(shell_record, "WIRE_SHELL", 1) else {
            return Ok(None);
        };
        for loop_id in loop_ids.iter().filter_map(Value::reference) {
            let Some(loop_record) = exchange.records().get(&loop_id) else {
                return Ok(None);
            };
            if loop_record.partial("EDGE_LOOP").is_some() {
                let Some(oriented_ids) = named_reference_values(loop_record, "EDGE_LOOP", 1) else {
                    return Ok(None);
                };
                for oriented_id in oriented_ids.iter().filter_map(Value::reference) {
                    let Some(oriented) = exchange.records().get(&oriented_id) else {
                        return Ok(None);
                    };
                    let Some(edge_id) = oriented_edge_reference(oriented) else {
                        return Ok(None);
                    };
                    let Some(edge) = edefs.get(&edge_id) else {
                        return Ok(None);
                    };
                    let Some(forward) = oriented_edge_forward(oriented) else {
                        return Ok(None);
                    };
                    push_topology_vec(&mut edge_uses, (edge_id, oriented_id, forward), ctx, "step_wire_edge_uses")?;
                    insert_topology_set(&mut used_vertices, edge.vertices().0, ctx, "step_wire_used_vertices")?;
                    insert_topology_set(&mut used_vertices, edge.vertices().1, ctx, "step_wire_used_vertices")?;
                    for claim in [loop_id, oriented_id, edge_id] {
                        insert_topology_hash_set(&mut typed, claim, ctx, "step_wire_typed")?;
                    }
                    if let Some(parent) = edge.parent() {
                        insert_topology_hash_set(&mut typed, parent, ctx, "step_wire_typed")?;
                    }
                }
            } else if loop_record.partial("VERTEX_LOOP").is_some() {
                let Some(vertex) = named_reference(loop_record, "VERTEX_LOOP", 1, 0) else {
                    return Ok(None);
                };
                insert_topology_set(&mut used_vertices, vertex, ctx, "step_wire_used_vertices")?;
                insert_topology_set(&mut free_vertices, vertex, ctx, "step_wire_free_vertices")?;
                for claim in [loop_id, vertex] {
                    insert_topology_hash_set(&mut typed, claim, ctx, "step_wire_typed")?;
                }
            } else {
                return Ok(None);
            }
        }
    } else if shell_record.partial("VERTEX_SHELL").is_some() {
        let Some(loop_id) = named_reference(shell_record, "VERTEX_SHELL", 1, 0) else {
            return Ok(None);
        };
        let Some(loop_record) = exchange.records().get(&loop_id) else {
            return Ok(None);
        };
        if loop_record.partial("VERTEX_LOOP").is_none() {
            return Ok(None);
        }
        let Some(vertex) = named_reference(loop_record, "VERTEX_LOOP", 1, 0) else {
            return Ok(None);
        };
        insert_topology_set(&mut used_vertices, vertex, ctx, "step_wire_used_vertices")?;
        insert_topology_set(&mut free_vertices, vertex, ctx, "step_wire_free_vertices")?;
        for claim in [loop_id, vertex] {
            insert_topology_hash_set(&mut typed, claim, ctx, "step_wire_typed")?;
        }
    } else {
        return Ok(None);
    }
    let suffix = if scoped {
        IdentityKeyTail::empty()
            .dash(key_word!("shell"))
            .dash(shell_id)
    } else {
        IdentityKeyTail::empty()
    };
    let vertex_suffix = IdentityKeyTail::empty()
        .dash(key_word!("wire"))
        .dash(id)
        .dash(key_word!("shell"))
        .dash(shell_id);
    let mut edges = Vec::new();
    let mut wire_edges = Vec::new();
    for (index, (edge_id, oriented_id, forward)) in edge_uses.into_iter().enumerate() {
        let Some(edge) = edefs.get(&edge_id) else {
            return Ok(None);
        };
        let (curve_start, curve_end) = edge.curve_vertices();
        let (start, end) = if forward {
            (curve_start, curve_end)
        } else {
            (curve_end, curve_start)
        };
        let ir_id = EdgeId::from(ids::data(
            kind!("edge"),
            IdentityKey::from(edge_id)
                .dash(key_word!("wire"))
                .dash(id)
                .dash(shell_id)
                .dash(oriented_id)
                .dash(index),
        ));
        push_topology_vec(
            &mut wire_edges,
            copy_topology_id(ir_id.as_str(), ctx, "step_wire_edge_ids")?,
            ctx,
            "step_wire_edge_ids",
        )?;
        push_topology_vec(&mut edges, Edge {
            id: ir_id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(edge_curve_id_reported(
                edge_id, edge, exchange, losses, ctx,
            )?),
            start: VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(start).with_tail(&vertex_suffix),
            )),
            end: VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(end).with_tail(&vertex_suffix),
            )),
            tolerance: None,
        }, ctx, "step_wire_edges")?;
    }
    let mut vertices = Vec::new();
    for vertex_id in used_vertices {
        let Some(vertex) = vdefs.get(&vertex_id) else {
            return Ok(None);
        };
        if point_positions.get(vertex.point).is_none() {
            return Ok(None);
        }
        push_topology_vec(&mut vertices, Vertex {
                id: VertexId::from(ids::data(
                    kind!("vertex"),
                    IdentityKey::from(vertex_id).with_tail(&vertex_suffix),
                )),
                point: PointId::from(ids::data(kind!("point"), vertex.point)),
                tolerance: None,
        }, ctx, "step_wire_vertices")?;
    }
    let body = BodyId::from(ids::data(
        kind!("body"),
        IdentityKey::from(id).with_tail(&suffix),
    ));
    let region = RegionId::from(ids::data(
        kind!("region"),
        IdentityKey::from(id).with_tail(&suffix),
    ));
    let shell = shell_identity(id, shell_id, scope_root);
    let mut free_vertex_ids = Vec::new();
    for vertex in free_vertices {
        push_topology_vec(
            &mut free_vertex_ids,
            VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(vertex).with_tail(&vertex_suffix),
            )),
            ctx,
            "step_wire_free_vertex_ids",
        )?;
    }
    let shell_value = match Shell::new(
        copy_topology_id(shell.as_str(), ctx, "step_wire_shell_id_copy")?,
        copy_topology_id(region.as_str(), ctx, "step_wire_region_id_copy")?,
        Vec::new(),
        wire_edges,
        free_vertex_ids,
    ) {
        Ok(shell) => shell,
        Err(error) => {
            push_topology_vec(losses,
                StepLossCode::DecodeWarning.note(format!("wire shell #{shell_id}: {error}")),
                ctx, "step_topology_losses")?;
            return Ok(None);
        }
    };
    let staged = staged_topology(
        typed,
        vertices,
        edges,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        one_topology_vec(shell_value, ctx, "step_wire_shells")?,
        Region {
            id: copy_topology_id(region.as_str(), ctx, "step_wire_region_id_copy")?,
            body: copy_topology_body_id(&body, ctx, "step_wire_body_id_copy")?,
            shells: one_topology_vec(shell, ctx, "step_wire_region_shells")?,
        },
        Body {
            id: copy_topology_body_id(&body, ctx, "step_wire_body_id_copy")?,
            kind: BodyKind::Wire,
            regions: one_topology_vec(region, ctx, "step_wire_body_regions")?,
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    );
    let mut built = match staged {
        Ok(built) => built,
        Err(error) => {
            push_topology_vec(losses,
                StepLossCode::DecodeWarning.note(format!("wire shell #{shell_id}: {error}")),
                ctx, "step_topology_losses")?;
            return Ok(None);
        }
    };
    insert_topology_set(&mut built.shell_sources, shell_id, ctx, "step_wire_shell_sources")?;
    Ok(Some(built))
}

fn mark_standalone_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(set_ids) = representation_items(representation) else {
        return Ok(false);
    };
    let mut decoded = false;
    for set_id in set_ids {
        let Some(set) = exchange.records().get(&set_id) else {
            continue;
        };
        let Some(set_type) = most_specific(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            continue;
        };
        let Some(items) = named_refs(set, set_type, 1) else {
            continue;
        };
        let has_decoded_member = items.into_iter().any(|item| {
            carrier_index.points.contains_key(&item)
                || carrier_index.curves.contains_key(&item)
                || carrier_index.surfaces.contains_key(&item)
        });
        if has_decoded_member {
            insert_topology_hash_set(typed, set_id, ctx, "step_topology_claims")?;
            decoded = true;
        }
    }
    if decoded {
        insert_topology_hash_set(typed, id, ctx, "step_topology_claims")?;
    }
    Ok(decoded)
}

fn build_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    losses: &mut Vec<LossNote>,
) -> Option<Built> {
    let Some(set_ids) = representation_items(representation) else {
        losses.push(StepLossCode::DecodeWarning.note(format!(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no item list"
        )));
        return None;
    };
    let mut typed = HashSet::from([id]);
    let body = BodyId::from(ids::data(kind!("body"), id));
    let region = RegionId::from(ids::data(kind!("region"), id));
    let shell_id = ShellId::from(ids::data(
        kind!("shell"),
        key_word!("geometric").dash(key_word!("set")).dash(id),
    ));
    let mut shell: Option<Shell> = None;
    let mut faces = Vec::new();
    for set_id in set_ids {
        let Some(set) = exchange.records().get(&set_id) else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped missing set #{set_id}"
            )));
            continue;
        };
        let Some(set_type) = most_specific(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped non-set member #{set_id}"
            )));
            continue;
        };
        let Some(items) = named_refs(set, set_type, 1) else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped set #{set_id} with no member list"
            )));
            continue;
        };
        typed.insert(set_id);
        for surface_step in items {
            let surface = SurfaceId::from(ids::data(kind!("surface"), surface_step));
            if carrier_index.surfaces.contains_key(&surface_step) {
                let face = Face {
                    id: FaceId::from(ids::data(
                        kind!("face"),
                        IdentityKey::from(surface_step)
                            .dash(key_word!("geometric"))
                            .dash(key_word!("set"))
                            .dash(id),
                    )),
                    shell: shell_id.clone(),
                    surface,
                    sense: Sense::Forward,
                    loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
                    name: None,
                    color: None,
                    tolerance: None,
                };
                match &mut shell {
                    Some(shell) => shell.add_face(face.id.clone()),
                    None => {
                        shell = Some(Shell::with_face(
                            shell_id.clone(),
                            region.clone(),
                            face.id.clone(),
                        ));
                    }
                }
                faces.push(face);
            }
        }
    }
    let Some(shell) = shell else {
        losses.push(StepLossCode::DecodeWarning.note(format!(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no indexed surface member; set dropped"
        )));
        return None;
    };
    staged_topology(
        typed,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        faces,
        Vec::new(),
        vec![shell],
        Region {
            id: region.clone(),
            body: body.clone(),
            shells: vec![shell_id],
        },
        Body {
            id: body,
            kind: BodyKind::Sheet,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    )
    .map_err(|error| {
        losses.push(StepLossCode::DecodeWarning.note(format!(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id}: {error}"
        )));
    })
    .ok()
}

#[derive(Clone)]
struct VertexDef {
    point: u64,
}
enum EdgeDef {
    Bare {
        start: u64,
        end: u64,
    },
    Curve {
        start: u64,
        end: u64,
        curve: u64,
        same: bool,
    },
    Subedge {
        start: u64,
        end: u64,
        parent: u64,
        basis: Rc<EdgeDef>,
    },
    Oriented {
        element: u64,
        basis: Rc<EdgeDef>,
        forward: bool,
    },
}

impl EdgeDef {
    fn vertices(&self) -> (u64, u64) {
        let mut current = self;
        loop {
            match current {
                Self::Bare { start, end }
                | Self::Curve { start, end, .. }
                | Self::Subedge { start, end, .. } => return (*start, *end),
                Self::Oriented { basis, .. } => current = basis,
            }
        }
    }

    fn curve_vertices(&self) -> (u64, u64) {
        let (start, end) = self.vertices();
        if self.same() {
            (start, end)
        } else {
            (end, start)
        }
    }

    fn curve(&self) -> Option<u64> {
        let mut current = self;
        loop {
            match current {
                Self::Bare { .. } => return None,
                Self::Curve { curve, .. } => return Some(*curve),
                Self::Subedge { basis, .. } | Self::Oriented { basis, .. } => current = basis,
            }
        }
    }

    fn same(&self) -> bool {
        let mut current = self;
        let mut orientation = true;
        loop {
            match current {
                Self::Bare { .. } => return orientation,
                Self::Curve { same, .. } => return *same == orientation,
                Self::Subedge { basis, .. } => current = basis,
                Self::Oriented { basis, forward, .. } => {
                    orientation = orientation == *forward;
                    current = basis;
                }
            }
        }
    }

    fn parent(&self) -> Option<u64> {
        match self {
            Self::Bare { .. } | Self::Curve { .. } => None,
            Self::Subedge { parent, .. } => Some(*parent),
            Self::Oriented { element, .. } => Some(*element),
        }
    }
}

#[derive(Clone, Copy)]
enum OrientedKind {
    Plain,
    Seam { pcurve: Option<u64> },
}

#[derive(Clone)]
struct OrientedDef {
    edge: u64,
    forward: bool,
    kind: OrientedKind,
}

fn vertex_defs(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, VertexDef>, CodecError> {
    let mut vertices = BTreeMap::new();
    for (id, record) in exchange.entities("VERTEX_POINT") {
        let Some(point) = named_reference(record, "VERTEX_POINT", 1, 0) else {
            continue;
        };
        insert_topology_map(&mut vertices, id, VertexDef { point }, ctx, "step_vertex_definitions")?;
    }
    Ok(vertices)
}
fn edge_defs(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Rc<EdgeDef>>, CodecError> {
    let mut edges = BTreeMap::new();
    let mut cache = BTreeMap::new();
    let mut active = BTreeSet::new();
    for (id, _) in exchange.entities_any(&[
        "EDGE_CURVE",
        "SEAM_EDGE",
        "ORIENTED_EDGE",
        "SUBEDGE",
        "EDGE",
    ]) {
        if let Some(edge) = edge_def_for(id, exchange, &mut active, &mut cache, ctx)? {
            insert_topology_map(&mut edges, id, edge, ctx, "step_edge_definitions")?;
        }
    }
    Ok(edges)
}

fn edge_def_for(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, Option<Rc<EdgeDef>>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Rc<EdgeDef>>, CodecError> {
    if let Some(edge) = cache.get(&id) {
        return Ok(edge.clone());
    }
    let _depth = ctx.enter_nested("step_edge_definition_recursion")?;
    if active.contains(&id) {
        return Ok(None);
    }
    insert_topology_set(active, id, ctx, "step_edge_definition_active")?;
    let result = if let Some(record) = exchange.records().get(&id) {
        match most_specific(record, &["EDGE_CURVE", "SEAM_EDGE", "ORIENTED_EDGE", "SUBEDGE", "EDGE"]) {
            Some("EDGE_CURVE") => edge_vertices(record)
                .zip(edge_geometry(record))
                .zip(edge_same_sense(record))
                .map(|(((start, end), curve), same)| EdgeDef::Curve { start, end, curve, same }),
            Some("EDGE") => edge_vertices(record)
                .map(|(start, end)| EdgeDef::Bare { start, end }),
            Some("SUBEDGE") => {
                if let Some(((start, end), parent)) = edge_vertices(record).zip(subedge_parent(record)) {
                    edge_def_for(parent, exchange, active, cache, ctx)?
                        .map(|basis| EdgeDef::Subedge { start, end, parent, basis })
                } else {
                    None
                }
            }
            Some("ORIENTED_EDGE" | "SEAM_EDGE") => {
                if let Some((element, forward)) = oriented_edge_reference(record)
                    .zip(oriented_edge_forward(record))
                {
                    edge_def_for(element, exchange, active, cache, ctx)?
                        .map(|basis| EdgeDef::Oriented { element, basis, forward })
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };
    active.remove(&id);
    let result = if let Some(definition) = result {
        ctx.charge_retained(
            u64_from_index(std::mem::size_of::<EdgeDef>() + 2 * std::mem::size_of::<usize>()),
            "step_edge_definition_node",
        )?;
        Some(Rc::new(definition))
    } else {
        None
    };
    insert_topology_map(&mut *cache, id, result.clone(), ctx, "step_edge_definition_cache")?;
    Ok(result)
}

fn edge_curve_id_reported(
    edge_id: u64,
    edge: &EdgeDef,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CurveId>, CodecError> {
    let Some(curve_step) = edge.curve() else {
        push_topology_vec(losses, StepLossCode::DecodeWarning.note(format!(
            "STEP edge #{edge_id} has no 3D curve carrier; edge committed without a curve"
        )), ctx, "step_topology_losses")?;
        return Ok(None);
    };
    let curve = exchange.records().get(&curve_step);
    let carrier = curve_carrier_record(curve_step, exchange);
    if carrier.is_none()
        && curve.is_some_and(|record| {
            record.partials.iter().any(|partial| {
                matches!(
                    partial.name.as_str(),
                    "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                )
            })
        })
    {
        push_topology_vec(losses, StepLossCode::DecodeWarning.note(format!(
            "STEP edge curve #{edge_id}: surface-curve #{curve_step} has no resolvable basis; edge committed without a curve"
        )), ctx, "step_topology_losses")?;
    }
    Ok(carrier.map(|curve| CurveId::from(ids::data(kind!("curve"), curve))))
}
fn oriented_defs(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, OrientedDef>, CodecError> {
    let mut oriented = BTreeMap::new();
    for (id, record) in exchange.entities_any(&["ORIENTED_EDGE", "SEAM_EDGE"]) {
        let Some(edge) = oriented_edge_reference(record) else {
            continue;
        };
        let Some(forward) = oriented_edge_forward(record) else {
            continue;
        };
        let kind = if most_specific(record, &["SEAM_EDGE"]).is_some() {
            OrientedKind::Seam {
                pcurve: record.partial("SEAM_EDGE").and_then(|partial| {
                    partial.parameters.iter().rev().find_map(ValueExt::reference)
                }),
            }
        } else {
            OrientedKind::Plain
        };
        insert_topology_map(
            &mut oriented, id, OrientedDef { edge, forward, kind }, ctx,
            "step_oriented_edge_definitions",
        )?;
    }
    Ok(oriented)
}

fn subedge_parent(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return entity_parameter(record, "SUBEDGE", 3).and_then(ValueExt::reference);
    }
    record
        .partial("SUBEDGE")
        .and_then(|partial| {
            partial
                .parameters
                .iter()
                .rev()
                .find_map(ValueExt::reference)
        })
        .or_else(|| {
            record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .filter_map(ValueExt::reference)
                .next_back()
        })
}

fn named_reference(
    record: &RawRecord,
    name: &str,
    simple_index: usize,
    complex_index: usize,
) -> Option<u64> {
    if record.partials.len() == 1 {
        return entity_parameter(record, name, simple_index)?.reference();
    }
    record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .and_then(|partial| {
            partial
                .parameters
                .iter()
                .filter_map(ValueExt::reference)
                .nth(complex_index)
        })
}

fn oriented_edge_reference(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return record.parameter(3).and_then(ValueExt::reference);
    }
    record
        .partial("ORIENTED_EDGE")
        .or_else(|| record.partial("SEAM_EDGE"))
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::reference))
}

fn oriented_edge_forward(record: &RawRecord) -> Option<bool> {
    if record.partials.len() == 1 {
        return record.parameter(4).and_then(ValueExt::logical);
    }
    record
        .partial("ORIENTED_EDGE")
        .or_else(|| record.partial("SEAM_EDGE"))
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::logical))
}

fn named_refs(record: &RawRecord, name: &str, simple_index: usize) -> Option<Vec<u64>> {
    if record.partials.len() == 1 {
        return refs(entity_parameter(record, name, simple_index)?);
    }
    record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .and_then(|partial| partial.parameters.iter().find_map(refs))
}

fn named_logical(
    record: &RawRecord,
    name: &str,
    simple_index: usize,
    _complex_index: usize,
) -> Option<bool> {
    if record.partials.len() == 1 {
        return entity_parameter(record, name, simple_index)?.logical();
    }
    record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::logical))
}

fn surface_curve_pcurves(record: &RawRecord) -> impl Iterator<Item = u64> + '_ {
    let values = if record.partials.len() == 1 {
        record.parameter(2).and_then(Value::list)
    } else {
        record
            .partial("SURFACE_CURVE")
            .or_else(|| record.partial("SEAM_CURVE"))
            .or_else(|| record.partial("INTERSECTION_CURVE"))
            .and_then(|partial| {
                partial.parameters.iter().find_map(|value| {
                    value
                        .list()
                        .filter(|values| values.iter().all(|item| item.reference().is_some()))
                })
            })
    };
    values
        .filter(|values| values.iter().all(|value| value.reference().is_some()))
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
}

fn edge_vertices(record: &RawRecord) -> Option<(u64, u64)> {
    if record.partials.len() == 1 {
        return Some((
            entity_parameter(record, record.simple_name()?, 1)?.reference()?,
            entity_parameter(record, record.simple_name()?, 2)?.reference()?,
        ));
    }
    record
        .partials
        .iter()
        .find(|partial| partial.name == "EDGE")
        .or_else(|| {
            record
                .partials
                .iter()
                .find(|partial| partial.name == "EDGE_CURVE")
        })
        .and_then(|partial| {
            let mut references = partial.parameters.iter().filter_map(ValueExt::reference);
            Some((references.next()?, references.next()?))
        })
}

fn edge_geometry(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return entity_parameter(record, record.simple_name()?, 3)?.reference();
    }
    record
        .partials
        .iter()
        .find(|partial| partial.name == "EDGE_CURVE")
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::reference))
}

fn edge_same_sense(record: &RawRecord) -> Option<bool> {
    if record.partials.len() == 1 {
        return entity_parameter(record, record.simple_name()?, 4)?.logical();
    }
    record
        .partials
        .iter()
        .find(|partial| partial.name == "EDGE_CURVE")
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::logical))
}

struct Built {
    typed: HashSet<u64>,
    draft: ModelDraft,
    body_id: BodyId,
    shell_sources: BTreeSet<u64>,
    /// Pcurve relations that the finite witness admitted while this body was
    /// staged. They are located source facts; the document report formats them.
    pcurve_admissions: Vec<PcurveAdmission>,
}

fn drop_committed_surfaces(draft: &mut ModelDraft, session: &mut CommitSession<'_>) {
    // Implicit surfaces can be staged by multiple roots. The session is the
    // authority on which ones a prior root committed; a pre-loop snapshot is
    // wrong because commits add surfaces while the loop is running.
    draft
        .model_mut()
        .surfaces
        .retain(|surface| !session.contains(surface.id.as_str()));
}

#[cfg(test)]
pub(crate) mod tests;

#[allow(clippy::too_many_arguments)]
fn staged_topology(
    typed: HashSet<u64>,
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    coedges: Vec<Coedge>,
    loops: Vec<Loop>,
    faces: Vec<Face>,
    surfaces: Vec<Surface>,
    shells: Vec<Shell>,
    region: Region,
    body: Body,
) -> Result<Built, cadmpeg_ir::draft::DraftError> {
    let mut draft = ModelDraft::new();
    for vertex in vertices {
        draft.insert(vertex)?;
    }
    for edge in edges {
        draft.insert(edge)?;
    }
    for coedge in coedges {
        draft.insert(coedge)?;
    }
    for loop_ in loops {
        draft.insert(loop_)?;
    }
    for face in faces {
        draft.insert(face)?;
    }
    let mut surface_ids = BTreeSet::new();
    for surface in surfaces {
        if surface_ids.insert(surface.id.as_str().to_owned()) {
            draft.insert(surface)?;
        }
    }
    for shell in shells {
        draft.insert(shell)?;
    }
    draft.insert(region)?;
    let body_id = body.id.clone();
    draft.insert(body)?;
    Ok(Built {
        typed,
        draft,
        body_id,
        shell_sources: BTreeSet::new(),
        pcurve_admissions: Vec::new(),
    })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RootKey {
    root_kind: &'static str,
    shell_keys: Vec<(u64, Option<bool>)>,
}

#[derive(Clone)]
struct RootBuilt {
    body_ids: Vec<BodyId>,
    body_by_shell: BTreeMap<u64, BTreeSet<BodyId>>,
}

fn root_shell_steps(
    root: &RawRecord,
    exchange: &Exchange,
    shell_definitions: &BTreeMap<u64, ShellDef>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u64>>, CodecError> {
    let mut ids = Vec::new();
    if root.partial("SHELL_BASED_SURFACE_MODEL").is_some() {
        let Some(values) = named_reference_values(root, "SHELL_BASED_SURFACE_MODEL", 1) else {
            return Ok(None);
        };
        for reference in values.iter().filter_map(ValueExt::reference) {
            push_topology_vec(&mut ids, reference, ctx, "step_root_shell_steps")?;
        }
        return Ok(Some(ids));
    }
    if root.partial("FACE_BASED_SURFACE_MODEL").is_some() {
        let Some(values) = named_reference_values(root, "FACE_BASED_SURFACE_MODEL", 1) else {
            return Ok(None);
        };
        for set_step in values.iter().filter_map(ValueExt::reference) {
            let Some(set) = exchange.records().get(&set_step) else {
                return Ok(None);
            };
            if connected_face_set_type(set).is_none() {
                return Ok(None);
            }
            push_topology_vec(&mut ids, set_step, ctx, "step_root_shell_steps")?;
        }
        return Ok(Some(ids));
    }
    if (root.partial("MANIFOLD_SOLID_BREP").is_some() || root.partial("FACETED_BREP").is_some())
        && root.partial("BREP_WITH_VOIDS").is_none()
    {
        let root_type = if root.partial("MANIFOLD_SOLID_BREP").is_some() {
            "MANIFOLD_SOLID_BREP"
        } else {
            "FACETED_BREP"
        };
        let Some(shell) = named_reference(root, root_type, 1, 0) else {
            return Ok(None);
        };
        push_topology_vec(&mut ids, shell, ctx, "step_root_shell_steps")?;
        return Ok(Some(ids));
    }
    if root.partial("BREP_WITH_VOIDS").is_some() {
        let Some(outer) = named_reference(root, "MANIFOLD_SOLID_BREP", 1, 0) else {
            return Ok(None);
        };
        let Some(values) = named_reference_values(root, "BREP_WITH_VOIDS", 2) else {
            return Ok(None);
        };
        push_topology_vec(&mut ids, outer, ctx, "step_root_shell_steps")?;
        for reference in values.iter().filter_map(ValueExt::reference) {
            push_topology_vec(&mut ids, reference, ctx, "step_root_shell_steps")?;
        }
        // `voids` is a STEP SET. CADIR keeps the outer shell at index zero
        // and canonicalizes the void suffix by resolved shell identity.
        ids[1..].sort_unstable_by_key(|reference| {
            shell_definitions
                .get(reference)
                .map_or((u64::MAX, true, *reference), |definition| {
                    (definition.base, definition.forward, *reference)
                })
        });
        return Ok(Some(ids));
    }
    Ok(None)
}

fn root_key(
    root: &RawRecord,
    exchange: &Exchange,
    shell_definitions: &BTreeMap<u64, ShellDef>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<RootKey>, CodecError> {
    let Some(root_kind) = most_specific(
        root,
        &[
            "BREP_WITH_VOIDS",
            "FACETED_BREP",
            "MANIFOLD_SOLID_BREP",
            "FACE_BASED_SURFACE_MODEL",
            "SHELL_BASED_SURFACE_MODEL",
        ],
    ) else {
        return Ok(None);
    };
    let mut shell_keys = Vec::new();
    let mut resolved = 0;
    let Some(shell_steps) = root_shell_steps(root, exchange, shell_definitions, ctx)? else {
        return Ok(None);
    };
    for shell in shell_steps {
        let key = if root.partial("FACE_BASED_SURFACE_MODEL").is_some() {
            Some((shell, Some(true)))
        } else {
            shell_definitions
                .get(&shell)
                .map(|definition| (definition.base, Some(definition.forward)))
                .or(Some((shell, None)))
        };
        if key.as_ref().is_some_and(|(_, forward)| forward.is_some()) {
            resolved += 1;
        }
        let Some(key) = key else {
            return Ok(None);
        };
        push_topology_vec(&mut shell_keys, key, ctx, "step_root_shell_keys")?;
    }
    if resolved == 0 {
        return Ok(None);
    }
    shell_keys.sort_unstable();
    Ok(Some(RootKey {
        root_kind,
        shell_keys,
    }))
}

#[allow(clippy::too_many_arguments)]
fn build(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
    ir: &CadIr,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, Rc<EdgeDef>>,
    odefs: &BTreeMap<u64, OrientedDef>,
    shell_definitions: &BTreeMap<u64, ShellDef>,
    decoded_pcurves: &BTreeSet<u64>,
    point_positions: &CarrierIndex,
    scope_root: bool,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<BuildOutcome, CodecError> {
    let Some(shell_steps) = root_shell_steps(root, exchange, shell_definitions, ctx)? else {
        return Ok(BuildOutcome::Partial {
            built: Vec::new(),
            failures: BuildFailures {
                count: NonZeroUsize::MIN,
                first: Some(BuildFailure {
                    record_id: id,
                    carrier_kind: CarrierKind::TopologyRootCarrier,
                }),
            },
        });
    };
    let solid = root.partial("MANIFOLD_SOLID_BREP").is_some()
        || root.partial("BREP_WITH_VOIDS").is_some()
        || root.partial("FACETED_BREP").is_some();
    if solid {
        let body = BodyId::from(ids::data(kind!("body"), id));
        let region = RegionId::from(ids::data(kind!("region"), id));
        let mut failure = None;
        let scope_shell_carriers = shell_steps.len() > 1 || scope_root;
        let built = build_one(
            id,
            root,
            exchange,
            ir,
            vdefs,
            edefs,
            odefs,
            shell_definitions,
            decoded_pcurves,
            point_positions,
            &shell_steps,
            body,
            &region,
            scope_shell_carriers,
            scope_shell_carriers,
            scope_root,
            losses,
            &mut failure,
            ctx,
        );
        return Ok(match built {
            Ok(built) => BuildOutcome::Built(vec![built]),
            Err(BuildError::Absent) => BuildOutcome::Partial {
                built: Vec::new(),
                failures: BuildFailures {
                    count: NonZeroUsize::MIN,
                    first: failure,
                },
            },
            Err(BuildError::Resource(error)) => return Err(error),
        });
    }

    let scoped = shell_steps.len() > 1;
    let mut outcome = BuildOutcome::Built(Vec::new());
    for shell_reference in shell_steps {
        let mut failure = None;
        let shell_step = if root.partial("FACE_BASED_SURFACE_MODEL").is_some() {
            shell_reference
        } else {
            match shell_definitions.get(&shell_reference) {
                Some(definition) => definition.base,
                None => {
                    outcome.fail(Some(BuildFailure {
                        record_id: shell_reference,
                        carrier_kind: CarrierKind::ShellCarrier,
                    }));
                    continue;
                }
            }
        };
        let suffix = if scoped {
            IdentityKeyTail::empty()
                .dash(key_word!("shell"))
                .dash(shell_step)
        } else {
            IdentityKeyTail::empty()
        };
        let body = BodyId::from(ids::data(
            kind!("body"),
            IdentityKey::from(id).with_tail(&suffix),
        ));
        let region = RegionId::from(ids::data(
            kind!("region"),
            IdentityKey::from(id).with_tail(&suffix),
        ));
        match build_one(
            id,
            root,
            exchange,
            ir,
            vdefs,
            edefs,
            odefs,
            shell_definitions,
            decoded_pcurves,
            point_positions,
            &[shell_reference],
            body,
            &region,
            scoped || scope_root,
            scoped || scope_root,
            scope_root,
            losses,
            &mut failure,
            ctx,
        ) {
            Ok(value) => outcome.push(value, ctx)?,
            Err(BuildError::Absent) => outcome.fail(failure),
            Err(BuildError::Resource(error)) => return Err(error),
        }
    }
    Ok(outcome)
}

enum BuildError {
    Absent,
    Resource(CodecError),
}

impl From<CodecError> for BuildError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

#[allow(clippy::too_many_arguments)]
fn build_one(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
    ir: &CadIr,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, Rc<EdgeDef>>,
    odefs: &BTreeMap<u64, OrientedDef>,
    shell_definitions: &BTreeMap<u64, ShellDef>,
    decoded_pcurves: &BTreeSet<u64>,
    point_positions: &CarrierIndex,
    shell_steps: &[u64],
    bid: BodyId,
    rid: &RegionId,
    scope_faces: bool,
    scope_edges: bool,
    scope_root: bool,
    losses: &mut Vec<LossNote>,
    failure: &mut Option<BuildFailure>,
    ctx: &DecodeContext<'_>,
) -> Result<Built, BuildError> {
    let solid = root.partial("MANIFOLD_SOLID_BREP").is_some()
        || root.partial("BREP_WITH_VOIDS").is_some()
        || root.partial("FACETED_BREP").is_some();
    let mut typed = HashSet::from([id]);
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    let mut coedges = Vec::new();
    let mut loops = Vec::new();
    let mut faces = Vec::new();
    let mut surfaces = Vec::new();
    let mut shells = Vec::new();
    let mut region = Region {
        id: rid.clone(),
        body: bid.clone(),
        shells: Vec::new(),
    };
    let body = Body {
        id: bid,
        kind: if solid {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        },
        regions: vec![rid.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let mut used_v = BTreeSet::<(u64, u64)>::new();
    let mut used_e = BTreeSet::<(u64, u64)>::new();
    let mut used_shells = BTreeSet::new();
    let mut used_faces = BTreeSet::new();
    let mut radial = BTreeMap::<EdgeId, Vec<usize>>::new();
    let mut poly_edges = BTreeMap::<(u64, EdgeId), (u64, u64)>::new();
    let mut poly_points = BTreeSet::<(u64, u64)>::new();
    let mut implicit_surface_ids = BTreeSet::new();
    let mut admissions = Vec::new();
    for &shell_reference in shell_steps {
        let (shell_step, shell_forward) = if root.partial("FACE_BASED_SURFACE_MODEL").is_some() {
            typed.insert(shell_reference);
            (shell_reference, true)
        } else {
            require_carrier(
                shell_def_for(shell_reference, shell_definitions, &mut typed, ctx)?,
                failure,
                shell_reference,
                CarrierKind::ShellCarrier,
            )
            .ok_or(BuildError::Absent)?
        };
        if !used_shells.insert(shell_step) {
            continue;
        }
        let sr = require_carrier(
            exchange.records().get(&shell_step),
            failure,
            shell_step,
            CarrierKind::ShellRecord,
        )
        .ok_or(BuildError::Absent)?;
        let (shell_type, face_steps) = if root.partial("FACE_BASED_SURFACE_MODEL").is_some() {
            let set_type = require_carrier(
                connected_face_set_type(sr),
                failure,
                shell_step,
                CarrierKind::ConnectedFaceSet,
            )
            .ok_or(BuildError::Absent)?;
            if set_type == "CONNECTED_FACE_SUB_SET"
                && !validate_subset_parent(shell_step, sr, set_type, exchange, losses, ctx)?
            {
                typed.remove(&shell_step);
            }
            let members = require_carrier(
                connected_set_members(sr, set_type),
                failure,
                shell_step,
                CarrierKind::ConnectedFaceSetMemberList,
            )
            .ok_or(BuildError::Absent)?;
            (set_type, members)
        } else {
            let shell_type = require_carrier(
                most_specific(sr, &["OPEN_SHELL", "CLOSED_SHELL"]),
                failure,
                shell_step,
                CarrierKind::ShellType,
            )
            .ok_or(BuildError::Absent)?;
            let members = require_carrier(
                named_reference_values(sr, shell_type, 1),
                failure,
                shell_step,
                CarrierKind::ShellFaceList,
            )
            .ok_or(BuildError::Absent)?;
            (shell_type, members)
        };
        if face_steps.is_empty() {
            note_failure(failure, shell_step, CarrierKind::ShellFaceList);
            return Err(BuildError::Absent);
        }
        let sid = shell_identity(id, shell_step, scope_root);
        let mut face_ids = vec![];
        for face_step in face_steps.iter().filter_map(Value::reference) {
            if !used_faces.insert((shell_step, face_step)) {
                continue;
            }
            let fr = require_carrier(
                exchange.records().get(&face_step),
                failure,
                face_step,
                CarrierKind::FaceRecord,
            )
            .ok_or(BuildError::Absent)?;
            if !is_face_record(fr) {
                note_failure(failure, face_step, CarrierKind::FaceCarrier);
                return Err(BuildError::Absent);
            }
            let face_info = require_carrier(
                face_attributes(face_step, fr, exchange, &mut BTreeSet::new()),
                failure,
                face_step,
                CarrierKind::FaceAttributes,
            )
            .ok_or(BuildError::Absent)?;
            let outer_bound_count = face_info
                .bounds
                .iter()
                .filter(|bound_step| {
                    exchange
                        .records()
                        .get(bound_step)
                        .is_some_and(|bound| bound.partial("FACE_OUTER_BOUND").is_some())
                })
                .count();
            if outer_bound_count > 1 {
                let note = StepLossCode::FaceMultipleOuterBounds.note(format!(
                    "face #{face_step} violates the STEP face-bound rule with {outer_bound_count} FACE_OUTER_BOUND loops; omitting the containing topology shell without assigning an outer role or deriving an implicit face carrier and retaining the source face, bounds, loops, and enclosing records as opaque"
                ));
                losses.push(
                    note.with_provenance(
                        cadmpeg_ir::SourceProvenance::root(
                            crate::dialect::FORMAT,
                            fr.span.start as u64,
                        )
                        .with_tag("face"),
                    ),
                );
                note_failure(failure, face_step, CarrierKind::FaceWithMultipleOuterBounds);
                return Err(BuildError::Absent);
            }
            typed.extend(face_info.typed);
            let face_suffix = if scope_faces {
                if scope_root {
                    IdentityKeyTail::empty()
                        .dash(key_word!("root"))
                        .dash(id)
                        .dash(key_word!("shell"))
                        .dash(shell_step)
                } else {
                    IdentityKeyTail::empty()
                        .dash(key_word!("shell"))
                        .dash(shell_step)
                }
            } else {
                IdentityKeyTail::empty()
            };
            let surface_id = if let Some(surface_step) = face_info.surface {
                SurfaceId::from(ids::data(kind!("surface"), surface_step))
            } else {
                let surface_id = SurfaceId::from(ids::data(
                    kind!("surface"),
                    key_word!("implicit")
                        .dash(key_word!("face"))
                        .dash(face_step)
                        .with_tail(&face_suffix),
                ));
                if implicit_surface_ids.insert(surface_id.clone()) {
                    surfaces.push(Surface {
                        id: surface_id.clone(),
                        geometry: require_carrier(
                            implicit_face_plane(
                                &face_info.bounds,
                                exchange,
                                vdefs,
                                point_positions,
                            ),
                            failure,
                            face_step,
                            CarrierKind::ImplicitFacePlane,
                        )
                        .ok_or(BuildError::Absent)?,
                        source_object: None,
                    });
                }
                surface_id
            };
            let surface_step = face_info.surface;
            let face_same_sense = face_info.same_sense;
            let fid = FaceId::from(ids::data(
                kind!("face"),
                IdentityKey::from(face_step).with_tail(&face_suffix),
            ));
            let name = face_info.name.as_ref().map(|value| {
                super::decode_text_charged(
                    exchange,
                    value,
                    losses,
                    face_step,
                    "face name",
                    StepLossCode::MetadataStringInvalid,
                    Some(ctx),
                )
            }).transpose()?.flatten();
            let mut loop_ids = vec![];
            for bound_step in face_info.bounds {
                let br = require_carrier(
                    exchange.records().get(&bound_step),
                    failure,
                    bound_step,
                    CarrierKind::FaceBound,
                )
                .ok_or(BuildError::Absent)?;
                if br.partial("FACE_BOUND").is_none() && br.partial("FACE_OUTER_BOUND").is_none() {
                    note_failure(failure, bound_step, CarrierKind::FaceBoundCarrier);
                    return Err(BuildError::Absent);
                }
                let is_outer_bound = br.partial("FACE_OUTER_BOUND").is_some();
                let Some(bound_type) = face_bound_attribute_type(br) else {
                    note_failure(failure, bound_step, CarrierKind::FaceBoundAttributes);
                    return Err(BuildError::Absent);
                };
                let loop_step = require_carrier(
                    named_reference(br, bound_type, 1, 0),
                    failure,
                    bound_step,
                    CarrierKind::BoundLoopReference,
                )
                .ok_or(BuildError::Absent)?;
                let lr = require_carrier(
                    exchange.records().get(&loop_step),
                    failure,
                    loop_step,
                    CarrierKind::LoopRecord,
                )
                .ok_or(BuildError::Absent)?;
                let lid = LoopId::from(ids::data(
                    kind!("loop"),
                    IdentityKey::from(loop_step)
                        .dash(key_word!("face"))
                        .dash(face_step)
                        .with_tail(&face_suffix),
                ));
                if lr.partial("VERTEX_LOOP").is_some() {
                    let vertex_step = require_carrier(
                        named_reference(lr, "VERTEX_LOOP", 1, 0),
                        failure,
                        loop_step,
                        CarrierKind::VertexLoopReference,
                    )
                    .ok_or(BuildError::Absent)?;
                    if !vdefs
                        .get(&vertex_step)
                        .is_some_and(|vertex| point_positions.contains_key(vertex.point))
                    {
                        note_failure(failure, vertex_step, CarrierKind::VertexPoint);
                        return Err(BuildError::Absent);
                    }
                    loops.push(Loop {
                        id: lid.clone(),
                        face: fid.clone(),
                        boundary: cadmpeg_ir::topology::LoopBoundary::Vertex {
                            vertex: scoped_vertex_id(
                                vertex_step,
                                id,
                                shell_step,
                                scope_edges,
                                scope_root,
                            ),
                            pcurves: Vec::new(),
                        },
                    });
                    loop_ids.push((is_outer_bound, lid));
                    used_v.insert((shell_step, vertex_step));
                    typed.extend([bound_step, loop_step]);
                    continue;
                }
                if lr.partial("POLY_LOOP").is_some() {
                    let bound_forward = require_carrier(
                        named_logical(br, bound_type, 2, 0),
                        failure,
                        bound_step,
                        CarrierKind::BoundOrientation,
                    )
                    .ok_or(BuildError::Absent)?;
                    let bound_forward = if face_info.reverse_bound_orientation {
                        !bound_forward
                    } else {
                        bound_forward
                    };
                    let mut points = require_carrier(
                        named_refs(lr, "POLY_LOOP", 1),
                        failure,
                        loop_step,
                        CarrierKind::PolyLoopPointList,
                    )
                    .ok_or(BuildError::Absent)?;
                    if points.first() == points.last() {
                        points.pop();
                    }
                    points.dedup();
                    if points.len() < 3
                        || points.iter().collect::<BTreeSet<_>>().len() != points.len()
                        || points
                            .iter()
                            .any(|point| !point_positions.contains_key(*point))
                    {
                        note_failure(failure, loop_step, CarrierKind::PolyLoopPointCarrier);
                        return Err(BuildError::Absent);
                    }
                    if !bound_forward {
                        points.reverse();
                    }
                    let mut coedge_ids = Vec::new();
                    for (index, &start_point) in points.iter().enumerate() {
                        let end_point = points[(index + 1) % points.len()];
                        let (canonical_start, canonical_end) =
                            (start_point.min(end_point), start_point.max(end_point));
                        let edge_id = poly_edge_id(
                            canonical_start,
                            canonical_end,
                            id,
                            shell_step,
                            scope_edges,
                            scope_root,
                        );
                        poly_edges
                            .entry((shell_step, edge_id.clone()))
                            .or_insert((canonical_start, canonical_end));
                        poly_points.extend([(shell_step, start_point), (shell_step, end_point)]);
                        let cid = CoedgeId::from(ids::data(
                            kind!("coedge"),
                            key_word!("poly")
                                .dash(loop_step)
                                .dash(index)
                                .dash(key_word!("face"))
                                .dash(face_step)
                                .with_tail(&face_suffix),
                        ));
                        coedge_ids.push(cid.clone());
                        coedges.push(Coedge {
                            id: cid.clone(),
                            owner_loop: lid.clone(),
                            edge: edge_id.clone(),
                            radial_next: cid,
                            sense: if (canonical_start, canonical_end) == (start_point, end_point) {
                                Sense::Forward
                            } else {
                                Sense::Reversed
                            },
                            pcurves: Vec::new(),
                            use_curve: None,
                        });
                        radial.entry(edge_id).or_default().push(coedges.len() - 1);
                        typed.insert(loop_step);
                    }
                    let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(coedge_ids, Vec::new())
                    else {
                        note_failure(failure, loop_step, CarrierKind::PolyLoopPointCarrier);
                        return Err(BuildError::Absent);
                    };
                    loops.push(Loop {
                        id: lid.clone(),
                        face: fid.clone(),
                        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                    });
                    loop_ids.push((is_outer_bound, lid));
                    typed.insert(bound_step);
                    continue;
                }
                if lr.partial("EDGE_LOOP").is_none() {
                    note_failure(failure, loop_step, CarrierKind::EdgeLoopCarrier);
                    return Err(BuildError::Absent);
                }
                let bound_forward = require_carrier(
                    named_logical(br, bound_type, 2, 0),
                    failure,
                    bound_step,
                    CarrierKind::BoundOrientation,
                )
                .ok_or(BuildError::Absent)?;
                let bound_forward = if face_info.reverse_bound_orientation {
                    !bound_forward
                } else {
                    bound_forward
                };
                let mut uses = require_carrier(
                    named_refs(lr, "EDGE_LOOP", 1),
                    failure,
                    loop_step,
                    CarrierKind::EdgeLoopMemberList,
                )
                .ok_or(BuildError::Absent)?;
                if !bound_forward {
                    uses.reverse();
                }
                if uses.is_empty() {
                    note_failure(failure, loop_step, CarrierKind::EdgeLoopMember);
                    return Err(BuildError::Absent);
                }
                let mut coedge_ids = vec![];
                for use_step in uses {
                    let o = require_carrier(
                        odefs.get(&use_step),
                        failure,
                        use_step,
                        CarrierKind::OrientedEdgeDefinition,
                    )
                    .ok_or(BuildError::Absent)?;
                    let edge = require_carrier(
                        edefs.get(&o.edge),
                        failure,
                        o.edge,
                        CarrierKind::EdgeDefinition,
                    )
                    .ok_or(BuildError::Absent)?;
                    let cid = CoedgeId::from(ids::data(
                        kind!("coedge"),
                        IdentityKey::from(use_step)
                            .dash(key_word!("face"))
                            .dash(face_step)
                            .with_tail(&face_suffix),
                    ));
                    let pcurves: Vec<(PcurveId, Option<[f64; 2]>)> = if let OrientedKind::Seam {
                        pcurve,
                    } = o.kind
                    {
                        let explicit_pcurve = surface_step.and_then(|surface_step| {
                            let pcurve_step = pcurve?;
                            let pcurve = exchange.records().get(&pcurve_step)?;
                            let pcurve_id = PcurveId::from(ids::data(kind!("pcurve"), pcurve_step));
                            let edge_curve = edge.curve()?;
                            let associated = exchange.records().get(&edge_curve).is_some_and(
                                |curve_record| {
                                    surface_curve_pcurves(curve_record)
                                        .any(|step| step == pcurve_step)
                                },
                            );
                            (pcurve.partial("PCURVE").is_some()
                                && entity_parameter(pcurve, "PCURVE", 1)?.reference()?
                                    == surface_step
                                && decoded_pcurves.contains(&pcurve_step)
                                && associated)
                            .then_some(pcurve_id)
                        });
                        if let Some(pcurve) = explicit_pcurve {
                            vec![(pcurve, None)]
                        } else {
                            losses.push(StepLossCode::SeamEdgePcurveUnresolved.note(format!(
                                    "SEAM_EDGE #{use_step} has no decoded pcurve reference that belongs to its edge curve and face surface; the coedge has no pcurve"
                                )));
                            Vec::new()
                        }
                    } else if let (Some(surface), Some(curve)) = (surface_step, edge.curve()) {
                        let associated = associated_pcurves(
                            curve, surface, exchange, decoded_pcurves, ctx,
                        )?;
                        if associated.is_empty() {
                            Vec::new()
                        } else {
                            match select_associated_pcurve(
                                ir,
                                exchange,
                                surface,
                                edge,
                                vdefs,
                                point_positions,
                                &associated,
                            ) {
                                Ok(selected) => {
                                    admissions.push(PcurveAdmission {
                                        curve,
                                        surface,
                                        coedge_use: use_step,
                                    });
                                    vec![(selected.id, selected.parameter_range)]
                                }
                                Err(PcurveSelectionFailure::ResourceLimit(limit)) => {
                                    return Err(CodecError::ResourceLimit(limit).into());
                                }
                                Err(failure) => {
                                    let note = match failure {
                                        PcurveSelectionFailure::NotUnique { count } =>
                                            StepLossCode::PcurveAssociationAmbiguous.note(format!(
                                                "curve #{curve} associates {count} pcurves with surface #{surface}; Part 42 provides no non-seam selector, so the coedge has no pcurve"
                                            )),
                                        PcurveSelectionFailure::Carrier =>
                                            StepLossCode::PcurveCandidatesCarrierUnresolved.note(format!(
                                                "coedge use #{use_step} has one pcurve candidate but its decoded surface, pcurve, or vertex point carrier is unresolved; the coedge has no pcurve"
                                            )),
                                        PcurveSelectionFailure::Endpoint =>
                                            StepLossCode::PcurveEndpointsDiscontinuous.note(format!(
                                                "curve #{curve} has one optional pcurve on surface #{surface} whose mapped endpoints are not continuous with the edge vertices; the pcurve is omitted"
                                            )),
                                        PcurveSelectionFailure::Locus =>
                                            StepLossCode::PcurveLocusDiscontinuous.note(format!(
                                                "curve #{curve} has one endpoint-continuous pcurve on surface #{surface} whose bounded model-space locus or direction witness fails; the pcurve is omitted"
                                            )),
                                        PcurveSelectionFailure::ResourceLimit(limit) => {
                                            return Err(CodecError::ResourceLimit(limit).into());
                                        }
                                    };
                                    losses.push(note);
                                    Vec::new()
                                }
                            }
                        }
                    } else {
                        losses.push(StepLossCode::EdgeNoSurfaceOrCurveForPcurve.note(format!(
                            "edge #{} has no decoded surface or curve carrier, so its coedge has no pcurve",
                            o.edge
                        )));
                        Vec::new()
                    };
                    coedge_ids.push(cid.clone());
                    coedges.push(Coedge {
                        id: cid.clone(),
                        owner_loop: lid.clone(),
                        edge: scoped_edge_id(o.edge, id, shell_step, scope_edges, scope_root),
                        radial_next: cid,
                        sense: if (o.forward == edge.same()) == bound_forward {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves: pcurves
                            .into_iter()
                            .map(|(pcurve, parameter_range)| {
                                Ok(PcurveUse {
                                    pcurve,
                                    isoparametric: None,
                                    parameter_range: parameter_range
                                        .map(cadmpeg_ir::geometry::DirectedParameterRange::new)
                                        .transpose()?,
                                })
                            })
                            .collect::<Result<Vec<_>, cadmpeg_ir::geometry::ParameterRangeError>>()
                            .map_err(|error| {
                                losses.push(
                                    StepLossCode::DecodeWarning
                                        .note(format!("coedge pcurve parameter_range: {error}")),
                                );
                            })
                            .ok()
                            .ok_or(BuildError::Absent)?,
                        use_curve: None,
                    });
                    radial
                        .entry(scoped_edge_id(
                            o.edge,
                            id,
                            shell_step,
                            scope_edges,
                            scope_root,
                        ))
                        .or_default()
                        .push(coedges.len() - 1);
                    used_e.insert((shell_step, o.edge));
                    used_v.extend([
                        (shell_step, edge.vertices().0),
                        (shell_step, edge.vertices().1),
                    ]);
                    typed.extend([use_step, o.edge]);
                    if let Some(parent) = edge.parent() {
                        typed.insert(parent);
                    }
                }
                let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(coedge_ids, Vec::new()) else {
                    note_failure(failure, loop_step, CarrierKind::EdgeLoopCarrier);
                    return Err(BuildError::Absent);
                };
                loops.push(Loop {
                    id: lid.clone(),
                    face: fid.clone(),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                });
                loop_ids.push((is_outer_bound, lid));
                typed.extend([bound_step, loop_step]);
            }
            // A face with more than one FACE_OUTER_BOUND is refused above, so
            // at most one bound carries the outer role here. A face with no
            // outer bound states no classification.
            let (outer_bounds, inner_bounds): (Vec<_>, Vec<_>) =
                loop_ids.into_iter().partition(|(is_outer, _)| *is_outer);
            let inner: Vec<_> = inner_bounds.into_iter().map(|(_, id)| id).collect();
            let face_loops = match outer_bounds.into_iter().next() {
                Some((_, outer)) => cadmpeg_ir::topology::FaceLoops::classified(outer, inner),
                None => cadmpeg_ir::topology::FaceLoops::unspecified(inner),
            };
            let face_forward = face_same_sense == shell_forward;
            faces.push(Face {
                id: fid.clone(),
                shell: sid.clone(),
                surface: surface_id,
                sense: if face_forward {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                loops: face_loops,
                name,
                color: None,
                tolerance: None,
            });
            face_ids.push(fid);
            typed.insert(face_step);
        }
        let mut component_edge_vertices = BTreeMap::new();
        for (used_shell, edge_id) in &used_e {
            if *used_shell != shell_step {
                continue;
            }
            let Some(edge) = edefs.get(edge_id) else {
                continue;
            };
            let (start, end) = edge.curve_vertices();
            component_edge_vertices.insert(
                scoped_edge_id(*edge_id, id, shell_step, scope_edges, scope_root).into_string(),
                (
                    scoped_vertex_id(start, id, shell_step, scope_edges, scope_root).into_string(),
                    scoped_vertex_id(end, id, shell_step, scope_edges, scope_root).into_string(),
                ),
            );
        }
        for ((used_shell, edge_id), (start, end)) in &poly_edges {
            if *used_shell != shell_step {
                continue;
            }
            component_edge_vertices.insert(
                edge_id.as_str().to_owned(),
                (
                    scoped_poly_vertex_id(*start, id, shell_step, scope_edges, scope_root)
                        .into_string(),
                    scoped_poly_vertex_id(*end, id, shell_step, scope_edges, scope_root)
                        .into_string(),
                ),
            );
        }
        let components =
            connected_face_components(&face_ids, &loops, &coedges, &component_edge_vertices, ctx)?;
        if components.len() > 1 {
            let note = StepLossCode::ShellDisconnectedFaces.note(format!(
                    "source {shell_type} #{shell_step} contains {} disconnected face components across {} faces",
                    components.len(),
                    face_ids.len(),
                ));
            losses.push(
                note.with_provenance(
                    cadmpeg_ir::SourceProvenance::root(
                        crate::dialect::FORMAT,
                        sr.span.start as u64,
                    )
                    .with_tag(shell_type.to_ascii_lowercase()),
                ),
            );
        }
        for (component_index, component) in components.into_iter().enumerate() {
            if root.partial("BREP_WITH_VOIDS").is_some()
                && shell_steps.first().copied() == Some(shell_reference)
                && component_index > 0
            {
                note_failure(failure, shell_step, CarrierKind::ConnectedOuterShell);
                return Err(BuildError::Absent);
            }
            let component_shell = if component_index == 0 {
                sid.clone()
            } else {
                ShellId::from(ids::data(
                    kind!("shell"),
                    if scope_root {
                        IdentityKey::from(shell_step)
                            .dash(key_word!("root"))
                            .dash(id)
                            .dash(key_word!("component"))
                            .dash(component_index)
                    } else {
                        IdentityKey::from(shell_step)
                            .dash(key_word!("component"))
                            .dash(component_index)
                    },
                ))
            };
            let component_faces = component
                .into_iter()
                .map(|face_index| {
                    let face_id = face_ids[face_index].clone();
                    faces[face_index].shell = component_shell.clone();
                    face_id
                })
                .collect();
            shells.push(
                match Shell::new(
                    component_shell.clone(),
                    rid.clone(),
                    component_faces,
                    vec![],
                    vec![],
                ) {
                    Ok(shell) => shell,
                    Err(error) => {
                        losses.push(
                            StepLossCode::DecodeWarning
                                .note(format!("{shell_type} #{shell_step}: {error}")),
                        );
                        return Err(BuildError::Absent);
                    }
                },
            );
            region.shells.push(component_shell);
        }
        typed.insert(shell_step);
    }
    for (shell_step, edge_id) in used_e {
        let e = require_carrier(
            edefs.get(&edge_id),
            failure,
            edge_id,
            CarrierKind::EdgeDefinition,
        )
        .ok_or(BuildError::Absent)?;
        let (start, end) = e.curve_vertices();
        edges.push(Edge {
            id: scoped_edge_id(edge_id, id, shell_step, scope_edges, scope_root),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(edge_curve_id_reported(
                edge_id, e, exchange, losses, ctx,
            )?),
            start: scoped_vertex_id(start, id, shell_step, scope_edges, scope_root),
            end: scoped_vertex_id(end, id, shell_step, scope_edges, scope_root),
            tolerance: None,
        });
    }
    for ((shell_step, edge_identity), (start, end)) in poly_edges {
        edges.push(Edge {
            id: edge_identity,
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
            start: scoped_poly_vertex_id(start, id, shell_step, scope_edges, scope_root),
            end: scoped_poly_vertex_id(end, id, shell_step, scope_edges, scope_root),
            tolerance: None,
        });
    }
    for (shell_step, vertex_id) in used_v {
        let v = require_carrier(
            vdefs.get(&vertex_id),
            failure,
            vertex_id,
            CarrierKind::VertexDefinition,
        )
        .ok_or(BuildError::Absent)?;
        require_carrier(
            point_positions.get(v.point),
            failure,
            v.point,
            CarrierKind::VertexPoint,
        )
        .ok_or(BuildError::Absent)?;
        vertices.push(Vertex {
            id: scoped_vertex_id(vertex_id, id, shell_step, scope_edges, scope_root),
            point: PointId::from(ids::data(kind!("point"), v.point)),
            tolerance: None,
        });
        typed.insert(vertex_id);
    }
    for (shell_step, point_id) in poly_points {
        require_carrier(
            point_positions.get(point_id),
            failure,
            point_id,
            CarrierKind::PolyVertexPoint,
        )
        .ok_or(BuildError::Absent)?;
        vertices.push(Vertex {
            id: scoped_poly_vertex_id(point_id, id, shell_step, scope_edges, scope_root),
            point: PointId::from(ids::data(kind!("point"), point_id)),
            tolerance: None,
        });
        typed.insert(point_id);
    }
    for indices in radial.values() {
        for (position, &index) in indices.iter().enumerate() {
            coedges[index].radial_next =
                coedges[indices[(position + 1) % indices.len()]].id.clone();
        }
    }
    let edge_by_id = edges
        .iter()
        .map(|edge| (edge.id.clone(), edge))
        .collect::<BTreeMap<_, _>>();
    let coedge_by_id = coedges
        .iter()
        .map(|coedge| (coedge.id.clone(), coedge))
        .collect::<BTreeMap<_, _>>();
    for loop_ in &loops {
        if loop_.coedges().is_empty() {
            continue;
        }
        let loop_source = source_numeric_id(loop_.id.as_str(), "loop").unwrap_or(0);
        for (index, current_id) in loop_.coedges().iter().enumerate() {
            let next_id = &loop_.coedges()[(index + 1) % loop_.coedges().len()];
            let current = require_carrier(
                coedge_by_id.get(current_id),
                failure,
                loop_source,
                CarrierKind::Coedge,
            )
            .ok_or(BuildError::Absent)?;
            let next = require_carrier(
                coedge_by_id.get(next_id),
                failure,
                loop_source,
                CarrierKind::Coedge,
            )
            .ok_or(BuildError::Absent)?;
            let current_edge = require_carrier(
                edge_by_id.get(&current.edge),
                failure,
                loop_source,
                CarrierKind::CoedgeEdge,
            )
            .ok_or(BuildError::Absent)?;
            let next_edge = require_carrier(
                edge_by_id.get(&next.edge),
                failure,
                loop_source,
                CarrierKind::CoedgeEdge,
            )
            .ok_or(BuildError::Absent)?;
            let current_end = match current.sense {
                Sense::Forward => &current_edge.end,
                Sense::Reversed => &current_edge.start,
            };
            let next_start = match next.sense {
                Sense::Forward => &next_edge.start,
                Sense::Reversed => &next_edge.end,
            };
            if current_end != next_start {
                note_failure(failure, loop_source, CarrierKind::EdgeLoopContinuity);
                return Err(BuildError::Absent);
            }
        }
    }
    let mut built = require_carrier(
        staged_topology(
            typed, vertices, edges, coedges, loops, faces, surfaces, shells, region, body,
        )
        .ok(),
        failure,
        id,
        CarrierKind::TopologyDraft,
    )
    .ok_or(BuildError::Absent)?;
    built.pcurve_admissions = admissions;
    for &shell_reference in shell_steps {
        let shell_step = if root.partial("FACE_BASED_SURFACE_MODEL").is_some() {
            shell_reference
        } else {
            require_carrier(
                shell_definitions
                    .get(&shell_reference)
                    .map(|definition| (definition.base, definition.forward)),
                failure,
                shell_reference,
                CarrierKind::ShellCarrier,
            )
            .ok_or(BuildError::Absent)?
            .0
        };
        built.shell_sources.insert(shell_step);
    }
    Ok(built)
}

/// Partition a source shell into connected IR shells before committing it.
///
/// STEP shell records can contain several disconnected face components. The
/// IR shell invariant is stricter: every face must be reachable through a
/// shared edge or vertex. Keep the source body and region, but split only the
/// shell boundary so every decoded face remains available without weakening
/// validation.
fn connected_face_components(
    face_ids: &[FaceId],
    loops: &[Loop],
    coedges: &[Coedge],
    edge_vertices: &BTreeMap<String, (String, String)>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<Vec<usize>>, CodecError> {
    let mut neighbors = ctx.alloc_filled(
        face_ids.len(),
        BTreeSet::new(),
        "STEP connected-face neighbors",
    )?;
    let mut face_indices = BTreeMap::new();
    for (index, face) in face_ids.iter().enumerate() {
        ctx.charge_collection_items(1, "STEP connected-face indices")?;
        face_indices.insert(face.as_str(), index);
    }
    let mut coedge_edges = BTreeMap::new();
    for coedge in coedges {
        ctx.charge_collection_items(1, "STEP connected-face coedge edges")?;
        coedge_edges.insert(coedge.id.as_str(), coedge.edge.as_str());
    }
    let mut faces_by_edge = BTreeMap::<&str, BTreeSet<usize>>::new();
    let mut faces_by_vertex = BTreeMap::<&str, BTreeSet<usize>>::new();
    for loop_ in loops {
        let Some(&face_index) = face_indices.get(loop_.face.as_str()) else {
            continue;
        };
        for coedge_id in loop_.coedges() {
            let Some(edge_id) = coedge_edges.get(coedge_id.as_str()) else {
                continue;
            };
            insert_connected_face_group(&mut faces_by_edge, edge_id, face_index, ctx)?;
            if let Some((start, end)) = edge_vertices.get(*edge_id) {
                insert_connected_face_group(&mut faces_by_vertex, start, face_index, ctx)?;
                insert_connected_face_group(&mut faces_by_vertex, end, face_index, ctx)?;
            }
        }
        for vertex in loop_.vertices() {
            insert_connected_face_group(&mut faces_by_vertex, vertex.as_str(), face_index, ctx)?;
        }
    }

    for group in faces_by_edge.values().chain(faces_by_vertex.values()) {
        for &face in group {
            for &other in group {
                if other != face && !neighbors[face].contains(&other) {
                    ctx.charge_collection_items(1, "STEP connected-face links")?;
                    neighbors[face].insert(other);
                }
            }
        }
    }
    let mut reached = ctx.alloc_filled(face_ids.len(), false, "STEP connected-face reached")?;
    let mut components = Vec::new();
    for start in 0..face_ids.len() {
        if reached[start] {
            continue;
        }
        reached[start] = true;
        let mut component = Vec::new();
        let mut pending = Vec::new();
        push_connected_face_item(&mut pending, start, ctx, "STEP connected-face pending")?;
        while let Some(face) = pending.pop() {
            push_connected_face_item(&mut component, face, ctx, "STEP connected-face component")?;
            for &neighbor in &neighbors[face] {
                if !reached[neighbor] {
                    reached[neighbor] = true;
                    push_connected_face_item(
                        &mut pending,
                        neighbor,
                        ctx,
                        "STEP connected-face pending",
                    )?;
                }
            }
        }
        component.sort_unstable();
        push_connected_face_item(
            &mut components,
            component,
            ctx,
            "STEP connected-face components",
        )?;
    }
    Ok(components)
}

fn insert_connected_face_group<'a>(
    groups: &mut BTreeMap<&'a str, BTreeSet<usize>>,
    key: &'a str,
    face: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !groups.contains_key(key) {
        ctx.charge_collection_items(1, "STEP connected-face groups")?;
    }
    let group = groups.entry(key).or_default();
    if !group.contains(&face) {
        ctx.charge_collection_items(1, "STEP connected-face group faces")?;
        group.insert(face);
    }
    Ok(())
}

fn push_connected_face_item<T>(
    values: &mut Vec<T>,
    value: T,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values.try_reserve(1).map_err(|_| {
        cadmpeg_core::decode::refuse_local_limit(operation, u64_from_index(values.len()), 1)
    })?;
    values.push(value);
    Ok(())
}

fn shell_identity(root_id: u64, shell_step: u64, scope_root: bool) -> ShellId {
    if scope_root {
        ShellId::from(ids::data(
            kind!("shell"),
            IdentityKey::from(shell_step)
                .dash(key_word!("root"))
                .dash(root_id),
        ))
    } else {
        ShellId::from(ids::data(kind!("shell"), shell_step))
    }
}

fn scoped_edge_id(
    edge_step: u64,
    root_id: u64,
    shell_step: u64,
    scoped: bool,
    scope_root: bool,
) -> EdgeId {
    if scoped {
        if scope_root {
            EdgeId::from(ids::data(
                kind!("edge"),
                IdentityKey::from(edge_step)
                    .dash(key_word!("root"))
                    .dash(root_id)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        } else {
            EdgeId::from(ids::data(
                kind!("edge"),
                IdentityKey::from(edge_step)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        }
    } else {
        EdgeId::from(ids::data(kind!("edge"), edge_step))
    }
}

fn scoped_vertex_id(
    vertex_step: u64,
    root_id: u64,
    shell_step: u64,
    scoped: bool,
    scope_root: bool,
) -> VertexId {
    if scoped {
        if scope_root {
            VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(vertex_step)
                    .dash(key_word!("root"))
                    .dash(root_id)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        } else {
            VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(vertex_step)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        }
    } else {
        VertexId::from(ids::data(kind!("vertex"), vertex_step))
    }
}

fn scoped_poly_vertex_id(
    point_step: u64,
    root_id: u64,
    shell_step: u64,
    scoped: bool,
    scope_root: bool,
) -> VertexId {
    if scoped {
        if scope_root {
            VertexId::from(ids::data(
                kind!("vertex"),
                key_word!("poly")
                    .dash(key_word!("point"))
                    .dash(point_step)
                    .dash(key_word!("root"))
                    .dash(root_id)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        } else {
            VertexId::from(ids::data(
                kind!("vertex"),
                key_word!("poly")
                    .dash(key_word!("point"))
                    .dash(point_step)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        }
    } else {
        VertexId::from(ids::data(
            kind!("vertex"),
            key_word!("poly").dash(key_word!("point")).dash(point_step),
        ))
    }
}

fn poly_edge_id(
    start: u64,
    end: u64,
    root_id: u64,
    shell_step: u64,
    scoped: bool,
    scope_root: bool,
) -> EdgeId {
    if scoped {
        if scope_root {
            EdgeId::from(ids::data(
                kind!("edge"),
                key_word!("poly")
                    .dash(start)
                    .dash(end)
                    .dash(key_word!("root"))
                    .dash(root_id)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        } else {
            EdgeId::from(ids::data(
                kind!("edge"),
                key_word!("poly")
                    .dash(start)
                    .dash(end)
                    .dash(key_word!("shell"))
                    .dash(shell_step),
            ))
        }
    } else {
        EdgeId::from(ids::data(
            kind!("edge"),
            key_word!("poly").dash(start).dash(end),
        ))
    }
}

fn implicit_face_points(
    bounds: &[u64],
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    point_positions: &CarrierIndex,
) -> Option<Vec<Vec<Point3>>> {
    let mut loops = Vec::with_capacity(bounds.len());
    for &bound_step in bounds {
        let bound = exchange.records().get(&bound_step)?;
        let bound_type = face_bound_attribute_type(bound)?;
        let loop_step = named_reference(bound, bound_type, 1, 0)?;
        let loop_record = exchange.records().get(&loop_step)?;
        loop_record.partial("POLY_LOOP")?;
        let bound_forward = named_logical(bound, bound_type, 2, 0)?;
        let mut point_steps = named_refs(loop_record, "POLY_LOOP", 1)?;
        if point_steps.first() == point_steps.last() {
            point_steps.pop();
        }
        point_steps.dedup();
        if point_steps.len() < 3
            || point_steps.iter().collect::<BTreeSet<_>>().len() != point_steps.len()
        {
            return None;
        }
        if !bound_forward {
            point_steps.reverse();
        }
        let mut points = Vec::with_capacity(point_steps.len());
        for point_step in point_steps {
            let point_step = vdefs
                .get(&point_step)
                .map_or(point_step, |vertex| vertex.point);
            let point = point_positions.get(point_step).copied()?;
            if points.last().is_none_or(|previous| *previous != point) {
                points.push(point);
            }
        }
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 3 {
            return None;
        }
        loops.push(points);
    }
    (!loops.is_empty()).then_some(loops)
}

const IMPLICIT_FACE_AREA_RELATIVE_TOLERANCE: f64 = EPS_TOPOLOGY_READ_EXACT_GEOMETRY;
const IMPLICIT_FACE_NORMAL_ALIGNMENT_TOLERANCE: f64 = EPS_TOPOLOGY_READ_DEGENERATE;
const IMPLICIT_FACE_PLANAR_RELATIVE_TOLERANCE: f64 = EPS_TOPOLOGY_READ_EXACT_GEOMETRY;

fn implicit_face_plane(
    bounds: &[u64],
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    point_positions: &CarrierIndex,
) -> Option<SurfaceGeometry> {
    let loops = implicit_face_points(bounds, exchange, vdefs, point_positions)?;
    let mut points = loops.iter().flatten().copied().collect::<Vec<_>>();
    points.sort_by(|left, right| {
        left.x
            .total_cmp(&right.x)
            .then_with(|| left.y.total_cmp(&right.y))
            .then_with(|| left.z.total_cmp(&right.z))
    });
    let point_count = points.len() as f64;
    let origin = Point3::new(
        points.iter().map(|point| point.x).sum::<f64>() / point_count,
        points.iter().map(|point| point.y).sum::<f64>() / point_count,
        points.iter().map(|point| point.z).sum::<f64>() / point_count,
    );
    let relative_points = points
        .iter()
        .map(|point| point.vector_from(origin))
        .collect::<Vec<_>>();
    let scale = relative_points
        .iter()
        .map(Vector3::norm)
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= f64::EPSILON {
        return None;
    }
    let mut loop_normals = Vec::with_capacity(loops.len());
    for loop_points in &loops {
        let loop_count = loop_points.len() as f64;
        let loop_origin = Point3::new(
            loop_points.iter().map(|point| point.x).sum::<f64>() / loop_count,
            loop_points.iter().map(|point| point.y).sum::<f64>() / loop_count,
            loop_points.iter().map(|point| point.z).sum::<f64>() / loop_count,
        );
        let relative_loop = loop_points
            .iter()
            .map(|point| point.vector_from(loop_origin))
            .collect::<Vec<_>>();
        let mut area_normal = Vector3::new(0.0, 0.0, 0.0);
        for (current, next) in relative_loop
            .iter()
            .zip(relative_loop.iter().cycle().skip(1))
            .take(relative_loop.len())
        {
            area_normal = area_normal + current.cross(*next);
        }
        let area = area_normal.norm();
        if !area.is_finite() || area <= IMPLICIT_FACE_AREA_RELATIVE_TOLERANCE * scale * scale {
            return None;
        }
        loop_normals.push((UnitVector3::normalized(area_normal)?, area));
    }
    let mut normal = loop_normals.first().map(|(normal, _)| *normal)?;
    let mut largest_area = loop_normals.first().map(|(_, area)| *area)?;
    for (candidate, area) in loop_normals.iter().skip(1).copied() {
        let (candidate_raw, normal_raw) = (candidate.as_raw(), normal.as_raw());
        if area > largest_area
            || (area == largest_area
                && (candidate_raw.x, candidate_raw.y, candidate_raw.z)
                    > (normal_raw.x, normal_raw.y, normal_raw.z))
        {
            normal = candidate;
            largest_area = area;
        }
    }
    for (candidate, _) in &loop_normals {
        if candidate.as_raw().dot(*normal.as_raw()) < 1.0 - IMPLICIT_FACE_NORMAL_ALIGNMENT_TOLERANCE
        {
            return None;
        }
    }
    let planarity_tolerance =
        COINCIDENCE_TOLERANCE.max(IMPLICIT_FACE_PLANAR_RELATIVE_TOLERANCE * scale);
    if relative_points
        .iter()
        .map(|point| point.dot(*normal.as_raw()).abs())
        .fold(0.0, f64::max)
        > planarity_tolerance
    {
        return None;
    }
    let mut u_axis = None;
    let mut u_axis_norm = 0.0;
    for axis in [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ] {
        let projected = axis - normal.as_raw().scale(axis.dot(*normal.as_raw()));
        let norm = projected.norm();
        if norm > u_axis_norm {
            u_axis_norm = norm;
            u_axis = UnitVector3::normalized(projected);
        }
    }
    let u_axis = u_axis?;
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::new(
            cadmpeg_ir::features::FinitePoint3::new(origin)?,
            OrthonormalFrame3::from_units(normal, u_axis)?,
        ),
    )))
}

fn associated_pcurves(
    curve_step: u64,
    surface_step: u64,
    exchange: &Exchange,
    decoded_pcurves: &BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<PcurveId>, CodecError> {
    let Some(curve) = exchange.records().get(&curve_step) else {
        return Ok(Vec::new());
    };
    if !curve.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        )
    }) {
        return Ok(Vec::new());
    }
    let mut associated = Vec::new();
    for pcurve_step in surface_curve_pcurves(curve) {
        let Some(pcurve) = exchange.records().get(&pcurve_step) else {
            continue;
        };
        if pcurve.partial("PCURVE").is_some()
            && entity_parameter(pcurve, "PCURVE", 1)
                .and_then(Value::reference)
                == Some(surface_step)
            && decoded_pcurves.contains(&pcurve_step)
        {
            let pcurve_id = PcurveId::from(ids::data(kind!("pcurve"), pcurve_step));
            push_topology_vec(&mut associated, pcurve_id, ctx, "step_associated_pcurves")?;
        }
    }
    Ok(associated)
}

/// Select the only non-seam pcurve candidate with endpoint and locus witnesses.
/// Part 42 supplies no selector for competing same-surface pcurves; the CADIR
/// policy leaves that set detached. A sole candidate uses its declared trim or
/// a bounded search when no usable finite trim is declared. The model-space
/// samples are an admission witness, not a global equality proof.
struct SelectedPcurve {
    id: PcurveId,
    parameter_range: Option<[f64; 2]>,
}

enum PcurveSelectionFailure {
    NotUnique { count: usize },
    Carrier,
    Endpoint,
    Locus,
    ResourceLimit(ResourceLimit),
}

impl From<ResourceLimit> for PcurveSelectionFailure {
    fn from(limit: ResourceLimit) -> Self {
        Self::ResourceLimit(limit)
    }
}

const PCURVE_ENDPOINT_GRID_DIVISIONS: usize = 64;

#[derive(Clone, Copy)]
struct PcurveEndpointFit {
    start_parameter: f64,
    end_parameter: f64,
    /// Maximum model-space residual at the two returned parameters. This is
    /// the admission witness. It does not claim that the search found a
    /// global nearest point on the mapped pcurve.
    max_residual: f64,
}

#[allow(clippy::too_many_arguments)]
fn select_associated_pcurve(
    ir: &CadIr,
    exchange: &Exchange,
    surface_step: u64,
    edge: &EdgeDef,
    vdefs: &BTreeMap<u64, VertexDef>,
    point_positions: &CarrierIndex,
    candidates: &[PcurveId],
) -> Result<SelectedPcurve, PcurveSelectionFailure> {
    let [candidate] = candidates else {
        return Err(PcurveSelectionFailure::NotUnique {
            count: candidates.len(),
        });
    };
    let candidate = candidate.clone();
    let surface_identity = ids::data(kind!("surface"), surface_step);
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == surface_identity.as_str())
        .map(|surface| surface.geometry.clone())
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let surface_id = SurfaceId::from(surface_identity);
    let index = ModelIndex::new(ir);
    let pcurve = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == candidate)
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let geometry = &pcurve.geometry;
    let bound = COINCIDENCE_TOLERANCE.max(ir.tolerances.linear.get());
    let start = vdefs
        .get(&edge.vertices().0)
        .and_then(|vertex| point_positions.get(vertex.point))
        .copied()
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let end = vdefs
        .get(&edge.vertices().1)
        .and_then(|vertex| point_positions.get(vertex.point))
        .copied()
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let (curve_start, curve_end) = if edge.same() {
        (start, end)
    } else {
        (end, start)
    };
    let endpoint = pcurve_endpoint_fit(
        &index,
        &surface_id,
        geometry,
        &surface,
        curve_start,
        curve_end,
    )?
    .ok_or(PcurveSelectionFailure::Endpoint)?;
    if !endpoint.max_residual.is_finite() || !bound.is_finite() || endpoint.max_residual > bound {
        return Err(PcurveSelectionFailure::Endpoint);
    }
    if !pcurve_locus_witness(
        &index,
        exchange,
        edge,
        &surface_id,
        geometry,
        endpoint,
        curve_start,
        curve_end,
        bound,
    )? {
        return Err(PcurveSelectionFailure::Locus);
    }
    let parameter_range = if let Some(range) = pcurve_declared_parameter_range(geometry) {
        let declared = pcurve_declared_endpoint_fit_directed(
            &index,
            &surface_id,
            geometry,
            range,
            curve_start,
            curve_end,
        )?;
        declared
            .filter(|declared| declared.is_finite() && *declared > COINCIDENCE_TOLERANCE)
            .map(|_| [endpoint.start_parameter, endpoint.end_parameter])
    } else {
        None
    };
    drop(index);
    Ok(SelectedPcurve {
        id: candidate,
        parameter_range,
    })
}

const PCURVE_LOCUS_SAMPLE_COUNT: usize = 23;

// Keep the witness inputs explicit: each one names a separate source or
// admission value in the Part 42 association check.
#[allow(clippy::too_many_arguments)]
fn pcurve_locus_witness(
    index: &ModelIndex<'_>,
    exchange: &Exchange,
    edge: &EdgeDef,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    endpoint: PcurveEndpointFit,
    curve_start: Point3,
    curve_end: Point3,
    bound: f64,
) -> Result<bool, ResourceLimit> {
    let Some(curve_step) = edge
        .curve()
        .and_then(|curve| curve_carrier_record(curve, exchange))
    else {
        return Ok(false);
    };
    let curve_id = CurveId::from(ids::data(kind!("curve"), curve_step));
    let curve_seeds = curve_selection_parameter_domain(index, &curve_id).map_or(
        [
            0.0,
            1.0,
            -1.0,
            std::f64::consts::PI,
            -std::f64::consts::PI,
            0.5,
        ],
        |domain| {
            [
                domain[0],
                domain[1],
                domain[0].midpoint(domain[1]),
                0.0,
                1.0,
                -1.0,
            ]
        },
    );
    let Some(curve_start_parameter) =
        curve_parameter_near_point(index, &curve_id, curve_start, &curve_seeds, bound)?
    else {
        return Ok(false);
    };
    let Some(curve_end_parameter) =
        curve_parameter_near_point(index, &curve_id, curve_end, &curve_seeds, bound)?
    else {
        return Ok(false);
    };
    let mut fractions = (0..PCURVE_LOCUS_SAMPLE_COUNT)
        .map(|step| step as f64 / (PCURVE_LOCUS_SAMPLE_COUNT - 1) as f64)
        .collect::<Vec<_>>();
    let mut break_fractions = Vec::new();
    pcurve_parameter_break_fractions(
        geometry,
        [endpoint.start_parameter, endpoint.end_parameter],
        &mut break_fractions,
    );
    fractions.extend(break_fractions);
    fractions.sort_by(f64::total_cmp);
    fractions.dedup_by(|left, right| *left == *right);
    for fraction in fractions {
        let pcurve_parameter = endpoint
            .start_parameter
            .mul_add(1.0 - fraction, endpoint.end_parameter * fraction);
        let Some(uv) = pcurve_selection_uv(geometry, pcurve_parameter)? else {
            return Ok(false);
        };
        let Some(mapped) = surface_selection_point(index, surface_id, uv.u, uv.v)? else {
            return Ok(false);
        };
        let curve_seed =
            curve_start_parameter.mul_add(1.0 - fraction, curve_end_parameter * fraction);
        let mut seeds = curve_seeds.to_vec();
        seeds.push(curve_seed);
        let Some(curve_parameter) =
            curve_parameter_near_point(index, &curve_id, mapped, &seeds, bound)?
        else {
            return Ok(false);
        };
        let curve_point = match model_curve_point_by_id(index, &curve_id, curve_parameter) {
            Ok(point) => point,
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(_) => return Ok(false),
        };
        if !curve_point.distance(mapped).is_finite()
            || curve_point.distance(mapped) > bound
            || !fraction.is_finite()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn curve_parameter_near_point(
    index: &ModelIndex<'_>,
    curve_id: &CurveId,
    point: Point3,
    seeds: &[f64],
    tolerance: f64,
) -> Result<Option<f64>, ResourceLimit> {
    let mut best: Option<(f64, f64)> = None;
    for &seed in seeds.iter().filter(|seed| seed.is_finite()) {
        let Some(parameter) = model_curve_parameter_near_point_in_index_with_tolerance(
            index, curve_id, point, seed, tolerance,
        )?
        else {
            continue;
        };
        let candidate = ((parameter.get() - seed).abs(), parameter.get());
        if best.is_none_or(|current| candidate.0.total_cmp(&current.0).is_lt()) {
            best = Some(candidate);
        }
    }
    Ok(best.map(|(_, parameter)| parameter))
}

fn pcurve_endpoint_fit(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    surface: &SurfaceGeometry,
    start: Point3,
    end: Point3,
) -> Result<Option<PcurveEndpointFit>, ResourceLimit> {
    if let Some(parameter_range) = pcurve_declared_parameter_range(geometry) {
        let Some(declared_score) = pcurve_declared_endpoint_fit_directed(
            index,
            surface_id,
            geometry,
            parameter_range,
            start,
            end,
        )?
        else {
            return Ok(None);
        };
        if declared_score <= COINCIDENCE_TOLERANCE {
            return Ok(Some(PcurveEndpointFit {
                start_parameter: parameter_range[0],
                end_parameter: parameter_range[1],
                max_residual: declared_score,
            }));
        }
        // A few producers retain a stale trim around an edge-local pcurve.
        // Search for an alternative interval, then use the evaluated residual
        // as the witness. The search does not establish a global minimum.
        let seeds = pcurve_selection_seeds(index, surface_id, geometry, surface);
        let Some(start) = pcurve_surface_closest(index, surface_id, geometry, start, &seeds)?
        else {
            return Ok(None);
        };
        let Some(end) = pcurve_surface_closest(index, surface_id, geometry, end, &seeds)? else {
            return Ok(None);
        };
        return Ok(Some(PcurveEndpointFit {
            start_parameter: start.1,
            end_parameter: end.1,
            max_residual: start.0.max(end.0),
        }));
    }
    let seeds = pcurve_selection_seeds(index, surface_id, geometry, surface);
    let Some(start) = pcurve_surface_closest(index, surface_id, geometry, start, &seeds)? else {
        return Ok(None);
    };
    let Some(end) = pcurve_surface_closest(index, surface_id, geometry, end, &seeds)? else {
        return Ok(None);
    };
    Ok(Some(PcurveEndpointFit {
        start_parameter: start.1,
        end_parameter: end.1,
        max_residual: start.0.max(end.0),
    }))
}

fn pcurve_declared_parameter_range(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            Some(parameter_range.endpoints())
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_declared_parameter_range(basis)
        }
        PcurveGeometry::Transformed(placed) => pcurve_declared_parameter_range(placed.basis()),
        PcurveGeometry::Line(_)
        | PcurveGeometry::Circle(_)
        | PcurveGeometry::Ellipse(_)
        | PcurveGeometry::Harmonic(_)
        | PcurveGeometry::Parabola(_)
        | PcurveGeometry::Hyperbola(_)
        | PcurveGeometry::Hyperbolic(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle(_)
        | PcurveGeometry::Nurbs { .. } => None,
    }
}

fn surface_selection_parameters(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    u: f64,
    v: f64,
) -> [f64; 2] {
    let domains = index
        .surfaces(surface_id.as_str())
        .map_or([None, None], |surface| {
            surface_selection_parameter_domains(index, surface_id, &surface.geometry)
        });
    [
        clamp_selection_parameter(u, domains[0]),
        clamp_selection_parameter(v, domains[1]),
    ]
}

fn clamp_selection_parameter(value: f64, domain: Option<[f64; 2]>) -> f64 {
    let Some([lower, upper]) = domain else {
        return value;
    };
    let tolerance = EPS_TOPOLOGY_READ_EXACT_GEOMETRY * (1.0 + lower.abs().max(upper.abs()));
    if value < lower && lower - value <= tolerance {
        lower
    } else if value > upper && value - upper <= tolerance {
        upper
    } else {
        value
    }
}

fn surface_selection_point(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    u: f64,
    v: f64,
) -> Result<Option<Point3>, ResourceLimit> {
    let [u, v] = surface_selection_parameters(index, surface_id, u, v);
    // A non-finite point is returned as the evaluation reached it; the
    // selection measures read it as a miss.
    match model_surface_point_by_id(index, surface_id, u, v) {
        Ok(point) => Ok(Some(point.get())),
        Err(failure) => failure.non_finite(),
    }
}

#[cfg(test)]
fn pcurve_declared_endpoint_fit(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    range: [f64; 2],
    start: Point3,
    end: Point3,
) -> Result<Option<f64>, ResourceLimit> {
    let Some(first_uv) = pcurve_selection_uv(geometry, range[0])? else {
        return Ok(None);
    };
    let Some(last_uv) = pcurve_selection_uv(geometry, range[1])? else {
        return Ok(None);
    };
    let Some(first) = surface_selection_point(index, surface_id, first_uv.u, first_uv.v)? else {
        return Ok(None);
    };
    let Some(last) = surface_selection_point(index, surface_id, last_uv.u, last_uv.v)? else {
        return Ok(None);
    };
    let forward = first.distance(start).max(last.distance(end));
    let reversed = first.distance(end).max(last.distance(start));
    Ok(Some(forward.min(reversed)))
}

fn pcurve_declared_endpoint_fit_directed(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    range: [f64; 2],
    start: Point3,
    end: Point3,
) -> Result<Option<f64>, ResourceLimit> {
    let Some(first_uv) = pcurve_selection_uv(geometry, range[0])? else {
        return Ok(None);
    };
    let Some(last_uv) = pcurve_selection_uv(geometry, range[1])? else {
        return Ok(None);
    };
    let Some(first) = surface_selection_point(index, surface_id, first_uv.u, first_uv.v)? else {
        return Ok(None);
    };
    let Some(last) = surface_selection_point(index, surface_id, last_uv.u, last_uv.v)? else {
        return Ok(None);
    };
    Ok(Some(first.distance(start).max(last.distance(end))))
}

/// The pcurve point at `parameter`. A non-finite offset-pcurve point is
/// returned as the evaluation reached it; the selection measures read it as a
/// miss.
fn pcurve_selection_uv(
    geometry: &PcurveGeometry,
    parameter: f64,
) -> Result<Option<Point2>, ResourceLimit> {
    match pcurve_uv(geometry, parameter) {
        Ok(uv) => Ok(Some(uv.get())),
        Err(failure) => failure.non_finite(),
    }
}

fn pcurve_surface_closest(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    target: Point3,
    seeds: &[f64],
) -> Result<Option<(f64, f64)>, ResourceLimit> {
    // The minimum is only over the finite seed set. The caller treats the
    // directly evaluated result as a witness and omits the optional relation
    // when no witness meets the tolerance.
    let mut best: Option<(f64, f64)> = None;
    for &seed in seeds {
        let Some(candidate) = mapped_pcurve_closest(index, surface_id, geometry, target, seed)?
        else {
            continue;
        };
        if best.is_none_or(|current| candidate.0.total_cmp(&current.0).is_lt()) {
            best = Some(candidate);
        }
    }
    Ok(best)
}

/// Search for a low-residual parameter on one pcurve branch. A pcurve and its
/// 3D surface curve need not share parameter units, so this is an independent
/// one-dimensional inverse rather than a parameter copy. The returned distance
/// is evaluated at the returned parameter and is an admission witness, not a
/// proof of a global minimum.
fn mapped_pcurve_closest(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    target: Point3,
    seed: f64,
) -> Result<Option<(f64, f64)>, ResourceLimit> {
    if !seed.is_finite() {
        return Ok(None);
    }
    let domain = pcurve_selection_parameter_domain(geometry);
    let clamp_to_domain =
        |parameter: f64| domain.map_or(parameter, |[lower, upper]| parameter.clamp(lower, upper));
    let evaluate_point = |parameter: f64| -> Result<Option<Point3>, ResourceLimit> {
        let Some(uv) = pcurve_selection_uv(geometry, parameter)? else {
            return Ok(None);
        };
        surface_selection_point(index, surface_id, uv.u, uv.v)
    };
    let evaluate_tangent = |parameter: f64| -> Result<Option<Vector3>, ResourceLimit> {
        let Some(uv) = pcurve_selection_uv(geometry, parameter)? else {
            return Ok(None);
        };
        let tangent_uv = match pcurve_tangent(geometry, parameter) {
            Ok(value) => value,
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(_) => return Ok(None),
        };
        let [u, v] = surface_selection_parameters(index, surface_id, uv.u, uv.v);
        let partials = match model_surface_partials_by_id(index, surface_id, u, v) {
            Ok(value) => value,
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(_) => return Ok(None),
        };
        Ok(Some(Vector3::new(
            partials.du.x * tangent_uv.u + partials.dv.x * tangent_uv.v,
            partials.du.y * tangent_uv.u + partials.dv.y * tangent_uv.v,
            partials.du.z * tangent_uv.u + partials.dv.z * tangent_uv.v,
        )))
    };

    let mut parameter = clamp_to_domain(seed);
    let mut best = f64::INFINITY;
    let mut best_parameter = parameter;
    for _ in 0..32 {
        let Some(point) = evaluate_point(parameter)? else {
            return Ok(None);
        };
        let error = point.distance(target);
        if !error.is_finite() {
            return Ok(None);
        }
        if error < best {
            best = error;
            best_parameter = parameter;
        }
        let Some(tangent) = evaluate_tangent(parameter)? else {
            break;
        };
        let Some(step) =
            cadmpeg_ir::math::solve::projection_step(tangent, point.vector_from(target))
                .map(cadmpeg_ir::scalar::FiniteReal::get)
        else {
            break;
        };
        let mut candidate = clamp_to_domain(parameter - step);
        let Some(candidate_point) = evaluate_point(candidate)? else {
            break;
        };
        let mut candidate_error = candidate_point.distance(target);
        for _ in 0..12 {
            if candidate_error < error {
                break;
            }
            let Some(midpoint) = cadmpeg_ir::math::interpolate(candidate, parameter, 0.5) else {
                return Ok(None);
            };
            candidate = clamp_to_domain(midpoint.get());
            let Some(candidate_point) = evaluate_point(candidate)? else {
                break;
            };
            candidate_error = candidate_point.distance(target);
        }
        if candidate == parameter || !candidate_error.is_finite() || candidate_error >= error {
            break;
        }
        parameter = candidate;
    }
    Ok(best.is_finite().then_some((best, best_parameter)))
}

fn pcurve_parameter_break_fractions(
    geometry: &PcurveGeometry,
    parameters: [f64; 2],
    fractions: &mut Vec<f64>,
) {
    let mut add = |parameter: f64| {
        if let Some(fraction) =
            cadmpeg_ir::math::parameter_fraction(parameter, parameters[0], parameters[1])
        {
            if fraction.get() > 0.0 && fraction.get() < 1.0 {
                fractions.push(fraction.get());
            }
        }
    };
    match geometry {
        PcurveGeometry::Nurbs { nurbs } => nurbs.knots().iter().copied().for_each(&mut add),
        PcurveGeometry::PolarNurbs { nurbs } => {
            nurbs.knots().iter().copied().for_each(&mut add);
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            let basis = trimmed_pcurve.basis();
            add(parameter_range.endpoints()[0]);
            add(parameter_range.endpoints()[1]);
            pcurve_parameter_break_fractions(basis, parameters, fractions);
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_parameter_break_fractions(basis, parameters, fractions);
        }
        PcurveGeometry::Transformed(placed) => {
            pcurve_parameter_break_fractions(placed.basis(), parameters, fractions);
        }
        PcurveGeometry::Line(_)
        | PcurveGeometry::Circle(_)
        | PcurveGeometry::Ellipse(_)
        | PcurveGeometry::Harmonic(_)
        | PcurveGeometry::Parabola(_)
        | PcurveGeometry::Hyperbola(_)
        | PcurveGeometry::Hyperbolic(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::SphericalGreatCircle(_) => {}
    }
}

fn pcurve_selection_seeds(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    surface: &SurfaceGeometry,
) -> Vec<f64> {
    let mut seeds = vec![0.0];
    if let Some([start, end]) = pcurve_selection_parameter_domain(geometry) {
        let at_fraction = |fraction: f64| {
            let ordinary = start + (end - start) * fraction;
            if ordinary.is_finite() {
                Some(ordinary)
            } else {
                cadmpeg_ir::math::interpolate(start, end, fraction)
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
            }
        };
        seeds.push(start);
        seeds.extend(at_fraction(0.5));
        seeds.push(end);
        for step in 0..=PCURVE_ENDPOINT_GRID_DIVISIONS {
            let fraction = step as f64 / PCURVE_ENDPOINT_GRID_DIVISIONS as f64;
            seeds.extend(at_fraction(fraction));
        }
        let mut fractions = vec![0.0, 1.0];
        pcurve_parameter_break_fractions(geometry, [start, end], &mut fractions);
        fractions.sort_by(f64::total_cmp);
        fractions.dedup_by(|left, right| *left == *right);
        seeds.extend(
            fractions
                .iter()
                .filter_map(|fraction| at_fraction(*fraction)),
        );
        seeds.extend(fractions.windows(2).filter_map(|window| {
            let lower = window[0];
            let upper = window[1];
            at_fraction(lower + (upper - lower) * 0.5)
        }));
    }
    if pcurve_has_angular_parameterization(geometry) {
        seeds.extend([
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            std::f64::consts::PI * 1.5,
        ]);
    }
    if let Some((origin, direction)) = geometry.line_parameters() {
        if let Some(domain) = surface
            .solved()
            .and_then(|surface| surface_periodic_domains(surface)[0])
        {
            if direction.u != 0.0 {
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    if let Some(coordinate) = periodic_seed_coordinate(domain, fraction) {
                        seeds.push((coordinate - origin.u) / direction.u);
                    }
                }
            }
        }
        if let Some(domain) = surface
            .solved()
            .and_then(|surface| surface_periodic_domains(surface)[1])
        {
            if direction.v != 0.0 {
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    if let Some(coordinate) = periodic_seed_coordinate(domain, fraction) {
                        seeds.push((coordinate - origin.v) / direction.v);
                    }
                }
            }
        }
        let [u_domain, v_domain] = surface_selection_parameter_domains(index, surface_id, surface);
        if let Some([u_lower, u_upper]) = u_domain {
            for boundary in [u_lower, u_lower.midpoint(u_upper), u_upper] {
                if direction.u != 0.0 {
                    seeds.push((boundary - origin.u) / direction.u);
                }
            }
        }
        if let Some([v_lower, v_upper]) = v_domain {
            for boundary in [v_lower, v_lower.midpoint(v_upper), v_upper] {
                if direction.v != 0.0 {
                    seeds.push((boundary - origin.v) / direction.v);
                }
            }
        }
    }
    seeds
        .into_iter()
        .filter(|seed| seed.is_finite())
        .fold(Vec::new(), |mut unique, seed| {
            if !unique.contains(&seed) {
                unique.push(seed);
            }
            unique
        })
}

fn periodic_seed_coordinate([lower, upper]: [f64; 2], fraction: f64) -> Option<f64> {
    let period = upper - lower;
    if period.is_finite() {
        Some(period * fraction)
    } else {
        cadmpeg_ir::math::interpolate(lower, upper, fraction)
            .map(cadmpeg_ir::scalar::FiniteReal::get)
    }
}

fn pcurve_has_angular_parameterization(geometry: &PcurveGeometry) -> bool {
    match geometry {
        PcurveGeometry::Circle(_)
        | PcurveGeometry::Ellipse(_)
        | PcurveGeometry::Harmonic(_)
        | PcurveGeometry::SphericalGreatCircle(_) => true,
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_has_angular_parameterization(basis)
        }
        PcurveGeometry::Transformed(placed) => pcurve_has_angular_parameterization(placed.basis()),
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let basis = trimmed_pcurve.basis();
            pcurve_has_angular_parameterization(basis)
        }
        PcurveGeometry::Line(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::Nurbs { .. }
        | PcurveGeometry::Parabola(_)
        | PcurveGeometry::Hyperbola(_)
        | PcurveGeometry::Hyperbolic(_) => false,
    }
}

fn pcurve_selection_parameter_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs { nurbs } => nurbs_pcurve_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.control_points().len(),
        )
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints),
        PcurveGeometry::PolarNurbs { nurbs } => {
            nurbs_pcurve_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.poles().len())
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            let basis = trimmed_pcurve.basis();
            if parameter_range.endpoints()[0] < parameter_range.endpoints()[1] {
                Some(parameter_range.endpoints())
            } else {
                pcurve_selection_parameter_domain(basis)
            }
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_selection_parameter_domain(basis)
        }
        PcurveGeometry::Transformed(placed) => pcurve_selection_parameter_domain(placed.basis()),
        PcurveGeometry::Line(_)
        | PcurveGeometry::Circle(_)
        | PcurveGeometry::Ellipse(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::SphericalGreatCircle(_)
        | PcurveGeometry::Harmonic(_)
        | PcurveGeometry::Parabola(_)
        | PcurveGeometry::Hyperbola(_)
        | PcurveGeometry::Hyperbolic(_) => None,
    }
}

fn surface_selection_parameter_domains(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    surface: &SurfaceGeometry,
) -> [Option<[f64; 2]>; 2] {
    let definition = index
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| {
            index.ir().model.procedural_surface_owner(&procedural.id) == Some(surface_id)
        })
        .map(cadmpeg_ir::geometry::ProceduralSurface::definition);
    match definition {
        Some(ProceduralSurfaceDefinition::Subset(definition_payload)) => {
            let parameter_ranges = definition_payload
                .parameter_ranges()
                .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints);
            [
                subset_parameter_domain(parameter_ranges[0]),
                subset_parameter_domain(parameter_ranges[1]),
            ]
        }
        Some(ProceduralSurfaceDefinition::AxisRevolution(definition_payload)) => [
            Some([0.0, std::f64::consts::TAU]),
            curve_selection_parameter_domain(index, definition_payload.directrix()),
        ],
        Some(ProceduralSurfaceDefinition::Extrusion(payload)) => [
            curve_selection_parameter_domain(index, payload.directrix()),
            None,
        ],
        Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => [
            curve_selection_parameter_domain(index, definition_payload.directrix()),
            None,
        ],
        Some(ProceduralSurfaceDefinition::Replica { source, .. }) => index
            .surfaces(source.as_str())
            .map_or([None, None], |source_surface| {
                surface_selection_parameter_domains(index, source, &source_surface.geometry)
            }),
        _ => surface.solved().map_or(
            [None, None],
            surface_selection_parameter_domains_from_geometry,
        ),
    }
}

fn surface_selection_parameter_domains_from_geometry(
    surface: &SolvedSurfaceGeometry,
) -> [Option<[f64; 2]>; 2] {
    match surface {
        SolvedSurfaceGeometry::Nurbs(surface) => {
            let (u_count, v_count) = (surface.u_count(), surface.v_count());
            [
                nurbs_pcurve_parameter_domain(surface.u_degree(), surface.u_knots(), u_count)
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints),
                nurbs_pcurve_parameter_domain(surface.v_degree(), surface.v_knots(), v_count)
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints),
            ]
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            surface_selection_parameter_domains_from_geometry(placed.basis())
        }
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => [None, None],
    }
}

fn subset_parameter_domain(range: [f64; 2]) -> Option<[f64; 2]> {
    let span = (range[1] - range[0]).abs();
    (span.is_finite() && span > 0.0).then_some([0.0, span])
}

fn curve_selection_parameter_domain(
    index: &ModelIndex<'_>,
    curve_id: &CurveId,
) -> Option<[f64; 2]> {
    let curve = index.curves(curve_id.as_str())?;
    curve_selection_parameter_domain_from_geometry(curve.geometry.solved()?)
}

fn curve_selection_parameter_domain_from_geometry(
    geometry: &SolvedCurveGeometry,
) -> Option<[f64; 2]> {
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            Some([0.0, std::f64::consts::TAU])
        }
        SolvedCurveGeometry::Nurbs(curve) => nurbs_curve_parameter_domain(curve)
            .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints),
        SolvedCurveGeometry::Polyline(polyline) => {
            let mut parameters = polyline.parameters()?;
            let lower = parameters.next()?.get();
            let upper = parameters.last().map_or(lower, |parameter| parameter.get());
            (lower < upper).then_some([lower, upper])
        }
        SolvedCurveGeometry::Transformed(placed) => {
            curve_selection_parameter_domain_from_geometry(placed.basis())
        }
        SolvedCurveGeometry::Line(_)
        | SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => None,
    }
}

struct ShellDef {
    base: u64,
    forward: bool,
    typed: HashSet<u64>,
}

fn shell_defs(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, ShellDef>, CodecError> {
    let mut cache = BTreeMap::<u64, Option<ShellDef>>::new();
    let mut active = BTreeSet::new();
    for (id, _) in exchange.entities_any(&[
        "ORIENTED_OPEN_SHELL",
        "ORIENTED_CLOSED_SHELL",
        "OPEN_SHELL",
        "CLOSED_SHELL",
    ]) {
        shell_def_cached(id, exchange, &mut active, &mut cache, ctx)?;
    }
    let mut shells = BTreeMap::new();
    for (id, definition) in cache {
        if let Some(definition) = definition {
            insert_topology_map(&mut shells, id, definition, ctx, "step_shell_definitions")?;
        }
    }
    Ok(shells)
}

fn copy_shell_def(
    definition: &ShellDef,
    ctx: &DecodeContext<'_>,
) -> Result<ShellDef, CodecError> {
    let mut typed = HashSet::new();
    for &id in &definition.typed {
        insert_topology_hash_set(&mut typed, id, ctx, "step_shell_definition_typed_copy")?;
    }
    Ok(ShellDef {
        base: definition.base,
        forward: definition.forward,
        typed,
    })
}

fn shell_def_cached(
    reference: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, Option<ShellDef>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ShellDef>, CodecError> {
    if let Some(definition) = cache.get(&reference) {
        return definition.as_ref().map(|definition| copy_shell_def(definition, ctx)).transpose();
    }
    let _depth = ctx.enter_nested("step_shell_definition_recursion")?;
    if active.contains(&reference) {
        return Ok(None);
    }
    insert_topology_set(active, reference, ctx, "step_shell_definition_active")?;
    let result = if let Some(record) = exchange.records().get(&reference) {
        match most_specific(record, &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL", "OPEN_SHELL", "CLOSED_SHELL"]) {
            Some("OPEN_SHELL" | "CLOSED_SHELL") => Some(ShellDef {
                base: reference,
                forward: true,
                typed: HashSet::new(),
            }),
            Some("ORIENTED_OPEN_SHELL" | "ORIENTED_CLOSED_SHELL") => {
                let shell_type = most_specific(record, &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL"]);
                let (element, orientation) = if record.partials.len() == 1 {
                    match record.parameter(1) {
                        Some(Value::Derived) => (
                            record.parameter(2).and_then(ValueExt::reference),
                            record.parameter(3).and_then(ValueExt::logical),
                        ),
                        Some(Value::Reference(_)) => (
                            record.parameter(1).and_then(ValueExt::reference),
                            record.parameter(2).and_then(ValueExt::logical),
                        ),
                        _ => (None, None),
                    }
                } else {
                    (
                        shell_type.and_then(|shell_type| named_reference(record, shell_type, 1, 0)),
                        shell_type.and_then(|shell_type| named_logical(record, shell_type, 2, 0)),
                    )
                };
                if let Some((element, orientation)) = element.zip(orientation) {
                    if let Some(mut definition) = shell_def_cached(element, exchange, active, cache, ctx)? {
                        definition.forward = definition.forward == orientation;
                        insert_topology_hash_set(&mut definition.typed, reference, ctx, "step_shell_definition_typed")?;
                        Some(definition)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };
    active.remove(&reference);
    let cached = result.as_ref().map(|definition| copy_shell_def(definition, ctx)).transpose()?;
    insert_topology_map(cache, reference, cached, ctx, "step_shell_definition_cache")?;
    Ok(result)
}

fn shell_def_for(
    reference: u64,
    shells: &BTreeMap<u64, ShellDef>,
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, bool)>, CodecError> {
    let Some(definition) = shells.get(&reference) else {
        return Ok(None);
    };
    for &id in &definition.typed {
        insert_topology_hash_set(typed, id, ctx, "step_shell_definition_claims")?;
    }
    Ok(Some((definition.base, definition.forward)))
}

#[derive(Default)]
struct FaceInfo {
    bounds: Vec<u64>,
    name: Option<Value>,
    surface: Option<u64>,
    same_sense: bool,
    reverse_bound_orientation: bool,
    typed: HashSet<u64>,
}

fn is_face_record(record: &RawRecord) -> bool {
    record.partial("FACE").is_some()
        || record.partial("ADVANCED_FACE").is_some()
        || record.partial("FACE_SURFACE").is_some()
        || record.partial("ORIENTED_FACE").is_some()
        || record.partial("SUBFACE").is_some()
}

fn face_attributes(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
) -> Option<FaceInfo> {
    if !active.insert(id) {
        return None;
    }
    let result = (|| match most_specific(
        record,
        &[
            "ORIENTED_FACE",
            "SUBFACE",
            "ADVANCED_FACE",
            "FACE_SURFACE",
            "FACE",
        ],
    )? {
        "ORIENTED_FACE" => {
            let face_element = oriented_face_element(record)?;
            let mut base = face_attributes(
                face_element,
                exchange.records().get(&face_element)?,
                exchange,
                active,
            )?;
            let orientation = oriented_face_orientation(record)?;
            if !orientation {
                base.reverse_bound_orientation = !base.reverse_bound_orientation;
            }
            base.same_sense = base.same_sense == orientation;
            if let Some(name) = face_name_value(record) {
                base.name = Some(name);
            }
            base.typed.insert(face_element);
            Some(base)
        }
        "SUBFACE" => {
            let parent = subface_parent(record)?;
            let mut parent_info =
                face_attributes(parent, exchange.records().get(&parent)?, exchange, active)?;
            let bounds = direct_face_bounds(record, exchange)?;
            parent_info.typed.insert(parent);
            if let Some(name) = face_name_value(record) {
                parent_info.name = Some(name);
            }
            Some(FaceInfo {
                bounds,
                name: parent_info.name,
                surface: parent_info.surface,
                same_sense: parent_info.same_sense,
                reverse_bound_orientation: parent_info.reverse_bound_orientation,
                typed: parent_info.typed,
            })
        }
        "FACE" => {
            let bounds = direct_face_bounds(record, exchange)?;
            Some(FaceInfo {
                bounds,
                name: face_name_value(record),
                surface: None,
                same_sense: true,
                reverse_bound_orientation: false,
                typed: HashSet::new(),
            })
        }
        "ADVANCED_FACE" | "FACE_SURFACE" => {
            let bounds = direct_face_bounds(record, exchange)?;
            let governing = most_specific(record, &["ADVANCED_FACE", "FACE_SURFACE"])?;
            let surface = direct_face_surface(record, &bounds, governing)?;
            let same_sense = direct_face_same_sense(record, governing)?;
            Some(FaceInfo {
                bounds,
                name: face_name_value(record),
                surface: Some(surface),
                same_sense,
                reverse_bound_orientation: false,
                typed: HashSet::new(),
            })
        }
        _ => None,
    })();
    active.remove(&id);
    result
}

fn face_name_value(record: &RawRecord) -> Option<Value> {
    let value = if record.partials.len() == 1 {
        record.parameter(0)
    } else {
        record
            .partial("REPRESENTATION_ITEM")
            .and_then(|partial| partial.parameters.first())
            .or_else(|| {
                [
                    "ORIENTED_FACE",
                    "SUBFACE",
                    "ADVANCED_FACE",
                    "FACE_SURFACE",
                    "FACE",
                ]
                .into_iter()
                .find_map(|name| {
                    record
                        .partial(name)
                        .and_then(|partial| partial.parameters.first())
                })
            })
    };
    value
        .filter(|value| !matches!(value, Value::String(bytes) if bytes.is_empty()))
        .cloned()
}

fn direct_face_bounds(record: &RawRecord, exchange: &Exchange) -> Option<Vec<u64>> {
    let values = if record.partials.len() == 1 {
        vec![entity_parameter(record, record.simple_name()?, 1)?]
    } else {
        record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
            .collect::<Vec<_>>()
    };
    values.into_iter().filter_map(refs).find(|ids| {
        !ids.is_empty()
            && ids.iter().all(|id| {
                exchange.records().get(id).is_some_and(|bound| {
                    bound.partial("FACE_BOUND").is_some()
                        || bound.partial("FACE_OUTER_BOUND").is_some()
                })
            })
    })
}

fn direct_face_surface(record: &RawRecord, bounds: &[u64], governing: &str) -> Option<u64> {
    if record.partials.len() > 1 {
        return named_reference(record, governing, 2, 0).or_else(|| {
            (governing == "ADVANCED_FACE")
                .then(|| named_reference(record, "FACE_SURFACE", 2, 0))
                .flatten()
        });
    }
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::reference)
        .find(|reference| !bounds.contains(reference))
}

fn direct_face_same_sense(record: &RawRecord, governing: &str) -> Option<bool> {
    if record.partials.len() > 1 {
        return named_logical(record, governing, 3, 0).or_else(|| {
            (governing == "ADVANCED_FACE")
                .then(|| named_logical(record, "FACE_SURFACE", 3, 0))
                .flatten()
        });
    }
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .find_map(ValueExt::logical)
}

fn oriented_face_element(record: &RawRecord) -> Option<u64> {
    if let Some(partial) = record.partials.iter().find(|p| p.name == "ORIENTED_FACE") {
        return partial
            .parameters
            .iter()
            .filter_map(ValueExt::reference)
            .next_back();
    }
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::reference)
        .next_back()
}

fn oriented_face_orientation(record: &RawRecord) -> Option<bool> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "ORIENTED_FACE")
        .into_iter()
        .flat_map(|partial| partial.parameters.iter())
        .find_map(ValueExt::logical)
        .or_else(|| direct_face_same_sense(record, "ORIENTED_FACE"))
}

fn subface_parent(record: &RawRecord) -> Option<u64> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "SUBFACE")
        .into_iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::reference)
        .next_back()
        .or_else(|| {
            record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .filter_map(ValueExt::reference)
                .next_back()
        })
}

fn refs(value: &Value) -> Option<Vec<u64>> {
    value.list()?.iter().map(ValueExt::reference).collect()
}

/// Selects the partial that carries inherited `FACE_BOUND` attributes.
/// `FACE_OUTER_BOUND` adds the outer role but may be empty in a complex
/// instance, so subtype classification and attribute lookup are separate.
fn face_bound_attribute_type(record: &RawRecord) -> Option<&'static str> {
    if record
        .partial("FACE_OUTER_BOUND")
        .is_some_and(|partial| partial.parameters.len() >= 3)
    {
        return Some("FACE_OUTER_BOUND");
    }
    if record
        .partial("FACE_BOUND")
        .is_some_and(|partial| partial.parameters.len() >= 3)
    {
        return Some("FACE_BOUND");
    }
    record
        .partial("FACE_BOUND")
        .map(|_| "FACE_BOUND")
        .or_else(|| {
            record
                .partial("FACE_OUTER_BOUND")
                .map(|_| "FACE_OUTER_BOUND")
        })
}

/// Returns the first partial name present in a subtype-first dispatch chain.
/// Complex STEP instances carry every inherited partial, so the first hit is
/// the governing subtype and its attributes must drive decoding.
fn most_specific<'a>(record: &RawRecord, chain: &[&'a str]) -> Option<&'a str> {
    chain
        .iter()
        .copied()
        .find(|name| record.partial(name).is_some())
}

fn connected_face_set_type(record: &RawRecord) -> Option<&'static str> {
    most_specific(record, &["CONNECTED_FACE_SUB_SET", "CONNECTED_FACE_SET"])
}

fn connected_set_members<'a>(record: &'a RawRecord, set_type: &str) -> Option<&'a [Value]> {
    let base_type = match set_type {
        "CONNECTED_EDGE_SUB_SET" => "CONNECTED_EDGE_SET",
        "CONNECTED_FACE_SUB_SET" => "CONNECTED_FACE_SET",
        _ => set_type,
    };
    named_reference_values(record, set_type, 1)
        .or_else(|| named_reference_values(record, base_type, 1))
}

fn validate_subset_parent(
    id: u64,
    record: &RawRecord,
    subset_type: &str,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let base_type = match subset_type {
        "CONNECTED_EDGE_SUB_SET" => "CONNECTED_EDGE_SET",
        "CONNECTED_FACE_SUB_SET" => "CONNECTED_FACE_SET",
        _ => return Ok(true),
    };
    let parent = if record.partials.len() == 1 {
        entity_parameter(record, subset_type, 2).and_then(ValueExt::reference)
    } else {
        record
            .partial(subset_type)
            .and_then(|partial| partial.parameters.iter().find_map(ValueExt::reference))
    };
    let Some(parent) = parent else {
        push_topology_vec(losses, StepLossCode::DecodeWarning.note(format!(
            "{subset_type} #{id} has no resolvable parent {base_type}"
        )), ctx, "step_topology_losses")?;
        return Ok(false);
    };
    if exchange
        .records()
        .get(&parent)
        .is_some_and(|parent_record| most_specific(parent_record, &[base_type]) == Some(base_type))
    {
        Ok(true)
    } else {
        push_topology_vec(losses, StepLossCode::DecodeWarning.note(format!(
            "{subset_type} #{id} parent #{parent} does not resolve to {base_type}"
        )), ctx, "step_topology_losses")?;
        Ok(false)
    }
}

fn entity_parameter<'a>(record: &'a RawRecord, name: &str, index: usize) -> Option<&'a Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .or_else(|| (record.partials.len() == 1).then(|| &record.partials[0]))?
        .parameters
        .get(index)
}
