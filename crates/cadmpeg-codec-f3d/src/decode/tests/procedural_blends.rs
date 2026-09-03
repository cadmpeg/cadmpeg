// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::Severity;

use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn generated_g2_blend_surfaces_decode_both_singularity_branches() {
    use cadmpeg_ir::geometry::{G2BlendFirstShape, LoftBridgeToken, ProceduralSurfaceDefinition};

    for name in ["g2_blend_spl_sur", "g2blnsur"] {
        for full in [true, false] {
            let result = F3dCodec
                .decode(
                    &mut Cursor::new(f3d_with_smbh(&synthetic_g2_blend_spl_sur_smbh(name, full))),
                    &DecodeOptions::default(),
                )
                .expect("G2 blend decode");
            let ProceduralSurfaceDefinition::G2Blend { construction } =
                &result.ir().model.procedural_surfaces[0].definition()
            else {
                panic!("expected G2 blend")
            };
            assert_eq!(construction.first.label, "first");
            assert_eq!(construction.second.label, "second");
            assert_eq!(construction.singularity, if full { 11 } else { 12 });
            assert_eq!(construction.center_parameters, [-0.5, 1.5]);
            assert_eq!(construction.parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
            assert_eq!(construction.trailing_parameters, [0.1, 0.2, 0.3, 0.4]);
            assert_eq!(
                construction.discontinuities,
                [vec![0.25], vec![], vec![0.5, 0.75]]
            );
            match &construction.first_shape {
                G2BlendFirstShape::Full {
                    support: Some(support),
                } if full => {
                    assert_eq!(support.tolerance, 0.02);
                }
                G2BlendFirstShape::None {
                    coefficients,
                    tolerance,
                    extension,
                    pcurve,
                } if !full => {
                    assert_eq!(*coefficients, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
                    assert_eq!(*tolerance, 0.03);
                    assert_eq!(*extension, Some(LoftBridgeToken::Integer(44)));
                    assert!(pcurve.is_some());
                }
                _ => panic!("wrong G2 singularity payload"),
            }
            let side_curves = [
                construction.first.curve.clone(),
                construction.second.curve.clone(),
            ];
            let center_curve = construction.center_curve.clone();

            let (mut source_less, _, _) = result.into_parts();
            source_less.source = None;
            source_less.set_native_unknowns("f3d", &[]).unwrap();
            for (ordinal, side) in side_curves.into_iter().enumerate() {
                source_less
                    .model
                    .curves
                    .iter_mut()
                    .find(|curve| curve.id == side)
                    .expect("G2 side curve")
                    .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -1.0),
                    direction: cadmpeg_ir::math::Vector3::new(3.0, -2.0, 4.0),
                };
            }
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == center_curve)
                .expect("G2 center curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-2.0, 1.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -3.0, 2.0),
            };
            let mut encoded = Vec::new();
            F3dCodec
                .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
                .and_then(|plan| plan.write_to(&mut encoded))
                .expect("source-less G2 encode");
            let round_trip = F3dCodec
                .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
                .expect("source-less G2 round trip");
            let ProceduralSurfaceDefinition::G2Blend { construction } =
                &round_trip.ir().model.procedural_surfaces[0].definition()
            else {
                panic!("expected round-trip G2 blend")
            };
            assert_eq!(construction.singularity, if full { 11 } else { 12 });
            assert_eq!(construction.center_parameters, [-0.5, 1.5]);
            assert_eq!(construction.parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
            assert_eq!(construction.discontinuities[2], [0.5, 0.75]);
            assert_eq!(
                matches!(construction.first_shape, G2BlendFirstShape::Full { .. }),
                full
            );
            for side in [&construction.first, &construction.second] {
                assert!(matches!(
                    round_trip
                        .ir()
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == side.curve)
                        .map(|curve| &curve.geometry),
                    Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                        if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
                ));
            }
            assert!(matches!(
                round_trip
                    .ir()
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == construction.center_curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1
                        && curve.knots == [-0.5, -0.5, 1.5, 1.5]
            ));
        }
    }
}

