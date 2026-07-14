// SPDX-License-Identifier: Apache-2.0
//! Face-local trimmed-surface projection.

use super::geometry::entity_loss;
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::{CurveGeometry, Pcurve, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct CurveOnSurface {
    surface: u32,
    pcurve: u32,
    model_curve: u32,
}

fn pointer(record: &ParameterRecord, index: usize) -> Option<u32> {
    record.integer(index).and_then(|value| {
        let sequence = u32::try_from(value).ok()?;
        (sequence % 2 == 1).then_some(sequence)
    })
}

fn close(left: Point3, right: Point3) -> bool {
    let scale = left
        .x
        .abs()
        .max(left.y.abs())
        .max(left.z.abs())
        .max(right.x.abs())
        .max(right.y.abs())
        .max(right.z.abs())
        .max(1.0);
    (left.x - right.x).abs() <= scale * 1.0e-10
        && (left.y - right.y).abs() <= scale * 1.0e-10
        && (left.z - right.z).abs() <= scale * 1.0e-10
}

fn point_position(ir: &CadIr, id: &VertexId) -> Option<Point3> {
    let point_id = &ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *id)?
        .point;
    ir.model
        .points
        .iter()
        .find(|point| point.id == *point_id)
        .map(|point| point.position)
}

fn pcurve_geometry(
    ir: &CadIr,
    sequence: u32,
    support: &SurfaceGeometry,
    factor: f64,
) -> Option<(PcurveGeometry, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    let curve = ir.model.curves.iter().find(|curve| curve.id == curve_id)?;
    let range = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&curve_id))?
        .param_range?;
    let (u_factor, v_factor) = match support {
        SurfaceGeometry::Plane { .. } => (1.0, 1.0),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => (1.0 / factor, 1.0),
        SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. }
        | SurfaceGeometry::Nurbs(_) => (1.0 / factor, 1.0 / factor),
        SurfaceGeometry::Unknown { .. } => return None,
    };
    match &curve.geometry {
        CurveGeometry::Nurbs(nurbs) => Some((
            PcurveGeometry::Nurbs {
                degree: nurbs.degree,
                knots: nurbs.knots.clone(),
                control_points: nurbs
                    .control_points
                    .iter()
                    .map(|point| Point2::new(point.x * u_factor, point.y * v_factor))
                    .collect(),
                weights: nurbs.weights.clone(),
                periodic: nurbs.periodic,
            },
            range,
        )),
        _ => None,
    }
}

