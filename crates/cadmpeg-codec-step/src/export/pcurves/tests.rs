// SPDX-License-Identifier: Apache-2.0
//! Edge-range reparameterization preserves the spline locus and remains local to a use.

use std::borrow::Cow;
use std::collections::HashSet;
use std::f64::consts::TAU;

use cadmpeg_ir::geometry::analytic::CircleCurve;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveMetadata, PcurveNurbs, PcurveNurbsPoles};
use cadmpeg_ir::geometry::{CurveGeometry, DirectedParameterRange, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{Edge, ParameterInterval};

const EPS_KNOT: f64 = 1.0e-12;

fn spline() -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            1,
            vec![-1.0, -1.0, 0.0, 0.0],
            PcurveNurbsPoles::Polynomial {
                points: vec![Point2::new(0.5, 0.0), Point2::new(-0.5, 0.0)],
            },
            false,
        )
        .unwrap(),
    }
}

fn circle() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    ))
}

fn edge(range: [f64; 2]) -> Edge {
    let mut edge = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .edges
        .remove(0);
    edge.set_param_range(Some(ParameterInterval::new(range).unwrap()));
    edge
}

#[test]
fn line_pcurve_is_parameterized_on_the_edge_in_both_directions() {
    let line = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
        Point2::new(0.0, 1.0),
        Point2::new(2.0, 0.0),
    )
    .unwrap();
    for source in [[2.0, 5.0], [5.0, 2.0]] {
        let aligned = super::align_line_to_edge(&line, Some(source), &edge([10.0, 20.0])).unwrap();
        for (target, source) in [(10.0, source[0]), (20.0, source[1])] {
            let point = cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &aligned,
                target,
            )
            .unwrap()
            .get();
            assert!((point.u - 2.0 * source).abs() < EPS_KNOT);
            assert!((point.v - 1.0).abs() < EPS_KNOT);
        }
    }
}

#[test]
fn deeply_placed_line_pcurve_aligns_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut geometry = PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 1.0),
                    Point2::new(2.0, 0.0),
                )
                .unwrap(),
            );
            for _ in 0..cadmpeg_ir::geometry::MAX_GEOMETRY_NESTING - 1 {
                geometry = PcurveGeometry::Transformed(
                    cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
                        Box::new(geometry),
                        cadmpeg_ir::transform::Transform2::identity(),
                    )
                    .unwrap(),
                );
            }
            let aligned =
                super::align_placed_line_to_edge(&geometry, Some([2.0, 5.0]), &edge([10.0, 20.0]))
                    .unwrap();
            let point = cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &aligned,
                20.0,
            )
            .unwrap()
            .get();
            assert!((point.u - 10.0).abs() < EPS_KNOT);
            assert!((point.v - 1.0).abs() < EPS_KNOT);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn whole_period_shifts_move_knots_without_moving_poles_or_the_shared_source() {
    let geometry = spline();
    let PcurveGeometry::Nurbs { nurbs: source } = &geometry else {
        panic!("spline")
    };
    let range = Some([-1.0, 0.0]);
    for turns in [-2.0, -1.0, 1.0, 2.0] {
        let shift = turns * TAU;
        let aligned = super::align_spline_to_edge(&geometry, range, &edge([shift - 1.0, shift]));
        let PcurveGeometry::Nurbs { nurbs } = aligned.as_ref() else {
            panic!("spline")
        };
        for (actual, expected) in nurbs
            .knots()
            .iter()
            .zip([shift - 1.0, shift - 1.0, shift, shift])
        {
            assert!((actual - expected).abs() < EPS_KNOT);
        }
        assert_eq!(nurbs.pole_rows(), source.pole_rows());
        assert_eq!(geometry, spline(), "alignment never edits a shared carrier");
    }
}

#[test]
fn a_use_that_already_runs_on_the_edge_range_or_states_no_range_is_kept() {
    let geometry = spline();
    let range = Some([-1.0, 0.0]);
    for (range, edge) in [(range, edge([-1.0, 0.0])), (None, edge([TAU - 1.0, TAU]))] {
        assert!(matches!(
            super::align_spline_to_edge(&geometry, range, &edge),
            Cow::Borrowed(_)
        ));
    }
}

/// The parameter-space point a reparameterized spline reaches at `t`.
fn point_at(geometry: &PcurveGeometry, t: f64) -> Point2 {
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("spline")
    };
    let knots = nurbs.knots();
    let (start, end) = (knots[1], knots[2]);
    let poles = nurbs.pole_rows().points();
    let fraction = (t - start) / (end - start);
    let (first, last) = (poles[0].get(), poles[1].get());
    Point2::new(
        first.u + (last.u - first.u) * fraction,
        first.v + (last.v - first.v) * fraction,
    )
}

