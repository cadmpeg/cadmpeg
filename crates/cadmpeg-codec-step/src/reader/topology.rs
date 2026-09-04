// SPDX-License-Identifier: Apache-2.0
//! STEP boundary-representation ownership and orientation decoding.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{alloc_filled, DecodeContext};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{CommitSession, DraftError, ModelDraft};
use cadmpeg_ir::eval::{
    model_curve_parameter_near_point_in_index_with_tolerance, model_curve_point_by_id,
    model_surface_partials_by_id, model_surface_point_by_id, nurbs_curve_parameter_domain,
    pcurve_tangent, pcurve_uv,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, PcurveGeometry, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, PcurveUse, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::COINCIDENCE_TOLERANCE;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use self::admissions::{pcurve_admission_note, PcurveAdmission};
use super::geometry::surface_parameter_periods;
use super::index::CarrierIndex;
use super::StageOutcome;

const EPS_TOPOLOGY_READ_DEGENERATE: f64 = 1.0e-10;
const EPS_TOPOLOGY_READ_EXACT_GEOMETRY: f64 = 1.0e-12;

mod admissions;

pub(super) struct TopologyData {
    pub body_by_root: BTreeMap<u64, Vec<BodyId>>,
    shape_representation_relationships: BTreeMap<u64, Vec<u64>>,
    pub body_by_shell: BTreeMap<u64, BTreeSet<BodyId>>,
    pub faces_by_source: BTreeMap<u64, Vec<FaceId>>,
    pub edges_by_source: BTreeMap<u64, Vec<EdgeId>>,
    pub vertices_by_source: BTreeMap<u64, Vec<VertexId>>,
}

fn topology_commit_error(context: &str, error: &DraftError) -> String {
    match error {
        DraftError::IdentityCollision(identity) => format!(
            "{context} conflicts with decoded topology: identity collision at '{identity}': {error}"
        ),
        _ => format!("{context} conflicts with decoded topology: {error}"),
    }
}

/// Resolve the bodies represented by a representation item graph.
///
/// A representation can contain a body root directly or contain mapped items
/// whose representation map points at another representation.  Keep this
/// traversal shared by topology classification and product placement so both
/// consumers apply the same graph and cycle rules.
pub(super) fn representation_bodies(
    representation: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    cache: &mut BTreeMap<u64, Vec<BodyId>>,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: Option<&DecodeContext<'_>>,
) -> Vec<BodyId> {
    if let Some(bodies) = cache.get(&representation) {
        return bodies.clone();
    }
    if depth >= super::record_graph_limit(ctx) {
        return Vec::new();
    }
    if let Some(bodies) = topology.body_by_root.get(&representation) {
        let bodies = bodies.clone();
        cache.insert(representation, bodies.clone());
        return bodies;
    }
    if !active.insert(representation) {
        return Vec::new();
    }
    let mut body_ids = BTreeSet::new();
    if let Some(items) = exchange
        .records
        .get(&representation)
        .and_then(representation_items)
    {
        for item in items {
            let Some(record) = exchange.records.get(&item) else {
                continue;
            };
            if let Some(bodies) = topology.body_by_root.get(&item) {
                body_ids.extend(bodies.iter().cloned());
                continue;
            }
            if !has_type(record, "MAPPED_ITEM") {
                continue;
            }
            let Some(mapped_representation) = mapped_representation(record, exchange) else {
                continue;
            };
            body_ids.extend(representation_bodies(
                mapped_representation,
                exchange,
                topology,
                cache,
                active,
                depth + 1,
                ctx,
            ));
        }
    }
    for related in topology
        .shape_representation_relationships
        .get(&representation)
        .into_iter()
        .flatten()
        .copied()
    {
        body_ids.extend(representation_bodies(
            related,
            exchange,
            topology,
            cache,
            active,
            depth + 1,
            ctx,
        ));
    }
    let bodies = body_ids.into_iter().collect::<Vec<_>>();
    active.remove(&representation);
    cache.insert(representation, bodies.clone());
    bodies
}

/// A product shape can use a placement-only `SHAPE_REPRESENTATION` and link
/// it to the body-producing representation with `SHAPE_REPRESENTATION_RELATIONSHIP`.
/// The relationship is undirected for body reachability; retain both endpoints
/// in one indexed graph so resolution does not rescan the exchange per call.
fn shape_representation_relationships(exchange: &Exchange) -> BTreeMap<u64, Vec<u64>> {
    let mut related = BTreeMap::<u64, Vec<u64>>::new();
    for record in exchange.records.values() {
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
        related.entry(first).or_default().push(second);
        related.entry(second).or_default().push(first);
    }
    for representations in related.values_mut() {
        representations.sort_unstable();
        representations.dedup();
    }
    related
}

fn representation_items(record: &RawRecord) -> Option<Vec<u64>> {
    named_refs(record, "REPRESENTATION", 1).or_else(|| {
        record
            .simple_name()
            .and_then(|name| named_refs(record, name, 1))
    })
}

fn mapped_representation(record: &RawRecord, exchange: &Exchange) -> Option<u64> {
    let map = named_reference(record, "MAPPED_ITEM", 1, 0)?;
    exchange
        .records
        .get(&map)
        .and_then(|map| named_reference(map, "REPRESENTATION_MAP", 1, 1))
}

pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    carrier_index: &CarrierIndex,
    ctx: Option<&DecodeContext<'_>>,
) -> StageOutcome<TopologyData> {
    let mut commit_session = CommitSession::new(ir);
    let mut result = StageOutcome {
        value: TopologyData {
            body_by_root: BTreeMap::new(),
            shape_representation_relationships: shape_representation_relationships(exchange),
            body_by_shell: BTreeMap::new(),
            faces_by_source: BTreeMap::new(),
            edges_by_source: BTreeMap::new(),
            vertices_by_source: BTreeMap::new(),
        },
        claims: HashSet::new(),
        warnings: Vec::new(),
        losses: Vec::new(),
        notes: Vec::new(),
    };
    for record in exchange.records.values() {
        let Some(name) = most_specific(record, &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL"])
        else {
            continue;
        };
        if record.partials.len() != 1 || matches!(record.parameter(1), Some(Value::Derived)) {
            continue;
        }
        result.losses.push(
            StepLossCode::OrientedShellOmitsCfsFaces
                .note(format!(
                    "{name} #{} omits the derived `cfs_faces` slot required by ISO 10303-21; \
                 read the shell element from positional slot 1",
                    record.id
                ))
                .with_provenance(
                    cadmpeg_ir::SourceProvenance::root(
                        crate::dialect::FORMAT,
                        record.span.start as u64,
                    )
                    .with_tag("oriented_shell"),
                ),
        );
    }
    let vertices = vertex_defs(exchange);
    let edges = edge_defs(exchange);
    let oriented = oriented_defs(exchange);
    let shells = shell_defs(exchange);
    let point_positions = carrier_index;
    for (vertex_id, vertex) in exchange.entities("VERTEX_POINT") {
        let Some(point_id) = named_reference(vertex, "VERTEX_POINT", 1, 0) else {
            result.warnings.push(format!(
                "VERTEX_POINT #{vertex_id} has no resolvable point carrier"
            ));
            continue;
        };
        if !carrier_index.points.contains_key(&point_id) {
            result.warnings.push(format!(
                "VERTEX_POINT #{vertex_id} has unresolved point carrier #{point_id}"
            ));
        }
    }
    let wire_models = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            representation_items(record).map(|items| {
                items
                    .into_iter()
                    .filter(|model| {
                        exchange
                            .records
                            .get(model)
                            .is_some_and(|record| has_type(record, "EDGE_BASED_WIREFRAME_MODEL"))
                    })
                    .map(move |model| (id, model))
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect::<Vec<_>>();
    let mut built_wire_models = BTreeSet::new();
    for (representation, model) in wire_models {
        if built_wire_models.contains(&model) {
            result.claims.insert(representation);
            if let Some(body_ids) = result.body_by_root.get(&model).cloned() {
                result.body_by_root.insert(representation, body_ids);
            }
            continue;
        }
        let outcome = build_wire(
            model,
            exchange,
            &vertices,
            &edges,
            point_positions,
            &mut result.warnings,
        );
        let mut committed = 0;
        for mut built in outcome.built {
            if let Err(error) = commit_session.commit_model(built.draft, ir) {
                result.warnings.push(topology_commit_error(
                    &format!("EDGE_BASED_WIREFRAME_MODEL #{model}"),
                    &error,
                ));
            } else {
                committed += 1;
                built_wire_models.insert(model);
                built.typed.insert(representation);
                result
                    .body_by_root
                    .entry(model)
                    .or_default()
                    .push(built.body_id.clone());
                result.claims.extend(std::mem::take(&mut built.typed));
            }
        }
        if committed == 0 {
            result.warnings.push(format!(
                "EDGE_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
            ));
        } else if outcome.failed != 0 {
            result.warnings.push(format!(
                "EDGE_BASED_WIREFRAME_MODEL #{model} omitted {} unresolved connected edge set(s)",
                outcome.failed
            ));
        }
    }
    for (model, record) in exchange.entities("SHELL_BASED_WIREFRAME_MODEL") {
        let scope_root = named_refs(record, "SHELL_BASED_WIREFRAME_MODEL", 1)
            .into_iter()
            .flatten()
            .any(|shell| result.body_by_shell.contains_key(&shell));
        let outcome = build_shell_wire(
            model,
            exchange,
            &vertices,
            &edges,
            point_positions,
            scope_root,
            &mut result.warnings,
        );
        let mut committed = 0;
        for mut built in outcome.built {
            if let Err(error) = commit_session.commit_model(built.draft, ir) {
                result.warnings.push(topology_commit_error(
                    &format!("SHELL_BASED_WIREFRAME_MODEL #{model}"),
                    &error,
                ));
            } else {
                committed += 1;
                for shell in &built.shell_sources {
                    result
                        .body_by_shell
                        .entry(*shell)
                        .or_default()
                        .insert(built.body_id.clone());
                }
                result
                    .body_by_root
                    .entry(model)
                    .or_default()
                    .push(built.body_id.clone());
                result.claims.extend(std::mem::take(&mut built.typed));
            }
        }
        if committed == 0 {
            result.warnings.push(format!(
                "SHELL_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
            ));
        } else if outcome.failed != 0 {
            result.warnings.push(format!(
                "SHELL_BASED_WIREFRAME_MODEL #{model} omitted {} unresolved wire shell(s)",
                outcome.failed
            ));
        }
    }
    let decoded_pcurves = ir
        .model
        .pcurves
        .iter()
        .map(|pcurve| pcurve.id.clone())
        .collect::<BTreeSet<_>>();
    let topology_root_types = [
        "SHELL_BASED_SURFACE_MODEL",
        "FACE_BASED_SURFACE_MODEL",
        "FACETED_BREP",
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
    ];
    let distinct_root_count = exchange
        .entities_any(&topology_root_types)
        .filter_map(|(_, record)| root_key(record, exchange, &shells))
        .collect::<BTreeSet<_>>()
        .len();
    let scope_distinct_roots = distinct_root_count > 1;
    let mut built_roots = BTreeMap::<RootKey, RootBuilt>::new();
    let mut representation_cache = BTreeMap::new();
    let mut admissions: Vec<PcurveAdmission> = Vec::new();
    for (id, record) in exchange.entities_any(&topology_root_types) {
        let Some(key) = root_key(record, exchange, &shells) else {
            result.warnings.push(format!(
                "STEP topology root #{id} does not resolve to a complete connected topology graph",
            ));
            continue;
        };
        if let Some(root_built) = built_roots.get(&key).cloned() {
            result.claims.insert(id);
            result.body_by_root.insert(id, root_built.body_ids.clone());
            for (shell, body_ids) in root_built.body_by_shell {
                result
                    .body_by_shell
                    .entry(shell)
                    .or_default()
                    .extend(body_ids);
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
            ir,
            &vertices,
            &edges,
            &oriented,
            &shells,
            &decoded_pcurves,
            point_positions,
            scope_root,
            &mut result.warnings,
            &mut result.losses,
        );
        let failure_message = outcome.failure.as_ref().map(BuildFailure::message);
        let mut body_ids = Vec::new();
        let mut body_by_shell = BTreeMap::<u64, BTreeSet<BodyId>>::new();
        for mut built in outcome.built {
            drop_committed_surfaces(&mut built.draft, &commit_session, ir);
            if let Err(error) = commit_session.commit_model(built.draft, ir) {
                result.warnings.push(topology_commit_error(
                    &format!("STEP topology root #{id}"),
                    &error,
                ));
            } else {
                for shell in &built.shell_sources {
                    result
                        .body_by_shell
                        .entry(*shell)
                        .or_default()
                        .insert(built.body_id.clone());
                    body_by_shell
                        .entry(*shell)
                        .or_default()
                        .insert(built.body_id.clone());
                }
                body_ids.push(built.body_id.clone());
                result.claims.extend(std::mem::take(&mut built.typed));
                // A rejected draft transfers no relation, so only a committed
                // body contributes its admitted relations to the document.
                admissions.extend(std::mem::take(&mut built.pcurve_admissions));
            }
        }
        if body_ids.is_empty() {
            if let Some(message) = failure_message {
                result.losses.push(
                    StepLossCode::TopologyRootRejected
                        .note(format!("STEP topology root #{id} rejected: {message}")),
                );
            } else {
                result.losses.push(StepLossCode::TopologyRootIncomplete.note(format!(
                        "STEP topology root #{id} does not resolve to a complete connected topology graph",
                    )));
            }
        } else {
            result.body_by_root.insert(id, body_ids.clone());
            built_roots.insert(
                key,
                RootBuilt {
                    body_ids,
                    body_by_shell,
                },
            );
            if outcome.failed != 0 {
                let detail = failure_message
                    .as_deref()
                    .map_or_else(String::new, |message| format!(": {message}"));
                result.warnings.push(format!(
                    "STEP topology root #{id} omitted {} unresolved shell(s){detail}",
                    outcome.failed,
                ));
            }
        }
    }
    // Every admitted relation shares one class of unproved invariant, so the
    // document reports the class once with its count and named examples.
    result.losses.extend(pcurve_admission_note(&admissions));
    for (id, record) in exchange.entities("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION") {
        let omitted = geometric_set_omissions(record, exchange, carrier_index);
        if !omitted.is_empty() {
            result.warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} omitted unsupported or unresolved member(s): {}",
                omitted
                    .iter()
                    .map(|member| format!("#{member}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let Some(mut built) =
            build_geometric_set(id, record, exchange, carrier_index, &mut result.warnings)
        else {
            if mark_standalone_geometric_set(
                id,
                record,
                exchange,
                carrier_index,
                &mut result.claims,
            ) {
                continue;
            }
            result.warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no decoded bounded surfaces"
            ));
            continue;
        };
        if let Err(error) = commit_session.commit_model(built.draft, ir) {
            result.warnings.push(topology_commit_error(
                &format!("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id}"),
                &error,
            ));
        } else {
            result.body_by_root.insert(id, vec![built.body_id.clone()]);
            result.claims.extend(std::mem::take(&mut built.typed));
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
        let omitted = geometric_set_omissions(record, exchange, carrier_index);
        if !omitted.is_empty() {
            result.warnings.push(format!(
                "{} #{id} omitted unsupported or unresolved member(s): {}",
                representation_type,
                omitted
                    .iter()
                    .map(|member| format!("#{member}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        mark_standalone_geometric_set(id, record, exchange, carrier_index, &mut result.claims);
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
            ctx,
        )
        .is_empty();
        if has_body {
            result.claims.insert(id);
        }
    }
    for face in &ir.model.faces {
        if let Some(source) = source_numeric_id(&face.id.as_str(), "face") {
            result
                .faces_by_source
                .entry(source)
                .or_default()
                .push(face.id.clone());
        }
    }
    for edge in &ir.model.edges {
        if let Some(source) = source_numeric_id(&edge.id.as_str(), "edge") {
            result
                .edges_by_source
                .entry(source)
                .or_default()
                .push(edge.id.clone());
        }
    }
    for vertex in &ir.model.vertices {
        if let Some(source) = source_numeric_id(&vertex.id.as_str(), "vertex") {
            result
                .vertices_by_source
                .entry(source)
                .or_default()
                .push(vertex.id.clone());
        }
    }
    result
}

fn source_numeric_id(identity: &str, kind: &str) -> Option<u64> {
    let suffix = identity.strip_prefix(&format!("step:data:{kind}#"))?;
    let suffix = suffix.strip_prefix("poly-point-").unwrap_or(suffix);
    suffix.split('-').next()?.parse().ok()
}

fn geometric_set_omissions(
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
) -> Vec<u64> {
    let Some(set_ids) = representation_items(representation) else {
        return Vec::new();
    };
    set_ids
        .into_iter()
        .filter_map(|set_id| exchange.records.get(&set_id))
        .filter_map(|set| {
            let set_type = most_specific(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])?;
            Some(named_refs(set, set_type, 1).unwrap_or_default())
        })
        .flatten()
        .filter(|member| {
            !carrier_index.points.contains_key(member)
                && !carrier_index.curves.contains_key(member)
                && !carrier_index.surfaces.contains_key(member)
        })
        .collect()
}

struct BuildOutcome {
    built: Vec<Built>,
    failed: usize,
    failure: Option<BuildFailure>,
}

#[derive(Clone, Debug)]
struct BuildFailure {
    record_id: u64,
    carrier_kind: &'static str,
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
    carrier_kind: &'static str,
) -> Option<T> {
    if value.is_none() {
        failure.get_or_insert(BuildFailure {
            record_id,
            carrier_kind,
        });
    }
    value
}

fn note_failure(failure: &mut Option<BuildFailure>, record_id: u64, carrier_kind: &'static str) {
    failure.get_or_insert(BuildFailure {
        record_id,
        carrier_kind,
    });
}

fn build_wire(
    id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    point_positions: &CarrierIndex,
    warnings: &mut Vec<String>,
) -> BuildOutcome {
    let Some(model) = exchange.records.get(&id) else {
        return BuildOutcome {
            built: Vec::new(),
            failed: 1,
            failure: None,
        };
    };
    let Some(sets) = named_refs(model, "EDGE_BASED_WIREFRAME_MODEL", 1) else {
        return BuildOutcome {
            built: Vec::new(),
            failed: 1,
            failure: None,
        };
    };
    let scoped = sets.len() > 1;
    let mut built = Vec::new();
    let mut failed = 0;
    for set_id in sets {
        match build_wire_set(
            id,
            set_id,
            exchange,
            vdefs,
            edefs,
            point_positions,
            scoped,
            warnings,
        ) {
            Some(value) => built.push(value),
            None => failed += 1,
        }
    }
    BuildOutcome {
        built,
        failed,
        failure: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_wire_set(
    id: u64,
    set_id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    point_positions: &CarrierIndex,
    scoped: bool,
    warnings: &mut Vec<String>,
) -> Option<Built> {
    let set = exchange.records.get(&set_id)?;
    let set_type = most_specific(set, &["CONNECTED_EDGE_SUB_SET", "CONNECTED_EDGE_SET"])?;
    let used_edges = connected_set_members(set, set_type)?;
    if used_edges.is_empty() {
        return None;
    }
    let suffix = if scoped {
        format!("-set-{set_id}")
    } else {
        String::new()
    };
    let mut typed = HashSet::from([id, set_id]);
    if set_type == "CONNECTED_EDGE_SUB_SET"
        && !validate_subset_parent(set, set_type, exchange, warnings)
    {
        typed.remove(&set_id);
    }
    let mut used_vertices = BTreeSet::new();
    let mut wire_edges = Vec::new();
    let mut built_edges = Vec::new();
    for edge_id in used_edges {
        let edge = edefs.get(&edge_id)?;
        let (start, end) = if edge.same {
            (edge.start, edge.end)
        } else {
            (edge.end, edge.start)
        };
        let edge_suffix = format!("-wire-{id}-set-{set_id}");
        let ir_id = EdgeId::mint(StepIdentity::data(
            "edge",
            format!("{edge_id}{edge_suffix}"),
        ))
        .expect("identity grammar");
        let vertex_suffix = format!("-wire-{id}-set-{set_id}");
        wire_edges.push(ir_id.clone());
        built_edges.push(Edge {
            id: ir_id,
            curve: edge_curve_id_reported(edge_id, edge, exchange, warnings),
            start: VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{start}{vertex_suffix}"),
            ))
            .expect("identity grammar"),
            end: VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{end}{vertex_suffix}"),
            ))
            .expect("identity grammar"),
            param_range: None,
            tolerance: None,
        });
        used_vertices.extend([start, end]);
        typed.insert(edge_id);
        if let Some(parent) = edge.parent {
            typed.insert(parent);
        }
    }
    let vertex_suffix = format!("-wire-{id}-set-{set_id}");
    let mut built_vertices = Vec::new();
    for vertex_id in used_vertices {
        let vertex = vdefs.get(&vertex_id)?;
        point_positions.get(vertex.point)?;
        built_vertices.push(Vertex {
            id: VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{vertex_id}{vertex_suffix}"),
            ))
            .expect("identity grammar"),
            point: PointId::mint(StepIdentity::data("point", vertex.point))
                .expect("identity grammar"),
            tolerance: None,
        });
        typed.insert(vertex_id);
    }
    let body = BodyId::mint(StepIdentity::data("body", format!("{id}{suffix}")))
        .expect("identity grammar");
    let region = RegionId::mint(StepIdentity::data("region", format!("{id}{suffix}")))
        .expect("identity grammar");
    let shell = ShellId::mint(StepIdentity::data("shell", format!("{id}{suffix}")))
        .expect("identity grammar");
    let mut built = staged_topology(
        typed,
        built_vertices,
        built_edges,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![Shell {
            id: shell.clone(),
            region: region.clone(),
            faces: Vec::new(),
            wire_edges,
            free_vertices: Vec::new(),
        }],
        Region {
            id: region.clone(),
            body: body.clone(),
            shells: vec![shell],
        },
        Body {
            id: body.clone(),
            kind: BodyKind::Wire,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    )?;
    built.shell_sources.insert(set_id);
    Some(built)
}

fn build_shell_wire(
    id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    point_positions: &CarrierIndex,
    scope_root: bool,
    warnings: &mut Vec<String>,
) -> BuildOutcome {
    let Some(model) = exchange.records.get(&id) else {
        return BuildOutcome {
            built: Vec::new(),
            failed: 1,
            failure: None,
        };
    };
    let Some(shell_ids) = named_refs(model, "SHELL_BASED_WIREFRAME_MODEL", 1) else {
        return BuildOutcome {
            built: Vec::new(),
            failed: 1,
            failure: None,
        };
    };
    let scoped = shell_ids.len() > 1;
    let mut built = Vec::new();
    let mut failed = 0;
    for shell_id in shell_ids {
        match build_shell_wire_set(
            id,
            shell_id,
            exchange,
            vdefs,
            edefs,
            point_positions,
            scoped,
            scope_root,
            warnings,
        ) {
            Some(value) => built.push(value),
            None => failed += 1,
        }
    }
    BuildOutcome {
        built,
        failed,
        failure: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_shell_wire_set(
    id: u64,
    shell_id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    point_positions: &CarrierIndex,
    scoped: bool,
    scope_root: bool,
    warnings: &mut Vec<String>,
) -> Option<Built> {
    let shell_record = exchange.records.get(&shell_id)?;
    let mut typed = HashSet::from([id, shell_id]);
    let mut edge_uses = Vec::new();
    let mut used_vertices = BTreeSet::new();
    let mut free_vertices = BTreeSet::new();
    if has_type(shell_record, "WIRE_SHELL") {
        for loop_id in named_refs(shell_record, "WIRE_SHELL", 1)? {
            let loop_record = exchange.records.get(&loop_id)?;
            if has_type(loop_record, "EDGE_LOOP") {
                for oriented_id in named_refs(loop_record, "EDGE_LOOP", 1)? {
                    let oriented = exchange.records.get(&oriented_id)?;
                    let edge_id = oriented_edge_reference(oriented)?;
                    let edge = edefs.get(&edge_id)?;
                    let forward = oriented_edge_forward(oriented)?;
                    edge_uses.push((edge_id, oriented_id, forward));
                    used_vertices.extend([edge.start, edge.end]);
                    typed.extend([loop_id, oriented_id, edge_id]);
                    if let Some(parent) = edge.parent {
                        typed.insert(parent);
                    }
                }
            } else if has_type(loop_record, "VERTEX_LOOP") {
                let vertex = named_reference(loop_record, "VERTEX_LOOP", 1, 0)?;
                used_vertices.insert(vertex);
                free_vertices.insert(vertex);
                typed.extend([loop_id, vertex]);
            } else {
                return None;
            }
        }
    } else if has_type(shell_record, "VERTEX_SHELL") {
        let loop_id = named_reference(shell_record, "VERTEX_SHELL", 1, 0)?;
        let loop_record = exchange.records.get(&loop_id)?;
        if !has_type(loop_record, "VERTEX_LOOP") {
            return None;
        }
        let vertex = named_reference(loop_record, "VERTEX_LOOP", 1, 0)?;
        used_vertices.insert(vertex);
        free_vertices.insert(vertex);
        typed.extend([loop_id, vertex]);
    } else {
        return None;
    }
    if edge_uses.is_empty() && used_vertices.is_empty() {
        return None;
    }
    let suffix = if scoped {
        format!("-shell-{shell_id}")
    } else {
        String::new()
    };
    let vertex_suffix = format!("-wire-{id}-shell-{shell_id}");
    let mut edges = Vec::new();
    let mut wire_edges = Vec::new();
    for (index, (edge_id, oriented_id, forward)) in edge_uses.into_iter().enumerate() {
        let edge = edefs.get(&edge_id)?;
        let (curve_start, curve_end) = if edge.same {
            (edge.start, edge.end)
        } else {
            (edge.end, edge.start)
        };
        let (start, end) = if forward {
            (curve_start, curve_end)
        } else {
            (curve_end, curve_start)
        };
        let ir_id = EdgeId::mint(StepIdentity::data(
            "edge",
            format!("{edge_id}-wire-{id}-{shell_id}-{oriented_id}-{index}"),
        ))
        .expect("identity grammar");
        wire_edges.push(ir_id.clone());
        edges.push(Edge {
            id: ir_id,
            curve: edge_curve_id_reported(edge_id, edge, exchange, warnings),
            start: VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{start}{vertex_suffix}"),
            ))
            .expect("identity grammar"),
            end: VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{end}{vertex_suffix}"),
            ))
            .expect("identity grammar"),
            param_range: None,
            tolerance: None,
        });
    }
    let vertices = used_vertices
        .into_iter()
        .map(|vertex_id| {
            let vertex = vdefs.get(&vertex_id)?;
            point_positions.get(vertex.point)?;
            Some(Vertex {
                id: VertexId::mint(StepIdentity::data(
                    "vertex",
                    format!("{vertex_id}{vertex_suffix}"),
                ))
                .expect("identity grammar"),
                point: PointId::mint(StepIdentity::data("point", vertex.point))
                    .expect("identity grammar"),
                tolerance: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let body = BodyId::mint(StepIdentity::data("body", format!("{id}{suffix}")))
        .expect("identity grammar");
    let region = RegionId::mint(StepIdentity::data("region", format!("{id}{suffix}")))
        .expect("identity grammar");
    let shell = shell_identity(id, shell_id, scope_root);
    let free_vertices = free_vertices
        .into_iter()
        .map(|vertex| {
            VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{vertex}{vertex_suffix}"),
            ))
            .expect("identity grammar")
        })
        .collect();
    let mut built = staged_topology(
        typed,
        vertices,
        edges,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![Shell {
            id: shell.clone(),
            region: region.clone(),
            faces: Vec::new(),
            wire_edges,
            free_vertices,
        }],
        Region {
            id: region.clone(),
            body: body.clone(),
            shells: vec![shell],
        },
        Body {
            id: body.clone(),
            kind: BodyKind::Wire,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    )?;
    built.shell_sources.insert(shell_id);
    Some(built)
}

fn mark_standalone_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    typed: &mut HashSet<u64>,
) -> bool {
    let Some(set_ids) = representation_items(representation) else {
        return false;
    };
    let mut decoded = false;
    for set_id in set_ids {
        let Some(set) = exchange.records.get(&set_id) else {
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
            typed.insert(set_id);
            decoded = true;
        }
    }
    if decoded {
        typed.insert(id);
    }
    decoded
}

fn build_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    carrier_index: &CarrierIndex,
    warnings: &mut Vec<String>,
) -> Option<Built> {
    let set_ids = representation_items(representation)?;
    let mut typed = HashSet::from([id]);
    let mut surfaces = Vec::new();
    for set_id in set_ids {
        let Some(set) = exchange.records.get(&set_id) else {
            warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped missing set #{set_id}"
            ));
            continue;
        };
        let Some(set_type) = most_specific(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped non-set member #{set_id}"
            ));
            continue;
        };
        let Some(items) = named_refs(set, set_type, 1) else {
            warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} skipped set #{set_id} with no member list"
            ));
            continue;
        };
        typed.insert(set_id);
        for surface_step in items {
            let surface = SurfaceId::mint(StepIdentity::data("surface", surface_step))
                .expect("identity grammar");
            if carrier_index.surfaces.contains_key(&surface_step) {
                surfaces.push((surface_step, surface));
            }
        }
    }
    if surfaces.is_empty() {
        return None;
    }
    let body = BodyId::mint(StepIdentity::data("body", id)).expect("identity grammar");
    let region = RegionId::mint(StepIdentity::data("region", id)).expect("identity grammar");
    let shell = ShellId::mint(StepIdentity::data("shell", format!("geometric-set-{id}")))
        .expect("identity grammar");
    let faces = surfaces
        .into_iter()
        .map(|(surface_step, surface)| Face {
            id: FaceId::mint(StepIdentity::data(
                "face",
                format!("{surface_step}-geometric-set-{id}"),
            ))
            .expect("identity grammar"),
            shell: shell.clone(),
            surface,
            sense: Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        })
        .collect::<Vec<_>>();
    let face_ids = faces.iter().map(|face| face.id.clone()).collect();
    staged_topology(
        typed,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        faces,
        Vec::new(),
        vec![Shell {
            id: shell.clone(),
            region: region.clone(),
            faces: face_ids,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        }],
        Region {
            id: region.clone(),
            body: body.clone(),
            shells: vec![shell],
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
}

#[derive(Clone)]
struct VertexDef {
    point: u64,
}
#[derive(Clone)]
struct EdgeDef {
    start: u64,
    end: u64,
    curve: Option<u64>,
    same: bool,
    parent: Option<u64>,
    pcurve: Option<u64>,
}
#[derive(Clone)]
struct OrientedDef {
    edge: u64,
    forward: bool,
    pcurve: Option<u64>,
    seam_edge: bool,
}

fn vertex_defs(exchange: &Exchange) -> BTreeMap<u64, VertexDef> {
    exchange
        .entities("VERTEX_POINT")
        .filter_map(|(id, r)| {
            if !has_type(r, "VERTEX_POINT") {
                return None;
            }
            Some((
                id,
                VertexDef {
                    point: named_reference(r, "VERTEX_POINT", 1, 0)?,
                },
            ))
        })
        .collect()
}
fn edge_defs(exchange: &Exchange) -> BTreeMap<u64, EdgeDef> {
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
        if let Some(edge) = edge_def_for(id, exchange, &mut active, &mut cache) {
            edges.insert(id, edge);
        }
    }
    edges
}

