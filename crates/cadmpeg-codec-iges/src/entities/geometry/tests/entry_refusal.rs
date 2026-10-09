// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::with_entry_context;
use std::collections::BTreeMap;

#[test]
fn null_and_invalid_transform_pointers_preserve_original_refusal_and_path() {
    let (_, global) = crate::test_support::sequence_index::parameter_inputs(0);
    with_entry_context(|ctx, original| {
        for sequence in [0, i64::MIN, -1, 2, i64::MAX] {
            for path in [BTreeSet::new(), BTreeSet::from([7])] {
                let mut actual = path.clone();
                let result = super::super::resolve_transform(sequence, &BTreeMap::new(), &BTreeMap::new(),
                    1.0, global.real_precision(), &mut actual, ctx);
                if let Some(first) = original {
                    assert!(matches!(result, Err(super::super::TransformResolutionError::Resource(
                        CodecError::ResourceLimit(last))) if last == first));
                } else if sequence == 0 {
                    assert_eq!(result.unwrap(), super::super::Transform::identity());
                } else {
                    assert!(matches!(result, Err(super::super::TransformResolutionError::Invalid(_))));
                }
                assert_eq!(actual, path);
            }
        }
    });
}

#[test]
fn short_self_intersection_route_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for points in [&[][..], &[[0.0, 0.0]][..], &[[0.0, 0.0], [1.0, 0.0]][..]] {
            let result = super::super::planar_polyline_has_self_intersection(points, ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(!result.unwrap()),
            }
        }
    });
}

#[test]
fn invalid_plane_coordinate_frame_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for normal in [Vector3::new(0.0, 0.0, 0.0), Vector3::new(f64::NAN, 0.0, 0.0)] {
            let result = super::super::plane_coordinates(&[], (cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), normal), ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(result.unwrap().is_none()),
            }
        }
    });
}

#[test]
fn invalid_linear_nurbs_layout_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for (degree, count, periodic, range) in [(0, 1, false, [0.0, 1.0]),
            (1, 2, false, [0.0, 1.0]), (1, 2, true, [0.0, 1.0]),
            (1, usize::MAX, false, [0.0, 1.0]), (1, 2, false, [f64::NAN, 1.0])] {
            let result = super::super::linear_nurbs_parameters(degree, &[], count, periodic, range, ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(result.unwrap().is_none()),
            }
        }
    });
}

fn integer_record(values: &[i64]) -> crate::parameter::ParameterRecord {
    crate::parameter::ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
        values.iter().map(|value| crate::parameter::Token { value: crate::parameter::TokenValue::Integer(*value), span: 0..0 }).collect(), Vec::new())
}

#[test]
fn fixed_point_symbol_routes_preserve_original_refusal() {
    for (values, valid) in [(&[116][..], true), (&[116, 0, 0, 0, 0][..], true),
        (&[116, 0, 0, 0, -1][..], false), (&[116, 0, 0, 0, 2][..], false),
        (&[116, 0, 0, 0, i64::MAX][..], false)] {
        let record = integer_record(values);
        with_entry_context(|ctx, original| {
            let result = super::super::point_display_symbol_valid(&record, &BTreeMap::new(), crate::global::GlobalTable::V5Later, ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert_eq!(result.unwrap(), valid),
            }
        });
    }
}

#[test]
fn empty_control_plane_classification_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        let result = super::super::classify_control_point_plane(&[], 0.0, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result.unwrap(), super::super::ControlPointPlane::NoUniquePlane)),
        }
    });
}

#[test]
fn empty_declared_plane_fit_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        let result = super::super::control_points_fit_plane(&[], Vector3::new(0.0, 0.0, 1.0), 0.0, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(!result.unwrap()),
        }
    });
}

#[test]
fn incomplete_type126_interval_layout_preserves_original_refusal() {
    let (_, global) = crate::test_support::sequence_index::parameter_inputs(0);
    for values in [&[126][..], &[126, -1, 1][..], &[126, i64::MAX, i64::MAX][..]] {
        let record = integer_record(values);
        with_entry_context(|ctx, original| {
            let result = super::super::type126_declared_control_points(&record, global.real_precision(), ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(result.unwrap().is_none()),
            }
        });
    }
}

#[test]
fn short_affine_progression_layout_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for (values, uncertainties) in [(&[][..], &[][..]), (&[0.0][..], &[0.0][..]),
            (&[0.0, 1.0][..], &[0.0][..])] {
            let result = super::super::declared_affine_progression(values, uncertainties, ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(!result.unwrap()),
            }
        }
    });
}

#[test]
fn unrooted_point_sequence_preserves_original_refusal() {
    let id = cadmpeg_ir::ids::PointId::mint("iges:model:point#free").unwrap();
    let stem = crate::ids::Stem::word(crate::ids::Word::FreeGeometry);
    with_entry_context(|ctx, original| {
        let mut sequences = super::super::SourceSequences::default();
        let result = sequences.record_point(&id, &stem, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => result.unwrap(),
        }
        assert!(sequences.points.is_empty());
    });
}