#[test]
fn generated_rolling_ball_and_sss_blends_decode_full_native_graphs() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, RollingBallRadiusSelector};

    for name in [
        "rb_blend_spl_sur",
        "rbblnsur",
        "pipe_spl_sur",
        "pipesur",
        "sss_blend_spl_sur",
        "sssblndsur",
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_rolling_ball_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("rolling-ball decode");
        let ProceduralSurfaceDefinition::Blend {
            native: Some(native),
            ..
        } = &result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected complete rolling-ball graph")
        };
        assert_eq!(native.definition_index, 22507);
        assert_eq!(
            native.sides[0].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Surface
        );
        assert_eq!(
            native.sides[1].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Curve
        );
        assert_eq!(
            native.sides[0].location,
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
        );
        assert!(native.sides.iter().all(|side| side.surface.is_some()));
        assert!(native.sides.iter().all(|side| side.pcurve.is_some()));
        assert_eq!(native.sides[0].extension, Some(3));
        assert_eq!(native.sides[1].extension, Some(4));
        assert_eq!(native.offsets, [-3.0, -6.0]);
        assert_eq!(native.radius_selector, RollingBallRadiusSelector::None);
        assert_eq!(native.u_range, [Some(-1.0), Some(2.0)]);
        assert_eq!(native.v_range, [None, None]);
        assert_eq!(native.shape_prefix, 1);
        assert_eq!(native.parameters, [0.1, 0.2]);
        assert_eq!(native.tail, 17);
        assert_eq!(native.cache.selector(), 0);
        assert_eq!(native.cache.parameterization(), None);
        assert_eq!(
            native.discontinuities,
            expected_revision_surface_tail_discontinuities()
        );
        assert!(!native.tail_flag);
        assert_eq!(native.tail_extensions, [11, 12, 13]);
        assert_eq!(native.third.is_some(), name.starts_with("sss"));
        if let Some(third) = &native.third {
            assert_eq!(third.label, "third");
            assert_eq!(third.extension, 23);
            assert!(third.secondary_pcurve.is_some());
            assert!(!third.flag);
        }

        let expected = native.clone();
        let side_curves = native
            .sides
            .iter()
            .map(|side| side.curve.clone())
            .collect::<Vec<_>>();
        let third_curve = native.third.as_ref().map(|third| third.curve.clone());
        let slice_curve = native.slice.clone();
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, side) in side_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| Some(&curve.id) == side.as_ref())
                .expect("rolling-ball side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 3.0, -2.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -1.0, 2.0),
            };
        }
        if let Some(third) = &third_curve {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == *third)
                .expect("rolling-ball third-side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(3.0, 4.0, -2.0),
            };
        }
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == slice_curve)
            .expect("rolling-ball slice curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, -3.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less rolling-ball encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less rolling-ball round trip");
        let ProceduralSurfaceDefinition::Blend {
            native: Some(actual),
            ..
        } = &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected complete round-trip rolling-ball graph")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        for side in actual.sides.iter() {
            assert!(matches!(
                round_trip
                    .ir()
                    .model
                    .curves
                    .iter()
                    .find(|curve| Some(&curve.id) == side.curve.as_ref())
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        if let Some(third) = &actual.third {
            assert!(matches!(
                round_trip
                    .ir()
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == third.curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == actual.slice)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [-1.0, -1.0, 2.0, 2.0]
        ));
    }
}

