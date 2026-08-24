// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

#[test]
fn decode_form_zero_classifies_from_coefficients_in_v4_and_v5_profiles() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let families = [
        ("ellipse", "104,0.25,0,1,0,0,-1,0,2,0,0,1;", 0),
        (
            "hyperbola",
            "104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;",
            1,
        ),
        ("parabola", "104,1,0,0,0,-4,0,0,2,1,-2,1;", 2),
    ];

    for (version, global) in [("4.0", &global_v4[..]), ("5.0", &global_v5[..])] {
        for (family, parameters, family_number) in families {
            let result = IgesCodec
                .decode(
                    &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                        &[OwnedTestEntity {
                            entity_type: 104,
                            form: 0,
                            label: "CONIC".into(),
                            status: "00000000",
                            parameters: parameters.into(),
                        }],
                        global,
                        &[(1, 1)],
                    )),
                    &DecodeOptions::default(),
                )
                .unwrap();

            assert_eq!(
                result.ir().source.as_ref().unwrap().attributes["iges_version"],
                version,
                "{family}"
            );
            assert_eq!(result.ir().model.curves.len(), 1, "{version} {family}");
            assert!(
                matches!(
                    (&result.ir().model.curves[0].geometry, family_number),
                    (cadmpeg_ir::geometry::CurveGeometry::Ellipse { .. }, 0)
                        | (cadmpeg_ir::geometry::CurveGeometry::Hyperbola { .. }, 1)
                        | (cadmpeg_ir::geometry::CurveGeometry::Parabola { .. }, 2)
                ),
                "{version} {family}: {:?}",
                result.ir().model.curves[0].geometry
            );
            assert!(
                result
                    .report()
                    .losses
                    .iter()
                    .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
                "{version} {family}: {:#?}",
                result.report().losses
            );
            assert_eq!(
                result
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code == IgesLossCode::SourceDialectUnverified.kind())
                    .count(),
                1,
                "{version} {family}: {:#?}",
                result.report().losses
            );
        }
    }
}

#[test]
fn decode_classifies_and_bounds_all_standard_conic_arc_families() {
    let fixtures: [(i64, &[u8]); 5] = [
        (0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        (1, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        (
            2,
            b"104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;",
        ),
        (3, b"104,1,0,0,0,-4,0,0,2,1,-2,1;"),
        (3, b"104,0,0,1,-4,0,0,0,1,2,1,-2;"),
    ];
    for (form, parameters) in fixtures {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(conic_arc_file(form, parameters)),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), 1, "form {form}");
        assert_eq!(result.ir().model.edges.len(), 1, "form {form}");
        assert_eq!(result.ir().model.edges[0].tolerance, Some(0.001));
        assert!(result
            .ir()
            .model
            .vertices
            .iter()
            .all(|vertex| vertex.tolerance == Some(0.001)));
        match (&result.ir().model.curves[0].geometry, form) {
            (cadmpeg_ir::geometry::CurveGeometry::Ellipse { .. }, 0 | 1)
            | (cadmpeg_ir::geometry::CurveGeometry::Hyperbola { .. }, 2)
            | (cadmpeg_ir::geometry::CurveGeometry::Parabola { .. }, 3) => {}
            (geometry, _) => panic!("unexpected form {form} geometry {geometry:?}"),
        }
        assert!(result.report().losses.is_empty(), "form {form}");
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(
            validation.is_ok(),
            "form {form}: {:#?}",
            validation.findings
        );
    }
}

#[test]
fn decode_brackets_conic_endpoint_agreement_at_the_global_resolution() {
    for (parameters, point_name, decoded) in [
        (
            b"104,0.25,0,1,0,0,-1,0,2.000999,0,0,1;".as_slice(),
            "start",
            true,
        ),
        (
            b"104,0.25,0,1,0,0,-1,0,2.001001,0,0,1;".as_slice(),
            "start",
            false,
        ),
        (
            b"104,0.25,0,1,0,0,-1,0,2,0,0,1.000999;".as_slice(),
            "terminate",
            true,
        ),
        (
            b"104,0.25,0,1,0,0,-1,0,2,0,0,1.001001;".as_slice(),
            "terminate",
            false,
        ),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(conic_arc_file(1, parameters)),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.curves.len(),
            usize::from(decoded),
            "{point_name}"
        );
        assert_eq!(
            result.ir().model.edges.len(),
            usize::from(decoded),
            "{point_name}"
        );
        if decoded {
            assert!(result.report().losses.is_empty(), "{point_name}");
        } else {
            assert_eq!(result.report().losses.len(), 1, "{point_name}");
            assert_eq!(
                result.report().losses[0].code,
                IgesLossCode::EntityNotProjected.kind(),
                "{point_name}"
            );
        }
    }
}

#[test]
fn decode_rejects_a_conic_endpoint_at_exact_global_resolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(conic_arc_file(3, b"104,1,0,0,0,4,0,0,0.001,1,-0.25;")),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityNotProjected.kind()
    );
}

#[test]
fn decode_retains_declared_conic_endpoints_after_carrier_validation() {
    const EPS_ENDPOINT_STORAGE: f64 = 1.0e-12;

    let result = IgesCodec
        .decode(
            &mut Cursor::new(conic_arc_file(
                1,
                b"104,0.25,0,1,0,0,-1,0,2.0005,0,0,1.0005;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.report().losses.is_empty());
    assert_eq!(result.ir().model.points.len(), 2);
    let start = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.position.x > 2.0)
        .expect("decoded start endpoint");
    let end = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.position.y > 1.0)
        .expect("decoded terminate endpoint");
    assert!((start.position.x - 2.0005).abs() < EPS_ENDPOINT_STORAGE);
    assert!((end.position.y - 1.0005).abs() < EPS_ENDPOINT_STORAGE);
    let range = result.ir().model.edges[0]
        .param_range
        .expect("fixture has a bounded conic range");
    let geometry = &result.ir().model.curves[0].geometry;
    let evaluated_start = cadmpeg_ir::eval::curve_point(geometry, range[0])
        .expect("coefficient-defined carrier evaluates at its start");
    let evaluated_end = cadmpeg_ir::eval::curve_point(geometry, range[1])
        .expect("coefficient-defined carrier evaluates at its end");
    assert!(start.position.distance(evaluated_start) > 0.0);
    assert!(end.position.distance(evaluated_end) > 0.0);
    assert!(start.position.distance(evaluated_start) < 0.001);
    assert!(end.position.distance(evaluated_end) < 0.001);
}

#[test]
fn decode_applies_the_scale_relative_standard_position_gate() {
    for (cross_term, decoded) in [
        (super::CONIC_STANDARD_POSITION_RELATIVE_EPSILON, true),
        (
            super::CONIC_STANDARD_POSITION_RELATIVE_EPSILON * 1.0001,
            false,
        ),
    ] {
        let parameters = format!("104,0.25,{cross_term},1,0,0,-1,0,2,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(conic_arc_file(1, parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), usize::from(decoded));
        if decoded {
            assert!(result.report().losses.is_empty());
        } else {
            assert_eq!(result.report().losses.len(), 1);
            assert_eq!(
                result.report().losses[0].code,
                IgesLossCode::EntityNotProjected.kind()
            );
            assert!(result.report().losses[0]
                .message
                .contains("standard position"));
        }
    }
}

#[test]
fn decode_canonicalizes_ellipse_arc_seam_noise() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(conic_arc_file(
                0,
                b"104,0.25,0,1,0,0,-1,0,2,-0.0000000000001,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir().model.edges[0].param_range.map(|range| range[0]),
        Some(0.0)
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