pub(super) struct TrimmingProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> TrimmingProjection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut associations = BTreeMap::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 142 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        if !matches!(record.integer(1), Some(0..=3)) || !matches!(record.integer(5), Some(0..=3)) {
            losses.push(entity_loss(
                entry,
                "curve-on-surface creation or preference flag is invalid",
            ));
            continue;
        }
        let Some(surface) = pointer(record, 2) else {
            losses.push(entity_loss(
                entry,
                "curve-on-surface surface pointer is invalid",
            ));
            continue;
        };
        let Some(pcurve) = pointer(record, 3) else {
            losses.push(entity_loss(
                entry,
                "curve-on-surface parameter curve pointer is invalid",
            ));
            continue;
        };
        let Some(model_curve) = pointer(record, 4) else {
            losses.push(entity_loss(
                entry,
                "curve-on-surface model curve pointer is invalid",
            ));
            continue;
        };
        let pcurve_entry = entries.get(&pcurve).copied();
        if pcurve_entry.is_none_or(|entry| entry.status.use_flag != 5) {
            losses.push(entity_loss(
                entry,
                "parameter curve does not have entity-use flag 05",
            ));
            continue;
        }
        associations.insert(
            entry.sequence,
            CurveOnSurface {
                surface,
                pcurve,
                model_curve,
            },
        );
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 144 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(surface_sequence) = pointer(record, 1) else {
            losses.push(entity_loss(
                entry,
                "trimmed-surface support pointer is invalid",
            ));
            continue;
        };
        if record.integer(2) != Some(1) {
            losses.push(entity_loss(
                entry,
                "implicit parameter-domain outer boundary is not projected",
            ));
            continue;
        }
        let Some(inner_count) = record
            .integer(3)
            .and_then(|value| usize::try_from(value).ok())
        else {
            losses.push(entity_loss(
                entry,
                "trimmed-surface inner-boundary count is invalid",
            ));
            continue;
        };
        let Some(outer) = pointer(record, 4) else {
            losses.push(entity_loss(
                entry,
                "trimmed-surface outer-boundary pointer is invalid",
            ));
            continue;
        };
        let mut boundary_sequences = vec![outer];
        let mut valid = true;
        for index in 0..inner_count {
            let Some(sequence) = pointer(record, 5 + index) else {
                losses.push(entity_loss(
                    entry,
                    "trimmed-surface inner-boundary pointer is invalid",
                ));
                valid = false;
                break;
            };
            boundary_sequences.push(sequence);
        }
        if !valid {
            continue;
        }
        let surface_id = SurfaceId(format!("iges:model:surface#D{surface_sequence}"));
        let Some(support) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == surface_id)
        else {
            losses.push(entity_loss(
                entry,
                "trimmed-surface support carrier is missing",
            ));
            continue;
        };
        let mut candidate = ir.clone();
        let stem = format!("D{}", entry.sequence);
        let body_id = BodyId(format!("iges:model:body#{stem}"));
        let region_id = RegionId(format!("iges:model:region#{stem}"));
        let shell_id = ShellId(format!("iges:model:shell#{stem}"));
        let face_id = FaceId(format!("iges:model:face#{stem}"));
        let mut loop_ids = Vec::new();
        let mut consumed = Vec::new();
        for (boundary_index, sequence) in boundary_sequences.iter().copied().enumerate() {
            let Some(association) = associations.get(&sequence).copied() else {
                losses.push(entity_loss(
                    entry,
                    "trimmed-surface boundary association is missing",
                ));
                valid = false;
                break;
            };
            if association.surface != surface_sequence {
                losses.push(entity_loss(
                    entry,
                    "boundary association names a different support surface",
                ));
                valid = false;
                break;
            }
            let model_curve_id = CurveId(format!("iges:model:curve#D{}", association.model_curve));
            let Some(source_edge) = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.curve.as_ref() == Some(&model_curve_id))
            else {
                losses.push(entity_loss(
                    entry,
                    "boundary model curve has no bounded edge",
                ));
                valid = false;
                break;
            };
            let (Some(start), Some(end)) = (
                point_position(ir, &source_edge.start),
                point_position(ir, &source_edge.end),
            ) else {
                losses.push(entity_loss(
                    entry,
                    "boundary model-curve endpoints are missing",
                ));
                valid = false;
                break;
            };
            if !close(start, end) {
                losses.push(entity_loss(
                    entry,
                    "single-carrier trimming boundary is not closed",
                ));
                valid = false;
                break;
            }
            let Some((geometry, parameter_range)) =
                pcurve_geometry(ir, association.pcurve, &support.geometry, factor)
            else {
                losses.push(entity_loss(
                    entry,
                    "boundary parameter curve has no line or NURBS carrier",
                ));
                valid = false;
                break;
            };
            let loop_id = LoopId(format!("iges:model:loop#{stem}:{boundary_index}"));
            let coedge_id = CoedgeId(format!("iges:model:coedge#{stem}:{boundary_index}:0"));
            let edge_id = EdgeId(format!("iges:model:edge#{stem}:{boundary_index}:0"));
            let vertex_id = VertexId(format!("iges:model:vertex#{stem}:{boundary_index}:0"));
            let point_id = PointId(format!("iges:model:point#{stem}:{boundary_index}:0"));
            let pcurve_id = PcurveId(format!("iges:model:pcurve#{stem}:{boundary_index}:0"));
            candidate.model.points.push(Point {
                id: point_id.clone(),
                position: start,
            });
            candidate.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            candidate.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(model_curve_id),
                start: vertex_id.clone(),
                end: vertex_id,
                param_range: source_edge.param_range,
                tolerance: None,
            });
            candidate.model.pcurves.push(Pcurve {
                id: pcurve_id.clone(),
                geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some(parameter_range),
                fit_tolerance: None,
            });
            candidate.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id.clone(),
                sense: Sense::Forward,
                pcurve: Some(pcurve_id),
            });
            candidate.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                coedges: vec![coedge_id],
            });
            loop_ids.push(loop_id);
            consumed.push(sequence);
        }
        if !valid {
            continue;
        }
        candidate.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id.clone(),
            surface: surface_id,
            sense: Sense::Forward,
            loops: loop_ids,
            name: None,
            color: None,
            tolerance: None,
        });
        candidate.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: vec![face_id],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        candidate.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        candidate.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Sheet,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        candidate.model.finalize();
        let validation = cadmpeg_ir::validate(&candidate, Vec::new());
        if !validation.is_ok() {
            losses.push(entity_loss(
                entry,
                "trimmed sheet candidate failed neutral validation",
            ));
            continue;
        }
        *ir = candidate;
        decoded.insert(entry.sequence);
        decoded.extend(consumed);
    }

    for sequence in associations
        .keys()
        .filter(|sequence| !decoded.contains(sequence))
    {
        if let Some(entry) = entries.get(sequence).copied() {
            losses.push(entity_loss(
                entry,
                "curve-on-surface association is not consumed by a projected trimmed surface",
            ));
        }
    }

    TrimmingProjection {
        handled,
        decoded,
        losses,
    }
}
