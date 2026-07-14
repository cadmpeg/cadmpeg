// SPDX-License-Identifier: Apache-2.0
//! STEP boundary-representation ownership and orientation decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Region, Sense, Shell, Vertex,
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
    for (&id, record) in &exchange.records {
        if !matches!(
            record.simple_name(),
            Some("SHELL_BASED_SURFACE_MODEL")
                | Some("MANIFOLD_SOLID_BREP")
                | Some("BREP_WITH_VOIDS")
        ) {
            continue;
        }
        if let Some(mut built) = build(id, record, exchange, &vertices, &edges, &oriented) {
            result.typed_records.append(&mut built.typed);
            ir.model.vertices.append(&mut built.vertices);
            ir.model.edges.append(&mut built.edges);
            ir.model.coedges.append(&mut built.coedges);
            ir.model.loops.append(&mut built.loops);
            ir.model.faces.append(&mut built.faces);
            ir.model.shells.append(&mut built.shells);
            ir.model.regions.push(built.region);
            ir.model.bodies.push(built.body);
        } else {
            result.warnings.push(format!(
                "{} #{id} does not resolve to a complete connected topology graph",
                record.simple_name().unwrap()
            ));
        }
    }
    for (&id, record) in &exchange.records {
        if record.simple_name() != Some("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION") {
            continue;
        }
        let Some(mut built) = build_geometric_set(id, record, exchange, ir) else {
            result.warnings.push(format!(
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #{id} has no decoded bounded surfaces"
            ));
            continue;
        };
        result.typed_records.append(&mut built.typed);
        ir.model.faces.append(&mut built.faces);
        ir.model.shells.append(&mut built.shells);
        ir.model.regions.push(built.region);
        ir.model.bodies.push(built.body);
    }
    if !ir.model.bodies.is_empty() {
        for (&id, record) in &exchange.records {
            if matches!(
                record.simple_name(),
                Some("MANIFOLD_SURFACE_SHAPE_REPRESENTATION")
                    | Some("ADVANCED_BREP_SHAPE_REPRESENTATION")
                    | Some("SHAPE_REPRESENTATION")
            ) {
                result.typed_records.insert(id);
            }
        }
    }
    result
}

