// SPDX-License-Identifier: Apache-2.0
//! STEP boundary-representation ownership and orientation decoding.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::rc::Rc;

use super::geometry::curve_carrier_record;
use super::{source_numeric_id, RecordExt, ValueExt};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{CommitSession, DraftError, ModelDraft};
use cadmpeg_ir::eval::model_curve_parameter_near_point_in_index_with_tolerance;
use cadmpeg_ir::eval::model_curve_point_by_id;
use cadmpeg_ir::eval::model_surface_partials_by_id;
use cadmpeg_ir::eval::model_surface_point_by_id;
use cadmpeg_ir::eval::nurbs_curve_parameter_domain;
use cadmpeg_ir::eval::nurbs_pcurve_parameter_domain;
use cadmpeg_ir::eval::pcurve_tangent;
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

fn push_topology_body_group(
    groups: &mut BTreeMap<u64, Vec<BodyId>>,
    key: u64,
    body: &BodyId,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    member_operation: &'static str,
) -> Result<(), CodecError> {
    ctx.admit_btree_entry(groups, &key, group_operation)?;
    let copy = body.try_clone_for_decode(ctx, member_operation)?;
    ctx.push_vec(groups.entry(key).or_default(), copy, member_operation)
}

fn insert_topology_body_group(
    groups: &mut BTreeMap<u64, BTreeSet<BodyId>>,
    key: u64,
    body: &BodyId,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    member_operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(bodies) = groups.get(&key) {
        if ctx.contains_btree_set(bodies, body, "STEP topology body group membership")? {
            return Ok(());
        }
    }
    ctx.admit_btree_entry(groups, &key, group_operation)?;
    let copy = body.try_clone_for_decode(ctx, member_operation)?;
    ctx.insert_btree_set(groups.entry(key).or_default(), copy, member_operation)?;
    Ok(())
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
        DraftError::Resource(limit) => Err(CodecError::ResourceLimit(*limit)),
DraftError::Admission(message) => ctx.copy_retained_text(message, "step_topology_commit_error_text"),
DraftError::IdentityCollision(identity) => ctx.format_retained(format_args!("{context} conflicts with decoded topology: identity collision at '{identity}': {error}"), "step_topology_commit_error_text"),
        DraftError::UnresolvedReference { .. }
        | DraftError::FeatureParents { .. } => {
            ctx.format_retained(format_args!("{context} conflicts with decoded topology: {error}"), "step_topology_commit_error_text")
        }
    }
}

/// Body identifiers held with their temporary decode reservation.
#[derive(Debug)]
pub(super) struct AdmittedRepresentationBodies<'a> {
    values: Vec<BodyId>,
    reservation: ScopedReservation<'a>,
}

impl<'a> AdmittedRepresentationBodies<'a> {
    pub(super) fn into_parts(self) -> (Vec<BodyId>, ScopedReservation<'a>) {
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
    ctx: &'a DecodeContext<'_>,
    operation: &'static str,
) -> Result<AdmittedRepresentationBodies<'a>, cadmpeg_core::CodecError> {
    ctx.charge_collection_items(u64_from_index(bodies.len()), operation)?;
    let mut bytes = ctx.reserve_scoped(0, operation)?;
    let mut values = Vec::new();

    bytes.with_storage(|| ctx.reserve_capacity(&mut values, bodies.len(), operation))?;
    for body in ctx.admit_iter(bodies, "STEP admitted body clone traversal")? {
        values.push(bytes.with_storage(|| body.try_clone_for_decode(ctx, operation))?);
    }
    Ok(AdmittedRepresentationBodies {
        values,
        reservation: bytes,
    })
}

fn cache_representation_bodies<'a>(
    cache: &mut BTreeMap<u64, AdmittedRepresentationBodies<'a>>,
    representation: u64,
    bodies: &[BodyId],
    ctx: &'a DecodeContext<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut admitted = admitted_body_clone(bodies, ctx, "step_representation_body_cache_values")?;
    admitted
        .reservation
        .grow(u64_from_index(std::mem::size_of::<(
            u64,
            AdmittedRepresentationBodies<'_>,
        )>()))?;
    ctx.insert_btree_map(
        cache,
        representation,
        admitted,
        "step_representation_body_cache_entries",
    )?;
    Ok(())
}

fn insert_body_id(
    bodies: &mut BTreeSet<BodyId>,
    body: &BodyId,
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    if ctx.contains_btree_set(bodies, body, "STEP bodies membership")? {
        return Ok(());
    }
    bytes.grow(u64_from_index(std::mem::size_of::<BodyId>()))?;
    let body =
        bytes.with_storage(|| body.try_clone_for_decode(ctx, "step_representation_body_set"))?;
    ctx.insert_btree_set(bodies, body, "step_representation_body_set")?;
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
    ctx: &'a DecodeContext<'_>,
) -> Result<AdmittedRepresentationBodies<'a>, cadmpeg_core::CodecError> {
    if let Some(bodies) = cache.get(&representation) {
        return admitted_body_clone(bodies, ctx, "step_representation_body_cache_copy");
    }
    let _depth_guard = ctx.enter_nested("step_representation_body_walk")?;
    if let Some(bodies) = topology.body_by_root.get(&representation) {
        let bodies = admitted_body_clone(bodies, ctx, "step_representation_body_root_copy")?;
        cache_representation_bodies(cache, representation, &bodies, ctx)?;
        return Ok(bodies);
    }
    if active.contains(&representation) {
        return admitted_body_clone(&[], ctx, "step_representation_body_empty");
    }
    let active_bytes = {
        ctx.reserve_scoped(
            u64_from_index(std::mem::size_of::<u64>()),
            "step_representation_body_active",
        )?
    };
    ctx.insert_btree_set(active, representation, "step_representation_body_active")?;
    let mut body_ids = BTreeSet::new();
    let mut body_ids_bytes = ctx.reserve_scoped(0, "step_representation_body_set")?;
    if let Some(items) = exchange
        .records()
        .get(&representation)
        .map(|record| representation_item_values(ctx, record))
        .transpose()?
        .flatten()
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
            if record.partial(ctx, "MAPPED_ITEM")?.is_none() {
                continue;
            }
            let Some(mapped_representation) = mapped_representation(ctx, record, exchange)? else {
                continue;
            };
            let nested = representation_bodies(
                mapped_representation,
                exchange,
                topology,
                cache,
                active,
                ctx,
            )?;
            for body in ctx.admit_iter(&nested[..], "STEP representation bodies traversal")? {
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
        let nested = representation_bodies(related, exchange, topology, cache, active, ctx)?;
        for body in ctx.admit_iter(&nested[..], "STEP representation bodies traversal")? {
            insert_body_id(&mut body_ids, body, ctx, &mut body_ids_bytes)?;
        }
    }

    let mut bodies = Vec::new();

    ctx.reserve_scoped_vec(
        &mut body_ids_bytes,
        &mut bodies,
        body_ids.len(),
        "step_representation_body_output",
    )?;
    bodies.extend(body_ids);
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
    for record in ctx
        .admit_iter(
            exchange.records(),
            "STEP shape representation relationships map traversal",
        )?
        .map(|(_, value)| value)
    {
        let Some(relationship) = record.partial(ctx, "SHAPE_REPRESENTATION_RELATIONSHIP")? else {
            continue;
        };
        let mut references = relationship
            .parameters
            .iter()
            .filter_map(ValueExt::reference);
        let (first, second) = match (references.next(), references.next()) {
            (Some(first), Some(second)) => (first, second),
            _ => {
                let Some(base) = record.partial(ctx, "REPRESENTATION_RELATIONSHIP")? else {
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
        ctx.push_btree_group(
            &mut related,
            first,
            second,
            "step_shape_relationship_groups",
            "step_shape_relationship_members",
        )?;
        ctx.push_btree_group(
            &mut related,
            second,
            first,
            "step_shape_relationship_groups",
            "step_shape_relationship_members",
        )?;
    }
    for representations in related.values_mut() {
        ctx.sort_unstable_by(
            representations,
            |value| value,
            Ord::cmp,
            "step_shape_relationship_sort",
        )?;
        representations.dedup();
    }
    Ok(related)
}

fn representation_item_values<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    if record.partials.len() == 1 {
        if let Some(value) = entity_parameter(ctx, record, "REPRESENTATION", 1)? {
            if let Some(items) = reference_values(ctx, value)? {
                return Ok(Some(items));
            }
        }
        if let Some(name) = record.simple_name() {
            if let Some(value) = entity_parameter(ctx, record, name, 1)? {
                return reference_values(ctx, value);
            }
        }
        return Ok(None);
    }
    let Some(partial) = record.partial(ctx, "REPRESENTATION")? else {
        return Ok(None);
    };
    for value in ctx.admit_iter(
        partial.parameters.as_slice(),
        "STEP representation reference parameter traversal",
    )? {
        if let Some(items) = reference_values(ctx, value)? {
            return Ok(Some(items));
        }
    }
    Ok(None)
}

fn named_reference_values<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    name: &'static str,
    simple_index: usize,
) -> Result<Option<&'a [Value]>, CodecError> {
    if record.partials.len() == 1 {
        return if let Some(value) = entity_parameter(ctx, record, name, simple_index)? {
            reference_values(ctx, value)
        } else {
            Ok(None)
        };
    }
    let Some(partial) = record.partial(ctx, name)? else {
        return Ok(None);
    };
    for value in ctx.admit_iter(
        partial.parameters.as_slice(),
        "STEP named reference parameter traversal",
    )? {
        if let Some(items) = reference_values(ctx, value)? {
            return Ok(Some(items));
        }
    }
    Ok(None)
}

fn reference_values<'a>(
    ctx: &DecodeContext<'_>,
    value: &'a Value,
) -> Result<Option<&'a [Value]>, CodecError> {
    let Some(items) = value.list() else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(
            items,
            |item| Ok(item.reference().is_some()),
            "STEP topology reference list validation",
        )?
        .then_some(items))
}

fn mapped_representation(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    exchange: &Exchange,
) -> Result<Option<u64>, CodecError> {
    let Some(map) = named_reference(ctx, record, "MAPPED_ITEM", 1, 0)? else {
        return Ok(None);
    };
    if let Some(map) = exchange.records().get(&map) {
        named_reference(ctx, map, "REPRESENTATION_MAP", 1, 1)
    } else {
        Ok(None)
    }
}

pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    carrier_index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<TopologyData>, CodecError> {
    let mut commit_session = CommitSession::new(ir, ctx, None)?;
    let mut result = StageOutcome {
        value: TopologyData {
            body_by_root: BTreeMap::new(),
            shape_representation_relationships: shape_representation_relationships(exchange, ctx)?,
            body_by_shell: BTreeMap::new(),
            faces_by_source: BTreeMap::new(),
            edges_by_source: BTreeMap::new(),
            vertices_by_source: BTreeMap::new(),
        },
        claims: BTreeSet::new(),
        losses: Vec::new(),
        notes: Vec::new(),
    };
    let mut losses: Vec<LossNote> = Vec::new();
    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        let Some(name) = most_specific(
            ctx,
            record,
            &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL"],
        )?
        else {
            continue;
        };
        if record.partials.len() != 1 || matches!(record.parameter(1), Some(Value::Derived)) {
            continue;
        }
        ctx.push_vec(
            &mut result.losses,
            StepLossCode::OrientedShellOmitsCfsFaces
                .note(ctx.format_retained(
                    format_args!(
                    "{name} #{id} omits the derived `cfs_faces` slot required by ISO 10303-21; \
                 read the shell element from positional slot 1"
                ),
                    "STEP decode text",
                )?)
                .with_provenance(
                    cadmpeg_ir::SourceProvenance::root(
                        crate::dialect::FORMAT,
                        u64_from_index(record.span.start),
                    )
                    .with_tag("oriented_shell"),
                ),
            "step_topology_losses",
        )?;
    }
    let vertices = vertex_defs(exchange, ctx)?;
    let edges = edge_defs(exchange, ctx)?;
    let oriented = oriented_defs(exchange, ctx)?;
    let shells = shell_defs(exchange, ctx)?;
    let point_positions = carrier_index;
    for indexed_entity in exchange.entities(ctx, "VERTEX_POINT")? {
        let (vertex_id, vertex) = indexed_entity?;
        let Some(point_id) = named_reference(ctx, vertex, "VERTEX_POINT", 1, 0)? else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "VERTEX_POINT #{vertex_id} has no resolvable point carrier"
                )),
                "step_topology_losses",
            )?;
            continue;
        };
        if !carrier_index.points.contains_key(&point_id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "VERTEX_POINT #{vertex_id} has unresolved point carrier #{point_id}"
                )),
                "step_topology_losses",
            )?;
        }
    }
    let mut built_wire_models = BTreeSet::new();
    for (&representation, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        let Some(items) = representation_item_values(ctx, record)? else {
            continue;
        };
        for model in items.iter().filter_map(Value::reference) {
            if exchange
                .records()
                .get(&model)
                .map(|record| record.partial(ctx, "EDGE_BASED_WIREFRAME_MODEL"))
                .transpose()?
                .flatten()
                .is_none()
            {
                continue;
            }
            if built_wire_models.contains(&model) {
                ctx.insert_btree_set(&mut result.claims, representation, "step_topology_claims")?;
                if let Some(body_ids) = result.body_by_root.get(&model) {
                    let copies = ctx.collect_indexed_vec(
                        body_ids.len(),
                        "step_topology_root_bodies",
                        |index| {
                            body_ids[index].try_clone_for_decode(ctx, "step_topology_root_bodies")
                        },
                    )?;
                    ctx.insert_btree_map(
                        &mut result.body_by_root,
                        representation,
                        copies,
                        "step_topology_root_groups",
                    )?;
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
                if let Err(error) = commit_session.commit_model(built.draft)? {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning.note(topology_commit_error(
                            &format!("EDGE_BASED_WIREFRAME_MODEL #{model}"),
                            &error,
                            ctx,
                        )?),
                        "step_topology_losses",
                    )?;
                } else {
                    committed += 1;
                    ctx.insert_btree_set(&mut built_wire_models, model, "step_built_wire_models")?;
                    ctx.insert_btree_set(&mut built.typed, representation, "step_wire_typed")?;
                    push_topology_body_group(
                        &mut result.body_by_root,
                        model,
                        &built.body_id,
                        ctx,
                        "step_topology_root_groups",
                        "step_topology_root_bodies",
                    )?;
                    for typed in
                        ctx.admit_iter(std::mem::take(&mut built.typed), "step_topology_claims")?
                    {
                        ctx.insert_btree_set(&mut result.claims, typed, "step_topology_claims")?;
                    }
                }
            }
            if committed == 0 {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "EDGE_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
                    )),
                    "step_topology_losses",
                )?;
            } else if let Some(failures) = failures {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(ctx.format_retained(
                        format_args!(
                "EDGE_BASED_WIREFRAME_MODEL #{model} omitted {} unresolved connected edge set(s)",
                failures.count
            ),
                        "STEP decode text",
                    )?),
                    "step_topology_losses",
                )?;
            }
        }
    }
    for indexed_entity in exchange.entities(ctx, "SHELL_BASED_WIREFRAME_MODEL")? {
        let (model, record) = indexed_entity?;
        let scope_root = ctx.any_by(
            named_reference_values(ctx, record, "SHELL_BASED_WIREFRAME_MODEL", 1)?
                .unwrap_or_default(),
            |raw| {
                let Some(shell) = Value::reference(raw) else {
                    return Ok(false);
                };
                Ok(result.body_by_shell.contains_key(&shell))
            },
            "STEP shell wireframe reference traversal",
        )?;
        let outcome = build_shell_wire(
            model,
            exchange,
            (&vertices, &edges),
            point_positions,
            scope_root,
            &mut losses,
            ctx,
        )?;
        let (built, failures) = outcome.into_parts();
        let mut committed = 0;
        for mut built in built {
            if let Err(error) = commit_session.commit_model(built.draft)? {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(topology_commit_error(
                        &format!("SHELL_BASED_WIREFRAME_MODEL #{model}"),
                        &error,
                        ctx,
                    )?),
                    "step_topology_losses",
                )?;
            } else {
                committed += 1;
                for shell in ctx.admit_iter(&built.shell_sources, "STEP decode traversal")? {
                    insert_topology_body_group(
                        &mut result.body_by_shell,
                        *shell,
                        &built.body_id,
                        ctx,
                        "step_topology_shell_groups",
                        "step_topology_shell_bodies",
                    )?;
                }
                push_topology_body_group(
                    &mut result.body_by_root,
                    model,
                    &built.body_id,
                    ctx,
                    "step_topology_root_groups",
                    "step_topology_root_bodies",
                )?;
                for typed in
                    ctx.admit_iter(std::mem::take(&mut built.typed), "step_topology_claims")?
                {
                    ctx.insert_btree_set(&mut result.claims, typed, "step_topology_claims")?;
                }
            }
        }
        if committed == 0 {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "SHELL_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
                )),
                "step_topology_losses",
            )?;
        } else if let Some(failures) = failures {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "SHELL_BASED_WIREFRAME_MODEL #{model} omitted {} unresolved wire shell(s)",
                        failures.count
                    ),
                    "STEP decode text",
                )?),
                "step_topology_losses",
            )?;
        }
    }
    let mut decoded_pcurves = BTreeSet::new();
    for pcurve in ctx.admit_iter(
        &(commit_session.document().model.pcurves)[..],
        "STEP decode traversal",
    )? {
        if let Some(id) = source_numeric_id(ctx, pcurve.id.as_str(), "pcurve")? {
            ctx.insert_btree_set(&mut decoded_pcurves, id, "step_decoded_topology_pcurves")?;
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
    for entity in exchange.entities_any(ctx, &topology_root_types)? {
        let (_, record) = entity?;
        if let Some(key) = root_key(record, exchange, &shells, ctx)? {
            ctx.insert_btree_set(&mut distinct_roots, key, "step_distinct_topology_roots")?;
        }
    }
    let distinct_root_count = distinct_roots.len();
    let scope_distinct_roots = distinct_root_count > 1;
    let mut built_roots = BTreeMap::<RootKey, RootBuilt>::new();
    let mut representation_cache = BTreeMap::new();
    let mut admissions: Vec<PcurveAdmission> = Vec::new();
    for entity in exchange.entities_any(ctx, &topology_root_types)? {
        let (id, record) = entity?;
        let Some(key) = root_key(record, exchange, &shells, ctx)? else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                "STEP topology root #{id} does not resolve to a complete connected topology graph",
            )),
                "step_topology_losses",
            )?;
            continue;
        };
        if let Some(root_built) = built_roots.get(&key) {
            ctx.insert_btree_set(&mut result.claims, id, "step_topology_claims")?;
            let copies = ctx.collect_indexed_vec(
                root_built.body_ids.len(),
                "step_topology_root_bodies",
                |index| {
                    root_built.body_ids[index]
                        .try_clone_for_decode(ctx, "step_topology_root_bodies")
                },
            )?;
            ctx.insert_btree_map(
                &mut result.body_by_root,
                id,
                copies,
                "step_topology_root_groups",
            )?;
            for (&shell, body_ids) in
                ctx.admit_iter(&root_built.body_by_shell, "STEP decode traversal")?
            {
                for body in body_ids {
                    insert_topology_body_group(
                        &mut result.body_by_shell,
                        shell,
                        body,
                        ctx,
                        "step_topology_shell_groups",
                        "step_topology_shell_bodies",
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
            BuildSources {
                exchange,
                ir: commit_session.document(),
                vdefs: &vertices,
                edefs: &edges,
                odefs: &oriented,
                shell_definitions: &shells,
                decoded_pcurves: &decoded_pcurves,
                point_positions,
                ctx,
            },
            scope_root,
            &mut losses,
        )?;
        let (built, failures) = outcome.into_parts();
        let failure_message = failures
            .as_ref()
            .and_then(|failures| failures.first.as_ref())
            .map(BuildFailure::message);
        let mut body_ids = Vec::new();
        let mut body_by_shell = BTreeMap::<u64, BTreeSet<BodyId>>::new();
        for mut built in built {
            drop_committed_surfaces(&mut built.draft, &mut commit_session, ctx)?;
            if let Err(error) = commit_session.commit_model(built.draft)? {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(topology_commit_error(
                        &format!("STEP topology root #{id}"),
                        &error,
                        ctx,
                    )?),
                    "step_topology_losses",
                )?;
            } else {
                for shell in ctx.admit_iter(&built.shell_sources, "STEP decode traversal")? {
                    insert_topology_body_group(
                        &mut result.body_by_shell,
                        *shell,
                        &built.body_id,
                        ctx,
                        "step_topology_shell_groups",
                        "step_topology_shell_bodies",
                    )?;
                    insert_topology_body_group(
                        &mut body_by_shell,
                        *shell,
                        &built.body_id,
                        ctx,
                        "step_topology_built_shell_groups",
                        "step_topology_built_shell_bodies",
                    )?;
                }
                ctx.push_vec(
                    &mut body_ids,
                    built
                        .body_id
                        .try_clone_for_decode(ctx, "step_topology_built_bodies")?,
                    "step_topology_built_bodies",
                )?;
                for typed in
                    ctx.admit_iter(std::mem::take(&mut built.typed), "step_topology_claims")?
                {
                    ctx.insert_btree_set(&mut result.claims, typed, "step_topology_claims")?;
                }
                // A rejected draft transfers no relation, so only a committed
                // body contributes its admitted relations to the document.
                ctx.append_vec(
                    &mut admissions,
                    &mut built.pcurve_admissions,
                    "step_topology_admissions",
                )?;
            }
        }
        if body_ids.is_empty() {
            if let Some(message) = failure_message {
                ctx.push_vec(
                    &mut result.losses,
                    StepLossCode::TopologyRootRejected.note(ctx.format_retained(
                        format_args!("STEP topology root #{id} rejected: {message}"),
                        "step_topology_root_rejected_text",
                    )?),
                    "step_topology_losses",
                )?;
            } else {
                ctx.push_vec(&mut result.losses, StepLossCode::TopologyRootIncomplete.note(format!(
                        "STEP topology root #{id} does not resolve to a complete connected topology graph",
                    )), "step_topology_losses")?;
            }
        } else {
            let copies =
                ctx.collect_indexed_vec(body_ids.len(), "step_topology_root_bodies", |index| {
                    body_ids[index].try_clone_for_decode(ctx, "step_topology_root_bodies")
                })?;
            ctx.insert_btree_map(
                &mut result.body_by_root,
                id,
                copies,
                "step_topology_root_groups",
            )?;
            ctx.insert_btree_map(
                &mut built_roots,
                key,
                RootBuilt {
                    body_ids,
                    body_by_shell,
                },
                "step_topology_built_roots",
            )?;
            if let Some(failures) = failures {
                let detail = failure_message
                    .as_deref()
                    .map(|message| {
                        ctx.format_retained(
                            format_args!(": {message}"),
                            "step_topology_root_failure_detail",
                        )
                    })
                    .transpose()?
                    .unwrap_or_default();
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(ctx.format_retained(
                        format_args!(
                            "STEP topology root #{id} omitted {} unresolved shell(s){detail}",
                            failures.count,
                        ),
                        "STEP decode text",
                    )?),
                    "step_topology_losses",
                )?;
            }
        }
    }
    // Every admitted relation shares one class of unproved invariant, so the
    // document reports the class once with its count and named examples.
    if let Some(note) = pcurve_admission_note(&admissions, ctx)? {
        ctx.push_vec(&mut result.losses, note, "step_topology_losses")?;
    }
    for indexed_entity in
        exchange.entities(ctx, "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION")?
    {
        let (id, record) = indexed_entity?;
        let omitted = geometric_set_omissions(record, exchange, carrier_index, ctx)?;
        if !omitted.is_empty() {
            let note = geometric_set_omission_message(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION",
                id,
                &omitted,
                ctx,
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(note),
                "step_topology_losses",
            )?;
        }
        let Some(mut built) =
            build_geometric_set(id, record, exchange, carrier_index, &mut losses, ctx)?
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
            ctx.push_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no decoded bounded surfaces"
            )), "step_topology_losses")?;
            continue;
        };
        if let Err(error) = commit_session.commit_model(built.draft)? {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(topology_commit_error(
                    &format!("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id}"),
                    &error,
                    ctx,
                )?),
                "step_topology_losses",
            )?;
        } else {
            push_topology_body_group(
                &mut result.body_by_root,
                id,
                &built.body_id,
                ctx,
                "step_topology_root_groups",
                "step_topology_root_bodies",
            )?;
            for typed in ctx.admit_iter(std::mem::take(&mut built.typed), "step_topology_claims")? {
                ctx.insert_btree_set(&mut result.claims, typed, "step_topology_claims")?;
            }
        }
    }
    for entity in exchange.entities_any(
        ctx,
        &[
            "SHAPE_REPRESENTATION",
            "ADVANCED_BREP_SHAPE_REPRESENTATION",
            "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION",
        ],
    )? {
        let (id, record) = entity?;
        let Some(representation_type) = most_specific(
            ctx,
            record,
            &[
                "ADVANCED_BREP_SHAPE_REPRESENTATION",
                "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION",
                "SHAPE_REPRESENTATION",
            ],
        )?
        else {
            continue;
        };
        let omitted = geometric_set_omissions(record, exchange, carrier_index, ctx)?;
        if !omitted.is_empty() {
            let note = geometric_set_omission_message(representation_type, id, &omitted, ctx)?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(note),
                "step_topology_losses",
            )?;
        }
        mark_standalone_geometric_set(
            id,
            record,
            exchange,
            carrier_index,
            &mut result.claims,
            ctx,
        )?;
    }
    for entity in exchange.entities_any(
        ctx,
        &[
            "MANIFOLD_SURFACE_SHAPE_REPRESENTATION",
            "ADVANCED_BREP_REPRESENTATION",
            "ADVANCED_BREP_SHAPE_REPRESENTATION",
            "SHAPE_REPRESENTATION",
        ],
    )? {
        let (id, record) = entity?;
        if most_specific(
            ctx,
            record,
            &[
                "MANIFOLD_SURFACE_SHAPE_REPRESENTATION",
                "ADVANCED_BREP_REPRESENTATION",
                "ADVANCED_BREP_SHAPE_REPRESENTATION",
                "SHAPE_REPRESENTATION",
            ],
        )?
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
            ctx,
        )?
        .is_empty();
        if has_body {
            ctx.insert_btree_set(&mut result.claims, id, "step_topology_claims")?;
        }
    }
    for face in ctx.admit_iter(
        &(commit_session.document().model.faces)[..],
        "STEP decode traversal",
    )? {
        if let Some(source) = source_numeric_id(ctx, face.id.as_str(), "face")? {
            ctx.push_btree_group(
                &mut result.faces_by_source,
                source,
                face.id
                    .try_clone_for_decode(ctx, "step_topology_source_faces")?,
                "step_topology_source_face_groups",
                "step_topology_source_faces",
            )?;
        }
    }
    for edge in ctx.admit_iter(
        &(commit_session.document().model.edges)[..],
        "STEP decode traversal",
    )? {
        if let Some(source) = source_numeric_id(ctx, edge.id.as_str(), "edge")? {
            ctx.push_btree_group(
                &mut result.edges_by_source,
                source,
                edge.id
                    .try_clone_for_decode(ctx, "step_topology_source_edges")?,
                "step_topology_source_edge_groups",
                "step_topology_source_edges",
            )?;
        }
    }
    for vertex in ctx.admit_iter(
        &(commit_session.document().model.vertices)[..],
        "STEP decode traversal",
    )? {
        if let Some(source) = source_numeric_id(ctx, vertex.id.as_str(), "vertex")? {
            ctx.push_btree_group(
                &mut result.vertices_by_source,
                source,
                vertex
                    .id
                    .try_clone_for_decode(ctx, "step_topology_source_vertices")?,
                "step_topology_source_vertex_groups",
                "step_topology_source_vertices",
            )?;
        }
    }
    ctx.append_vec(&mut result.losses, &mut losses, "step_topology_loss_merge")?;
    Ok(result)
}

fn geometric_set_omissions(
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<u64>, CodecError> {
    let Some(set_ids) = representation_item_values(ctx, representation)? else {
        return Ok(Vec::new());
    };
    let mut omitted = Vec::new();
    for set_id in set_ids.iter().filter_map(Value::reference) {
        let Some(set) = exchange.records().get(&set_id) else {
            continue;
        };
        let Some(set_type) = most_specific(ctx, set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])?
        else {
            continue;
        };
        let Some(members) = named_reference_values(ctx, set, set_type, 1)? else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            if !carrier_index.points.contains_key(&member)
                && !carrier_index.curves.contains_key(&member)
                && !carrier_index.surfaces.contains_key(&member)
            {
                ctx.push_vec(&mut omitted, member, "step_geometric_set_omissions")?;
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
    ctx.format_retained(
        format_args!(
            "{representation_type} #{id} omitted unsupported or unresolved member(s): {}",
            OmittedMembers(omitted)
        ),
        "step_geometric_set_omission_text",
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
                ctx.push_vec(built, value, "step_topology_built_outcome")
            }
        }
    }

    fn fail(&mut self, failure: Option<BuildFailure>) -> Result<(), CodecError> {
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
                failures.count = failures.count.checked_add(1).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "step topology failures",
                        u64::MAX,
                        u64::MAX,
                    )
                })?;
                if failures.first.is_none() {
                    failures.first = failure;
                }
            }
        }
        Ok(())
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
    let Some(sets) = named_reference_values(ctx, model, "EDGE_BASED_WIREFRAME_MODEL", 1)? else {
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
            WireSources {
                vdefs,
                edefs,
                point_positions,
            },
            scoped,
            losses,
            ctx,
        ) {
            Ok(Some(value)) => outcome.push(value, ctx)?,
            Ok(None) => outcome.fail(None)?,
            Err(error) => return Err(error),
        }
    }
    Ok(outcome)
}

#[derive(Clone, Copy)]
struct WireSources<'a> {
    vdefs: &'a BTreeMap<u64, VertexDef>,
    edefs: &'a BTreeMap<u64, Rc<EdgeDef>>,
    point_positions: &'a CarrierIndex,
}

#[derive(Clone, Copy)]
struct WireScope {
    scoped: bool,
    root: bool,
}

fn build_wire_set(
    id: u64,
    set_id: u64,
    exchange: &Exchange,
    sources: WireSources<'_>,
    scoped: bool,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Built>, CodecError> {
    let WireSources {
        vdefs,
        edefs,
        point_positions,
    } = sources;
    let Some(set) = exchange.records().get(&set_id) else {
        return Ok(None);
    };
    let Some(set_type) =
        most_specific(ctx, set, &["CONNECTED_EDGE_SUB_SET", "CONNECTED_EDGE_SET"])?
    else {
        return Ok(None);
    };
    let Some(used_edges) = connected_set_members(ctx, set, set_type)? else {
        return Ok(None);
    };
    let suffix = if scoped {
        IdentityKeyTail::empty().dash(key_word!("set")).dash(set_id)
    } else {
        IdentityKeyTail::empty()
    };
    let mut typed = BTreeSet::new();
    ctx.insert_btree_set(&mut typed, id, "step_wire_typed")?;
    ctx.insert_btree_set(&mut typed, set_id, "step_wire_typed")?;
    if set_type == "CONNECTED_EDGE_SUB_SET"
        && !validate_subset_parent(set_id, set, set_type, exchange, losses, ctx)?
    {
        ctx.remove_btree_set(&mut typed, &set_id, "step_wire_typed")?;
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
        ctx.push_vec(
            &mut wire_edges,
            ir_id.try_clone_for_decode(ctx, "step_wire_edge_ids")?,
            "step_wire_edge_ids",
        )?;
        ctx.push_vec(
            &mut built_edges,
            Edge {
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
            },
            "step_wire_edges",
        )?;
        ctx.insert_btree_set(&mut used_vertices, start, "step_wire_used_vertices")?;
        ctx.insert_btree_set(&mut used_vertices, end, "step_wire_used_vertices")?;
        ctx.insert_btree_set(&mut typed, edge_id, "step_wire_typed")?;
        if let Some(parent) = edge.parent() {
            ctx.insert_btree_set(&mut typed, parent, "step_wire_typed")?;
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
        ctx.push_vec(
            &mut built_vertices,
            Vertex {
                id: VertexId::from(ids::data(
                    kind!("vertex"),
                    IdentityKey::from(vertex_id).with_tail(&vertex_suffix),
                )),
                point: PointId::from(ids::data(kind!("point"), vertex.point)),
                tolerance: None,
            },
            "step_wire_vertices",
        )?;
        ctx.insert_btree_set(&mut typed, vertex_id, "step_wire_typed")?;
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
        shell.try_clone_for_decode(ctx, "step_wire_shell_id_copy")?,
        region.try_clone_for_decode(ctx, "step_wire_region_id_copy")?,
        Vec::new(),
        wire_edges,
        Vec::new(),
    ) {
        Ok(shell) => shell,
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("CONNECTED_EDGE_SET #{set_id}: {error}"),
                    "STEP build_wire_set text",
                )?),
                "step_topology_losses",
            )?;
            return Ok(None);
        }
    };
    let staged = staged_topology(
        StagedTopologyParts {
            typed,
            vertices: built_vertices,
            edges: built_edges,
            coedges: Vec::new(),
            loops: Vec::new(),
            faces: Vec::new(),
            surfaces: Vec::new(),
            shells: ctx.collect_vec([shell_value], "step_wire_shells")?,
            region: Region {
                id: region.try_clone_for_decode(ctx, "step_wire_region_id_copy")?,
                body: body.try_clone_for_decode(ctx, "step_wire_body_id_copy")?,
                shells: ctx.collect_vec([shell], "step_wire_region_shells")?,
            },
            body: Body {
                id: body.try_clone_for_decode(ctx, "step_wire_body_id_copy")?,
                kind: BodyKind::Wire,
                regions: ctx.collect_vec([region], "step_wire_body_regions")?,
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        },
        ctx,
    );
    let mut built = match staged {
        Ok(built) => built,
        Err(StageError::Draft(error)) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("CONNECTED_EDGE_SET #{set_id}: {error}"),
                    "STEP build_wire_set text",
                )?),
                "step_topology_losses",
            )?;
            return Ok(None);
        }
        Err(StageError::Resource(error)) => return Err(error),
    };
    ctx.insert_btree_set(&mut built.shell_sources, set_id, "step_wire_shell_sources")?;
    Ok(Some(built))
}

fn build_shell_wire(
    id: u64,
    exchange: &Exchange,
    (vdefs, edefs): (&BTreeMap<u64, VertexDef>, &BTreeMap<u64, Rc<EdgeDef>>),
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
    let Some(shell_ids) = named_reference_values(ctx, model, "SHELL_BASED_WIREFRAME_MODEL", 1)?
    else {
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
            WireSources {
                vdefs,
                edefs,
                point_positions,
            },
            WireScope {
                scoped,
                root: scope_root,
            },
            losses,
            ctx,
        ) {
            Ok(Some(value)) => outcome.push(value, ctx)?,
            Ok(None) => outcome.fail(None)?,
            Err(error) => return Err(error),
        }
    }
    Ok(outcome)
}

