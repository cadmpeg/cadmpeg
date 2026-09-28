// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]
use cadmpeg_test_support::edit;

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;

use crate::test_support::test_drawing_and_trimming::test_surface_domains::alternate_asymmetric_parameter_domain_surface_file;
use crate::test_support::test_drawing_and_trimming::test_surface_domains::asymmetric_parameter_domain_surface_file;
use crate::test_support::test_drawing_and_trimming::test_surface_domains::subrange_nurbs_surface_file;
use crate::test_support::test_owned::owned_test_file_with_global_and_line_fonts;
use crate::test_support::test_owned::OwnedTestEntity;

use crate::test_support::test_surface_fixtures::degree_zero_nurbs_surface_file;
use crate::test_support::test_surface_fixtures::multispan_degree_zero_nurbs_surface_file;
use crate::test_support::test_surface_fixtures::nurbs_surface_file;
use crate::test_support::test_surface_fixtures::offset_cylinder_file;
use crate::test_support::test_surface_fixtures::offset_plane_file;
use crate::test_support::test_surface_fixtures::offset_plane_file_with_indicator;

use crate::IgesCodec;

use super::super::homogeneous_curve_boundary_matches;
use super::type128_surface_with_closure;

#[test]
fn decode_accepts_asymmetric_nurbs_surface_parameter_domains() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(asymmetric_parameter_domain_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_permuted_nurbs_surface_parameter_domains() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(alternate_asymmetric_parameter_domain_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("u parameter range")));
}

#[test]
fn decode_retains_nurbs_surface_parameter_subranges() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(subrange_nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            result
                .ir()
                .model
                .procedural_surface_owner(&surface.id)
                .map(SurfaceId::as_str)
                == Some("iges:model:surface#D1")
        })
        .expect("Type 128 parameter-domain record");
    assert_eq!(
        procedural
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get),
        Some([Some(0.2), Some(0.8), Some(-1.0), Some(1.0)])
    );
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Exact(definition_payload) =
        procedural.definition()
    else {
        panic!("expected exact Type 128 construction")
    };
    let (cadmpeg_ir::geometry::ExactSpline::Legacy { ranges, .. },) =
        (definition_payload.spline(),)
    else {
        panic!("expected exact Type 128 construction")
    };

    assert_eq!(
        cadmpeg_ir::scalar::FiniteReal::raw_grid(*ranges),
        [[0.2, 0.8], [-1.0, 1.0]]
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_signed_analytic_offset_surfaces() {
    for (indicator_z, expected_z) in [(1.0, 2.0), (-1.0, -2.0)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_plane_file(indicator_z, 2.0)),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "iges:model:surface#D3")
            .unwrap();
        let SolvedSurfaceGeometry::Plane(plane_surface) = *offset
            .geometry
            .solved_cache()
            .expect("solved offset carrier")
        else {
            panic!("expected an exact plane offset carrier");
        };
        let origin = plane_surface.origin().get();
        assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, expected_z));
        assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset(definition_payload) =
            result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected an offset dependency");
        };
        let distance = definition_payload.distance();
        assert_eq!(distance.get(), expected_z);
        assert!(result.report().losses.is_empty());
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_uses_the_cylinder_normal_at_the_designated_parameters() {
    for (indicator_x, expected_radius) in [(1.0, 12.0), (-1.0, 8.0)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_cylinder_file(indicator_x)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "iges:model:surface#D7")
            .expect("offset cylinder");
        let SolvedSurfaceGeometry::Cylinder(cylinder_surface) = *surface
            .geometry
            .solved_cache()
            .expect("solved offset cylinder")
        else {
            panic!("expected cylindrical offset carrier")
        };
        let radius = cylinder_surface.radius().get();
        assert_eq!(radius, expected_radius);
        assert!(
            result.report().losses.is_empty(),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_applies_declared_real_significance_to_offset_surface_indicators() {
    for (components, decoded) in [
        (("0", "0", ".9999995"), true),
        (("0", "0", ".99999949"), false),
        (("0", "0", ".9999999D0"), false),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_plane_file_with_indicator(
                    components.0,
                    components.1,
                    components.2,
                    2.0,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id.as_str() == "iges:model:surface#D3");
        assert_eq!(offset, decoded, "{components:?}");
        if !decoded {
            assert!(result.report().losses.iter().any(|loss| {
                loss.message
                    .contains("offset indicator is not a unit vector")
                    || loss.message.contains("not the support normal")
            }));
        }
    }
}

#[test]
fn decode_rejects_a_unit_offset_indicator_that_is_not_the_designated_normal() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(offset_plane_file_with_indicator(".6", ".8", "0", 2.0)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "iges:model:surface#D3"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("not the support normal")));
}

