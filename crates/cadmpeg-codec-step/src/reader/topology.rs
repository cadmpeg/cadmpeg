// SPDX-License-Identifier: Apache-2.0
//! STEP boundary-representation ownership and orientation decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::ModelDraft;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, PcurveUse, Region, Sense, Shell,
    Vertex, VertexUse,
};

use crate::parse::{Exchange, RawRecord, Value};

pub(super) struct TopologyResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
}

pub(super) fn decode(exchange: &Exchange, ir: &mut CadIr) -> TopologyResult {
    let mut result = TopologyResult {
        typed_records: BTreeSet::new(),
        warnings: Vec::new(),
    };
    let vertices = vertex_defs(exchange);
    let edges = edge_defs(exchange);
    let oriented = oriented_defs(exchange);
    let decoded_points = ir
        .model
        .points
        .iter()
        .map(|point| point.id.clone())
        .collect::<BTreeSet<_>>();
    for (&vertex_id, vertex) in &exchange.records {
        if !has_type(vertex, "VERTEX_POINT") {
            continue;
        }
        let Some(point_id) = named_reference(vertex, "VERTEX_POINT", 1, 0) else {
            result.warnings.push(format!(
                "VERTEX_POINT #{vertex_id} has no resolvable point carrier"
            ));
            continue;
        };
        if !decoded_points.contains(&PointId(format!("step:data:point#{point_id}"))) {
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
            continue;
        }
        if let Some(mut built) = build_wire(model, exchange, &vertices, &edges) {
            if let Err(error) = built.draft.commit_model(ir) {
                result.warnings.push(format!(
                    "EDGE_BASED_WIREFRAME_MODEL #{model} conflicts with decoded topology: {error}"
                ));
            } else {
                built_wire_models.insert(model);
                built.typed.insert(representation);
                result.typed_records.append(&mut built.typed);
            }
        } else {
            result.warnings.push(format!(
                "EDGE_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
            ));
        }
    }
    for (&model, record) in &exchange.records {
        if !has_type(record, "SHELL_BASED_WIREFRAME_MODEL") {
            continue;
        }
        if let Some(mut built) = build_shell_wire(model, exchange, &vertices, &edges) {
            if let Err(error) = built.draft.commit_model(ir) {
                result.warnings.push(format!(
                    "SHELL_BASED_WIREFRAME_MODEL #{model} conflicts with decoded topology: {error}"
                ));
            } else {
                result.typed_records.append(&mut built.typed);
            }
        } else {
            result.warnings.push(format!(
                "SHELL_BASED_WIREFRAME_MODEL #{model} does not resolve to connected edges"
            ));
        }
    }
    let geometry_ids = GeometryIds {
        points: ir
            .model
            .points
            .iter()
            .map(|point| point.id.0.clone())
            .collect(),
        curves: ir
            .model
            .curves
            .iter()
            .map(|curve| curve.id.0.clone())
            .collect(),
        surfaces: ir
            .model
            .surfaces
            .iter()
            .map(|surface| surface.id.0.clone())
            .collect(),
    };
    let decoded_pcurves = ir
        .model
        .pcurves
        .iter()
        .map(|pcurve| pcurve.id.clone())
        .collect::<BTreeSet<_>>();
    for (&id, record) in &exchange.records {
        if ![
            "SHELL_BASED_SURFACE_MODEL",
            "FACE_BASED_SURFACE_MODEL",
            "FACETED_BREP",
            "MANIFOLD_SOLID_BREP",
            "BREP_WITH_VOIDS",
        ]
        .iter()
        .any(|name| has_type(record, name))
        {
            continue;
        }
        if let Some(mut built) = build(
            id,
            record,
            exchange,
            &vertices,
            &edges,
            &oriented,
            &decoded_pcurves,
        ) {
            if let Err(error) = built.draft.commit_model(ir) {
                result.warnings.push(format!(
                    "STEP topology root #{id} conflicts with decoded topology: {error}",
                ));
            } else {
                result.typed_records.append(&mut built.typed);
            }
        } else {
            result.warnings.push(format!(
                "STEP topology root #{id} does not resolve to a complete connected topology graph",
            ));
        }
    }
    for (&id, record) in &exchange.records {
        if record.simple_name() != Some("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION") {
            continue;
        }
        let omitted = geometric_set_omissions(record, exchange, &geometry_ids);
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
        let Some(mut built) = build_geometric_set(id, record, exchange, &geometry_ids) else {
            if mark_standalone_geometric_set(
                id,
                record,
                exchange,
                &geometry_ids,
                &mut result.typed_records,
            ) {
                continue;
            }
            result.warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no decoded bounded surfaces"
            ));
            continue;
        };
        if let Err(error) = built.draft.commit_model(ir) {
            result.warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} conflicts with decoded topology: {error}"
            ));
        } else {
            result.typed_records.append(&mut built.typed);
        }
    }
    for (&id, record) in &exchange.records {
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
        mark_standalone_geometric_set(
            id,
            record,
            exchange,
            &geometry_ids,
            &mut result.typed_records,
        );
    }
    let decoded_body_items = ir
        .model
        .bodies
        .iter()
        .filter_map(|body| {
            body.id
                .as_str()
                .strip_prefix("step:data:body#")?
                .parse()
                .ok()
        })
        .collect::<BTreeSet<u64>>();
    for (&id, record) in &exchange.records {
        if matches!(
            record.simple_name(),
            Some(
                "MANIFOLD_SURFACE_SHAPE_REPRESENTATION"
                    | "ADVANCED_BREP_SHAPE_REPRESENTATION"
                    | "SHAPE_REPRESENTATION"
            )
        ) && record
            .parameter(1)
            .and_then(refs)
            .is_some_and(|items| items.iter().any(|item| decoded_body_items.contains(item)))
        {
            result.typed_records.insert(id);
        }
    }
    result
}

