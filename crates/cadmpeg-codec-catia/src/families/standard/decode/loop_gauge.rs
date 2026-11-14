// SPDX-License-Identifier: Apache-2.0
//! Absolute gauge of boundary parity components from planar face loops.

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::CadIr;

use crate::families::standard::topology::admitted::StandardTopology;
use crate::families::standard::topology::CoedgeUse;

const CURVED_EDGE_SAMPLES: u32 = 32;
const PLANAR_LOOP_AREA_TOLERANCE: f64 = 1e-9;

/// Returns, for every face boundary, whether its whole-cycle gauge is
/// reversed. Each parity component of boundaries joined by shared edges takes
/// the reversal on which every evaluable planar face agrees: an outer loop
/// turns counterclockwise and an inner loop clockwise about the oriented face
/// normal. A component without votes, or with conflicting votes, keeps its
/// solved gauge.
pub(super) fn planar_loop_gauge(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    topology: &StandardTopology,
    edge_reversed: &[bool],
    curve_indices: &HashMap<CurveId, usize>,
    surface_indices: &HashMap<cadmpeg_ir::ids::SurfaceId, usize>,
) -> Result<Vec<Vec<bool>>, cadmpeg_core::CodecError> {
    let mut first_boundary = Vec::new();
    let mut boundary_count = 0usize;
    for face in topology.faces() {
        first_boundary.push(boundary_count);
        boundary_count += face.boundaries().len();
    }
    let mut parent = ctx.alloc_filled(
        boundary_count,
        0usize,
        "catia standard loop gauge components",
    )?;
    for (node, slot) in parent.iter_mut().enumerate() {
        *slot = node;
    }
    let find = |parent: &mut Vec<usize>, mut node: usize| {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    };
    let mut edge_owner = HashMap::<usize, usize>::new();
    for (face_index, face) in topology.faces().iter().enumerate() {
        for (loop_index, boundary) in face.boundaries().iter().enumerate() {
            let node = first_boundary[face_index] + loop_index;
            for coedge in boundary.coedges() {
                if let Some(&other) = edge_owner.get(&coedge.edge_row) {
                    let left = find(&mut parent, node);
                    let right = find(&mut parent, other);
                    parent[left] = right;
                } else {
                    edge_owner.insert(coedge.edge_row, node);
                }
            }
        }
    }

    let mut votes = HashMap::<usize, Option<bool>>::new();
    for (face_index, face) in topology.faces().iter().enumerate() {
        let Some(normal) = oriented_plane_normal(ir, surface_indices, face_index) else {
            continue;
        };
        let mut areas = Vec::new();
        for boundary in face.boundaries() {
            let Some(area) = signed_loop_area(
                ctx,
                ir,
                boundary.coedges(),
                edge_reversed,
                curve_indices,
                normal,
            )?
            else {
                break;
            };
            areas.push(area);
        }
        if areas.len() != face.boundaries().len()
            || areas
                .iter()
                .any(|area| area.abs() <= PLANAR_LOOP_AREA_TOLERANCE)
        {
            continue;
        }
        let Some(outer) = unique_largest(&areas) else {
            continue;
        };
        for (loop_index, area) in areas.iter().enumerate() {
            let counterclockwise = *area > 0.0;
            let reverse = counterclockwise != (loop_index == outer);
            let component = find(&mut parent, first_boundary[face_index] + loop_index);
            votes
                .entry(component)
                .and_modify(|vote| {
                    if *vote != Some(reverse) {
                        *vote = None;
                    }
                })
                .or_insert(Some(reverse));
        }
    }

    let mut result = Vec::new();
    for (face_index, face) in topology.faces().iter().enumerate() {
        let mut flips = Vec::new();
        for loop_index in 0..face.boundaries().len() {
            let component = find(&mut parent, first_boundary[face_index] + loop_index);
            flips.push(votes.get(&component).copied().flatten().unwrap_or(false));
        }
        result.push(flips);
    }
    Ok(result)
}

/// The boundary's edge uses in traversal order after a whole-cycle reversal.
pub(super) fn gauged_coedges(coedges: &[CoedgeUse], reverse: bool) -> Vec<CoedgeUse> {
    if !reverse {
        return coedges.to_vec();
    }
    coedges
        .iter()
        .rev()
        .map(|coedge| CoedgeUse {
            edge_row: coedge.edge_row,
            reversed: !coedge.reversed,
            start_vertex: coedge.end_vertex,
            end_vertex: coedge.start_vertex,
        })
        .collect()
}

fn unique_largest(areas: &[f64]) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut tied = false;
    for (index, area) in areas.iter().enumerate() {
        match best {
            Some(current) if area.abs() > areas[current].abs() => {
                best = Some(index);
                tied = false;
            }
            Some(current) if area.abs() == areas[current].abs() => tied = true,
            Some(_) => {}
            None => best = Some(index),
        }
    }
    best.filter(|_| !tied)
}

fn oriented_plane_normal(
    ir: &CadIr,
    surface_indices: &HashMap<cadmpeg_ir::ids::SurfaceId, usize>,
    face_index: usize,
) -> Option<Vector3> {
    let face = ir.model.faces.get(face_index)?;
    let surface = ir
        .model
        .surfaces
        .get(*surface_indices.get(&face.surface)?)?;
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        return None;
    };
    let normal = *plane.frame().axis().as_raw();
    Some(match face.sense {
        Sense::Forward => normal,
        Sense::Reversed => normal.scale(-1.0),
    })
}

fn signed_loop_area(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    coedges: &[CoedgeUse],
    edge_reversed: &[bool],
    curve_indices: &HashMap<CurveId, usize>,
    normal: Vector3,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut points: Vec<Point3> = Vec::new();
    for coedge in coedges {
        let Some(edge) = ir.model.edges.get(coedge.edge_row) else {
            return Ok(None);
        };
        let (Some(curve), Some(range)) = (
            edge.curve()
                .and_then(|id| curve_indices.get(id))
                .and_then(|index| ir.model.curves.get(*index)),
            edge.param_range(),
        ) else {
            return Ok(None);
        };
        let [lower, upper] = range.get();
        let forward =
            !(coedge.reversed ^ edge_reversed.get(coedge.edge_row).copied().unwrap_or(false));
        let samples = match curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => 1,
            _ => CURVED_EDGE_SAMPLES,
        };
        for step in 0..samples {
            let fraction = f64::from(step) / f64::from(samples);
            let parameter = if forward {
                lower + (upper - lower) * fraction
            } else {
                upper - (upper - lower) * fraction
            };
            let Some(point) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point(ctx, &curve.geometry, parameter),
                )?)?
            else {
                return Ok(None);
            };
            points.push(point.get());
        }
    }
    let Some(&origin) = points.first() else {
        return Ok(None);
    };
    let mut twice_area = 0.0;
    for (current, next) in points.iter().zip(points.iter().cycle().skip(1)) {
        twice_area += current
            .vector_from(origin)
            .cross(next.vector_from(origin))
            .dot(normal);
    }
    Ok(twice_area.is_finite().then_some(0.5 * twice_area))
}