fn build_shell_wire_set(
    id: u64,
    shell_id: u64,
    exchange: &Exchange,
    sources: WireSources<'_>,
    scope: WireScope,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Built>, CodecError> {
    let WireSources {
        vdefs,
        edefs,
        point_positions,
    } = sources;
    let WireScope {
        scoped,
        root: scope_root,
    } = scope;
    let Some(shell_record) = exchange.records().get(&shell_id) else {
        return Ok(None);
    };
    let mut typed = BTreeSet::new();
    ctx.insert_btree_set(&mut typed, id, "step_wire_typed")?;
    ctx.insert_btree_set(&mut typed, shell_id, "step_wire_typed")?;
    let mut edge_uses = Vec::new();
    let mut used_vertices = BTreeSet::new();
    let mut free_vertices = BTreeSet::new();
    if shell_record.partial(ctx, "WIRE_SHELL")?.is_some() {
        let Some(loop_ids) = named_reference_values(ctx, shell_record, "WIRE_SHELL", 1)? else {
            return Ok(None);
        };
        for loop_id in loop_ids.iter().filter_map(Value::reference) {
            let Some(loop_record) = exchange.records().get(&loop_id) else {
                return Ok(None);
            };
            if loop_record.partial(ctx, "EDGE_LOOP")?.is_some() {
                let Some(oriented_ids) = named_reference_values(ctx, loop_record, "EDGE_LOOP", 1)?
                else {
                    return Ok(None);
                };
                for oriented_id in oriented_ids.iter().filter_map(Value::reference) {
                    let Some(oriented) = exchange.records().get(&oriented_id) else {
                        return Ok(None);
                    };
                    let Some(edge_id) = oriented_edge_reference(ctx, oriented)? else {
                        return Ok(None);
                    };
                    let Some(edge) = edefs.get(&edge_id) else {
                        return Ok(None);
                    };
                    let Some(forward) = oriented_edge_forward(ctx, oriented)? else {
                        return Ok(None);
                    };
                    ctx.push_vec(
                        &mut edge_uses,
                        (edge_id, oriented_id, forward),
                        "step_wire_edge_uses",
                    )?;
                    ctx.insert_btree_set(
                        &mut used_vertices,
                        edge.vertices().0,
                        "step_wire_used_vertices",
                    )?;
                    ctx.insert_btree_set(
                        &mut used_vertices,
                        edge.vertices().1,
                        "step_wire_used_vertices",
                    )?;
                    for claim in [loop_id, oriented_id, edge_id] {
                        ctx.insert_btree_set(&mut typed, claim, "step_wire_typed")?;
                    }
                    if let Some(parent) = edge.parent() {
                        ctx.insert_btree_set(&mut typed, parent, "step_wire_typed")?;
                    }
                }
            } else if loop_record.partial(ctx, "VERTEX_LOOP")?.is_some() {
                let Some(vertex) = named_reference(ctx, loop_record, "VERTEX_LOOP", 1, 0)? else {
                    return Ok(None);
                };
                ctx.insert_btree_set(&mut used_vertices, vertex, "step_wire_used_vertices")?;
                ctx.insert_btree_set(&mut free_vertices, vertex, "step_wire_free_vertices")?;
                for claim in [loop_id, vertex] {
                    ctx.insert_btree_set(&mut typed, claim, "step_wire_typed")?;
                }
            } else {
                return Ok(None);
            }
        }
    } else if shell_record.partial(ctx, "VERTEX_SHELL")?.is_some() {
        let Some(loop_id) = named_reference(ctx, shell_record, "VERTEX_SHELL", 1, 0)? else {
            return Ok(None);
        };
        let Some(loop_record) = exchange.records().get(&loop_id) else {
            return Ok(None);
        };
        if loop_record.partial(ctx, "VERTEX_LOOP")?.is_none() {
            return Ok(None);
        }
        let Some(vertex) = named_reference(ctx, loop_record, "VERTEX_LOOP", 1, 0)? else {
            return Ok(None);
        };
        ctx.insert_btree_set(&mut used_vertices, vertex, "step_wire_used_vertices")?;
        ctx.insert_btree_set(&mut free_vertices, vertex, "step_wire_free_vertices")?;
        for claim in [loop_id, vertex] {
            ctx.insert_btree_set(&mut typed, claim, "step_wire_typed")?;
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
        ctx.push_vec(
            &mut wire_edges,
            ir_id.try_clone_for_decode(ctx, "step_wire_edge_ids")?,
            "step_wire_edge_ids",
        )?;
        ctx.push_vec(
            &mut edges,
            Edge {
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
            },
            "step_wire_edges",
        )?;
    }
    let mut vertices = Vec::new();
    for vertex_id in used_vertices {
        let Some(vertex) = vdefs.get(&vertex_id) else {
            return Ok(None);
        };
        if point_positions.get(vertex.point).is_none() {
            return Ok(None);
        }
        ctx.push_vec(
            &mut vertices,
            Vertex {
                id: VertexId::from(ids::data(
                    kind!("vertex"),
                    IdentityKey::from(vertex_id).with_tail(&vertex_suffix),
                )),
                point: PointId::from(ids::data(kind!("point"), vertex.point)),
                tolerance: None,
            },
            "step_wire_vertices",
        )?;
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
        ctx.push_vec(
            &mut free_vertex_ids,
            VertexId::from(ids::data(
                kind!("vertex"),
                IdentityKey::from(vertex).with_tail(&vertex_suffix),
            )),
            "step_wire_free_vertex_ids",
        )?;
    }
    let shell_value = match Shell::new(
        shell.try_clone_for_decode(ctx, "step_wire_shell_id_copy")?,
        region.try_clone_for_decode(ctx, "step_wire_region_id_copy")?,
        Vec::new(),
        wire_edges,
        free_vertex_ids,
    ) {
        Ok(shell) => shell,
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("wire shell #{shell_id}: {error}"),
                    "STEP build_shell_wire_set text",
                )?),
                "step_topology_losses",
            )?;
            return Ok(None);
        }
    };
    let staged = staged_topology(
        StagedTopologyParts {
            typed,
            vertices,
            edges,
            coedges: Vec::new(),
            loops: Vec::new(),
            faces: Vec::new(),
            surfaces: Vec::new(),
            shells: ctx.collect_vec([shell_value], "step_wire_shells")?,
            region: Region {
                id: region.try_clone_for_decode(ctx, "step_wire_region_id_copy")?,
                body: body.try_clone_for_decode(ctx, "step_wire_body_id_copy")?,
                shells: ctx.collect_vec([shell], "step_wire_region_shells")?,
            },
            body: Body {
                id: body.try_clone_for_decode(ctx, "step_wire_body_id_copy")?,
                kind: BodyKind::Wire,
                regions: ctx.collect_vec([region], "step_wire_body_regions")?,
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        },
        ctx,
    );
    let mut built = match staged {
        Ok(built) => built,
        Err(StageError::Draft(error)) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("wire shell #{shell_id}: {error}"),
                    "STEP build_shell_wire_set text",
                )?),
                "step_topology_losses",
            )?;
            return Ok(None);
        }
        Err(StageError::Resource(error)) => return Err(error),
    };
    ctx.insert_btree_set(
        &mut built.shell_sources,
        shell_id,
        "step_wire_shell_sources",
    )?;
    Ok(Some(built))
}

fn mark_standalone_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    typed: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(set_ids) = representation_item_values(ctx, representation)? else {
        return Ok(false);
    };
    let mut decoded = false;
    for set_id in set_ids.iter().filter_map(ValueExt::reference) {
        let Some(set) = exchange.records().get(&set_id) else {
            continue;
        };
        let Some(set_type) = most_specific(ctx, set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])?
        else {
            continue;
        };
        let Some(items) = named_reference_values(ctx, set, set_type, 1)? else {
            continue;
        };
        let has_decoded_member = items.iter().filter_map(ValueExt::reference).any(|item| {
            carrier_index.points.contains_key(&item)
                || carrier_index.curves.contains_key(&item)
                || carrier_index.surfaces.contains_key(&item)
        });
        if has_decoded_member {
            ctx.insert_btree_set(typed, set_id, "step_topology_claims")?;
            decoded = true;
        }
    }
    if decoded {
        ctx.insert_btree_set(typed, id, "step_topology_claims")?;
    }
    Ok(decoded)
}

fn build_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Built>, CodecError> {
    let Some(set_ids) = representation_item_values(ctx, representation)? else {
        ctx.push_vec(
            losses,
            StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no item list"
            )),
            "step_topology_losses",
        )?;
        return Ok(None);
    };
    let mut typed = BTreeSet::new();
    ctx.insert_btree_set(&mut typed, id, "step_geometric_set_typed")?;
    let body = BodyId::from(ids::data(kind!("body"), id));
    let region = RegionId::from(ids::data(kind!("region"), id));
    let shell_id = ShellId::from(ids::data(
        kind!("shell"),
        key_word!("geometric").dash(key_word!("set")).dash(id),
    ));
    let mut shell_faces = Vec::new();
    let mut faces = Vec::new();
    for set_id in set_ids.iter().filter_map(ValueExt::reference) {
        let Some(set) = exchange.records().get(&set_id) else {
            ctx.push_vec(losses, StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped missing set #{set_id}"
            )), "step_topology_losses")?;
            continue;
        };
        let Some(set_type) = most_specific(ctx, set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])?
        else {
            ctx.push_vec(losses, StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped non-set member #{set_id}"
            )), "step_topology_losses")?;
            continue;
        };
        let Some(items) = named_reference_values(ctx, set, set_type, 1)? else {
            ctx.push_vec(losses, StepLossCode::DecodeWarning.note(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped set #{set_id} with no member list"
            )), "step_topology_losses")?;
            continue;
        };
        ctx.insert_btree_set(&mut typed, set_id, "step_geometric_set_typed")?;
        for surface_step in items.iter().filter_map(ValueExt::reference) {
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
                    shell: shell_id.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                    surface,
                    sense: Sense::Forward,
                    loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
                    name: None,
                    color: None,
                    tolerance: None,
                };
                ctx.push_vec(
                    &mut shell_faces,
                    face.id
                        .try_clone_for_decode(ctx, "step_geometric_set_shell_faces")?,
                    "step_geometric_set_shell_faces",
                )?;
                ctx.push_vec(&mut faces, face, "step_geometric_set_faces")?;
            }
        }
    }
    if shell_faces.is_empty() {
        ctx.push_vec(losses, StepLossCode::DecodeWarning.note(format!(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no indexed surface member; set dropped"
        )), "step_topology_losses")?;
        return Ok(None);
    }
    let shell = Shell::new(
        shell_id.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
        region.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
        shell_faces,
        Vec::new(),
        Vec::new(),
    )
    .map_err(CodecError::malformed)?;
    let staged = staged_topology(
        StagedTopologyParts {
            typed,
            vertices: Vec::new(),
            edges: Vec::new(),
            coedges: Vec::new(),
            loops: Vec::new(),
            faces,
            surfaces: Vec::new(),
            shells: ctx.collect_vec([shell], "step_geometric_set_shells")?,
            region: Region {
                id: region.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                body: body.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                shells: ctx.collect_vec([shell_id], "step_geometric_set_region_shells")?,
            },
            body: Body {
                id: body,
                kind: BodyKind::Sheet,
                regions: ctx.collect_vec([region], "step_geometric_set_body_regions")?,
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        },
        ctx,
    );
    match staged {
        Ok(built) => Ok(Some(built)),
        Err(StageError::Draft(error)) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id}: {error}"
                    ),
                    "STEP build_geometric_set text",
                )?),
                "step_topology_losses",
            )?;
            Ok(None)
        }
        Err(StageError::Resource(error)) => Err(error),
    }
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
    for indexed_entity in exchange.entities(ctx, "VERTEX_POINT")? {
        let (id, record) = indexed_entity?;
        let Some(point) = named_reference(ctx, record, "VERTEX_POINT", 1, 0)? else {
            continue;
        };
        ctx.insert_btree_map(
            &mut vertices,
            id,
            VertexDef { point },
            "step_vertex_definitions",
        )?;
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
    for entity in exchange.entities_any(
        ctx,
        &[
            "EDGE_CURVE",
            "SEAM_EDGE",
            "ORIENTED_EDGE",
            "SUBEDGE",
            "EDGE",
        ],
    )? {
        let (id, _) = entity?;
        if let Some(edge) = edge_def_for(id, exchange, &mut active, &mut cache, ctx)? {
            ctx.insert_btree_map(&mut edges, id, edge, "step_edge_definitions")?;
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
    ctx.insert_btree_set(active, id, "step_edge_definition_active")?;
    let result = if let Some(record) = exchange.records().get(&id) {
        match most_specific(
            ctx,
            record,
            &[
                "EDGE_CURVE",
                "SEAM_EDGE",
                "ORIENTED_EDGE",
                "SUBEDGE",
                "EDGE",
            ],
        )? {
            Some("EDGE_CURVE") => edge_vertices(ctx, record)?
                .zip(edge_geometry(ctx, record)?)
                .zip(edge_same_sense(ctx, record)?)
                .map(|(((start, end), curve), same)| EdgeDef::Curve {
                    start,
                    end,
                    curve,
                    same,
                }),
            Some("EDGE") => {
                edge_vertices(ctx, record)?.map(|(start, end)| EdgeDef::Bare { start, end })
            }
            Some("SUBEDGE") => {
                if let Some(((start, end), parent)) =
                    edge_vertices(ctx, record)?.zip(subedge_parent(ctx, record)?)
                {
                    edge_def_for(parent, exchange, active, cache, ctx)?.map(|basis| {
                        EdgeDef::Subedge {
                            start,
                            end,
                            parent,
                            basis,
                        }
                    })
                } else {
                    None
                }
            }
            Some("ORIENTED_EDGE" | "SEAM_EDGE") => {
                if let Some((element, forward)) =
                    oriented_edge_reference(ctx, record)?.zip(oriented_edge_forward(ctx, record)?)
                {
                    edge_def_for(element, exchange, active, cache, ctx)?.map(|basis| {
                        EdgeDef::Oriented {
                            element,
                            basis,
                            forward,
                        }
                    })
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
    ctx.insert_btree_map(
        &mut *cache,
        id,
        result.clone(),
        "step_edge_definition_cache",
    )?;
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
        ctx.push_vec(
            losses,
            StepLossCode::DecodeWarning.note(format!(
                "STEP edge #{edge_id} has no 3D curve carrier; edge committed without a curve"
            )),
            "step_topology_losses",
        )?;
        return Ok(None);
    };
    let curve = exchange.records().get(&curve_step);
    let carrier = curve_carrier_record(ctx, curve_step, exchange)?;
    let unresolved_surface_curve = if carrier.is_none() {
        if let Some(record) = curve {
            ctx.any_by(
                &record.partials[..],
                |partial| {
                    Ok(matches!(
                        partial.name.as_str(),
                        "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                    ))
                },
                "STEP unresolved edge carrier partial traversal",
            )?
        } else {
            false
        }
    } else {
        false
    };
    if unresolved_surface_curve {
        ctx.push_vec(losses, StepLossCode::DecodeWarning.note(format!(
            "STEP edge curve #{edge_id}: surface-curve #{curve_step} has no resolvable basis; edge committed without a curve"
        )), "step_topology_losses")?;
    }
    Ok(carrier.map(|curve| CurveId::from(ids::data(kind!("curve"), curve))))
}
fn oriented_defs(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, OrientedDef>, CodecError> {
    let mut oriented = BTreeMap::new();
    for entity in exchange.entities_any(ctx, &["ORIENTED_EDGE", "SEAM_EDGE"])? {
        let (id, record) = entity?;
        let Some(edge) = oriented_edge_reference(ctx, record)? else {
            continue;
        };
        let Some(forward) = oriented_edge_forward(ctx, record)? else {
            continue;
        };
        let kind = if most_specific(ctx, record, &["SEAM_EDGE"])?.is_some() {
            OrientedKind::Seam {
                pcurve: record
                    .partial(ctx, "SEAM_EDGE")?
                    .map(|partial| {
                        ctx.find_map(
                            partial.parameters.as_slice().iter().rev(),
                            |value| Ok(ValueExt::reference(value)),
                            "STEP topology reference parameter traversal",
                        )
                    })
                    .transpose()?
                    .flatten(),
            }
        } else {
            OrientedKind::Plain
        };
        ctx.insert_btree_map(
            &mut oriented,
            id,
            OrientedDef {
                edge,
                forward,
                kind,
            },
            "step_oriented_edge_definitions",
        )?;
    }
    Ok(oriented)
}

fn subedge_parent(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<u64>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(entity_parameter(ctx, record, "SUBEDGE", 3)?.and_then(ValueExt::reference));
    }
    if let Some(partial) = record.partial(ctx, "SUBEDGE")? {
        if let Some(id) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP subedge parent reference traversal",
        )? {
            return Ok(Some(id));
        }
    }
    for partial in ctx
        .admit_iter(
            &record.partials[..],
            "STEP subedge parent partial traversal",
        )?
        .rev()
    {
        if let Some(id) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP subedge parent reference traversal",
        )? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn named_reference(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    name: &str,
    simple_index: usize,
    complex_index: usize,
) -> Result<Option<u64>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(entity_parameter(ctx, record, name, simple_index)?.and_then(ValueExt::reference));
    }
    let Some(partial) = ctx.find_map(
        &record.partials[..],
        |partial| -> Result<Option<_>, CodecError> {
            Ok(
                (ctx.equal(partial.name.as_str(), name, "STEP named reference equality")?)
                    .then_some(partial),
            )
        },
        "STEP named topology partial traversal",
    )?
    else {
        return Ok(None);
    };
    Ok(ctx
        .admit_iter(
            partial.parameters.as_slice(),
            "STEP named topology parameter traversal",
        )?
        .filter_map(ValueExt::reference)
        .nth(complex_index))
}

fn oriented_edge_reference(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(record.parameter(3).and_then(ValueExt::reference));
    }
    let Some(partial) = record.partial(ctx, "ORIENTED_EDGE")?.map_or_else(
        || record.partial(ctx, "SEAM_EDGE"),
        |partial| Ok(Some(partial)),
    )?
    else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(ValueExt::reference(value)),
        "STEP oriented edge reference parameter traversal",
    )
}

fn oriented_edge_forward(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<bool>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(record.parameter(4).and_then(ValueExt::logical));
    }
    let Some(partial) = record.partial(ctx, "ORIENTED_EDGE")?.map_or_else(
        || record.partial(ctx, "SEAM_EDGE"),
        |partial| Ok(Some(partial)),
    )?
    else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(ValueExt::logical(value)),
        "STEP oriented edge forward parameter traversal",
    )
}

fn named_logical(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    name: &str,
    simple_index: usize,
    _complex_index: usize,
) -> Result<Option<bool>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(entity_parameter(ctx, record, name, simple_index)?.and_then(ValueExt::logical));
    }
    let Some(partial) = ctx.find_map(
        &record.partials[..],
        |partial| -> Result<Option<_>, CodecError> {
            Ok(
                (ctx.equal(partial.name.as_str(), name, "STEP named logical equality")?)
                    .then_some(partial),
            )
        },
        "STEP named topology partial traversal",
    )?
    else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(ValueExt::logical(value)),
        "STEP named topology parameter traversal",
    )
}

fn surface_curve_pcurves<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<impl Iterator<Item = u64> + 'a, CodecError> {
    let mut values = if record.partials.len() == 1 {
        record.parameter(2).and_then(Value::list)
    } else {
        None
    };
    if record.partials.len() != 1 {
        if let Some(partial) = record
            .partial(ctx, "SURFACE_CURVE")?
            .map_or_else(
                || record.partial(ctx, "SEAM_CURVE"),
                |partial| Ok(Some(partial)),
            )?
            .map_or_else(
                || record.partial(ctx, "INTERSECTION_CURVE"),
                |partial| Ok(Some(partial)),
            )?
        {
            for value in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP surface curve pcurve parameter traversal",
            )? {
                if let Some(items) = reference_values(ctx, value)? {
                    values = Some(items);
                    break;
                }
            }
        }
    }
    let mut items = values.unwrap_or_default();
    if values.is_some()
        && !ctx.all_by(
            items,
            |value| Ok(value.reference().is_some()),
            "STEP surface curve pcurve reference validation",
        )?
    {
        items = &[];
    }
    Ok(ctx
        .admit_iter(items, "STEP surface curve pcurve reference traversal")?
        .filter_map(Value::reference))
}