#[test]
fn a_decreasing_use_reverses_the_spline_onto_the_increasing_edge_range() {
    let geometry = spline();
    let range = Some([0.0, -1.0]);
    let aligned = super::align_spline_to_edge(&geometry, range, &edge([2.0, 3.0]));
    let PcurveGeometry::Nurbs { nurbs } = aligned.as_ref() else {
        panic!("spline")
    };
    for (actual, expected) in nurbs.knots().iter().zip([2.0, 2.0, 3.0, 3.0]) {
        assert!((actual - expected).abs() < EPS_KNOT);
    }
    // The edge start maps to the use start `s = 0`, and the edge end to `s = -1`.
    for (t, s) in [(2.0, 0.0), (2.5, -0.5), (3.0, -1.0)] {
        let expected = point_at(&geometry, s);
        let actual = point_at(aligned.as_ref(), t);
        assert!((actual.u - expected.u).abs() < EPS_KNOT);
        assert!((actual.v - expected.v).abs() < EPS_KNOT);
    }
}

#[test]
fn a_scaled_use_maps_the_spline_onto_the_edge_range_linearly() {
    let geometry = spline();
    let range = Some([-1.0, 0.0]);
    let aligned = super::align_spline_to_edge(&geometry, range, &edge([0.0, 10.0]));
    for (t, s) in [(0.0, -1.0), (2.5, -0.75), (10.0, 0.0)] {
        let expected = point_at(&geometry, s);
        let actual = point_at(aligned.as_ref(), t);
        assert!((actual.u - expected.u).abs() < EPS_KNOT);
        assert!((actual.v - expected.v).abs() < EPS_KNOT);
    }
}

#[test]
fn normal_and_split_edges_keep_the_coedge_interval_until_spline_emission() {
    for split in [false, true] {
        let mut ir = if split {
            cadmpeg_test_support::polyhedron::edge_touching_cubes().unwrap()
        } else {
            cadmpeg_ir::examples::unit_cube().unwrap()
        };
        let edge_id = ir
            .model
            .edges
            .iter()
            .find(|edge| {
                !split
                    || ir
                        .model
                        .coedges
                        .iter()
                        .filter(|coedge| coedge.edge == edge.id)
                        .count()
                        == 4
            })
            .unwrap()
            .id
            .clone();
        let edge = ir
            .model
            .edges
            .iter_mut()
            .find(|edge| edge.id == edge_id)
            .unwrap();
        let curve_id = edge.curve().unwrap().clone();
        edge.set_param_range(Some(ParameterInterval::new([TAU - 1.0, TAU]).unwrap()));
        ir.model
            .curves
            .iter_mut()
            .find(|c| c.id == curve_id)
            .unwrap()
            .geometry = circle();
        let pcurve = cadmpeg_ir::ids::PcurveId::mint("test:model:pcurve#periodic").unwrap();
        ir.model.pcurves.push(cadmpeg_ir::geometry::pcurve::Pcurve {
            id: pcurve.clone(),
            geometry: spline(),
            metadata: PcurveMetadata::default(),
        });
        for coedge in ir
            .model
            .coedges
            .iter_mut()
            .filter(|coedge| coedge.edge == edge_id)
        {
            coedge.pcurves.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: pcurve.clone(),
                isoparametric: None,
                parameter_range: Some(DirectedParameterRange::new([-1.0, 0.0]).unwrap()),
            });
        }
        let mut builder =
            super::super::Builder::new(&ir, crate::StepSchema::Ap214, HashSet::new()).unwrap();
        if split {
            builder.split_non_manifold_edges(&ir.model.regions[0].shells);
        }
        for coedge in ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge_id)
        {
            assert!(builder.emit_coedge_edge(coedge).is_some());
        }
        let lines = builder.emitter.into_lines().unwrap();
        let splines = lines
            .iter()
            .filter(|line| line.contains("B_SPLINE_CURVE_WITH_KNOTS"))
            .collect::<Vec<_>>();
        assert_eq!(splines.len(), if split { 4 } else { 2 });
        for spline in splines {
            let knots = spline
                .split("(2,2),(")
                .nth(1)
                .unwrap()
                .split(')')
                .next()
                .unwrap();
            let knots = knots
                .split(',')
                .map(|value| value.parse::<f64>().unwrap())
                .collect::<Vec<_>>();
            assert!((knots[0] - (TAU - 1.0)).abs() < EPS_KNOT);
            assert!((knots[1] - TAU).abs() < EPS_KNOT);
        }
    }
}

