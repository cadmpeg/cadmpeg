// SPDX-License-Identifier: Apache-2.0
//! Resolve unbounded circular and elliptic edge intervals from incident
//! revolution-surface boundary loops, complementary two-edge circular loops,
//! and unambiguous face-box evidence.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, EllipseCurve};
use cadmpeg_ir::math::Vector3;

use crate::families::standard::records::{
    StandardCurveGeometry, StandardCurveSupport, StandardFaceBounds,
};
use crate::families::standard::topology::admitted::StandardTopology;

use super::arc_bounds;
use super::{
    standard_analytic_curve_angle, standard_circle_axis_from_carrier, unwrap_angle,
    witness_arc_end, CadIr, CurveGeometry, CurveId, DecodeContext, HashMap, Point3,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry, SurfaceId,
    ANALYTIC_CURVE_ENDPOINT_TOLERANCE, CYLINDER_PLANE_CONIC_TOLERANCE,
    EPS_STANDARD_DECODE_GEOMETRY,
};

const AZIMUTH_ENDPOINT_MARGIN_RAD: f64 = 1.0e-3;
const AXIS_PARALLEL_TOLERANCE: f64 = 1.0e-4;
const BOX_DISAMBIGUATION_RATIO: f64 = 10.0;
const BOX_MAX_EXCESS_FRACTION: f64 = 0.05;
const BOX_MAX_EXCESS_MM: f64 = 0.5;
const SEGMENT_CROSSING_DENOM_TOLERANCE: f64 = 1.0e-12;

pub(in crate::families::standard::decode) struct CylindricalArcInputs<'a> {
    pub(in crate::families::standard::decode) ir: &'a CadIr,
    pub(in crate::families::standard::decode) bindings: &'a [(SurfaceId, bool, usize)],
    pub(in crate::families::standard::decode) surface_indices: &'a HashMap<SurfaceId, usize>,
    pub(in crate::families::standard::decode) curve_indices: &'a HashMap<CurveId, usize>,
    pub(in crate::families::standard::decode) supports: &'a [StandardCurveSupport],
    pub(in crate::families::standard::decode) face_bounds: &'a [Option<StandardFaceBounds>],
    pub(in crate::families::standard::decode) topology: &'a StandardTopology,
}

pub(in crate::families::standard::decode) fn ranges(
    ctx: &DecodeContext<'_>,
    inputs: &CylindricalArcInputs<'_>,
    endpoints: impl Fn(usize) -> [Point3; 2],
    vertex_position: impl Fn(usize) -> Point3,
) -> Result<BTreeMap<usize, Option<[f64; 2]>>, cadmpeg_core::CodecError> {
    let CylindricalArcInputs {
        ir,
        bindings,
        surface_indices,
        curve_indices,
        supports,
        face_bounds,
        topology,
    } = *inputs;
    let mut recovered = BTreeMap::<usize, Option<[f64; 2]>>::new();
    let mut active_ranges = BTreeMap::<usize, [f64; 2]>::new();
    let mut conflicted = BTreeSet::<usize>::new();
    for (row, edge) in ir.geometry.edges.iter().enumerate() {
        if let Some(range) = edge.param_range() {
            active_ranges.insert(row, range.get());
        }
    }

    let mut edge_incident_faces = vec![BTreeSet::<usize>::new(); ir.geometry.edges.len()];
    for (face_idx, face_topology) in topology.faces().iter().enumerate() {
        for boundary in face_topology.boundaries() {
            for cu in boundary.coedges() {
                if let Some(slot) = edge_incident_faces.get_mut(cu.edge_row) {
                    slot.insert(face_idx);
                }
            }
        }
    }
    for (row, support) in supports.iter().enumerate() {
        if let Some(slot) = edge_incident_faces.get_mut(row) {
            slot.insert(support.faces[0]);
            slot.insert(support.faces[1]);
        }
    }

    let edge_curve_geom = |row: usize| -> Option<&CurveGeometry> {
        let curve = ir.geometry.edges.get(row)?.curve()?;
        Some(&ir.geometry.curves[*curve_indices.get(curve)?].geometry)
    };
    let edge_circle = |row: usize| -> Option<&CircleCurve> {
        match edge_curve_geom(row)? {
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => Some(circle),
            _ => None,
        }
    };
    let edge_ellipse = |row: usize| -> Option<&EllipseCurve> {
        match edge_curve_geom(row)? {
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)) => Some(ellipse),
            _ => None,
        }
    };

    // Pre-pass: invalidate pre-assigned circular arc ranges that contradict
    // topology so the fixed-point solver below can re-assign consistent arcs:
    //  1) Two-edge circular loops where both edges chose the SAME overlapping
    //     arc instead of complementary arcs (degenerate zero-area loop).
    //  2) Coaxial circular edges on a revolution-surface loop that strictly
    //     contain another vertex of a co-circular edge on the same circle.
    let mut invalidated = BTreeSet::<usize>::new();
    for face_topology in topology.faces() {
        for boundary in face_topology.boundaries() {
            let [u0, u1] = boundary.coedges() else {
                continue;
            };
            let (e0, e1) = (u0.edge_row, u1.edge_row);
            if e0 == e1 {
                continue;
            }
            let (Some(&r0), Some(&r1)) = (active_ranges.get(&e0), active_ranges.get(&e1)) else {
                continue;
            };
            let (Some(c0), Some(c1)) = (edge_circle(e0), edge_circle(e1)) else {
                continue;
            };
            if !same_circle_carrier(c0, c1)
                || !same_distinct_unordered_pair(endpoints(e0), endpoints(e1))
            {
                continue;
            }
            if circles_overlap_same_arc(c0, r0, c1, r1) {
                invalidated.insert(e0);
                invalidated.insert(e1);
            }
        }
    }
    for (face, face_topology) in topology.faces().iter().enumerate() {
        let Some(surface) = bindings
            .get(face)
            .and_then(|(id, _, _)| surface_indices.get(id))
            .and_then(|&idx| ir.geometry.surfaces.get(idx))
            .map(|s| &s.geometry)
        else {
            continue;
        };
        if surface_revolution_axis(surface).is_none() {
            continue;
        }
        for boundary in face_topology.boundaries() {
            let coedges = boundary.coedges();
            for cu in coedges {
                let row = cu.edge_row;
                let (Some(&range), Some(circle)) = (active_ranges.get(&row), edge_circle(row))
                else {
                    continue;
                };
                let ends = endpoints(row);
                if ends[0].distance(ends[1]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
                    continue;
                }
                let center = circle.center().get();
                let reference = *circle.frame().reference().as_raw();
                let tangent = circle.frame().axis().as_raw().cross(reference);
                let overlaps_cocircular = coedges.iter().any(|other| {
                    let other_row = other.edge_row;
                    if other_row == row {
                        return false;
                    }
                    let Some(other_circle) = edge_circle(other_row) else {
                        return false;
                    };
                    if !same_circle_carrier(circle, other_circle) {
                        return false;
                    }
                    let other_ends = endpoints(other_row);
                    other_ends.into_iter().any(|pt| {
                        if pt.distance(ends[0]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                            || pt.distance(ends[1]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                        {
                            return false;
                        }
                        let v = pt.vector_from(center);
                        let theta = v.dot(tangent).atan2(v.dot(reference));
                        theta.is_finite() && angle_strictly_in_periodic_arc(theta, range)
                    })
                });
                if overlaps_cocircular {
                    invalidated.insert(row);
                }
            }
        }
    }
    for row in invalidated {
        active_ranges.remove(&row);
    }

    let commit_proposals = |proposals: BTreeMap<usize, Option<[f64; 2]>>,
                            recovered: &mut BTreeMap<usize, Option<[f64; 2]>>,
                            active_ranges: &mut BTreeMap<usize, [f64; 2]>,
                            conflicted: &mut BTreeSet<usize>|
     -> bool {
        let mut changed = false;
        for (row, range) in proposals {
            if active_ranges.contains_key(&row) || conflicted.contains(&row) {
                continue;
            }
            match range {
                Some(directed) => {
                    let ordered = if directed[0] <= directed[1] {
                        directed
                    } else {
                        [directed[1], directed[0]]
                    };
                    active_ranges.insert(row, ordered);
                    recovered.insert(row, Some(directed));
                    changed = true;
                }
                None => {
                    conflicted.insert(row);
                    recovered.insert(row, None);
                }
            }
        }
        changed
    };

    loop {
        // Pass 1: Coaxial cylinder/cone/torus/sphere boundary loop witnesses
        // (covers both 4-edge generator-checked cylinder strips and multi-edge
        // curved faces, for circles and oblique cylinder-plane ellipses).
        let mut proposals = BTreeMap::<usize, Option<[f64; 2]>>::new();
        for (face, face_topology) in topology.faces().iter().enumerate() {
            let Some(surface) = bindings
                .get(face)
                .and_then(|(id, _, _)| surface_indices.get(id))
                .and_then(|&idx| ir.geometry.surfaces.get(idx))
                .map(|s| &s.geometry)
            else {
                continue;
            };
            let Some((surf_origin, surf_axis, cyl_ref)) = surface_revolution_axis(surface) else {
                continue;
            };
            for boundary in face_topology.boundaries() {
                let coedges = boundary.coedges();
                if let (Some(cylinder), [a, b, c, d]) = (cyl_ref, coedges) {
                    let is_line = |edge: usize| {
                        matches!(supports[edge].geometry, StandardCurveGeometry::Line)
                    };
                    let strip_pair = if is_line(b.edge_row) && is_line(d.edge_row) {
                        Some(([a.edge_row, c.edge_row], [b.edge_row, d.edge_row]))
                    } else if is_line(a.edge_row) && is_line(c.edge_row) {
                        Some(([b.edge_row, d.edge_row], [a.edge_row, c.edge_row]))
                    } else {
                        None
                    };
                    if let Some(([first, second], generators)) = strip_pair {
                        if edge_circle(first).is_some() && edge_circle(second).is_some() {
                            if generators.into_iter().any(|edge| {
                                let [start, end] = endpoints(edge);
                                end.vector_from(start)
                                    .cross(*cylinder.frame().axis().as_raw())
                                    .norm()
                                    > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                            }) {
                                for edge in [first, second] {
                                    if !active_ranges.contains_key(&edge) {
                                        proposals.insert(edge, None);
                                    }
                                }
                                continue;
                            }
                            let (target, witness, range) =
                                match [active_ranges.get(&first), active_ranges.get(&second)] {
                                    [None, Some(&range)] => (first, second, range),
                                    [Some(&range), None] => (second, first, range),
                                    _ => continue,
                                };
                            if conflicted.contains(&target) {
                                continue;
                            }
                            if let (Some(target_circle), Some(witness_circle)) =
                                (edge_circle(target), edge_circle(witness))
                            {
                                if let Some(range) = translated_range(
                                    surface,
                                    target_circle,
                                    endpoints(target),
                                    witness_circle,
                                    endpoints(witness),
                                    range,
                                ) {
                                    merge_proposal(&mut proposals, target, range);
                                }
                            }
                            continue;
                        }
                    }
                }
                let mut unranged_targets = Vec::new();
                for cu in coedges {
                    let row = cu.edge_row;
                    if active_ranges.contains_key(&row) || conflicted.contains(&row) {
                        continue;
                    }
                    if edge_circle(row).is_some()
                        || (cyl_ref.is_some() && edge_ellipse(row).is_some())
                    {
                        unranged_targets.push(row);
                    }
                }
                if unranged_targets.is_empty() {
                    continue;
                }
                let witnesses = collect_loop_witness_points(
                    ctx,
                    coedges,
                    &active_ranges,
                    &edge_curve_geom,
                    &endpoints,
                )?;
                if witnesses.is_empty() {
                    continue;
                }
                for target in unranged_targets {
                    let target_ends = endpoints(target);
                    if target_ends[0].distance(target_ends[1]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                    {
                        continue;
                    }
                    let proposed = if let Some(circle) = edge_circle(target) {
                        circle_range_from_revolution_witnesses(
                            surf_origin,
                            surf_axis,
                            circle,
                            target_ends,
                            &witnesses,
                        )
                    } else if let (Some(cylinder), Some(ellipse)) = (cyl_ref, edge_ellipse(target))
                    {
                        ellipse_range_from_cylinder_witnesses(
                            cylinder,
                            ellipse,
                            target_ends,
                            &witnesses,
                        )
                    } else {
                        None
                    };
                    if let Some(range) = proposed {
                        merge_proposal(&mut proposals, target, range);
                    }
                }
            }
        }
        if commit_proposals(
            proposals,
            &mut recovered,
            &mut active_ranges,
            &mut conflicted,
        ) {
            continue;
        }

        // Pass 2: Complementary 2-edge circular loops where one edge is ranged
        // and the other shares the same circle and endpoints.
        let mut proposals = BTreeMap::<usize, Option<[f64; 2]>>::new();
        for face_topology in topology.faces() {
            for boundary in face_topology.boundaries() {
                let [u0, u1] = boundary.coedges() else {
                    continue;
                };
                let (e0, e1) = (u0.edge_row, u1.edge_row);
                if e0 == e1 {
                    continue;
                }
                let (Some(c0), Some(c1)) = (edge_circle(e0), edge_circle(e1)) else {
                    continue;
                };
                if !same_circle_carrier(c0, c1) {
                    continue;
                }
                let (known_row, known_circle, known_range, target_row, target_circle) =
                    match (active_ranges.get(&e0), active_ranges.get(&e1)) {
                        (Some(&r), None) => (e0, c0, r, e1, c1),
                        (None, Some(&r)) => (e1, c1, r, e0, c0),
                        _ => continue,
                    };
                if conflicted.contains(&target_row) {
                    continue;
                }
                let known_ends = endpoints(known_row);
                let target_ends = endpoints(target_row);
                if !same_distinct_unordered_pair(known_ends, target_ends) {
                    continue;
                }
                if let Some(range) = complementary_circle_range(
                    known_circle,
                    known_range,
                    target_circle,
                    target_ends,
                ) {
                    merge_proposal(&mut proposals, target_row, range);
                }
            }
        }
        if commit_proposals(
            proposals,
            &mut recovered,
            &mut active_ranges,
            &mut conflicted,
        ) {
            continue;
        }

        // Pass 3: Unambiguous face-box disambiguation for unranged circular and
        // elliptic edges where tessellation box shortfall prevented exact-zero
        // fitting.
        let mut proposals = BTreeMap::<usize, Option<[f64; 2]>>::new();
        for (row, incident_faces) in edge_incident_faces.iter().enumerate() {
            if active_ranges.contains_key(&row) || conflicted.contains(&row) {
                continue;
            }
            let ends = endpoints(row);
            if ends[0].distance(ends[1]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
                continue;
            }
            let bounds: Vec<StandardFaceBounds> = incident_faces
                .iter()
                .filter_map(|&f| face_bounds.get(f).and_then(|b| *b))
                .collect();
            let proposed = if let Some(circle) = edge_circle(row) {
                unambiguous_box_circle_range(circle, ends, &bounds)
            } else if let Some(ellipse) = edge_ellipse(row) {
                unambiguous_box_ellipse_range(ellipse, ends, &bounds)
            } else {
                None
            };
            if let Some(range) = proposed {
                merge_proposal(&mut proposals, row, range);
            }
        }
        if commit_proposals(
            proposals,
            &mut recovered,
            &mut active_ranges,
            &mut conflicted,
        ) {
            continue;
        }

        // Pass 4: Complementary 2-edge circular loop pair breaking where both
        // edges remain unranged (antipodal semicircles or non-antipodal
        // complementary arcs invalidated by overlap). Resolves one pair at a
        // time and re-enters Pass 1 so cylinder/cone walls propagate the
        // choice consistently to the opposite rim.
        let mut broke_pair = false;
        'pair_search: for (face_idx, face_topology) in topology.faces().iter().enumerate() {
            for boundary in face_topology.boundaries() {
                let [u0, u1] = boundary.coedges() else {
                    continue;
                };
                let (e0, e1) = (u0.edge_row, u1.edge_row);
                if e0 == e1
                    || active_ranges.contains_key(&e0)
                    || active_ranges.contains_key(&e1)
                    || conflicted.contains(&e0)
                    || conflicted.contains(&e1)
                {
                    continue;
                }
                let (Some(c0), Some(c1)) = (edge_circle(e0), edge_circle(e1)) else {
                    continue;
                };
                if !same_circle_carrier(c0, c1) {
                    continue;
                }
                let ends0 = endpoints(e0);
                let ends1 = endpoints(e1);
                if !same_distinct_unordered_pair(ends0, ends1) {
                    continue;
                }
                let Some((r0, r1)) = assign_antipodal_semicircle_pair(
                    face_idx,
                    (e0, c0, ends0),
                    (e1, c1, ends1),
                    supports,
                    face_bounds,
                ) else {
                    continue;
                };
                let mut proposals = BTreeMap::new();
                proposals.insert(e0, Some(r0));
                proposals.insert(e1, Some(r1));
                if commit_proposals(
                    proposals,
                    &mut recovered,
                    &mut active_ranges,
                    &mut conflicted,
                ) {
                    broke_pair = true;
                    break 'pair_search;
                }
            }
        }
        if !broke_pair {
            break;
        }
    }

    repair_revolution_single_wire_winding(
        ctx,
        &WindingRepairInputs {
            ir,
            bindings,
            surface_indices,
            edge_incident_faces: &edge_incident_faces,
            face_bounds,
            topology,
        },
        &edge_curve_geom,
        &edge_circle,
        (&endpoints, &vertex_position),
        &mut active_ranges,
        &mut recovered,
    )?;

    Ok(recovered)
}

struct WindingRepairInputs<'a> {
    ir: &'a CadIr,
    bindings: &'a [(SurfaceId, bool, usize)],
    surface_indices: &'a HashMap<SurfaceId, usize>,
    edge_incident_faces: &'a [BTreeSet<usize>],
    face_bounds: &'a [Option<StandardFaceBounds>],
    topology: &'a StandardTopology,
}

