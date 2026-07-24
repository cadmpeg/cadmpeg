// SPDX-License-Identifier: Apache-2.0
//! Face-local trimmed-surface projection.

use super::evaluation;
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
use cadmpeg_ir::topology::builder::{BodySpec, CoedgeSpec, FaceSpec, TopologyBuilder};
use cadmpeg_ir::topology::{BodyKind, Edge, LoopBoundaryRole, PcurveUse, Point, Sense, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct BoundarySegment {
    model_curve: u32,
    pcurves: Vec<u32>,
    sense: Sense,
    require_carrier_agreement: bool,
}

#[derive(Clone)]
struct BoundaryDefinition {
    surface: u32,
    segments: Vec<BoundarySegment>,
}

struct BoundaryItem {
    segment: BoundarySegment,
    model_curve: CurveId,
    source_edge: Edge,
    start: Point3,
    end: Point3,
    pcurves: Vec<(PcurveGeometry, [f64; 2])>,
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

fn face_vertex(
    candidate: &mut CadIr,
    vertices: &mut Vec<(Point3, VertexId)>,
    stem: &str,
    boundary: usize,
    position: Point3,
) -> VertexId {
    if let Some((_, id)) = vertices
        .iter()
        .find(|(existing, _)| close(*existing, position))
    {
        return id.clone();
    }
    let index = vertices.len();
    let point_id = PointId(format!("iges:model:point#{stem}:{boundary}:{index}"));
    let vertex_id = VertexId(format!("iges:model:vertex#{stem}:{boundary}:{index}"));
    candidate.model.points.push(Point {
        source_object: None,
        id: point_id.clone(),
        position,
    });
    candidate.model.vertices.push(Vertex {
        id: vertex_id.clone(),
        point: point_id,
        tolerance: None,
    });
    vertices.push((position, vertex_id.clone()));
    vertex_id
}

pub(super) fn pcurve_geometry(
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
        SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => return None,
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
    let mut boundaries = BTreeMap::new();

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
        let pcurve = match record.integer(3) {
            Some(0) => None,
            Some(value) => u32::try_from(value)
                .ok()
                .filter(|sequence| sequence % 2 == 1),
            None => None,
        };
        if record
            .integer(3)
            .is_none_or(|value| value != 0 && pcurve.is_none())
        {
            losses.push(entity_loss(
                entry,
                "curve-on-surface parameter curve pointer is invalid",
            ));
            continue;
        }
        let Some(model_curve) = pointer(record, 4) else {
            losses.push(entity_loss(
                entry,
                "curve-on-surface model curve pointer is invalid",
            ));
            continue;
        };
        if pcurve.is_some_and(|pcurve| {
            entries
                .get(&pcurve)
                .is_none_or(|entry| entry.status.use_flag != 5)
        }) {
            losses.push(entity_loss(
                entry,
                "parameter curve does not have entity-use flag 05",
            ));
            continue;
        }
        boundaries.insert(
            entry.sequence,
            BoundaryDefinition {
                surface,
                segments: vec![BoundarySegment {
                    pcurves: pcurve.into_iter().collect(),
                    model_curve,
                    sense: Sense::Forward,
                    require_carrier_agreement: pcurve.is_some(),
                }],
            },
        );
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 141 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(boundary_type) = record.integer(1).filter(|value| matches!(value, 0 | 1)) else {
            losses.push(entity_loss(
                entry,
                "boundary representation type is not 0 or 1",
            ));
            continue;
        };
        if !matches!(record.integer(2), Some(0..=3)) {
            losses.push(entity_loss(entry, "boundary preference flag is invalid"));
            continue;
        }
        let Some(surface) = pointer(record, 3) else {
            losses.push(entity_loss(entry, "boundary support pointer is invalid"));
            continue;
        };
        let Some(segment_count) = record.count(4).filter(|count| *count > 0) else {
            losses.push(entity_loss(entry, "boundary segment count is not positive"));
            continue;
        };
        let mut index = 5;
        let mut segments = Vec::with_capacity(segment_count);
        let mut valid = true;
        for _ in 0..segment_count {
            let Some(model_curve) = pointer(record, index) else {
                losses.push(entity_loss(
                    entry,
                    "boundary model-curve pointer is invalid",
                ));
                valid = false;
                break;
            };
            let sense = match record.integer(index + 1) {
                Some(1) => Sense::Forward,
                Some(2) => Sense::Reversed,
                _ => {
                    losses.push(entity_loss(entry, "boundary segment sense is not 1 or 2"));
                    valid = false;
                    break;
                }
            };
            let Some(pcurve_count) = record.count(index + 2) else {
                losses.push(entity_loss(entry, "boundary pcurve count is invalid"));
                valid = false;
                break;
            };
            if (boundary_type == 0 && pcurve_count != 0)
                || (boundary_type == 1 && pcurve_count == 0)
            {
                losses.push(entity_loss(
                    entry,
                    "boundary pcurve collection cardinality disagrees with its representation type",
                ));
                valid = false;
                break;
            }
            let mut pcurves = Vec::with_capacity(pcurve_count);
            for pcurve_index in 0..pcurve_count {
                let Some(pcurve) = pointer(record, index + 3 + pcurve_index) else {
                    pcurves.clear();
                    break;
                };
                if entries
                    .get(&pcurve)
                    .is_none_or(|entry| entry.status.use_flag != 5)
                {
                    losses.push(entity_loss(
                        entry,
                        "boundary pcurve does not have entity-use flag 05",
                    ));
                    pcurves.clear();
                    break;
                }
                pcurves.push(pcurve);
            }
            if pcurves.len() != pcurve_count {
                losses.push(entity_loss(entry, "boundary pcurve pointer is invalid"));
                valid = false;
                break;
            }
            segments.push(BoundarySegment {
                model_curve,
                pcurves,
                sense,
                require_carrier_agreement: false,
            });
            index += 3 + pcurve_count;
        }
        if valid {
            boundaries.insert(entry.sequence, BoundaryDefinition { surface, segments });
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 143 | 144) && entry.form == 0)
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
        let trimmed_surface = entry.entity_type == 144;
        let (surface_sequence, boundary_sequences, has_explicit_outer, mut valid) =
            if trimmed_surface {
                let Some(surface) = pointer(record, 1) else {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface support pointer is invalid",
                    ));
                    continue;
                };
                let Some(has_explicit_outer) = record.integer(2).and_then(|value| match value {
                    0 => Some(false),
                    1 => Some(true),
                    _ => None,
                }) else {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface outer-boundary flag is not 0 or 1",
                    ));
                    continue;
                };
                let Some(inner_count) = record.count(3) else {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface inner-boundary count is invalid",
                    ));
                    continue;
                };
                let mut sequences =
                    Vec::with_capacity(inner_count + usize::from(has_explicit_outer));
                if has_explicit_outer {
                    let Some(outer) = pointer(record, 4) else {
                        losses.push(entity_loss(
                            entry,
                            "trimmed-surface outer-boundary pointer is invalid",
                        ));
                        continue;
                    };
                    sequences.push(outer);
                } else if record.integer(4) != Some(0) {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface parameter-domain outer boundary has a nonzero pointer",
                    ));
                    continue;
                }
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
                    sequences.push(sequence);
                }
                (surface, sequences, has_explicit_outer, valid)
            } else {
                let Some(representation) = record.integer(1).filter(|value| matches!(value, 0 | 1))
                else {
                    losses.push(entity_loss(
                        entry,
                        "bounded-surface representation type is not 0 or 1",
                    ));
                    continue;
                };
                let Some(surface) = pointer(record, 2) else {
                    losses.push(entity_loss(
                        entry,
                        "bounded-surface support pointer is invalid",
                    ));
                    continue;
                };
                let Some(count) = record.count(3).filter(|count| *count > 0) else {
                    losses.push(entity_loss(
                        entry,
                        "bounded-surface boundary count is not positive",
                    ));
                    continue;
                };
                let mut sequences = Vec::with_capacity(count);
                let mut valid = true;
                for index in 0..count {
                    let Some(sequence) = pointer(record, 4 + index) else {
                        losses.push(entity_loss(
                            entry,
                            "bounded-surface boundary pointer is invalid",
                        ));
                        valid = false;
                        break;
                    };
                    if boundaries.get(&sequence).is_some_and(|boundary| {
                        (representation == 0
                            && boundary
                                .segments
                                .iter()
                                .all(|segment| segment.pcurves.is_empty()))
                            || (representation == 1
                                && boundary
                                    .segments
                                    .iter()
                                    .all(|segment| !segment.pcurves.is_empty()))
                    }) {
                        sequences.push(sequence);
                    } else {
                        losses.push(entity_loss(
                            entry,
                            "bounded-surface representation disagrees with its boundary",
                        ));
                        valid = false;
                        break;
                    }
                }
                (surface, sequences, false, valid)
            };
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
        let mut consumed = Vec::new();
        let mut builder = TopologyBuilder::new();
        builder
            .body(
                body_id.clone(),
                BodySpec {
                    kind: BodyKind::Sheet,
                    ..BodySpec::default()
                },
            )
            .expect("trimmed-sheet body id is unique");
        builder
            .region(region_id.clone(), &body_id)
            .expect("trimmed-sheet region id is unique under a registered body");
        builder
            .shell(shell_id.clone(), &region_id)
            .expect("trimmed-sheet shell id is unique under a registered region");
        builder
            .face(
                face_id.clone(),
                &shell_id,
                FaceSpec {
                    surface: surface_id,
                    sense: Sense::Forward,
                    name: None,
                    color: None,
                    tolerance: None,
                },
            )
            .expect("trimmed-sheet face id is unique under a registered shell");
        for (boundary_index, sequence) in boundary_sequences.iter().copied().enumerate() {
            let Some(boundary) = boundaries.get(&sequence).cloned() else {
                losses.push(entity_loss(
                    entry,
                    "trimmed-surface boundary definition is missing",
                ));
                valid = false;
                break;
            };
            if boundary.surface != surface_sequence {
                losses.push(entity_loss(
                    entry,
                    "boundary definition names a different support surface",
                ));
                valid = false;
                break;
            }
            let mut items = Vec::with_capacity(boundary.segments.len());
            for segment in &boundary.segments {
                let model_curve_id = CurveId(format!("iges:model:curve#D{}", segment.model_curve));
                let Some(source_edge) = ir
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.curve.as_ref() == Some(&model_curve_id))
                    .cloned()
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
                let pcurves = segment
                    .pcurves
                    .iter()
                    .map(|sequence| pcurve_geometry(ir, *sequence, &support.geometry, factor))
                    .collect::<Option<Vec<_>>>();
                let Some(pcurves) = pcurves else {
                    losses.push(entity_loss(
                        entry,
                        "boundary parameter curve has no NURBS carrier",
                    ));
                    valid = false;
                    break;
                };
                if segment.require_carrier_agreement {
                    let agrees = pcurves.len() == 1
                        && global.minimum_resolution_mm().is_some_and(|tolerance| {
                            let (geometry, range) = &pcurves[0];
                            let mapped_start = evaluation::pcurve(geometry, range[0])
                                .and_then(|uv| evaluation::surface(&support.geometry, uv));
                            let mapped_end = evaluation::pcurve(geometry, range[1])
                                .and_then(|uv| evaluation::surface(&support.geometry, uv));
                            mapped_start.is_some_and(|point| {
                                evaluation::distance(point, start) <= tolerance
                            }) && mapped_end
                                .is_some_and(|point| evaluation::distance(point, end) <= tolerance)
                        });
                    if !agrees {
                        losses.push(entity_loss(
                            entry,
                            "curve-on-surface carriers disagree beyond the minimum resolution",
                        ));
                        valid = false;
                        break;
                    }
                }
                items.push(BoundaryItem {
                    segment: segment.clone(),
                    model_curve: model_curve_id,
                    source_edge,
                    start,
                    end,
                    pcurves,
                });
            }
            if !valid {
                break;
            }
            let traversal = |item: &BoundaryItem| {
                if item.segment.sense == Sense::Forward {
                    (item.start, item.end)
                } else {
                    (item.end, item.start)
                }
            };
            if items.iter().enumerate().any(|(index, item)| {
                let (_, end) = traversal(item);
                let (next_start, _) = traversal(&items[(index + 1) % items.len()]);
                !close(end, next_start)
            }) {
                losses.push(entity_loss(
                    entry,
                    "ordered boundary segments do not form a closed ring",
                ));
                valid = false;
                break;
            }
            let loop_id = LoopId(format!("iges:model:loop#{stem}:{boundary_index}"));
            let coedge_ids = (0..items.len())
                .map(|index| CoedgeId(format!("iges:model:coedge#{stem}:{boundary_index}:{index}")))
                .collect::<Vec<_>>();
            let mut local_vertices = Vec::new();
            let mut loop_coedges: Vec<CoedgeSpec> = Vec::with_capacity(coedge_ids.len());
            for (segment_index, item) in items.into_iter().enumerate() {
                let edge_id = EdgeId(format!(
                    "iges:model:edge#{stem}:{boundary_index}:{segment_index}"
                ));
                let start_vertex = face_vertex(
                    &mut candidate,
                    &mut local_vertices,
                    &stem,
                    boundary_index,
                    item.start,
                );
                let end_vertex = face_vertex(
                    &mut candidate,
                    &mut local_vertices,
                    &stem,
                    boundary_index,
                    item.end,
                );
                candidate.model.edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(item.model_curve),
                    start: start_vertex,
                    end: end_vertex,
                    param_range: item.source_edge.param_range,
                    tolerance: None,
                });
                let pcurve_uses = item
                    .pcurves
                    .into_iter()
                    .enumerate()
                    .map(|(pcurve_index, (geometry, parameter_range))| {
                        let id = PcurveId(format!(
                            "iges:model:pcurve#{stem}:{boundary_index}:{segment_index}:{pcurve_index}"
                        ));
                        candidate.model.pcurves.push(Pcurve {
                            id: id.clone(),
                            geometry,
                            wrapper_reversed: None,
                            native_tail_flags: None,
                            parameter_range: Some(parameter_range),
                            fit_tolerance: None,
                        });
                        PcurveUse {
                            pcurve: id,
                            isoparametric: None,
                                    parameter_range: None,
                        }
                    })
                    .collect();
                let coedge_id = coedge_ids[segment_index].clone();
                loop_coedges.push(CoedgeSpec {
                    id: coedge_id,
                    edge: edge_id,
                    sense: item.segment.sense,
                    pcurves: pcurve_uses,
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
            let boundary_role = if trimmed_surface {
                if has_explicit_outer && boundary_index == 0 {
                    LoopBoundaryRole::Outer
                } else {
                    LoopBoundaryRole::Inner
                }
            } else {
                LoopBoundaryRole::Unspecified
            };
            builder
                .ring(loop_id, &face_id, boundary_role, loop_coedges, Vec::new())
                .expect("boundary ring registers under a registered face");
            consumed.push(sequence);
        }
        if !valid {
            continue;
        }
        builder
            .finish(&mut candidate.model)
            .expect("trimmed-sheet topology appends without id or owner conflicts");
        candidate.model.finalize();
        let validation = cadmpeg_ir::validate::validate(&candidate, Vec::new());
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

    for sequence in boundaries
        .keys()
        .filter(|sequence| !decoded.contains(sequence))
    {
        if let Some(entry) = entries.get(sequence).copied() {
            losses.push(entity_loss(
                entry,
                "boundary definition is not consumed by a projected trimmed surface",
            ));
        }
    }

    TrimmingProjection {
        handled,
        decoded,
        losses,
    }
}
