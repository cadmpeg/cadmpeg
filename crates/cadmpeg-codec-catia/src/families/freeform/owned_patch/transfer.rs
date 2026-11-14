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

use super::roster::BodyRoots;
use crate::assemble::annotate;
use crate::families::FamilyEntityAdmission;
use crate::loss::CatiaLossCode;
use crate::nurbs::LaneRefusals;

use super::{
    curve_point, edge_alignment, interval_parameter, trimmed_face, Patch, SheetSurfaceFit, Use,
    GAP_MM,
};

const EDGE_SAMPLES: u32 = 40;
const MAX_SHARED_EDGE_SEED_GAP_MM: f64 = 0.005;
const MAX_CORROBORATED_VERTEX_GAP_MM: f64 = 0.005;
type AdmittedEdges = BTreeMap<usize, (usize, usize, usize)>;

#[derive(Clone, Copy, Default)]
struct Reconciliation {
    vertex_repair: Option<f64>,
    seed_alignment: Option<f64>,
    retry: Option<f64>,
    replaced_edge: Option<f64>,
}

fn edge_discrepancy(a: &Use, b: &Use) -> f64 {
    let Some(same) = edge_alignment::edge_relation(a, b) else {
        return 0.0;
    };
    (0..=EDGE_SAMPLES)
        .filter_map(|step| {
            let fraction = f64::from(step) / f64::from(EDGE_SAMPLES);
            let a = curve_point(
                &a.curve,
                interval_parameter(a.curve.full_knot_endpoints().endpoints(), fraction),
            )?;
            let b = curve_point(
                &b.curve,
                interval_parameter(
                    b.curve.full_knot_endpoints().endpoints(),
                    if same { fraction } else { 1.0 - fraction },
                ),
            )?;
            Some(a.distance(b))
        })
        .fold(0.0, f64::max)
}

fn matches_edge(a: &Use, b: &Use) -> Option<bool> {
    matches_edge_within(a, b, GAP_MM)
}

fn matches_edge_within(a: &Use, b: &Use, tolerance: f64) -> Option<bool> {
    let same = edge_alignment::edge_relation(a, b)?;
    let sample = |side: &Use, f: f64| {
        let t = interval_parameter(side.curve.full_knot_endpoints().endpoints(), f);
        eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, &side.curve, t)
            .ok()
            .map(FinitePoint3::get)
    };
    for step in 0..=EDGE_SAMPLES {
        let f = f64::from(step) / f64::from(EDGE_SAMPLES);
        if sample(a, f)?.distance(sample(b, if same { f } else { 1.0 - f })?) > tolerance {
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
    body_roots: &BodyRoots,
) -> Result<Option<LossNote>, CodecError> {
    let mut by_body = BTreeMap::<_, Vec<_>>::new();
    for patch in patches {
        by_body.entry(patch.body).or_default().push(patch);
    }
    let mut count = 0;
    let mut inferred_senses = 0;
    let mut boundary_recovery = [(0_usize, 0.0_f64); 3];
    let mut reconciled = [0; 4];
    let mut max_discrepancy_mm = 0.0_f64;
    let (mut rolling_ball, mut offset, mut ruled, mut extrusion) = (0, 0, 0, 0);
    let (mut adjusted, mut max_shift_mm) = (0, 0.0_f64);
    for ((source, body), mut patches) in by_body {
        let roots = body_roots
            .get(&(source, body))
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([body]));
        patches.sort_by_key(|patch| patch.priority);
        let mut reconciliations = BTreeMap::new();
        let selected = compatible_patches(&mut patches, &mut reconciliations);
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
            for &root in &roots {
                if append_component(ir, annotations, source, (root, body), &patches, admission)? {
                    count += patches.len();
                    for index in &component {
                        if let Some(record) = reconciliations.get(index) {
                            for (count, gap) in reconciled.iter_mut().zip([
                                record.vertex_repair,
                                record.seed_alignment,
                                record.retry,
                                record.replaced_edge,
                            ]) {
                                if let Some(gap) = gap {
                                    *count += 1;
                                    max_discrepancy_mm = max_discrepancy_mm.max(gap);
                                }
                            }
                        }
                    }
                    for patch in &patches {
                        let accuracy_gap = patch
                            .sides
                            .iter()
                            .map(|side| side.lift_error_mm)
                            .filter(|&gap| gap > GAP_MM)
                            .fold(0.0_f64, f64::max);
                        for ((faces, largest), gap) in boundary_recovery.iter_mut().zip([
                            patch.recovery.rolling_extension_mm,
                            patch.recovery.affine_vertex_shift_mm,
                            accuracy_gap,
                        ]) {
                            if gap > 0.0 {
                                *faces += 1;
                                *largest = largest.max(gap);
                            }
                        }
                    }
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
    }
    if reconciled.iter().any(|&count| count != 0) {
        refusal.push_shared_boundary_reconciled(reconciled, max_discrepancy_mm);
    }
    for (code, (faces, gap)) in [
        CatiaLossCode::GeometryRollingSheetExtended,
        CatiaLossCode::TopologyAffineVertexReconciled,
        CatiaLossCode::GeometryParameterMapAccuracyInferred,
    ]
    .into_iter()
    .zip(boundary_recovery)
    {
        if faces > 0 {
            refusal.push_sheet_boundary_recovery(code, faces, gap);
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
            "Transferred {count} bounded faces as sheet subsets of their native body rosters. The complete native body is retained."
        ))
    }))
}