#[derive(Clone)]
struct LoopEdgeUv {
    edge_row: usize,
    du: f64,
    pts: Vec<[f64; 2]>,
    flippable: Option<FlipCandidate>,
}

#[derive(Clone)]
struct FlipCandidate {
    flipped_directed: [f64; 2],
    flipped_du: f64,
    flipped_pts: Vec<[f64; 2]>,
    orig_box_gap: f64,
    flipped_box_gap: f64,
}

fn repair_revolution_single_wire_winding<'a>(
    ctx: &DecodeContext<'_>,
    inputs: &WindingRepairInputs<'_>,
    edge_curve_geom: &impl Fn(usize) -> Option<&'a CurveGeometry>,
    edge_circle: &impl Fn(usize) -> Option<&'a CircleCurve>,
    (endpoints, vertex_position): (&impl Fn(usize) -> [Point3; 2], &impl Fn(usize) -> Point3),
    active_ranges: &mut BTreeMap<usize, [f64; 2]>,
    recovered: &mut BTreeMap<usize, Option<[f64; 2]>>,
) -> Result<(), cadmpeg_core::CodecError> {
    use std::f64::consts::{PI, TAU};

    let WindingRepairInputs {
        ir,
        bindings,
        surface_indices,
        edge_incident_faces,
        face_bounds,
        topology,
    } = *inputs;

    let mut flipped_edges = BTreeSet::<usize>::new();

    for (face, face_topology) in topology.faces().iter().enumerate() {
        let [boundary] = face_topology.boundaries() else {
            continue;
        };
        let Some(surface) = bindings
            .get(face)
            .and_then(|(id, _, _)| surface_indices.get(id))
            .and_then(|&idx| ir.geometry.surfaces.get(idx))
            .map(|s| &s.geometry)
        else {
            continue;
        };
        let (surf_origin, surf_axis, is_cylinder) = match surface {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(c)) => {
                (c.origin().get(), *c.frame().axis().as_raw(), true)
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(c)) => {
                (c.origin().get(), *c.frame().axis().as_raw(), false)
            }
            _ => continue,
        };
        let ref_dir = cadmpeg_ir::geometry::derive_reference_direction(surf_axis);
        let tan_dir = surf_axis.cross(ref_dir);
        let uv_of = |p: Point3| -> Option<[f64; 2]> {
            let offset = p.vector_from(surf_origin);
            let v = offset.dot(surf_axis);
            let radial = offset - surf_axis.scale(v);
            if radial.norm() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
                return None;
            }
            let u = radial.dot(tan_dir).atan2(radial.dot(ref_dir));
            (u.is_finite() && v.is_finite()).then_some([u, v])
        };

        let coedges = boundary.coedges();
        if coedges.len() < 3 {
            continue;
        }

        let mut loop_edges = Vec::with_capacity(coedges.len());
        let mut complete = true;
        for cu in coedges {
            let row = cu.edge_row;
            let raw_ends = endpoints(row);
            let p_start = vertex_position(cu.start_vertex);
            let p_end = vertex_position(cu.end_vertex);
            let (Some(uv0), Some(uv1)) = (uv_of(p_start), uv_of(p_end)) else {
                complete = false;
                break;
            };
            let Some(curve) = edge_curve_geom(row) else {
                complete = false;
                break;
            };
            let solved = match curve {
                CurveGeometry::Solved(s) => Some(s),
                CurveGeometry::Procedural { cache: Some(s), .. } => Some(s),
                CurveGeometry::Procedural { cache: None, .. } => None,
            };
            match solved {
                Some(SolvedCurveGeometry::Line(_)) => {
                    let du = (uv1[0] - uv0[0] + PI).rem_euclid(TAU) - PI;
                    loop_edges.push(LoopEdgeUv {
                        edge_row: row,
                        du,
                        pts: vec![[0.0, uv0[1]], [du, uv1[1]]],
                        flippable: None,
                    });
                }
                Some(SolvedCurveGeometry::Circle(circle)) => {
                    let default_range = active_ranges.get(&row).copied().or_else(|| {
                        if raw_ends[0].distance(raw_ends[1]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
                            return None;
                        }
                        let (pos, neg) = circle_directed_arcs(circle, raw_ends)?;
                        let pos_ord = [pos[0].min(pos[1]), pos[0].max(pos[1])];
                        let neg_ord = [neg[0].min(neg[1]), neg[0].max(neg[1])];
                        Some(if (pos_ord[1] - pos_ord[0]) <= (neg_ord[1] - neg_ord[0]) {
                            pos_ord
                        } else {
                            neg_ord
                        })
                    });
                    let Some([lo, hi]) = default_range else {
                        complete = false;
                        break;
                    };
                    let c_axis = *circle.frame().axis().as_raw();
                    let coaxial = c_axis.cross(surf_axis).norm() <= AXIS_PARALLEL_TOLERANCE
                        && circle
                            .center()
                            .get()
                            .vector_from(surf_origin)
                            .cross(surf_axis)
                            .norm()
                            <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE;
                    if coaxial
                        && raw_ends[0].distance(raw_ends[1]) > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                    {
                        let p_lo = circle_point_at(circle, lo);
                        let p_hi = circle_point_at(circle, hi);
                        let forward_on_curve = p_start.distance(p_lo) <= p_start.distance(p_hi);
                        let sweep = hi - lo;
                        let axis_sign = if c_axis.dot(surf_axis) >= 0.0 {
                            1.0
                        } else {
                            -1.0
                        };
                        let dir_sign = if forward_on_curve { 1.0 } else { -1.0 };
                        let du = axis_sign * dir_sign * sweep;
                        let flippable = circle_directed_arcs(circle, raw_ends).map(|(pos, neg)| {
                            let mid_cur = lo.midpoint(hi);
                            let in_pos = angle_strictly_in_periodic_arc(mid_cur, pos);
                            let flipped_directed = if in_pos { neg } else { pos };
                            let flipped_du = if du > 0.0 { du - TAU } else { du + TAU };
                            let bounds: Vec<StandardFaceBounds> = edge_incident_faces[row]
                                .iter()
                                .filter_map(|&f| face_bounds.get(f).and_then(|b| *b))
                                .collect();
                            let orig_box_gap = circle_bounds_excess(circle, [lo, hi], &bounds);
                            let fl_ord = [
                                flipped_directed[0].min(flipped_directed[1]),
                                flipped_directed[0].max(flipped_directed[1]),
                            ];
                            let flipped_box_gap = circle_bounds_excess(circle, fl_ord, &bounds);
                            FlipCandidate {
                                flipped_directed,
                                flipped_du,
                                flipped_pts: vec![[0.0, uv0[1]], [flipped_du, uv1[1]]],
                                orig_box_gap,
                                flipped_box_gap,
                            }
                        });
                        loop_edges.push(LoopEdgeUv {
                            edge_row: row,
                            du,
                            pts: vec![[0.0, uv0[1]], [du, uv1[1]]],
                            flippable,
                        });
                    } else {
                        let Some((du, pts)) =
                            sample_curve_uv(ctx, curve, [lo, hi], p_start, &uv_of)?
                        else {
                            complete = false;
                            break;
                        };
                        loop_edges.push(LoopEdgeUv {
                            edge_row: row,
                            du,
                            pts,
                            flippable: None,
                        });
                    }
                }
                Some(SolvedCurveGeometry::Ellipse(ellipse)) => {
                    let default_range = active_ranges.get(&row).copied().or_else(|| {
                        if raw_ends[0].distance(raw_ends[1]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
                            return None;
                        }
                        let (pos, neg) = ellipse_directed_arcs(ellipse, raw_ends)?;
                        let pos_ord = [pos[0].min(pos[1]), pos[0].max(pos[1])];
                        let neg_ord = [neg[0].min(neg[1]), neg[0].max(neg[1])];
                        Some(if (pos_ord[1] - pos_ord[0]) <= (neg_ord[1] - neg_ord[0]) {
                            pos_ord
                        } else {
                            neg_ord
                        })
                    });
                    let Some([lo, hi]) = default_range else {
                        complete = false;
                        break;
                    };
                    let Some((du, pts)) = sample_curve_uv(ctx, curve, [lo, hi], p_start, &uv_of)?
                    else {
                        complete = false;
                        break;
                    };
                    let flippable = if is_cylinder
                        && raw_ends[0].distance(raw_ends[1]) > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                    {
                        if let Some((pos, neg)) = ellipse_directed_arcs(ellipse, raw_ends) {
                            let mid_cur = lo.midpoint(hi);
                            let in_pos = angle_strictly_in_periodic_arc(mid_cur, pos);
                            let flipped_directed = if in_pos { neg } else { pos };
                            let fl_ord = [
                                flipped_directed[0].min(flipped_directed[1]),
                                flipped_directed[0].max(flipped_directed[1]),
                            ];
                            if let Some((flipped_du, flipped_pts)) =
                                sample_curve_uv(ctx, curve, fl_ord, p_start, &uv_of)?
                            {
                                let bounds: Vec<StandardFaceBounds> = edge_incident_faces[row]
                                    .iter()
                                    .filter_map(|&f| face_bounds.get(f).and_then(|b| *b))
                                    .collect();
                                let orig_box_gap =
                                    ellipse_bounds_excess(ellipse, [lo, hi], &bounds);
                                let flipped_box_gap =
                                    ellipse_bounds_excess(ellipse, fl_ord, &bounds);
                                Some(FlipCandidate {
                                    flipped_directed,
                                    flipped_du,
                                    flipped_pts,
                                    orig_box_gap,
                                    flipped_box_gap,
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    loop_edges.push(LoopEdgeUv {
                        edge_row: row,
                        du,
                        pts,
                        flippable,
                    });
                }
                Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    let [lo, hi] = active_ranges
                        .get(&row)
                        .copied()
                        .unwrap_or_else(|| nurbs.full_knot_endpoints().endpoints());
                    let Some((du, pts)) = sample_curve_uv(ctx, curve, [lo, hi], p_start, &uv_of)?
                    else {
                        complete = false;
                        break;
                    };
                    loop_edges.push(LoopEdgeUv {
                        edge_row: row,
                        du,
                        pts,
                        flippable: None,
                    });
                }
                _ => {
                    let Some(&[lo, hi]) = active_ranges.get(&row) else {
                        complete = false;
                        break;
                    };
                    let Some((du, pts)) = sample_curve_uv(ctx, curve, [lo, hi], p_start, &uv_of)?
                    else {
                        complete = false;
                        break;
                    };
                    loop_edges.push(LoopEdgeUv {
                        edge_row: row,
                        du,
                        pts,
                        flippable: None,
                    });
                }
            }
        }
        if !complete {
            continue;
        }

        let total_du: f64 = loop_edges.iter().map(|e| e.du).sum();
        let turns = (total_du / TAU).round();
        if turns.abs() < 0.5 || (total_du - turns * TAU).abs() > 0.08 {
            continue;
        }
        let need: i32 = if (turns - 1.0).abs() < 0.1 {
            1
        } else if (turns + 1.0).abs() < 0.1 {
            -1
        } else if (turns - 2.0).abs() < 0.1 {
            2
        } else if (turns + 2.0).abs() < 0.1 {
            -2
        } else {
            continue;
        };

        // Collect candidate indices whose flip changes total_du by -sign(need) * TAU.
        let want_delta = if need > 0 { -TAU } else { TAU };
        let candidates: Vec<usize> = loop_edges
            .iter()
            .enumerate()
            .filter_map(|(idx, e)| {
                let fl = e.flippable.as_ref()?;
                let delta = fl.flipped_du - e.du;
                ((delta - want_delta).abs() <= 0.15).then_some(idx)
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }

        let chosen: Option<Vec<usize>> = if need.abs() == 1 {
            let mut valid = Vec::new();
            let mut box_improved = Vec::new();
            for &ci in &candidates {
                let eval = evaluate_uv_loop(&loop_edges, &[ci]);
                let fl = loop_edges[ci].flippable.as_ref();
                if let Some(fl) = fl {
                    if !eval.has_crossing && !eval.has_overlap && eval.u_span <= TAU + 1.0e-2 {
                        valid.push((ci, fl.flipped_box_gap, eval.u_span));
                    } else if eval.u_span <= TAU + 1.0e-2
                        && fl.orig_box_gap > 0.5
                        && fl.flipped_box_gap <= 0.05 * fl.orig_box_gap
                    {
                        box_improved.push((ci, fl.flipped_box_gap, eval.u_span));
                    }
                }
            }
            if !valid.is_empty() {
                valid.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.total_cmp(&b.2)));
                Some(vec![valid[0].0])
            } else if box_improved.len() == 1 {
                Some(vec![box_improved[0].0])
            } else {
                None
            }
        } else if need.abs() == 2 && candidates.len() <= 12 {
            let mut valid = Vec::new();
            for a in 0..candidates.len() {
                for b in (a + 1)..candidates.len() {
                    let pair = [candidates[a], candidates[b]];
                    if loop_edges[pair[0]].edge_row == loop_edges[pair[1]].edge_row {
                        continue;
                    }
                    let eval = evaluate_uv_loop(&loop_edges, &pair);
                    if !eval.has_crossing && !eval.has_overlap && eval.u_span <= TAU + 1.0e-2 {
                        let gap = loop_edges[pair[0]]
                            .flippable
                            .as_ref()
                            .map_or(0.0, |f| f.flipped_box_gap)
                            + loop_edges[pair[1]]
                                .flippable
                                .as_ref()
                                .map_or(0.0, |f| f.flipped_box_gap);
                        valid.push((pair, gap, eval.u_span));
                    }
                }
            }
            if valid.is_empty() {
                None
            } else {
                valid.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.total_cmp(&b.2)));
                Some(valid[0].0.to_vec())
            }
        } else {
            None
        };

        if let Some(indices) = chosen {
            for idx in indices {
                let row = loop_edges[idx].edge_row;
                if let Some(fl) = &loop_edges[idx].flippable {
                    let directed = fl.flipped_directed;
                    let ordered = [directed[0].min(directed[1]), directed[0].max(directed[1])];
                    active_ranges.insert(row, ordered);
                    recovered.insert(row, Some(directed));
                    flipped_edges.insert(row);
                }
            }
        }
    }

    // Keep any 2-edge complementary circle loop partner synchronized if one edge
    // was flipped above.
    if !flipped_edges.is_empty() {
        for face_topology in topology.faces() {
            for boundary in face_topology.boundaries() {
                let [u0, u1] = boundary.coedges() else {
                    continue;
                };
                let (e0, e1) = (u0.edge_row, u1.edge_row);
                if e0 == e1 {
                    continue;
                }
                let (known, target) =
                    match (flipped_edges.contains(&e0), flipped_edges.contains(&e1)) {
                        (true, false) => (e0, e1),
                        (false, true) => (e1, e0),
                        _ => continue,
                    };
                let (Some(&r_known), Some(&r_target)) =
                    (active_ranges.get(&known), active_ranges.get(&target))
                else {
                    continue;
                };
                let (Some(c_known), Some(c_target)) = (edge_circle(known), edge_circle(target))
                else {
                    continue;
                };
                if !same_circle_carrier(c_known, c_target)
                    || !same_distinct_unordered_pair(endpoints(known), endpoints(target))
                {
                    continue;
                }
                if circles_overlap_same_arc(c_known, r_known, c_target, r_target) {
                    if let Some(comp) =
                        complementary_circle_range(c_known, r_known, c_target, endpoints(target))
                    {
                        let ordered = [comp[0].min(comp[1]), comp[0].max(comp[1])];
                        active_ranges.insert(target, ordered);
                        recovered.insert(target, Some(comp));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Post-attachment winding repair on single-wire `Cylinder` and `Cone` faces in
/// the assembled `CadIr`, after `bind_journal_standard_surfaces` and
/// `weighted_edges::attach` have resolved freeform surfaces and `BSpline` edges.
pub(in crate::families::standard::decode) fn repair_ir_revolution_winding(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut cadmpeg_ir::AnnotationBuilder<
        impl cadmpeg_ir::annotations::AnnotationStorage,
    >,
) -> Result<usize, cadmpeg_core::CodecError> {
    use cadmpeg_ir::topology::Sense;
    use std::f64::consts::{PI, TAU};

    struct IrFlip {
        edge_idx: usize,
        flipped_range: [f64; 2],
        flipped_du: f64,
        flipped_pts: Vec<[f64; 2]>,
    }

    struct IrLoopEdge {
        edge_idx: usize,
        du: f64,
        pts: Vec<[f64; 2]>,
        flippable: Option<IrFlip>,
    }

    let surfaces: HashMap<_, _> = ir
        .geometry
        .surfaces
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.clone(), i))
        .collect();
    let curves: HashMap<_, _> = ir
        .geometry
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.clone(), i))
        .collect();
    let loops: HashMap<_, _> = ir
        .geometry
        .loops
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.clone(), i))
        .collect();
    let coedges: HashMap<_, _> = ir
        .geometry
        .coedges
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.clone(), i))
        .collect();
    let edges: HashMap<_, _> = ir
        .geometry
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i))
        .collect();
    let point_pos: HashMap<_, _> = ir
        .geometry
        .points
        .iter()
        .map(|p| (p.id.clone(), p.position().get()))
        .collect();
    let vertex_pos: HashMap<_, _> = ir
        .geometry
        .vertices
        .iter()
        .filter_map(|v| Some((v.id.clone(), *point_pos.get(&v.point)?)))
        .collect();

    let mut coedges_by_edge: Vec<Vec<usize>> = vec![Vec::new(); ir.geometry.edges.len()];
    for (ci, coedge) in ir.geometry.coedges.iter().enumerate() {
        if let Some(&ei) = edges.get(&coedge.edge) {
            coedges_by_edge[ei].push(ci);
        }
    }

    let mut flipped_edges = BTreeMap::<usize, [f64; 2]>::new();

    for face in &ir.geometry.faces {
        let Some(boundary) = face.loops.boundary().filter(|loops| loops.len() == 1) else {
            continue;
        };
        let Some(loop_id) = boundary.into_iter().next() else {
            continue;
        };
        let Some(&loop_idx) = loops.get(loop_id) else {
            continue;
        };
        let Some(&surf_idx) = surfaces.get(&face.surface) else {
            continue;
        };
        let (surf_origin, surf_axis, is_cylinder) = match &ir.geometry.surfaces[surf_idx].geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(c)) => {
                (c.origin().get(), *c.frame().axis().as_raw(), true)
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(c)) => {
                (c.origin().get(), *c.frame().axis().as_raw(), false)
            }
            _ => continue,
        };
        let ref_dir = cadmpeg_ir::geometry::derive_reference_direction(surf_axis);
        let tan_dir = surf_axis.cross(ref_dir);
        let uv_of = |p: Point3| -> Option<[f64; 2]> {
            let offset = p.vector_from(surf_origin);
            let v = offset.dot(surf_axis);
            let radial = offset - surf_axis.scale(v);
            if radial.norm() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
                return None;
            }
            let u = radial.dot(tan_dir).atan2(radial.dot(ref_dir));
            (u.is_finite() && v.is_finite()).then_some([u, v])
        };

        let loop_coedges = ir.geometry.loops[loop_idx].coedges();
        if loop_coedges.len() < 3 {
            continue;
        }

        let mut loop_edges: Vec<IrLoopEdge> = Vec::with_capacity(loop_coedges.len());
        let mut complete = true;
        for coedge_id in loop_coedges {
            let Some(&ci) = coedges.get(coedge_id) else {
                complete = false;
                break;
            };
            let coedge = &ir.geometry.coedges[ci];
            let Some(&ei) = edges.get(&coedge.edge) else {
                complete = false;
                break;
            };
            if flipped_edges.contains_key(&ei) {
                complete = false;
                break;
            }
            let edge = &ir.geometry.edges[ei];
            let Some(ends) = &edge.ends else {
                complete = false;
                break;
            };
            let (v_start, v_end) = if coedge.sense == Sense::Reversed {
                (&ends.end, &ends.start)
            } else {
                (&ends.start, &ends.end)
            };
            let (Some(&p_start), Some(&p_end)) = (vertex_pos.get(v_start), vertex_pos.get(v_end))
            else {
                complete = false;
                break;
            };
            let (Some(uv0), Some(uv1)) = (uv_of(p_start), uv_of(p_end)) else {
                complete = false;
                break;
            };
            let Some(curve) = edge
                .curve()
                .and_then(|id| curves.get(id))
                .and_then(|&idx| ir.geometry.curves.get(idx))
                .map(|c| &c.geometry)
            else {
                complete = false;
                break;
            };
            let solved = match curve {
                CurveGeometry::Solved(s) => Some(s),
                CurveGeometry::Procedural { cache: Some(s), .. } => Some(s),
                CurveGeometry::Procedural { cache: None, .. } => None,
            };
            match solved {
                Some(SolvedCurveGeometry::Line(_)) => {
                    let du = (uv1[0] - uv0[0] + PI).rem_euclid(TAU) - PI;
                    loop_edges.push(IrLoopEdge {
                        edge_idx: ei,
                        du,
                        pts: vec![[0.0, uv0[1]], [du, uv1[1]]],
                        flippable: None,
                    });
                }
                Some(SolvedCurveGeometry::Circle(circle)) => {
                    let circle_angle = |pt: Point3| -> Option<f64> {
                        let reference = *circle.frame().reference().as_raw();
                        let tangent = circle.frame().axis().as_raw().cross(reference);
                        let offset = pt.vector_from(circle.center().get());
                        let u = offset.dot(reference);
                        let v = offset.dot(tangent);
                        (u.hypot(v) > ANALYTIC_CURVE_ENDPOINT_TOLERANCE).then(|| v.atan2(u))
                    };
                    let default_range = edge
                        .param_range()
                        .map(cadmpeg_ir::units::FiniteVector::get)
                        .or_else(|| {
                            let p_edge_start = *vertex_pos.get(&ends.start)?;
                            let p_edge_end = *vertex_pos.get(&ends.end)?;
                            if p_edge_start.distance(p_edge_end)
                                <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                            {
                                return None;
                            }
                            let a0 = circle_angle(p_edge_start)?;
                            let a1 = circle_angle(p_edge_end)?;
                            let sweep = (a1 - a0).rem_euclid(TAU);
                            (sweep > 1.0e-5 && sweep < TAU - 1.0e-5).then_some([a0, a0 + sweep])
                        });
                    let Some([lo, hi]) = default_range else {
                        complete = false;
                        break;
                    };
                    let sweep = hi - lo;
                    if sweep <= 1.0e-5
                        && p_start.distance(p_end) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                    {
                        loop_edges.push(IrLoopEdge {
                            edge_idx: ei,
                            du: 0.0,
                            pts: vec![[0.0, uv0[1]]],
                            flippable: None,
                        });
                        continue;
                    }
                    let c_axis = *circle.frame().axis().as_raw();
                    let coaxial = c_axis.cross(surf_axis).norm() <= AXIS_PARALLEL_TOLERANCE
                        && circle
                            .center()
                            .get()
                            .vector_from(surf_origin)
                            .cross(surf_axis)
                            .norm()
                            <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE;
                    if coaxial
                        && p_start.distance(p_end) > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                        && sweep > 1.0e-5
                        && sweep < TAU - 1.0e-5
                    {
                        let p_lo = circle_point_at(circle, lo);
                        let p_hi = circle_point_at(circle, hi);
                        let forward_on_curve = p_start.distance(p_lo) <= p_start.distance(p_hi);
                        let axis_sign = if c_axis.dot(surf_axis) >= 0.0 {
                            1.0
                        } else {
                            -1.0
                        };
                        let dir_sign = if forward_on_curve { 1.0 } else { -1.0 };
                        let du = axis_sign * dir_sign * sweep;
                        let flipped_du = if du > 0.0 { du - TAU } else { du + TAU };
                        let fl_start = hi.rem_euclid(TAU);
                        let fl_range = [fl_start, fl_start + (TAU - sweep)];
                        loop_edges.push(IrLoopEdge {
                            edge_idx: ei,
                            du,
                            pts: vec![[0.0, uv0[1]], [du, uv1[1]]],
                            flippable: Some(IrFlip {
                                edge_idx: ei,
                                flipped_range: fl_range,
                                flipped_du,
                                flipped_pts: vec![[0.0, uv0[1]], [flipped_du, uv1[1]]],
                            }),
                        });
                    } else {
                        let Some((du, pts)) =
                            sample_curve_uv(ctx, curve, [lo, hi], p_start, &uv_of)?
                        else {
                            complete = false;
                            break;
                        };
                        loop_edges.push(IrLoopEdge {
                            edge_idx: ei,
                            du,
                            pts,
                            flippable: None,
                        });
                    }
                }
                Some(SolvedCurveGeometry::Ellipse(_)) => {
                    let default_range = edge
                        .param_range()
                        .map(cadmpeg_ir::units::FiniteVector::get)
                        .or_else(|| {
                            let p_edge_start = *vertex_pos.get(&ends.start)?;
                            let p_edge_end = *vertex_pos.get(&ends.end)?;
                            if p_edge_start.distance(p_edge_end)
                                <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                            {
                                return None;
                            }
                            let a0 = standard_analytic_curve_angle(curve, p_edge_start)?;
                            let a1 = standard_analytic_curve_angle(curve, p_edge_end)?;
                            let sweep = (a1 - a0).rem_euclid(TAU);
                            (sweep > 1.0e-5 && sweep < TAU - 1.0e-5).then_some([a0, a0 + sweep])
                        });
                    let Some([lo, hi]) = default_range else {
                        complete = false;
                        break;
                    };
                    let sweep = hi - lo;
                    let Some((du, pts)) = sample_curve_uv(ctx, curve, [lo, hi], p_start, &uv_of)?
                    else {
                        complete = false;
                        break;
                    };
                    let flippable = if is_cylinder
                        && p_start.distance(p_end) > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                        && sweep > 1.0e-5
                        && sweep < TAU - 1.0e-5
                    {
                        let fl_start = hi.rem_euclid(TAU);
                        let fl_range = [fl_start, fl_start + (TAU - sweep)];
                        sample_curve_uv(ctx, curve, fl_range, p_start, &uv_of)?.map(
                            |(flipped_du, flipped_pts)| IrFlip {
                                edge_idx: ei,
                                flipped_range: fl_range,
                                flipped_du,
                                flipped_pts,
                            },
                        )
                    } else {
                        None
                    };
                    loop_edges.push(IrLoopEdge {
                        edge_idx: ei,
                        du,
                        pts,
                        flippable,
                    });
                }
                Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    let [lo, hi] = edge.param_range().map_or_else(
                        || nurbs.full_knot_endpoints().endpoints(),
                        cadmpeg_ir::units::FiniteVector::get,
                    );
                    let Some((du, pts)) = sample_curve_uv(ctx, curve, [lo, hi], p_start, &uv_of)?
                    else {
                        complete = false;
                        break;
                    };
                    loop_edges.push(IrLoopEdge {
                        edge_idx: ei,
                        du,
                        pts,
                        flippable: None,
                    });
                }
                _ => {
                    complete = false;
                    break;
                }
            }
        }
        if !complete {
            continue;
        }

        let total_du: f64 = loop_edges.iter().map(|e| e.du).sum();
        let turns = (total_du / TAU).round();
        if turns.abs() < 0.5 || (total_du - turns * TAU).abs() > 0.12 {
            continue;
        }
        let need: i32 = if (turns - 1.0).abs() < 0.1 {
            1
        } else if (turns + 1.0).abs() < 0.1 {
            -1
        } else if (turns - 2.0).abs() < 0.1 {
            2
        } else if (turns + 2.0).abs() < 0.1 {
            -2
        } else {
            continue;
        };

        let want_delta = if need > 0 { -TAU } else { TAU };
        let candidates: Vec<usize> = loop_edges
            .iter()
            .enumerate()
            .filter_map(|(idx, e)| {
                let fl = e.flippable.as_ref()?;
                let delta = fl.flipped_du - e.du;
                ((delta - want_delta).abs() <= 0.18).then_some(idx)
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }

        let eval_ir = |flips: &[usize]| -> UvLoopEval {
            let adapted: Vec<LoopEdgeUv> = loop_edges
                .iter()
                .map(|e| LoopEdgeUv {
                    edge_row: e.edge_idx,
                    du: e.du,
                    pts: e.pts.clone(),
                    flippable: e.flippable.as_ref().map(|fl| FlipCandidate {
                        flipped_directed: fl.flipped_range,
                        flipped_du: fl.flipped_du,
                        flipped_pts: fl.flipped_pts.clone(),
                        orig_box_gap: 0.0,
                        flipped_box_gap: 0.0,
                    }),
                })
                .collect();
            evaluate_uv_loop(&adapted, flips)
        };

        let chosen: Option<Vec<usize>> = if need.abs() == 1 {
            let mut valid = Vec::new();
            for &ci in &candidates {
                let ev = eval_ir(&[ci]);
                if !ev.has_crossing && !ev.has_overlap && ev.u_span <= TAU + 2.0e-2 {
                    valid.push((ci, ev.u_span));
                }
            }
            if valid.is_empty() {
                None
            } else {
                valid.sort_by(|a, b| a.1.total_cmp(&b.1));
                Some(vec![valid[0].0])
            }
        } else if need.abs() == 2 && candidates.len() <= 12 {
            let mut valid = Vec::new();
            for a in 0..candidates.len() {
                for b in (a + 1)..candidates.len() {
                    let pair = [candidates[a], candidates[b]];
                    if loop_edges[pair[0]].edge_idx == loop_edges[pair[1]].edge_idx {
                        continue;
                    }
                    let ev = eval_ir(&pair);
                    if !ev.has_crossing && !ev.has_overlap && ev.u_span <= TAU + 2.0e-2 {
                        valid.push((pair, ev.u_span));
                    }
                }
            }
            if valid.is_empty() {
                None
            } else {
                valid.sort_by(|a, b| a.1.total_cmp(&b.1));
                Some(valid[0].0.to_vec())
            }
        } else {
            None
        };

        if let Some(indices) = chosen {
            for idx in indices {
                if let Some(fl) = &loop_edges[idx].flippable {
                    flipped_edges.insert(fl.edge_idx, fl.flipped_range);
                }
            }
        }
    }

    let mut applied = 0usize;
    let mut removed_pcurves = BTreeSet::new();
    for (ei, range) in flipped_edges {
        let ordered = crate::assemble::ordered_range(range);
        let curve_geom = ir.geometry.edges[ei]
            .curve()
            .and_then(|id| curves.get(id))
            .and_then(|&idx| ir.geometry.curves.get(idx))
            .map(|c| &c.geometry);
        let start_pt_at_lo = match curve_geom {
            Some(c) => {
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point(ctx, c, ordered[0]),
                )?)?
                .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
            }
            None => None,
        };
        let edge = &mut ir.geometry.edges[ei];
        let had_range = edge.param_range().is_some();
        let Ok(carrier) =
            cadmpeg_ir::topology::EdgeCarrier::new(edge.curve().cloned(), Some(ordered))
        else {
            continue;
        };
        edge.carrier = carrier;
        if !had_range {
            annotations.derived(&edge.id, "carrier.param_range")?;
        }
        let need_swap = match (&edge.ends, start_pt_at_lo) {
            (Some(ends), Some(p_lo)) => {
                match (vertex_pos.get(&ends.start), vertex_pos.get(&ends.end)) {
                    (Some(&ps), Some(&pe)) => p_lo.distance(pe) < p_lo.distance(ps),
                    _ => true,
                }
            }
            _ => true,
        };
        if need_swap {
            if let Some(ends) = &mut edge.ends {
                std::mem::swap(&mut ends.start, &mut ends.end);
            }
        }
        for &ci in &coedges_by_edge[ei] {
            let coedge = &mut ir.geometry.coedges[ci];
            if need_swap {
                coedge.sense = match coedge.sense {
                    Sense::Forward => Sense::Reversed,
                    Sense::Reversed => Sense::Forward,
                };
            }
            coedge.use_curve = None;
            for usage in coedge.pcurves.drain(..) {
                removed_pcurves.insert(usage.pcurve);
            }
        }
        applied += 1;
    }

    if !removed_pcurves.is_empty() {
        let mut still_used = BTreeSet::new();
        for coedge in &ir.geometry.coedges {
            for usage in &coedge.pcurves {
                if removed_pcurves.contains(&usage.pcurve) {
                    still_used.insert(usage.pcurve.clone());
                }
            }
        }
        for id in &removed_pcurves {
            if !still_used.contains(id) {
                annotations.remove_entity(id.as_str());
            }
        }
        ir.geometry
            .pcurves
            .retain(|p| !removed_pcurves.contains(&p.id) || still_used.contains(&p.id));
    }

    Ok(applied)
}