fn edge_def_for(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, Option<EdgeDef>>,
) -> Option<EdgeDef> {
    if let Some(edge) = cache.get(&id) {
        return edge.clone();
    }
    if !active.insert(id) {
        return None;
    }
    let result = (|| match most_specific(
        exchange.records.get(&id)?,
        &[
            "EDGE_CURVE",
            "SEAM_EDGE",
            "ORIENTED_EDGE",
            "SUBEDGE",
            "EDGE",
        ],
    )? {
        "EDGE_CURVE" => {
            let record = exchange.records.get(&id)?;
            let (start, end) = edge_vertices(record)?;
            Some(EdgeDef {
                start,
                end,
                curve: Some(edge_geometry(record)?),
                same: edge_same_sense(record)?,
                parent: None,
                pcurve: None,
            })
        }
        "EDGE" => {
            let record = exchange.records.get(&id)?;
            let (start, end) = edge_vertices(record)?;
            Some(EdgeDef {
                start,
                end,
                curve: None,
                same: true,
                parent: None,
                pcurve: None,
            })
        }
        "SUBEDGE" => {
            let record = exchange.records.get(&id)?;
            let (start, end) = edge_vertices(record)?;
            let parent = subedge_parent(record)?;
            let parent_def = edge_def_for(parent, exchange, active, cache)?;
            Some(EdgeDef {
                start,
                end,
                curve: parent_def.curve,
                same: parent_def.same,
                parent: Some(parent),
                pcurve: parent_def.pcurve,
            })
        }
        "ORIENTED_EDGE" | "SEAM_EDGE" => {
            let record = exchange.records.get(&id)?;
            let element = oriented_edge_reference(record)?;
            let element_def = edge_def_for(element, exchange, active, cache)?;
            let forward = oriented_edge_forward(record)?;
            Some(EdgeDef {
                start: element_def.start,
                end: element_def.end,
                curve: element_def.curve,
                same: element_def.same == forward,
                parent: Some(element),
                pcurve: if most_specific(record, &["SEAM_EDGE"]).is_some() {
                    record
                        .partial("SEAM_EDGE")
                        .and_then(|partial| {
                            partial
                                .parameters
                                .iter()
                                .rev()
                                .find_map(ValueExt::reference)
                        })
                        .or(element_def.pcurve)
                } else {
                    element_def.pcurve
                },
            })
        }
        _ => None,
    })();
    active.remove(&id);
    cache.insert(id, result.clone());
    result
}

