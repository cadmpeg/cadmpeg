// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::{FRAC_PI_2, PI};

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::report::WitnessKind;
use crate::select::Selection;
use crate::test_support::builders::{assert_measured, measure, Builder, Circle, Frame};

fn cap(radius: f64, north: bool, reversed: bool, owned_pole: bool) -> CadIr {
    let mut builder = Builder::new();
    let rim = builder.vertex([0.8 * radius, 0.0, 0.6 * radius]);
    let end = if north { FRAC_PI_2 } else { -FRAC_PI_2 };
    let pole = builder.vertex([0.0, 0.0, radius * end.sin()]);
    let seam = builder.arc(
        Circle {
            frame: Frame {
                origin: [0.0; 3],
                axis: [0.0, if north { -1.0 } else { 1.0 }, 0.0],
                reference: [1.0, 0.0, 0.0],
            },
            radius,
        },
        [
            if north {
                0.6_f64.asin()
            } else {
                -0.6_f64.asin()
            },
            FRAC_PI_2,
        ],
        &rim,
        &pole,
    );
    let circle = builder.circle(
        [0.0, 0.0, 0.6 * radius],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        0.8 * radius,
        &rim,
    );
    let mut ring = vec![(seam.clone(), Sense::Forward)];
    if owned_pole {
        ring.push((builder.bare_edge(&pole, &pole), Sense::Forward));
    }
    ring.extend([
        (seam, Sense::Reversed),
        (
            circle,
            if north {
                Sense::Forward
            } else {
                Sense::Reversed
            },
        ),
    ]);
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), radius);
    let face = builder.face(
        &sphere,
        if reversed {
            Sense::Reversed
        } else {
            Sense::Forward
        },
        &[ring],
        false,
    );
    if owned_pole {
        builder.line_pcurve(
            &face,
            [0, 1],
            [0.0, end],
            [if north { -1.0 } else { 1.0 }, 0.0],
            [0.0, 2.0 * PI],
        );
    }
    builder.finish(BodyKind::Sheet)
}

#[test]
fn owned_poles_confirm_their_spherical_caps_at_different_scales() {
    for radius in [0.1, 10.0, 10000.0] {
        for north in [false, true] {
            let ir = cap(radius, north, false, true);
            let report = measure(
                &ir,
                &Selection {
                    all_faces: true,
                    ..Selection::default()
                },
            );
            let [face] = report.faces().as_slice() else {
                panic!("one cap")
            };
            let area = face.measures().area().measured().unwrap_or_else(|| {
                panic!(
                    "cap area at radius {radius}, north {north}: {:?}",
                    face.measures().area()
                )
            });
            assert_eq!(*face.orientation().consistent(), Some(true));
            assert_eq!(
                *face.orientation().witness().as_ref().unwrap().kind(),
                WitnessKind::OwnedPole
            );
            assert!(area.reading().is_none());
            let expected = 2.0 * PI * radius * radius * if north { 0.4 } else { 1.6 };
            assert_measured(face.measures().area(), expected, radius * radius);
        }
    }
}

#[test]
fn owned_poles_contradict_reversed_spherical_caps() {
    for north in [false, true] {
        let ir = cap(10.0, north, true, true);
        let report = measure(
            &ir,
            &Selection {
                all_faces: true,
                ..Selection::default()
            },
        );
        let [face] = report.faces().as_slice() else {
            panic!("one cap")
        };
        assert_eq!(*face.orientation().consistent(), Some(false));
        assert!(face.measures().area().unavailable().is_some());
    }
}

#[test]
fn a_synthesized_pole_crossing_keeps_the_cap_unconfirmed() {
    let ir = cap(10.0, true, false, false);
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let [face] = report.faces().as_slice() else {
        panic!("one cap")
    };
    assert_eq!(*face.orientation().consistent(), None);
    assert!(face
        .measures()
        .area()
        .measured()
        .unwrap()
        .reading()
        .is_some());
}

#[test]
fn a_placed_cap_uses_its_model_space_pole() {
    let mut ir = cap(10.0, true, false, true);
    ir.model.bodies[0].transform = Transform::affine([
        [0.0, 0.0, 1.0, 100.0],
        [0.0, 1.0, 0.0, -20.0],
        [-1.0, 0.0, 0.0, 30.0],
    ]);
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let [face] = report.faces().as_slice() else {
        panic!("one cap")
    };
    assert_eq!(*face.orientation().consistent(), Some(true));
    assert_measured(face.measures().area(), 80.0 * PI, 100.0);
}