/// Tail form `2` stores no cache and no fit tolerance: it stores the U
/// parameter interval, the V parameter interval, and the U closure, V closure,
/// U singularity, and V singularity enums. The carrier fields that follow the
/// tail decode at their stored values only when the tail is framed exactly.
#[test]
fn parameterized_tail_form_decodes_in_every_blend_carrier() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_variable_blend_smbh_with_tail_form("var_blend_spl_sur", 2),
            )),
            &DecodeOptions::default(),
        )
        .expect("parameterized variable-blend decode");
    let procedural = &decoded.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let ProceduralSurfaceDefinition::VariableBlend { construction } = procedural.definition()
    else {
        panic!("expected variable-blend construction")
    };
    assert_eq!(construction.cache.selector(), 2);
    assert_eq!(
        construction.cache.parameterization(),
        Some(&expected_revision_surface_tail_parameterization())
    );
    // Fields after the tail; a misframed tail shifts every one of them.
    assert_eq!(construction.tail_extensions, [31, 32, 33]);
    assert_eq!(construction.post_range, [Some(0.0), Some(1.0)]);

    for name in ["rb_blend_spl_sur", "sss_blend_spl_sur"] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_rolling_ball_with_tail_smbh(
                    name, 2,
                ))),
                &DecodeOptions::default(),
            )
            .expect("parameterized rolling-ball decode");
        let procedural = &decoded.ir().model.procedural_surfaces[0];
        assert_eq!(procedural.cache_fit_tolerance(), None);
        let ProceduralSurfaceDefinition::Blend {
            native: Some(native),
            ..
        } = procedural.definition()
        else {
            panic!("expected complete rolling-ball graph")
        };
        assert_eq!(native.cache.selector(), 2);
        assert_eq!(
            native.cache.parameterization(),
            Some(&expected_revision_surface_tail_parameterization())
        );
        assert_eq!(
            native.discontinuities,
            expected_revision_surface_tail_discontinuities()
        );
        assert!(!native.tail_flag);
        assert_eq!(native.third.is_some(), name.starts_with("sss"));
        // Fields after the tail; a misframed tail shifts every one of them.
        assert_eq!(native.tail_extensions, [11, 12, 13]);
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_versioned_cyl_spl_sur_with_tail_smbh(2),
            )),
            &DecodeOptions::default(),
        )
        .expect("parameterized extrusion decode");
    let procedural = &decoded.ir().model.procedural_surfaces[0];
    // Form `2` stores no cache and no fit tolerance. The surface block inside
    // the directrix scope is not this record's cache and its trailing scalar is
    // not this record's fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let ProceduralSurfaceDefinition::Extrusion {
        parameter_interval: Some([0.25, 0.75]),
        revision_form: Some(form),
        ..
    } = procedural.definition()
    else {
        panic!("expected a parameterized revision-gated extrusion")
    };
    assert_eq!(form.cache.selector(), 2);
    assert_eq!(
        form.cache.parameterization(),
        Some(&expected_revision_surface_tail_parameterization())
    );
    assert_eq!(
        form.discontinuities,
        expected_revision_surface_tail_discontinuities()
    );
    assert!(!form.tail_flag);
}

#[test]
fn stale_variable_blend_cache_yields_to_the_construction_carrier() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    let current = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_variable_blend_smbh_with_cache_state("var_blend_spl_sur", 1),
            )),
            &DecodeOptions::default(),
        )
        .expect("current variable-blend cache");
    let current_procedural = &current.ir().model.procedural_surfaces[0];
    let current_carrier = current
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == current_procedural.surface)
        .expect("current variable-blend carrier");
    assert!(matches!(
        current_carrier.geometry,
        SurfaceGeometry::Nurbs(_)
    ));
    assert!(current_procedural.cache_fit_tolerance().is_some());

    let stale = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_variable_blend_smbh_with_cache_state("var_blend_spl_sur", 0),
            )),
            &DecodeOptions::default(),
        )
        .expect("stale variable-blend cache");
    let stale_procedural = &stale.ir().model.procedural_surfaces[0];
    let stale_carrier = stale
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == stale_procedural.surface)
        .expect("stale variable-blend carrier");
    assert!(matches!(
        stale_carrier.geometry,
        SurfaceGeometry::Procedural { .. }
    ));
    assert_eq!(stale_procedural.cache_fit_tolerance(), None);
    assert!(matches!(
        stale_procedural.definition(),
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction.shape_prefix == 0
                && matches!(
                    construction.cache,
                    cadmpeg_ir::geometry::RevisionCacheForm::SolvedCache {
                        fit_tolerance: cadmpeg_ir::geometry::VariableBlendSolvedCache::Stale
                    }
                )
    ));
    assert!(!cadmpeg_ir::validate_neutral(stale.ir(), Vec::new())
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Error));
}

