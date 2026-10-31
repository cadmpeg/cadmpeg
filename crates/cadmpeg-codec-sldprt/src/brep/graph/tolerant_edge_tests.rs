// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use super::{attach_surface_curve, tolerant_edge_curve, Brep};
use crate::brep::sp_curve::SurfaceCurveCarrier;
use crate::brep::topology::Coedge;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{
    CurveGeometry, FitTolerance, ProceduralCurveDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::Sense;

fn ctx() -> cadmpeg_core::decode::DecodeContext<'static> {
    cadmpeg_test_support::service_decode_context()
}

fn stream() -> StreamHandle {
    StreamHandle::new(cadmpeg_ir::stream_name!("test"))
}

fn fin(attr: u16, partner: u16, edge: u16, curve: u16) -> (u16, Coedge) {
    let coedge = Coedge {
        attr,
        refs: [1, 0, 0, 0, 0, partner, edge, curve, 1],
        sense: Sense::Forward,
        offset: 0,
    };
    (attr, coedge)
}

fn bounded(attr: u16, surface: u16) -> SurfaceCurveCarrier {
    SurfaceCurveCarrier {
        attr,
        offset: 0,
        surface,
        pcurve: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
                .expect("a line pcurve"),
        ),
        parameter_range: [0.0, 1.0],
        points: [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        sweep: None,
        offset_support: None,
    }
}

/// Attach `carrier` as curve 31 of `out`, its support 200 emitted as the
/// face surfaces of `emitted`.
fn attach(out: &mut Brep, carrier: &SurfaceCurveCarrier, emitted: &HashMap<u16, u16>) {
    attach_surface_curve(
        &ctx(),
        out,
        &mut AnnotationBuilder::new(),
        carrier,
        emitted,
        31,
        &stream(),
    )
    .expect("fixture budget");
}

#[test]
fn a_tolerant_edge_reads_a_decoded_curve_of_a_fin_on_it() {
    // Curve 31 is the one that decodes.
    let decoded = |curve: u16| curve == 31;
    let coedges = HashMap::from([
        fin(10, 11, 5, 30),
        fin(11, 10, 5, 31),
        fin(12, 13, 6, 30),
        fin(13, 12, 7, 31),
        fin(14, 15, 8, 1),
    ]);
    // The canonical fin's undecoded curve gives way to its partner's.
    assert_eq!(tolerant_edge_curve(10, &coedges, decoded), Some(31));
    // A partner on another edge is ignored; an undecoded curve is kept.
    assert_eq!(tolerant_edge_curve(12, &coedges, decoded), Some(30));
    // A fin whose curve is null too gives no curve.
    assert_eq!(tolerant_edge_curve(14, &coedges, decoded), None);
}

#[test]
fn a_tolerant_edge_takes_a_stated_fin_curve_before_a_fitted_one() {
    // Curve 30, the canonical fin's, only decodes (a fitted curve on a
    // sweep); its partner's, 31, is stated on a surface.
    let coedges = HashMap::from([fin(10, 11, 5, 30), fin(11, 10, 5, 31)]);
    let stated = |curve: u16| curve == 31;
    let decodes = |curve: u16| curve == 30 || curve == 31;
    assert_eq!(
        super::tolerant_fin_curve(10, &coedges, stated, decodes),
        Some(31)
    );
    // With no stated curve the decoded one is taken, and with none
    // decoded the canonical fin's own.
    assert_eq!(
        super::tolerant_fin_curve(10, &coedges, |_| false, decodes),
        Some(30)
    );
    assert_eq!(
        super::tolerant_fin_curve(10, &coedges, |_| false, |_| false),
        Some(30)
    );
}

fn plane(id: &str) -> Surface {
    Surface {
        id: SurfaceId::mint(id).expect("identity grammar"),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("a plane"),
        )),
    }
}