fn circle_point_at(circle: &CircleCurve, angle: f64) -> Point3 {
    let center = circle.center().get();
    let reference = *circle.frame().reference().as_raw();
    let tangent = circle.frame().axis().as_raw().cross(reference);
    let r = circle.radius().get();
    Point3::new(
        center.x + r * (angle.cos() * reference.x + angle.sin() * tangent.x),
        center.y + r * (angle.cos() * reference.y + angle.sin() * tangent.y),
        center.z + r * (angle.cos() * reference.z + angle.sin() * tangent.z),
    )
}

fn ellipse_directed_arcs(
    ellipse: &EllipseCurve,
    ends: [Point3; 2],
) -> Option<([f64; 2], [f64; 2])> {
    let geom = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(*ellipse));
    let a0 = standard_analytic_curve_angle(&geom, ends[0])?;
    let a1 = standard_analytic_curve_angle(&geom, ends[1])?;
    let sweep_pos = (a1 - a0).rem_euclid(std::f64::consts::TAU);
    let sweep_neg = (a0 - a1).rem_euclid(std::f64::consts::TAU);
    if sweep_pos <= EPS_STANDARD_DECODE_GEOMETRY || sweep_neg <= EPS_STANDARD_DECODE_GEOMETRY {
        return None;
    }
    let pos = crate::nurbs::canonical_periodic_range([a0, a0 + sweep_pos])?;
    let neg_ordered = crate::nurbs::canonical_periodic_range([a1, a1 + sweep_neg])?;
    Some((pos, [neg_ordered[1], neg_ordered[0]]))
}