/// Tail form `2` stores no solved cache, so a blend record carrying it owns
/// every surface block it holds as a construction support and rests on its
/// procedural carrier. Source-less generation writes the construction back from
/// the stored parameterization rather than needing a cache it never had.
#[test]
fn parameterized_blend_tails_round_trip_source_less_generation() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for stream in [
        synthetic_full_rolling_ball_with_tail_smbh("rb_blend_spl_sur", 2),
        synthetic_variable_blend_smbh_with_tail_form("var_blend_spl_sur", 2),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&stream)),
                &DecodeOptions::default(),
            )
            .expect("parameterized blend decode");
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let expected = source_less.model.procedural_surfaces[0].clone();
        assert_eq!(expected.cache_fit_tolerance(), None);
        let carrier = source_less
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == expected.surface)
            .expect("blend surface carrier");
        assert!(matches!(
            carrier.geometry,
            SurfaceGeometry::Procedural { .. }
        ));

        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("parameterized blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("parameterized blend round trip");
        let actual = &round_trip.ir().model.procedural_surfaces[0];
        assert_eq!(actual.cache_fit_tolerance(), None);
        let (enumeration, parameterization) = match actual.definition() {
            ProceduralSurfaceDefinition::Blend {
                native: Some(native),
                ..
            } => (
                native.cache.selector(),
                native.cache.parameterization().cloned(),
            ),
            ProceduralSurfaceDefinition::VariableBlend { construction } => (
                construction.cache.selector(),
                construction.cache.parameterization().cloned(),
            ),
            other => panic!("expected a parameterized blend construction: {other:?}"),
        };
        assert_eq!(enumeration, 2);
        assert_eq!(
            parameterization,
            Some(expected_revision_surface_tail_parameterization())
        );
    }
}

#[test]
fn variable_blend_second_interval_decodes_unbounded_upper_bound() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    // The tail's second interval carries a lower bound with an
    // unbounded-above marker: `(T lo, F)` decodes to `[Some(lo), None]`.
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                false,
                None,
                [Some(-0.5), None],
            ))),
            &DecodeOptions::default(),
        )
        .expect("half-bounded second-interval decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.u_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(construction.v_range, [Some(-0.5), None]);
    assert_eq!(construction.shape_prefix, 11);
    assert_eq!(construction.shape_length, 6.0);
}

#[test]
fn generated_interp_radius_law_leaves_the_cross_section_enum_unconsumed() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendCrossSection, VariableBlendValuePayload,
    };

    // An `interp` payload ends at its last radius point. The enum that follows
    // is the record's cross-section selector; reading it as a trailing flag
    // costs the cross-section while leaving the byte count intact, so the
    // decoded cross-section is what pins the boundary.
    for (selector, expected) in [
        (Some(0), Some(VariableBlendCrossSection::Circular)),
        (
            Some(7),
            Some(VariableBlendCrossSection::G2Round {
                parameters: [2.0, 2.0],
            }),
        ),
        (None, None),
    ] {
        let smbh =
            synthetic_variable_blend_smbh_with_interp_radius("srf_srf_v_bl_spl_sur", selector);
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("interp variable-blend decode");
        let ProceduralSurfaceDefinition::VariableBlend { construction } =
            &result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected variable blend")
        };
        let VariableBlendValuePayload::Interpolated {
            function, points, ..
        } = &construction.first_value.payload
        else {
            panic!("expected interpolated radius law")
        };
        assert_eq!(points.len(), 1);
        let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } = function else {
            panic!("expected NURBS radius function")
        };
        assert_eq!(control_points[0], cadmpeg_ir::math::Point2::new(2.5, 0.5));
        assert_eq!(control_points[1], cadmpeg_ir::math::Point2::new(7.5, 1.5));
        assert_eq!(construction.cross_section, expected);

        assert_revision_surface_round_trip(smbh, "variable_blend");
    }
}