/// On a spun support the curve is its fitted B-curve, with no construction.
#[test]
fn an_sp_curve_on_a_spun_surface_is_emitted_as_its_fitted_curve() {
    use crate::brep::sp_curve::SweepSupport;
    use crate::brep::sweep::SweepKind;
    // A line 10 mm off the z axis, a quarter turn about it.
    let sweep = SweepSupport {
        profile: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(10.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("a line"),
        )),
        kind: SweepKind::Spun {
            base: Point3::new(0.0, 0.0, 0.0),
            axis: cadmpeg_ir::units::SumSquaresUnitVector3::new(Vector3::new(0.0, 0.0, 1.0))
                .expect("a unit axis"),
        },
    };
    let quarter = std::f64::consts::FRAC_PI_2;
    let carrier = SurfaceCurveCarrier {
        pcurve: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(0.0, 1.0))
                .expect("a line pcurve"),
        ),
        parameter_range: [0.0, quarter],
        points: [Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0)],
        sweep: Some(sweep),
        ..bounded(31, 200)
    };
    let mut out = Brep::default();
    attach(&mut out, &carrier, &HashMap::new());
    assert!(out.procedural_curves.is_empty());
    let CurveGeometry::Solved(fitted @ SolvedCurveGeometry::Nurbs(_)) = &out.curves[0].geometry
    else {
        panic!("{:?}", out.curves[0].geometry);
    };
    for (t, expected) in [
        (0.0, Point3::new(10.0, 0.0, 0.0)),
        (
            0.5 * quarter,
            Point3::new(10.0 * 0.5_f64.sqrt(), 10.0 * 0.5_f64.sqrt(), 0.0),
        ),
        (quarter, Point3::new(0.0, 10.0, 0.0)),
    ] {
        let at = cadmpeg_ir::eval::decode::curve_point_solved(&ctx(), fitted, t)
            .expect("in the domain")
            .get();
        assert!(at.distance(expected) < 1.0e-6, "t {t}: {at:?}");
    }
}

#[test]
fn a_bounded_sp_curve_is_emitted_on_its_supports_face_surface() {
    let carrier = bounded(31, 200);
    let mut out = Brep::default();
    out.surfaces.push(plane("sldprt:brep:surf#100"));
    let emitted = HashMap::from([(200, 100)]);
    attach(&mut out, &carrier, &emitted);
    assert!(matches!(
        out.curves[0].geometry,
        CurveGeometry::Procedural { .. }
    ));
    let ProceduralCurveDefinition::SurfaceCurve {
        family: SurfaceCurveFamily::Parametric { context, .. },
    } = out.procedural_curves[0].definition()
    else {
        panic!("{:?}", out.procedural_curves[0].definition());
    };
    assert_eq!(
        context.sides()[0].surface.as_ref().map(SurfaceId::as_str),
        Some("sldprt:brep:surf#100")
    );
    assert_eq!(context.parameter_range().endpoints(), [0.0, 1.0]);

    // An alternate site's body qualifies the construction, its owner's
    // reference to it and the support it names.
    let mut qualified = Brep {
        surfaces: out.surfaces.clone(),
        curves: out.curves.clone(),
        procedural_curves: out.procedural_curves.clone(),
        ..Brep::default()
    };
    qualified
        .qualify_ids("block@1.body0")
        .expect("qualified identities");
    let construction = &qualified.procedural_curves[0];
    assert_eq!(
        construction.id.as_str(),
        "sldprt:brep:sp-curve#31@block@1.body0"
    );
    assert_eq!(
        qualified.curves[0].geometry.procedural_construction(),
        Some(&construction.id)
    );
    let ProceduralCurveDefinition::SurfaceCurve {
        family: SurfaceCurveFamily::Parametric { context, .. },
    } = construction.definition()
    else {
        panic!("{:?}", construction.definition());
    };
    assert_eq!(
        context.sides()[0].surface.as_ref().map(SurfaceId::as_str),
        Some("sldprt:brep:surf#100@block@1.body0")
    );

    // A support whose face surface is opaque leaves the curve opaque.
    let mut out = Brep::default();
    let mut opaque = plane("sldprt:brep:surf#100");
    opaque.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    out.surfaces.push(opaque);
    attach(&mut out, &carrier, &emitted);
    assert!(matches!(
        out.curves[0].geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
    ));

    // A support no face emits leaves the curve opaque.
    let mut out = Brep::default();
    attach(&mut out, &carrier, &HashMap::new());
    assert!(matches!(
        out.curves[0].geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
    ));
    assert!(out.procedural_curves.is_empty());
}

fn cylinder(id: &str) -> Surface {
    Surface {
        id: SurfaceId::mint(id).expect("identity grammar"),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                100.0,
            )
            .expect("a cylinder"),
        )),
    }
}

/// The pcurve of [`bounded`] steepened at `t = 1` into a corner, over
/// `range`.
fn cornered(range: [f64; 2]) -> SurfaceCurveCarrier {
    let poles = [[0.0, 0.0], [2.0, 0.0], [4.0, 30.0]].map(|[u, v]| Point2::new(u, v));
    let knots = vec![0.0, 0.0, 1.0, 2.0, 2.0];
    let nurbs =
        PcurveNurbs::from_lanes(1, knots, poles.to_vec(), None, false).expect("a clamped polyline");
    SurfaceCurveCarrier {
        pcurve: PcurveGeometry::Nurbs { nurbs },
        parameter_range: range,
        ..bounded(31, 200)
    }
}

/// The cache's point at `t`.
fn cache_point(cache: &SolvedCurveGeometry, t: f64) -> Point3 {
    cadmpeg_ir::eval::decode::curve_point_solved(&ctx(), cache, t)
        .expect("in the domain")
        .get()
}