/// A cylinder strip of two lines and two arcs, as in #190: the first arc lies
/// one period above the lines and the second one period below.
fn strip_ends() -> Vec<([f64; 2], [f64; 2])> {
    let (first, second, height) = (5.6, 6.9, 8.15);
    vec![
        ([first, 0.0], [first, height]),
        ([first + TAU, height], [second + TAU, height]),
        ([second, height], [second, 0.0]),
        ([second - TAU, 0.0], [first - TAU, 0.0]),
    ]
}

#[test]
fn loop_pcurves_in_other_periods_move_onto_the_previous_pcurve() {
    let shifts = super::loop_period_shifts(&strip_ends(), [Some(TAU), None]);
    let expected = [[0.0, 0.0], [-TAU, 0.0], [0.0, 0.0], [TAU, 0.0]];
    for (shift, expected) in shifts.iter().zip(expected) {
        assert!((shift[0] - expected[0]).abs() < EPS_KNOT);
        assert!((shift[1] - expected[1]).abs() < EPS_KNOT);
    }
}

#[test]
fn a_gap_that_no_whole_period_closes_and_a_nonperiodic_surface_keep_their_pcurves() {
    let mut ends = strip_ends();
    ends[1].0[0] += 0.5;
    let shifts = super::loop_period_shifts(&ends, [Some(TAU), None]);
    assert_eq!(shifts[1], [0.0, 0.0]);

    let shifts = super::loop_period_shifts(&strip_ends(), [None, None]);
    assert!(shifts.iter().all(|shift| *shift == [0.0, 0.0]));
}

#[test]
fn a_shifted_spline_moves_every_pole_and_keeps_its_knots() {
    let geometry = spline();
    let moved = super::shifted(&geometry, [-TAU, 1.0]).unwrap();
    let PcurveGeometry::Nurbs { nurbs } = &moved else {
        panic!("spline")
    };
    let PcurveGeometry::Nurbs { nurbs: source } = &geometry else {
        panic!("spline")
    };
    assert_eq!(nurbs.knots(), source.knots());
    for (moved, source) in nurbs
        .pole_rows()
        .points()
        .iter()
        .zip(source.pole_rows().points())
    {
        assert!((moved.get().u - (source.get().u - TAU)).abs() < EPS_KNOT);
        assert!((moved.get().v - (source.get().v + 1.0)).abs() < EPS_KNOT);
    }
}

/// The unit cube with the surface of a face on its first edge replaced by a
/// plane whose u axis runs along that edge from its start vertex, and a
/// linear spline pcurve from `from` to `to` in that plane.
fn cube_edge_with_pcurve(from: f64, to: f64) -> (cadmpeg_ir::CadIr, PcurveGeometry, String, Edge) {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let edge = ir.model.edges[0].clone();
    let position = |vertex: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir.model.vertices.iter().find(|v| v.id == *vertex).unwrap();
        ir.model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let (start, end) = (position(&edge.start), position(&edge.end));
    let along = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
    let length = (along.x * along.x + along.y * along.y + along.z * along.z).sqrt();
    let along = Vector3::new(along.x / length, along.y / length, along.z / length);
    let normal = if along.x.abs() < 0.5 {
        Vector3::new(0.0, along.z, -along.y)
    } else {
        Vector3::new(along.y, -along.x, 0.0)
    };
    let width = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
    let normal = Vector3::new(normal.x / width, normal.y / width, normal.z / width);
    let coedge = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge.id)
        .unwrap();
    let face = ir
        .model
        .faces
        .iter()
        .find(|face| face.loops.contains(&coedge.owner_loop))
        .unwrap();
    let surface_id = face.surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(start, normal, along).unwrap(),
        ),
    );
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            PcurveNurbsPoles::Polynomial {
                points: vec![
                    Point2::new(from * length, 0.0),
                    Point2::new(to * length, 0.0),
                ],
            },
            false,
        )
        .unwrap(),
    };
    (ir, pcurve, surface_id.as_str().to_owned(), edge)
}

#[test]
fn a_use_interval_is_directed_by_the_vertex_its_surface_image_reaches() {
    for (from, to, stored, expected) in [
        (0.0, 1.0, [1.0, 0.0], [0.0, 1.0]),
        (0.0, 1.0, [0.0, 1.0], [0.0, 1.0]),
        (1.0, 0.0, [0.0, 1.0], [1.0, 0.0]),
        (1.0, 0.0, [1.0, 0.0], [1.0, 0.0]),
    ] {
        let (ir, pcurve, surface, edge) = cube_edge_with_pcurve(from, to);
        let builder =
            super::super::Builder::new(&ir, crate::StepSchema::Ap214, HashSet::new()).unwrap();
        let directed = builder
            .directed_use_range(
                &pcurve,
                Some(DirectedParameterRange::new(stored).unwrap()),
                &surface,
                &edge,
            )
            .unwrap();
        assert_eq!(
            directed,
            Some(expected),
            "pcurve {from} -> {to}, use {stored:?}"
        );
    }
}