#[test]
fn a_cap_within_the_coincidence_allowance_stays_unconfirmed() {
    let ir = cap(0.01, true, false, true);
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let [face] = report.faces().as_slice() else {
        panic!("one cap")
    };
    assert!(face.measures().area().unavailable().is_some());
    assert!(face.orientation().witness().is_none());
}

#[test]
fn orientation_findings_use_owned_poles_without_face_rows() {
    for reversed in [false, true] {
        let ir = cap(10.0, true, reversed, true);
        let ctx = crate::test_support::builders::service_context();
        let findings = crate::findings(&ctx, &ir, crate::MeasureOptions::default()).unwrap();
        assert_eq!(
            findings.iter().any(|finding| {
                finding.check == cadmpeg_ir::report::check::Check::Orientation
                    && finding.entity.as_deref() == Some(ir.model.faces[0].id.as_str())
            }),
            reversed
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn an_unresolved_closed_edge_at_a_pole_cannot_confirm_the_face() {
    let mut ir = cap(10.0, true, false, true);
    let pole_edge = ir
        .model
        .coedges
        .iter()
        .find(|coedge| !coedge.pcurves.is_empty())
        .unwrap()
        .edge
        .clone();
    ir.model
        .edges
        .iter_mut()
        .find(|edge| edge.id == pole_edge)
        .unwrap()
        .carrier = cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(
        cadmpeg_ir::ids::CurveId::mint("test:missing:curve#pole").unwrap(),
    ));
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let [face] = report.faces().as_slice() else {
        panic!("one cap")
    };
    assert!(face.orientation().witness().is_none());
}

#[test]
fn a_sampled_nurbs_trim_does_not_certify_pole_clearance() {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;

    let mut ir = cap(10.0, true, false, true);
    let circle_edge = &ir.model.coedges.last().unwrap().edge;
    let circle_curve = ir
        .model
        .edges
        .iter()
        .find(|edge| &edge.id == circle_edge)
        .unwrap()
        .carrier
        .curve()
        .unwrap()
        .clone();
    let points = [
        [8.0, 0.0],
        [8.0, 8.0],
        [0.0, 8.0],
        [-8.0, 8.0],
        [-8.0, 0.0],
        [-8.0, -8.0],
        [0.0, -8.0],
        [8.0, -8.0],
        [8.0, 0.0],
    ]
    .into_iter()
    .map(|[x, y]| Point3::new(x, y, 6.0))
    .collect();
    let weight = 0.5_f64.sqrt();
    let geometry = NurbsCurve::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
        NurbsPoles3::from_lanes(
            points,
            Some(vec![
                1.0, weight, 1.0, weight, 1.0, weight, 1.0, weight, 1.0,
            ]),
        )
        .unwrap(),
        false,
    )
    .unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == circle_curve)
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry));
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let [face] = report.faces().as_slice() else {
        panic!("one cap")
    };
    assert_measured(face.measures().area(), 80.0 * PI, 100.0);
    assert_eq!(*face.orientation().consistent(), None);
    assert!(face
        .measures()
        .area()
        .measured()
        .unwrap()
        .reading()
        .is_some());
}

#[test]
fn a_degenerate_carrier_away_from_its_vertex_cannot_witness_the_pole() {
    use cadmpeg_ir::geometry::analytic::DegenerateCurve;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::EdgeCarrier;

    let mut ir = cap(10.0, true, false, true);
    let pole_edge = ir
        .model
        .coedges
        .iter()
        .find(|coedge| !coedge.pcurves.is_empty())
        .unwrap()
        .edge
        .clone();
    let curve = CurveId::mint("test:misplaced:curve#pole").unwrap();
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            DegenerateCurve::try_new(Point3::new(0.0, 0.0, -10.0)).unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    ir.model
        .edges
        .iter_mut()
        .find(|edge| edge.id == pole_edge)
        .unwrap()
        .carrier = EdgeCarrier::unbounded(Some(curve));
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let [face] = report.faces().as_slice() else {
        panic!("one cap")
    };
    assert!(face.orientation().witness().is_none());
}
