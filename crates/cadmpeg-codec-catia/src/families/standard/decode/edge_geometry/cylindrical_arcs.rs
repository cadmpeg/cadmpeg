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

pub(in crate::families::standard::decode) fn ranges(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    curve_indices: &HashMap<CurveId, usize>,
    supports: &[StandardCurveSupport],
    face_bounds: &[Option<StandardFaceBounds>],
    topology: &StandardTopology,
    endpoints: impl Fn(usize) -> [Point3; 2],
) -> Result<BTreeMap<usize, Option<[f64; 2]>>, cadmpeg_core::CodecError> {
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

        // Pass 3: Unambiguous face-box disambiguation for unranged circular
        // edges where tessellation box shortfall prevented exact-zero fitting.
        let mut proposals = BTreeMap::<usize, Option<[f64; 2]>>::new();
        for row in 0..ir.geometry.edges.len() {
            if active_ranges.contains_key(&row) || conflicted.contains(&row) {
                continue;
            }
            let Some(circle) = edge_circle(row) else {
                continue;
            };
            let ends = endpoints(row);
            if ends[0].distance(ends[1]) <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
                continue;
            }
            let bounds: Vec<StandardFaceBounds> = edge_incident_faces[row]
                .iter()
                .filter_map(|&f| face_bounds.get(f).and_then(|b| *b))
                .collect();
            if let Some(range) = unambiguous_box_circle_range(circle, ends, &bounds) {
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

        // Pass 4: Complementary antipodal semicircle pair breaking on 2-edge
        // circular loops where both edges remain unranged. Resolves one pair at
        // a time and re-enters Pass 1 so cylinder/cone walls propagate the
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
                let radius = c0.radius().get();
                if (ends0[0].distance(ends0[1]) - 2.0 * radius).abs()
                    > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                {
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

    Ok(recovered)
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
}
