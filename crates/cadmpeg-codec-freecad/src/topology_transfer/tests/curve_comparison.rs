// SPDX-License-Identifier: Apache-2.0
//! Admission of exact curve records with independently owned lanes.

use std::fmt::Write as _;

use super::{archive_entries, assert_codec_work_refusal, select_exact_curve_representation};
use crate::brep::{NestedCurve, Tables, TextCurve, TextEdgeRepresentation, TextTShapes};
use crate::FcstdCodec;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn nurbs(rational: bool, last_x: f64) -> TextCurve {
    const POLES: usize = 128;
    let mut points = vec![FinitePoint3::ZERO; POLES];
    points[POLES - 1] = FinitePoint3::new(Point3::new(last_x, 0.0, 0.0)).unwrap();
    let mut knots = vec![FiniteReal::ZERO];
    for index in 0..POLES {
        knots.push(FiniteReal::new(cadmpeg_core::convert::f64_from_index(index).unwrap()).unwrap());
    }
    knots.push(*knots.last().unwrap());
    let curve = NurbsCurve::from_finite_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        knots,
        points,
        rational.then(|| vec![FiniteReal::ONE; POLES]),
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }),
        rational
    );
    TextCurve::Nurbs(curve)
}

fn exact(curve: usize) -> TextEdgeRepresentation {
    TextEdgeRepresentation::Curve3d {
        curve,
        location: 0,
        parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
    }
}

#[test]
fn exact_curve_comparison_preserves_owned_polynomial_rational_and_nested_records() {
    for rational in [false, true] {
        for nested in [false, true] {
            let wrap = |curve| {
                if nested {
                    TextCurve::Offset {
                        distance: FiniteReal::ONE,
                        direction: FiniteVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                        basis: NestedCurve::try_new(TextCurve::Trimmed {
                            parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
                            basis: NestedCurve::try_new(curve).unwrap(),
                        })
                        .unwrap(),
                    }
                } else {
                    curve
                }
            };
            let curves = [
                wrap(nurbs(rational, 1.0)),
                wrap(nurbs(rational, 1.0)),
                wrap(nurbs(rational, 2.0)),
            ];
            let shapes = TextTShapes::default();
            let tables = Tables {
                curves: &curves,
                tshapes: &shapes,
                locations: &[],
                curve2ds: &[],
                surfaces: &[],
                polygons3d: &[],
                polygons_on_triangulations: &[],
                triangulations: &[],
                roots: &[],
            };
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let equivalent = [exact(1), exact(2), exact(1), exact(2)];
            assert_eq!(
                select_exact_curve_representation(&ctx, 7, &equivalent, &tables)
                    .unwrap()
                    .unwrap()
                    .0,
                0
            );
            crate::test_support::refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                &[],
                "FreeCAD exact curve equality",
                |ctx| select_exact_curve_representation(ctx, 7, &equivalent, &tables),
            );
            let error = select_exact_curve_representation(&ctx, 7, &[exact(1), exact(3)], &tables)
                .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::Malformed(message)
                if message == "edge TShape 7 has non-equivalent 3D curve representations"));
            let _probe = RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "FreeCAD exact curve equality",
                None,
            );
            for representations in [[exact(1), exact(4)], [exact(0), exact(0)]] {
                let error = select_exact_curve_representation(&ctx, 7, &representations, &tables)
                    .unwrap_err();
                assert!(matches!(error, cadmpeg_core::CodecError::Malformed(message)
                    if message == "edge TShape 7 has non-equivalent 3D curve representations"));
            }
            assert_eq!(
                select_exact_curve_representation(&ctx, 7, &[exact(4), exact(5)], &tables)
                    .unwrap()
                    .unwrap()
                    .0,
                0
            );
            assert_eq!(ctx.resource_refusal(), None);
        }
    }
}

fn repeated_nurbs_archive() -> Vec<u8> {
    const POLES: usize = 128;
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut curve = format!("7 0 0 1 {POLES} {POLES}");
    for point in 0..POLES {
        write!(curve, " {point} 0 0").expect("write fixture text");
    }
    for knot in 0..POLES {
        let multiplicity = if knot == 0 || knot + 1 == POLES { 2 } else { 1 };
        write!(curve, " {knot} {multiplicity}").expect("write fixture text");
    }
    let mut brep = format!("CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 2\n{curve}\n{curve}\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 3\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 127 0 0 0 0 1001000 *\nEd 0.001 1 1 0");
    for curve in [1, 2, 1, 2, 1, 2, 1, 2] {
        write!(brep, " 1 {curve} 0 0 127").expect("write fixture text");
    }
    brep.push_str(" 0 1001000 +3 0 -2 0 *\n+1 0 *");
    archive_entries(&[("Document.xml", document), ("Shape.brp", brep.as_bytes())])
}

#[test]
fn exact_nurbs_comparison_refuses_through_decode() {
    let input = repeated_nurbs_archive();
    let result = FcstdCodec
        .decode(&mut Cursor::new(&input), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.curves.len(), 2);
    assert_eq!(
        result.ir().model.edges[0].curve(),
        Some(&result.ir().model.curves[0].id)
    );
    assert_codec_work_refusal(&input, "FreeCAD exact curve equality");
}