fn circle_bounds_excess(
    circle: &CircleCurve,
    ordered: [f64; 2],
    bounds: &[StandardFaceBounds],
) -> f64 {
    let center = circle.center().get();
    let r = circle.radius().get();
    let reference = *circle.frame().reference().as_raw();
    let tangent = circle.frame().axis().as_raw().cross(reference);
    bounds
        .iter()
        .map(|b| {
            arc_bounds::branch_bounds_gap(center, reference.scale(r), tangent.scale(r), ordered, b)
                .0
        })
        .fold(0.0_f64, f64::max)
}

fn ellipse_bounds_excess(
    ellipse: &EllipseCurve,
    ordered: [f64; 2],
    bounds: &[StandardFaceBounds],
) -> f64 {
    let center = ellipse.center().get();
    let reference = *ellipse.frame().reference().as_raw();
    let tangent = ellipse.frame().axis().as_raw().cross(reference);
    let cosine = reference.scale(ellipse.major_radius().get());
    let sine = tangent.scale(ellipse.minor_radius().get());
    bounds
        .iter()
        .map(|b| arc_bounds::branch_bounds_gap(center, cosine, sine, ordered, b).0)
        .fold(0.0_f64, f64::max)
}

type SampledCurveUv = (f64, Vec<[f64; 2]>);

