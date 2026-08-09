// SPDX-License-Identifier: Apache-2.0
//! STEP boundary-representation ownership and orientation decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{CommitSession, DraftError, ModelDraft};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{LossKind, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, PcurveUse, Region, Sense, Shell,
    Vertex, VertexUse,
};

use crate::parse::{Exchange, RawRecord, Value};

use super::index::CarrierIndex;

pub(super) struct TopologyResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
    pub losses: Vec<LossNote>,
    pub body_by_root: BTreeMap<u64, Vec<BodyId>>,
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

/// Returns the STEP representation families that produce a neutral topology body.
pub(super) fn is_body_representation(record: &RawRecord) -> bool {
    [
        "SHELL_BASED_SURFACE_MODEL",
        "FACE_BASED_SURFACE_MODEL",
        "FACETED_BREP",
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
        "SHELL_BASED_WIREFRAME_MODEL",
        "EDGE_BASED_WIREFRAME_MODEL",
        "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION",
    ]
    .iter()
    .any(|name| has_type(record, name))
}

pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    carrier_index: &CarrierIndex,
) -> TopologyResult {
    let mut commit_session = CommitSession::new(ir);
    let mut result = TopologyResult {
        typed_records: BTreeSet::new(),
        warnings: Vec::new(),
        losses: Vec::new(),
        body_by_root: BTreeMap::new(),
        body_by_shell: BTreeMap::new(),
        faces_by_source: BTreeMap::new(),
        edges_by_source: BTreeMap::new(),
        vertices_by_source: BTreeMap::new(),
    };
    for record in exchange.records.values() {
        let Some(name) = most_specific(record, &["ORIENTED_OPEN_SHELL", "ORIENTED_CLOSED_SHELL"])
        else {
            continue;
        };
        if record.partials.len() != 1 || matches!(record.parameter(1), Some(Value::Derived)) {
            continue;
        }
        result.losses.push(LossNote {
            code: LossKind::NoncanonicalSourceSyntax,
            severity: Severity::Warning,
            message: format!(
                "{name} #{} omits the derived `cfs_faces` slot required by ISO 10303-21; \
                 read the shell element from positional slot 1",
                record.id
            ),
            provenance: Some(cadmpeg_ir::LossProvenance {
                format: "step".into(),
                stream: String::new(),
                offset: record.span.start as u64,
                tag: Some("oriented_shell".into()),
            }),
        });
    }
    let vertices = vertex_defs(exchange);
    let edges = edge_defs(exchange);
    let oriented = oriented_defs(exchange);
    let shells = shell_defs(exchange);
    let point_positions = carrier_index;
    for diagnostic in source_invalid_shells(exchange, &shells, &edges, &oriented) {
        let provenance =
            exchange
                .records
                .get(&diagnostic.shell)
                .map(|record| cadmpeg_ir::LossProvenance {
                    format: "step".into(),
                    stream: String::new(),
                    offset: record.span.start as u64,
                    tag: Some(diagnostic.shell_type.to_ascii_lowercase()),
                });
        let note = LossNote::new(
            LossKind::SourceTopologyInvalid,
            format!(
                "source {} #{} contains {} disconnected face component(s) across {} face(s); topology retained as decoded",
                diagnostic.shell_type,
                diagnostic.shell,
                diagnostic.components,
                diagnostic.face_count,
            ),
        );
        result.losses.push(match provenance {
            Some(provenance) => note.with_provenance(provenance),
            None => note,
        });
    }
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
            record.parameter(1).and_then(refs).map(|items| {
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
            result.typed_records.insert(representation);
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
                result.typed_records.append(&mut built.typed);
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
                result.typed_records.append(&mut built.typed);
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
    for (id, record) in exchange.entities_any(&[
        "SHELL_BASED_SURFACE_MODEL",
        "FACE_BASED_SURFACE_MODEL",
        "FACETED_BREP",
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
    ]) {
        let Some(key) = root_key(record, exchange, &shells) else {
            result.warnings.push(format!(
                "STEP topology root #{id} does not resolve to a complete connected topology graph",
            ));
            continue;
        };
        if let Some(root_built) = built_roots.get(&key).cloned() {
            result.typed_records.insert(id);
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
        let scope_root = scope_distinct_roots;
        let outcome = build(
            id,
            record,
            exchange,
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
            drop_committed_surfaces(&mut built.draft, &commit_session);
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
                result.typed_records.append(&mut built.typed);
            }
        }
        if body_ids.is_empty() {
            if let Some(message) = failure_message {
                result.losses.push(LossNote {
                    code: LossKind::TopologyNotTransferred,
                    severity: Severity::Error,
                    message: format!("STEP topology root #{id} rejected: {message}"),
                    provenance: None,
                });
            } else {
                result.losses.push(LossNote {
                    code: LossKind::TopologyNotTransferred,
                    severity: Severity::Error,
                    message: format!(
                        "STEP topology root #{id} does not resolve to a complete connected topology graph",
                    ),
                    provenance: None,
                });
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
                &mut result.typed_records,
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
            result.typed_records.append(&mut built.typed);
        }
    }
    for (id, record) in exchange.entities_any(&[
        "SHAPE_REPRESENTATION",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION",
    ]) {
        if !matches!(
            record.simple_name(),
            Some(
                "SHAPE_REPRESENTATION"
                    | "ADVANCED_BREP_SHAPE_REPRESENTATION"
                    | "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION"
            )
        ) {
            continue;
        }
        let omitted = geometric_set_omissions(record, exchange, carrier_index);
        if !omitted.is_empty() {
            result.warnings.push(format!(
                "{} #{id} omitted unsupported or unresolved member(s): {}",
                record.simple_name().unwrap_or("representation"),
                omitted
                    .iter()
                    .map(|member| format!("#{member}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        mark_standalone_geometric_set(
            id,
            record,
            exchange,
            carrier_index,
            &mut result.typed_records,
        );
    }
    for (id, record) in exchange.entities_any(&[
        "MANIFOLD_SURFACE_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "SHAPE_REPRESENTATION",
    ]) {
        if !matches!(
            record.simple_name(),
            Some(
                "MANIFOLD_SURFACE_SHAPE_REPRESENTATION"
                    | "ADVANCED_BREP_SHAPE_REPRESENTATION"
                    | "SHAPE_REPRESENTATION"
            )
        ) {
            continue;
        }
        if record.parameter(1).and_then(refs).is_some_and(|items| {
            items
                .iter()
                .any(|item| result.body_by_root.contains_key(item))
        }) {
            result.typed_records.insert(id);
        }
    }
    for face in &ir.model.faces {
        if let Some(source) = source_numeric_id(&face.id.0, "face") {
            result
                .faces_by_source
                .entry(source)
                .or_default()
                .push(face.id.clone());
        }
    }
    for edge in &ir.model.edges {
        if let Some(source) = source_numeric_id(&edge.id.0, "edge") {
            result
                .edges_by_source
                .entry(source)
                .or_default()
                .push(edge.id.clone());
        }
    }
    for vertex in &ir.model.vertices {
        if let Some(source) = source_numeric_id(&vertex.id.0, "vertex") {
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
    let Some(set_ids) = representation.parameter(1).and_then(refs) else {
        return Vec::new();
    };
    set_ids
        .into_iter()
        .filter_map(|set_id| exchange.records.get(&set_id))
        .filter(|set| has_type(set, "GEOMETRIC_SET") || has_type(set, "GEOMETRIC_CURVE_SET"))
        .flat_map(|set| set.parameter(1).and_then(refs).unwrap_or_default())
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

#[allow(
    clippy::too_many_arguments,
    reason = "Wire-set construction keeps source maps, decoded carriers, owner scope, and diagnostics explicit."
)]
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
    let mut typed = BTreeSet::from([id, set_id]);
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
        let ir_id = EdgeId(format!("step:data:edge#{edge_id}{edge_suffix}"));
        let vertex_suffix = format!("-wire-{id}-set-{set_id}");
        wire_edges.push(ir_id.clone());
        built_edges.push(Edge {
            id: ir_id,
            curve: edge_curve_id_reported(edge_id, edge, exchange, warnings),
            start: VertexId(format!("step:data:vertex#{start}{vertex_suffix}")),
            end: VertexId(format!("step:data:vertex#{end}{vertex_suffix}")),
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
            id: VertexId(format!("step:data:vertex#{vertex_id}{vertex_suffix}")),
            point: PointId(format!("step:data:point#{}", vertex.point)),
            tolerance: None,
        });
        typed.insert(vertex_id);
    }
    let body = BodyId(format!("step:data:body#{id}{suffix}"));
    let region = RegionId(format!("step:data:region#{id}{suffix}"));
    let shell = ShellId(format!("step:data:shell#{id}{suffix}"));
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

#[allow(
    clippy::too_many_arguments,
    reason = "Wire construction keeps source records, decoded carrier maps, and owner scope explicit."
)]
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
    let mut typed = BTreeSet::from([id, shell_id]);
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
        let ir_id = EdgeId(format!(
            "step:data:edge#{edge_id}-wire-{id}-{shell_id}-{oriented_id}-{index}"
        ));
        wire_edges.push(ir_id.clone());
        edges.push(Edge {
            id: ir_id,
            curve: edge_curve_id_reported(edge_id, edge, exchange, warnings),
            start: VertexId(format!("step:data:vertex#{start}{vertex_suffix}")),
            end: VertexId(format!("step:data:vertex#{end}{vertex_suffix}")),
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
                id: VertexId(format!("step:data:vertex#{vertex_id}{vertex_suffix}")),
                point: PointId(format!("step:data:point#{}", vertex.point)),
                tolerance: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let body = BodyId(format!("step:data:body#{id}{suffix}"));
    let region = RegionId(format!("step:data:region#{id}{suffix}"));
    let shell = shell_identity(id, shell_id, scope_root);
    let free_vertices = free_vertices
        .into_iter()
        .map(|vertex| VertexId(format!("step:data:vertex#{vertex}{vertex_suffix}")))
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
    typed: &mut BTreeSet<u64>,
) -> bool {
    let Some(set_ids) = representation.parameter(1).and_then(refs) else {
        return false;
    };
    let mut decoded = false;
    for set_id in set_ids {
        let Some(set) = exchange.records.get(&set_id) else {
            continue;
        };
        if !matches!(
            set.simple_name(),
            Some("GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET")
        ) {
            continue;
        }
        let Some(items) = set.parameter(1).and_then(refs) else {
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
    let set_ids = refs(representation.parameter(1)?)?;
    let mut typed = BTreeSet::from([id]);
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
            let surface = SurfaceId(format!("step:data:surface#{surface_step}"));
            if carrier_index.surfaces.contains_key(&surface_step) {
                surfaces.push((surface_step, surface));
            }
        }
    }
    if surfaces.is_empty() {
        return None;
    }
    let body = BodyId(format!("step:data:body#{id}"));
    let region = RegionId(format!("step:data:region#{id}"));
    let shell = ShellId(format!("step:data:shell#geometric-set-{id}"));
    let faces = surfaces
        .into_iter()
        .map(|(surface_step, surface)| Face {
            id: FaceId(format!("step:data:face#{surface_step}-geometric-set-{id}")),
            shell: shell.clone(),
            surface,
            sense: Sense::Forward,
            loops: Vec::new(),
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
    seam_edge: bool,
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
                seam_edge: false,
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
                seam_edge: false,
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
                seam_edge: parent_def.seam_edge,
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
                seam_edge: most_specific(record, &["SEAM_EDGE"]).is_some() || element_def.seam_edge,
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
                    partial.name.as_ref(),
                    "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                )
            })
        })
    {
        warnings.push(format!(
            "STEP edge curve #{edge_id}: surface-curve #{curve_step} has no resolvable basis; edge committed without a curve"
        ));
    }
    carrier.map(|curve| CurveId(format!("step:data:curve#{curve}")))
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
                    pcurve: r
                        .partials
                        .iter()
                        .find(|partial| partial.name.as_ref() == "SEAM_EDGE")
                        .and_then(|partial| {
                            partial
                                .parameters
                                .iter()
                                .rev()
                                .find_map(ValueExt::reference)
                        }),
                    seam_edge: most_specific(r, &["SEAM_EDGE"]).is_some(),
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
        .find(|partial| partial.name.as_ref() == name)
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
        .find(|partial| partial.name.as_ref() == name)
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
        .find(|partial| partial.name.as_ref() == name)
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
        .and_then(|partial| partial.parameters.get(1).and_then(refs))
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
        .find(|partial| partial.name.as_ref() == "EDGE")
        .or_else(|| {
            record
                .partials
                .iter()
                .find(|partial| partial.name.as_ref() == "EDGE_CURVE")
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
        .find(|partial| partial.name.as_ref() == "EDGE_CURVE")
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::reference))
}

fn edge_same_sense(record: &RawRecord) -> Option<bool> {
    if record.partials.len() == 1 {
        return entity_parameter(record, record.simple_name()?, 4)?.logical();
    }
    record
        .partials
        .iter()
        .find(|partial| partial.name.as_ref() == "EDGE_CURVE")
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::logical))
}

struct Built {
    typed: BTreeSet<u64>,
    draft: ModelDraft,
    body_id: BodyId,
    shell_sources: BTreeSet<u64>,
}

fn drop_committed_surfaces(draft: &mut ModelDraft, session: &CommitSession) {
    // Implicit surfaces can be staged by multiple roots. The session is the
    // authority on which ones a prior root committed; a pre-loop snapshot is
    // wrong because commits add surfaces while the loop is running.
    draft
        .model_mut()
        .surfaces
        .retain(|surface| !session.contains(surface.id.as_str()));
}

#[cfg(test)]
mod tests {
    use super::drop_committed_surfaces;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::draft::{CommitSession, ModelDraft};
    use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::Vertex;
    use cadmpeg_ir::units::Units;

    fn surface_draft(id: &str) -> ModelDraft {
        let mut draft = ModelDraft::new();
        draft
            .insert(Surface {
                id: SurfaceId(id.into()),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            })
            .expect("insert surface into draft");
        draft
    }

    #[test]
    fn cross_root_surface_filter_tracks_successful_commits_only() {
        let committed_id = "step:data:surface#implicit-face-1";
        let rejected_id = "step:data:surface#implicit-face-2";
        let mut ir = CadIr::empty(Units::default());
        let mut session = CommitSession::new(&ir);

        session
            .commit_model(surface_draft(committed_id), &mut ir)
            .expect("first root commit");
        let mut second_root = surface_draft(committed_id);
        drop_committed_surfaces(&mut second_root, &session);
        assert!(second_root.model().surfaces.is_empty());

        let mut rejected_root = surface_draft(rejected_id);
        rejected_root
            .insert(Vertex {
                id: "step:data:vertex#rejected".into(),
                point: "step:data:point#missing".into(),
                tolerance: None,
            })
            .expect("insert invalid root reference");
        assert!(session.commit_model(rejected_root, &mut ir).is_err());

        let mut later_root = surface_draft(rejected_id);
        drop_committed_surfaces(&mut later_root, &session);
        assert_eq!(later_root.model().surfaces.len(), 1);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Decode/encode helper keeps one parameter per independent arena, table, or control flag rather than a catch-all context struct."
)]
fn staged_topology(
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
    for surface in surfaces {
        draft.insert(surface).ok()?;
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
    })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RootKey {
    root_type: &'static str,
    shell_keys: Vec<(u64, Option<bool>)>,
}

#[derive(Clone)]
struct RootBuilt {
    body_ids: Vec<BodyId>,
    body_by_shell: BTreeMap<u64, BTreeSet<BodyId>>,
}

fn root_shell_steps(root: &RawRecord, exchange: &Exchange) -> Option<Vec<u64>> {
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
        return Some(ids);
    }
    None
}

fn root_key(
    root: &RawRecord,
    exchange: &Exchange,
    shell_definitions: &BTreeMap<u64, ShellDef>,
) -> Option<RootKey> {
    let root_type = if has_type(root, "SHELL_BASED_SURFACE_MODEL") {
        "SHELL_BASED_SURFACE_MODEL"
    } else if has_type(root, "FACE_BASED_SURFACE_MODEL") {
        "FACE_BASED_SURFACE_MODEL"
    } else if has_type(root, "BREP_WITH_VOIDS") {
        "BREP_WITH_VOIDS"
    } else if has_type(root, "FACETED_BREP") {
        "FACETED_BREP"
    } else if has_type(root, "MANIFOLD_SOLID_BREP") {
        "MANIFOLD_SOLID_BREP"
    } else {
        return None;
    };
    let mut shell_keys = Vec::new();
    let mut resolved = 0;
    for shell in root_shell_steps(root, exchange)? {
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
        root_type,
        shell_keys,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "Root construction keeps source maps, decoded optional carriers, and the collision scope explicit."
)]
fn build(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
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
    let Some(shell_steps) = root_shell_steps(root, exchange) else {
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
        let body = BodyId(format!("step:data:body#{id}"));
        let region = RegionId(format!("step:data:region#{id}"));
        let mut failure = None;
        let built = build_one(
            id,
            root,
            exchange,
            vdefs,
            edefs,
            odefs,
            shell_definitions,
            decoded_pcurves,
            point_positions,
            &shell_steps,
            body,
            &region,
            shell_steps.len() > 1 || scope_root,
            scope_root,
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
        let body = BodyId(format!(
            "step:data:body#{id}{}",
            suffix.as_deref().unwrap_or_default()
        ));
        let region = RegionId(format!(
            "step:data:region#{id}{}",
            suffix.as_deref().unwrap_or_default()
        ));
        if let Some(value) = build_one(
            id,
            root,
            exchange,
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

#[allow(
    clippy::too_many_arguments,
    reason = "Decode helper keeps independent source maps, carrier sets, owner scope, and destination identities explicit."
)]
fn build_one(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
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
    let mut typed = BTreeSet::from([id]);
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
        let face_steps = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
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
            require_carrier(
                connected_set_members(sr, set_type),
                failure,
                shell_step,
                "connected face set member list",
            )?
        } else {
            let shell_type = require_carrier(
                most_specific(sr, &["OPEN_SHELL", "CLOSED_SHELL"]),
                failure,
                shell_step,
                "shell type",
            )?;
            require_carrier(
                named_refs(sr, shell_type, 1),
                failure,
                shell_step,
                "shell face list",
            )?
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
                SurfaceId(format!("step:data:surface#{surface_step}"))
            } else {
                let surface_id = SurfaceId(format!(
                    "step:data:surface#implicit-face-{face_step}{face_suffix}"
                ));
                if implicit_surface_ids.insert(surface_id.clone()) {
                    surfaces.push(Surface {
                        id: surface_id.clone(),
                        geometry: require_carrier(
                            implicit_face_plane(
                                &face_info.bounds,
                                face_info.reverse_bound_orientation,
                                exchange,
                                vdefs,
                                edefs,
                                odefs,
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
            let fid = FaceId(format!("step:data:face#{face_step}{face_suffix}"));
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
                let bound_type = if has_type(br, "FACE_BOUND") {
                    "FACE_BOUND"
                } else {
                    "FACE_OUTER_BOUND"
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
                let lid = LoopId(format!(
                    "step:data:loop#{loop_step}-face-{face_step}{face_suffix}"
                ));
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
                        boundary_role: if has_type(br, "FACE_OUTER_BOUND") {
                            LoopBoundaryRole::Outer
                        } else {
                            LoopBoundaryRole::Inner
                        },
                        coedges: Vec::new(),
                        vertex_uses: vec![VertexUse {
                            vertex: scoped_vertex_id(
                                vertex_step,
                                id,
                                shell_step,
                                scope_edges,
                                scope_root,
                            ),
                            after: None,
                            pcurves: Vec::new(),
                        }],
                    });
                    loop_ids.push((has_type(br, "FACE_OUTER_BOUND"), lid));
                    used_v.insert((shell_step, vertex_step));
                    typed.extend([bound_step, loop_step]);
                    continue;
                }
                if has_type(lr, "POLY_LOOP") {
                    let bound_type = if has_type(br, "FACE_BOUND") {
                        "FACE_BOUND"
                    } else {
                        "FACE_OUTER_BOUND"
                    };
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
                        let cid = CoedgeId(format!(
                            "step:data:coedge#poly-{loop_step}-{index}-face-{face_step}{face_suffix}"
                        ));
                        coedge_ids.push(cid.clone());
                        coedges.push(Coedge {
                            id: cid,
                            owner_loop: lid.clone(),
                            edge: edge_id.clone(),
                            next: CoedgeId(String::new()),
                            previous: CoedgeId(String::new()),
                            radial_next: CoedgeId(String::new()),
                            sense: if (canonical_start, canonical_end) == (start_point, end_point) {
                                Sense::Forward
                            } else {
                                Sense::Reversed
                            },
                            pcurves: Vec::new(),
                            use_curve: None,
                            use_curve_parameter_range: None,
                        });
                        radial.entry(edge_id).or_default().push(coedges.len() - 1);
                        typed.insert(loop_step);
                    }
                    let n = coedge_ids.len();
                    let start = coedges.len() - n;
                    for i in 0..n {
                        coedges[start + i].next = coedge_ids[(i + 1) % n].clone();
                        coedges[start + i].previous = coedge_ids[(i + n - 1) % n].clone();
                    }
                    loops.push(Loop {
                        id: lid.clone(),
                        face: fid.clone(),
                        boundary_role: if has_type(br, "FACE_OUTER_BOUND") {
                            LoopBoundaryRole::Outer
                        } else {
                            LoopBoundaryRole::Inner
                        },
                        coedges: coedge_ids,
                        vertex_uses: Vec::new(),
                    });
                    loop_ids.push((has_type(br, "FACE_OUTER_BOUND"), lid));
                    typed.insert(bound_step);
                    continue;
                }
                if !has_type(lr, "EDGE_LOOP") {
                    note_failure(failure, loop_step, "edge loop carrier");
                    return None;
                }
                let bound_type = if has_type(br, "FACE_BOUND") {
                    "FACE_BOUND"
                } else {
                    "FACE_OUTER_BOUND"
                };
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
                    let seam_edge = o.seam_edge || edge.seam_edge;
                    let associated = if seam_edge {
                        let explicit_pcurve = surface_step.and_then(|surface_step| {
                            let pcurve_step = o.pcurve.or(edge.pcurve)?;
                            let pcurve = exchange.records.get(&pcurve_step)?;
                            let pcurve_id = PcurveId(format!("step:data:pcurve#{pcurve_step}"));
                            let edge_candidate = edge.curve.and_then(|curve| {
                                associated_pcurves(curve, surface_step, exchange, decoded_pcurves)
                                    .into_iter()
                                    .find(|candidate| candidate == &pcurve_id)
                            });
                            (has_type(pcurve, "PCURVE")
                                && entity_parameter(pcurve, "PCURVE", 1)?.reference()?
                                    == surface_step
                                && edge_candidate.is_some())
                            .then_some(pcurve_id)
                        });
                        if let Some(pcurve) = explicit_pcurve {
                            vec![pcurve]
                        } else {
                            losses.push(LossNote {
                                code: LossKind::ReferenceGraphNotClosed,
                                severity: Severity::Warning,
                                message: format!(
                                    "SEAM_EDGE #{use_step} has no decoded pcurve reference that belongs to its edge curve and face surface; the coedge has no pcurve"
                                ),
                                provenance: None,
                            });
                            Vec::new()
                        }
                    } else {
                        match (surface_step, edge.curve) {
                            (Some(surface_step), Some(curve)) => {
                                associated_pcurves(curve, surface_step, exchange, decoded_pcurves)
                            }
                            _ => {
                                losses.push(LossNote {
                                    code: LossKind::ReferenceGraphNotClosed,
                                    severity: Severity::Warning,
                                    message: format!(
                                        "edge #{} has no decoded surface or curve carrier, so its coedge has no pcurve",
                                        o.edge
                                    ),
                                    provenance: None,
                                });
                                Vec::new()
                            }
                        }
                    };
                    let pcurves = if seam_edge {
                        associated
                    } else {
                        match associated.len() {
                            0 | 1 => associated,
                            n => {
                                let message = match (edge.curve, surface_step) {
                                    (Some(curve), Some(surface)) => format!(
                                        "curve #{curve} associates {n} pcurves with surface #{surface}; no UV-continuity rule selects one, so the coedge has no pcurve"
                                    ),
                                    _ => format!(
                                        "coedge use #{use_step} has {n} pcurve candidates but its source surface or curve carrier is unresolved; no UV-continuity rule selects one, so the coedge has no pcurve"
                                    ),
                                };
                                losses.push(LossNote {
                                    code: LossKind::ReferenceGraphNotClosed,
                                    severity: Severity::Warning,
                                    message,
                                    provenance: None,
                                });
                                Vec::new()
                            }
                        }
                    };
                    let cid = CoedgeId(format!(
                        "step:data:coedge#{use_step}-face-{face_step}{face_suffix}"
                    ));
                    coedge_ids.push(cid.clone());
                    coedges.push(Coedge {
                        id: cid,
                        owner_loop: lid.clone(),
                        edge: scoped_edge_id(o.edge, id, shell_step, scope_edges, scope_root),
                        next: CoedgeId(String::new()),
                        previous: CoedgeId(String::new()),
                        radial_next: CoedgeId(String::new()),
                        sense: if (o.forward == edge.same) == bound_forward {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves: pcurves
                            .into_iter()
                            .map(|pcurve| PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            })
                            .collect(),
                        use_curve: None,
                        use_curve_parameter_range: None,
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
                let n = coedge_ids.len();
                let start = coedges.len() - n;
                for i in 0..n {
                    coedges[start + i].next = coedge_ids[(i + 1) % n].clone();
                    coedges[start + i].previous = coedge_ids[(i + n - 1) % n].clone();
                }
                loops.push(Loop {
                    id: lid.clone(),
                    face: fid.clone(),
                    boundary_role: if has_type(br, "FACE_OUTER_BOUND") {
                        LoopBoundaryRole::Outer
                    } else {
                        LoopBoundaryRole::Inner
                    },
                    coedges: coedge_ids,
                    vertex_uses: Vec::new(),
                });
                loop_ids.push((has_type(br, "FACE_OUTER_BOUND"), lid));
                typed.extend([bound_step, loop_step]);
            }
            let outer_count = loop_ids.iter().filter(|(outer, _)| *outer).count();
            if outer_count > 1 {
                let note = LossNote::new(
                    LossKind::SourceTopologyInvalid,
                    format!(
                        "face #{face_step} violates the STEP face-bound rule with {outer_count} FACE_OUTER_BOUND loops; retaining all explicit roles for diagnostics"
                    ),
                );
                losses.push(match exchange.records.get(&face_step) {
                    Some(record) => note.with_provenance(cadmpeg_ir::LossProvenance {
                        format: "step".into(),
                        stream: String::new(),
                        offset: record.span.start as u64,
                        tag: Some("advanced_face".into()),
                    }),
                    None => note,
                });
            }
            loop_ids.sort_by_key(|(outer, _)| !outer);
            let loop_ids = loop_ids.into_iter().map(|(_, id)| id).collect();
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
                loops: loop_ids,
                name: None,
                color: None,
                tolerance: None,
            });
            face_ids.push(fid);
            typed.insert(face_step);
        }
        shells.push(Shell {
            id: sid.clone(),
            region: rid.clone(),
            faces: face_ids,
            wire_edges: vec![],
            free_vertices: vec![],
        });
        region.shells.push(sid);
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
            point: PointId(format!("step:data:point#{}", v.point)),
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
            point: PointId(format!("step:data:point#{point_id}")),
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
        if loop_.coedges.is_empty() {
            continue;
        }
        let loop_source = source_numeric_id(loop_.id.as_str(), "loop").unwrap_or(0);
        for (index, current_id) in loop_.coedges.iter().enumerate() {
            let next_id = &loop_.coedges[(index + 1) % loop_.coedges.len()];
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

fn shell_identity(root_id: u64, shell_step: u64, scope_root: bool) -> ShellId {
    if scope_root {
        ShellId(format!("step:data:shell#{shell_step}-root-{root_id}"))
    } else {
        ShellId(format!("step:data:shell#{shell_step}"))
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
            EdgeId(format!(
                "step:data:edge#{edge_step}-root-{root_id}-shell-{shell_step}"
            ))
        } else {
            EdgeId(format!("step:data:edge#{edge_step}-shell-{shell_step}"))
        }
    } else {
        EdgeId(format!("step:data:edge#{edge_step}"))
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
            VertexId(format!(
                "step:data:vertex#{vertex_step}-root-{root_id}-shell-{shell_step}"
            ))
        } else {
            VertexId(format!("step:data:vertex#{vertex_step}-shell-{shell_step}"))
        }
    } else {
        VertexId(format!("step:data:vertex#{vertex_step}"))
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
            VertexId(format!(
                "step:data:vertex#poly-point-{point_step}-root-{root_id}-shell-{shell_step}"
            ))
        } else {
            VertexId(format!(
                "step:data:vertex#poly-point-{point_step}-shell-{shell_step}"
            ))
        }
    } else {
        VertexId(format!("step:data:vertex#poly-point-{point_step}"))
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
            EdgeId(format!(
                "step:data:edge#poly-{start}-{end}-root-{root_id}-shell-{shell_step}"
            ))
        } else {
            EdgeId(format!(
                "step:data:edge#poly-{start}-{end}-shell-{shell_step}"
            ))
        }
    } else {
        EdgeId(format!("step:data:edge#poly-{start}-{end}"))
    }
}

fn implicit_face_points(
    bounds: &[u64],
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    odefs: &BTreeMap<u64, OrientedDef>,
    point_positions: &CarrierIndex,
) -> Vec<Point3> {
    let mut points = Vec::new();
    for &bound_step in bounds {
        let Some(bound) = exchange.records.get(&bound_step) else {
            continue;
        };
        let Some(loop_step) = named_reference(
            bound,
            if has_type(bound, "FACE_OUTER_BOUND") {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            },
            1,
            0,
        ) else {
            continue;
        };
        let Some(loop_record) = exchange.records.get(&loop_step) else {
            continue;
        };
        let candidates = if has_type(loop_record, "POLY_LOOP") {
            named_refs(loop_record, "POLY_LOOP", 1)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>()
        } else if has_type(loop_record, "VERTEX_LOOP") {
            named_reference(loop_record, "VERTEX_LOOP", 1, 0)
                .into_iter()
                .collect::<Vec<_>>()
        } else if has_type(loop_record, "EDGE_LOOP") {
            named_refs(loop_record, "EDGE_LOOP", 1)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|oriented_step| odefs.get(&oriented_step))
                .filter_map(|oriented| {
                    let edge = edefs.get(&oriented.edge)?;
                    Some(if oriented.forward {
                        [edge.start, edge.end]
                    } else {
                        [edge.end, edge.start]
                    })
                })
                .flatten()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for vertex_or_point in candidates {
            let point_step = vdefs
                .get(&vertex_or_point)
                .map_or(vertex_or_point, |vertex| vertex.point);
            let Some(point) = point_positions.get(point_step).copied() else {
                continue;
            };
            points.push(point);
        }
    }
    points
}

fn implicit_face_plane(
    bounds: &[u64],
    reverse_bound_orientation: bool,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    odefs: &BTreeMap<u64, OrientedDef>,
    point_positions: &CarrierIndex,
) -> Option<SurfaceGeometry> {
    const RELATIVE_COLLINEAR_TOLERANCE: f64 = 1.0e-12;
    let bound_step = bounds
        .iter()
        .copied()
        .find(|bound| {
            exchange
                .records
                .get(bound)
                .is_some_and(|record| has_type(record, "FACE_OUTER_BOUND"))
        })
        .or_else(|| bounds.first().copied())?;
    let bound = exchange.records.get(&bound_step)?;
    let bound_type = if has_type(bound, "FACE_OUTER_BOUND") {
        "FACE_OUTER_BOUND"
    } else {
        "FACE_BOUND"
    };
    let bound_forward = named_logical(bound, bound_type, 2, 0).unwrap_or(true);
    let bound_forward = if reverse_bound_orientation {
        !bound_forward
    } else {
        bound_forward
    };
    let points = implicit_face_points(
        std::slice::from_ref(&bound_step),
        exchange,
        vdefs,
        edefs,
        odefs,
        point_positions,
    );
    if points.len() < 3 {
        return None;
    }
    let origin = *points.first()?;
    let mut normal = Vector3::new(0.0, 0.0, 0.0);
    for (&current, &next) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        normal.x += (current.y - next.y) * (current.z + next.z);
        normal.y += (current.z - next.z) * (current.x + next.x);
        normal.z += (current.x - next.x) * (current.y + next.y);
    }
    let extent = points
        .iter()
        .map(|point| point.distance(origin))
        .filter(|distance| distance.is_finite())
        .fold(0.0, f64::max);
    let area_scale = extent.max(f64::MIN_POSITIVE).powi(2);
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= RELATIVE_COLLINEAR_TOLERANCE * area_scale {
        return None;
    }
    normal = normal.unit()?;
    if !bound_forward {
        normal = normal.scale(-1.0);
    }
    let tangent_tolerance = RELATIVE_COLLINEAR_TOLERANCE * extent.max(f64::MIN_POSITIVE);
    let u_axis = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .find_map(|(&current, &next)| {
            let edge = next.vector_from(current);
            let tangent = edge - normal.scale(edge.dot(normal));
            (tangent.norm() > tangent_tolerance)
                .then(|| tangent.unit())
                .flatten()
        })?;
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
            partial.name.as_ref(),
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
            partial.name.as_ref(),
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
            let pcurve_id = PcurveId(format!("step:data:pcurve#{pcurve_step}"));
            (has_type(pcurve, "PCURVE")
                && entity_parameter(pcurve, "PCURVE", 1)?.reference()? == surface_step
                && decoded_pcurves.contains(&pcurve_id))
            .then_some(pcurve_id)
        })
        .collect()
}