fn edge_curve_id_reported(
    edge_id: u64,
    edge: &EdgeDef,
    exchange: &Exchange,
    warnings: &mut Vec<String>,
) -> Option<CurveId> {
    let Some(curve_step) = edge.curve else {
        warnings.push(format!(
            "STEP edge #{edge_id} has no 3D curve carrier; edge committed without a curve"
        ));
        return None;
    };
    let curve = exchange.records.get(&curve_step);
    let carrier = curve_carrier_step(curve_step, exchange);
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
        warnings.push(format!(
            "STEP edge curve #{edge_id}: surface-curve #{curve_step} has no resolvable basis; edge committed without a curve"
        ));
    }
    carrier
        .map(|curve| CurveId::mint(StepIdentity::data("curve", curve)).expect("identity grammar"))
}
fn oriented_defs(exchange: &Exchange) -> BTreeMap<u64, OrientedDef> {
    exchange
        .entities_any(&["ORIENTED_EDGE", "SEAM_EDGE"])
        .filter_map(|(id, r)| {
            if !has_type(r, "ORIENTED_EDGE") && !has_type(r, "SEAM_EDGE") {
                return None;
            }
            Some((
                id,
                OrientedDef {
                    edge: oriented_edge_reference(r)?,
                    forward: oriented_edge_forward(r)?,
                    seam_edge: most_specific(r, &["SEAM_EDGE"]).is_some(),
                    pcurve: r
                        .partials
                        .iter()
                        .find(|partial| partial.name == "SEAM_EDGE")
                        .and_then(|partial| {
                            partial
                                .parameters
                                .iter()
                                .rev()
                                .find_map(ValueExt::reference)
                        }),
                },
            ))
        })
        .collect()
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

fn surface_curve_basis(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return record.parameter(1).and_then(ValueExt::reference);
    }
    record
        .partial("SURFACE_CURVE")
        .or_else(|| record.partial("SEAM_CURVE"))
        .or_else(|| record.partial("INTERSECTION_CURVE"))
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::reference))
}