fn sample_curve_uv(
    ctx: &DecodeContext<'_>,
    curve: &CurveGeometry,
    range: [f64; 2],
    p_start: Point3,
    uv_of: &impl Fn(Point3) -> Option<[f64; 2]>,
) -> Result<Option<SampledCurveUv>, cadmpeg_core::CodecError> {
    use std::f64::consts::{PI, TAU};
    const SAMPLES: u32 = 16;

    let Some(p_lo) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
        cadmpeg_ir::eval::decode::curve_point(ctx, curve, range[0]),
    )?)?
    else {
        return Ok(None);
    };
    let Some(p_hi) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
        cadmpeg_ir::eval::decode::curve_point(ctx, curve, range[1]),
    )?)?
    else {
        return Ok(None);
    };
    let (t0, t1) = if p_start.distance(p_lo.get()) <= p_start.distance(p_hi.get()) {
        (range[0], range[1])
    } else {
        (range[1], range[0])
    };
    let mut pts = Vec::with_capacity(usize::try_from(SAMPLES).unwrap_or(16) + 1);
    let mut prev_u: Option<f64> = None;
    let mut acc_u = 0.0_f64;
    for step in 0..=SAMPLES {
        let frac = f64::from(step) / f64::from(SAMPLES);
        let t = t0 + (t1 - t0) * frac;
        let Some(p) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::curve_point(ctx, curve, t),
            )?)?
        else {
            return Ok(None);
        };
        let Some([u, v]) = uv_of(p.get()) else {
            return Ok(None);
        };
        if let Some(pu) = prev_u {
            let step_du = (u - pu + PI).rem_euclid(TAU) - PI;
            acc_u += step_du;
        }
        prev_u = Some(u);
        pts.push([acc_u, v]);
    }
    Ok(Some((acc_u, pts)))
}