#[derive(Clone)]
struct ShellDef {
    base: u64,
    forward: bool,
    typed: BTreeSet<u64>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SourceTopologyKey {
    Edge(u64),
    Point(u64),
    Vertex(u64),
}

struct SourceShellDiagnostic {
    shell: u64,
    shell_type: &'static str,
    face_count: usize,
    components: usize,
}

fn source_invalid_shells(
    exchange: &Exchange,
    shell_definitions: &BTreeMap<u64, ShellDef>,
    edge_definitions: &BTreeMap<u64, EdgeDef>,
    oriented_definitions: &BTreeMap<u64, OrientedDef>,
) -> Vec<SourceShellDiagnostic> {
    let root_types = [
        "SHELL_BASED_SURFACE_MODEL",
        "FACE_BASED_SURFACE_MODEL",
        "FACETED_BREP",
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
    ];
    let mut shell_steps = BTreeSet::new();
    for (_, root) in exchange.entities_any(&root_types) {
        let Some(references) = root_shell_steps(root, exchange) else {
            continue;
        };
        for reference in references {
            let shell = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
                Some(reference)
            } else {
                shell_definitions
                    .get(&reference)
                    .map(|definition| definition.base)
            };
            if let Some(shell) = shell {
                shell_steps.insert(shell);
            }
        }
    }