#[test]
fn generated_edge_offset_radius_law_reads_two_parameters_and_one_offset() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendValuePayload};

    // `edge_offset` without the leading sub-discriminator stores its law-domain
    // parameter range and one offset: the second field is a parameter, and only
    // the third is a length, so only the third takes the centimetre-to-
    // millimetre conversion.
    let smbh = synthetic_variable_blend_smbh_with_edge_offset_radius("srf_srf_v_bl_spl_sur");
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("edge-offset variable-blend decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected variable blend")
    };
    let VariableBlendValuePayload::EdgeOffset { scalars, lengths } =
        &construction.first_value.payload
    else {
        panic!("expected edge-offset radius law")
    };
    assert_eq!(scalars, &[0.25, 0.75]);
    assert_eq!(lengths, &[15.0]);

    assert_revision_surface_round_trip(smbh, "variable_blend");
}

#[test]
fn generated_variable_blends_decode_complete_single_radius_graphs() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendSurfaceSubtype, VariableBlendValuePayload,
    };

    for (name, subtype) in [
        (
            "var_blend_spl_sur",
            VariableBlendSurfaceSubtype::VariableBlend,
        ),
        ("varblendsplsur", VariableBlendSurfaceSubtype::VariableBlend),
        (
            "srf_srf_v_bl_spl_sur",
            VariableBlendSurfaceSubtype::SurfaceSurface,
        ),
        ("srfsrfblndsur", VariableBlendSurfaceSubtype::SurfaceSurface),
        (
            "crv_crv_v_bl_spl_sur",
            VariableBlendSurfaceSubtype::CurveCurve,
        ),
        ("crvcrvblndsur", VariableBlendSurfaceSubtype::CurveCurve),
        (
            "crv_srf_v_bl_spl_sur",
            VariableBlendSurfaceSubtype::CurveSurface,
        ),
        ("crvsrfblndsur", VariableBlendSurfaceSubtype::CurveSurface),
        (
            "sfcv_free_bl_spl_sur",
            VariableBlendSurfaceSubtype::SurfaceCurveFree,
        ),
        (
            "sfcvfreeblndsur",
            VariableBlendSurfaceSubtype::SurfaceCurveFree,
        ),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("variable-blend decode");
        let ProceduralSurfaceDefinition::VariableBlend { construction } =
            &result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected variable blend")
        };
        assert_eq!(construction.subtype, subtype);
        assert_eq!(construction.revision, 23100);
        assert_eq!(
            construction.sides[0].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Surface
        );
        assert_eq!(
            construction.sides[1].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Curve
        );
        assert_eq!(construction.sides[0].extension, Some(0));
        assert_eq!(construction.sides[1].extension, Some(5));
        assert_eq!(
            construction.sides[0].location,
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(construction.offsets, [-2.0, 4.0]);
        assert_eq!(
            construction.radius_kind,
            cadmpeg_ir::geometry::VariableBlendRadiusKind::SingleRadius
        );
        let VariableBlendValuePayload::TwoEnds { parameters, radii } =
            &construction.first_value.payload
        else {
            panic!("expected two-ends radius law")
        };
        assert!(construction.first_value.modern_flag);
        assert_eq!(construction.first_value.discriminator, 7);
        assert_eq!(construction.first_value.calibrated, 3);
        assert_eq!(*parameters, [0.25, 0.75]);
        assert_eq!(*radii, [15.0, 25.0]);
        assert_eq!(construction.slice_range, [None, None]);
        assert_eq!(construction.u_range, [Some(-1.0), Some(2.0)]);
        assert_eq!(construction.v_range, [None, None]);
        assert_eq!(construction.shape_prefix, 11);
        assert_eq!(construction.shape_length, 6.0);
        assert_eq!(construction.cache.selector(), 0);
        assert_eq!(
            construction.discontinuities,
            [
                vec![0.125],
                vec![],
                vec![0.25, 0.375],
                vec![],
                vec![0.5],
                vec![]
            ]
        );
        assert!(construction.tail_flag);
        assert_eq!(construction.tail_extensions, [31, 32, 33]);
        assert!(construction.secondary_curve.is_some());
        assert_eq!(construction.secondary_range, [None, None]);
        assert_eq!(
            construction.convexity,
            cadmpeg_ir::geometry::VariableBlendConvexity::Convex
        );
        assert_eq!(
            construction.render_mode,
            cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallSnapshot
        );
        assert_eq!(construction.post_range, [Some(0.0), Some(1.0)]);
        assert!(construction.post_curve.is_some());
        assert!(construction.post_pcurve.is_none());
        assert!(construction.sides.iter().all(|side| side.pcurve.is_some()));

        let expected = construction.clone();
        let post_curve = construction.post_curve.clone().expect("post curve");
        let slice_curve = construction.slice.clone();
        let side_curves = construction
            .sides
            .iter()
            .map(|side| side.curve.clone().expect("side curve"))
            .collect::<Vec<_>>();
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == post_curve)
            .expect("variable-blend post curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(-2.0, 1.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, -4.0, 2.0),
        };
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == slice_curve)
            .expect("variable-blend slice curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(3.0, -2.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0),
        };
        for (ordinal, side) in side_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == *side)
                .expect("variable-blend side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -1.0, 2.0),
                direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, -4.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less variable-blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less variable-blend round trip");
        let ProceduralSurfaceDefinition::VariableBlend {
            construction: actual,
        } = &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip variable blend")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| Some(&curve.id) == actual.post_curve.as_ref())
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
        ));
        for side in actual.sides.iter() {
            assert!(matches!(
                round_trip
                    .ir()
                    .model
                    .curves
                    .iter()
                    .find(|curve| Some(&curve.id) == side.curve.as_ref())
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == actual.slice)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [-1.0, -1.0, 2.0, 2.0]
        ));
    }
}