/// The solved cache of a bounded `SP_CURVE` is the support through the
/// pcurve, in the pcurve's parameter over the bounded range, within the
/// fit's bound, which the construction declares; a knot of the pcurve
/// where the curve turns is a corner of the cache.
#[test]
fn a_bounded_sp_curve_carries_a_fitted_cache_in_its_own_parameter() {
    // No bisection of this range lands on the corner at `t = 1`.
    let range = [0.25, 1.5];
    let carrier = cornered(range);
    let mut out = Brep::default();
    // Another face's surface comes first: the cache is on the curve's own.
    out.surfaces.push(plane("sldprt:brep:surf#99"));
    out.surfaces.push(cylinder("sldprt:brep:surf#100"));
    let emitted = HashMap::from([(200, 100)]);
    attach(&mut out, &carrier, &emitted);
    let cache = out.curves[0].geometry.solved_cache().expect("a cache");
    let SolvedCurveGeometry::Nurbs(nurbs) = cache else {
        panic!("{cache:?}");
    };
    let knots = nurbs.knots();
    assert_eq!([knots[0], knots[knots.len() - 1]], range);
    assert_eq!(knots.iter().filter(|knot| **knot == 1.0).count(), 3);
    // A 100 mm cylinder about z through (1, 2, 3): a 2 rad arc per unit
    // of `t` up to the corner, then a helix rising 30 mm per unit.
    let exact = |t: f64| {
        let (u, v) = if t <= 1.0 {
            (2.0 * t, 0.0)
        } else {
            (2.0 * t, 30.0 * (t - 1.0))
        };
        Point3::new(1.0 + 100.0 * u.cos(), 2.0 + 100.0 * u.sin(), 3.0 + v)
    };
    let worst = (0..=4_001)
        .map(|i| range[0] + (range[1] - range[0]) * f64::from(i) / 4_001.0)
        .map(|t| cache_point(cache, t).distance(exact(t)))
        .fold(0.0, f64::max);
    assert!(worst <= crate::brep::fit::FIT_TOLERANCE_MM, "{worst}");
    assert_eq!(
        out.procedural_curves[0]
            .cache_fit_tolerance()
            .map(FitTolerance::get),
        Some(crate::brep::fit::FIT_TOLERANCE_MM)
    );
}

/// On a B-surface support the fit resolves the support's knot lines: a
/// ridge narrower than the gaps between its samples is in the cache.
#[test]
fn a_bounded_sp_curve_on_a_b_surface_resolves_its_knot_lines() {
    let carrier = bounded(31, 200);
    let mut out = Brep::default();
    out.surfaces.push(Surface {
        id: SurfaceId::mint("sldprt:brep:surf#100").expect("identity grammar"),
        source_object: None,
        geometry: crate::test_support::surfaces::ridged(),
    });
    let emitted = HashMap::from([(200, 100)]);
    attach(&mut out, &carrier, &emitted);
    let cache = out.curves[0].geometry.solved_cache().expect("a cache");
    let worst = (0..=20_011)
        .map(|i| f64::from(i) / 20_011.0)
        .map(|t| {
            cache_point(cache, t).distance(
                crate::brep::sp_curve::point_at(
                    &ctx(),
                    &out.surfaces[0].geometry,
                    &carrier.pcurve,
                    t,
                )
                .expect("fixture budget")
                .expect("on the support"),
            )
        })
        .fold(0.0, f64::max);
    assert!(worst <= crate::brep::fit::FIT_TOLERANCE_MM, "{worst}");
}

/// A curve the fit refuses stays the bare construction, and declares no
/// fit: here a range of no width.
#[test]
fn a_bounded_sp_curve_the_fit_refuses_has_no_cache() {
    let carrier = cornered([0.5, 0.5]);
    let mut out = Brep::default();
    out.surfaces.push(cylinder("sldprt:brep:surf#100"));
    let emitted = HashMap::from([(200, 100)]);
    attach(&mut out, &carrier, &emitted);
    assert!(matches!(
        out.curves[0].geometry,
        CurveGeometry::Procedural { cache: None, .. }
    ));
    assert_eq!(out.procedural_curves[0].cache_fit_tolerance(), None);
}

/// The planar triangle with `edge_40` and `coedge_30` in place of its own, and
/// an `SP_CURVE` (attr 70) on its plane whose B-curve runs from `from` to
/// `to`, metres, bounded over `[0, 1]` by attr 75. Edge 40 runs from vertex 50
/// at the origin to vertex 51 at `x = 1` m.
fn triangle_on_sp_curve(edge_40: &[u8], coedge_30: &[u8], from: [f64; 2], to: [f64; 2]) -> Vec<u8> {
    use crate::test_support::parasolid::{
        coedge, edge_use, planar_b_curve, sp_curve_records, triangle_body,
    };
    let replace = |body: &mut Vec<u8>, old: &[u8], new: &[u8]| {
        let at = body
            .windows(old.len())
            .position(|window| window == old)
            .expect("triangle record");
        body.splice(at..at + old.len(), new.iter().copied());
    };
    let mut body = triangle_body();
    replace(&mut body, &edge_use(40, 0), edge_40);
    replace(&mut body, &coedge(30, 20, 32, 51, 0, 40, false), coedge_30);
    body.extend(planar_b_curve(from, to, false));
    body.extend(sp_curve_records(
        100,
        [[from[0], from[1], 0.0], [to[0], to[1], 0.0]],
    ));
    body
}