fn edge_vertices(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<(u64, u64)>, CodecError> {
    if record.partials.len() == 1 {
        let Some(name) = record.simple_name() else {
            return Ok(None);
        };
        let start = entity_parameter(ctx, record, name, 1)?.and_then(ValueExt::reference);
        if start.is_none() {
            return Ok(None);
        }
        let end = entity_parameter(ctx, record, name, 2)?.and_then(ValueExt::reference);
        return Ok(start.zip(end));
    }
    let mut partial = record.partial(ctx, "EDGE")?;
    if partial.is_none() {
        partial = record.partial(ctx, "EDGE_CURVE")?;
    }
    let Some(partial) = partial else {
        return Ok(None);
    };
    let mut references = ctx
        .admit_iter(
            partial.parameters.as_slice(),
            "STEP edge vertex reference traversal",
        )?
        .filter_map(ValueExt::reference);
    Ok(references.next().zip(references.next()))
}

fn edge_geometry(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<u64>, CodecError> {
    if record.partials.len() == 1 {
        let Some(name) = record.simple_name() else {
            return Ok(None);
        };
        return Ok(entity_parameter(ctx, record, name, 3)?.and_then(ValueExt::reference));
    }
    let Some(partial) = record.partial(ctx, "EDGE_CURVE")? else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(ValueExt::reference(value)),
        "STEP edge geometry parameter traversal",
    )
}

fn edge_same_sense(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<bool>, CodecError> {
    if record.partials.len() == 1 {
        let Some(name) = record.simple_name() else {
            return Ok(None);
        };
        return Ok(entity_parameter(ctx, record, name, 4)?.and_then(ValueExt::logical));
    }
    let Some(partial) = record.partial(ctx, "EDGE_CURVE")? else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(ValueExt::logical(value)),
        "STEP edge same sense parameter traversal",
    )
}

struct Built {
    typed: BTreeSet<u64>,
    draft: ModelDraft,
    body_id: BodyId,
    shell_sources: BTreeSet<u64>,
    /// Pcurve relations that the finite witness admitted while this body was
    /// staged. They are located source facts; the document report formats them.
    pcurve_admissions: Vec<PcurveAdmission>,
}

fn drop_committed_surfaces(
    draft: &mut ModelDraft,
    session: &mut CommitSession<'_, &mut CadIr>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.retain_vec(
        &mut draft.model_mut().surfaces,
        |surface| Ok(!session.contains(surface.id.as_str())?),
        "filter committed surfaces",
    )
}

#[cfg(test)]
pub(crate) mod tests;

#[derive(Debug)]
enum StageError {
    Draft(DraftError),
    Resource(CodecError),
}

impl From<DraftError> for StageError {
    fn from(error: DraftError) -> Self {
        match error {
            DraftError::Resource(limit) => Self::Resource(CodecError::ResourceLimit(limit)),
            error => Self::Draft(error),
        }
    }
}

impl From<CodecError> for StageError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

struct StagedTopologyParts {
    typed: BTreeSet<u64>,
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    coedges: Vec<Coedge>,
    loops: Vec<Loop>,
    faces: Vec<Face>,
    surfaces: Vec<Surface>,
    shells: Vec<Shell>,
    region: Region,
    body: Body,
}

fn staged_topology(
    parts: StagedTopologyParts,
    ctx: &DecodeContext<'_>,
) -> Result<Built, StageError> {
    let StagedTopologyParts {
        typed,
        vertices,
        edges,
        coedges,
        loops,
        faces,
        surfaces,
        shells,
        region,
        body,
    } = parts;
    let mut draft = ModelDraft::new();
    for vertex in vertices {
        draft.insert(vertex, ctx)?;
    }
    for edge in edges {
        draft.insert(edge, ctx)?;
    }
    for coedge in coedges {
        draft.insert(coedge, ctx)?;
    }
    for loop_ in loops {
        draft.insert(loop_, ctx)?;
    }
    for face in faces {
        draft.insert(face, ctx)?;
    }
    let mut surface_ids = BTreeSet::new();
    for surface in surfaces {
        if !ctx.contains_btree_set(
            &surface_ids,
            surface.id.as_str(),
            "STEP surface ids membership",
        )? {
            let id =
                ctx.copy_retained(surface.id.as_str().as_bytes(), "step_staged_surface_ids")?;
            let id = String::from_utf8(id).map_err(CodecError::malformed)?;
            ctx.insert_btree_set(&mut surface_ids, id, "step_staged_surface_ids")?;
            draft.insert(surface, ctx)?;
        }
    }
    for shell in shells {
        draft.insert(shell, ctx)?;
    }
    draft.insert(region, ctx)?;
    let body_id = body
        .id
        .try_clone_for_decode(ctx, "step_topology_identity_copy")?;
    draft.insert(body, ctx)?;
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

impl cadmpeg_core::decode::cost::DecodeCost for RootKey {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(self.root_kind, &self.shell_keys),
            ctx,
            operation,
        )
    }
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
    if root.partial(ctx, "SHELL_BASED_SURFACE_MODEL")?.is_some() {
        let Some(values) = named_reference_values(ctx, root, "SHELL_BASED_SURFACE_MODEL", 1)?
        else {
            return Ok(None);
        };
        for reference in values.iter().filter_map(ValueExt::reference) {
            ctx.push_vec(&mut ids, reference, "step_root_shell_steps")?;
        }
        return Ok(Some(ids));
    }
    if root.partial(ctx, "FACE_BASED_SURFACE_MODEL")?.is_some() {
        let Some(values) = named_reference_values(ctx, root, "FACE_BASED_SURFACE_MODEL", 1)? else {
            return Ok(None);
        };
        for set_step in values.iter().filter_map(ValueExt::reference) {
            let Some(set) = exchange.records().get(&set_step) else {
                return Ok(None);
            };
            if connected_face_set_type(ctx, set)?.is_none() {
                return Ok(None);
            }
            ctx.push_vec(&mut ids, set_step, "step_root_shell_steps")?;
        }
        return Ok(Some(ids));
    }
    if (root.partial(ctx, "MANIFOLD_SOLID_BREP")?.is_some()
        || root.partial(ctx, "FACETED_BREP")?.is_some())
        && root.partial(ctx, "BREP_WITH_VOIDS")?.is_none()
    {
        let root_type = if root.partial(ctx, "MANIFOLD_SOLID_BREP")?.is_some() {
            "MANIFOLD_SOLID_BREP"
        } else {
            "FACETED_BREP"
        };
        let Some(shell) = named_reference(ctx, root, root_type, 1, 0)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut ids, shell, "step_root_shell_steps")?;
        return Ok(Some(ids));
    }
    if root.partial(ctx, "BREP_WITH_VOIDS")?.is_some() {
        let Some(outer) = named_reference(ctx, root, "MANIFOLD_SOLID_BREP", 1, 0)? else {
            return Ok(None);
        };
        let Some(values) = named_reference_values(ctx, root, "BREP_WITH_VOIDS", 2)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut ids, outer, "step_root_shell_steps")?;
        for reference in values.iter().filter_map(ValueExt::reference) {
            ctx.push_vec(&mut ids, reference, "step_root_shell_steps")?;
        }
        // `voids` is a STEP SET. CADIR keeps the outer shell at index zero
        // and canonicalizes the void suffix by resolved shell identity.
        ctx.sort_unstable_by(
            &mut ids[1..],
            |value| value,
            |left, right| {
                let key = |reference: &u64| {
                    shell_definitions
                        .get(reference)
                        .map_or((u64::MAX, true, *reference), |definition| {
                            (definition.base, definition.forward, *reference)
                        })
                };
                key(left).cmp(&key(right))
            },
            "step_root_shell_steps_sort",
        )?;
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
        ctx,
        root,
        &[
            "BREP_WITH_VOIDS",
            "FACETED_BREP",
            "MANIFOLD_SOLID_BREP",
            "FACE_BASED_SURFACE_MODEL",
            "SHELL_BASED_SURFACE_MODEL",
        ],
    )?
    else {
        return Ok(None);
    };
    let mut shell_keys = Vec::new();
    let mut resolved = 0;
    let Some(shell_steps) = root_shell_steps(root, exchange, shell_definitions, ctx)? else {
        return Ok(None);
    };
    for shell in shell_steps {
        let key = if root.partial(ctx, "FACE_BASED_SURFACE_MODEL")?.is_some() {
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
        ctx.push_vec(&mut shell_keys, key, "step_root_shell_keys")?;
    }
    if resolved == 0 {
        return Ok(None);
    }
    ctx.sort_unstable_by(
        &mut shell_keys,
        |value| value,
        Ord::cmp,
        "step_root_shell_keys_sort",
    )?;
    Ok(Some(RootKey {
        root_kind,
        shell_keys,
    }))
}

#[derive(Clone, Copy)]
struct BuildSources<'a, 'b> {
    exchange: &'a Exchange,
    ir: &'a CadIr,
    vdefs: &'a BTreeMap<u64, VertexDef>,
    edefs: &'a BTreeMap<u64, Rc<EdgeDef>>,
    odefs: &'a BTreeMap<u64, OrientedDef>,
    shell_definitions: &'a BTreeMap<u64, ShellDef>,
    decoded_pcurves: &'a BTreeSet<u64>,
    point_positions: &'a CarrierIndex,
    ctx: &'a DecodeContext<'b>,
}

struct BuildRoot<'a> {
    shell_steps: &'a [u64],
    bid: BodyId,
    rid: &'a RegionId,
}

#[derive(Clone, Copy)]
struct BuildScope {
    faces: bool,
    edges: bool,
    root: bool,
}

