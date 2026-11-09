// SPDX-License-Identifier: Apache-2.0
//! Transfer bounded patches as sheet components with shared native edges.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, SolvedCurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::assemble::annotate;
use crate::families::FamilyEntityAdmission;
use crate::loss::CatiaLossCode;
use crate::nurbs::LaneRefusals;

use super::{curve_point, edge_alignment, interval_parameter, Patch, SheetSurfaceFit, Use, GAP_MM};

const EDGE_SAMPLES: u32 = 40;

fn matches_edge(a: &Use, b: &Use) -> Option<bool> {
    let same = edge_alignment::edge_relation(a, b)?;
    let sample = |side: &Use, f: f64| {
        let t = interval_parameter(side.curve.full_knot_endpoints().endpoints(), f);
        eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, &side.curve, t)
            .ok()
            .map(FinitePoint3::get)
    };
    for step in 0..=EDGE_SAMPLES {
        let f = f64::from(step) / f64::from(EDGE_SAMPLES);
        if sample(a, f)?.distance(sample(b, if same { f } else { 1.0 - f })?) > GAP_MM {
            return None;
        }
    }
    Some(same)
}

pub(super) fn append(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    patches: Vec<Patch>,
    refusal: &mut LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<Option<LossNote>, CodecError> {
    let mut by_body = BTreeMap::<_, Vec<_>>::new();
    for patch in patches {
        by_body.entry(patch.body).or_default().push(patch);
    }
    let mut count = 0;
    let mut inferred_senses = 0;
    let (mut rolling_ball, mut offset, mut ruled, mut extrusion) = (0, 0, 0, 0);
    let (mut adjusted, mut max_shift_mm) = (0, 0.0_f64);
    for ((source, body), mut patches) in by_body {
        patches.sort_by_key(|patch| patch.priority);
        let selected = compatible_patches(&mut patches);
        let mut by_edge = BTreeMap::<_, Vec<_>>::new();
        for &index in &selected {
            for side in &patches[index].sides {
                by_edge.entry(side.edge).or_default().push(index);
            }
        }
        let mut pending = selected;
        while let Some(start) = pending.pop_first() {
            let mut component = vec![start];
            let mut next = 0;
            while let Some(&index) = component.get(next) {
                next += 1;
                for side in &patches[index].sides {
                    for &neighbor in &by_edge[&side.edge] {
                        if pending.remove(&neighbor) {
                            component.push(neighbor);
                        }
                    }
                }
            }
            component.sort_unstable();
            let patches = component.iter().map(|&i| &patches[i]).collect::<Vec<_>>();
            if append_component(ir, annotations, source, body, &patches, admission)? {
                count += patches.len();
                inferred_senses += patches.iter().filter(|patch| patch.sense_inferred).count();
                for fit in patches.iter().filter_map(|patch| patch.surface_fit) {
                    match fit {
                        SheetSurfaceFit::RollingBall { max_rail_shift_mm } => {
                            rolling_ball += 1;
                            adjusted += usize::from(max_rail_shift_mm > 0.0);
                            max_shift_mm = max_shift_mm.max(max_rail_shift_mm);
                        }
                        SheetSurfaceFit::Offset => offset += 1,
                        SheetSurfaceFit::Ruled => ruled += 1,
                        SheetSurfaceFit::Extrusion => extrusion += 1,
                    }
                }
            }
        }
    }
    if inferred_senses != 0 {
        refusal.push_owned_patch_sense_inferred(inferred_senses);
    }
    if rolling_ball + offset + ruled + extrusion != 0 {
        refusal.push_sheet_surface_fitted(rolling_ball, offset, ruled, extrusion);
    }
    if adjusted != 0 {
        refusal.push_rolling_ball_rails_adjusted(adjusted, max_shift_mm);
    }
    Ok((count != 0).then(|| {
        CatiaLossCode::TopologyOwnedPatchSubset.note(format!(
            "Transferred {count} bounded faces as sheet subsets of their native body rosters. The complete body and its active selection remain native."
        ))
    }))
}

fn compatible_patches(patches: &mut [Patch]) -> BTreeSet<usize> {
    let mut admitted = BTreeSet::<usize>::new();
    let mut vertices = BTreeMap::<usize, Point3>::new();
    let mut edges = BTreeMap::<usize, (usize, usize, usize)>::new();
    for index in 0..patches.len() {
        let (prior, rest) = patches.split_at_mut(index);
        let patch = &mut rest[0];
        let rolling_boundary_retry =
            patch.priority == super::sheet_group::ROLLING_BOUNDARY_RETRY_PRIORITY;
        if rolling_boundary_retry && admitted.iter().any(|&i| prior[i].face == patch.face) {
            continue;
        }
        let mut local = BTreeMap::<usize, Point3>::new();
        let mut replacements = Vec::new();
        if !patch.sides.iter_mut().all(|side| {
            side.vertices.iter().all(|&(vertex, point)| {
                let existing = *local
                    .entry(vertex)
                    .or_insert_with(|| *vertices.get(&vertex).unwrap_or(&point));
                point.distance(existing) <= GAP_MM
            }) && match edges.get(&side.edge) {
                Some(&(1, face, side_index)) => {
                    let first = &prior[face].sides[side_index];
                    if matches_edge(first, side).is_some()
                        || edge_alignment::align(first, side, &patch.surface).is_some()
                    {
                        true
                    } else if rolling_boundary_retry {
                        false
                    } else {
                        let mut aligned = first.clone();
                        let closed =
                            edge_alignment::align(side, &mut aligned, &prior[face].surface)
                                .is_some();
                        if closed {
                            replacements.push((face, side_index, aligned));
                        }
                        closed
                    }
                }
                None => true,
                Some(_) => false,
            }
        }) {
            continue;
        }
        for (face, side, aligned) in replacements {
            prior[face].sides[side] = aligned;
        }
        admitted.insert(index);
        vertices.extend(local);
        for (side_index, side) in patch.sides.iter().enumerate() {
            edges
                .entry(side.edge)
                .and_modify(|(count, _, _)| *count += 1)
                .or_insert((1, index, side_index));
        }
    }
    admitted
}

fn append_component(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    source: usize,
    body: usize,
    patches: &[&Patch],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<bool, CodecError> {
    let mut vertices = BTreeMap::new();
    let mut vertex_gaps = BTreeMap::<usize, f64>::new();
    let mut edges = BTreeMap::<usize, Vec<(usize, &Use)>>::new();
    let mut orders = BTreeMap::new();
    for patch in patches {
        for side in &patch.sides {
            let parameters = side.curve.full_knot_endpoints().endpoints();
            for (&(vertex, point), parameter) in side.vertices.iter().zip(parameters) {
                let representative = vertices.entry(vertex).or_insert(point);
                if point.distance(*representative) > GAP_MM {
                    return Ok(false);
                }
                let Some(on_curve) = curve_point(&side.curve, parameter) else {
                    return Ok(false);
                };
                let gap = representative.distance(on_curve);
                vertex_gaps
                    .entry(vertex)
                    .and_modify(|old| *old = old.max(gap))
                    .or_insert(gap);
            }
            let uses = edges.entry(side.edge).or_default();
            let same = match uses.as_slice() {
                [] => true,
                [(_, first)] => {
                    let Some(same) = matches_edge(first, side) else {
                        return Ok(false);
                    };
                    same
                }
                _ => return Ok(false),
            };
            orders.insert((patch.face, side.edge), same);
            uses.push((patch.face, side));
        }
    }
    let first = patches[0].face;
    macro_rules! id {
        ($kind:ty, $label:literal, $offset:expr) => {
            <$kind>::mint(format!(
                concat!("catia:owned-patch:", $label, "#{}:{}:{}"),
                source, first, $offset
            ))
            .map_err(CodecError::malformed)?
        };
    }
    let mut admit = |id: &str, offset: usize, role: &str| {
        admission.charge()?;
        annotate(
            annotations,
            id,
            "consolidated_owned_face",
            u64_from_index(offset),
            role,
            Exactness::Derived,
        )
    };
    let body_id = id!(BodyId, "body", body);
    let region = id!(RegionId, "region", body);
    let shell = id!(ShellId, "shell", body);
    for id in [body_id.as_str(), region.as_str(), shell.as_str()] {
        admit(id, body, "bounded_face_subset")?;
    }
    let tolerance = PositiveReal::new(GAP_MM);
    for (vertex, point) in vertices {
        let point_id = id!(PointId, "point", vertex);
        let vertex_id = id!(VertexId, "vertex", vertex);
        let point = FinitePoint3::new(point)
            .ok_or_else(|| CodecError::malformed("CATIA patch vertex is non-finite"))?;
        admit(point_id.as_str(), vertex, "lifted_boundary_vertex")?;
        admit(vertex_id.as_str(), vertex, "boundary_vertex")?;
        ir.model
            .points
            .push(Point::new(point_id.clone(), point, None));
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: PositiveReal::new(vertex_gaps[&vertex].max(GAP_MM)),
        });
    }
    for (&edge, uses) in &edges {
        let side = uses[0].1;
        let curve = id!(CurveId, "curve", edge);
        let edge_id = id!(EdgeId, "edge", edge);
        admit(curve.as_str(), edge, "fitted_boundary_curve")?;
        admit(edge_id.as_str(), edge, "boundary_edge")?;
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(side.curve.clone())),
            parameter_range: None,
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: edge_id,
            carrier: EdgeCarrier::new(
                Some(curve),
                Some(side.curve.full_knot_endpoints().endpoints()),
            )
            .map_err(CodecError::malformed)?,
            start: id!(VertexId, "vertex", side.vertices[0].0),
            end: id!(VertexId, "vertex", side.vertices[1].0),
            tolerance,
        });
    }
    let mut faces = Vec::new();
    for patch in patches {
        let face = id!(FaceId, "face", patch.face);
        let ring = id!(LoopId, "loop", patch.face);
        let surface = id!(SurfaceId, "surface", patch.face);
        admit(face.as_str(), patch.face, "roster_face")?;
        admit(ring.as_str(), patch.face, "outer_boundary")?;
        admit(surface.as_str(), patch.face, "face_surface")?;
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(patch.surface.clone()),
            source_object: None,
        });
        let outer = ring.clone();
        let mut inner_rings = Vec::new();
        let mut start = 0;
        let ends = patch
            .inner_starts
            .iter()
            .copied()
            .chain([patch.sides.len()]);
        for (index, end) in ends.enumerate() {
            let sides = &patch.sides[start..end];
            start = end;
            let ring = if index != 0 {
                let id = id!(LoopId, "inner-loop", format_args!("{}:{index}", patch.face));
                admit(id.as_str(), patch.face, "inner_boundary")?;
                inner_rings.push(id.clone());
                id
            } else {
                outer.clone()
            };
            let mut coedges = Vec::new();
            for side in sides {
                let id = id!(
                    CoedgeId,
                    "coedge",
                    format_args!("{}:{}", patch.face, side.edge)
                );
                let neighbor = edges[&side.edge]
                    .iter()
                    .find(|(other, _)| *other != patch.face);
                let radial_next = if let Some((other, _)) = neighbor {
                    id!(CoedgeId, "coedge", format_args!("{}:{}", other, side.edge))
                } else {
                    id.clone()
                };
                admit(id.as_str(), side.edge, "boundary_occurrence")?;
                let pcurve = id!(
                    PcurveId,
                    "pcurve",
                    format_args!("{}:{}", patch.face, side.edge)
                );
                admit(pcurve.as_str(), side.edge, "boundary_surface_curve")?;
                ir.model.pcurves.push(Pcurve {
                    id: pcurve.clone(),
                    geometry: side.pcurve.clone(),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
                });
                let [lo, hi] = side.curve.full_knot_endpoints().endpoints();
                let range = if orders[&(patch.face, side.edge)] {
                    [lo, hi]
                } else {
                    [hi, lo]
                };
                let range = DirectedParameterRange::new(range).map_err(CodecError::malformed)?;
                coedges.push(id.clone());
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: ring.clone(),
                    edge: id!(EdgeId, "edge", side.edge),
                    radial_next,
                    sense: sense(side.forward == orders[&(patch.face, side.edge)]),
                    pcurves: vec![PcurveUse {
                        pcurve,
                        isoparametric: None,
                        parameter_range: Some(range),
                    }],
                    use_curve: None,
                });
            }
            ir.model.loops.push(Loop {
                id: ring.clone(),
                face: face.clone(),
                boundary: LoopBoundary::Ring(
                    LoopRing::new(coedges, Vec::new()).map_err(CodecError::malformed)?,
                ),
            });
        }
        ir.model.faces.push(Face {
            id: face.clone(),
            shell: shell.clone(),
            surface,
            sense: sense(patch.forward),
            loops: FaceLoops::classified(outer, inner_rings),
            name: None,
            color: None,
            tolerance,
        });
        faces.push(face);
    }
    for (face, patch) in faces.iter().zip(patches) {
        if patch.sense_inferred {
            annotations.field_exactness(face, "sense", Exactness::Inferred)?;
        }
    }
    ir.model.shells.push(
        Shell::with_faces(shell.clone(), region.clone(), faces).map_err(CodecError::malformed)?,
    );
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body_id.clone(),
        shells: vec![shell],
    });
    ir.model.bodies.push(Body {
        removed_by: None,
        id: body_id,
        kind: BodyKind::Sheet,
        regions: vec![region],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    Ok(true)
}

fn sense(forward: bool) -> Sense {
    if forward {
        Sense::Forward
    } else {
        Sense::Reversed
    }
}

#[cfg(test)]
mod tests;
