// SPDX-License-Identifier: Apache-2.0

use super::*;

use std::collections::BTreeMap;

fn integer_record(values: &[i64]) -> crate::parameter::ParameterRecord {
    crate::parameter::ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .iter()
            .map(|value| crate::parameter::Token {
                value: crate::parameter::TokenValue::Integer(*value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    )
}

fn with_zero_context(mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>)) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        run(&ctx);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn empty_controls_have_no_unique_plane_without_work() {
    with_zero_context(|ctx| {
        let result = super::super::classify_control_point_plane(&[], 0.0, ctx);
        assert!(matches!(
            result.unwrap(),
            super::super::ControlPointPlane::NoUniquePlane
        ));
    });
}

#[test]
fn empty_declared_plane_fit_is_false_without_work() {
    with_zero_context(|ctx| {
        let result =
            super::super::control_points_fit_plane(&[], Vector3::new(0.0, 0.0, 1.0), 0.0, ctx);
        assert!(!result.unwrap());
    });
}

#[test]
fn point_symbol_routes_validate_fixed_pointer_forms_without_work() {
    for (values, valid) in [
        (&[116][..], true),
        (&[116, 0, 0, 0, 0][..], true),
        (&[116, 0, 0, 0, -1][..], false),
        (&[116, 0, 0, 0, 2][..], false),
        (&[116, 0, 0, 0, i64::MAX][..], false),
    ] {
        let record = integer_record(values);
        with_zero_context(|ctx| {
            let result = super::super::point_display_symbol_valid(
                &record,
                &BTreeMap::new(),
                crate::global::GlobalTable::V5Later,
                ctx,
            );
            assert_eq!(result.unwrap(), valid);
        });
    }
}

#[test]
fn incomplete_type126_layout_has_no_declared_controls_or_work() {
    let (_, global) = crate::test_support::sequence_index::parameter_inputs(0);
    for values in [
        &[126][..],
        &[126, -1, 1][..],
        &[126, i64::MAX, i64::MAX][..],
    ] {
        let record = integer_record(values);
        with_zero_context(|ctx| {
            let result = super::super::type126_declared_control_points(
                &record,
                global.real_precision(),
                ctx,
            );
            assert!(result.unwrap().is_none());
        });
    }
}

#[test]
fn invalid_linear_nurbs_layout_rejects_without_work() {
    with_zero_context(|ctx| {
        for (degree, count, periodic, range) in [
            (0, 1, false, [0.0, 1.0]),
            (1, 2, false, [0.0, 1.0]),
            (1, 2, true, [0.0, 1.0]),
            (1, usize::MAX, false, [0.0, 1.0]),
            (1, 2, false, [f64::NAN, 1.0]),
        ] {
            let result =
                super::super::linear_nurbs_parameters(degree, &[], count, periodic, range, ctx);
            assert!(result.unwrap().is_none());
        }
    });
}

#[test]
fn invalid_plane_coordinate_frame_rejects_without_work() {
    with_zero_context(|ctx| {
        for normal in [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(f64::NAN, 0.0, 0.0),
        ] {
            let result = super::super::plane_coordinates(
                &[],
                (cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), normal),
                ctx,
            );
            assert!(result.unwrap().is_none());
        }
    });
}

#[test]
fn null_and_invalid_transform_pointers_leave_active_paths_unchanged() {
    let (_, global) = crate::test_support::sequence_index::parameter_inputs(0);
    with_zero_context(|ctx| {
        for sequence in [0, i64::MIN, -1, 2, i64::MAX] {
            for path in [BTreeSet::new(), BTreeSet::from([7])] {
                let mut actual = path.clone();
                let result = super::super::resolve_transform(
                    sequence,
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    1.0,
                    global.real_precision(),
                    &mut actual,
                    ctx,
                );
                if sequence == 0 {
                    assert_eq!(result.unwrap(), super::super::Transform::identity());
                } else {
                    assert!(matches!(
                        result,
                        Err(super::super::TransformResolutionError::Invalid(_))
                    ));
                }
                assert_eq!(actual, path);
            }
        }
    });
}

#[test]
fn short_affine_progression_layout_has_no_progression_or_work() {
    with_zero_context(|ctx| {
        for (values, uncertainties) in [
            (&[][..], &[][..]),
            (&[0.0][..], &[0.0][..]),
            (&[0.0, 1.0][..], &[0.0][..]),
        ] {
            let result = super::super::declared_affine_progression(values, uncertainties, ctx);
            assert!(!result.unwrap());
        }
    });
}

#[test]
fn short_polylines_have_no_self_intersection_or_work() {
    with_zero_context(|ctx| {
        for points in [&[][..], &[[0.0, 0.0]][..], &[[0.0, 0.0], [1.0, 0.0]][..]] {
            let result = super::super::planar_polyline_has_self_intersection(points, None, ctx);
            assert!(!result.unwrap());
        }
    });
}

#[test]
fn unrooted_point_sequence_adds_no_identity_or_work() {
    let id = cadmpeg_ir::ids::PointId::mint("iges:model:point#free").unwrap();
    let stem = crate::ids::Stem::word(crate::ids::Word::FreeGeometry);
    with_zero_context(|ctx| {
        let mut sequences = super::super::SourceSequences::default();
        let result = sequences.record_point(&id, &stem, ctx);
        result.unwrap();
        assert!(sequences.points.is_empty());
    });
}