struct UvLoopEval {
    u_span: f64,
    has_crossing: bool,
    has_overlap: bool,
}

fn evaluate_uv_loop(edges: &[LoopEdgeUv], flip_indices: &[usize]) -> UvLoopEval {
    let mut cur_u = 0.0_f64;
    let mut min_u = 0.0_f64;
    let mut max_u = 0.0_f64;
    let mut edge_polys: Vec<Vec<[f64; 2]>> = Vec::with_capacity(edges.len());

    for (idx, edge) in edges.iter().enumerate() {
        let flipped = flip_indices.contains(&idx);
        let (du, local_pts) = match (&edge.flippable, flipped) {
            (Some(fl), true) => (fl.flipped_du, fl.flipped_pts.as_slice()),
            _ => (edge.du, edge.pts.as_slice()),
        };
        let mut poly = Vec::with_capacity(local_pts.len());
        for &[lu, lv] in local_pts {
            let u = cur_u + lu;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            poly.push([u, lv]);
        }
        cur_u += du;
        edge_polys.push(poly);
    }

    let n = edge_polys.len();
    let mut has_crossing = false;
    let mut has_overlap = false;
    for i in 0..n {
        for j in (i + 1)..n {
            let adj = j == i + 1 || (i == 0 && j + 1 == n);
            let pi = &edge_polys[i];
            let pj = &edge_polys[j];
            // Check co-linear horizontal overlap at same v.
            if pi.len() == 2
                && pj.len() == 2
                && (pi[0][1] - pi[1][1]).abs() <= 1.0e-3
                && (pj[0][1] - pj[1][1]).abs() <= 1.0e-3
                && (pi[0][1] - pj[0][1]).abs() <= 1.0e-3
            {
                let lo_i = pi[0][0].min(pi[1][0]);
                let hi_i = pi[0][0].max(pi[1][0]);
                let lo_j = pj[0][0].min(pj[1][0]);
                let hi_j = pj[0][0].max(pj[1][0]);
                if hi_i.min(hi_j) - lo_i.max(lo_j) > 1.0e-3 {
                    has_overlap = true;
                }
            }
            if adj {
                continue;
            }
            for si in pi.windows(2) {
                for sj in pj.windows(2) {
                    if segments_properly_cross_2d(si[0], si[1], sj[0], sj[1]) {
                        has_crossing = true;
                    }
                }
            }
        }
    }

    UvLoopEval {
        u_span: max_u - min_u,
        has_crossing,
        has_overlap,
    }
}

fn segments_properly_cross_2d(p0: [f64; 2], p1: [f64; 2], q0: [f64; 2], q1: [f64; 2]) -> bool {
    let seg_p = [p1[0] - p0[0], p1[1] - p0[1]];
    let seg_q = [q1[0] - q0[0], q1[1] - q0[1]];
    let den = seg_p[0] * seg_q[1] - seg_p[1] * seg_q[0];
    if den.abs() <= SEGMENT_CROSSING_DENOM_TOLERANCE {
        return false;
    }
    let delta = [q0[0] - p0[0], q0[1] - p0[1]];
    let t_param = (delta[0] * seg_q[1] - delta[1] * seg_q[0]) / den;
    let u_param = (delta[0] * seg_p[1] - delta[1] * seg_p[0]) / den;
    t_param > 1.0e-3 && t_param < 1.0 - 1.0e-3 && u_param > 1.0e-3 && u_param < 1.0 - 1.0e-3
}

fn merge_proposal(proposals: &mut BTreeMap<usize, Option<[f64; 2]>>, row: usize, range: [f64; 2]) {
    proposals
        .entry(row)
        .and_modify(|prior| {
            if prior.is_some_and(|prior| {
                prior
                    .into_iter()
                    .zip(range)
                    .any(|(a, b)| (a - b).abs() > EPS_STANDARD_DECODE_GEOMETRY)
            }) {
                *prior = None;
            }
        })
        .or_insert(Some(range));
}

fn surface_revolution_axis(
    surface: &SurfaceGeometry,
) -> Option<(Point3, Vector3, Option<&CylinderSurface>)> {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(c)) => {
            Some((c.origin().get(), *c.frame().axis().as_raw(), Some(c)))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(c)) => {
            Some((c.origin().get(), *c.frame().axis().as_raw(), None))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(t)) => {
            Some((t.center().get(), *t.frame().axis().as_raw(), None))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(s)) => {
            Some((s.center().get(), *s.frame().axis().as_raw(), None))
        }
        _ => None,
    }
}

fn collect_loop_witness_points<'a>(
    ctx: &DecodeContext<'_>,
    coedges: &[crate::families::standard::topology::CoedgeUse],
    active_ranges: &BTreeMap<usize, [f64; 2]>,
    edge_curve_geom: &impl Fn(usize) -> Option<&'a CurveGeometry>,
    endpoints: &impl Fn(usize) -> [Point3; 2],
) -> Result<Vec<Point3>, cadmpeg_core::CodecError> {
    let mut points = Vec::new();
    let mut seen_edges = BTreeSet::new();
    for cu in coedges {
        let row = cu.edge_row;
        if !seen_edges.insert(row) {
            continue;
        }
        let ends = endpoints(row);
        points.push(ends[0]);
        points.push(ends[1]);
        let Some(&[lo, hi]) = active_ranges.get(&row) else {
            continue;
        };
        let Some(curve) = edge_curve_geom(row) else {
            continue;
        };
        if matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Line(_))) {
            continue;
        }
        if let Some(mid) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::curve_point(ctx, curve, lo.midpoint(hi)),
            )?)?
        {
            points.push(mid.get());
        }
    }
    Ok(points)
}

fn circle_directed_arcs(circle: &CircleCurve, ends: [Point3; 2]) -> Option<([f64; 2], [f64; 2])> {
    let center = circle.center().get();
    let reference = *circle.frame().reference().as_raw();
    let tangent = circle.frame().axis().as_raw().cross(reference);
    let angle = |p: Point3| {
        let v = p.vector_from(center);
        v.dot(tangent).atan2(v.dot(reference))
    };
    let a0 = angle(ends[0]);
    let a1 = angle(ends[1]);
    if !a0.is_finite() || !a1.is_finite() {
        return None;
    }
    let sweep_pos = (a1 - a0).rem_euclid(std::f64::consts::TAU);
    let sweep_neg = (a0 - a1).rem_euclid(std::f64::consts::TAU);
    if sweep_pos <= EPS_STANDARD_DECODE_GEOMETRY || sweep_neg <= EPS_STANDARD_DECODE_GEOMETRY {
        return None;
    }
    let pos = crate::nurbs::canonical_periodic_range([a0, a0 + sweep_pos])?;
    let neg_ordered = crate::nurbs::canonical_periodic_range([a1, a1 + sweep_neg])?;
    Some((pos, [neg_ordered[1], neg_ordered[0]]))
}

fn angle_strictly_in_periodic_arc(angle: f64, range: [f64; 2]) -> bool {
    let lo = range[0].min(range[1]);
    let hi = range[0].max(range[1]);
    let span = hi - lo;
    let delta = (angle - lo).rem_euclid(std::f64::consts::TAU);
    delta > AZIMUTH_ENDPOINT_MARGIN_RAD && delta < span - AZIMUTH_ENDPOINT_MARGIN_RAD
}

fn circle_range_from_revolution_witnesses(
    surf_origin: Point3,
    surf_axis: Vector3,
    circle: &CircleCurve,
    ends: [Point3; 2],
    witnesses: &[Point3],
) -> Option<[f64; 2]> {
    let c_axis = *circle.frame().axis().as_raw();
    if c_axis.cross(surf_axis).norm() > AXIS_PARALLEL_TOLERANCE {
        return None;
    }
    let center_offset = circle.center().get().vector_from(surf_origin);
    if center_offset.cross(surf_axis).norm() > ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
        return None;
    }
    let (pos, neg) = circle_directed_arcs(circle, ends)?;
    let reference = *circle.frame().reference().as_raw();
    let tangent = c_axis.cross(reference);
    let mut saw_pos = false;
    let mut saw_neg = false;
    for &w in witnesses {
        let offset = w.vector_from(surf_origin);
        let radial = offset - surf_axis.scale(offset.dot(surf_axis));
        if radial.norm() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
            continue;
        }
        let theta = radial.dot(tangent).atan2(radial.dot(reference));
        if !theta.is_finite() {
            continue;
        }
        let in_pos = angle_strictly_in_periodic_arc(theta, pos);
        let in_neg = angle_strictly_in_periodic_arc(theta, neg);
        if in_pos && !in_neg {
            saw_pos = true;
        } else if in_neg && !in_pos {
            saw_neg = true;
        }
    }
    match (saw_pos, saw_neg) {
        (true, false) => Some(pos),
        (false, true) => Some(neg),
        _ => None,
    }
}