#[test]
fn decode_projects_a_bspline_surface_with_u_major_control_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((nurbs.u_degree(), nurbs.v_degree()), (1, 1));
    assert_eq!((nurbs.u_count(), nurbs.v_count()), (2, 2));
    assert_eq!(
        nurbs.poles(),
        [
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ]
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(nurbs, 0.25, 0.75)
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_degree_zero_bspline_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(degree_zero_nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (0, 0));
    assert_eq!((surface.u_count(), surface.v_count()), (1, 1));
    assert_eq!(surface.u_knots().as_slice(), [0.0, 1.0]);
    assert_eq!(surface.v_knots().as_slice(), [0.0, 1.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75)
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(1.0, 2.0, 3.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_multispan_degree_zero_bspline_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(multispan_degree_zero_nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (0, 0));
    assert_eq!((surface.u_count(), surface.v_count()), (2, 1));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5)
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 1.5, 0.5)
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(4.0, 5.0, 6.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_enforces_type128_closure_flags_in_iges_4_and_5_0() {
    for global in [
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,6,0;".as_slice(),
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;".as_slice(),
    ] {
        let invalid = IgesCodec
            .decode(
                &mut Cursor::new(type128_surface_with_closure(
                    global,
                    1,
                    0,
                    "0,0,0,1,0,0,0,1,0,1,0,0",
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(invalid.ir().model.surfaces.is_empty());
        assert!(invalid
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("U-closed surface flag")));

        for (closed_u, closed_v, poles) in [
            (1, 0, "0,0,0,0,0,0,0,1,0,0,1,0"),
            (0, 1, "0,0,0,1,0,0,0,0,0,1,0,0"),
        ] {
            let valid = IgesCodec
                .decode(
                    &mut Cursor::new(type128_surface_with_closure(
                        global, closed_u, closed_v, poles,
                    )),
                    &DecodeOptions::default(),
                )
                .unwrap();
            assert_eq!(valid.ir().model.surfaces.len(), 1);
            assert!(!valid
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("closed surface")));
        }
    }
}

#[test]
fn rational_boundary_comparison_accepts_projectively_scaled_curves() {
    let first = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        Some(vec![1.0, 1.0]),
        false,
    )
    .expect("valid rational boundary");
    let mut scaled = first.clone();
    let scaled_poles = cadmpeg_ir::geometry::nurbs::NurbsPoles3::from_lanes(
        scaled.pole_rows().raw_points(),
        Some(vec![2.0; scaled.pole_count()]),
    )
    .unwrap();
    {
        let replacement = scaled_poles;
        edit::replace(&mut scaled, |previous| {
            cadmpeg_ir::geometry::nurbs::NurbsCurve::new(
                previous.degree(),
                previous.knots().to_vec(),
                replacement,
                previous.periodic(),
            )
        })
    }
    .unwrap();
    assert_eq!(
        homogeneous_curve_boundary_matches(None, &first, &scaled, [0.0, 1.0], 0.0).unwrap(),
        Some(true)
    );

    let mut scaled_index = 0usize;
    scaled
        .edit_control_points(|point| {
            if scaled_index == 1 {
                point.x = 1.1;
            }
            scaled_index += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        homogeneous_curve_boundary_matches(None, &first, &scaled, [0.0, 1.0], 0.0).unwrap(),
        Some(false)
    );
}

#[test]
fn decode_applies_rational_surface_weight_declaration_in_iges_4_and_5_0() {
    for global in [
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,6,0;".as_slice(),
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;".as_slice(),
    ] {
        for (weights, projected, expected_message) in [
            (
                "1,1,1,1",
                false,
                "rational surface has equal weights but PROP3 declares rational",
            ),
            ("1,0.99,1,1", true, ""),
        ] {
            let parameters = format!(
                "128,1,1,1,1,0,0,0,0,0,0,0,1,1,0,0,1,1,{weights},0,0,0,1,0,0,1,0,1,1,0,1,0,1,0,1;"
            );
            let result = IgesCodec
                .decode(
                    &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                        &[OwnedTestEntity {
                            entity_type: 128,
                            form: 0,
                            label: "SURFACE".into(),
                            status: "00000000",
                            parameters,
                        }],
                        global,
                        &[(1, 1)],
                    )),
                    &DecodeOptions::default(),
                )
                .unwrap();

            assert_eq!(result.ir().model.surfaces.len(), usize::from(projected));
            if projected {
                let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
                    result.ir().model.surfaces[0].geometry.solved()
                else {
                    panic!("expected a NURBS surface carrier");
                };
                assert_eq!(
                    surface.pole_grid().weights().map(|rows| rows.concat()),
                    Some(vec![1.0, 1.0, 0.99, 1.0])
                );
            } else {
                assert!(result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.message.contains(expected_message)));
            }
        }
    }
}

