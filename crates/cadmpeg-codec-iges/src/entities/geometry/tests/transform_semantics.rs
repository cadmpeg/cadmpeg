// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::{resolve_transform, TransformFailure, TransformResolutionError};

#[test]
fn transform_depth_overflow_is_a_structured_resource_refusal() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
        let transform_count = 65_u32;
        let mut directory = (0..transform_count)
            .map(|index| {
                let sequence = 1 + index * 2;
                let transform = if index + 1 < transform_count {
                    sequence + 2
                } else {
                    0
                };
                transform_entry(sequence, i64::from(transform))
            })
            .collect::<Vec<_>>();
        directory.push(transform_entry(1 + transform_count * 2, 1));

        let error = enforce_transform_depth(&directory, decode_ctx).unwrap_err();
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::Codec("iges_transform_depth")
                    && limit.limit == 64
                    && limit.used == 64
                    && limit.additional == 1
        ));
    });
}

#[test]
fn transform_preflight_admits_directory_index_and_walk_path() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let directory = [transform_entry(1, 3), transform_entry(3, 0)];
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges transform preflight path",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            enforce_transform_depth(&directory, &ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(enforce_transform_depth(&directory, &ctx).is_ok());
}

#[test]
fn declared_transform_validation_separates_frame_and_handedness_invariants() {
    let intervals = |rows: [[f64; 3]; 3]| {
        std::array::from_fn::<_, 9, _>(|index| {
            DeclaredInterval::around(rows[index / 3][index % 3], 0.0)
        })
    };

    assert_eq!(
        validate_declared_transform_frame(
            intervals([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Ok(())
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            -1.0,
        ),
        Ok(())
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Err(DeclaredTransformFrameError::WrongDeterminant)
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[1.1, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Err(DeclaredTransformFrameError::NotOrthonormal)
    );
}

#[test]
fn decode_accepts_rounded_transformed_circular_arc_frame() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1.0000049,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Circle(circle_curve)) =
        result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a circle carrier");
    };
    let radius = circle_curve.radius().get();
    assert!((radius - 1.0).abs() < EPS_RADIUS_COMPARISON);
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
fn decode_rejects_transform_roundoff_beyond_its_declared_precision() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1.0000051,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("not orthonormal within its declared numeric precision")
    }));
}

#[test]
fn decode_applies_declared_double_precision_to_transform_coefficients() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,.8D0,-.6000001D0,0,0,.6D0,.8D0,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("not orthonormal within its declared numeric precision")
    }));
}

#[test]
fn decode_canonicalizes_a_rounded_left_handed_transform() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file_with_form(
                1,
                b"124,.7071068,-.7071068,0,0,.7071068,.7071068,0,0,0,0,-1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Circle(circle_curve)) =
        result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a circle carrier");
    };
    let axis = circle_curve.frame().axis().as_raw();
    let radius = circle_curve.radius().get();
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, -0.0, 1.0));
    assert_eq!(radius, 1.0);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_applies_nested_transforms_reflection_units_and_model_scale_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_transformed_point_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position().get().x, 0.0);
    assert_eq!(result.ir().model.points[0].position().get().y, 80.0);
    assert_eq!(result.ir().model.points[0].position().get().z, 60.0);
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas()["transformations"].len(),
        2
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn transform_translation_overflow_after_inch_scaling_is_rejected() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
        use crate::parameter::{ParameterRecord, Token, TokenValue};
        use std::collections::{BTreeMap, BTreeSet};
        let entry = transform_entry(1, 0);
        let values = [
            124.0,
            1.0,
            0.0,
            0.0,
            f64::MAX,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
        ];
        let record = ParameterRecord::from_test_tokens(
            1,
            1..2,
            Vec::new(),
            values.len(),
            values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::real(value),
                    span: 0..0,
                })
                .collect(),
            Vec::new(),
        );
        let result = resolve_transform(
            1,
            &BTreeMap::from([(1, &entry)]),
            &BTreeMap::from([(1, &record)]),
            25.4,
            crate::global::RealPrecision {
                single_significance: 6,
                double_significance: 15,
            },
            &mut BTreeSet::new(),
            decode_ctx,
        );
        assert!(result.is_err());
    });
}

