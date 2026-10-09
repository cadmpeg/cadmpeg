// SPDX-License-Identifier: Apache-2.0

use crate::curve::{
    apply_declared_relation_unit, expression_helix, expression_identifier_end,
    format_relation_real_admitted, synchronize_solve_blocks, valid_expression_identifier,
    ConditionalFrame, ConditionalStack, CurveExpressionActivation, CurveExpressionRecord,
    CurveExpressionSolveBlock, CurveExpressionValue,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashSet};

fn active_and_refused<T>(mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, CodecError>) {
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        if refused {
            let original = ctx
                .charge_work_limit(1, "before curve recovery")
                .expect_err("zero work");
            assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let _value = run(&ctx).expect("constant recovery is free");
            assert!(ctx.resource_refusal().is_none());
            ctx.finish_session().expect("active session");
        }
    }
}

#[test]
fn empty_solve_synchronization_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        synchronize_solve_blocks(ctx, &mut [], &[], &BTreeMap::new())
    });
}

#[test]
fn empty_relation_identifier_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let value = valid_expression_identifier(ctx, "")?;
        assert!(!value);
        Ok(value)
    });
}

#[test]
fn missing_relation_identifier_start_is_free_and_keeps_original_refusal() {
    for (source, start) in [(b"".as_slice(), 0), (b"a".as_slice(), 1)] {
        active_and_refused(|ctx| {
            let value = expression_identifier_end(ctx, source, start)?;
            assert_eq!(value, None);
            Ok(value)
        });
    }
}

#[test]
fn invalid_relation_identifier_start_is_free_and_keeps_original_refusal() {
    for source in [b"1".as_slice(), b":".as_slice(), b" ".as_slice()] {
        active_and_refused(|ctx| {
            let value = expression_identifier_end(ctx, source, 0)?;
            assert_eq!(value, None);
            Ok(value)
        });
    }
}

#[test]
fn first_conditional_frame_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let mut stack = ConditionalStack::default();
        let result = stack.push(ctx, ConditionalFrame {
            parent: CurveExpressionActivation::Active,
            condition: Some(true),
        });
        if result.is_err() {
            assert!(matches!(stack, ConditionalStack::Empty));
        } else {
            assert!(matches!(stack, ConditionalStack::Open { .. }));
            assert_eq!(stack.end(), CurveExpressionActivation::Active);
            assert!(matches!(stack, ConditionalStack::Empty));
        }
        result
    });
}

#[test]
fn nonfinite_relation_formatting_is_free_and_keeps_original_refusal() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        active_and_refused(|ctx| {
            let result = format_relation_real_admitted(ctx, value, None, false)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn zero_relation_formatting_is_free_and_keeps_original_refusal() {
    for value in [0.0, -0.0] {
        for (decimals, scientific) in [(None, false), (Some(2), false), (Some(2), true)] {
            active_and_refused(|ctx| {
                let result = format_relation_real_admitted(ctx, value, decimals, scientific)?;
                assert_eq!(result.as_deref(), Some(""));
                assert_eq!(result.as_ref().map(String::capacity), Some(0));
                Ok(result)
            });
        }
    }
}

#[test]
fn absent_declared_relation_unit_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let value = CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(7.0).expect("finite"),
        );
        let result = apply_declared_relation_unit(ctx, value.clone(), None)?;
        assert_eq!(result, Some(value));
        Ok(result)
    });
}

