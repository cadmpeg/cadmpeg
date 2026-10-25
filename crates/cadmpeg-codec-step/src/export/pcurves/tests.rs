// SPDX-License-Identifier: Apache-2.0
//! Periodic branch changes preserve spline geometry and remain local to a use.

use std::borrow::Cow;
use std::f64::consts::TAU;

use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve};
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
fn whole_period_shifts_move_knots_without_moving_poles_or_the_shared_source() {
    let geometry = spline();
    let PcurveGeometry::Nurbs { nurbs: source } = &geometry else {
        panic!("spline")
    };
    let range = Some(DirectedParameterRange::new([-1.0, 0.0]).unwrap());
    for turns in [-2.0, -1.0, 1.0, 2.0] {
        let shift = turns * TAU;
        let aligned =
            super::align_periodic_spline(&geometry, range, &edge([shift - 1.0, shift]), &circle());
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
fn unrelated_intervals_and_nonperiodic_carriers_are_not_reparameterized() {
    let geometry = spline();
    let range = Some(DirectedParameterRange::new([-1.0, 0.0]).unwrap());
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    ));
    for (range, edge, curve) in [
        (range, edge([-1.0, 0.0]), circle()),
        (None, edge([TAU - 1.0, TAU]), circle()),
        (range, edge([TAU - 2.0, TAU]), circle()),
        (range, edge([0.0, 1.0]), circle()),
        (range, edge([TAU, TAU]), circle()),
        (range, edge([TAU - 1.0, TAU]), line),
    ] {
        assert!(matches!(
            super::align_periodic_spline(&geometry, range, &edge, &curve),
            Cow::Borrowed(_)
        ));
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
        ir.model
            .pcurves
            .push(cadmpeg_ir::geometry::pcurve::Pcurve::new(
                pcurve.clone(),
                spline(),
                PcurveMetadata::default(),
            ));
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
        let mut builder = super::super::Builder::new(&ir, crate::StepSchema::Ap214).unwrap();
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