fn ellipse_range_from_cylinder_witnesses(
    cylinder: &CylinderSurface,
    ellipse: &EllipseCurve,
    ends: [Point3; 2],
    witnesses: &[Point3],
) -> Option<[f64; 2]> {
    let cyl_axis = *cylinder.frame().axis().as_raw();
    let cyl_origin = cylinder.origin().get();
    let cyl_radius = cylinder.radius().get();
    let normal = *ellipse.frame().axis().as_raw();
    let axis_dot_normal = cyl_axis.dot(normal);
    if !axis_dot_normal.is_finite() || axis_dot_normal.abs() <= CYLINDER_PLANE_CONIC_TOLERANCE {
        return None;
    }
    let geom = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(*ellipse));
    let a0 = standard_analytic_curve_angle(&geom, ends[0])?;
    let a1 = standard_analytic_curve_angle(&geom, ends[1])?;
    let sweep_pos = (a1 - a0).rem_euclid(std::f64::consts::TAU);
    let sweep_neg = (a0 - a1).rem_euclid(std::f64::consts::TAU);
    if sweep_pos <= EPS_STANDARD_DECODE_GEOMETRY || sweep_neg <= EPS_STANDARD_DECODE_GEOMETRY {
        return None;
    }
    let pos = crate::nurbs::canonical_periodic_range([a0, a0 + sweep_pos])?;
    let neg_ordered = crate::nurbs::canonical_periodic_range([a1, a1 + sweep_neg])?;
    let neg = [neg_ordered[1], neg_ordered[0]];

    let mut saw_pos = false;
    let mut saw_neg = false;
    for &w in witnesses {
        let offset = w.vector_from(cyl_origin);
        let radial = offset - cyl_axis.scale(offset.dot(cyl_axis));
        let norm = radial.norm();
        if !norm.is_finite() || norm <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
            continue;
        }
        let radial_point = cyl_origin.translated(radial.scale(cyl_radius / norm), 1.0);
        let axial = ellipse.center().get().vector_from(radial_point).dot(normal) / axis_dot_normal;
        if !axial.is_finite() {
            continue;
        }
        let projected = radial_point.translated(cyl_axis, axial);
        let Some(theta) = standard_analytic_curve_angle(&geom, projected) else {
            continue;
        };
        let in_pos = angle_strictly_in_periodic_arc(theta, pos);
        let in_neg = angle_strictly_in_periodic_arc(theta, neg);
        if in_pos && !in_neg {
            saw_pos = true;
        } else if in_neg && !in_pos {
            saw_neg = true;
        }
    }
    match (saw_pos, saw_neg) {
        (true, false) => Some(pos),
        (false, true) => Some(neg),
        _ => None,
    }
}

fn same_circle_carrier(left: &CircleCurve, right: &CircleCurve) -> bool {
    left.center().get().distance(right.center().get()) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
        && (left.radius().get() - right.radius().get()).abs() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE
        && left
            .frame()
            .axis()
            .as_raw()
            .cross(*right.frame().axis().as_raw())
            .norm()
            <= AXIS_PARALLEL_TOLERANCE
}

fn same_distinct_unordered_pair(a: [Point3; 2], b: [Point3; 2]) -> bool {
    let tol = ANALYTIC_CURVE_ENDPOINT_TOLERANCE;
    if a[0].distance(a[1]) <= tol || b[0].distance(b[1]) <= tol {
        return false;
    }
    (a[0].distance(b[0]) <= tol && a[1].distance(b[1]) <= tol)
        || (a[0].distance(b[1]) <= tol && a[1].distance(b[0]) <= tol)
}

fn circles_overlap_same_arc(
    c0: &CircleCurve,
    r0: [f64; 2],
    c1: &CircleCurve,
    r1: [f64; 2],
) -> bool {
    let mid0 = r0[0].midpoint(r0[1]);
    let ref0 = *c0.frame().reference().as_raw();
    let tan0 = c0.frame().axis().as_raw().cross(ref0);
    let mid_dir0 = ref0.scale(mid0.cos()) + tan0.scale(mid0.sin());

    let ref1 = *c1.frame().reference().as_raw();
    let tan1 = c1.frame().axis().as_raw().cross(ref1);
    let theta1 = mid_dir0.dot(tan1).atan2(mid_dir0.dot(ref1));
    theta1.is_finite() && angle_strictly_in_periodic_arc(theta1, r1)
}

fn complementary_circle_range(
    known: &CircleCurve,
    known_range: [f64; 2],
    target: &CircleCurve,
    target_ends: [Point3; 2],
) -> Option<[f64; 2]> {
    let mid_angle = known_range[0].midpoint(known_range[1]);
    let known_ref = *known.frame().reference().as_raw();
    let known_tan = known.frame().axis().as_raw().cross(known_ref);
    let mid_dir = known_ref.scale(mid_angle.cos()) + known_tan.scale(mid_angle.sin());

    let (pos, neg) = circle_directed_arcs(target, target_ends)?;
    let target_ref = *target.frame().reference().as_raw();
    let target_tan = target.frame().axis().as_raw().cross(target_ref);
    let mid_theta = mid_dir.dot(target_tan).atan2(mid_dir.dot(target_ref));
    if !mid_theta.is_finite() {
        return None;
    }
    let in_pos = angle_strictly_in_periodic_arc(mid_theta, pos);
    let in_neg = angle_strictly_in_periodic_arc(mid_theta, neg);
    match (in_pos, in_neg) {
        (true, false) => Some(neg),
        (false, true) => Some(pos),
        _ => None,
    }
}

fn unambiguous_box_circle_range(
    circle: &CircleCurve,
    ends: [Point3; 2],
    bounds: &[StandardFaceBounds],
) -> Option<[f64; 2]> {
    if bounds.is_empty() {
        return None;
    }
    let (pos, neg) = circle_directed_arcs(circle, ends)?;
    let radius = circle.radius().get();
    let center = circle.center().get();
    let reference = *circle.frame().reference().as_raw();
    let tangent = circle.frame().axis().as_raw().cross(reference);
    let cosine = reference.scale(radius);
    let sine = tangent.scale(radius);

    let max_raw_gap = |ordered: [f64; 2]| -> f64 {
        bounds
            .iter()
            .map(|b| arc_bounds::branch_bounds_gap(center, cosine, sine, ordered, b).0)
            .fold(0.0_f64, f64::max)
    };
    let pos_gap = max_raw_gap(pos);
    let neg_gap = max_raw_gap([neg[1], neg[0]]);
    if !pos_gap.is_finite() || !neg_gap.is_finite() {
        return None;
    }
    let max_allowed = (BOX_MAX_EXCESS_FRACTION * radius).min(BOX_MAX_EXCESS_MM);
    let min_other = BOX_MAX_EXCESS_FRACTION * radius;
    if pos_gap <= max_allowed
        && neg_gap >= min_other
        && neg_gap >= BOX_DISAMBIGUATION_RATIO * pos_gap.max(ANALYTIC_CURVE_ENDPOINT_TOLERANCE)
    {
        return Some(pos);
    }
    if neg_gap <= max_allowed
        && pos_gap >= min_other
        && pos_gap >= BOX_DISAMBIGUATION_RATIO * neg_gap.max(ANALYTIC_CURVE_ENDPOINT_TOLERANCE)
    {
        return Some(neg);
    }
    None
}

fn unambiguous_box_ellipse_range(
    ellipse: &EllipseCurve,
    ends: [Point3; 2],
    bounds: &[StandardFaceBounds],
) -> Option<[f64; 2]> {
    if bounds.is_empty() {
        return None;
    }
    let geom = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(*ellipse));
    let a0 = standard_analytic_curve_angle(&geom, ends[0])?;
    let a1 = standard_analytic_curve_angle(&geom, ends[1])?;
    let sweep_pos = (a1 - a0).rem_euclid(std::f64::consts::TAU);
    let sweep_neg = (a0 - a1).rem_euclid(std::f64::consts::TAU);
    if sweep_pos <= EPS_STANDARD_DECODE_GEOMETRY || sweep_neg <= EPS_STANDARD_DECODE_GEOMETRY {
        return None;
    }
    let pos = crate::nurbs::canonical_periodic_range([a0, a0 + sweep_pos])?;
    let neg_ordered = crate::nurbs::canonical_periodic_range([a1, a1 + sweep_neg])?;
    let neg = [neg_ordered[1], neg_ordered[0]];

    let center = ellipse.center().get();
    let reference = *ellipse.frame().reference().as_raw();
    let tangent = ellipse.frame().axis().as_raw().cross(reference);
    let cosine = reference.scale(ellipse.major_radius().get());
    let sine = tangent.scale(ellipse.minor_radius().get());
    let scale = ellipse.minor_radius().get();

    let max_raw_gap = |ordered: [f64; 2]| -> f64 {
        bounds
            .iter()
            .map(|b| arc_bounds::branch_bounds_gap(center, cosine, sine, ordered, b).0)
            .fold(0.0_f64, f64::max)
    };
    let pos_gap = max_raw_gap(pos);
    let neg_gap = max_raw_gap(neg_ordered);
    if !pos_gap.is_finite() || !neg_gap.is_finite() {
        return None;
    }
    let max_allowed = (BOX_MAX_EXCESS_FRACTION * scale).min(BOX_MAX_EXCESS_MM);
    let min_other = BOX_MAX_EXCESS_FRACTION * scale;
    if pos_gap <= max_allowed
        && neg_gap >= min_other
        && neg_gap >= BOX_DISAMBIGUATION_RATIO * pos_gap.max(ANALYTIC_CURVE_ENDPOINT_TOLERANCE)
    {
        return Some(pos);
    }
    if neg_gap <= max_allowed
        && pos_gap >= min_other
        && pos_gap >= BOX_DISAMBIGUATION_RATIO * neg_gap.max(ANALYTIC_CURVE_ENDPOINT_TOLERANCE)
    {
        return Some(neg);
    }
    None
}