#[test]
fn a_closed_edge_directs_only_an_increasing_use_interval() {
    let (mut ir, pcurve, surface, mut edge) = cube_edge_with_pcurve(0.0, 0.0);
    edge.end = edge.start.clone();
    ir.model.edges[0] = edge.clone();
    let builder =
        super::super::Builder::new(&ir, crate::StepSchema::Ap214, HashSet::new()).unwrap();
    for (stored, expected) in [([0.0, 1.0], Some([0.0, 1.0])), ([1.0, 0.0], None)] {
        let directed = builder
            .directed_use_range(
                &pcurve,
                Some(DirectedParameterRange::new(stored).unwrap()),
                &surface,
                &edge,
            )
            .unwrap();
        assert_eq!(directed, expected);
    }
}

#[test]
fn reflected_conic_emission_uses_a_rational_spline() {
    use cadmpeg_ir::geometry::pcurve::{CirclePcurve, PlacedPcurve};
    use cadmpeg_ir::transform::Transform2;
    let circle = PcurveGeometry::Circle(
        CirclePcurve::try_new(
            Point2::new(1.0, 2.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            3.0,
        )
        .unwrap(),
    );
    let reflected = PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(circle),
            Transform2::affine([[1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]).unwrap(),
        )
        .unwrap(),
    );
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].set_param_range(Some(ParameterInterval::new([10.0, 20.0]).unwrap()));
    let pcurve_id = cadmpeg_ir::ids::PcurveId::mint("test:model:pcurve#reflected").unwrap();
    ir.model.pcurves.push(cadmpeg_ir::geometry::pcurve::Pcurve {
        id: pcurve_id.clone(),
        geometry: reflected,
        metadata: PcurveMetadata::default(),
    });
    for coedge in ir.model.coedges.iter_mut().filter(|c| c.edge == edge_id) {
        coedge.pcurves.push(cadmpeg_ir::topology::PcurveUse {
            pcurve: pcurve_id.clone(),
            isoparametric: None,
            parameter_range: Some(DirectedParameterRange::new([0.0, TAU]).unwrap()),
        });
    }
    let mut builder =
        super::super::Builder::new(&ir, crate::StepSchema::Ap214, HashSet::new()).unwrap();
    for coedge in ir.model.coedges.iter().filter(|c| c.edge == edge_id) {
        let (start, end) = builder
            .coedge_pcurve_ends(coedge.id.as_str())
            .unwrap()
            .unwrap();
        for point in [start, end] {
            assert!((point[0] - 4.0).abs() < EPS_KNOT);
            assert!((point[1] + 2.0).abs() < EPS_KNOT);
        }
        assert!(builder.emit_coedge_edge(coedge).is_some());
    }
    let lines = builder.emitter.into_lines().unwrap();
    assert!(lines
        .iter()
        .any(|line| line.contains("RATIONAL_B_SPLINE_CURVE")));
    assert!(!lines.iter().any(|line| line.contains("CURVE_REPLICA")));
    for spline in lines
        .iter()
        .filter(|line| line.contains("RATIONAL_B_SPLINE_CURVE"))
    {
        let knots = spline
            .split("(3,2,2,2,3),(")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        let knots = knots
            .split(',')
            .map(|value| value.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(knots.len(), 5);
        for (actual, expected) in knots.iter().zip([10.0, 12.5, 15.0, 17.5, 20.0]) {
            assert!((actual - expected).abs() < EPS_KNOT);
        }
    }
}

