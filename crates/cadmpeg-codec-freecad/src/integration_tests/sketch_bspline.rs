// SPDX-License-Identifier: Apache-2.0

use std::{io::Cursor, path::Path};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::{sketches::SketchGeometryDefinition, Codec, DecodeOptions};
use cadmpeg_measure::test_support::round_trip::measured;

use crate::{test_support::test_archive::assert_valid_document, FcstdCodec};

const POINT_TOLERANCE: f64 = 1.0e-12;
const VOLUME_TOLERANCE: f64 = 0.01;
const BOX_TOLERANCE: f64 = 1.0e-6;

#[test]
fn recipe_sketch_bspline_knots_and_points() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (variant, periodic) in [("a", true), ("b", true), ("c", false)] {
            let label = format!("recipe_{version}_N04{variant}");
            let bytes = std::fs::read(root.join(format!("{label}.FCStd"))).unwrap();
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            assert_valid_document(decoded.ir());
            let entities = &decoded.ir().model.sketch_entities;
            assert_eq!(entities.len(), 1, "{label}");
            let SketchGeometryDefinition::Nurbs { curve } = entities[0].geometry.definition()
            else {
                panic!("{label}: expected neutral B-spline");
            };
            assert_eq!(curve.degree(), 2);
            assert_eq!(curve.periodic(), periodic);
            let mut poles = vec![[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];
            poles.push(poles[0]);
            if periodic {
                poles.push(poles[1]);
            }
            let points: Vec<_> = curve
                .control_points()
                .iter()
                .map(|point| point.coordinates().map(cadmpeg_ir::scalar::FiniteReal::get))
                .collect();
            assert_eq!(points, poles);
            assert_eq!(curve.pole_rows().weights(), None);
            let expected_knots: &[f64] = if periodic {
                &[-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
            } else {
                &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0]
            };
            assert_eq!(curve.knots().as_slice(), expected_knots);
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
            let raw_poles = curve.pole_rows().raw_points();
            let point = cadmpeg_ir::eval::nurbs_pcurve_uv(
                &ctx,
                curve.degree(),
                expected_knots,
                &raw_poles,
                None,
                0.0,
            )
            .unwrap()
            .coordinates()
            .map(cadmpeg_ir::scalar::FiniteReal::get);
            let expected = if periodic {
                [0.0, -10.0]
            } else {
                [-10.0, -10.0]
            };
            for (actual, expected) in point.into_iter().zip(expected) {
                assert!((actual - expected).abs() < POINT_TOLERANCE, "{label}");
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
#[ignore = "extrusion support surfaces are procedural; cadmpeg-measure does not measure them"]
fn recipe_sketch_bspline_body_measures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (variant, periodic, volume, height) in [
            ("a", true, 5000.0 / 3.0, 5.0),
            ("b", true, 2000.0, 6.0),
            ("c", false, 4750.0 / 3.0, 5.0),
        ] {
            let label = format!("recipe_{version}_N04{variant}");
            let bytes = std::fs::read(root.join(format!("{label}.FCStd"))).unwrap();
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let report = measured(&label, decoded.ir()).unwrap();
            let bodies: Vec<_> = report
                .bodies()
                .iter()
                .filter(|body| {
                    body.volume()
                        .as_ref()
                        .and_then(|value| value.measured())
                        .is_some()
                })
                .collect();
            assert!(!bodies.is_empty(), "{label}: measured body");
            for body in bodies {
                let actual = body
                    .volume()
                    .as_ref()
                    .unwrap()
                    .measured()
                    .unwrap()
                    .value()
                    .value
                    .get();
                assert!(
                    (actual - volume).abs() < VOLUME_TOLERANCE,
                    "{label}: volume {actual}"
                );
                if !periodic {
                    assert!((actual - 1583.33).abs() < VOLUME_TOLERANCE);
                }
                let bbox = body.bbox().measured().unwrap().value();
                for (actual, expected) in bbox
                    .min
                    .coordinates()
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
                    .into_iter()
                    .chain(
                        bbox.max
                            .coordinates()
                            .map(cadmpeg_ir::scalar::FiniteReal::get),
                    )
                    .zip([-10.0, -10.0, 0.0, 10.0, 10.0, height])
                {
                    assert!(
                        (actual - expected).abs() < BOX_TOLERANCE,
                        "{label}: bbox {actual}"
                    );
                }
            }
        }
    }
}

fn sketch_document(knots: &str, poles: &str, periodic: &str) -> String {
    format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
<Geometry type="Part::GeomBSplineCurve"><BSplineCurve PolesCount="4" KnotsCount="5" Degree="2" IsPeriodic="{periodic}">
{poles}{knots}</BSplineCurve></Geometry></GeometryList></Property>
</Properties></Object></ObjectData></Document>"#
    )
}