fn build(
    id: u64,
    root: &RawRecord,
    sources: BuildSources<'_, '_>,
    scope_root: bool,
    losses: &mut Vec<LossNote>,
) -> Result<BuildOutcome, CodecError> {
    let BuildSources {
        exchange,
        shell_definitions,
        ctx,
        ..
    } = sources;
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
    let solid = root.partial(ctx, "MANIFOLD_SOLID_BREP")?.is_some()
        || root.partial(ctx, "BREP_WITH_VOIDS")?.is_some()
        || root.partial(ctx, "FACETED_BREP")?.is_some();
    if solid {
        let body = BodyId::from(ids::data(kind!("body"), id));
        let region = RegionId::from(ids::data(kind!("region"), id));
        let mut failure = None;
        let scope_shell_carriers = shell_steps.len() > 1 || scope_root;
        let built = build_one(
            id,
            root,
            sources,
            BuildRoot {
                shell_steps: &shell_steps,
                bid: body,
                rid: &region,
            },
            BuildScope {
                faces: scope_shell_carriers,
                edges: scope_shell_carriers,
                root: scope_root,
            },
            losses,
            &mut failure,
        );
        return Ok(match built {
            Ok(built) => {
                BuildOutcome::Built(ctx.collect_vec([built], "step_topology_built_outcome")?)
            }
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
        let shell_step = if root.partial(ctx, "FACE_BASED_SURFACE_MODEL")?.is_some() {
            shell_reference
        } else {
            match shell_definitions.get(&shell_reference) {
                Some(definition) => definition.base,
                None => {
                    outcome.fail(Some(BuildFailure {
                        record_id: shell_reference,
                        carrier_kind: CarrierKind::ShellCarrier,
                    }))?;
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
            sources,
            BuildRoot {
                shell_steps: &[shell_reference],
                bid: body,
                rid: &region,
            },
            BuildScope {
                faces: scoped || scope_root,
                edges: scoped || scope_root,
                root: scope_root,
            },
            losses,
            &mut failure,
        ) {
            Ok(value) => outcome.push(value, ctx)?,
            Err(BuildError::Absent) => outcome.fail(failure)?,
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

fn build_one(
    id: u64,
    root: &RawRecord,
    sources: BuildSources<'_, '_>,
    root_parts: BuildRoot<'_>,
    scope: BuildScope,
    losses: &mut Vec<LossNote>,
    failure: &mut Option<BuildFailure>,
) -> Result<Built, BuildError> {
    let BuildSources {
        exchange,
        ir,
        vdefs,
        edefs,
        odefs,
        shell_definitions,
        decoded_pcurves,
        point_positions,
        ctx,
    } = sources;
    let BuildRoot {
        shell_steps,
        bid,
        rid,
    } = root_parts;
    let BuildScope {
        faces: scope_faces,
        edges: scope_edges,
        root: scope_root,
    } = scope;
    let solid = root.partial(ctx, "MANIFOLD_SOLID_BREP")?.is_some()
        || root.partial(ctx, "BREP_WITH_VOIDS")?.is_some()
        || root.partial(ctx, "FACETED_BREP")?.is_some();
    let mut typed = BTreeSet::new();
    ctx.insert_btree_set(&mut typed, id, "step_brep_typed")?;
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    let mut coedges = Vec::new();
    let mut loops = Vec::new();
    let mut faces = Vec::new();
    let mut surfaces = Vec::new();
    let mut shells = Vec::new();
    let mut region = Region {
        id: rid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
        body: bid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
        shells: Vec::new(),
    };
    let body = Body {
        id: bid,
        kind: if solid {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        },
        regions: ctx.collect_vec(
            [rid.try_clone_for_decode(ctx, "step_topology_identity_copy")?],
            "step_brep_body_regions",
        )?,
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
    let mut surface_index_storage =
        ctx.reserve_scoped(0, "STEP temporary implicit surface identities")?;
    let mut implicit_surface_ids = BTreeSet::new();
    let mut admissions = Vec::new();
    for &shell_reference in ctx
        .admit_iter(shell_steps, "STEP body topology traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let (shell_step, shell_forward) =
            if root.partial(ctx, "FACE_BASED_SURFACE_MODEL")?.is_some() {
                ctx.insert_btree_set(&mut typed, shell_reference, "step_brep_typed")?;
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
        if used_shells.contains(&shell_step) {
            continue;
        }
        ctx.insert_btree_set(&mut used_shells, shell_step, "step_brep_used_shells")?;
        let sr = require_carrier(
            exchange.records().get(&shell_step),
            failure,
            shell_step,
            CarrierKind::ShellRecord,
        )
        .ok_or(BuildError::Absent)?;
        let (shell_type, face_steps) = if root.partial(ctx, "FACE_BASED_SURFACE_MODEL")?.is_some() {
            let set_type = require_carrier(
                connected_face_set_type(ctx, sr)?,
                failure,
                shell_step,
                CarrierKind::ConnectedFaceSet,
            )
            .ok_or(BuildError::Absent)?;
            if set_type == "CONNECTED_FACE_SUB_SET"
                && !validate_subset_parent(shell_step, sr, set_type, exchange, losses, ctx)?
            {
                ctx.remove_btree_set(&mut typed, &shell_step, "step_brep_typed")?;
            }
            let members = require_carrier(
                connected_set_members(ctx, sr, set_type)?,
                failure,
                shell_step,
                CarrierKind::ConnectedFaceSetMemberList,
            )
            .ok_or(BuildError::Absent)?;
            (set_type, members)
        } else {
            let shell_type = require_carrier(
                most_specific(ctx, sr, &["OPEN_SHELL", "CLOSED_SHELL"])?,
                failure,
                shell_step,
                CarrierKind::ShellType,
            )
            .ok_or(BuildError::Absent)?;
            let members = require_carrier(
                named_reference_values(ctx, sr, shell_type, 1)?,
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
        for face_step in ctx
            .admit_iter(&face_steps[..], "STEP body topology traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .filter_map(Value::reference)
        {
            if used_faces.contains(&(shell_step, face_step)) {
                continue;
            }
            ctx.insert_btree_set(
                &mut used_faces,
                (shell_step, face_step),
                "step_brep_used_faces",
            )?;
            let fr = require_carrier(
                exchange.records().get(&face_step),
                failure,
                face_step,
                CarrierKind::FaceRecord,
            )
            .ok_or(BuildError::Absent)?;
            if !is_face_record(ctx, fr)? {
                note_failure(failure, face_step, CarrierKind::FaceCarrier);
                return Err(BuildError::Absent);
            }
            let face_info = require_carrier(
                face_attributes(face_step, fr, exchange, &mut BTreeSet::new(), ctx)?,
                failure,
                face_step,
                CarrierKind::FaceAttributes,
            )
            .ok_or(BuildError::Absent)?;
            let mut outer_bound_count = 0usize;
            for bound_step in ctx
                .admit_iter(&face_info.bounds[..], "STEP body topology traversal")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                if exchange
                    .records()
                    .get(bound_step)
                    .map(|bound| bound.partial(ctx, "FACE_OUTER_BOUND"))
                    .transpose()?
                    .flatten()
                    .is_some()
                {
                    outer_bound_count = outer_bound_count
                        .checked_add(1)
                        .ok_or_else(|| CodecError::malformed("face outer bound count overflow"))?;
                }
            }
            if outer_bound_count > 1 {
                let note = StepLossCode::FaceMultipleOuterBounds.note(format!(
                    "face #{face_step} violates the STEP face-bound rule with {outer_bound_count} FACE_OUTER_BOUND loops; omitting the containing topology shell without assigning an outer role or deriving an implicit face carrier and retaining the source face, bounds, loops, and enclosing records as opaque"
                ));
                ctx.push_vec(
                    losses,
                    note.with_provenance(
                        cadmpeg_ir::SourceProvenance::root(
                            crate::dialect::FORMAT,
                            u64_from_index(fr.span.start),
                        )
                        .with_tag("face"),
                    ),
                    "step_topology_losses",
                )?;
                note_failure(failure, face_step, CarrierKind::FaceWithMultipleOuterBounds);
                return Err(BuildError::Absent);
            }
            for claim in face_info.typed {
                ctx.insert_btree_set(&mut typed, claim, "step_brep_typed")?;
            }
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
                if !ctx.contains_btree_set(
                    &implicit_surface_ids,
                    &surface_id,
                    "STEP implicit surface ids membership",
                )? {
                    ctx.insert_btree_set(
                        &mut implicit_surface_ids,
                        surface_index_storage.with_storage(|| {
                            surface_id.try_clone_for_decode(ctx, "step_topology_identity_copy")
                        })?,
                        "step_brep_implicit_surface_ids",
                    )?;
                    ctx.push_vec(
                        &mut surfaces,
                        Surface {
                            id: surface_id
                                .try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                            geometry: require_carrier(
                                implicit_face_plane(
                                    &face_info.bounds,
                                    exchange,
                                    vdefs,
                                    point_positions,
                                    ctx,
                                )?,
                                failure,
                                face_step,
                                CarrierKind::ImplicitFacePlane,
                            )
                            .ok_or(BuildError::Absent)?,
                            source_object: None,
                        },
                        "step_brep_surfaces",
                    )?;
                }
                surface_id
            };
            let surface_step = face_info.surface;
            let face_same_sense = face_info.same_sense;
            let fid = FaceId::from(ids::data(
                kind!("face"),
                IdentityKey::from(face_step).with_tail(&face_suffix),
            ));
            let name = face_info
                .name
                .as_ref()
                .map(|value| {
                    super::decode_text_charged(
                        exchange,
                        value,
                        losses,
                        face_step,
                        "face name",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten();
            let mut loop_ids = vec![];
            for bound_step in face_info.bounds {
                let br = require_carrier(
                    exchange.records().get(&bound_step),
                    failure,
                    bound_step,
                    CarrierKind::FaceBound,
                )
                .ok_or(BuildError::Absent)?;
                if br.partial(ctx, "FACE_BOUND")?.is_none()
                    && br.partial(ctx, "FACE_OUTER_BOUND")?.is_none()
                {
                    note_failure(failure, bound_step, CarrierKind::FaceBoundCarrier);
                    return Err(BuildError::Absent);
                }
                let is_outer_bound = br.partial(ctx, "FACE_OUTER_BOUND")?.is_some();
                let Some(bound_type) = face_bound_attribute_type(ctx, br)? else {
                    note_failure(failure, bound_step, CarrierKind::FaceBoundAttributes);
                    return Err(BuildError::Absent);
                };
                let loop_step = require_carrier(
                    named_reference(ctx, br, bound_type, 1, 0)?,
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
                if lr.partial(ctx, "VERTEX_LOOP")?.is_some() {
                    let vertex_step = require_carrier(
                        named_reference(ctx, lr, "VERTEX_LOOP", 1, 0)?,
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
                    ctx.push_vec(
                        &mut loops,
                        Loop {
                            id: lid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                            face: fid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
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
                        },
                        "step_brep_loops",
                    )?;
                    ctx.push_vec(&mut loop_ids, (is_outer_bound, lid), "step_brep_loop_ids")?;
                    ctx.insert_btree_set(
                        &mut used_v,
                        (shell_step, vertex_step),
                        "step_brep_used_vertices",
                    )?;
                    for claim in [bound_step, loop_step] {
                        ctx.insert_btree_set(&mut typed, claim, "step_brep_typed")?;
                    }
                    continue;
                }
                if lr.partial(ctx, "POLY_LOOP")?.is_some() {
                    let bound_forward = require_carrier(
                        named_logical(ctx, br, bound_type, 2, 0)?,
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
                    let point_values = require_carrier(
                        named_reference_values(ctx, lr, "POLY_LOOP", 1)?,
                        failure,
                        loop_step,
                        CarrierKind::PolyLoopPointList,
                    )
                    .ok_or(BuildError::Absent)?;
                    let mut points = Vec::new();
                    for point in ctx
                        .admit_iter(&point_values[..], "STEP body topology traversal")
                        .map_err(cadmpeg_core::CodecError::from)?
                        .filter_map(ValueExt::reference)
                    {
                        ctx.push_vec(&mut points, point, "step_brep_poly_loop_points")?;
                    }
                    if points.first() == points.last() {
                        points.pop();
                    }
                    points.dedup();
                    let mut distinct_points = BTreeSet::new();
                    for &point in ctx
                        .admit_iter(&points, "STEP body topology traversal")
                        .map_err(cadmpeg_core::CodecError::from)?
                    {
                        ctx.insert_btree_set(
                            &mut distinct_points,
                            point,
                            "step_brep_poly_loop_distinct_points",
                        )?;
                    }
                    if points.len() < 3
                        || distinct_points.len() != points.len()
                        || ctx.any_by(
                            &points[..],
                            |point| Ok(!point_positions.contains_key(*point)),
                            "STEP body topology traversal",
                        )?
                    {
                        note_failure(failure, loop_step, CarrierKind::PolyLoopPointCarrier);
                        return Err(BuildError::Absent);
                    }
                    if !bound_forward {
                        points.reverse();
                    }
                    let mut coedge_ids = Vec::new();
                    for (index, &start_point) in ctx
                        .admit_iter(&points[..], "STEP body topology traversal")
                        .map_err(cadmpeg_core::CodecError::from)?
                        .enumerate()
                    {
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
                        let mut lookup_storage =
                            ctx.reserve_scoped(0, "step poly edge lookup identity")?;
                        let lookup_edge = lookup_storage.with_storage(|| {
                            edge_id.try_clone_for_decode(ctx, "step poly edge lookup identity")
                        })?;
                        if !poly_edges.contains_key(&(shell_step, lookup_edge)) {
                            ctx.insert_btree_map(
                                &mut poly_edges,
                                (
                                    shell_step,
                                    edge_id
                                        .try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                                ),
                                (canonical_start, canonical_end),
                                "step_brep_poly_edges",
                            )?;
                        }
                        for point in [start_point, end_point] {
                            ctx.insert_btree_set(
                                &mut poly_points,
                                (shell_step, point),
                                "step_brep_poly_points",
                            )?;
                        }
                        let cid = CoedgeId::from(ids::data(
                            kind!("coedge"),
                            key_word!("poly")
                                .dash(loop_step)
                                .dash(index)
                                .dash(key_word!("face"))
                                .dash(face_step)
                                .with_tail(&face_suffix),
                        ));
                        ctx.push_vec(
                            &mut coedge_ids,
                            cid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                            "step_brep_coedge_ids",
                        )?;
                        ctx.push_vec(
                            &mut coedges,
                            Coedge {
                                id: cid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                                owner_loop: lid
                                    .try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                                edge: edge_id
                                    .try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                                radial_next: cid,
                                sense: if (canonical_start, canonical_end)
                                    == (start_point, end_point)
                                {
                                    Sense::Forward
                                } else {
                                    Sense::Reversed
                                },
                                pcurves: Vec::new(),
                                use_curve: None,
                            },
                            "step_brep_coedges",
                        )?;
                        ctx.push_btree_group(
                            &mut radial,
                            edge_id,
                            coedges.len() - 1,
                            "step_brep_radial_groups",
                            "step_brep_radial_members",
                        )?;
                        ctx.insert_btree_set(&mut typed, loop_step, "step_brep_typed")?;
                    }
                    let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(ctx, coedge_ids, Vec::new())
                        .map_err(cadmpeg_core::CodecError::from)?
                    else {
                        note_failure(failure, loop_step, CarrierKind::PolyLoopPointCarrier);
                        return Err(BuildError::Absent);
                    };
                    ctx.push_vec(
                        &mut loops,
                        Loop {
                            id: lid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                            face: fid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                        },
                        "step_brep_loops",
                    )?;
                    ctx.push_vec(&mut loop_ids, (is_outer_bound, lid), "step_brep_loop_ids")?;
                    ctx.insert_btree_set(&mut typed, bound_step, "step_brep_typed")?;
                    continue;
                }
                if lr.partial(ctx, "EDGE_LOOP")?.is_none() {
                    note_failure(failure, loop_step, CarrierKind::EdgeLoopCarrier);
                    return Err(BuildError::Absent);
                }
                let bound_forward = require_carrier(
                    named_logical(ctx, br, bound_type, 2, 0)?,
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
                let use_values = require_carrier(
                    named_reference_values(ctx, lr, "EDGE_LOOP", 1)?,
                    failure,
                    loop_step,
                    CarrierKind::EdgeLoopMemberList,
                )
                .ok_or(BuildError::Absent)?;
                let mut uses = Vec::new();
                for use_step in ctx
                    .admit_iter(&use_values[..], "STEP body topology traversal")
                    .map_err(cadmpeg_core::CodecError::from)?
                    .filter_map(ValueExt::reference)
                {
                    ctx.push_vec(&mut uses, use_step, "step_brep_edge_loop_uses")?;
                }
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
                        let candidate = surface_step.and_then(|surface_step| {
                            let pcurve_step = pcurve?;
                            let pcurve = exchange.records().get(&pcurve_step)?;
                            let pcurve_id = PcurveId::from(ids::data(kind!("pcurve"), pcurve_step));
                            let edge_curve = edge.curve()?;
                            Some((surface_step, pcurve_step, pcurve, pcurve_id, edge_curve))
                        });
                        let explicit_pcurve = if let Some((
                            surface_step,
                            pcurve_step,
                            pcurve,
                            pcurve_id,
                            edge_curve,
                        )) = candidate
                        {
                            let associated =
                                if let Some(curve_record) = exchange.records().get(&edge_curve) {
                                    surface_curve_pcurves(ctx, curve_record)?
                                        .any(|step| step == pcurve_step)
                                } else {
                                    false
                                };
                            (pcurve.partial(ctx, "PCURVE")?.is_some()
                                && entity_parameter(ctx, pcurve, "PCURVE", 1)?
                                    .and_then(ValueExt::reference)
                                    == Some(surface_step)
                                && decoded_pcurves.contains(&pcurve_step)
                                && associated)
                                .then_some(pcurve_id)
                        } else {
                            None
                        };
                        if let Some(pcurve) = explicit_pcurve {
                            ctx.collect_vec([(pcurve, None)], "step_brep_pcurve_candidates")?
                        } else {
                            ctx.push_vec(losses, StepLossCode::SeamEdgePcurveUnresolved.note(format!(
                                    "SEAM_EDGE #{use_step} has no decoded pcurve reference that belongs to its edge curve and face surface; the coedge has no pcurve"
                                )), "step_topology_losses")?;
                            Vec::new()
                        }
                    } else if let (Some(surface), Some(curve)) = (surface_step, edge.curve()) {
                        let associated =
                            associated_pcurves(curve, surface, exchange, decoded_pcurves, ctx)?;
                        if associated.is_empty() {
                            Vec::new()
                        } else {
                            match select_associated_pcurve(
                                ir,
                                exchange,
                                surface,
                                edge,
                                PcurveAssociationSources {
                                    vdefs,
                                    point_positions,
                                    candidates: &associated,
                                },
                                ctx,
                            ) {
                                Ok(selected) => {
                                    ctx.push_vec(
                                        &mut admissions,
                                        PcurveAdmission {
                                            curve,
                                            surface,
                                            coedge_use: use_step,
                                        },
                                        "step_brep_pcurve_admissions",
                                    )?;
                                    ctx.collect_vec(
                                        [(selected.id, selected.parameter_range)],
                                        "step_brep_pcurve_candidates",
                                    )?
                                }
                                Err(PcurveSelectionFailure::ResourceLimit(limit)) => {
                                    return Err(CodecError::ResourceLimit(limit).into());
                                }
                                Err(PcurveSelectionFailure::Resource(error)) => {
                                    return Err(error.into());
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
                                        PcurveSelectionFailure::Resource(error) => {
                                            return Err(error.into());
                                        }
                                    };
                                    ctx.push_vec(losses, note, "step_topology_losses")?;
                                    Vec::new()
                                }
                            }
                        }
                    } else {
                        ctx.push_vec(losses, StepLossCode::EdgeNoSurfaceOrCurveForPcurve.note(format!(
                            "edge #{} has no decoded surface or curve carrier, so its coedge has no pcurve",
                            o.edge
                        )), "step_topology_losses")?;
                        Vec::new()
                    };
                    let mut pcurve_uses = Vec::new();
                    for (pcurve, parameter_range) in pcurves {
                        let parameter_range = match parameter_range
                            .map(cadmpeg_ir::geometry::DirectedParameterRange::new)
                            .transpose()
                        {
                            Ok(range) => range,
                            Err(error) => {
                                ctx.push_vec(
                                    losses,
                                    StepLossCode::DecodeWarning
                                        .note(format!("coedge pcurve parameter_range: {error}")),
                                    "step_topology_losses",
                                )?;
                                return Err(BuildError::Absent);
                            }
                        };
                        ctx.push_vec(
                            &mut pcurve_uses,
                            PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range,
                            },
                            "step_brep_pcurve_uses",
                        )?;
                    }
                    ctx.push_vec(
                        &mut coedge_ids,
                        cid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                        "step_brep_coedge_ids",
                    )?;
                    ctx.push_vec(
                        &mut coedges,
                        Coedge {
                            id: cid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                            owner_loop: lid
                                .try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                            edge: scoped_edge_id(o.edge, id, shell_step, scope_edges, scope_root),
                            radial_next: cid,
                            sense: if (o.forward == edge.same()) == bound_forward {
                                Sense::Forward
                            } else {
                                Sense::Reversed
                            },
                            pcurves: pcurve_uses,
                            use_curve: None,
                        },
                        "step_brep_coedges",
                    )?;
                    ctx.push_btree_group(
                        &mut radial,
                        scoped_edge_id(o.edge, id, shell_step, scope_edges, scope_root),
                        coedges.len() - 1,
                        "step_brep_radial_groups",
                        "step_brep_radial_members",
                    )?;
                    ctx.insert_btree_set(
                        &mut used_e,
                        (shell_step, o.edge),
                        "step_brep_used_edges",
                    )?;
                    for vertex in [edge.vertices().0, edge.vertices().1] {
                        ctx.insert_btree_set(
                            &mut used_v,
                            (shell_step, vertex),
                            "step_brep_used_vertices",
                        )?;
                    }
                    for claim in [use_step, o.edge] {
                        ctx.insert_btree_set(&mut typed, claim, "step_brep_typed")?;
                    }
                    if let Some(parent) = edge.parent() {
                        ctx.insert_btree_set(&mut typed, parent, "step_brep_typed")?;
                    }
                }
                let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(ctx, coedge_ids, Vec::new())
                    .map_err(cadmpeg_core::CodecError::from)?
                else {
                    note_failure(failure, loop_step, CarrierKind::EdgeLoopCarrier);
                    return Err(BuildError::Absent);
                };
                ctx.push_vec(
                    &mut loops,
                    Loop {
                        id: lid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                        face: fid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                    },
                    "step_brep_loops",
                )?;
                ctx.push_vec(&mut loop_ids, (is_outer_bound, lid), "step_brep_loop_ids")?;
                for claim in [bound_step, loop_step] {
                    ctx.insert_btree_set(&mut typed, claim, "step_brep_typed")?;
                }
            }
            // A face with more than one FACE_OUTER_BOUND is refused above, so
            // at most one bound carries the outer role here. A face with no
            // outer bound states no classification.
            let mut outer = None;
            let mut inner = Vec::new();
            for (is_outer, id) in loop_ids {
                if is_outer {
                    if outer.is_none() {
                        outer = Some(id);
                    }
                } else {
                    ctx.push_vec(&mut inner, id, "step_brep_inner_loops")?;
                }
            }
            let face_loops = match outer {
                Some(outer) => cadmpeg_ir::topology::FaceLoops::classified(outer, inner),
                None => cadmpeg_ir::topology::FaceLoops::unspecified(inner),
            };
            let face_forward = face_same_sense == shell_forward;
            ctx.push_vec(
                &mut faces,
                Face {
                    id: fid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                    shell: sid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
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
                },
                "step_brep_faces",
            )?;
            ctx.push_vec(&mut face_ids, fid, "step_brep_face_ids")?;
            ctx.insert_btree_set(&mut typed, face_step, "step_brep_typed")?;
        }
        let mut component_edge_text_storage =
            ctx.reserve_scoped(0, "STEP component edge identity")?;
        let mut component_edge_vertices = BTreeMap::new();
        for (used_shell, edge_id) in ctx
            .admit_iter(&used_e, "STEP body topology traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            if *used_shell != shell_step {
                continue;
            }
            let Some(edge) = edefs.get(edge_id) else {
                continue;
            };
            let (start, end) = edge.curve_vertices();
            ctx.insert_btree_map(
                &mut component_edge_vertices,
                scoped_edge_id(*edge_id, id, shell_step, scope_edges, scope_root).into_string(),
                (
                    scoped_vertex_id(start, id, shell_step, scope_edges, scope_root).into_string(),
                    scoped_vertex_id(end, id, shell_step, scope_edges, scope_root).into_string(),
                ),
                "step_brep_component_edges",
            )?;
        }
        for ((used_shell, edge_id), (start, end)) in ctx
            .admit_iter(&poly_edges, "STEP body topology traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            if *used_shell != shell_step {
                continue;
            }
            ctx.insert_btree_map(
                &mut component_edge_vertices,
                component_edge_text_storage.with_storage(|| {
                    ctx.copy_retained_text(edge_id.as_str(), "STEP component edge identity")
                })?,
                (
                    scoped_poly_vertex_id(*start, id, shell_step, scope_edges, scope_root)
                        .into_string(),
                    scoped_poly_vertex_id(*end, id, shell_step, scope_edges, scope_root)
                        .into_string(),
                ),
                "step_brep_component_edges",
            )?;
        }
        let components =
            connected_face_components(&face_ids, &loops, &coedges, &component_edge_vertices, ctx)?;
        if components.len() > 1 {
            let note = StepLossCode::ShellDisconnectedFaces.note(ctx.format_retained(format_args!(
                    "source {shell_type} #{shell_step} contains {} disconnected face components across {} faces",
                    components.len(),
                    face_ids.len(),
                ), "STEP disconnected shell message")?);
            ctx.push_vec(
                losses,
                note.with_provenance(
                    cadmpeg_ir::SourceProvenance::root(
                        crate::dialect::FORMAT,
                        u64_from_index(sr.span.start),
                    )
                    .with_tag(ctx.to_ascii_lowercase(shell_type, "STEP shell type lowercase")?),
                ),
                "step_topology_losses",
            )?;
        }
        for (component_index, component) in components.into_iter().enumerate() {
            if root.partial(ctx, "BREP_WITH_VOIDS")?.is_some()
                && shell_steps.first().copied() == Some(shell_reference)
                && component_index > 0
            {
                note_failure(failure, shell_step, CarrierKind::ConnectedOuterShell);
                return Err(BuildError::Absent);
            }
            let component_shell = if component_index == 0 {
                sid.try_clone_for_decode(ctx, "step_topology_identity_copy")?
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
            let mut component_faces = Vec::new();
            for face_index in component {
                let face_id =
                    face_ids[face_index].try_clone_for_decode(ctx, "step_brep_component_faces")?;
                faces[face_index].shell =
                    component_shell.try_clone_for_decode(ctx, "step_topology_identity_copy")?;
                ctx.push_vec(&mut component_faces, face_id, "step_brep_component_faces")?;
            }
            ctx.charge_collection_items(
                u64_from_index(component_faces.len()),
                "step_brep_shell_validation",
            )?;
            ctx.push_vec(
                &mut shells,
                match Shell::new(
                    component_shell.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                    rid.try_clone_for_decode(ctx, "step_topology_identity_copy")?,
                    component_faces,
                    vec![],
                    vec![],
                ) {
                    Ok(shell) => shell,
                    Err(error) => {
                        ctx.push_vec(
                            losses,
                            StepLossCode::DecodeWarning.note(ctx.format_retained(
                                format_args!("{shell_type} #{shell_step}: {error}"),
                                "STEP shell failure message",
                            )?),
                            "step_topology_losses",
                        )?;
                        return Err(BuildError::Absent);
                    }
                },
                "step_brep_shells",
            )?;
            ctx.push_vec(
                &mut region.shells,
                component_shell,
                "step_brep_region_shells",
            )?;
        }
        ctx.insert_btree_set(&mut typed, shell_step, "step_brep_typed")?;
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
        ctx.push_vec(
            &mut edges,
            Edge {
                id: scoped_edge_id(edge_id, id, shell_step, scope_edges, scope_root),
                carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(edge_curve_id_reported(
                    edge_id, e, exchange, losses, ctx,
                )?),
                start: scoped_vertex_id(start, id, shell_step, scope_edges, scope_root),
                end: scoped_vertex_id(end, id, shell_step, scope_edges, scope_root),
                tolerance: None,
            },
            "step_brep_edges",
        )?;
    }
    for ((shell_step, edge_identity), (start, end)) in poly_edges {
        ctx.push_vec(
            &mut edges,
            Edge {
                id: edge_identity,
                carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
                start: scoped_poly_vertex_id(start, id, shell_step, scope_edges, scope_root),
                end: scoped_poly_vertex_id(end, id, shell_step, scope_edges, scope_root),
                tolerance: None,
            },
            "step_brep_edges",
        )?;
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
        ctx.push_vec(
            &mut vertices,
            Vertex {
                id: scoped_vertex_id(vertex_id, id, shell_step, scope_edges, scope_root),
                point: PointId::from(ids::data(kind!("point"), v.point)),
                tolerance: None,
            },
            "step_brep_vertices",
        )?;
        ctx.insert_btree_set(&mut typed, vertex_id, "step_brep_typed")?;
    }
    for (shell_step, point_id) in poly_points {
        require_carrier(
            point_positions.get(point_id),
            failure,
            point_id,
            CarrierKind::PolyVertexPoint,
        )
        .ok_or(BuildError::Absent)?;
        ctx.push_vec(
            &mut vertices,
            Vertex {
                id: scoped_poly_vertex_id(point_id, id, shell_step, scope_edges, scope_root),
                point: PointId::from(ids::data(kind!("point"), point_id)),
                tolerance: None,
            },
            "step_brep_vertices",
        )?;
        ctx.insert_btree_set(&mut typed, point_id, "step_brep_typed")?;
    }
    for indices in ctx
        .admit_iter(&radial, "STEP body topology radial traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .map(|(_, value)| value)
    {
        for (position, &index) in ctx
            .admit_iter(&indices[..], "STEP body topology traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            coedges[index].radial_next = coedges[indices[(position + 1) % indices.len()]]
                .id
                .try_clone_for_decode(ctx, "step_topology_identity_copy")?;
        }
    }
    let mut edge_by_id = BTreeMap::<&EdgeId, &Edge>::new();
    for edge in ctx
        .admit_iter(&edges, "STEP body topology traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        ctx.insert_btree_map(&mut edge_by_id, &edge.id, edge, "step_brep_edge_index")?;
    }
    let mut coedge_by_id = BTreeMap::<&CoedgeId, &Coedge>::new();
    for coedge in ctx
        .admit_iter(&coedges, "STEP body topology traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        ctx.insert_btree_map(
            &mut coedge_by_id,
            &coedge.id,
            coedge,
            "step_brep_coedge_index",
        )?;
    }
    for loop_ in ctx
        .admit_iter(&loops, "STEP body topology traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        if loop_.coedges().is_empty() {
            continue;
        }
        let loop_source = source_numeric_id(ctx, loop_.id.as_str(), "loop")?.unwrap_or(0);
        for (index, current_id) in ctx
            .admit_iter(&(loop_.coedges())[..], "STEP body topology traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
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
            if !ctx.equal(current_end, next_start, "STEP edge loop vertex equality")? {
                note_failure(failure, loop_source, CarrierKind::EdgeLoopContinuity);
                return Err(BuildError::Absent);
            }
        }
    }
    let mut built = match staged_topology(
        StagedTopologyParts {
            typed,
            vertices,
            edges,
            coedges,
            loops,
            faces,
            surfaces,
            shells,
            region,
            body,
        },
        ctx,
    ) {
        Ok(built) => built,
        Err(StageError::Draft(_)) => {
            note_failure(failure, id, CarrierKind::TopologyDraft);
            return Err(BuildError::Absent);
        }
        Err(StageError::Resource(error)) => return Err(BuildError::Resource(error)),
    };
    built.pcurve_admissions = admissions;
    for &shell_reference in ctx
        .admit_iter(shell_steps, "STEP body topology traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let shell_step = if root.partial(ctx, "FACE_BASED_SURFACE_MODEL")?.is_some() {
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
        ctx.insert_btree_set(
            &mut built.shell_sources,
            shell_step,
            "step_brep_shell_sources",
        )?;
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
    let mut neighbors =
        ctx.collect_indexed_vec(face_ids.len(), "STEP connected-face neighbors", |_| {
            Ok(BTreeSet::new())
        })?;
    let mut face_indices = BTreeMap::new();
    for (index, face) in ctx
        .admit_iter(&face_ids[..], "STEP connected face components traversal")?
        .enumerate()
    {
        ctx.insert_btree_map(
            &mut face_indices,
            face.as_str(),
            index,
            "STEP connected-face indices",
        )?;
    }
    let mut coedge_edges = BTreeMap::new();
    for coedge in ctx.admit_iter(coedges, "STEP connected face components traversal")? {
        ctx.insert_btree_map(
            &mut coedge_edges,
            coedge.id.as_str(),
            coedge.edge.as_str(),
            "STEP connected-face coedge edges",
        )?;
    }
    let mut faces_by_edge = BTreeMap::<&str, BTreeSet<usize>>::new();
    let mut faces_by_vertex = BTreeMap::<&str, BTreeSet<usize>>::new();
    for loop_ in ctx.admit_iter(loops, "STEP connected face components traversal")? {
        let Some(&face_index) = face_indices.get(loop_.face.as_str()) else {
            continue;
        };
        for coedge_id in ctx.admit_iter(
            loop_.coedges(),
            "STEP connected face components view traversal",
        )? {
            let Some(edge_id) = coedge_edges.get(coedge_id.as_str()) else {
                continue;
            };
            insert_connected_face_group(&mut faces_by_edge, edge_id, face_index, ctx)?;
            if let Some((start, end)) = edge_vertices.get(*edge_id) {
                insert_connected_face_group(&mut faces_by_vertex, start, face_index, ctx)?;
                insert_connected_face_group(&mut faces_by_vertex, end, face_index, ctx)?;
            }
        }
        for vertex in loop_
            .singular_vertex()
            .map(|(vertex, _)| vertex)
            .into_iter()
            .chain(
                ctx.admit_iter(
                    loop_.anchored_vertex_uses(),
                    "STEP connected face anchored vertex traversal",
                )?
                .map(|use_| &use_.vertex),
            )
        {
            insert_connected_face_group(&mut faces_by_vertex, vertex.as_str(), face_index, ctx)?;
        }
    }

    for group in ctx
        .admit_iter(
            &(faces_by_edge),
            "STEP connected face components map traversal",
        )?
        .map(|(_, value)| value)
        .chain(
            ctx.admit_iter(
                &faces_by_vertex,
                "STEP connected face components chain traversal",
            )?
            .map(|(_, value)| value),
        )
    {
        for &face in group {
            for &other in group {
                if other != face {
                    ctx.insert_btree_set(&mut neighbors[face], other, "STEP connected-face links")?;
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
        ctx.push_vec(&mut pending, start, "STEP connected-face pending")?;
        while let Some(face) = pending.pop() {
            ctx.push_vec(&mut component, face, "STEP connected-face component")?;
            for &neighbor in
                ctx.admit_iter(&neighbors[face], "STEP connected face components traversal")?
            {
                if !reached[neighbor] {
                    reached[neighbor] = true;
                    ctx.push_vec(&mut pending, neighbor, "STEP connected-face pending")?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut component,
            |value| value,
            Ord::cmp,
            "STEP connected-face component sort",
        )?;
        ctx.push_vec(&mut components, component, "STEP connected-face components")?;
    }
    Ok(components)
}

fn insert_connected_face_group<'a>(
    groups: &mut BTreeMap<&'a str, BTreeSet<usize>>,
    key: &'a str,
    face: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.admit_btree_entry(groups, &key, "STEP connected-face groups")?;
    let group = groups.entry(key).or_default();
    ctx.insert_btree_set(group, face, "STEP connected-face group faces")?;
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Vec<Point3>>>, CodecError> {
    let mut loops = Vec::new();
    for &bound_step in ctx.admit_iter(bounds, "STEP implicit face points traversal")? {
        let Some(bound) = exchange.records().get(&bound_step) else {
            return Ok(None);
        };
        let Some(bound_type) = face_bound_attribute_type(ctx, bound)? else {
            return Ok(None);
        };
        let Some(loop_step) = named_reference(ctx, bound, bound_type, 1, 0)? else {
            return Ok(None);
        };
        let Some(loop_record) = exchange.records().get(&loop_step) else {
            return Ok(None);
        };
        if loop_record.partial(ctx, "POLY_LOOP")?.is_none() {
            return Ok(None);
        }
        let Some(bound_forward) = named_logical(ctx, bound, bound_type, 2, 0)? else {
            return Ok(None);
        };
        let Some(point_values) = named_reference_values(ctx, loop_record, "POLY_LOOP", 1)? else {
            return Ok(None);
        };
        let mut point_steps = Vec::new();
        for point in point_values.iter().filter_map(ValueExt::reference) {
            ctx.push_vec(&mut point_steps, point, "step_implicit_face_point_steps")?;
        }
        if point_steps.first() == point_steps.last() {
            point_steps.pop();
        }
        point_steps.dedup();
        let mut distinct = BTreeSet::new();
        for &point in ctx.admit_iter(&point_steps[..], "STEP implicit face points traversal")? {
            ctx.insert_btree_set(&mut distinct, point, "step_implicit_face_distinct_points")?;
        }
        if point_steps.len() < 3 || distinct.len() != point_steps.len() {
            return Ok(None);
        }
        if !bound_forward {
            point_steps.reverse();
        }
        let mut points = Vec::new();
        for point_step in point_steps {
            let point_step = vdefs
                .get(&point_step)
                .map_or(point_step, |vertex| vertex.point);
            let Some(point) = point_positions.get(point_step).copied() else {
                return Ok(None);
            };
            if points.last().is_none_or(|previous| *previous != point) {
                ctx.push_vec(&mut points, point, "step_implicit_face_points")?;
            }
        }
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 3 {
            return Ok(None);
        }
        ctx.push_vec(&mut loops, points, "step_implicit_face_loops")?;
    }
    Ok((!loops.is_empty()).then_some(loops))
}

const IMPLICIT_FACE_AREA_RELATIVE_TOLERANCE: f64 = EPS_TOPOLOGY_READ_EXACT_GEOMETRY;
const IMPLICIT_FACE_NORMAL_ALIGNMENT_TOLERANCE: f64 = EPS_TOPOLOGY_READ_DEGENERATE;
const IMPLICIT_FACE_PLANAR_RELATIVE_TOLERANCE: f64 = EPS_TOPOLOGY_READ_EXACT_GEOMETRY;

fn implicit_face_plane(
    bounds: &[u64],
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    point_positions: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<Option<SurfaceGeometry>, CodecError> {
    let Some(loops) = implicit_face_points(bounds, exchange, vdefs, point_positions, ctx)? else {
        return Ok(None);
    };
    let mut points = Vec::new();
    for loop_points in
        ctx.admit_iter(loops.as_slice(), "STEP implicit face plane loop traversal")?
    {
        for point in ctx
            .admit_iter(
                loop_points.as_slice(),
                "STEP implicit face plane point traversal",
            )?
            .copied()
        {
            ctx.push_vec(&mut points, point, "step_implicit_face_plane_points")?;
        }
    }
    ctx.stable_sort_by_key(
        &mut points,
        |value| (value.x, value.y, value.z),
        |left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.total_cmp(&right.1))
                .then_with(|| left.2.total_cmp(&right.2))
        },
        "step_implicit_face_plane_sort",
    )?;
    let Some(point_count) = cadmpeg_core::convert::f64_from_index(points.len()) else {
        return Ok(None);
    };
    let origin = Point3::new(
        ctx.admit_iter(&points[..], "STEP implicit face plane traversal")?
            .map(|point| point.x)
            .sum::<f64>()
            / point_count,
        ctx.admit_iter(&points[..], "STEP implicit face plane traversal")?
            .map(|point| point.y)
            .sum::<f64>()
            / point_count,
        ctx.admit_iter(&points[..], "STEP implicit face plane traversal")?
            .map(|point| point.z)
            .sum::<f64>()
            / point_count,
    );
    let scale = ctx
        .admit_iter(&points[..], "STEP implicit face plane traversal")?
        .map(|point| point.vector_from(origin))
        .map(|vector| vector.norm())
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= f64::EPSILON {
        return Ok(None);
    }
    let mut loop_normals = Vec::new();
    for loop_points in &loops {
        let Some(loop_count) = cadmpeg_core::convert::f64_from_index(loop_points.len()) else {
            return Ok(None);
        };
        let loop_origin = Point3::new(
            ctx.admit_iter(&loop_points[..], "STEP implicit face plane traversal")?
                .map(|point| point.x)
                .sum::<f64>()
                / loop_count,
            ctx.admit_iter(&loop_points[..], "STEP implicit face plane traversal")?
                .map(|point| point.y)
                .sum::<f64>()
                / loop_count,
            ctx.admit_iter(&loop_points[..], "STEP implicit face plane traversal")?
                .map(|point| point.z)
                .sum::<f64>()
                / loop_count,
        );
        let mut area_normal = Vector3::new(0.0, 0.0, 0.0);
        for (current, next) in ctx
            .admit_iter(&loop_points[..], "STEP implicit face plane traversal")?
            .zip(
                ctx.admit_iter(
                    &(loop_points)[..],
                    "STEP implicit face plane next-point traversal",
                )?
                .skip(1)
                .chain(loop_points.first()),
            )
            .take(loop_points.len())
        {
            area_normal = area_normal
                + current
                    .vector_from(loop_origin)
                    .cross(next.vector_from(loop_origin));
        }
        let area = area_normal.norm();
        if !area.is_finite() || area <= IMPLICIT_FACE_AREA_RELATIVE_TOLERANCE * scale * scale {
            return Ok(None);
        }
        let Some(normal) = UnitVector3::normalized(area_normal) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut loop_normals,
            (normal, area),
            "step_implicit_face_loop_normals",
        )?;
    }
    let Some((mut normal, mut largest_area)) = loop_normals.first().copied() else {
        return Ok(None);
    };
    for (candidate, area) in ctx
        .admit_iter(&loop_normals[..], "STEP implicit face plane traversal")?
        .skip(1)
        .copied()
    {
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
    for (candidate, _) in ctx.admit_iter(&loop_normals[..], "STEP implicit face plane traversal")? {
        if candidate.as_raw().dot(*normal.as_raw()) < 1.0 - IMPLICIT_FACE_NORMAL_ALIGNMENT_TOLERANCE
        {
            return Ok(None);
        }
    }
    let planarity_tolerance =
        COINCIDENCE_TOLERANCE.max(IMPLICIT_FACE_PLANAR_RELATIVE_TOLERANCE * scale);
    if ctx
        .admit_iter(&points[..], "STEP implicit face plane traversal")?
        .map(|point| point.vector_from(origin))
        .map(|point| point.dot(*normal.as_raw()).abs())
        .fold(0.0, f64::max)
        > planarity_tolerance
    {
        return Ok(None);
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
    let Some(u_axis) = u_axis else {
        return Ok(None);
    };
    let Some(origin) = cadmpeg_ir::features::FinitePoint3::new(origin) else {
        return Ok(None);
    };
    let Some(frame) = OrthonormalFrame3::from_units(normal, u_axis) else {
        return Ok(None);
    };
    Ok(Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::new(origin, frame),
    ))))
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
    if !ctx.any_by(
        &curve.partials[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
            ))
        },
        "STEP associated pcurves traversal",
    )? {
        return Ok(Vec::new());
    }
    let mut associated = Vec::new();
    for pcurve_step in surface_curve_pcurves(ctx, curve)? {
        let Some(pcurve) = exchange.records().get(&pcurve_step) else {
            continue;
        };
        if pcurve.partial(ctx, "PCURVE")?.is_some()
            && entity_parameter(ctx, pcurve, "PCURVE", 1)?.and_then(Value::reference)
                == Some(surface_step)
            && decoded_pcurves.contains(&pcurve_step)
        {
            let pcurve_id = PcurveId::from(ids::data(kind!("pcurve"), pcurve_step));
            ctx.push_vec(&mut associated, pcurve_id, "step_associated_pcurves")?;
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

#[derive(Debug)]
enum PcurveSelectionFailure {
    NotUnique { count: usize },
    Carrier,
    Endpoint,
    Locus,
    ResourceLimit(ResourceLimit),
    Resource(CodecError),
}

impl From<ResourceLimit> for PcurveSelectionFailure {
    fn from(limit: ResourceLimit) -> Self {
        Self::ResourceLimit(limit)
    }
}

impl From<CodecError> for PcurveSelectionFailure {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
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

#[derive(Clone, Copy)]
struct PcurveAssociationSources<'a> {
    vdefs: &'a BTreeMap<u64, VertexDef>,
    point_positions: &'a CarrierIndex,
    candidates: &'a [PcurveId],
}

fn select_associated_pcurve(
    ir: &CadIr,
    exchange: &Exchange,
    surface_step: u64,
    edge: &EdgeDef,
    sources: PcurveAssociationSources<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<SelectedPcurve, PcurveSelectionFailure> {
    let PcurveAssociationSources {
        vdefs,
        point_positions,
        candidates,
    } = sources;
    let [candidate] = candidates else {
        return Err(PcurveSelectionFailure::NotUnique {
            count: candidates.len(),
        });
    };
    let candidate = candidate.try_clone_for_decode(ctx, "step_selected_pcurve_id")?;
    let surface_identity = ids::data(kind!("surface"), surface_step);
    let surface = ctx
        .find_map(
            &(ir.model.surfaces)[..],
            |surface| -> Result<Option<_>, CodecError> {
                Ok((ctx.equal(
                    surface.id.as_str(),
                    surface_identity.as_str(),
                    "STEP select associated pcurve equality",
                )?)
                .then_some(surface))
            },
            "STEP select associated pcurve traversal",
        )?
        .map(|surface| &surface.geometry)
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let surface_id = SurfaceId::from(surface_identity);
    let index = ModelIndex::build(ir, ctx)?;
    let pcurve = ctx
        .find_map(
            &(ir.model.pcurves)[..],
            |pcurve| -> Result<Option<_>, CodecError> {
                Ok((ctx.equal(
                    &pcurve.id,
                    &candidate,
                    "STEP select associated pcurve equality",
                )?)
                .then_some(pcurve))
            },
            "STEP select associated pcurve traversal",
        )?
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
        surface,
        curve_start,
        curve_end,
        ctx,
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
        PcurveWitness {
            endpoint,
            curve_start,
            curve_end,
            bound,
        },
        ctx,
    )? {
        return Err(PcurveSelectionFailure::Locus);
    }
    let parameter_range = if let Some(range) = pcurve_declared_parameter_range(geometry) {
        let declared = pcurve_declared_endpoint_fit_directed(
            ctx,
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

#[derive(Clone, Copy)]
struct PcurveWitness {
    endpoint: PcurveEndpointFit,
    curve_start: Point3,
    curve_end: Point3,
    bound: f64,
}

fn pcurve_locus_witness(
    index: &ModelIndex<'_>,
    exchange: &Exchange,
    edge: &EdgeDef,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    witness: PcurveWitness,
    ctx: &DecodeContext<'_>,
) -> Result<bool, PcurveSelectionFailure> {
    let PcurveWitness {
        endpoint,
        curve_start,
        curve_end,
        bound,
    } = witness;
    let Some(curve_step) = edge
        .curve()
        .map(|curve| curve_carrier_record(ctx, curve, exchange))
        .transpose()?
        .flatten()
    else {
        return Ok(false);
    };
    let curve_id = CurveId::from(ids::data(kind!("curve"), curve_step));
    let curve_seeds = curve_selection_parameter_domain(index, &curve_id, ctx)?.map_or(
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
        curve_parameter_near_point(ctx, index, &curve_id, curve_start, &curve_seeds, bound)?
    else {
        return Ok(false);
    };
    let Some(curve_end_parameter) =
        curve_parameter_near_point(ctx, index, &curve_id, curve_end, &curve_seeds, bound)?
    else {
        return Ok(false);
    };
    let mut fractions = Vec::new();
    for step in 0..PCURVE_LOCUS_SAMPLE_COUNT {
        ctx.push_vec(
            &mut fractions,
            cadmpeg_core::convert::f64_from_index(step)
                .ok_or_else(|| ctx.refuse_codec_limit("step_pcurve_locus_fractions", 0, 1))?
                / cadmpeg_core::convert::f64_from_index(PCURVE_LOCUS_SAMPLE_COUNT - 1)
                    .ok_or_else(|| ctx.refuse_codec_limit("step_pcurve_locus_fractions", 0, 1))?,
            "step_pcurve_locus_fractions",
        )?;
    }
    let mut break_fractions = Vec::new();
    pcurve_parameter_break_fractions(
        geometry,
        [endpoint.start_parameter, endpoint.end_parameter],
        &mut break_fractions,
        ctx,
    )?;
    ctx.append_vec(
        &mut fractions,
        &mut break_fractions,
        "step_pcurve_locus_fractions",
    )?;
    ctx.stable_sort_by(
        &mut fractions,
        |value| value,
        f64::total_cmp,
        "step_pcurve_locus_fractions_sort",
    )?;
    fractions.dedup_by(|left, right| *left == *right);
    for fraction in fractions {
        let pcurve_parameter = endpoint
            .start_parameter
            .mul_add(1.0 - fraction, endpoint.end_parameter * fraction);
        let Some(uv) = pcurve_selection_uv(ctx, geometry, pcurve_parameter)? else {
            return Ok(false);
        };
        let Some(mapped) = surface_selection_point(ctx, index, surface_id, uv.u, uv.v)? else {
            return Ok(false);
        };
        let curve_seed =
            curve_start_parameter.mul_add(1.0 - fraction, curve_end_parameter * fraction);
        let seeds = [
            curve_seeds[0],
            curve_seeds[1],
            curve_seeds[2],
            curve_seeds[3],
            curve_seeds[4],
            curve_seeds[5],
            curve_seed,
        ];
        let Some(curve_parameter) =
            curve_parameter_near_point(ctx, index, &curve_id, mapped, &seeds, bound)?
        else {
            return Ok(false);
        };
        let curve_point = match model_curve_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
            index,
            &curve_id,
            curve_parameter,
        ) {
            Ok(point) => point,
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
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
    ctx: &DecodeContext<'_>,
    index: &ModelIndex<'_>,
    curve_id: &CurveId,
    point: Point3,
    seeds: &[f64],
    tolerance: f64,
) -> Result<Option<f64>, CodecError> {
    let mut best: Option<(f64, f64)> = None;
    for &seed in ctx
        .admit_iter(&seeds[..], "STEP curve parameter near point traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .filter(|seed| seed.is_finite())
    {
        let Some(parameter) = model_curve_parameter_near_point_in_index_with_tolerance(
            ctx, index, curve_id, point, seed, tolerance,
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<PcurveEndpointFit>, PcurveSelectionFailure> {
    if let Some(parameter_range) = pcurve_declared_parameter_range(geometry) {
        let Some(declared_score) = pcurve_declared_endpoint_fit_directed(
            ctx,
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
        let seeds = pcurve_selection_seeds(index, surface_id, geometry, surface, ctx)?;
        let Some(start) = pcurve_surface_closest(ctx, index, surface_id, geometry, start, &seeds)?
        else {
            return Ok(None);
        };
        let Some(end) = pcurve_surface_closest(ctx, index, surface_id, geometry, end, &seeds)?
        else {
            return Ok(None);
        };
        return Ok(Some(PcurveEndpointFit {
            start_parameter: start.1,
            end_parameter: end.1,
            max_residual: start.0.max(end.0),
        }));
    }
    let seeds = pcurve_selection_seeds(index, surface_id, geometry, surface, ctx)?;
    let Some(start) = pcurve_surface_closest(ctx, index, surface_id, geometry, start, &seeds)?
    else {
        return Ok(None);
    };
    let Some(end) = pcurve_surface_closest(ctx, index, surface_id, geometry, end, &seeds)? else {
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
    ctx: &DecodeContext<'_>,
) -> Result<[f64; 2], CodecError> {
    let domains = match index.surfaces(surface_id.as_str(), ctx)? {
        Some(surface) => {
            surface_selection_parameter_domains(index, surface_id, &surface.geometry, ctx)?
        }
        None => [None, None],
    };
    Ok([
        clamp_selection_parameter(u, domains[0]),
        clamp_selection_parameter(v, domains[1]),
    ])
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    u: f64,
    v: f64,
) -> Result<Option<Point3>, CodecError> {
    let [u, v] = surface_selection_parameters(index, surface_id, u, v, ctx)?;
    // A non-finite point is returned as the evaluation reached it; the
    // selection measures read it as a miss.
    match model_surface_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
        index,
        surface_id,
        u,
        v,
    ) {
        Ok(point) => Ok(Some(point.get())),
        Err(failure) => failure.non_finite().map_err(CodecError::from),
    }
}

#[cfg(test)]
fn pcurve_declared_endpoint_fit(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    range: [f64; 2],
    start: Point3,
    end: Point3,
) -> Result<Option<f64>, CodecError> {
    let Some(first_uv) = pcurve_selection_uv(ctx, geometry, range[0])? else {
        return Ok(None);
    };
    let Some(last_uv) = pcurve_selection_uv(ctx, geometry, range[1])? else {
        return Ok(None);
    };
    let Some(first) = surface_selection_point(ctx, index, surface_id, first_uv.u, first_uv.v)?
    else {
        return Ok(None);
    };
    let Some(last) = surface_selection_point(ctx, index, surface_id, last_uv.u, last_uv.v)? else {
        return Ok(None);
    };
    let forward = first.distance(start).max(last.distance(end));
    let reversed = first.distance(end).max(last.distance(start));
    Ok(Some(forward.min(reversed)))
}

fn pcurve_declared_endpoint_fit_directed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    range: [f64; 2],
    start: Point3,
    end: Point3,
) -> Result<Option<f64>, CodecError> {
    let Some(first_uv) = pcurve_selection_uv(ctx, geometry, range[0])? else {
        return Ok(None);
    };
    let Some(last_uv) = pcurve_selection_uv(ctx, geometry, range[1])? else {
        return Ok(None);
    };
    let Some(first) = surface_selection_point(ctx, index, surface_id, first_uv.u, first_uv.v)?
    else {
        return Ok(None);
    };
    let Some(last) = surface_selection_point(ctx, index, surface_id, last_uv.u, last_uv.v)? else {
        return Ok(None);
    };
    Ok(Some(first.distance(start).max(last.distance(end))))
}

/// The pcurve point at `parameter`. A non-finite offset-pcurve point is
/// returned as the evaluation reached it; the selection measures read it as a
/// miss.
fn pcurve_selection_uv(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &PcurveGeometry,
    parameter: f64,
) -> Result<Option<Point2>, ResourceLimit> {
    match cadmpeg_ir::eval::decode::pcurve_uv(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
        geometry,
        parameter,
    ) {
        Ok(uv) => Ok(Some(uv.get())),
        Err(failure) => failure.non_finite(),
    }
}

fn pcurve_surface_closest(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    target: Point3,
    seeds: &[f64],
) -> Result<Option<(f64, f64)>, CodecError> {
    ctx.charge_work_limit(0, "geometry helper boundary")?;
    // The minimum is only over the finite seed set. The caller treats the
    // directly evaluated result as a witness and omits the optional relation
    // when no witness meets the tolerance.
    let mut best: Option<(f64, f64)> = None;
    for &seed in seeds {
        ctx.charge_work_limit(1, "step pcurve seed visit")?;
        let Some(candidate) =
            mapped_pcurve_closest(ctx, index, surface_id, geometry, target, seed)?
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    target: Point3,
    seed: f64,
) -> Result<Option<(f64, f64)>, CodecError> {
    ctx.charge_work_limit(0, "geometry helper boundary")?;
    if !seed.is_finite() {
        return Ok(None);
    }
    let domain = pcurve_selection_parameter_domain(geometry);
    let clamp_to_domain =
        |parameter: f64| domain.map_or(parameter, |[lower, upper]| parameter.clamp(lower, upper));
    let evaluate_point = |parameter: f64| -> Result<Option<Point3>, CodecError> {
        let Some(uv) = pcurve_selection_uv(ctx, geometry, parameter)? else {
            return Ok(None);
        };
        surface_selection_point(ctx, index, surface_id, uv.u, uv.v)
    };
    let evaluate_tangent = |parameter: f64| -> Result<Option<Vector3>, CodecError> {
        let Some(uv) = pcurve_selection_uv(ctx, geometry, parameter)? else {
            return Ok(None);
        };
        let tangent_uv = match pcurve_tangent(ctx, geometry, parameter) {
            Ok(value) => value,
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
            Err(_) => return Ok(None),
        };
        let [u, v] = surface_selection_parameters(index, surface_id, uv.u, uv.v, ctx)?;
        let partials = match model_surface_partials_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
            index,
            surface_id,
            u,
            v,
        ) {
            Ok(value) => value,
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
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
        ctx.charge_work_limit(1, "step pcurve inverse step")?;
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
            ctx.charge_work_limit(1, "step pcurve inverse backtrack")?;
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

fn add_pcurve_break_fraction(
    parameter: f64,
    parameters: [f64; 2],
    fractions: &mut Vec<f64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if let Some(fraction) =
        cadmpeg_ir::math::parameter_fraction(parameter, parameters[0], parameters[1])
    {
        if fraction.get() > 0.0 && fraction.get() < 1.0 {
            ctx.push_vec(fractions, fraction.get(), "step_pcurve_break_fractions")?;
        }
    }
    Ok(())
}

fn pcurve_parameter_break_fractions(
    geometry: &PcurveGeometry,
    parameters: [f64; 2],
    fractions: &mut Vec<f64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("step_pcurve_break_recursion")?;
    match geometry {
        PcurveGeometry::Nurbs { nurbs } => {
            for parameter in ctx
                .admit_iter(
                    &(nurbs.knots())[..],
                    "STEP pcurve parameter break fractions traversal",
                )?
                .copied()
            {
                add_pcurve_break_fraction(parameter, parameters, fractions, ctx)?;
            }
        }
        PcurveGeometry::PolarNurbs { nurbs } => {
            for parameter in ctx
                .admit_iter(
                    &(nurbs.knots())[..],
                    "STEP pcurve parameter break fractions traversal",
                )?
                .copied()
            {
                add_pcurve_break_fraction(parameter, parameters, fractions, ctx)?;
            }
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            let basis = trimmed_pcurve.basis();
            add_pcurve_break_fraction(parameter_range.endpoints()[0], parameters, fractions, ctx)?;
            add_pcurve_break_fraction(parameter_range.endpoints()[1], parameters, fractions, ctx)?;
            pcurve_parameter_break_fractions(basis, parameters, fractions, ctx)?;
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_parameter_break_fractions(basis, parameters, fractions, ctx)?;
        }
        PcurveGeometry::Transformed(placed) => {
            pcurve_parameter_break_fractions(placed.basis(), parameters, fractions, ctx)?;
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
    Ok(())
}

fn pcurve_selection_seeds(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    surface: &SurfaceGeometry,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<f64>, CodecError> {
    let mut seeds = ctx.collect_vec([0.0], "step_pcurve_selection_seeds")?;
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
        ctx.push_vec(&mut seeds, start, "step_pcurve_selection_seeds")?;
        if let Some(seed) = at_fraction(0.5) {
            ctx.push_vec(&mut seeds, seed, "step_pcurve_selection_seeds")?;
        }
        ctx.push_vec(&mut seeds, end, "step_pcurve_selection_seeds")?;
        for step in 0..=PCURVE_ENDPOINT_GRID_DIVISIONS {
            let fraction = cadmpeg_core::convert::f64_from_index(step)
                .ok_or_else(|| ctx.refuse_codec_limit("step_pcurve_selection_seeds", 0, 1))?
                / cadmpeg_core::convert::f64_from_index(PCURVE_ENDPOINT_GRID_DIVISIONS)
                    .ok_or_else(|| ctx.refuse_codec_limit("step_pcurve_selection_seeds", 0, 1))?;
            if let Some(seed) = at_fraction(fraction) {
                ctx.push_vec(&mut seeds, seed, "step_pcurve_selection_seeds")?;
            }
        }
        let mut fractions = Vec::new();
        for fraction in [0.0, 1.0] {
            ctx.push_vec(&mut fractions, fraction, "step_pcurve_selection_fractions")?;
        }
        pcurve_parameter_break_fractions(geometry, [start, end], &mut fractions, ctx)?;
        ctx.stable_sort_by(
            &mut fractions,
            |value| value,
            f64::total_cmp,
            "step_pcurve_selection_fractions_sort",
        )?;
        fractions.dedup_by(|left, right| *left == *right);
        for seed in ctx
            .admit_iter(&fractions[..], "STEP pcurve selection seeds traversal")?
            .filter_map(|fraction| at_fraction(*fraction))
        {
            ctx.push_vec(&mut seeds, seed, "step_pcurve_selection_seeds")?;
        }
        for seed in fractions.windows(2).filter_map(|window| {
            let lower = window[0];
            let upper = window[1];
            at_fraction(lower + (upper - lower) * 0.5)
        }) {
            ctx.push_vec(&mut seeds, seed, "step_pcurve_selection_seeds")?;
        }
    }
    if pcurve_has_angular_parameterization(geometry) {
        for seed in [
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            std::f64::consts::PI * 1.5,
        ] {
            ctx.push_vec(&mut seeds, seed, "step_pcurve_selection_seeds")?;
        }
    }
    if let Some((origin, direction)) = geometry.line_parameters(ctx)? {
        if let Some(domain) = surface
            .solved()
            .and_then(|surface| surface_periodic_domains(surface)[0])
        {
            if direction.u != 0.0 {
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    if let Some(coordinate) = periodic_seed_coordinate(domain, fraction) {
                        ctx.push_vec(
                            &mut seeds,
                            (coordinate - origin.u) / direction.u,
                            "step_pcurve_selection_seeds",
                        )?;
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
                        ctx.push_vec(
                            &mut seeds,
                            (coordinate - origin.v) / direction.v,
                            "step_pcurve_selection_seeds",
                        )?;
                    }
                }
            }
        }
        let [u_domain, v_domain] =
            surface_selection_parameter_domains(index, surface_id, surface, ctx)?;
        if let Some([u_lower, u_upper]) = u_domain {
            for boundary in [u_lower, u_lower.midpoint(u_upper), u_upper] {
                if direction.u != 0.0 {
                    ctx.push_vec(
                        &mut seeds,
                        (boundary - origin.u) / direction.u,
                        "step_pcurve_selection_seeds",
                    )?;
                }
            }
        }
        if let Some([v_lower, v_upper]) = v_domain {
            for boundary in [v_lower, v_lower.midpoint(v_upper), v_upper] {
                if direction.v != 0.0 {
                    ctx.push_vec(
                        &mut seeds,
                        (boundary - origin.v) / direction.v,
                        "step_pcurve_selection_seeds",
                    )?;
                }
            }
        }
    }
    let mut unique = Vec::new();
    for seed in seeds.into_iter().filter(|seed| seed.is_finite()) {
        if !unique.contains(&seed) {
            ctx.push_vec(&mut unique, seed, "step_pcurve_unique_seeds")?;
        }
    }
    Ok(unique)
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
        PcurveGeometry::Nurbs { nurbs } => {
            nurbs_pcurve_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.pole_rows().count())
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
        }
        PcurveGeometry::PolarNurbs { nurbs } => {
            nurbs_pcurve_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.pole_rows().count())
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
    ctx: &DecodeContext<'_>,
) -> Result<[Option<[f64; 2]>; 2], CodecError> {
    let _depth = ctx.enter_nested_limit("STEP surface selection domain depth")?;
    let definition = ctx
        .find_map(
            &(index.ir().model.procedural_surfaces)[..],
            |procedural| -> Result<Option<_>, CodecError> {
                let mut owners = ctx
                    .admit_iter(
                        index.ir().model.surfaces.as_slice(),
                        "STEP procedural surface owner traversal",
                    )?
                    .map(|surface| -> Result<Option<_>, CodecError> {
                        Ok(ctx
                            .equal(
                                &surface.geometry.procedural_construction(),
                                &Some(&procedural.id),
                                "STEP procedural construction equality",
                            )?
                            .then_some(&surface.id))
                    })
                    .filter_map(Result::transpose);
                let owner = owners.next().transpose()?;
                let owner = if owner.is_some() && owners.next().transpose()?.is_some() {
                    None
                } else {
                    owner
                };
                Ok(ctx
                    .equal(
                        &owner,
                        &Some(surface_id),
                        "STEP procedural surface owner equality",
                    )?
                    .then_some(procedural))
            },
            "STEP surface selection parameter domains traversal",
        )?
        .map(cadmpeg_ir::geometry::ProceduralSurface::definition);
    Ok(match definition {
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
            curve_selection_parameter_domain(index, definition_payload.directrix(), ctx)?,
        ],
        Some(ProceduralSurfaceDefinition::Extrusion(payload)) => [
            curve_selection_parameter_domain(index, payload.directrix(), ctx)?,
            None,
        ],
        Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => [
            curve_selection_parameter_domain(index, definition_payload.directrix(), ctx)?,
            None,
        ],
        Some(ProceduralSurfaceDefinition::Replica { source, .. }) => match index
            .surfaces(source.as_str(), ctx)?
        {
            Some(source_surface) => {
                surface_selection_parameter_domains(index, source, &source_surface.geometry, ctx)?
            }
            None => [None, None],
        },
        _ => surface.solved().map_or(
            [None, None],
            surface_selection_parameter_domains_from_geometry,
        ),
    })
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<[f64; 2]>, ResourceLimit> {
    let Some(curve) = index.curves(curve_id.as_str(), ctx)? else {
        return Ok(None);
    };
    Ok(curve
        .geometry
        .solved()
        .and_then(curve_selection_parameter_domain_from_geometry))
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
            let upper = parameters
                .last()
                .map_or(lower, cadmpeg_ir::scalar::FiniteReal::get);
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
    typed: BTreeSet<u64>,
}

fn shell_defs(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, ShellDef>, CodecError> {
    let mut cache = BTreeMap::<u64, Option<ShellDef>>::new();
    let mut active = BTreeSet::new();
    for entity in exchange.entities_any(
        ctx,
        &[
            "ORIENTED_OPEN_SHELL",
            "ORIENTED_CLOSED_SHELL",
            "OPEN_SHELL",
            "CLOSED_SHELL",
        ],
    )? {
        let (id, _) = entity?;
        shell_def_cached(id, exchange, &mut active, &mut cache, ctx)?;
    }
    let mut shells = BTreeMap::new();
    for (id, definition) in cache {
        if let Some(definition) = definition {
            ctx.insert_btree_map(&mut shells, id, definition, "step_shell_definitions")?;
        }
    }
    Ok(shells)
}

fn copy_shell_def(definition: &ShellDef, ctx: &DecodeContext<'_>) -> Result<ShellDef, CodecError> {
    let typed = ctx.collect_btree_set(
        definition.typed.iter().copied(),
        "step_shell_definition_typed_copy",
    )?;
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
        return definition
            .as_ref()
            .map(|definition| copy_shell_def(definition, ctx))
            .transpose();
    }
    let _depth = ctx.enter_nested("step_shell_definition_recursion")?;
    if active.contains(&reference) {
        return Ok(None);
    }
    ctx.insert_btree_set(active, reference, "step_shell_definition_active")?;
    let result = if let Some(record) = exchange.records().get(&reference) {
        match most_specific(
            ctx,
            record,
            &[
                "ORIENTED_OPEN_SHELL",
                "ORIENTED_CLOSED_SHELL",
                "OPEN_SHELL",
                "CLOSED_SHELL",
            ],
        )? {
            Some("OPEN_SHELL" | "CLOSED_SHELL") => Some(ShellDef {
                base: reference,
                forward: true,
                typed: BTreeSet::new(),
            }),
            Some("ORIENTED_OPEN_SHELL" | "ORIENTED_CLOSED_SHELL") => {
                let shell_type = most_specific(
                    ctx,
                    record,
                    &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL"],
                )?;
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
                        shell_type
                            .map(|shell_type| named_reference(ctx, record, shell_type, 1, 0))
                            .transpose()?
                            .flatten(),
                        shell_type
                            .map(|shell_type| named_logical(ctx, record, shell_type, 2, 0))
                            .transpose()?
                            .flatten(),
                    )
                };
                if let Some((element, orientation)) = element.zip(orientation) {
                    if let Some(mut definition) =
                        shell_def_cached(element, exchange, active, cache, ctx)?
                    {
                        definition.forward = definition.forward == orientation;
                        ctx.insert_btree_set(
                            &mut definition.typed,
                            reference,
                            "step_shell_definition_typed",
                        )?;
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
    let cached = result
        .as_ref()
        .map(|definition| copy_shell_def(definition, ctx))
        .transpose()?;
    ctx.insert_btree_map(cache, reference, cached, "step_shell_definition_cache")?;
    Ok(result)
}

fn shell_def_for(
    reference: u64,
    shells: &BTreeMap<u64, ShellDef>,
    typed: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, bool)>, CodecError> {
    let Some(definition) = shells.get(&reference) else {
        return Ok(None);
    };
    for &id in ctx.admit_iter(&definition.typed, "STEP shell def for traversal")? {
        ctx.insert_btree_set(typed, id, "step_shell_definition_claims")?;
    }
    Ok(Some((definition.base, definition.forward)))
}

#[derive(Default)]
struct FaceInfo<'a> {
    bounds: Vec<u64>,
    name: Option<&'a Value>,
    surface: Option<u64>,
    same_sense: bool,
    reverse_bound_orientation: bool,
    typed: BTreeSet<u64>,
}

fn is_face_record(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<bool, CodecError> {
    for name in [
        "FACE",
        "ADVANCED_FACE",
        "FACE_SURFACE",
        "ORIENTED_FACE",
        "SUBFACE",
    ] {
        if record.partial(ctx, name)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn face_attributes<'a>(
    id: u64,
    record: &'a RawRecord,
    exchange: &'a Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FaceInfo<'a>>, CodecError> {
    let _depth = ctx.enter_nested("step_face_attribute_recursion")?;
    if active.contains(&id) {
        return Ok(None);
    }
    ctx.insert_btree_set(active, id, "step_face_attribute_active")?;
    let result = face_attributes_inner(id, record, exchange, active, ctx);
    active.remove(&id);
    result
}

fn face_attributes_inner<'a>(
    _id: u64,
    record: &'a RawRecord,
    exchange: &'a Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FaceInfo<'a>>, CodecError> {
    let Some(kind) = most_specific(
        ctx,
        record,
        &[
            "ORIENTED_FACE",
            "SUBFACE",
            "ADVANCED_FACE",
            "FACE_SURFACE",
            "FACE",
        ],
    )?
    else {
        return Ok(None);
    };
    let result = match kind {
        "ORIENTED_FACE" => {
            let Some(face_element) = oriented_face_element(ctx, record)? else {
                return Ok(None);
            };
            let Some(element_record) = exchange.records().get(&face_element) else {
                return Ok(None);
            };
            let Some(mut base) =
                face_attributes(face_element, element_record, exchange, active, ctx)?
            else {
                return Ok(None);
            };
            let Some(orientation) = oriented_face_orientation(ctx, record)? else {
                return Ok(None);
            };
            if !orientation {
                base.reverse_bound_orientation = !base.reverse_bound_orientation;
            }
            base.same_sense = base.same_sense == orientation;
            if let Some(name) = face_name_value(ctx, record)? {
                base.name = Some(name);
            }
            ctx.insert_btree_set(&mut base.typed, face_element, "step_face_attribute_typed")?;
            Some(base)
        }
        "SUBFACE" => {
            let Some(parent) = subface_parent(ctx, record)? else {
                return Ok(None);
            };
            let Some(parent_record) = exchange.records().get(&parent) else {
                return Ok(None);
            };
            let Some(mut parent_info) =
                face_attributes(parent, parent_record, exchange, active, ctx)?
            else {
                return Ok(None);
            };
            let Some(bounds) = direct_face_bounds(record, exchange, ctx)? else {
                return Ok(None);
            };
            ctx.insert_btree_set(&mut parent_info.typed, parent, "step_face_attribute_typed")?;
            if let Some(name) = face_name_value(ctx, record)? {
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
            let Some(bounds) = direct_face_bounds(record, exchange, ctx)? else {
                return Ok(None);
            };
            Some(FaceInfo {
                bounds,
                name: face_name_value(ctx, record)?,
                surface: None,
                same_sense: true,
                reverse_bound_orientation: false,
                typed: BTreeSet::new(),
            })
        }
        "ADVANCED_FACE" | "FACE_SURFACE" => {
            let Some(bounds) = direct_face_bounds(record, exchange, ctx)? else {
                return Ok(None);
            };
            let Some(governing) = most_specific(ctx, record, &["ADVANCED_FACE", "FACE_SURFACE"])?
            else {
                return Ok(None);
            };
            let Some(surface) = direct_face_surface(ctx, record, &bounds, governing)? else {
                return Ok(None);
            };
            let Some(same_sense) = direct_face_same_sense(ctx, record, governing)? else {
                return Ok(None);
            };
            Some(FaceInfo {
                bounds,
                name: face_name_value(ctx, record)?,
                surface: Some(surface),
                same_sense,
                reverse_bound_orientation: false,
                typed: BTreeSet::new(),
            })
        }
        _ => None,
    };
    Ok(result)
}

fn face_name_value<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    let mut value = if record.partials.len() == 1 {
        record.parameter(0)
    } else {
        None
    };
    if record.partials.len() != 1 {
        for name in [
            "REPRESENTATION_ITEM",
            "ORIENTED_FACE",
            "SUBFACE",
            "ADVANCED_FACE",
            "FACE_SURFACE",
            "FACE",
        ] {
            value = record
                .partial(ctx, name)?
                .and_then(|partial| partial.parameters.first());
            if value.is_some() {
                break;
            }
        }
    }
    Ok(value.filter(|value| !matches!(value, Value::String(bytes) if bytes.is_empty())))
}

fn direct_face_bounds(
    record: &RawRecord,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u64>>, CodecError> {
    let simple_value = if record.partials.len() == 1 {
        let Some(name) = record.simple_name() else {
            return Ok(None);
        };
        let Some(value) = entity_parameter(ctx, record, name, 1)? else {
            return Ok(None);
        };
        Some(value)
    } else {
        None
    };
    let decode_bounds = |value: &Value| -> Result<Option<Vec<u64>>, CodecError> {
        let Some(items) = value.list() else {
            return Ok(None);
        };
        if items.is_empty() {
            return Ok(None);
        }
        for item in ctx.admit_iter(items, "STEP face bound list validation")? {
            let Some(id) = item.reference() else {
                return Ok(None);
            };
            let Some(bound) = exchange.records().get(&id) else {
                return Ok(None);
            };
            if bound.partial(ctx, "FACE_BOUND")?.is_none()
                && bound.partial(ctx, "FACE_OUTER_BOUND")?.is_none()
            {
                return Ok(None);
            }
        }
        let mut bounds = Vec::new();
        for id in ctx
            .admit_iter(items, "STEP direct face bounds traversal")?
            .filter_map(ValueExt::reference)
        {
            ctx.push_vec(&mut bounds, id, "step_face_attribute_bounds")?;
        }
        Ok(Some(bounds))
    };
    if let Some(value) = simple_value {
        if let Some(bounds) = decode_bounds(value)? {
            return Ok(Some(bounds));
        }
    }
    for partial in ctx.admit_iter(&record.partials[..], "STEP face bound partial traversal")? {
        if record.partials.len() == 1 {
            continue;
        }
        for value in ctx.admit_iter(
            partial.parameters.as_slice(),
            "STEP face bound parameter traversal",
        )? {
            if let Some(bounds) = decode_bounds(value)? {
                return Ok(Some(bounds));
            }
        }
    }
    Ok(None)
}

fn direct_face_surface(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    bounds: &[u64],
    governing: &str,
) -> Result<Option<u64>, CodecError> {
    if record.partials.len() > 1 {
        if let Some(value) = named_reference(ctx, record, governing, 2, 0)? {
            return Ok(Some(value));
        }
        return if governing == "ADVANCED_FACE" {
            named_reference(ctx, record, "FACE_SURFACE", 2, 0)
        } else {
            Ok(None)
        };
    }
    for partial in ctx.admit_iter(&record.partials[..], "STEP direct face partial traversal")? {
        if let Some(value) = ctx.find_map(
            partial.parameters.as_slice(),
            |raw| {
                let Some(reference) = ValueExt::reference(raw) else {
                    return Ok(None);
                };
                Ok((!bounds.contains(&reference)).then_some(reference))
            },
            "STEP direct face parameter traversal",
        )? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn direct_face_same_sense(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    governing: &str,
) -> Result<Option<bool>, CodecError> {
    if record.partials.len() > 1 {
        if let Some(value) = named_logical(ctx, record, governing, 3, 0)? {
            return Ok(Some(value));
        }
        return if governing == "ADVANCED_FACE" {
            named_logical(ctx, record, "FACE_SURFACE", 3, 0)
        } else {
            Ok(None)
        };
    }
    for partial in ctx.admit_iter(&record.partials[..], "STEP direct face partial traversal")? {
        if let Some(value) = ctx.find_map(
            partial.parameters.as_slice(),
            |value| Ok(ValueExt::logical(value)),
            "STEP direct face parameter traversal",
        )? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn oriented_face_element(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    if let Some(partial) = record.partial(ctx, "ORIENTED_FACE")? {
        return ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP oriented face element traversal",
        );
    }
    for partial in ctx
        .admit_iter(
            &record.partials[..],
            "STEP oriented face fallback partial traversal",
        )?
        .rev()
    {
        if let Some(element) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP oriented face fallback element traversal",
        )? {
            return Ok(Some(element));
        }
    }
    Ok(None)
}

fn oriented_face_orientation(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<bool>, CodecError> {
    if let Some(partial) = record.partial(ctx, "ORIENTED_FACE")? {
        if let Some(forward) = ctx.find_map(
            partial.parameters.as_slice(),
            |value| Ok(ValueExt::logical(value)),
            "STEP oriented face orientation parameter traversal",
        )? {
            return Ok(Some(forward));
        }
    }
    direct_face_same_sense(ctx, record, "ORIENTED_FACE")
}

fn subface_parent(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<u64>, CodecError> {
    if let Some(partial) = record.partial(ctx, "SUBFACE")? {
        if let Some(parent) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP subface parent traversal",
        )? {
            return Ok(Some(parent));
        }
    }
    for partial in ctx
        .admit_iter(
            &record.partials[..],
            "STEP subface fallback partial traversal",
        )?
        .rev()
    {
        if let Some(parent) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP subface fallback parent traversal",
        )? {
            return Ok(Some(parent));
        }
    }
    Ok(None)
}

/// Selects the partial that carries inherited `FACE_BOUND` attributes.
/// `FACE_OUTER_BOUND` adds the outer role but may be empty in a complex
/// instance, so subtype classification and attribute lookup are separate.
fn face_bound_attribute_type(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<&'static str>, CodecError> {
    for (name, minimum_parameters) in [
        ("FACE_OUTER_BOUND", 3),
        ("FACE_BOUND", 3),
        ("FACE_BOUND", 0),
        ("FACE_OUTER_BOUND", 0),
    ] {
        if record
            .partial(ctx, name)?
            .is_some_and(|partial| partial.parameters.len() >= minimum_parameters)
        {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

/// Returns the first partial name present in a subtype-first dispatch chain.
/// Complex STEP instances carry every inherited partial, so the first hit is
/// the governing subtype and its attributes must drive decoding.
fn most_specific<'a>(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    chain: &[&'a str],
) -> Result<Option<&'a str>, CodecError> {
    for name in ctx.admit_iter(chain, "STEP topology subtype dispatch traversal")? {
        if ctx
            .find_map(
                &record.partials[..],
                |partial| -> Result<Option<_>, CodecError> {
                    Ok(
                        (ctx.equal(partial.name.as_str(), name, "STEP most specific equality")?)
                            .then_some(()),
                    )
                },
                "STEP topology subtype partial traversal",
            )?
            .is_some()
        {
            return Ok(Some(*name));
        }
    }
    Ok(None)
}

fn connected_face_set_type(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<&'static str>, CodecError> {
    most_specific(
        ctx,
        record,
        &["CONNECTED_FACE_SUB_SET", "CONNECTED_FACE_SET"],
    )
}

fn connected_set_members<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    set_type: &'static str,
) -> Result<Option<&'a [Value]>, CodecError> {
    let base_type = match set_type {
        "CONNECTED_EDGE_SUB_SET" => "CONNECTED_EDGE_SET",
        "CONNECTED_FACE_SUB_SET" => "CONNECTED_FACE_SET",
        _ => set_type,
    };
    if let Some(members) = named_reference_values(ctx, record, set_type, 1)? {
        return Ok(Some(members));
    }
    named_reference_values(ctx, record, base_type, 1)
}

fn validate_subset_parent(
    id: u64,
    record: &RawRecord,
    subset_type: &'static str,
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
        entity_parameter(ctx, record, subset_type, 2)?.and_then(ValueExt::reference)
    } else {
        record
            .partial(ctx, subset_type)?
            .map(|partial| {
                ctx.find_map(
                    partial.parameters.as_slice(),
                    |value| Ok(ValueExt::reference(value)),
                    "STEP topology reference parameter traversal",
                )
            })
            .transpose()?
            .flatten()
    };
    let Some(parent) = parent else {
        ctx.push_vec(
            losses,
            StepLossCode::DecodeWarning.note(ctx.format_retained(
                format_args!("{subset_type} #{id} has no resolvable parent {base_type}"),
                "STEP validate_subset_parent text",
            )?),
            "step_topology_losses",
        )?;
        return Ok(false);
    };
    let valid_parent = if let Some(parent_record) = exchange.records().get(&parent) {
        ctx.equal(
            &most_specific(ctx, parent_record, &[base_type])?,
            &Some(base_type),
            "STEP subset parent type equality",
        )?
    } else {
        false
    };
    if valid_parent {
        Ok(true)
    } else {
        ctx.push_vec(
            losses,
            StepLossCode::DecodeWarning.note(ctx.format_retained(
                format_args!(
                    "{subset_type} #{id} parent #{parent} does not resolve to {base_type}"
                ),
                "STEP validate_subset_parent text",
            )?),
            "step_topology_losses",
        )?;
        Ok(false)
    }
}

fn entity_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    name: &str,
    index: usize,
) -> Result<Option<&'a Value>, CodecError> {
    Ok(ctx
        .find_map(
            &record.partials[..],
            |partial| -> Result<Option<_>, CodecError> {
                Ok((ctx.equal(
                    partial.name.as_str(),
                    name,
                    "STEP entity parameter equality",
                )?)
                .then_some(partial))
            },
            "STEP topology entity parameter partial traversal",
        )?
        .or_else(|| (record.partials.len() == 1).then(|| &record.partials[0]))
        .and_then(|partial| partial.parameters.get(index)))
}