fn geometric_set_omissions(
    representation: &RawRecord,
    exchange: &Exchange,
    geometry_ids: &GeometryIds,
) -> Vec<u64> {
    let Some(set_ids) = representation.parameter(1).and_then(refs) else {
        return Vec::new();
    };
    set_ids
        .into_iter()
        .filter_map(|set_id| exchange.records.get(&set_id))
        .filter(|set| has_type(set, "GEOMETRIC_SET"))
        .flat_map(|set| set.parameter(1).and_then(refs).unwrap_or_default())
        .filter(|member| {
            !geometry_ids
                .surfaces
                .contains(&format!("step:data:surface#{member}"))
        })
        .collect()
}

fn build_wire(
    id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
) -> Option<Built> {
    let model = exchange.records.get(&id)?;
    let sets = named_refs(model, "EDGE_BASED_WIREFRAME_MODEL", 1)?;
    let mut typed = BTreeSet::from([id]);
    let mut used_edges = BTreeSet::new();
    for set_id in sets {
        let set = exchange.records.get(&set_id)?;
        if !has_type(set, "CONNECTED_EDGE_SET") && !has_type(set, "CONNECTED_EDGE_SUB_SET") {
            return None;
        }
        let set_type = if has_type(set, "CONNECTED_EDGE_SET") {
            "CONNECTED_EDGE_SET"
        } else {
            "CONNECTED_EDGE_SUB_SET"
        };
        used_edges.extend(named_refs(set, set_type, 1)?);
        typed.insert(set_id);
    }
    if used_edges.is_empty() {
        return None;
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
        let ir_id = EdgeId(format!("step:data:edge#{edge_id}"));
        wire_edges.push(ir_id.clone());
        built_edges.push(Edge {
            id: ir_id,
            curve: Some(CurveId(format!(
                "step:data:curve#{}",
                curve_carrier_step(edge.curve, exchange)?
            ))),
            start: VertexId(format!("step:data:vertex#{start}")),
            end: VertexId(format!("step:data:vertex#{end}")),
            param_range: None,
            tolerance: None,
        });
        used_vertices.extend([start, end]);
        typed.insert(edge_id);
        if let Some(parent) = edge.parent {
            typed.insert(parent);
        }
    }
    let mut built_vertices = Vec::new();
    for vertex_id in used_vertices {
        let vertex = vdefs.get(&vertex_id)?;
        built_vertices.push(Vertex {
            id: VertexId(format!("step:data:vertex#{vertex_id}")),
            point: PointId(format!("step:data:point#{}", vertex.point)),
            tolerance: None,
        });
        typed.insert(vertex_id);
    }
    let body = BodyId(format!("step:data:body#{id}"));
    let region = RegionId(format!("step:data:region#{id}"));
    let shell = ShellId(format!("step:data:shell#{id}"));
    staged_topology(
        typed,
        built_vertices,
        built_edges,
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
            id: body,
            kind: BodyKind::Wire,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    )
}