fn decode_edge_40(body: &[u8]) -> (Brep, cadmpeg_ir::topology::Edge) {
    let brep = super::decode_body(&ctx(), body, &cadmpeg_ir::stream_name!("test"))
        .expect("the triangle decodes");
    let edge = brep
        .edges
        .iter()
        .find(|edge| edge.id.as_str().ends_with("edge#40"))
        .expect("edge 40")
        .clone();
    (brep, edge)
}

fn plain_coedge_30() -> Vec<u8> {
    crate::test_support::parasolid::coedge(30, 20, 32, 51, 0, 40, false)
}

#[test]
fn an_edge_on_a_bare_sp_curve_runs_from_the_domain_low_end() {
    use crate::test_support::parasolid::edge_use;
    let edge_40 = edge_use(40, 70);
    let (brep, edge) = decode_edge_40(&triangle_on_sp_curve(
        &edge_40,
        &plain_coedge_30(),
        [0.0, 0.0],
        [1.0, 0.0],
    ));
    assert!(edge
        .curve()
        .is_some_and(|id| id.as_str().ends_with("curve#70")));
    assert!(edge.start.as_str().ends_with("vertex#50"));
    assert!(brep.curves.iter().any(|curve| {
        curve.id.as_str().ends_with("curve#70")
            && matches!(curve.geometry, CurveGeometry::Procedural { .. })
    }));
    // A B-curve from vertex 51 to vertex 50 reverses the edge.
    let (_, edge) = decode_edge_40(&triangle_on_sp_curve(
        &edge_40,
        &plain_coedge_30(),
        [1.0, 0.0],
        [0.0, 0.0],
    ));
    assert!(edge.start.as_str().ends_with("vertex#51"));
    assert!(edge.end.as_str().ends_with("vertex#50"));
}

#[test]
fn a_bare_sp_curve_that_misses_its_vertices_stays_opaque() {
    use crate::test_support::parasolid::edge_use;
    let (brep, edge) = decode_edge_40(&triangle_on_sp_curve(
        &edge_use(40, 70),
        &plain_coedge_30(),
        [0.0, 0.0],
        [0.5, 0.0],
    ));
    let curve = edge.curve().expect("the edge keeps its curve slot");
    assert!(brep.curves.iter().any(|candidate| {
        candidate.id == *curve
            && matches!(
                candidate.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            )
    }));
}

#[test]
fn a_tolerant_edge_takes_its_fin_curve_and_its_tolerance_in_millimetres() {
    use crate::test_support::parasolid::{tolerant_edge_use, with_fin_curve};
    let (_, edge) = decode_edge_40(&triangle_on_sp_curve(
        &tolerant_edge_use(40, 2.0e-6),
        &with_fin_curve(plain_coedge_30(), 75),
        [0.0, 0.0],
        [1.0, 0.0],
    ));
    assert!(edge
        .curve()
        .is_some_and(|id| id.as_str().ends_with("curve#75")));
    let tolerance = edge.tolerance.expect("a tolerant edge").get();
    assert!((tolerance - 2.0e-3).abs() < 1e-15, "{tolerance}");
}

#[test]
fn a_tolerant_edge_whose_fins_name_no_curve_has_no_curve() {
    use crate::test_support::parasolid::tolerant_edge_use;
    let (_, edge) = decode_edge_40(&triangle_on_sp_curve(
        &tolerant_edge_use(40, 2.0e-6),
        &plain_coedge_30(),
        [0.0, 0.0],
        [1.0, 0.0],
    ));
    assert_eq!(edge.curve(), None);
}

#[test]
fn a_ring_on_a_bounded_sp_curve_seams_where_its_stored_ends_meet() {
    let mut ring = bounded(75, 200);
    ring.parameter_range = [1.0, 0.0];
    ring.points = [Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 5.0e-6)];
    // The low end of the range is the second stored point.
    assert_eq!(
        super::surface_curve_ring_start(&ring),
        Some((Point3::new(2.0, 0.0, 5.0e-6), [0.0, 1.0]))
    );
    ring.points[1] = Point3::new(2.0, 0.0, 2.0e-5);
    assert_eq!(super::surface_curve_ring_start(&ring), None);
}