#[test]
fn reflected_conics_lower_to_exact_rational_splines() {
    use cadmpeg_ir::geometry::pcurve::{CirclePcurve, PlacedPcurve};
    use cadmpeg_ir::transform::Transform2;
    use std::f64::consts::{FRAC_PI_2, PI};

    let circle = PcurveGeometry::Circle(
        CirclePcurve::try_new(
            Point2::new(1.0, 2.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            3.0,
        )
        .unwrap(),
    );
    let placed = |rows| {
        PcurveGeometry::Transformed(
            PlacedPcurve::try_new(Box::new(circle.clone()), Transform2::affine(rows).unwrap())
                .unwrap(),
        )
    };
    let reflected = placed([[1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]);
    let Cow::Owned(PcurveGeometry::Nurbs { nurbs }) = super::lower_placed_conic(&reflected, None)
    else {
        panic!("a reflected circle lowers to a spline");
    };
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(
        nurbs.knots().as_slice(),
        [
            0.0,
            0.0,
            0.0,
            FRAC_PI_2,
            FRAC_PI_2,
            PI,
            PI,
            3.0 * FRAC_PI_2,
            3.0 * FRAC_PI_2,
            TAU,
            TAU,
            TAU
        ]
    );
    let points = nurbs
        .pole_rows()
        .points()
        .iter()
        .map(|point| point.get())
        .collect::<Vec<_>>();
    let weights = nurbs.pole_rows().weights().expect("rational spline");
    let on_reflected = |angle: f64| Point2::new(1.0 + 3.0 * angle.cos(), -2.0 - 3.0 * angle.sin());
    for (index, angle) in [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2, TAU]
        .into_iter()
        .enumerate()
    {
        let expected = on_reflected(angle);
        let point = points[2 * index];
        assert!((point.u - expected.u).abs() < EPS_KNOT && (point.v - expected.v).abs() < EPS_KNOT);
    }
    for span in 0..4 {
        let [start, middle, end] = [points[2 * span], points[2 * span + 1], points[2 * span + 2]];
        let weight = weights[2 * span + 1];
        let denominator = 0.5 + 0.5 * weight;
        let u = (0.25 * start.u + 0.5 * weight * middle.u + 0.25 * end.u) / denominator;
        let v = (0.25 * start.v + 0.5 * weight * middle.v + 0.25 * end.v) / denominator;
        assert!(((u - 1.0).hypot(v + 2.0) - 3.0).abs() < EPS_KNOT);
    }

    let arc = super::lower_placed_conic(
        &reflected,
        Some(DirectedParameterRange::new([0.0, FRAC_PI_2]).unwrap()),
    );
    let PcurveGeometry::Nurbs { nurbs } = arc.as_ref() else {
        panic!("a reflected arc lowers to a spline");
    };
    assert_eq!(nurbs.pole_rows().count(), 3);

    let turned = placed([[0.0, -1.0, 0.0], [1.0, 0.0, 0.0]]);
    assert!(matches!(
        super::lower_placed_conic(&turned, None),
        Cow::Borrowed(_)
    ));
}

#[test]
fn reflected_ellipse_uses_follow_edge_ranges_and_loop_shifts() {
    use cadmpeg_ir::geometry::pcurve::{EllipsePcurve, PlacedPcurve};
    use cadmpeg_ir::transform::Transform2;
    use std::f64::consts::FRAC_PI_2;

    let ellipse = PcurveGeometry::Ellipse(
        EllipsePcurve::try_new(
            Point2::new(1.0, 2.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            3.0,
            2.0,
        )
        .unwrap(),
    );
    let reflected = PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(ellipse),
            Transform2::affine([[1.0, 0.0, TAU], [0.0, -1.0, 0.0]]).unwrap(),
        )
        .unwrap(),
    );
    let source = DirectedParameterRange::new([FRAC_PI_2, 0.0]).unwrap();
    let lowered = super::lower_placed_conic(&reflected, Some(source));
    let target = edge([10.0, 20.0]);
    let aligned = super::align_spline_to_edge(&lowered, Some(source.endpoints()), &target);
    let evaluate = |geometry: &PcurveGeometry, parameter| {
        cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            geometry,
            parameter,
        )
        .unwrap()
        .get()
    };
    for (parameter, angle) in [(10.0, FRAC_PI_2), (15.0, FRAC_PI_2 / 2.0), (20.0, 0.0)] {
        let actual = evaluate(&aligned, parameter);
        assert!((actual.u - (1.0 + TAU + 3.0 * angle.cos())).abs() < EPS_KNOT);
        assert!((actual.v - (-2.0 - 2.0 * angle.sin())).abs() < EPS_KNOT);
    }
    let start = evaluate(&aligned, 10.0);
    let end = evaluate(&aligned, 20.0);
    let shifts = super::loop_period_shifts(
        &[
            ([0.0, 0.0], [start.u - TAU, start.v]),
            ([start.u, start.v], [end.u, end.v]),
        ],
        [Some(TAU), None],
    );
    let moved = super::shifted(&aligned, shifts[1]).unwrap();
    for parameter in [10.0, 15.0, 20.0] {
        let before = evaluate(&aligned, parameter);
        let after = evaluate(&moved, parameter);
        assert!((after.u - (before.u - TAU)).abs() < EPS_KNOT);
        assert!((after.v - before.v).abs() < EPS_KNOT);
    }
}