fn assign_antipodal_semicircle_pair(
    loop_face: usize,
    (e0, c0, ends0): (usize, &CircleCurve, [Point3; 2]),
    (e1, c1, ends1): (usize, &CircleCurve, [Point3; 2]),
    supports: &[StandardCurveSupport],
    face_bounds: &[Option<StandardFaceBounds>],
) -> Option<([f64; 2], [f64; 2])> {
    let (pos0, neg0) = circle_directed_arcs(c0, ends0)?;
    let other_face = |row: usize| -> Option<usize> {
        let faces = supports.get(row)?.faces;
        if faces[0] == loop_face {
            Some(faces[1])
        } else if faces[1] == loop_face {
            Some(faces[0])
        } else {
            None
        }
    };
    let pick_from_center = |circle: &CircleCurve, pos: [f64; 2], neg: [f64; 2], face: usize| {
        let b = (*face_bounds.get(face)?)?;
        let center = Point3::new(
            b.aabb_center[0].get(),
            b.aabb_center[1].get(),
            b.aabb_center[2].get(),
        );
        let offset = center.vector_from(circle.center().get());
        let axis = *circle.frame().axis().as_raw();
        let radial = offset - axis.scale(offset.dot(axis));
        if radial.norm() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
            return None;
        }
        let reference = *circle.frame().reference().as_raw();
        let tangent = axis.cross(reference);
        let theta = radial.dot(tangent).atan2(radial.dot(reference));
        let in_pos = angle_strictly_in_periodic_arc(theta, pos);
        let in_neg = angle_strictly_in_periodic_arc(theta, neg);
        match (in_pos, in_neg) {
            (true, false) => Some(pos),
            (false, true) => Some(neg),
            _ => None,
        }
    };

    let chosen0 = if let (Some(f0), Some(f1)) = (other_face(e0), other_face(e1)) {
        if f0 != f1 {
            let (pos1, neg1) = circle_directed_arcs(c1, ends1)?;
            if let (Some(r0), Some(r1)) = (
                pick_from_center(c0, pos0, neg0, f0),
                pick_from_center(c1, pos1, neg1, f1),
            ) {
                let comp1 = complementary_circle_range(
                    c0,
                    [r0[0].min(r0[1]), r0[0].max(r0[1])],
                    c1,
                    ends1,
                )?;
                if (comp1[0] - r1[0]).abs() <= EPS_STANDARD_DECODE_GEOMETRY
                    && (comp1[1] - r1[1]).abs() <= EPS_STANDARD_DECODE_GEOMETRY
                {
                    return Some((r0, r1));
                }
            }
        }
        pos0
    } else {
        pos0
    };
    let chosen1 = complementary_circle_range(
        c0,
        [chosen0[0].min(chosen0[1]), chosen0[0].max(chosen0[1])],
        c1,
        ends1,
    )?;
    Some((chosen0, chosen1))
}

fn translated_range(
    surface: &SurfaceGeometry,
    target: &CircleCurve,
    endpoints: [Point3; 2],
    witness: &CircleCurve,
    witness_endpoints: [Point3; 2],
    range: [f64; 2],
) -> Option<[f64; 2]> {
    let tolerance = ANALYTIC_CURVE_ENDPOINT_TOLERANCE;
    for circle in [target, witness] {
        let axis = standard_circle_axis_from_carrier(
            circle.center().get(),
            circle.radius().get(),
            surface,
        )?;
        if axis.as_raw().cross(*circle.frame().axis().as_raw()).norm()
            > EPS_STANDARD_DECODE_GEOMETRY
        {
            return None;
        }
    }
    if target.center().get().distance(witness.center().get()) <= tolerance {
        return None;
    }
    let target_radials = endpoints.map(|point| point.vector_from(target.center().get()));
    let witness_radials = witness_endpoints.map(|point| point.vector_from(witness.center().get()));
    let matches = |other| {
        target_radials
            .into_iter()
            .zip(other)
            .all(|(a, b)| (a - b).norm() <= tolerance)
    };
    if !matches(witness_radials) && !matches([witness_radials[1], witness_radials[0]]) {
        return None;
    }
    let witness_reference = *witness.frame().reference().as_raw();
    let witness_tangent = witness.frame().axis().as_raw().cross(witness_reference);
    let midpoint = range[0].midpoint(range[1]);
    let radial = witness_reference.scale(midpoint.cos()) + witness_tangent.scale(midpoint.sin());
    let reference = *target.frame().reference().as_raw();
    let tangent = target.frame().axis().as_raw().cross(reference);
    let angle = |v: Vector3| v.dot(tangent).atan2(v.dot(reference));
    let [start, end] = target_radials.map(angle);
    let end = witness_arc_end(start, unwrap_angle(end, start), angle(radial))?;
    if start < end {
        crate::nurbs::canonical_periodic_range([start, end])
    } else {
        crate::nurbs::canonical_periodic_range([end, start]).map(|[lo, hi]| [hi, lo])
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{PI, TAU};

    use cadmpeg_ir::geometry::analytic::CylinderSurface;
    use cadmpeg_ir::math::Vector3;

    use super::*;

    #[test]
    fn opposite_cylinder_arc_selects_the_same_semicircle() {
        let origin = Point3::new(0.0, 0.0, 0.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(origin, axis, reference, 2.0).expect("cylinder"),
        ));
        let witness = CircleCurve::try_new(origin, axis, reference, 2.0).expect("circle");
        let target = CircleCurve::try_new(Point3::new(0.0, 0.0, 3.0), axis, reference, 2.0)
            .expect("translated circle");
        let ends = [Point3::new(2.0, 0.0, 0.0), Point3::new(-2.0, 0.0, 0.0)];
        let translated = ends.map(|point| point.translated(axis, 3.0));

        assert_eq!(
            translated_range(&surface, &target, translated, &witness, ends, [0.0, PI]),
            Some([0.0, PI])
        );
        assert_eq!(
            translated_range(&surface, &target, translated, &witness, ends, [PI, TAU]),
            Some([TAU, PI])
        );
        assert_eq!(
            translated_range(
                &surface,
                &target,
                [translated[0], Point3::new(0.0, 2.0, 3.0)],
                &witness,
                ends,
                [0.0, PI],
            ),
            None
        );
    }

    #[test]
    fn complementary_circle_range_selects_the_opposite_semicircle() {
        let center = Point3::new(0.0, 0.0, 0.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        let circle = CircleCurve::try_new(center, axis, reference, 2.0).expect("circle");
        let ends = [Point3::new(2.0, 0.0, 0.0), Point3::new(-2.0, 0.0, 0.0)];
        let comp = complementary_circle_range(&circle, [0.0, PI], &circle, ends)
            .expect("complementary semicircle");
        assert!((comp[0] - TAU).abs() < 1.0e-12);
        assert!((comp[1] - PI).abs() < 1.0e-12);
    }

    #[test]
    fn revolution_witnesses_resolve_multi_edge_cylinder_semicircle() {
        let origin = Point3::new(0.0, 0.0, 0.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        let circle = CircleCurve::try_new(origin, axis, reference, 2.0).expect("circle");
        let ends = [Point3::new(2.0, 0.0, 0.0), Point3::new(-2.0, 0.0, 0.0)];
        let witnesses = [
            Point3::new(2.0, 0.0, 5.0),
            Point3::new(0.0, -2.0, 5.0),
            Point3::new(-2.0, 0.0, 5.0),
        ];
        let range = circle_range_from_revolution_witnesses(origin, axis, &circle, ends, &witnesses)
            .expect("lower semicircle selected by opposite-rim vertex");
        assert!((range[0] - TAU).abs() < 1.0e-12);
        assert!((range[1] - PI).abs() < 1.0e-12);
    }

    #[test]
    fn unambiguous_box_ellipse_range_selects_branch_inside_face_bounds() {
        use cadmpeg_ir::scalar::{FiniteReal, NonNegativeLength};

        let ellipse = EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            10.0,
            5.0,
        )
        .expect("ellipse");
        let ends = [Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 5.0, 0.0)];
        let center = [
            FiniteReal::new(5.0).expect("finite"),
            FiniteReal::new(2.5).expect("finite"),
            FiniteReal::new(0.0).expect("finite"),
        ];
        let bounds = StandardFaceBounds {
            aabb_center: center,
            aabb_half_extents: [
                NonNegativeLength::new(5.05).expect("non-negative"),
                NonNegativeLength::new(2.55).expect("non-negative"),
                NonNegativeLength::new(0.1).expect("non-negative"),
            ],
            sphere_center: center,
            sphere_radius: NonNegativeLength::new(6.0).expect("non-negative"),
        };
        let range = unambiguous_box_ellipse_range(&ellipse, ends, &[bounds])
            .expect("first-quadrant arc fits inside the face box");
        assert!(range[0].abs() < 1.0e-12);
        assert!((range[1] - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    }

    #[test]
    fn circles_overlap_same_arc_distinguishes_duplicate_from_complementary_arcs() {
        let circle = CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("circle");
        assert!(circles_overlap_same_arc(
            &circle,
            [0.0, PI],
            &circle,
            [0.0, PI]
        ));
        assert!(!circles_overlap_same_arc(
            &circle,
            [0.0, PI],
            &circle,
            [PI, TAU]
        ));
    }

    #[test]
    fn evaluate_uv_loop_rejects_overlap_and_accepts_repaired_winding() {
        let edges = vec![
            LoopEdgeUv {
                edge_row: 0,
                du: 0.0,
                pts: vec![[0.0, 0.0], [0.0, 1.0]],
                flippable: None,
            },
            LoopEdgeUv {
                edge_row: 1,
                du: 1.0,
                pts: vec![[0.0, 1.0], [1.0, 1.0]],
                flippable: Some(FlipCandidate {
                    flipped_directed: [1.0, TAU],
                    flipped_du: 1.0 - TAU,
                    flipped_pts: vec![[0.0, 1.0], [1.0 - TAU, 1.0]],
                    orig_box_gap: 0.0,
                    flipped_box_gap: 0.0,
                }),
            },
            LoopEdgeUv {
                edge_row: 2,
                du: 0.0,
                pts: vec![[0.0, 1.0], [0.0, 0.0]],
                flippable: None,
            },
            LoopEdgeUv {
                edge_row: 3,
                du: TAU - 1.0,
                pts: vec![[0.0, 0.0], [TAU - 1.0, 0.0]],
                flippable: Some(FlipCandidate {
                    flipped_directed: [0.0, 1.0],
                    flipped_du: -1.0,
                    flipped_pts: vec![[0.0, 0.0], [-1.0, 0.0]],
                    orig_box_gap: 0.0,
                    flipped_box_gap: 0.0,
                }),
            },
        ];
        let flip_short = evaluate_uv_loop(&edges, &[3]);
        assert!(!flip_short.has_crossing);
        assert!(!flip_short.has_overlap);
        assert!((flip_short.u_span - 1.0).abs() < 1.0e-12);
        let flip_long = evaluate_uv_loop(&edges, &[1]);
        assert!(!flip_long.has_crossing);
        assert!(!flip_long.has_overlap);
        assert!((flip_long.u_span - (TAU - 1.0)).abs() < 1.0e-12);
    }
}
