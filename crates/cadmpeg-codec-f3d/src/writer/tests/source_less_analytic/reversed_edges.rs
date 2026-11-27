// SPDX-License-Identifier: Apache-2.0
//! Source-less edges whose retained ASM sense is reversed.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::curve_point_solved;
use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{EdgeCarrier, Sense};

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

const EPS_EDGE_POINT_MM: f64 = 1.0e-9;

fn point_at(geometry: &CurveGeometry, parameter: f64) -> Point3 {
    curve_point_solved(
        EvaluationAdmission::Standard,
        geometry.solved().expect("a solved edge curve"),
        parameter,
    )
    .expect("the edge curve evaluates")
    .get()
}

/// The decoder states the curve of a reversed ASM edge on the edge's own
/// parameterization (asm.md §5, "Edge"). The writer writes the curve record
/// as its reverse, so the edge reads back on the same curve and range.
#[test]
fn a_retained_reversed_edge_writes_its_curve_reversed() {
    let cases = [
        (
            "line",
            SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(1.0, 2.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                    .unwrap(),
            ),
            [0.5, 3.0],
        ),
        (
            "circle",
            SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(4.0, -2.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    6.5,
                )
                .unwrap(),
            ),
            [0.25, 1.75],
        ),
        (
            "nurbs",
            SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(3.0, 4.0, 0.0),
                        Point3::new(8.0, 1.0, 0.0),
                    ],
                    None,
                    false,
                )
                .unwrap(),
            ),
            [0.2, 0.9],
        ),
    ];
    for (name, geometry, range) in cases {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
                &DecodeOptions::default(),
            )
            .expect("generated planar triangle decode");
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                "f3d",
                &[],
            )
            .unwrap();
        let curve = CurveId::mint(format!("generated:test:{name}#0")).unwrap();
        let expected = CurveGeometry::Solved(geometry);
        source_less.geometry.curves.push(Curve {
            parameter_range: None,
            id: curve.clone(),
            geometry: expected.clone(),
            source_object: None,
            native_ref: None,
        });
        source_less.geometry.edges[0].carrier = EdgeCarrier::new(Some(curve), Some(range)).unwrap();
        let edge = source_less.geometry.edges[0].id.clone();
        {
            let mut native = f3d_native_mut(&mut source_less);
            native
                .edge_continuities
                .iter_mut()
                .find(|metadata| metadata.edge == edge)
                .expect("generated edge continuity")
                .sense = Sense::Reversed;
        }

        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None))
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect(name);
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect(name);
        let geometry = &round_trip.ir().geometry;
        let written = &geometry.edges[0];
        let carrier = geometry
            .curves
            .iter()
            .find(|candidate| Some(&candidate.id) == written.curve())
            .expect(name);
        let [start, end] = written.param_range().expect(name).get();
        for (stated, read) in [
            (range[0], start),
            (f64::midpoint(range[0], range[1]), f64::midpoint(start, end)),
            (range[1], end),
        ] {
            let stated = point_at(&expected, stated);
            let read = point_at(&carrier.geometry, read);
            assert!(
                stated.distance(read) < EPS_EDGE_POINT_MM,
                "{name}: {stated:?} != {read:?}"
            );
        }
        assert_eq!(
            f3d_native(round_trip.ir()).edge_continuities[0].sense,
            Sense::Reversed,
            "{name}"
        );
    }
}