const PERIODIC_POLES: &str = r#"<Pole X="-10" Y="-10" Z="0" Weight="1"/>
<Pole X="10" Y="-10" Z="0" Weight="2"/>
<Pole X="10" Y="10" Z="0" Weight="1"/>
<Pole X="-10" Y="10" Z="0" Weight="1"/>"#;
const PERIODIC_KNOTS: &str = r#"<Knot Value="0" Mult="1"/><Knot Value="1" Mult="1"/>
<Knot Value="2" Mult="1"/><Knot Value="3" Mult="1"/><Knot Value="4" Mult="1"/>"#;

#[test]
fn sketch_bspline_rational_periodic_weights_and_point() {
    let xml = sketch_document(PERIODIC_KNOTS, PERIODIC_POLES, "1");
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_valid_document(result.ir());
    let SketchGeometryDefinition::Nurbs { curve } =
        result.ir().model.sketch_entities[0].geometry.definition()
    else {
        panic!("neutral rational curve");
    };
    let weights = curve.pole_rows().weights().unwrap();
    assert_eq!(weights, [1.0, 2.0, 1.0, 1.0, 1.0, 2.0]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let point = cadmpeg_ir::eval::nurbs_pcurve_uv(
        &ctx,
        2,
        curve.knots().as_slice(),
        &curve.pole_rows().raw_points(),
        Some(&weights),
        0.0,
    )
    .unwrap()
    .coordinates()
    .map(cadmpeg_ir::scalar::FiniteReal::get);
    assert!((point[0] - 10.0 / 3.0).abs() < POINT_TOLERANCE);
    assert!((point[1] + 10.0).abs() < POINT_TOLERANCE);
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_bspline_malformed_counts_fail() {
    let xml = sketch_document(PERIODIC_KNOTS, PERIODIC_POLES, "1");
    for invalid in [
        xml.replace("PolesCount=\"4\"", "PolesCount=\"5\""),
        xml.replace("PolesCount=\"4\"", "PolesCount=\"invalid\""),
        xml.replace("Value=\"2\" Mult=\"1\"", "Value=\"2\" Mult=\"0\""),
        xml.replace("KnotsCount=\"5\"", "KnotsCount=\"6\""),
        xml.replace("Value=\"2\" Mult=\"1\"", "Value=\"2\" Mult=\"2\""),
        xml.replace("Value=\"4\" Mult=\"1\"", "Value=\"4\" Mult=\"2\""),
        sketch_document(PERIODIC_KNOTS, PERIODIC_POLES, "0"),
    ] {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(&invalid)),
                &DecodeOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn sketch_bspline_unprojected_form_has_native_loss() {
    for xml in [
        sketch_document(
            PERIODIC_KNOTS,
            &PERIODIC_POLES.replace("Z=\"0\"", "Z=\"1e-300\""),
            "1",
        ),
        sketch_document(
            PERIODIC_KNOTS,
            &PERIODIC_POLES.replace("Z=\"0\"", "Z=\"1\""),
            "1",
        ),
        sketch_document(
            &PERIODIC_KNOTS
                .replace("Value=\"0\"", "Value=\"-1e308\"")
                .replace("Value=\"4\"", "Value=\"1e308\""),
            PERIODIC_POLES,
            "1",
        ),
    ] {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_valid_document(result.ir());
        assert!(matches!(
            result.ir().model.sketch_entities[0].geometry.definition(),
            SketchGeometryDefinition::Native { .. }
        ));
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "sketch.nurbs-native"));
    }
}

#[test]
fn sketch_bspline_keeps_near_unit_weights() {
    let xml = sketch_document(
        PERIODIC_KNOTS,
        &PERIODIC_POLES.replace("Weight=\"2\"", "Weight=\"1.0000000000000002\""),
        "1",
    );
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let SketchGeometryDefinition::Nurbs { curve } =
        result.ir().model.sketch_entities[0].geometry.definition()
    else {
        panic!("neutral rational curve");
    };
    assert_eq!(
        curve.pole_rows().weights(),
        Some(vec![
            1.0,
            1.0 + f64::EPSILON,
            1.0,
            1.0,
            1.0,
            1.0 + f64::EPSILON
        ])
    );
    assert_valid_document(result.ir());
}
