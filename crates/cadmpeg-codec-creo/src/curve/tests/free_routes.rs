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

fn at_zero_budget<T>(run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(&ctx).expect("constant route is free");
    ctx.finish_session().expect("active session");
}

#[test]
fn empty_solve_synchronization_is_free() {
    at_zero_budget(|ctx| {
        synchronize_solve_blocks(ctx, &mut [], &[], &BTreeMap::new())
    });
}

#[test]
fn empty_relation_identifier_is_free() {
    at_zero_budget(|ctx| {
        let value = valid_expression_identifier(ctx, "")?;
        assert!(!value);
        Ok(value)
    });
}

#[test]
fn missing_relation_identifier_start_is_free() {
    for (source, start) in [(b"".as_slice(), 0), (b"a".as_slice(), 1)] {
        at_zero_budget(|ctx| {
            let value = expression_identifier_end(ctx, source, start)?;
            assert_eq!(value, None);
            Ok(value)
        });
    }
}

#[test]
fn invalid_relation_identifier_start_is_free() {
    for source in [b"1".as_slice(), b":".as_slice(), b" ".as_slice()] {
        at_zero_budget(|ctx| {
            let value = expression_identifier_end(ctx, source, 0)?;
            assert_eq!(value, None);
            Ok(value)
        });
    }
}

#[test]
fn first_conditional_frame_is_free() {
    at_zero_budget(|ctx| {
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
fn nonfinite_relation_formatting_is_free() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        at_zero_budget(|ctx| {
            let result = format_relation_real_admitted(ctx, value, None, false)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn zero_relation_formatting_is_free() {
    for value in [0.0, -0.0] {
        for (decimals, scientific) in [(None, false), (Some(2), false), (Some(2), true)] {
            at_zero_budget(|ctx| {
                let result = format_relation_real_admitted(ctx, value, decimals, scientific)?;
                assert_eq!(result.as_deref(), Some(""));
                assert_eq!(result.as_ref().map(String::capacity), Some(0));
                Ok(result)
            });
        }
    }
}

#[test]
fn absent_declared_relation_unit_is_free() {
    at_zero_budget(|ctx| {
        let value = CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(7.0).expect("finite"),
        );
        let result = apply_declared_relation_unit(ctx, value.clone(), None)?;
        assert_eq!(result, Some(value));
        Ok(result)
    });
}

#[test]
fn unsupported_helix_control_is_free() {
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
        at_zero_budget(|ctx| {
            let result = expression_helix(ctx, &record)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn invalid_curve_terminator_range_is_free() {
    for (start, end) in [(2, 1), (0, 2), (2, 3)] {
        at_zero_budget(|ctx| {
            let result = crate::curve::row_terminator(ctx, &[0], start, end)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn invalid_curve_frame_range_is_free() {
    for bounds in [(2, 1), (0, 2), (2, 3)] {
        at_zero_budget(|ctx| {
            let result = crate::curve::framed_segment_with_face_ids(ctx, &[0], 0, bounds, false, None, None)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}

#[test]
fn missing_two_chart_extent_is_free() {
    let cache = crate::scalar::ScalarCache::default();
    at_zero_budget(|ctx| {
        let result = crate::curve::complete_two_chart_samples(ctx, &[], 1, None, "unused chart samples", &cache)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn undersized_two_chart_count_is_free() {
    let cache = crate::scalar::ScalarCache::default();
    at_zero_budget(|ctx| {
        let result = crate::curve::complete_two_chart_samples(ctx, &[0; 7], 0, Some(2), "unused chart samples", &cache)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn fewer_than_two_chart_samples_is_free() {
    let cache = crate::scalar::ScalarCache::default();
    for (body, count) in [(b"".as_slice(), None), (&[0; 3], None), (&[0; 4], Some(1)), (&[0; 8], Some(0))] {
        at_zero_budget(|ctx| {
            let result = crate::curve::complete_two_chart_samples(ctx, body, 0, count, "unused chart samples", &cache)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}

#[test]
fn missing_topology_suffix_is_free() {
    at_zero_budget(|ctx| {
        let result = crate::curve::topology_suffix_with_face_ids(ctx, &[], None, None)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn empty_topology_candidates_are_free() {
    at_zero_budget(|ctx| {
        let result = crate::curve::topology_suffix_with_face_ids(ctx, &[crate::psb::token::COMPOUND_CLOSE], None, None)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn unique_fixed_topology_suffix_is_free() {
    at_zero_budget(|ctx| {
        let result = crate::curve::topology_suffix_with_face_ids(ctx, &[1, 2, 3, 4, 0, 0, crate::psb::token::COMPOUND_CLOSE], None, None)?;
        let candidate = result.expect("one exact suffix");
        assert_eq!(candidate.start, 0);
        assert_eq!(candidate.stored_references(), [1, 2, 3, 4]);
        assert_eq!(candidate.reference_geometry, [0, 0]);
        Ok(result)
    });
}

#[test]
fn empty_curve_scalar_lane_is_free() {
    let cache = crate::scalar::ScalarCache::default();
    at_zero_budget(|ctx| {
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
fn empty_dependency_scan_is_free() {
    at_zero_budget(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "empty dependency index")?;
        let mut dependencies = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..2 {
            assert_eq!(crate::curve::extend_expression_dependencies(
                ctx, &mut dependencies, &mut seen, &mut storage, ""
            )?, Some(()));
            assert!(dependencies.is_empty());
            assert!(seen.is_empty());
            assert_eq!(dependencies.capacity(), 0);
            assert_eq!(seen.capacity(), 0);
        }
        Ok(())
    });
}

#[test]
fn empty_separator_scan_is_free() {
    at_zero_budget(|ctx| {
        let result = crate::curve::split_expression_assignment(ctx, "")?;
        assert_eq!(result, None);
        Ok(result)
    });
}

#[test]
fn empty_counted_row_linkage_is_free() {
    for bytes in [b"".as_slice(), &[crate::psb::token::ARRAY_OPEN, 0], &[0, 0, 0, 0]] {
        at_zero_budget(|ctx| {
            let result = crate::curve::complete_curve_row_linkage(ctx, bytes)?;
            assert!(result);
            Ok(result)
        });
    }
}

#[test]
fn depdb_suffix_without_prefix_is_free() {
    let cache = crate::scalar::ScalarCache::default();
    at_zero_budget(|ctx| {
        let result = crate::curve::parse_depdb_curve_segment(ctx, &[0, 0, 0, 0], 0, &cache)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn topology_segment_without_window_is_free() {
    for segment in [b"".as_slice(), &[0], &[0, 0]] {
        at_zero_budget(|ctx| {
            let result = crate::curve::unique_topology_suffix_in_segment(ctx, segment)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}