#[test]
fn transform_failures_render_original_diagnostics_without_allocating_early() {

    let cases = [
        (TransformFailure::Literal("transformation chain is cyclic"), "transformation chain is cyclic"),
        (TransformFailure::Depth, "transformation chain exceeds 64 entities"),
        (TransformFailure::MissingEntry(7), "transformation D7 is missing"),
        (TransformFailure::WrongTypeForm { sequence: 7, entity_type: 123, form: 2 }, "transformation D7 is type 123 form 2, expected defining type 124 form 0 or 1"),
        (TransformFailure::MissingParameters(7), "transformation D7 parameters are missing"),
        (TransformFailure::NonNumericCoefficient { sequence: 7, index: 12 }, "transformation D7 coefficient 12 is not numeric"),
        (TransformFailure::NonFiniteCoefficient(7), "transformation D7 has a non-finite coefficient"),
        (TransformFailure::NotOrthonormal(7), "transformation D7 linear part is not orthonormal within its declared numeric precision"),
        (TransformFailure::WrongDeterminant { sequence: 7, form: 1 }, "transformation D7 determinant disagrees with form 1 within its declared numeric precision"),
        (TransformFailure::FirstAxis(7), "transformation D7 first axis cannot be normalized"),
        (TransformFailure::SecondAxis(7), "transformation D7 second axis cannot be normalized"),
        (TransformFailure::NonFiniteScaled(7), "transformation D7 has non-finite coefficients after length scaling"),
        (TransformFailure::NonFiniteComposed(7), "transformation D7 has non-finite coefficients after composition"),
    ];
    for (reason, expected) in cases {
        assert_eq!(reason.to_string(), expected);
    }
}

#[test]
fn transform_chain_path_refuses_collection_limit_before_insertion() {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::{BTreeMap, BTreeSet};

    let identity_record = |sequence| {
        let values = [
            124.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];
        ParameterRecord::from_test_tokens(
            sequence,
            1..2,
            Vec::new(),
            values.len(),
            values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::real(value),
                    span: 0..0,
                })
                .collect(),
            Vec::new(),
        )
    };
    let parent = transform_entry(1, 0);
    let child = transform_entry(3, 1);
    let parent_record = identity_record(1);
    let child_record = identity_record(3);
    let entries = BTreeMap::from([(1, &parent), (3, &child)]);
    let records = BTreeMap::from([(1, &parent_record), (3, &child_record)]);
    let precision = crate::global::RealPrecision {
        single_significance: 6,
        double_significance: 15,
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges transform chain path",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            resolve_transform(
                3,
                &entries,
                &records,
                1.0,
                precision,
                &mut BTreeSet::new(),
                &ctx,
            )
            .map_err(|error| match error {
                TransformResolutionError::Resource(error) => error,
                other @ TransformResolutionError::Invalid(_) => {
                    panic!("unexpected transform refusal: {other:?}")
                }
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(resolve_transform(
        3,
        &entries,
        &records,
        1.0,
        precision,
        &mut BTreeSet::new(),
        &ctx,
    )
    .is_ok());
}

#[test]
fn affine_composition_rejects_translation_overflow() {
    let transform = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert!(transform.compose(transform).is_err());
}

#[test]
fn decode_reports_transform_translation_overflow_after_inch_scaling() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,1,2HIN,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let bytes = transformed_circular_arc_file_with_global(
        0,
        b"124,1,0,0,1.7D308,0,1,0,0,0,0,1,0;",
        b"100,0,0,0,1,0,0,1;",
        global,
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("non-finite coefficients after length scaling")));
}