fn build_shell_wire(
    id: u64,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
) -> Option<Built> {
    let model = exchange.records.get(&id)?;
    let shell_ids = named_refs(model, "SHELL_BASED_WIREFRAME_MODEL", 1)?;
    let mut typed = BTreeSet::from([id]);
    let mut used_edges = BTreeSet::new();
    let mut used_vertices = BTreeSet::new();
    let mut free_vertices = BTreeSet::new();
    for shell_id in shell_ids {
        let shell = exchange.records.get(&shell_id)?;
        if has_type(shell, "WIRE_SHELL") {
            for loop_id in named_refs(shell, "WIRE_SHELL", 1)? {
                let loop_record = exchange.records.get(&loop_id)?;
                if has_type(loop_record, "EDGE_LOOP") {
                    for oriented_id in named_refs(loop_record, "EDGE_LOOP", 1)? {
                        let oriented = exchange.records.get(&oriented_id)?;
                        let edge_id = oriented_edge_reference(oriented)?;
                        let edge = edefs.get(&edge_id)?;
                        used_edges.insert(edge_id);
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
        } else if has_type(shell, "VERTEX_SHELL") {
            let loop_id = named_reference(shell, "VERTEX_SHELL", 1, 0)?;
            let loop_record = exchange.records.get(&loop_id)?;
            if !has_type(loop_record, "VERTEX_LOOP") {
                return None;
            }
            let vertex = named_reference(loop_record, "VERTEX_LOOP", 1, 0)?;
            used_vertices.insert(vertex);
            free_vertices.insert(vertex);
            typed.extend([shell_id, loop_id, vertex]);
        } else {
            return None;
        }
        typed.insert(shell_id);
    }
    if used_edges.is_empty() && used_vertices.is_empty() {
        return None;
    }
    let edges = used_edges
        .into_iter()
        .map(|edge_id| {
            let edge = edefs.get(&edge_id)?;
            Some(Edge {
                id: EdgeId(format!("step:data:edge#{edge_id}")),
                curve: Some(CurveId(format!(
                    "step:data:curve#{}",
                    curve_carrier_step(edge.curve, exchange)?
                ))),
                start: VertexId(format!("step:data:vertex#{}", edge.start)),
                end: VertexId(format!("step:data:vertex#{}", edge.end)),
                param_range: None,
                tolerance: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let vertices = used_vertices
        .into_iter()
        .map(|vertex_id| {
            let vertex = vdefs.get(&vertex_id)?;
            Some(Vertex {
                id: VertexId(format!("step:data:vertex#{vertex_id}")),
                point: PointId(format!("step:data:point#{}", vertex.point)),
                tolerance: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let body = BodyId(format!("step:data:body#{id}"));
    let region = RegionId(format!("step:data:region#{id}"));
    let shell = ShellId(format!("step:data:shell#{id}"));
    let wire_edges = edges.iter().map(|edge| edge.id.clone()).collect();
    let free_vertices = free_vertices
        .into_iter()
        .map(|vertex| VertexId(format!("step:data:vertex#{vertex}")))
        .collect();
    staged_topology(
        typed,
        vertices,
        edges,
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
            id: body,
            kind: BodyKind::Wire,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    )
}

fn mark_standalone_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    geometry_ids: &GeometryIds,
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
            let point = format!("step:data:point#{item}");
            let curve = format!("step:data:curve#{item}");
            geometry_ids.points.contains(&point) || geometry_ids.curves.contains(&curve)
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
    geometry_ids: &GeometryIds,
) -> Option<Built> {
    let set_ids = refs(representation.parameter(1)?)?;
    let mut typed = BTreeSet::from([id]);
    let mut surfaces = Vec::new();
    for set_id in set_ids {
        let set = exchange.records.get(&set_id)?;
        if set.simple_name() != Some("GEOMETRIC_SET") {
            continue;
        }
        typed.insert(set_id);
        for surface_step in refs(set.parameter(1)?)? {
            let surface = SurfaceId(format!("step:data:surface#{surface_step}"));
            if geometry_ids.surfaces.contains(surface.as_str()) {
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

struct GeometryIds {
    points: BTreeSet<String>,
    curves: BTreeSet<String>,
    surfaces: BTreeSet<String>,
}

#[derive(Clone)]
struct VertexDef {
    point: u64,
}
#[derive(Clone)]
struct EdgeDef {
    start: u64,
    end: u64,
    curve: u64,
    same: bool,
    parent: Option<u64>,
}
#[derive(Clone)]
struct OrientedDef {
    edge: u64,
    forward: bool,
    pcurve: Option<u64>,
}

fn vertex_defs(exchange: &Exchange) -> BTreeMap<u64, VertexDef> {
    exchange
        .records
        .iter()
        .filter_map(|(&id, r)| {
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
    exchange
        .records
        .iter()
        .filter_map(|(&id, _)| {
            edge_def_for(id, exchange, &mut BTreeSet::new()).map(|edge| (id, edge))
        })
        .collect()
}

fn edge_def_for(id: u64, exchange: &Exchange, active: &mut BTreeSet<u64>) -> Option<EdgeDef> {
    if !active.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    let result = if has_type(record, "EDGE_CURVE") {
        let (start, end) = edge_vertices(record)?;
        Some(EdgeDef {
            start,
            end,
            curve: edge_geometry(record)?,
            same: edge_same_sense(record)?,
            parent: None,
        })
    } else if has_type(record, "SUBEDGE") {
        let (start, end) = edge_vertices(record)?;
        let parent = named_reference(record, "SUBEDGE", 3, 0).or_else(|| {
            record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .filter_map(ValueExt::reference)
                .next_back()
        })?;
        let parent_def = edge_def_for(parent, exchange, active)?;
        Some(EdgeDef {
            start,
            end,
            curve: parent_def.curve,
            same: parent_def.same,
            parent: Some(parent),
        })
    } else {
        None
    };
    active.remove(&id);
    result
}
fn oriented_defs(exchange: &Exchange) -> BTreeMap<u64, OrientedDef> {
    exchange
        .records
        .iter()
        .filter_map(|(&id, r)| {
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
        .and_then(|partial| partial.parameters.iter().find_map(ValueExt::reference))
}

fn surface_curve_pcurves(record: &RawRecord) -> Option<Vec<u64>> {
    if record.partials.len() == 1 {
        return record.parameter(2).and_then(refs);
    }
    record
        .partial("SURFACE_CURVE")
        .or_else(|| record.partial("SEAM_CURVE"))
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
    typed: BTreeSet<u64>,
    draft: ModelDraft,
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
    for shell in shells {
        draft.insert(shell).ok()?;
    }
    draft.insert(region).ok()?;
    draft.insert(body).ok()?;
    Some(Built { typed, draft })
}

fn build(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    odefs: &BTreeMap<u64, OrientedDef>,
    decoded_pcurves: &BTreeSet<PcurveId>,
) -> Option<Built> {
    let solid = has_type(root, "MANIFOLD_SOLID_BREP")
        || has_type(root, "BREP_WITH_VOIDS")
        || has_type(root, "FACETED_BREP");
    let shell_steps = if has_type(root, "SHELL_BASED_SURFACE_MODEL") {
        named_refs(root, "SHELL_BASED_SURFACE_MODEL", 1)?
    } else if has_type(root, "FACE_BASED_SURFACE_MODEL") {
        let mut sets = Vec::new();
        for set_step in named_refs(root, "FACE_BASED_SURFACE_MODEL", 1)? {
            let set = exchange.records.get(&set_step)?;
            connected_face_set_type(set)?;
            sets.push(set_step);
        }
        sets
    } else if (has_type(root, "MANIFOLD_SOLID_BREP") || has_type(root, "FACETED_BREP"))
        && !has_type(root, "BREP_WITH_VOIDS")
    {
        let root_type = if has_type(root, "MANIFOLD_SOLID_BREP") {
            "MANIFOLD_SOLID_BREP"
        } else {
            "FACETED_BREP"
        };
        vec![named_reference(root, root_type, 1, 0)?]
    } else if has_type(root, "BREP_WITH_VOIDS") {
        let mut ids = vec![named_reference(root, "MANIFOLD_SOLID_BREP", 1, 0)?];
        ids.extend(named_refs(root, "BREP_WITH_VOIDS", 2)?);
        ids
    } else {
        return None;
    };
    let bid = BodyId(format!("step:data:body#{id}"));
    let rid = RegionId(format!("step:data:region#{id}"));
    let mut typed = BTreeSet::from([id]);
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    let mut coedges = Vec::new();
    let mut loops = Vec::new();
    let mut faces = Vec::new();
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
    let mut used_v = BTreeSet::new();
    let mut used_e = BTreeSet::new();
    let mut used_shells = BTreeSet::new();
    let mut used_faces = BTreeSet::new();
    let mut radial = BTreeMap::<EdgeId, Vec<usize>>::new();
    let mut poly_edges = BTreeMap::<EdgeId, (u64, u64)>::new();
    let mut poly_points = BTreeSet::new();
    let mut pcurve_use_counts = BTreeMap::<(u64, u64), usize>::new();
    for shell_reference in shell_steps {
        let (shell_step, shell_forward) = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
            typed.insert(shell_reference);
            (shell_reference, true)
        } else {
            resolve_shell(shell_reference, exchange, &mut typed)?
        };
        if !used_shells.insert(shell_step) {
            continue;
        }
        let sr = exchange.records.get(&shell_step)?;
        let face_steps = if has_type(root, "FACE_BASED_SURFACE_MODEL") {
            named_refs(sr, connected_face_set_type(sr)?, 1)?
        } else {
            if !has_type(sr, "OPEN_SHELL") && !has_type(sr, "CLOSED_SHELL") {
                return None;
            }
            let shell_type = if has_type(sr, "OPEN_SHELL") {
                "OPEN_SHELL"
            } else {
                "CLOSED_SHELL"
            };
            named_refs(sr, shell_type, 1)?
        };
        if face_steps.is_empty() {
            return None;
        }
        let sid = ShellId(format!("step:data:shell#{shell_step}"));
        let mut face_ids = vec![];
        for face_step in face_steps {
            if !used_faces.insert(face_step) {
                continue;
            }
            let fr = exchange.records.get(&face_step)?;
            if !is_face_record(fr) {
                return None;
            }
            let face_info = face_attributes(fr, exchange, &mut BTreeSet::new())?;
            typed.extend(face_info.typed);
            let surface_step = face_info.surface;
            let face_same_sense = face_info.same_sense;
            let fid = FaceId(format!("step:data:face#{face_step}"));
            let mut loop_ids = vec![];
            for bound_step in face_info.bounds {
                let br = exchange.records.get(&bound_step)?;
                if !has_type(br, "FACE_BOUND") && !has_type(br, "FACE_OUTER_BOUND") {
                    return None;
                }
                let bound_type = if has_type(br, "FACE_BOUND") {
                    "FACE_BOUND"
                } else {
                    "FACE_OUTER_BOUND"
                };
                let loop_step = named_reference(br, bound_type, 1, 0)?;
                let lr = exchange.records.get(&loop_step)?;
                let lid = LoopId(format!("step:data:loop#{loop_step}-face-{face_step}"));
                if has_type(lr, "VERTEX_LOOP") {
                    let vertex_step = named_reference(lr, "VERTEX_LOOP", 1, 0)?;
                    if !vdefs.contains_key(&vertex_step) {
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
                            vertex: VertexId(format!("step:data:vertex#{vertex_step}")),
                            after: None,
                            pcurves: Vec::new(),
                        }],
                    });
                    loop_ids.push((has_type(br, "FACE_OUTER_BOUND"), lid));
                    used_v.insert(vertex_step);
                    typed.extend([bound_step, loop_step]);
                    continue;
                }
                if has_type(lr, "POLY_LOOP") {
                    let bound_type = if has_type(br, "FACE_BOUND") {
                        "FACE_BOUND"
                    } else {
                        "FACE_OUTER_BOUND"
                    };
                    let bound_forward = named_logical(br, bound_type, 2, 0)?;
                    let mut points = named_refs(lr, "POLY_LOOP", 1)?;
                    if points.len() < 3
                        || points.iter().collect::<BTreeSet<_>>().len() != points.len()
                    {
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
                        let edge_id = EdgeId(format!(
                            "step:data:edge#poly-{canonical_start}-{canonical_end}"
                        ));
                        poly_edges
                            .entry(edge_id.clone())
                            .or_insert((canonical_start, canonical_end));
                        poly_points.extend([start_point, end_point]);
                        let cid = CoedgeId(format!(
                            "step:data:coedge#poly-{loop_step}-{index}-face-{face_step}"
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
                    return None;
                }
                let bound_type = if has_type(br, "FACE_BOUND") {
                    "FACE_BOUND"
                } else {
                    "FACE_OUTER_BOUND"
                };
                let bound_forward = named_logical(br, bound_type, 2, 0)?;
                let mut uses = named_refs(lr, "EDGE_LOOP", 1)?;
                if !bound_forward {
                    uses.reverse();
                }
                if uses.is_empty() {
                    return None;
                }
                let mut coedge_ids = vec![];
                for use_step in uses {
                    let o = odefs.get(&use_step)?;
                    let edge = edefs.get(&o.edge)?;
                    let explicit_pcurve = o.pcurve.and_then(|pcurve_step| {
                        let pcurve = exchange.records.get(&pcurve_step)?;
                        (has_type(pcurve, "PCURVE")
                            && entity_parameter(pcurve, "PCURVE", 1)?.reference()? == surface_step
                            && decoded_pcurves
                                .contains(&PcurveId(format!("step:data:pcurve#{pcurve_step}"))))
                        .then_some(PcurveId(format!("step:data:pcurve#{pcurve_step}")))
                    });
                    let associated = explicit_pcurve.into_iter().collect::<Vec<_>>();
                    let associated = if associated.is_empty() {
                        associated_pcurves(edge.curve, surface_step, exchange, decoded_pcurves)
                    } else {
                        associated
                    };
                    let pcurves = if associated.len() <= 1 {
                        associated
                    } else {
                        let use_count = pcurve_use_counts
                            .entry((edge.curve, surface_step))
                            .or_default();
                        let selected = associated[*use_count % associated.len()].clone();
                        *use_count += 1;
                        vec![selected]
                    };
                    let cid = CoedgeId(format!("step:data:coedge#{use_step}-face-{face_step}"));
                    coedge_ids.push(cid.clone());
                    coedges.push(Coedge {
                        id: cid,
                        owner_loop: lid.clone(),
                        edge: EdgeId(format!("step:data:edge#{}", o.edge)),
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
                        .entry(EdgeId(format!("step:data:edge#{}", o.edge)))
                        .or_default()
                        .push(coedges.len() - 1);
                    used_e.insert(o.edge);
                    used_v.extend([edge.start, edge.end]);
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
            loop_ids.sort_by_key(|(outer, _)| !outer);
            let loop_ids = loop_ids.into_iter().map(|(_, id)| id).collect();
            let face_forward = face_same_sense == shell_forward;
            faces.push(Face {
                id: fid.clone(),
                shell: sid.clone(),
                surface: SurfaceId(format!("step:data:surface#{surface_step}")),
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
    for edge_id in used_e {
        let e = edefs.get(&edge_id)?;
        let (start, end) = if e.same {
            (e.start, e.end)
        } else {
            (e.end, e.start)
        };
        edges.push(Edge {
            id: EdgeId(format!("step:data:edge#{edge_id}")),
            curve: Some(CurveId(format!(
                "step:data:curve#{}",
                curve_carrier_step(e.curve, exchange)?
            ))),
            start: VertexId(format!("step:data:vertex#{start}")),
            end: VertexId(format!("step:data:vertex#{end}")),
            param_range: None,
            tolerance: None,
        });
    }
    for (id, (start, end)) in poly_edges {
        edges.push(Edge {
            id,
            curve: None,
            start: VertexId(format!("step:data:vertex#poly-point-{start}")),
            end: VertexId(format!("step:data:vertex#poly-point-{end}")),
            param_range: None,
            tolerance: None,
        });
    }
    for vertex_id in used_v {
        let v = vdefs.get(&vertex_id)?;
        vertices.push(Vertex {
            id: VertexId(format!("step:data:vertex#{vertex_id}")),
            point: PointId(format!("step:data:point#{}", v.point)),
            tolerance: None,
        });
        typed.insert(vertex_id);
    }
    for point_id in poly_points {
        vertices.push(Vertex {
            id: VertexId(format!("step:data:vertex#poly-point-{point_id}")),
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
        for (index, current_id) in loop_.coedges.iter().enumerate() {
            let next_id = &loop_.coedges[(index + 1) % loop_.coedges.len()];
            let current = coedge_by_id.get(current_id)?;
            let next = coedge_by_id.get(next_id)?;
            let current_edge = edge_by_id.get(&current.edge)?;
            let next_edge = edge_by_id.get(&next.edge)?;
            let current_end = match current.sense {
                Sense::Forward => &current_edge.end,
                Sense::Reversed => &current_edge.start,
            };
            let next_start = match next.sense {
                Sense::Forward => &next_edge.start,
                Sense::Reversed => &next_edge.end,
            };
            if current_end != next_start {
                return None;
            }
        }
    }
    staged_topology(
        typed, vertices, edges, coedges, loops, faces, shells, region, body,
    )
}

fn curve_carrier_step(curve_step: u64, exchange: &Exchange) -> Option<u64> {
    let curve = exchange.records.get(&curve_step)?;
    if curve
        .partials
        .iter()
        .any(|partial| matches!(partial.name.as_str(), "SURFACE_CURVE" | "SEAM_CURVE"))
    {
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
    if !curve
        .partials
        .iter()
        .any(|partial| matches!(partial.name.as_str(), "SURFACE_CURVE" | "SEAM_CURVE"))
    {
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
            (pcurve.simple_name() == Some("PCURVE")
                && pcurve.parameter(1)?.reference()? == surface_step
                && decoded_pcurves.contains(&pcurve_id))
            .then_some(pcurve_id)
        })
        .collect()
}

fn resolve_shell(
    reference: u64,
    exchange: &Exchange,
    typed: &mut BTreeSet<u64>,
) -> Option<(u64, bool)> {
    let record = exchange.records.get(&reference)?;
    if has_type(record, "OPEN_SHELL") || has_type(record, "CLOSED_SHELL") {
        return Some((reference, true));
    }
    if has_type(record, "ORIENTED_OPEN_SHELL") || has_type(record, "ORIENTED_CLOSED_SHELL") {
        typed.insert(reference);
        let shell_type = if has_type(record, "ORIENTED_OPEN_SHELL") {
            "ORIENTED_OPEN_SHELL"
        } else {
            "ORIENTED_CLOSED_SHELL"
        };
        return Some((
            named_reference(record, shell_type, 1, 0)?,
            named_logical(record, shell_type, 2, 0)?,
        ));
    }
    None
}

#[derive(Default)]
struct FaceInfo {
    bounds: Vec<u64>,
    surface: u64,
    same_sense: bool,
    typed: BTreeSet<u64>,
}

fn is_face_record(record: &RawRecord) -> bool {
    has_type(record, "ADVANCED_FACE")
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
    let result = (|| {
        if has_type(record, "ORIENTED_FACE") {
            let face_element = oriented_face_element(record)?;
            let mut base = face_attributes(exchange.records.get(&face_element)?, exchange, active)?;
            let orientation = oriented_face_orientation(record)?;
            if !orientation {
                base.bounds.reverse();
            }
            base.same_sense = base.same_sense == orientation;
            base.typed.insert(face_element);
            Some(base)
        } else if has_type(record, "SUBFACE") {
            let parent = subface_parent(record)?;
            let mut parent_info =
                face_attributes(exchange.records.get(&parent)?, exchange, active)?;
            let bounds = direct_face_bounds(record, exchange)?;
            parent_info.typed.insert(parent);
            Some(FaceInfo {
                bounds,
                surface: parent_info.surface,
                same_sense: parent_info.same_sense,
                typed: parent_info.typed,
            })
        } else if has_type(record, "ADVANCED_FACE") || has_type(record, "FACE_SURFACE") {
            let bounds = direct_face_bounds(record, exchange)?;
            let surface = direct_face_surface(record, &bounds)?;
            let same_sense = direct_face_same_sense(record)?;
            Some(FaceInfo {
                bounds,
                surface,
                same_sense,
                typed: BTreeSet::new(),
            })
        } else {
            None
        }
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

fn direct_face_surface(record: &RawRecord, bounds: &[u64]) -> Option<u64> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::reference)
        .find(|reference| !bounds.contains(reference))
}

fn direct_face_same_sense(record: &RawRecord) -> Option<bool> {
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
        .or_else(|| direct_face_same_sense(record))
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

fn connected_face_set_type(record: &RawRecord) -> Option<&'static str> {
    if has_type(record, "CONNECTED_FACE_SET") {
        Some("CONNECTED_FACE_SET")
    } else if has_type(record, "CONNECTED_FACE_SUB_SET") {
        Some("CONNECTED_FACE_SUB_SET")
    } else {
        None
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