// Use the admitted vertex when a geometrically matching occurrence of the
// same edge corroborates it. Retain the independently fitted curve endpoints.
fn repair_corroborated_vertices(
    patch: &mut Patch,
    prior: &[Patch],
    vertices: &BTreeMap<usize, Point3>,
    edges: &AdmittedEdges,
    retry: bool,
) -> f64 {
    let mut repairs = BTreeMap::new();
    for side in &patch.sides {
        for &(vertex, point) in &side.vertices {
            let Some(&anchor) = vertices.get(&vertex) else {
                continue;
            };
            let gap = anchor.distance(point);
            if gap <= GAP_MM || gap > MAX_CORROBORATED_VERTEX_GAP_MM {
                continue;
            }
            let witnessed = patch.sides.iter().any(|witness| {
                let Some(&(1, face, edge)) = edges.get(&witness.edge) else {
                    return false;
                };
                let prior = &prior[face].sides[edge];
                let incoming = witness
                    .vertices
                    .iter()
                    .any(|&(id, point)| id == vertex && point.distance(anchor) <= GAP_MM);
                let inherited = retry
                    && prior
                        .vertices
                        .iter()
                        .zip(prior.curve.full_knot_endpoints().endpoints())
                        .any(|(&(id, point), parameter)| {
                            id == vertex
                                && point.distance(anchor) <= GAP_MM
                                && curve_point(&prior.curve, parameter).is_some_and(|end| {
                                    let gap = end.distance(anchor);
                                    gap > GAP_MM && gap <= MAX_CORROBORATED_VERTEX_GAP_MM
                                })
                        });
                (incoming || inherited) && matches_edge(prior, witness).is_some()
            });
            if witnessed {
                repairs.insert(vertex, anchor);
            }
        }
    }
    let mut max_gap_mm = 0.0_f64;
    for side in &mut patch.sides {
        for (vertex, point) in &mut side.vertices {
            if let Some(&anchor) = repairs.get(vertex) {
                let gap = point.distance(anchor);
                if gap <= MAX_CORROBORATED_VERTEX_GAP_MM {
                    max_gap_mm = max_gap_mm.max(gap);
                    *point = anchor;
                }
            }
        }
    }
    max_gap_mm
}

struct TrimEndpoint {
    face: usize,
    edge: usize,
    point: Point3,
}

fn endpoint_witnesses(patches: &[Patch]) -> BTreeMap<usize, Vec<TrimEndpoint>> {
    let mut witnesses = BTreeMap::<_, Vec<_>>::new();
    for patch in patches {
        for side in &patch.sides {
            for (&(vertex, _), parameter) in side
                .vertices
                .iter()
                .zip(side.curve.full_knot_endpoints().endpoints())
            {
                if let Some(point) = curve_point(&side.curve, parameter) {
                    witnesses.entry(vertex).or_default().push(TrimEndpoint {
                        face: patch.face,
                        edge: side.edge,
                        point,
                    });
                }
            }
        }
    }
    witnesses
}

fn has_trim_witnesses(
    face: usize,
    side: &Use,
    witnesses: &BTreeMap<usize, Vec<TrimEndpoint>>,
) -> bool {
    side.vertices
        .iter()
        .zip(side.curve.full_knot_endpoints().endpoints())
        .all(|(&(vertex, anchor), parameter)| {
            curve_point(&side.curve, parameter).is_some_and(|point| {
                point.distance(anchor) <= GAP_MM
                    || witnesses.get(&vertex).is_some_and(|ends| {
                        ends.iter().any(|end| {
                            end.face != face
                                && end.edge != side.edge
                                && end.point.distance(point) <= GAP_MM
                        })
                    })
            })
        })
}