    shell_steps
        .into_iter()
        .filter_map(|shell| {
            let record = exchange.records.get(&shell)?;
            let shell_type = most_specific(
                record,
                &[
                    "OPEN_SHELL",
                    "CLOSED_SHELL",
                    "CONNECTED_FACE_SUB_SET",
                    "CONNECTED_FACE_SET",
                ],
            )?;
            let faces = source_shell_faces(record, shell_type)?;
            if faces.len() < 2 {
                return None;
            }
            let face_keys = faces
                .into_iter()
                .map(|face| {
                    source_face_topology_keys(
                        face,
                        exchange,
                        edge_definitions,
                        oriented_definitions,
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            let components = source_face_components(&face_keys);
            (components > 1).then_some(SourceShellDiagnostic {
                shell,
                shell_type,
                face_count: face_keys.len(),
                components,
            })
        })
        .collect()
}

fn source_shell_faces(record: &RawRecord, shell_type: &str) -> Option<Vec<u64>> {
    match shell_type {
        "OPEN_SHELL" | "CLOSED_SHELL" => named_refs(record, shell_type, 1),
        "CONNECTED_FACE_SUB_SET" | "CONNECTED_FACE_SET" => {
            connected_set_members(record, shell_type)
        }
        _ => None,
    }
}

fn source_face_topology_keys(
    face_step: u64,
    exchange: &Exchange,
    edge_definitions: &BTreeMap<u64, EdgeDef>,
    oriented_definitions: &BTreeMap<u64, OrientedDef>,
) -> Option<BTreeSet<SourceTopologyKey>> {
    let face = exchange.records.get(&face_step)?;
    let face_info = face_attributes(face, exchange, &mut BTreeSet::new())?;
    let mut keys = BTreeSet::new();
    for bound_step in face_info.bounds {
        let bound = exchange.records.get(&bound_step)?;
        let bound_type = most_specific(bound, &["FACE_OUTER_BOUND", "FACE_BOUND"])?;
        let loop_step = named_reference(bound, bound_type, 1, 0)?;
        let loop_record = exchange.records.get(&loop_step)?;
        if has_type(loop_record, "VERTEX_LOOP") {
            keys.insert(SourceTopologyKey::Vertex(named_reference(
                loop_record,
                "VERTEX_LOOP",
                1,
                0,
            )?));
            continue;
        }
        if has_type(loop_record, "POLY_LOOP") {
            for point in named_refs(loop_record, "POLY_LOOP", 1)? {
                keys.insert(SourceTopologyKey::Point(point));
            }
            continue;
        }
        if !has_type(loop_record, "EDGE_LOOP") {
            return None;
        }
        for oriented_step in named_refs(loop_record, "EDGE_LOOP", 1)? {
            let oriented = oriented_definitions.get(&oriented_step)?;
            let edge = edge_definitions.get(&oriented.edge)?;
            keys.insert(SourceTopologyKey::Edge(oriented.edge));
            keys.insert(SourceTopologyKey::Vertex(edge.start));
            keys.insert(SourceTopologyKey::Vertex(edge.end));
        }
    }
    (!keys.is_empty()).then_some(keys)
}

fn source_face_components(face_keys: &[BTreeSet<SourceTopologyKey>]) -> usize {
    let mut faces_by_key = BTreeMap::<SourceTopologyKey, Vec<usize>>::new();
    for (face, keys) in face_keys.iter().enumerate() {
        for &key in keys {
            faces_by_key.entry(key).or_default().push(face);
        }
    }
    let mut remaining = (0..face_keys.len()).collect::<BTreeSet<_>>();
    let mut components = 0;
    while let Some(&start) = remaining.first() {
        components += 1;
        let mut pending = vec![start];
        remaining.remove(&start);
        while let Some(face) = pending.pop() {
            for key in &face_keys[face] {
                let Some(neighbors) = faces_by_key.get(key) else {
                    continue;
                };
                for &neighbor in neighbors {
                    if remaining.remove(&neighbor) {
                        pending.push(neighbor);
                    }
                }
            }
        }
    }
    components
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
                typed: BTreeSet::new(),
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
    typed: &mut BTreeSet<u64>,
) -> Option<(u64, bool)> {
    let definition = shells.get(&reference)?;
    typed.extend(definition.typed.iter().copied());
    Some((definition.base, definition.forward))
}

#[derive(Default)]
struct FaceInfo {
    bounds: Vec<u64>,
    surface: Option<u64>,
    same_sense: bool,
    reverse_bound_orientation: bool,
    typed: BTreeSet<u64>,
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
            base.typed.insert(face_element);
            Some(base)
        }
        "SUBFACE" => {
            let parent = subface_parent(record)?;
            let mut parent_info =
                face_attributes(exchange.records.get(&parent)?, exchange, active)?;
            let bounds = direct_face_bounds(record, exchange)?;
            parent_info.typed.insert(parent);
            Some(FaceInfo {
                bounds,
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
                surface: None,
                same_sense: true,
                reverse_bound_orientation: false,
                typed: BTreeSet::new(),
            })
        }
        "ADVANCED_FACE" | "FACE_SURFACE" => {
            let bounds = direct_face_bounds(record, exchange)?;
            let governing = most_specific(record, &["ADVANCED_FACE", "FACE_SURFACE"])?;
            let surface = direct_face_surface(record, &bounds, governing)?;
            let same_sense = direct_face_same_sense(record, governing)?;
            Some(FaceInfo {
                bounds,
                surface: Some(surface),
                same_sense,
                reverse_bound_orientation: false,
                typed: BTreeSet::new(),
            })
        }
        _ => None,
    })();
    active.remove(&record.id);
    result
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
    if let Some(partial) = record
        .partials
        .iter()
        .find(|p| p.name.as_ref() == "ORIENTED_FACE")
    {
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
        .find(|partial| partial.name.as_ref() == "ORIENTED_FACE")
        .into_iter()
        .flat_map(|partial| partial.parameters.iter())
        .find_map(ValueExt::logical)
        .or_else(|| direct_face_same_sense(record, "ORIENTED_FACE"))
}

fn subface_parent(record: &RawRecord) -> Option<u64> {
    record
        .partials
        .iter()
        .find(|partial| partial.name.as_ref() == "SUBFACE")
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
    record
        .partials
        .iter()
        .any(|partial| partial.name.as_ref() == name)
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
        .find(|partial| partial.name.as_ref() == name)
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
        (self.partials.len() == 1).then(|| self.partials[0].name.as_ref())
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials
            .iter()
            .find(|partial| partial.name.as_ref() == name)
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