fn surface_curve_pcurves(record: &RawRecord) -> Option<Vec<u64>> {
    if record.partials.len() == 1 {
        return record.parameter(2).and_then(refs);
    }
    record
        .partial("SURFACE_CURVE")
        .or_else(|| record.partial("SEAM_CURVE"))
        .or_else(|| record.partial("INTERSECTION_CURVE"))
        .and_then(|partial| partial.parameters.iter().find_map(refs))
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

fn drop_committed_surfaces(draft: &mut ModelDraft, session: &CommitSession, ir: &CadIr) {
    // Implicit surfaces can be staged by multiple roots. The session is the
    // authority on which ones a prior root committed; a pre-loop snapshot is
    // wrong because commits add surfaces while the loop is running.
    draft
        .model_mut()
        .surfaces
        .retain(|surface| !session.contains(ir, surface.id.as_str()));
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
) -> Option<Built> {
    let mut draft = ModelDraft::new();
    for vertex in vertices {
        draft.insert(vertex).ok()?;
    }
    for edge in edges {
        draft.insert(edge).ok()?;
    }
    for coedge in coedges {
        draft.insert(coedge).ok()?;
    }
    for loop_ in loops {
        draft.insert(loop_).ok()?;
    }
    for face in faces {
        draft.insert(face).ok()?;
    }
    let mut surface_ids = BTreeSet::new();
    for surface in surfaces {
        if surface_ids.insert(surface.id.0.clone()) {
            draft.insert(surface).ok()?;
        }
    }
    for shell in shells {
        draft.insert(shell).ok()?;
    }
    draft.insert(region).ok()?;
    let body_id = body.id.clone();
    draft.insert(body).ok()?;
    Some(Built {
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
) -> Option<Vec<u64>> {
    if has_type(root, "SHELL_BASED_SURFACE_MODEL") {
        return named_refs(root, "SHELL_BASED_SURFACE_MODEL", 1);
    }
    if has_type(root, "FACE_BASED_SURFACE_MODEL") {
        let mut sets = Vec::new();
        for set_step in named_refs(root, "FACE_BASED_SURFACE_MODEL", 1)? {
            let set = exchange.records.get(&set_step)?;
            connected_face_set_type(set)?;
            sets.push(set_step);
        }
        return Some(sets);
    }
    if (has_type(root, "MANIFOLD_SOLID_BREP") || has_type(root, "FACETED_BREP"))
        && !has_type(root, "BREP_WITH_VOIDS")
    {
        let root_type = if has_type(root, "MANIFOLD_SOLID_BREP") {
            "MANIFOLD_SOLID_BREP"
        } else {
            "FACETED_BREP"
        };
        return Some(vec![named_reference(root, root_type, 1, 0)?]);
    }
    if has_type(root, "BREP_WITH_VOIDS") {
        let mut ids = vec![named_reference(root, "MANIFOLD_SOLID_BREP", 1, 0)?];
        ids.extend(named_refs(root, "BREP_WITH_VOIDS", 2)?);
        // `voids` is a STEP SET. CADIR keeps the outer shell at index zero
        // and canonicalizes the void suffix by resolved shell identity.
        ids[1..].sort_unstable_by_key(|reference| {
            shell_definitions
                .get(reference)
                .map_or((u64::MAX, true, *reference), |definition| {
                    (definition.base, definition.forward, *reference)
                })
        });
        return Some(ids);
    }
    None
}

fn root_key(
    root: &RawRecord,
    exchange: &Exchange,
    shell_definitions: &BTreeMap<u64, ShellDef>,
) -> Option<RootKey> {
    let root_kind = most_specific(
        root,
        &[
            "BREP_WITH_VOIDS",
            "FACETED_BREP",
            "MANIFOLD_SOLID_BREP",
            "FACE_BASED_SURFACE_MODEL",
            "SHELL_BASED_SURFACE_MODEL",
        ],
    )?;
    let mut shell_keys = Vec::new();
    let mut resolved = 0;
    for shell in root_shell_steps(root, exchange, shell_definitions)? {
        let key = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
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
        shell_keys.push(key?);
    }
    if resolved == 0 {
        return None;
    }
    shell_keys.sort_unstable();
    Some(RootKey {
        root_kind,
        shell_keys,
    })
}

#[allow(clippy::too_many_arguments)]
fn build(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
    ir: &mut CadIr,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    odefs: &BTreeMap<u64, OrientedDef>,
    shell_definitions: &BTreeMap<u64, ShellDef>,
    decoded_pcurves: &BTreeSet<PcurveId>,
    point_positions: &CarrierIndex,
    scope_root: bool,
    warnings: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
) -> BuildOutcome {
    let Some(shell_steps) = root_shell_steps(root, exchange, shell_definitions) else {
        return BuildOutcome {
            built: Vec::new(),
            failed: 1,
            failure: Some(BuildFailure {
                record_id: id,
                carrier_kind: "topology root carrier",
            }),
        };
    };
    let solid = has_type(root, "MANIFOLD_SOLID_BREP")
        || has_type(root, "BREP_WITH_VOIDS")
        || has_type(root, "FACETED_BREP");
    if solid {
        let body = BodyId::mint(StepIdentity::data("body", id)).expect("identity grammar");
        let region = RegionId::mint(StepIdentity::data("region", id)).expect("identity grammar");
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
            warnings,
            losses,
            &mut failure,
        );
        let failed = usize::from(built.is_none());
        return BuildOutcome {
            built: built.into_iter().collect(),
            failed,
            failure,
        };
    }

    let scoped = shell_steps.len() > 1;
    let mut built = Vec::new();
    let mut failed = 0;
    let mut failure = None;
    for shell_reference in shell_steps {
        let shell_step = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
            shell_reference
        } else {
            match shell_definitions.get(&shell_reference) {
                Some(definition) => definition.base,
                None => {
                    failure.get_or_insert(BuildFailure {
                        record_id: shell_reference,
                        carrier_kind: "shell carrier",
                    });
                    failed += 1;
                    continue;
                }
            }
        };
        let suffix = if scoped {
            Some(format!("-shell-{shell_step}"))
        } else {
            None
        };
        let body = BodyId::mint(StepIdentity::data(
            "body",
            format!("{id}{}", suffix.as_deref().unwrap_or_default()),
        ))
        .expect("identity grammar");
        let region = RegionId::mint(StepIdentity::data(
            "region",
            format!("{id}{}", suffix.as_deref().unwrap_or_default()),
        ))
        .expect("identity grammar");
        if let Some(value) = build_one(
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
            warnings,
            losses,
            &mut failure,
        ) {
            built.push(value);
        } else {
            failed += 1;
        }
    }
    BuildOutcome {
        built,
        failed,
        failure,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_one(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
    ir: &mut CadIr,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    odefs: &BTreeMap<u64, OrientedDef>,
    shell_definitions: &BTreeMap<u64, ShellDef>,
    decoded_pcurves: &BTreeSet<PcurveId>,
    point_positions: &CarrierIndex,
    shell_steps: &[u64],
    bid: BodyId,
    rid: &RegionId,
    scope_faces: bool,
    scope_edges: bool,
    scope_root: bool,
    warnings: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
    failure: &mut Option<BuildFailure>,
) -> Option<Built> {
    let solid = has_type(root, "MANIFOLD_SOLID_BREP")
        || has_type(root, "BREP_WITH_VOIDS")
        || has_type(root, "FACETED_BREP");
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
        let (shell_step, shell_forward) = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
            typed.insert(shell_reference);
            (shell_reference, true)
        } else {
            require_carrier(
                shell_def_for(shell_reference, shell_definitions, &mut typed),
                failure,
                shell_reference,
                "shell carrier",
            )?
        };
        if !used_shells.insert(shell_step) {
            continue;
        }
        let sr = require_carrier(
            exchange.records.get(&shell_step),
            failure,
            shell_step,
            "shell record",
        )?;
        let (shell_type, face_steps) = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
            let set_type = require_carrier(
                connected_face_set_type(sr),
                failure,
                shell_step,
                "connected face set",
            )?;
            if set_type == "CONNECTED_FACE_SUB_SET"
                && !validate_subset_parent(sr, set_type, exchange, warnings)
            {
                typed.remove(&shell_step);
            }
            let members = require_carrier(
                connected_set_members(sr, set_type),
                failure,
                shell_step,
                "connected face set member list",
            )?;
            (set_type, members)
        } else {
            let shell_type = require_carrier(
                most_specific(sr, &["OPEN_SHELL", "CLOSED_SHELL"]),
                failure,
                shell_step,
                "shell type",
            )?;
            let members = require_carrier(
                named_refs(sr, shell_type, 1),
                failure,
                shell_step,
                "shell face list",
            )?;
            (shell_type, members)
        };
        if face_steps.is_empty() {
            note_failure(failure, shell_step, "shell face list");
            return None;
        }
        let sid = shell_identity(id, shell_step, scope_root);
        let mut face_ids = vec![];
        for face_step in face_steps {
            if !used_faces.insert((shell_step, face_step)) {
                continue;
            }
            let fr = require_carrier(
                exchange.records.get(&face_step),
                failure,
                face_step,
                "face record",
            )?;
            if !is_face_record(fr) {
                note_failure(failure, face_step, "face carrier");
                return None;
            }
            let face_info = require_carrier(
                face_attributes(fr, exchange, &mut BTreeSet::new()),
                failure,
                face_step,
                "face attributes",
            )?;
            let outer_bound_count = face_info
                .bounds
                .iter()
                .filter(|bound_step| {
                    exchange
                        .records
                        .get(bound_step)
                        .is_some_and(|bound| has_type(bound, "FACE_OUTER_BOUND"))
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
                note_failure(failure, face_step, "face with multiple outer bounds");
                return None;
            }
            typed.extend(face_info.typed);
            let face_suffix = if scope_faces {
                if scope_root {
                    format!("-root-{id}-shell-{shell_step}")
                } else {
                    format!("-shell-{shell_step}")
                }
            } else {
                String::new()
            };
            let surface_id = if let Some(surface_step) = face_info.surface {
                SurfaceId::mint(StepIdentity::data("surface", surface_step))
                    .expect("identity grammar")
            } else {
                let surface_id = SurfaceId::mint(StepIdentity::data(
                    "surface",
                    format!("implicit-face-{face_step}{face_suffix}"),
                ))
                .expect("identity grammar");
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
                            "implicit face plane",
                        )?,
                        source_object: None,
                    });
                }
                surface_id
            };
            let surface_step = face_info.surface;
            let face_same_sense = face_info.same_sense;
            let fid = FaceId::mint(StepIdentity::data(
                "face",
                format!("{face_step}{face_suffix}"),
            ))
            .expect("identity grammar");
            let name = face_info.name.as_ref().and_then(|value| {
                super::decode_text(
                    exchange,
                    value,
                    losses,
                    face_step,
                    "face name",
                    StepLossCode::MetadataStringInvalid,
                )
            });
            let mut loop_ids = vec![];
            for bound_step in face_info.bounds {
                let br = require_carrier(
                    exchange.records.get(&bound_step),
                    failure,
                    bound_step,
                    "face bound",
                )?;
                if !has_type(br, "FACE_BOUND") && !has_type(br, "FACE_OUTER_BOUND") {
                    note_failure(failure, bound_step, "face bound carrier");
                    return None;
                }
                let is_outer_bound = has_type(br, "FACE_OUTER_BOUND");
                let Some(bound_type) = face_bound_attribute_type(br) else {
                    note_failure(failure, bound_step, "face bound attributes");
                    return None;
                };
                let loop_step = require_carrier(
                    named_reference(br, bound_type, 1, 0),
                    failure,
                    bound_step,
                    "bound loop reference",
                )?;
                let lr = require_carrier(
                    exchange.records.get(&loop_step),
                    failure,
                    loop_step,
                    "loop record",
                )?;
                let lid = LoopId::mint(StepIdentity::data(
                    "loop",
                    format!("{loop_step}-face-{face_step}{face_suffix}"),
                ))
                .expect("identity grammar");
                if has_type(lr, "VERTEX_LOOP") {
                    let vertex_step = require_carrier(
                        named_reference(lr, "VERTEX_LOOP", 1, 0),
                        failure,
                        loop_step,
                        "vertex loop reference",
                    )?;
                    if !vdefs
                        .get(&vertex_step)
                        .is_some_and(|vertex| point_positions.contains_key(vertex.point))
                    {
                        note_failure(failure, vertex_step, "vertex point");
                        return None;
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
                if has_type(lr, "POLY_LOOP") {
                    let bound_forward = require_carrier(
                        named_logical(br, bound_type, 2, 0),
                        failure,
                        bound_step,
                        "bound orientation",
                    )?;
                    let bound_forward = if face_info.reverse_bound_orientation {
                        !bound_forward
                    } else {
                        bound_forward
                    };
                    let mut points = require_carrier(
                        named_refs(lr, "POLY_LOOP", 1),
                        failure,
                        loop_step,
                        "poly loop point list",
                    )?;
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
                        note_failure(failure, loop_step, "poly loop point carrier");
                        return None;
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
                        let cid = CoedgeId::mint(StepIdentity::data(
                            "coedge",
                            format!("poly-{loop_step}-{index}-face-{face_step}{face_suffix}"),
                        ))
                        .expect("identity grammar");
                        coedge_ids.push(cid.clone());
                        coedges.push(Coedge {
                            id: cid,
                            owner_loop: lid.clone(),
                            edge: edge_id.clone(),
                            radial_next: CoedgeId::mint(String::new()).expect("identity grammar"),
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
                    loops.push(Loop {
                        id: lid.clone(),
                        face: fid.clone(),
                        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
                            coedges: coedge_ids,
                            vertex_uses: Vec::new(),
                        },
                    });
                    loop_ids.push((is_outer_bound, lid));
                    typed.insert(bound_step);
                    continue;
                }
                if !has_type(lr, "EDGE_LOOP") {
                    note_failure(failure, loop_step, "edge loop carrier");
                    return None;
                }
                let bound_forward = require_carrier(
                    named_logical(br, bound_type, 2, 0),
                    failure,
                    bound_step,
                    "bound orientation",
                )?;
                let bound_forward = if face_info.reverse_bound_orientation {
                    !bound_forward
                } else {
                    bound_forward
                };
                let mut uses = require_carrier(
                    named_refs(lr, "EDGE_LOOP", 1),
                    failure,
                    loop_step,
                    "edge loop member list",
                )?;
                if !bound_forward {
                    uses.reverse();
                }
                if uses.is_empty() {
                    note_failure(failure, loop_step, "edge loop member");
                    return None;
                }
                let mut coedge_ids = vec![];
                for use_step in uses {
                    let o = require_carrier(
                        odefs.get(&use_step),
                        failure,
                        use_step,
                        "oriented edge definition",
                    )?;
                    let edge =
                        require_carrier(edefs.get(&o.edge), failure, o.edge, "edge definition")?;
                    let cid = CoedgeId::mint(StepIdentity::data(
                        "coedge",
                        format!("{use_step}-face-{face_step}{face_suffix}"),
                    ))
                    .expect("identity grammar");
                    let pcurves: Vec<(PcurveId, Option<[f64; 2]>)> = if o.seam_edge {
                        let explicit_pcurve = surface_step.and_then(|surface_step| {
                            let pcurve_step = o.pcurve?;
                            let pcurve = exchange.records.get(&pcurve_step)?;
                            let pcurve_id =
                                PcurveId::mint(StepIdentity::data("pcurve", pcurve_step))
                                    .expect("identity grammar");
                            let edge_curve = edge.curve?;
                            let associated = associated_pcurves(
                                edge_curve,
                                surface_step,
                                exchange,
                                decoded_pcurves,
                            );
                            (has_type(pcurve, "PCURVE")
                                && entity_parameter(pcurve, "PCURVE", 1)?.reference()?
                                    == surface_step
                                && associated.contains(&pcurve_id))
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
                    } else {
                        let associated = match (surface_step, edge.curve) {
                            (Some(surface_step), Some(curve)) => {
                                associated_pcurves(curve, surface_step, exchange, decoded_pcurves)
                            }
                            _ => {
                                losses.push(StepLossCode::EdgeNoSurfaceOrCurveForPcurve.note(format!(
                                        "edge #{} has no decoded surface or curve carrier, so its coedge has no pcurve",
                                        o.edge
                                    )));
                                Vec::new()
                            }
                        };
                        match associated.as_slice() {
                            [] => associated
                                .into_iter()
                                .map(|pcurve| (pcurve, None))
                                .collect(),
                            candidates => {
                                let selection = surface_step.map(|surface| {
                                    select_associated_pcurve(
                                        ir,
                                        exchange,
                                        surface,
                                        edge,
                                        vdefs,
                                        point_positions,
                                        candidates,
                                    )
                                });
                                match selection {
                                    Some(Ok(selected)) => {
                                        let (Some(curve), Some(surface)) =
                                            (edge.curve, surface_step)
                                        else {
                                            unreachable!(
                                                "successful pcurve selection has one curve and surface"
                                            )
                                        };
                                        admissions.push(PcurveAdmission {
                                            curve,
                                            surface,
                                            coedge_use: use_step,
                                        });
                                        vec![(selected.id, selected.parameter_range)]
                                    }
                                    Some(Err(PcurveSelectionFailure::Locus)) => {
                                        let (Some(curve), Some(surface)) =
                                            (edge.curve, surface_step)
                                        else {
                                            unreachable!(
                                                "a locus failure has one curve and surface"
                                            )
                                        };
                                        losses.push(StepLossCode::PcurveLocusDiscontinuous.note(
                                            format!(
                                                "curve #{curve} has one endpoint-continuous pcurve on surface #{surface} whose bounded model-space locus or direction witness fails; the pcurve is omitted"
                                            ),
                                        ));
                                        Vec::new()
                                    }
                                    Some(Err(_)) | None => {
                                        let n = candidates.len();
                                        let note = match (edge.curve, surface_step, n) {
                                                (Some(curve), Some(surface), 1) => {
                                                    StepLossCode::PcurveEndpointsDiscontinuous.note(format!(
                                                        "curve #{curve} has one optional pcurve on surface #{surface} whose mapped endpoints are not continuous with the edge vertices; the pcurve is omitted"
                                                    ))
                                                }
                                                (Some(curve), Some(surface), _) => {
                                                    StepLossCode::PcurveAssociationAmbiguous.note(format!(
                                                        "curve #{curve} associates {n} pcurves with surface #{surface}; Part 42 provides no non-seam selector, so the coedge has no pcurve"
                                                    ))
                                                }
                                                _ => StepLossCode::PcurveCandidatesCarrierUnresolved.note(format!(
                                                        "coedge use #{use_step} has {n} pcurve candidates but its source surface or curve carrier is unresolved; no unique endpoint-continuous pcurve selects one, so the coedge has no pcurve"
                                                    )),
                                            };
                                        losses.push(note);
                                        Vec::new()
                                    }
                                }
                            }
                        }
                    };
                    coedge_ids.push(cid.clone());
                    coedges.push(Coedge {
                        id: cid,
                        owner_loop: lid.clone(),
                        edge: scoped_edge_id(o.edge, id, shell_step, scope_edges, scope_root),
                        radial_next: CoedgeId::mint(String::new()).expect("identity grammar"),
                        sense: if (o.forward == edge.same) == bound_forward {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves: pcurves
                            .into_iter()
                            .map(|(pcurve, parameter_range)| PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range,
                            })
                            .collect(),
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
                    used_v.extend([(shell_step, edge.start), (shell_step, edge.end)]);
                    typed.extend([use_step, o.edge]);
                    if let Some(parent) = edge.parent {
                        typed.insert(parent);
                    }
                }
                loops.push(Loop {
                    id: lid.clone(),
                    face: fid.clone(),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
                        coedges: coedge_ids,
                        vertex_uses: Vec::new(),
                    },
                });
                loop_ids.push((is_outer_bound, lid));
                typed.extend([bound_step, loop_step]);
            }
            loop_ids.sort_by_key(|(outer, _)| !outer);
            let outer = loop_ids
                .iter()
                .find(|(is_outer, _)| *is_outer)
                .map(|(_, id)| id.clone());
            let inner = loop_ids
                .into_iter()
                .filter_map(|(is_outer, id)| (!is_outer).then_some(id))
                .collect();
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
                loops: cadmpeg_ir::topology::FaceLoops::classified(outer, inner),
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
            let (start, end) = if edge.same {
                (edge.start, edge.end)
            } else {
                (edge.end, edge.start)
            };
            component_edge_vertices.insert(
                scoped_edge_id(*edge_id, id, shell_step, scope_edges, scope_root).0,
                (
                    scoped_vertex_id(start, id, shell_step, scope_edges, scope_root).0,
                    scoped_vertex_id(end, id, shell_step, scope_edges, scope_root).0,
                ),
            );
        }
        for ((used_shell, edge_id), (start, end)) in &poly_edges {
            if *used_shell != shell_step {
                continue;
            }
            component_edge_vertices.insert(
                edge_id.0.clone(),
                (
                    scoped_poly_vertex_id(*start, id, shell_step, scope_edges, scope_root).0,
                    scoped_poly_vertex_id(*end, id, shell_step, scope_edges, scope_root).0,
                ),
            );
        }
        let components =
            connected_face_components(&face_ids, &loops, &coedges, &component_edge_vertices)?;
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
            if has_type(root, "BREP_WITH_VOIDS")
                && shell_steps.first().copied() == Some(shell_reference)
                && component_index > 0
            {
                note_failure(failure, shell_step, "connected outer shell");
                return None;
            }
            let component_shell = if component_index == 0 {
                sid.clone()
            } else {
                ShellId::mint(format!("{}-component-{component_index}", sid.0))
                    .expect("identity grammar")
            };
            let component_faces = component
                .into_iter()
                .map(|face_index| {
                    let face_id = face_ids[face_index].clone();
                    faces[face_index].shell = component_shell.clone();
                    face_id
                })
                .collect();
            shells.push(Shell {
                id: component_shell.clone(),
                region: rid.clone(),
                faces: component_faces,
                wire_edges: vec![],
                free_vertices: vec![],
            });
            region.shells.push(component_shell);
        }
        typed.insert(shell_step);
    }
    for (shell_step, edge_id) in used_e {
        let e = require_carrier(edefs.get(&edge_id), failure, edge_id, "edge definition")?;
        let (start, end) = if e.same {
            (e.start, e.end)
        } else {
            (e.end, e.start)
        };
        edges.push(Edge {
            id: scoped_edge_id(edge_id, id, shell_step, scope_edges, scope_root),
            curve: edge_curve_id_reported(edge_id, e, exchange, warnings),
            start: scoped_vertex_id(start, id, shell_step, scope_edges, scope_root),
            end: scoped_vertex_id(end, id, shell_step, scope_edges, scope_root),
            param_range: None,
            tolerance: None,
        });
    }
    for ((shell_step, edge_identity), (start, end)) in poly_edges {
        edges.push(Edge {
            id: edge_identity,
            curve: None,
            start: scoped_poly_vertex_id(start, id, shell_step, scope_edges, scope_root),
            end: scoped_poly_vertex_id(end, id, shell_step, scope_edges, scope_root),
            param_range: None,
            tolerance: None,
        });
    }
    for (shell_step, vertex_id) in used_v {
        let v = require_carrier(
            vdefs.get(&vertex_id),
            failure,
            vertex_id,
            "vertex definition",
        )?;
        require_carrier(
            point_positions.get(v.point),
            failure,
            v.point,
            "vertex point",
        )?;
        vertices.push(Vertex {
            id: scoped_vertex_id(vertex_id, id, shell_step, scope_edges, scope_root),
            point: PointId::mint(StepIdentity::data("point", v.point)).expect("identity grammar"),
            tolerance: None,
        });
        typed.insert(vertex_id);
    }
    for (shell_step, point_id) in poly_points {
        require_carrier(
            point_positions.get(point_id),
            failure,
            point_id,
            "poly vertex point",
        )?;
        vertices.push(Vertex {
            id: scoped_poly_vertex_id(point_id, id, shell_step, scope_edges, scope_root),
            point: PointId::mint(StepIdentity::data("point", point_id)).expect("identity grammar"),
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
            let current =
                require_carrier(coedge_by_id.get(current_id), failure, loop_source, "coedge")?;
            let next = require_carrier(coedge_by_id.get(next_id), failure, loop_source, "coedge")?;
            let current_edge = require_carrier(
                edge_by_id.get(&current.edge),
                failure,
                loop_source,
                "coedge edge",
            )?;
            let next_edge = require_carrier(
                edge_by_id.get(&next.edge),
                failure,
                loop_source,
                "coedge edge",
            )?;
            let current_end = match current.sense {
                Sense::Forward => &current_edge.end,
                Sense::Reversed => &current_edge.start,
            };
            let next_start = match next.sense {
                Sense::Forward => &next_edge.start,
                Sense::Reversed => &next_edge.end,
            };
            if current_end != next_start {
                note_failure(failure, loop_source, "edge loop continuity");
                return None;
            }
        }
    }
    let mut built = require_carrier(
        staged_topology(
            typed, vertices, edges, coedges, loops, faces, surfaces, shells, region, body,
        ),
        failure,
        id,
        "topology draft",
    )?;
    built.pcurve_admissions = admissions;
    for &shell_reference in shell_steps {
        let shell_step = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
            shell_reference
        } else {
            require_carrier(
                shell_definitions
                    .get(&shell_reference)
                    .map(|definition| (definition.base, definition.forward)),
                failure,
                shell_reference,
                "shell carrier",
            )?
            .0
        };
        built.shell_sources.insert(shell_step);
    }
    Some(built)
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
) -> Option<Vec<Vec<usize>>> {
    let face_indices = face_ids
        .iter()
        .enumerate()
        .map(|(index, face)| (face.0.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let coedge_edges = coedges
        .iter()
        .map(|coedge| (coedge.id.0.clone(), coedge.edge.0.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut faces_by_edge = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut faces_by_vertex = BTreeMap::<String, BTreeSet<usize>>::new();
    for loop_ in loops {
        let Some(&face_index) = face_indices.get(&loop_.face.0) else {
            continue;
        };
        for coedge_id in loop_.coedges() {
            let Some(edge_id) = coedge_edges.get(&coedge_id.0) else {
                continue;
            };
            faces_by_edge
                .entry(edge_id.clone())
                .or_default()
                .insert(face_index);
            if let Some((start, end)) = edge_vertices.get(edge_id) {
                faces_by_vertex
                    .entry(start.clone())
                    .or_default()
                    .insert(face_index);
                faces_by_vertex
                    .entry(end.clone())
                    .or_default()
                    .insert(face_index);
            }
        }
        for vertex in loop_.vertices() {
            faces_by_vertex
                .entry(vertex.0.clone())
                .or_default()
                .insert(face_index);
        }
    }

    let mut neighbors = alloc_filled(
        face_ids.len(),
        BTreeSet::new(),
        "STEP connected-face neighbors",
    )
    .ok()?;
    for group in faces_by_edge.values().chain(faces_by_vertex.values()) {
        for &face in group {
            neighbors[face].extend(group.iter().copied().filter(|other| *other != face));
        }
    }
    let mut reached = BTreeSet::new();
    let mut components = Vec::new();
    for start in 0..face_ids.len() {
        if !reached.insert(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![start];
        while let Some(face) = pending.pop() {
            component.push(face);
            for &neighbor in &neighbors[face] {
                if reached.insert(neighbor) {
                    pending.push(neighbor);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    Some(components)
}

fn shell_identity(root_id: u64, shell_step: u64, scope_root: bool) -> ShellId {
    if scope_root {
        ShellId::mint(StepIdentity::data(
            "shell",
            format!("{shell_step}-root-{root_id}"),
        ))
        .expect("identity grammar")
    } else {
        ShellId::mint(StepIdentity::data("shell", shell_step)).expect("identity grammar")
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
            EdgeId::mint(StepIdentity::data(
                "edge",
                format!("{edge_step}-root-{root_id}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        } else {
            EdgeId::mint(StepIdentity::data(
                "edge",
                format!("{edge_step}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        }
    } else {
        EdgeId::mint(StepIdentity::data("edge", edge_step)).expect("identity grammar")
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
            VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{vertex_step}-root-{root_id}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        } else {
            VertexId::mint(StepIdentity::data(
                "vertex",
                format!("{vertex_step}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        }
    } else {
        VertexId::mint(StepIdentity::data("vertex", vertex_step)).expect("identity grammar")
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
            VertexId::mint(StepIdentity::data(
                "vertex",
                format!("poly-point-{point_step}-root-{root_id}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        } else {
            VertexId::mint(StepIdentity::data(
                "vertex",
                format!("poly-point-{point_step}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        }
    } else {
        VertexId::mint(StepIdentity::data(
            "vertex",
            format!("poly-point-{point_step}"),
        ))
        .expect("identity grammar")
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
            EdgeId::mint(StepIdentity::data(
                "edge",
                format!("poly-{start}-{end}-root-{root_id}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        } else {
            EdgeId::mint(StepIdentity::data(
                "edge",
                format!("poly-{start}-{end}-shell-{shell_step}"),
            ))
            .expect("identity grammar")
        }
    } else {
        EdgeId::mint(StepIdentity::data("edge", format!("poly-{start}-{end}")))
            .expect("identity grammar")
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
        let bound = exchange.records.get(&bound_step)?;
        let bound_type = face_bound_attribute_type(bound)?;
        let loop_step = named_reference(bound, bound_type, 1, 0)?;
        let loop_record = exchange.records.get(&loop_step)?;
        if !has_type(loop_record, "POLY_LOOP") {
            return None;
        }
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
        loop_normals.push((area_normal.unit()?, area));
    }
    let mut normal = loop_normals.first().map(|(normal, _)| *normal)?;
    let mut largest_area = loop_normals.first().map(|(_, area)| *area)?;
    for (candidate, area) in loop_normals.iter().skip(1).copied() {
        if area > largest_area
            || (area == largest_area
                && (candidate.x, candidate.y, candidate.z) > (normal.x, normal.y, normal.z))
        {
            normal = candidate;
            largest_area = area;
        }
    }
    for (candidate, _) in &loop_normals {
        if candidate.dot(normal) < 1.0 - IMPLICIT_FACE_NORMAL_ALIGNMENT_TOLERANCE {
            return None;
        }
    }
    let planarity_tolerance =
        COINCIDENCE_TOLERANCE.max(IMPLICIT_FACE_PLANAR_RELATIVE_TOLERANCE * scale);
    if relative_points
        .iter()
        .map(|point| point.dot(normal).abs())
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
        let projected = axis - normal.scale(axis.dot(normal));
        let norm = projected.norm();
        if norm > u_axis_norm {
            u_axis_norm = norm;
            u_axis = projected.unit();
        }
    }
    let u_axis = u_axis?;
    Some(SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    })
}

fn curve_carrier_step(curve_step: u64, exchange: &Exchange) -> Option<u64> {
    let curve = exchange.records.get(&curve_step)?;
    if curve.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        )
    }) {
        surface_curve_basis(curve)
    } else {
        Some(curve_step)
    }
}

fn associated_pcurves(
    curve_step: u64,
    surface_step: u64,
    exchange: &Exchange,
    decoded_pcurves: &BTreeSet<PcurveId>,
) -> Vec<PcurveId> {
    let Some(curve) = exchange.records.get(&curve_step) else {
        return Vec::new();
    };
    if !curve.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        )
    }) {
        return Vec::new();
    }
    let Some(pcurves) = surface_curve_pcurves(curve) else {
        return Vec::new();
    };
    pcurves
        .into_iter()
        .filter_map(|pcurve_step| {
            let pcurve = exchange.records.get(&pcurve_step)?;
            let pcurve_id = PcurveId::mint(StepIdentity::data("pcurve", pcurve_step))
                .expect("identity grammar");
            (has_type(pcurve, "PCURVE")
                && entity_parameter(pcurve, "PCURVE", 1)?.reference()? == surface_step
                && decoded_pcurves.contains(&pcurve_id))
            .then_some(pcurve_id)
        })
        .collect()
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
    NotUnique,
    Carrier,
    Endpoint,
    Locus,
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
    ir: &mut CadIr,
    exchange: &Exchange,
    surface_step: u64,
    edge: &EdgeDef,
    vdefs: &BTreeMap<u64, VertexDef>,
    point_positions: &CarrierIndex,
    candidates: &[PcurveId],
) -> Result<SelectedPcurve, PcurveSelectionFailure> {
    if candidates.len() != 1 {
        return Err(PcurveSelectionFailure::NotUnique);
    }
    let surface_identity = StepIdentity::data("surface", surface_step);
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == surface_identity)
        .map(|surface| surface.geometry.clone())
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let surface_id = SurfaceId::mint(surface_identity).expect("identity grammar");
    let index = ModelIndex::new(ir);
    let candidate = candidates
        .first()
        .cloned()
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let pcurve = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == candidate)
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let geometry = &pcurve.geometry;
    let bound = COINCIDENCE_TOLERANCE.max(ir.tolerances.linear);
    let start = vdefs
        .get(&edge.start)
        .and_then(|vertex| point_positions.get(vertex.point))
        .copied()
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let end = vdefs
        .get(&edge.end)
        .and_then(|vertex| point_positions.get(vertex.point))
        .copied()
        .ok_or(PcurveSelectionFailure::Carrier)?;
    let (curve_start, curve_end) = if edge.same {
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
    )
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
    ) {
        return Err(PcurveSelectionFailure::Locus);
    }
    let parameter_range = pcurve_declared_parameter_range(geometry).and_then(|range| {
        let declared = pcurve_declared_endpoint_fit_directed(
            &index,
            &surface_id,
            geometry,
            range,
            curve_start,
            curve_end,
        )?;
        (declared.is_finite() && declared > COINCIDENCE_TOLERANCE)
            .then_some([endpoint.start_parameter, endpoint.end_parameter])
    });
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
) -> bool {
    let Some(curve_step) = edge
        .curve
        .and_then(|curve| curve_carrier_step(curve, exchange))
    else {
        return false;
    };
    let curve_id =
        CurveId::mint(StepIdentity::data("curve", curve_step)).expect("identity grammar");
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
                (domain[0] + domain[1]) * 0.5,
                0.0,
                1.0,
                -1.0,
            ]
        },
    );
    let Some(curve_start_parameter) =
        curve_parameter_near_point(index, &curve_id, curve_start, &curve_seeds, bound)
    else {
        return false;
    };
    let Some(curve_end_parameter) =
        curve_parameter_near_point(index, &curve_id, curve_end, &curve_seeds, bound)
    else {
        return false;
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
    let parameter_span = endpoint.end_parameter - endpoint.start_parameter;
    let curve_parameter_span = curve_end_parameter - curve_start_parameter;
    if !parameter_span.is_finite() || !curve_parameter_span.is_finite() {
        return false;
    }
    for fraction in fractions {
        let pcurve_parameter = endpoint
            .start_parameter
            .mul_add(1.0 - fraction, endpoint.end_parameter * fraction);
        let Some(uv) = pcurve_uv(geometry, pcurve_parameter) else {
            return false;
        };
        let Some(mapped) = surface_selection_point(index, surface_id, uv.u, uv.v) else {
            return false;
        };
        let curve_seed =
            curve_start_parameter.mul_add(1.0 - fraction, curve_end_parameter * fraction);
        let mut seeds = curve_seeds.to_vec();
        seeds.push(curve_seed);
        let Some(curve_parameter) =
            curve_parameter_near_point(index, &curve_id, mapped, &seeds, bound)
        else {
            return false;
        };
        let Some(curve_point) = model_curve_point_by_id(index, &curve_id, curve_parameter) else {
            return false;
        };
        if !curve_point.distance(mapped).is_finite()
            || curve_point.distance(mapped) > bound
            || !fraction.is_finite()
        {
            return false;
        }
    }
    true
}

fn curve_parameter_near_point(
    index: &ModelIndex<'_>,
    curve_id: &CurveId,
    point: Point3,
    seeds: &[f64],
    tolerance: f64,
) -> Option<f64> {
    seeds
        .iter()
        .copied()
        .filter(|seed| seed.is_finite())
        .filter_map(|seed| {
            model_curve_parameter_near_point_in_index_with_tolerance(
                index, curve_id, point, seed, tolerance,
            )
            .map(|parameter| ((parameter - seed).abs(), parameter))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, parameter)| parameter)
}

fn pcurve_endpoint_fit(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    surface: &SurfaceGeometry,
    start: Point3,
    end: Point3,
) -> Option<PcurveEndpointFit> {
    if let Some(parameter_range) = pcurve_declared_parameter_range(geometry) {
        let declared_score = pcurve_declared_endpoint_fit_directed(
            index,
            surface_id,
            geometry,
            parameter_range,
            start,
            end,
        )?;
        if declared_score <= COINCIDENCE_TOLERANCE {
            return Some(PcurveEndpointFit {
                start_parameter: parameter_range[0],
                end_parameter: parameter_range[1],
                max_residual: declared_score,
            });
        }
        // A few producers retain a stale trim around an edge-local pcurve.
        // Search for an alternative interval, then use the evaluated residual
        // as the witness. The search does not establish a global minimum.
        let seeds = pcurve_selection_seeds(index, surface_id, geometry, surface);
        let start = pcurve_surface_closest(index, surface_id, geometry, start, &seeds)?;
        let end = pcurve_surface_closest(index, surface_id, geometry, end, &seeds)?;
        return Some(PcurveEndpointFit {
            start_parameter: start.1,
            end_parameter: end.1,
            max_residual: start.0.max(end.0),
        });
    }
    let seeds = pcurve_selection_seeds(index, surface_id, geometry, surface);
    let start = pcurve_surface_closest(index, surface_id, geometry, start, &seeds)?;
    let end = pcurve_surface_closest(index, surface_id, geometry, end, &seeds)?;
    Some(PcurveEndpointFit {
        start_parameter: start.1,
        end_parameter: end.1,
        max_residual: start.0.max(end.0),
    })
}

fn pcurve_declared_parameter_range(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => Some(*parameter_range),
        PcurveGeometry::Offset { basis, .. } | PcurveGeometry::Transformed { basis, .. } => {
            pcurve_declared_parameter_range(basis)
        }
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. }
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
        .surfaces(&surface_id.0)
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
) -> Option<Point3> {
    let [u, v] = surface_selection_parameters(index, surface_id, u, v);
    model_surface_point_by_id(index, surface_id, u, v)
}

#[cfg(test)]
fn pcurve_declared_endpoint_fit(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    range: [f64; 2],
    start: Point3,
    end: Point3,
) -> Option<f64> {
    let first_uv = pcurve_uv(geometry, range[0])?;
    let last_uv = pcurve_uv(geometry, range[1])?;
    let first = surface_selection_point(index, surface_id, first_uv.u, first_uv.v)?;
    let last = surface_selection_point(index, surface_id, last_uv.u, last_uv.v)?;
    let forward = first.distance(start).max(last.distance(end));
    let reversed = first.distance(end).max(last.distance(start));
    Some(forward.min(reversed))
}

fn pcurve_declared_endpoint_fit_directed(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    range: [f64; 2],
    start: Point3,
    end: Point3,
) -> Option<f64> {
    let first_uv = pcurve_uv(geometry, range[0])?;
    let last_uv = pcurve_uv(geometry, range[1])?;
    let first = surface_selection_point(index, surface_id, first_uv.u, first_uv.v)?;
    let last = surface_selection_point(index, surface_id, last_uv.u, last_uv.v)?;
    Some(first.distance(start).max(last.distance(end)))
}

fn pcurve_surface_closest(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    target: Point3,
    seeds: &[f64],
) -> Option<(f64, f64)> {
    // The minimum is only over the finite seed set. The caller treats the
    // directly evaluated result as a witness and omits the optional relation
    // when no witness meets the tolerance.
    seeds
        .iter()
        .copied()
        .filter_map(|seed| mapped_pcurve_closest(index, surface_id, geometry, target, seed))
        .min_by(|left, right| left.0.total_cmp(&right.0))
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
) -> Option<(f64, f64)> {
    if !seed.is_finite() {
        return None;
    }
    let domain = pcurve_selection_parameter_domain(geometry);
    let clamp_to_domain =
        |parameter: f64| domain.map_or(parameter, |[lower, upper]| parameter.clamp(lower, upper));
    let evaluate_point = |parameter: f64| {
        let uv = pcurve_uv(geometry, parameter)?;
        surface_selection_point(index, surface_id, uv.u, uv.v)
    };
    let evaluate_tangent = |parameter: f64| {
        let uv = pcurve_uv(geometry, parameter)?;
        let tangent_uv = pcurve_tangent(geometry, parameter)?;
        let [u, v] = surface_selection_parameters(index, surface_id, uv.u, uv.v);
        let partials = model_surface_partials_by_id(index, surface_id, u, v)?;
        Some(Vector3::new(
            partials.du.x * tangent_uv.u + partials.dv.x * tangent_uv.v,
            partials.du.y * tangent_uv.u + partials.dv.y * tangent_uv.v,
            partials.du.z * tangent_uv.u + partials.dv.z * tangent_uv.v,
        ))
    };

    let mut parameter = clamp_to_domain(seed);
    let mut best = f64::INFINITY;
    let mut best_parameter = parameter;
    for _ in 0..32 {
        let point = evaluate_point(parameter)?;
        let error = point.distance(target);
        if !error.is_finite() {
            return None;
        }
        if error < best {
            best = error;
            best_parameter = parameter;
        }
        let Some(tangent) = evaluate_tangent(parameter) else {
            break;
        };
        let denominator = tangent.dot(tangent);
        if !denominator.is_finite() || denominator <= f64::EPSILON {
            break;
        }
        let residual = point.vector_from(target);
        let step = residual.dot(tangent) / denominator;
        if !step.is_finite() {
            break;
        }
        let mut candidate = clamp_to_domain(parameter - step);
        let Some(candidate_point) = evaluate_point(candidate) else {
            break;
        };
        let mut candidate_error = candidate_point.distance(target);
        for _ in 0..12 {
            if candidate_error < error {
                break;
            }
            candidate = clamp_to_domain(0.5 * (candidate + parameter));
            let Some(candidate_point) = evaluate_point(candidate) else {
                break;
            };
            candidate_error = candidate_point.distance(target);
        }
        if candidate == parameter || !candidate_error.is_finite() || candidate_error >= error {
            break;
        }
        parameter = candidate;
    }
    best.is_finite().then_some((best, best_parameter))
}

fn pcurve_parameter_break_fractions(
    geometry: &PcurveGeometry,
    parameters: [f64; 2],
    fractions: &mut Vec<f64>,
) {
    let parameter_span = parameters[1] - parameters[0];
    if !parameter_span.is_finite() || parameter_span == 0.0 {
        return;
    }
    let mut add = |parameter: f64| {
        let fraction = (parameter - parameters[0]) / parameter_span;
        if fraction.is_finite() && fraction > 0.0 && fraction < 1.0 {
            fractions.push(fraction);
        }
    };
    match geometry {
        PcurveGeometry::Nurbs { nurbs } => nurbs.knots().iter().copied().for_each(&mut add),
        PcurveGeometry::PolarNurbs { nurbs } => {
            nurbs.knots().iter().copied().for_each(&mut add);
        }
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
            ..
        } => {
            add(parameter_range[0]);
            add(parameter_range[1]);
            pcurve_parameter_break_fractions(basis, parameters, fractions);
        }
        PcurveGeometry::Offset { basis, .. } | PcurveGeometry::Transformed { basis, .. } => {
            pcurve_parameter_break_fractions(basis, parameters, fractions);
        }
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => {}
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
        seeds.extend([start, start + (end - start) * 0.5, end]);
        for step in 0..=PCURVE_ENDPOINT_GRID_DIVISIONS {
            let fraction = step as f64 / PCURVE_ENDPOINT_GRID_DIVISIONS as f64;
            seeds.push(start + (end - start) * fraction);
        }
        let mut fractions = vec![0.0, 1.0];
        pcurve_parameter_break_fractions(geometry, [start, end], &mut fractions);
        fractions.sort_by(f64::total_cmp);
        fractions.dedup_by(|left, right| *left == *right);
        seeds.extend(
            fractions
                .iter()
                .map(|fraction| start + (end - start) * fraction),
        );
        seeds.extend(fractions.windows(2).map(|window| {
            let lower = window[0];
            let upper = window[1];
            start + (end - start) * (lower + (upper - lower) * 0.5)
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
        if let Some(period) = surface_parameter_periods(surface)[0] {
            if direction.u != 0.0 {
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    seeds.push((period * fraction - origin.u) / direction.u);
                }
            }
        }
        if let Some(period) = surface_parameter_periods(surface)[1] {
            if direction.v != 0.0 {
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    seeds.push((period * fraction - origin.v) / direction.v);
                }
            }
        }
        let [u_domain, v_domain] = surface_selection_parameter_domains(index, surface_id, surface);
        if let Some([u_lower, u_upper]) = u_domain {
            for boundary in [u_lower, (u_lower + u_upper) * 0.5, u_upper] {
                if direction.u != 0.0 {
                    seeds.push((boundary - origin.u) / direction.u);
                }
            }
        }
        if let Some([v_lower, v_upper]) = v_domain {
            for boundary in [v_lower, (v_lower + v_upper) * 0.5, v_upper] {
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

fn pcurve_has_angular_parameterization(geometry: &PcurveGeometry) -> bool {
    match geometry {
        PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => true,
        PcurveGeometry::Offset { basis, .. }
        | PcurveGeometry::Transformed { basis, .. }
        | PcurveGeometry::Trimmed { basis, .. } => pcurve_has_angular_parameterization(basis),
        PcurveGeometry::Line { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::Nurbs { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. } => false,
    }
}

fn pcurve_selection_parameter_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs { nurbs } => selection_nurbs_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.control_points().len(),
        ),
        PcurveGeometry::PolarNurbs { nurbs } => {
            selection_nurbs_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.poles().len())
        }
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
            ..
        } => {
            if parameter_range[0] < parameter_range[1] {
                Some(*parameter_range)
            } else {
                pcurve_selection_parameter_domain(basis)
            }
        }
        PcurveGeometry::Offset { basis, .. } => pcurve_selection_parameter_domain(basis),
        PcurveGeometry::Transformed { basis, .. } => pcurve_selection_parameter_domain(basis),
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::SphericalGreatCircle { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. } => None,
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
        .map(|procedural| procedural.definition());
    match definition {
        Some(ProceduralSurfaceDefinition::Subset {
            parameter_ranges, ..
        }) => [
            subset_parameter_domain(parameter_ranges[0]),
            subset_parameter_domain(parameter_ranges[1]),
        ],
        Some(ProceduralSurfaceDefinition::AxisRevolution { directrix, .. }) => [
            Some([0.0, std::f64::consts::TAU]),
            curve_selection_parameter_domain(index, directrix),
        ],
        Some(
            ProceduralSurfaceDefinition::Extrusion { directrix, .. }
            | ProceduralSurfaceDefinition::LinearSweep { directrix, .. },
        ) => [curve_selection_parameter_domain(index, directrix), None],
        Some(ProceduralSurfaceDefinition::Replica { source, .. }) => index
            .surfaces(&source.0)
            .map_or([None, None], |source_surface| {
                surface_selection_parameter_domains(index, source, &source_surface.geometry)
            }),
        _ => surface_selection_parameter_domains_from_geometry(surface),
    }
}

fn surface_selection_parameter_domains_from_geometry(
    surface: &SurfaceGeometry,
) -> [Option<[f64; 2]>; 2] {
    match surface {
        SurfaceGeometry::Procedural {
            cache: Some(geometry),
            ..
        } => surface_selection_parameter_domains_from_geometry(geometry),
        SurfaceGeometry::Nurbs(surface) => {
            let (u_count, v_count) = (surface.u_count() as usize, surface.v_count() as usize);
            [
                selection_nurbs_parameter_domain(surface.u_degree(), surface.u_knots(), u_count),
                selection_nurbs_parameter_domain(surface.v_degree(), surface.v_knots(), v_count),
            ]
        }
        SurfaceGeometry::Transformed { basis, .. } => {
            surface_selection_parameter_domains_from_geometry(basis)
        }
        SurfaceGeometry::Plane { .. }
        | SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal(_)
        | SurfaceGeometry::Unknown { .. } => [None, None],
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
    let curve = index.curves(&curve_id.0)?;
    curve_selection_parameter_domain_from_geometry(&curve.geometry)
}

fn curve_selection_parameter_domain_from_geometry(geometry: &CurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            Some([0.0, std::f64::consts::TAU])
        }
        CurveGeometry::Nurbs(curve) => nurbs_curve_parameter_domain(curve),
        CurveGeometry::Polyline(polyline) => {
            let parameters = polyline.parameters()?;
            let lower = *parameters.first()?;
            let upper = *parameters.last()?;
            (lower.is_finite() && upper.is_finite() && lower < upper).then_some([lower, upper])
        }
        CurveGeometry::Transformed { basis, .. } => {
            curve_selection_parameter_domain_from_geometry(basis)
        }
        CurveGeometry::Line { .. }
        | CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

fn selection_nurbs_parameter_domain(
    degree: u32,
    knots: &[f64],
    control_point_count: usize,
) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    if control_point_count <= degree
        || knots.len() < control_point_count.checked_add(degree)?.checked_add(1)?
    {
        return None;
    }
    let start = *knots.get(degree)?;
    let end = *knots.get(control_point_count)?;
    (start.is_finite() && end.is_finite() && start < end).then_some([start, end])
}

#[derive(Clone)]
struct ShellDef {
    base: u64,
    forward: bool,
    typed: HashSet<u64>,
}

fn shell_defs(exchange: &Exchange) -> BTreeMap<u64, ShellDef> {
    let mut cache = BTreeMap::<u64, Option<ShellDef>>::new();
    let mut active = BTreeSet::new();
    for (id, _) in exchange.entities_any(&[
        "ORIENTED_OPEN_SHELL",
        "ORIENTED_CLOSED_SHELL",
        "OPEN_SHELL",
        "CLOSED_SHELL",
    ]) {
        shell_def_cached(id, exchange, &mut active, &mut cache);
    }
    cache
        .into_iter()
        .filter_map(|(id, definition)| definition.map(|definition| (id, definition)))
        .collect()
}

fn shell_def_cached(
    reference: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, Option<ShellDef>>,
) -> Option<ShellDef> {
    if let Some(definition) = cache.get(&reference) {
        return definition.clone();
    }
    if !active.insert(reference) {
        return None;
    }
    let result = (|| {
        let record = exchange.records.get(&reference)?;
        match most_specific(
            record,
            &[
                "ORIENTED_OPEN_SHELL",
                "ORIENTED_CLOSED_SHELL",
                "OPEN_SHELL",
                "CLOSED_SHELL",
            ],
        )? {
            "OPEN_SHELL" | "CLOSED_SHELL" => Some(ShellDef {
                base: reference,
                forward: true,
                typed: HashSet::new(),
            }),
            "ORIENTED_OPEN_SHELL" | "ORIENTED_CLOSED_SHELL" => {
                let shell_type =
                    most_specific(record, &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL"])?;
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
                        named_reference(record, shell_type, 1, 0),
                        named_logical(record, shell_type, 2, 0),
                    )
                };
                let (element, orientation) = element.zip(orientation)?;
                let mut definition = shell_def_cached(element, exchange, active, cache)?;
                definition.forward = definition.forward == orientation;
                definition.typed.insert(reference);
                Some(definition)
            }
            _ => None,
        }
    })();
    active.remove(&reference);
    cache.insert(reference, result.clone());
    result
}

fn shell_def_for(
    reference: u64,
    shells: &BTreeMap<u64, ShellDef>,
    typed: &mut HashSet<u64>,
) -> Option<(u64, bool)> {
    let definition = shells.get(&reference)?;
    typed.extend(definition.typed.iter().copied());
    Some((definition.base, definition.forward))
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
    has_type(record, "FACE")
        || has_type(record, "ADVANCED_FACE")
        || has_type(record, "FACE_SURFACE")
        || has_type(record, "ORIENTED_FACE")
        || has_type(record, "SUBFACE")
}

fn face_attributes(
    record: &RawRecord,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
) -> Option<FaceInfo> {
    if !active.insert(record.id) {
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
            let mut base = face_attributes(exchange.records.get(&face_element)?, exchange, active)?;
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
                face_attributes(exchange.records.get(&parent)?, exchange, active)?;
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
    active.remove(&record.id);
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
                exchange.records.get(id).is_some_and(|bound| {
                    has_type(bound, "FACE_BOUND") || has_type(bound, "FACE_OUTER_BOUND")
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

fn has_type(record: &RawRecord, name: &str) -> bool {
    record.partials.iter().any(|partial| partial.name == name)
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
    chain.iter().copied().find(|name| has_type(record, name))
}

fn connected_face_set_type(record: &RawRecord) -> Option<&'static str> {
    most_specific(record, &["CONNECTED_FACE_SUB_SET", "CONNECTED_FACE_SET"])
}

fn connected_set_members(record: &RawRecord, set_type: &str) -> Option<Vec<u64>> {
    let base_type = match set_type {
        "CONNECTED_EDGE_SUB_SET" => "CONNECTED_EDGE_SET",
        "CONNECTED_FACE_SUB_SET" => "CONNECTED_FACE_SET",
        _ => set_type,
    };
    named_refs(record, set_type, 1).or_else(|| named_refs(record, base_type, 1))
}

fn validate_subset_parent(
    record: &RawRecord,
    subset_type: &str,
    exchange: &Exchange,
    warnings: &mut Vec<String>,
) -> bool {
    let base_type = match subset_type {
        "CONNECTED_EDGE_SUB_SET" => "CONNECTED_EDGE_SET",
        "CONNECTED_FACE_SUB_SET" => "CONNECTED_FACE_SET",
        _ => return true,
    };
    let parent = if record.partials.len() == 1 {
        entity_parameter(record, subset_type, 2).and_then(ValueExt::reference)
    } else {
        record
            .partial(subset_type)
            .and_then(|partial| partial.parameters.iter().find_map(ValueExt::reference))
    };
    let Some(parent) = parent else {
        warnings.push(format!(
            "{subset_type} #{} has no resolvable parent {base_type}",
            record.id
        ));
        return false;
    };
    if exchange
        .records
        .get(&parent)
        .is_some_and(|parent_record| most_specific(parent_record, &[base_type]) == Some(base_type))
    {
        true
    } else {
        warnings.push(format!(
            "{subset_type} #{} parent #{parent} does not resolve to {base_type}",
            record.id
        ));
        false
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

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first()?.parameters.get(index)
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn logical(&self) -> Option<bool>;
}
impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    fn list(&self) -> Option<&[Value]> {
        if let Value::List(values) = self {
            Some(values)
        } else {
            None
        }
    }
    fn logical(&self) -> Option<bool> {
        match self {
            Value::Enumeration(v) if v == "T" => Some(true),
            Value::Enumeration(v) if v == "F" => Some(false),
            _ => None,
        }
    }
}