#[test]
fn a_ruled_weight_lane_shorter_than_its_pole_lane_reaches_the_codec_error() {
    let rail = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("valid rail");
    let weight = cadmpeg_ir::scalar::NonZeroReal::try_from(0.5).expect("nonzero weight");
    let error = super::super::same_basis_ruled_surface(&rail, &rail, &[weight], None)
        .expect_err("a weight lane one shorter than the pole lane is refused");
    let reported = error;
    let CodecError::Malformed(message) = &reported else {
        panic!("expected a malformed refusal, got {reported:?}");
    };
    assert!(
        message.contains("pole(s) against"),
        "the refusal states both lane counts: {message}"
    );
}

#[test]
fn numerical_audit_similarity_orientation_survives_uniform_scale() {
    for scale in [f64::from_bits(1), 1.0e-110, 1.0, 1.0e110, f64::MAX] {
        for orientation in [-1.0, 1.0] {
            let transform = cadmpeg_ir::transform::Transform::affine([
                [scale * orientation, 0.0, 0.0, 0.0],
                [0.0, scale, 0.0, 0.0],
                [0.0, 0.0, scale, 0.0],
            ])
            .unwrap();
            assert_eq!(
                super::super::similarity_orientation(transform),
                Some(orientation)
            );
        }
    }
    let skew = cadmpeg_ir::transform::Transform::affine([
        [1.0e110, 1.0e110, 0.0, 0.0],
        [0.0, 1.0e110, 0.0, 0.0],
        [0.0, 0.0, 1.0e110, 0.0],
    ])
    .unwrap();
    assert_eq!(super::super::similarity_orientation(skew), None);
}

#[test]
fn numerical_followup_closure_uses_every_span_control_and_weight_scale() {
    let knots = vec![0., 0., 0., 1., 1., 1., 2., 2., 2.];
    let first = (0..6)
        .map(|i| Point3::new(f64::from(i), 0., 0.))
        .collect::<Vec<_>>();
    let mut second = first.clone();
    second[5].y = 4.;
    let a = NurbsCurve::from_lanes(2, knots.clone(), first, None, false).unwrap();
    let b = NurbsCurve::from_lanes(2, knots, second, None, false).unwrap();
    assert_eq!(
        homogeneous_curve_boundary_matches(None, &a, &b, [0., 2.], 0.).unwrap(),
        Some(false)
    );
    for weight in [1., 1e-200, 1e200] {
        let a = NurbsCurve::from_lanes(
            1,
            vec![0., 0., 1., 1.],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
            Some(vec![weight; 2]),
            false,
        )
        .unwrap();
        let b = NurbsCurve::from_lanes(
            1,
            vec![0., 0., 1., 1.],
            vec![Point3::new(0., 2., 0.), Point3::new(1., 2., 0.)],
            Some(vec![weight; 2]),
            false,
        )
        .unwrap();
        assert_eq!(
            homogeneous_curve_boundary_matches(None, &a, &b, [0., 1.], 0.001).unwrap(),
            Some(false)
        );
        assert_eq!(
            homogeneous_curve_boundary_matches(None, &a, &a, [0., 1.], 0.).unwrap(),
            Some(true)
        );
    }
}

#[test]
fn numerical_followup_ruled_rails_align_across_overflowing_knot_domains() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::Point3;
    let line = |domain: [f64; 2], y| {
        NurbsCurve::from_lanes(
            1,
            vec![domain[0], domain[0], domain[1], domain[1]],
            vec![Point3::new(0., y, 0.), Point3::new(1., y, 0.)],
            None,
            false,
        )
        .unwrap()
    };
    let first = line([-1e308, 1e308], 0.);
    let second = NurbsCurve::from_lanes(
        1,
        vec![0., 0., 0.5, 1., 1.],
        vec![
            Point3::new(0., 1., 0.),
            Point3::new(0.5, 1., 0.),
            Point3::new(1., 1., 0.),
        ],
        None,
        false,
    )
    .unwrap();
    let pairs = super::super::aligned_homogeneous_spans(None, &first, &second)
        .unwrap()
        .unwrap();
    assert_eq!(pairs.len(), 2);
    for (index, (a, b)) in pairs.into_iter().enumerate() {
        assert_eq!(a.controls.len(), 2);
        assert_eq!(b.controls.len(), 2);
        for (pole, expected) in a
            .controls
            .iter()
            .zip([0.5 * index as f64, 0.5 * (index + 1) as f64])
        {
            assert!((pole[0] / pole[3] - expected).abs() < 16. * f64::EPSILON);
        }
    }
}