#[test]
fn unsupported_helix_control_is_free_and_keeps_original_refusal() {
    for route in 0..3 {
        let record = CurveExpressionRecord {
            entity_id: 7,
            backup: false,
            local_system: None,
            lines: Vec::new(),
            assignments: Vec::new(),
            solve_blocks: if route == 1 { vec![CurveExpressionSolveBlock {
                equations: Vec::new(), assignments: Vec::new(), unknowns: Vec::new(),
                offset: 0, for_offset: 1,
            }] } else { Vec::new() },
            unresolved_solve_control: route == 2,
            prohibited_constructs: if route == 0 { vec!["abs".into()] } else { Vec::new() },
            offset: 0,
            expression_offset: 1,
        };
        active_and_refused(|ctx| {
            let result = expression_helix(ctx, &record)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn invalid_curve_terminator_range_is_free_and_keeps_original_refusal() {
    for (start, end) in [(2, 1), (0, 2), (2, 3)] {
        active_and_refused(|ctx| {
            let result = crate::curve::row_terminator(ctx, &[0], start, end)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn invalid_curve_frame_range_is_free_and_keeps_original_refusal() {
    for bounds in [(2, 1), (0, 2), (2, 3)] {
        active_and_refused(|ctx| {
            let result = crate::curve::framed_segment_with_face_ids(ctx, &[0], 0, bounds, false, None, None)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}

#[test]
fn missing_two_chart_extent_is_free_and_keeps_original_refusal() {
    let cache = crate::scalar::ScalarCache::default();
    active_and_refused(|ctx| {
        let result = crate::curve::complete_two_chart_samples(ctx, &[], 1, None, "unused chart samples", &cache)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn undersized_two_chart_count_is_free_and_keeps_original_refusal() {
    let cache = crate::scalar::ScalarCache::default();
    active_and_refused(|ctx| {
        let result = crate::curve::complete_two_chart_samples(ctx, &[0; 7], 0, Some(2), "unused chart samples", &cache)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn fewer_than_two_chart_samples_is_free_and_keeps_original_refusal() {
    let cache = crate::scalar::ScalarCache::default();
    for (body, count) in [(b"".as_slice(), None), (&[0; 3], None), (&[0; 4], Some(1)), (&[0; 8], Some(0))] {
        active_and_refused(|ctx| {
            let result = crate::curve::complete_two_chart_samples(ctx, body, 0, count, "unused chart samples", &cache)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}

#[test]
fn missing_topology_suffix_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let result = crate::curve::topology_suffix_with_face_ids(ctx, &[], None, None)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn empty_topology_candidates_are_free_and_keep_original_refusal() {
    active_and_refused(|ctx| {
        let result = crate::curve::topology_suffix_with_face_ids(ctx, &[crate::psb::token::COMPOUND_CLOSE], None, None)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn unique_fixed_topology_suffix_is_free_and_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let result = crate::curve::topology_suffix_with_face_ids(ctx, &[1, 2, 3, 4, 0, 0, crate::psb::token::COMPOUND_CLOSE], None, None)?;
        let candidate = result.expect("one exact suffix");
        assert_eq!(candidate.start, 0);
        assert_eq!(candidate.stored_references(), [1, 2, 3, 4]);
        assert_eq!(candidate.reference_geometry, [0, 0]);
        Ok(result)
    });
}

#[test]
fn empty_curve_scalar_lane_is_free_and_keeps_original_refusal() {
    let cache = crate::scalar::ScalarCache::default();
    active_and_refused(|ctx| {
        let result = crate::curve::curve_scalar_lane(ctx, &[], 0, &cache)?;
        assert!(result.scalar_tokens.is_empty());
        assert!(result.references.is_empty());
        assert!(result.opaque_spans.is_empty());
        assert_eq!(result.scalar_tokens.capacity(), 0);
        assert_eq!(result.references.capacity(), 0);
        assert_eq!(result.opaque_spans.capacity(), 0);
        Ok(result)
    });
}

#[test]
fn empty_dependency_scan_is_free_and_keeps_original_refusal() {
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        // Prepare the receipt while the session is active, so its entry fuse
        // cannot satisfy the dependency scanner's refusal assertion.
        let mut storage = ctx.reserve_scoped(0, "empty dependency index").expect("empty receipt");
        let mut dependencies = Vec::new();
        let mut seen = HashSet::new();
        let original = refused.then(|| ctx.charge_work_limit(1, "before empty dependencies").expect_err("zero work"));
        for _ in 0..2 {
            let result = crate::curve::extend_expression_dependencies(&ctx, &mut dependencies, &mut seen, &mut storage, "");
            if let Some(original) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert_eq!(result.expect("empty traversal"), Some(()));
            }
            assert!(dependencies.is_empty());
            assert!(seen.is_empty());
            assert_eq!(dependencies.capacity(), 0);
            assert_eq!(seen.capacity(), 0);
        }
        drop(storage);
        if let Some(original) = original {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            ctx.finish_session().expect("active session");
        }
    }
}

#[test]
fn empty_separator_scan_keeps_original_refusal() {
    active_and_refused(|ctx| {
        let result = crate::curve::split_expression_assignment(ctx, "")?;
        assert_eq!(result, None);
        Ok(result)
    });
}

#[test]
fn empty_counted_row_linkage_keeps_original_refusal() {
    for bytes in [b"".as_slice(), &[crate::psb::token::ARRAY_OPEN, 0], &[0, 0, 0, 0]] {
        active_and_refused(|ctx| {
            let result = crate::curve::complete_curve_row_linkage(ctx, bytes)?;
            assert!(result);
            Ok(result)
        });
    }
}

#[test]
fn depdb_suffix_without_prefix_keeps_original_refusal() {
    let cache = crate::scalar::ScalarCache::default();
    active_and_refused(|ctx| {
        let result = crate::curve::parse_depdb_curve_segment(ctx, &[0, 0, 0, 0], 0, &cache)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn topology_segment_without_window_keeps_original_refusal() {
    for segment in [b"".as_slice(), &[0], &[0, 0]] {
        active_and_refused(|ctx| {
            let result = crate::curve::unique_topology_suffix_in_segment(ctx, segment)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}