fn compatible_patches(
    patches: &mut Vec<Patch>,
    reconciliations: &mut BTreeMap<usize, Reconciliation>,
) -> BTreeSet<usize> {
    let mut admitted = BTreeSet::<usize>::new();
    let mut vertices = BTreeMap::<usize, Point3>::new();
    let mut edges = AdmittedEdges::new();
    let witnesses = endpoint_witnesses(patches);
    for index in 0..patches.len() {
        let (prior, rest) = patches.split_at_mut(index);
        let patch = &mut rest[0];
        let rolling_boundary_retry =
            patch.priority == super::sheet_group::ROLLING_BOUNDARY_RETRY_PRIORITY;
        if rolling_boundary_retry && admitted.iter().any(|&i| prior[i].face == patch.face) {
            continue;
        }
        let vertex_gap = repair_corroborated_vertices(patch, prior, &vertices, &edges, false);
        let repaired_vertices = vertex_gap > 0.0;
        if repaired_vertices {
            reconciliations.entry(index).or_default().vertex_repair = Some(vertex_gap);
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
                    } else if rolling_boundary_retry || repaired_vertices {
                        false
                    } else {
                        let mut aligned = first.clone();
                        let surface = &prior[face].surface;
                        let ordinary = edge_alignment::align(side, &mut aligned, surface).is_some();
                        let seeded = !ordinary
                            && has_trim_witnesses(patch.face, side, &witnesses)
                            && edge_alignment::align_seed(side, &mut aligned, surface).is_some();
                        let closed = ordinary || seeded;
                        if closed {
                            replacements.push((
                                face,
                                side_index,
                                aligned,
                                seeded.then(|| edge_discrepancy(first, side)),
                            ));
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
        for (face, side, aligned, gap) in replacements {
            prior[face].sides[side] = aligned;
            if let Some(gap) = gap {
                let admission = reconciliations.entry(index).or_default();
                admission.seed_alignment = Some(admission.seed_alignment.unwrap_or(0.0).max(gap));
                let changed = reconciliations.entry(face).or_default();
                changed.replaced_edge = Some(changed.replaced_edge.unwrap_or(0.0).max(gap));
            }
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
    let mut admitted_faces = admitted
        .iter()
        .map(|&i| patches[i].face)
        .collect::<BTreeSet<_>>();
    loop {
        let before = admitted.len();
        for index in 0..patches.len() {
            let patch = &patches[index];
            if admitted_faces.contains(&patch.face) {
                continue;
            }
            let Some((retry, mut record)) =
                retry_shared_boundary(patch, patches, &vertices, &edges)
            else {
                continue;
            };
            if let Some(gap) = reconciliations
                .get(&index)
                .and_then(|record| record.vertex_repair)
            {
                record.vertex_repair = Some(record.vertex_repair.unwrap_or(0.0).max(gap));
                record.retry = Some(record.retry.unwrap_or(0.0).max(gap));
            }
            let index = patches.len();
            reconciliations.insert(index, record);
            for (side_index, side) in retry.sides.iter().enumerate() {
                for &(id, point) in &side.vertices {
                    vertices.entry(id).or_insert(point);
                }
                edges
                    .entry(side.edge)
                    .and_modify(|(count, _, _)| *count += 1)
                    .or_insert((1, index, side_index));
            }
            admitted.insert(index);
            admitted_faces.insert(retry.face);
            patches.push(retry);
        }
        if admitted.len() == before {
            break;
        }
    }
    admitted
}

fn retry_shared_boundary(
    patch: &Patch,
    admitted: &[Patch],
    vertices: &BTreeMap<usize, Point3>,
    edges: &AdmittedEdges,
) -> Option<(Patch, Reconciliation)> {
    let mut retry = Patch {
        sides: patch.sides.clone(),
        inner_starts: patch.inner_starts.clone(),
        surface: patch.surface.clone(),
        ..*patch
    };
    let vertex_gap = repair_corroborated_vertices(&mut retry, admitted, vertices, edges, true);
    let mut record = Reconciliation {
        vertex_repair: (vertex_gap > 0.0).then_some(vertex_gap),
        ..Reconciliation::default()
    };
    let mut max_gap_mm = vertex_gap;
    let mut modified = vertex_gap > 0.0;
    let mut local = BTreeMap::new();
    for side in &mut retry.sides {
        match edges.get(&side.edge) {
            Some(&(1, face, edge)) => {
                let prior = &admitted[face].sides[edge];
                if matches_edge(prior, side).is_none() {
                    max_gap_mm = max_gap_mm.max(edge_discrepancy(prior, side));
                    if edge_alignment::align(prior, side, &retry.surface).is_none() {
                        matches_edge_within(prior, side, MAX_SHARED_EDGE_SEED_GAP_MM)?;
                        *side =
                            edge_alignment::reproject_corroborated(prior, side, &retry.surface)?;
                    }
                    modified = true;
                }
            }
            None => (),
            Some(_) => return None,
        }
        for &(id, point) in &side.vertices {
            let existing = *local
                .entry(id)
                .or_insert_with(|| *vertices.get(&id).unwrap_or(&point));
            if point.distance(existing) > GAP_MM {
                return None;
            }
        }
    }
    if !modified {
        return None;
    }
    record.retry = Some(max_gap_mm);
    trimmed_face::holes_are_contained(&retry)?.then_some((retry, record))
}

fn append_component(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    source: usize,
    (root, body): (usize, usize),
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
    let affine_vertices = patches
        .iter()
        .filter(|patch| patch.recovery.affine_vertex_shift_mm > 0.0)
        .flat_map(|patch| &patch.sides)
        .flat_map(|side| side.vertices.map(|(id, _)| id))
        .collect::<BTreeSet<_>>();
    let mut inferred_fields = Vec::new();
    let first = patches[0].face;
    macro_rules! id {
        ($kind:ty, $label:literal, $offset:expr) => {
            <$kind>::mint(format!(
                concat!("catia:owned-patch:", $label, "#{}:{}:{}:{}:{}"),
                source, root, body, first, $offset
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
    let body_id = BodyId::mint(format!("catia:owned-patch:body#{source}:{root}"))
        .map_err(CodecError::malformed)?;
    let region = RegionId::mint(format!("catia:owned-patch:region#{source}:{root}"))
        .map_err(CodecError::malformed)?;
    let shell = id!(ShellId, "shell", body);
    let new_body = !ir
        .model
        .bodies
        .iter()
        .any(|candidate| candidate.id == body_id);
    if new_body {
        for id in [body_id.as_str(), region.as_str()] {
            admit(id, root, "bounded_face_subset")?;
        }
    }
    admit(shell.as_str(), body, "bounded_face_subset")?;
    let tolerance = PositiveReal::new(GAP_MM);
    for (vertex, point) in vertices {
        let point_id = id!(PointId, "point", vertex);
        let vertex_id = id!(VertexId, "vertex", vertex);
        let point = FinitePoint3::new(point)
            .ok_or_else(|| CodecError::malformed("CATIA patch vertex is non-finite"))?;
        admit(point_id.as_str(), vertex, "lifted_boundary_vertex")?;
        admit(vertex_id.as_str(), vertex, "boundary_vertex")?;
        if affine_vertices.contains(&vertex) {
            inferred_fields.push((point_id.to_string(), "position"));
        }
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
        let mut edge_accuracy_mm = GAP_MM;
        for (_, occurrence) in uses {
            if occurrence.lift_error_mm > GAP_MM {
                let accuracy = occurrence.inferred_accuracy.ok_or_else(|| {
                    CodecError::malformed("CATIA repaired edge has no inferred accuracy")
                })?;
                edge_accuracy_mm = edge_accuracy_mm.max(accuracy.bound_mm + GAP_MM);
                admit(edge_id.as_str(), accuracy.source, "parameter_map_accuracy")?;
            }
        }
        let tolerance = PositiveReal::new(edge_accuracy_mm);
        if edge_accuracy_mm > GAP_MM {
            inferred_fields.push((edge_id.to_string(), "tolerance"));
        }
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
    for (id, field) in inferred_fields {
        annotations.field_exactness(id, field, Exactness::Inferred)?;
    }
    for (face, patch) in faces.iter().zip(patches) {
        if patch.sense_inferred {
            annotations.field_exactness(face, "sense", Exactness::Inferred)?;
        }
    }
    ir.model.shells.push(
        Shell::with_faces(shell.clone(), region.clone(), faces).map_err(CodecError::malformed)?,
    );
    if let Some(owner) = ir.model.regions.iter_mut().find(|owner| owner.id == region) {
        owner.shells.push(shell);
    } else {
        ir.model.regions.push(Region {
            id: region.clone(),
            body: body_id.clone(),
            shells: vec![shell],
        });
    }
    if new_body {
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Sheet,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }
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