#[test]
fn generated_variable_blend_rejects_radius_cardinality_mismatch() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh(
                "var_blend_spl_sur",
            ))),
            &DecodeOptions::default(),
        )
        .expect("variable-blend decode")
        .into_parts()
        .0;
    decoded.source = None;
    decoded.set_native_unknowns("f3d", &[]).unwrap();
    decoded.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend { construction } = definition else {
            panic!("expected variable blend")
        };
        construction.second_value = Some(construction.first_value.clone());
    });

    assert!(cadmpeg_ir::validate_neutral(&decoded, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "variable blend construction payload is invalid"));
    let error = F3dCodec
        .plan(EncodeInput::new(&decoded, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("single-radius variable blend carries two-radii payloads"));
}

#[test]
fn generated_two_radii_variable_blend_round_trips_rounded_chamfer() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendRadiusKind, VariableBlendValuePayload,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_branch(
                "var_blend_spl_sur",
                true,
            ))),
            &DecodeOptions::default(),
        )
        .expect("two-radii variable-blend decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.radius_kind, VariableBlendRadiusKind::TwoRadii);
    assert!(matches!(
        construction
            .second_value
            .as_ref()
            .map(|value| &value.payload),
        Some(VariableBlendValuePayload::TwoEnds {
            parameters: [0.1, 0.9],
            radii: [35.0, 45.0]
        })
    ));
    let Some(cadmpeg_ir::geometry::VariableBlendCrossSection::RoundedChamfer {
        radius: Some(radius),
    }) = &construction.cross_section
    else {
        panic!("expected rounded-chamfer cross section with a radius law")
    };
    assert!(matches!(
        &radius.payload,
        VariableBlendValuePayload::TwoEnds {
            parameters: [0.0, 1.0],
            radii: [55.0, 65.0]
        }
    ));

    let expected = construction.clone();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("two-radii variable-blend source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("two-radii variable-blend round trip");
    assert!(matches!(
        &round_trip.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}

#[test]
fn generated_two_radii_variable_blend_decodes_explicit_circular_cross_section() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendRadiusKind};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                true,
                Some(0),
                [None, None],
            ))),
            &DecodeOptions::default(),
        )
        .expect("two-radii selector-zero decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.radius_kind, VariableBlendRadiusKind::TwoRadii);
    assert!(matches!(
        &construction.cross_section,
        Some(cadmpeg_ir::geometry::VariableBlendCrossSection::Circular)
    ));
    let expected = construction.clone();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("selector-zero source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("selector-zero round trip");
    assert!(matches!(
        &round_trip.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}