fn build_geometric_set(
    id: u64,
    representation: &RawRecord,
    exchange: &Exchange,
    ir: &CadIr,
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
            if ir
                .model
                .surfaces
                .iter()
                .any(|candidate| candidate.id == surface)
            {
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
    Some(Built {
        typed,
        vertices: Vec::new(),
        edges: Vec::new(),
        coedges: Vec::new(),
        loops: Vec::new(),
        faces,
        shells: vec![Shell {
            id: shell.clone(),
            region: region.clone(),
            faces: face_ids,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        }],
        region: Region {
            id: region.clone(),
            body: body.clone(),
            shells: vec![shell],
        },
        body: Body {
            id: body,
            kind: BodyKind::Sheet,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    })
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
}
#[derive(Clone)]
struct OrientedDef {
    edge: u64,
    forward: bool,
}

fn vertex_defs(exchange: &Exchange) -> BTreeMap<u64, VertexDef> {
    exchange
        .records
        .iter()
        .filter_map(|(&id, r)| {
            if r.simple_name() != Some("VERTEX_POINT") {
                return None;
            }
            Some((
                id,
                VertexDef {
                    point: r.parameter(1)?.reference()?,
                },
            ))
        })
        .collect()
}
fn edge_defs(exchange: &Exchange) -> BTreeMap<u64, EdgeDef> {
    exchange
        .records
        .iter()
        .filter_map(|(&id, r)| {
            if r.simple_name() != Some("EDGE_CURVE") {
                return None;
            }
            Some((
                id,
                EdgeDef {
                    start: r.parameter(1)?.reference()?,
                    end: r.parameter(2)?.reference()?,
                    curve: r.parameter(3)?.reference()?,
                    same: r.parameter(4)?.logical()?,
                },
            ))
        })
        .collect()
}
fn oriented_defs(exchange: &Exchange) -> BTreeMap<u64, OrientedDef> {
    exchange
        .records
        .iter()
        .filter_map(|(&id, r)| {
            if r.simple_name() != Some("ORIENTED_EDGE") {
                return None;
            }
            Some((
                id,
                OrientedDef {
                    edge: r.parameter(3)?.reference()?,
                    forward: r.parameter(4)?.logical()?,
                },
            ))
        })
        .collect()
}

struct Built {
    typed: BTreeSet<u64>,
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    coedges: Vec<Coedge>,
    loops: Vec<Loop>,
    faces: Vec<Face>,
    shells: Vec<Shell>,
    region: Region,
    body: Body,
}

fn build(
    id: u64,
    root: &RawRecord,
    exchange: &Exchange,
    vdefs: &BTreeMap<u64, VertexDef>,
    edefs: &BTreeMap<u64, EdgeDef>,
    odefs: &BTreeMap<u64, OrientedDef>,
) -> Option<Built> {
    let solid = matches!(
        root.simple_name(),
        Some("MANIFOLD_SOLID_BREP") | Some("BREP_WITH_VOIDS")
    );
    let shell_steps = match root.simple_name()? {
        "SHELL_BASED_SURFACE_MODEL" => refs(root.parameter(1)?)?,
        "MANIFOLD_SOLID_BREP" => vec![root.parameter(1)?.reference()?],
        "BREP_WITH_VOIDS" => {
            let mut ids = vec![root.parameter(1)?.reference()?];
            ids.extend(refs(root.parameter(2)?)?);
            ids
        }
        _ => return None,
    };
    let bid = BodyId(format!("step:data:body#{id}"));
    let rid = RegionId(format!("step:data:region#{id}"));
    let mut built = Built {
        typed: BTreeSet::from([id]),
        vertices: vec![],
        edges: vec![],
        coedges: vec![],
        loops: vec![],
        faces: vec![],
        shells: vec![],
        region: Region {
            id: rid.clone(),
            body: bid.clone(),
            shells: vec![],
        },
        body: Body {
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
        },
    };
    let mut used_v = BTreeSet::new();
    let mut used_e = BTreeSet::new();
    let mut radial = BTreeMap::<u64, Vec<usize>>::new();
    for shell_reference in shell_steps {
        let (shell_step, shell_forward) =
            resolve_shell(shell_reference, exchange, &mut built.typed)?;
        let sr = exchange.records.get(&shell_step)?;
        if !matches!(sr.simple_name(), Some("OPEN_SHELL") | Some("CLOSED_SHELL")) {
            return None;
        }
        let sid = ShellId(format!("step:data:shell#{shell_step}"));
        let mut face_ids = vec![];
        for face_step in refs(sr.parameter(1)?)? {
            let fr = exchange.records.get(&face_step)?;
            if fr.simple_name() != Some("ADVANCED_FACE") {
                return None;
            }
            let surface_step = fr.parameter(2)?.reference()?;
            let fid = FaceId(format!("step:data:face#{face_step}"));
            let mut loop_ids = vec![];
            for bound_step in refs(fr.parameter(1)?)? {
                let br = exchange.records.get(&bound_step)?;
                if !matches!(
                    br.simple_name(),
                    Some("FACE_BOUND") | Some("FACE_OUTER_BOUND")
                ) {
                    return None;
                }
                let loop_step = br.parameter(1)?.reference()?;
                let lr = exchange.records.get(&loop_step)?;
                if lr.simple_name() != Some("EDGE_LOOP") {
                    return None;
                }
                let bound_forward = br.parameter(2)?.logical()?;
                let mut uses = refs(lr.parameter(1)?)?;
                if !bound_forward {
                    uses.reverse();
                }
                if uses.is_empty() {
                    return None;
                }
                let lid = LoopId(format!("step:data:loop#{loop_step}-face-{face_step}"));
                let mut coedge_ids = vec![];
                for use_step in uses {
                    let o = odefs.get(&use_step)?;
                    let edge = edefs.get(&o.edge)?;
                    let cid = CoedgeId(format!("step:data:coedge#{use_step}-face-{face_step}"));
                    coedge_ids.push(cid.clone());
                    built.coedges.push(Coedge {
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
                        pcurve: associated_pcurve(edge.curve, surface_step, exchange),
                    });
                    radial
                        .entry(o.edge)
                        .or_default()
                        .push(built.coedges.len() - 1);
                    used_e.insert(o.edge);
                    used_v.extend([edge.start, edge.end]);
                    built.typed.extend([use_step, o.edge]);
                }
                let n = coedge_ids.len();
                let start = built.coedges.len() - n;
                for i in 0..n {
                    built.coedges[start + i].next = coedge_ids[(i + 1) % n].clone();
                    built.coedges[start + i].previous = coedge_ids[(i + n - 1) % n].clone();
                }
                built.loops.push(Loop {
                    id: lid.clone(),
                    face: fid.clone(),
                    coedges: coedge_ids,
                });
                loop_ids.push(lid);
                built.typed.extend([bound_step, loop_step]);
            }
            let face_forward = fr.parameter(3)?.logical()? == shell_forward;
            built.faces.push(Face {
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
            built.typed.insert(face_step);
        }
        built.shells.push(Shell {
            id: sid.clone(),
            region: rid.clone(),
            faces: face_ids,
            wire_edges: vec![],
            free_vertices: vec![],
        });
        built.region.shells.push(sid);
        built.typed.insert(shell_step);
    }
    for edge_id in used_e {
        let e = edefs.get(&edge_id)?;
        let (start, end) = if e.same {
            (e.start, e.end)
        } else {
            (e.end, e.start)
        };
        built.edges.push(Edge {
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
    for vertex_id in used_v {
        let v = vdefs.get(&vertex_id)?;
        built.vertices.push(Vertex {
            id: VertexId(format!("step:data:vertex#{vertex_id}")),
            point: PointId(format!("step:data:point#{}", v.point)),
            tolerance: None,
        });
        built.typed.insert(vertex_id);
    }
    for indices in radial.values() {
        for (position, &index) in indices.iter().enumerate() {
            built.coedges[index].radial_next = built.coedges
                [indices[(position + 1) % indices.len()]]
            .id
            .clone();
        }
    }
    Some(built)
}

fn curve_carrier_step(curve_step: u64, exchange: &Exchange) -> Option<u64> {
    let curve = exchange.records.get(&curve_step)?;
    if matches!(curve.simple_name(), Some("SURFACE_CURVE" | "SEAM_CURVE")) {
        curve.parameter(1)?.reference()
    } else {
        Some(curve_step)
    }
}

fn associated_pcurve(curve_step: u64, surface_step: u64, exchange: &Exchange) -> Option<PcurveId> {
    let curve = exchange.records.get(&curve_step)?;
    if !matches!(curve.simple_name(), Some("SURFACE_CURVE" | "SEAM_CURVE")) {
        return None;
    }
    refs(curve.parameter(2)?)?
        .into_iter()
        .find_map(|pcurve_step| {
            let pcurve = exchange.records.get(&pcurve_step)?;
            (pcurve.simple_name() == Some("PCURVE")
                && pcurve.parameter(1)?.reference()? == surface_step)
                .then(|| PcurveId(format!("step:data:pcurve#{pcurve_step}")))
        })
}

fn resolve_shell(
    reference: u64,
    exchange: &Exchange,
    typed: &mut BTreeSet<u64>,
) -> Option<(u64, bool)> {
    let record = exchange.records.get(&reference)?;
    if matches!(record.simple_name(), Some("OPEN_SHELL" | "CLOSED_SHELL")) {
        return Some((reference, true));
    }
    if matches!(
        record.simple_name(),
        Some("ORIENTED_OPEN_SHELL" | "ORIENTED_CLOSED_SHELL")
    ) {
        typed.insert(reference);
        return Some((
            record.parameter(1)?.reference()?,
            record.parameter(2)?.logical()?,
        ));
    }
    None
}

fn refs(value: &Value) -> Option<Vec<u64>> {
    value.list()?.iter().map(ValueExt::reference).collect()
}
trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
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
