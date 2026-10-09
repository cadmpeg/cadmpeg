// SPDX-License-Identifier: Apache-2.0

use crate::curve::{
    complete_curve_row_linkage, expression_helix, expression_program_control_is_valid,
    framed_segment_with_face_ids, parse_depdb_curve_segment, split_assignment_target_arguments,
    split_expression_assignment, unique_topology_suffix_in_segment, CurveExpressionActivation,
    CurveExpressionAssignment, CurveExpressionLine, CurveExpressionRecord, CurveExpressionTarget,
    CurveExpressionValue, DimensionForm, DimensionRational, ExpressionParser, ExpressionValue,
    RelationDimension, RelationEvaluationContext, SimultaneousAffineValue,
    MAX_NONLINEAR_SOLVE_VARIABLES,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn work_boundary<T>(
    work: u64,
    caps: impl IntoIterator<Item = u64>,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    for cap in caps {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = run(&ctx);
        if cap < work {
            let Err(CodecError::ResourceLimit(original)) = result else {
                panic!("work below the source-derived bound must refuse");
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.limit, cap);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let value = result.expect("exact source-derived work is admitted");
            // All expected work has executed. One more unit must refuse at
            // the exact bound, independently of the operation's return value.
            let original = ctx.charge_work_limit(1, "after present curve steps").expect_err("exact work");
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.used, work);
            assert_eq!(original.additional, 1);
            drop(value);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn assignment_separator_exhaustion_pays_only_source_bytes() {
    for source in ["", "abc", "'a=b'", "é", "(a==b)"] {
        let work = u64::try_from(source.len()).expect("small source");
        work_boundary(work, 0..=work, |ctx| {
            let result = split_expression_assignment(ctx, source)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn assignment_separator_match_pays_only_visited_prefix() {
    let source = "a=unvisited_tail";
    work_boundary(2, 0..=2, |ctx| {
        let result = split_expression_assignment(ctx, source)?;
        assert_eq!(result, Some(("a", "unvisited_tail")));
        Ok(result)
    });
}

#[test]
fn assignment_separator_bad_close_pays_only_first_byte() {
    work_boundary(1, 0..=1, |ctx| {
        let result = split_expression_assignment(ctx, ")unvisited_tail")?;
        assert_eq!(result, None);
        Ok(result)
    });
}

#[test]
fn empty_target_arguments_do_not_advance_source() {
    work_boundary(0, [0], |ctx| {
        let result = split_assignment_target_arguments(ctx, "")?;
        assert_eq!(result, None);
        Ok(result)
    });
}

#[test]
fn unfinished_target_arguments_pay_only_visited_source() {
    for (source, work) in [("'abc", 4), ("(a", 2), (")tail", 1)] {
        work_boundary(work, 0..=work, |ctx| {
            let result = split_assignment_target_arguments(ctx, source)?;
            assert_eq!(result, None);
            Ok(result)
        });
    }
}

#[test]
fn empty_control_lines_pay_only_present_rows() {
    for count in [0, 1, 3] {
        let lines: Vec<_> = (0..count).map(|offset| CurveExpressionLine {
            text: String::new(),
            offset,
        }).collect();
        let work = u64::try_from(count).expect("small count");
        work_boundary(work, 0..=work, |ctx| {
            let result = expression_program_control_is_valid(ctx, &lines)?;
            assert!(result);
            Ok(result)
        });
    }
}

#[test]
fn incomplete_helix_outputs_pay_only_present_assignments() {
    for count in [0, 1, 3] {
        let record = CurveExpressionRecord {
            entity_id: 1,
            backup: false,
            local_system: None,
            lines: Vec::new(),
            assignments: (0..count).map(|offset| CurveExpressionAssignment {
                target: CurveExpressionTarget::Parameter {
                    name: "other".to_owned(),
                    declared_unit: None,
                },
                expression: String::new(),
                dependencies: Vec::new(),
                value: None,
                activation: CurveExpressionActivation::Active,
                offset,
            }).collect(),
            solve_blocks: Vec::new(),
            unresolved_solve_control: false,
            prohibited_constructs: Vec::new(),
            offset: 0,
            expression_offset: 0,
        };
        let work = u64::try_from(count).expect("small count");
        work_boundary(work, 0..=work, |ctx| {
            let result = expression_helix(ctx, &record)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}

#[test]
fn unique_topology_close_scan_pays_only_complete_windows() {
    for count in 0usize..=6 {
        let segment = vec![0; count];
        let work = u64::try_from(count.saturating_sub(2)).expect("small count");
        work_boundary(work, 0..=work, |ctx| {
            let result = unique_topology_suffix_in_segment(ctx, &segment)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}

#[test]
fn counted_row_linkage_pays_only_declared_links() {
    for count in [0u8, 1, 3] {
        let mut bytes = vec![crate::psb::token::ARRAY_OPEN, count];
        bytes.extend(std::iter::repeat_n(0, usize::from(count)));
        let work = u64::from(count);
        work_boundary(work, 0..=work, |ctx| {
            let result = complete_curve_row_linkage(ctx, &bytes)?;
            assert!(result);
            Ok(result)
        });
    }
    for bytes in [b"".as_slice(), &[0, 0, 0, 0], &[crate::psb::token::ARRAY_OPEN, 2, 0]] {
        work_boundary(0, [0], |ctx| {
            let result = complete_curve_row_linkage(ctx, bytes)?;
            assert_eq!(result, bytes != [crate::psb::token::ARRAY_OPEN, 2, 0]);
            Ok(result)
        });
    }
}

#[test]
fn depdb_suffix_without_prefix_does_not_advance_prefix_scan() {
    let cache = crate::scalar::ScalarCache::default();
    work_boundary(0, [0], |ctx| {
        let result = parse_depdb_curve_segment(ctx, &[0, 0, 0, 0], 0, &cache)?;
        assert!(result.is_none());
        Ok(result)
    });
}

#[test]
fn framed_curve_without_candidates_pays_two_actual_byte_passes() {
    for count in [0usize, 1, 3] {
        let payload = vec![0; count];
        let work = 2 * u64::try_from(count).expect("small count");
        work_boundary(work, 0..=work, |ctx| {
            let result = framed_segment_with_face_ids(ctx, &payload, 0, (0, count), false, None, None)?;
            assert!(result.is_none());
            Ok(result)
        });
    }
}

#[test]
fn unary_replay_pays_only_present_operators() {
    // The current core position operation admits its end probe. The number
    // and four whitespace searches, UTF-8 validation and scalar parsing use
    // eight units. Each sign adds a source step, a whitespace search and one
    // replay step. No replay step exists for a plain number.
    for count in 0usize..=4 {
        let source = format!("{}1", "-".repeat(count));
        let work = 8 + 3 * u64::try_from(count).expect("small count");
        work_boundary(work, 0..=work, |ctx| {
            let values = BTreeMap::new();
            let mut parser = ExpressionParser::<CurveExpressionValue> {
                source: source.as_bytes(),
                cursor: 0,
                values: &values,
                context: RelationEvaluationContext::default(),
                ctx,
                nesting: 0,
            };
            let result = parser.unary().map_err(|error| *error)?;
            let value = if count % 2 == 0 { 1.0 } else { -1.0 };
            assert_eq!(result, crate::curve::quantity_value(value, RelationDimension::default()));
            assert_eq!(parser.cursor, source.len());
            Ok(result)
        });
    }
}

fn named_entries<T: Copy>(count: usize, value: T) -> BTreeMap<String, T> {
    (b'a'..=b'l').take(count).map(|byte| (char::from(byte).to_string(), value)).collect()
}

fn variable_steps(count: usize) -> u64 {
    if count <= MAX_NONLINEAR_SOLVE_VARIABLES { 0 } else { u64::try_from(count).expect("small count") }
}

#[test]
fn zero_affine_combination_pays_only_present_variable_steps() {
    for count in [0, 1, MAX_NONLINEAR_SOLVE_VARIABLES, MAX_NONLINEAR_SOLVE_VARIABLES + 1, 12] {
        let right = SimultaneousAffineValue { dimension: RelationDimension::default(), constant: 2.0, coefficients: named_entries(count, 0.0) };
        let work = variable_steps(count);
        work_boundary(work, 0..=work, |ctx| {
            let result = SimultaneousAffineValue::constant(1.0, RelationDimension::default()).combine_admitted(right.clone(), false, ctx)?;
            let value = result.as_ref().expect("same dimension");
            assert_eq!(value.constant, 3.0);
            assert!(value.coefficients.is_empty());
            Ok(result)
        });
    }
}

#[test]
fn zero_dimension_combination_pays_only_present_variable_steps() {
    for count in [0, 1, MAX_NONLINEAR_SOLVE_VARIABLES, MAX_NONLINEAR_SOLVE_VARIABLES + 1, 12] {
        let right = DimensionForm { constant: DimensionRational::integer(2), variables: named_entries(count, DimensionRational::default()) };
        let work = variable_steps(count);
        work_boundary(work, 0..=work, |ctx| {
            let result = DimensionForm::constant(1).combine_admitted(ctx, right.clone(), false)?;
            let value = result.as_ref().expect("finite rational");
            assert_eq!(value.constant, DimensionRational::integer(3));
            assert!(value.variables.is_empty());
            Ok(result)
        });
    }
}

#[test]
fn equal_affine_coefficient_streams_pay_only_present_steps_and_key_bytes() {
    for count in [0, 1, MAX_NONLINEAR_SOLVE_VARIABLES, MAX_NONLINEAR_SOLVE_VARIABLES + 1, 12] {
        let left = SimultaneousAffineValue { dimension: RelationDimension::default(), constant: 5.0, coefficients: named_entries(count, 0.0) };
        let right = SimultaneousAffineValue { constant: 2.0, ..left.clone() };
        // Two one-byte key comparisons and, above the fixed ceiling, two
        // present traversal steps per equal coefficient.
        let work = 2 * u64::try_from(count).expect("small count") + 2 * variable_steps(count);
        work_boundary(work, 0..=work, |ctx| {
            let result = left.constant_difference_admitted(&right, ctx)?;
            assert_eq!(result, Some(3.0));
            Ok(result)
        });
    }
}

#[test]
fn one_empty_affine_stream_pays_only_present_other_stream_steps() {
    let count = MAX_NONLINEAR_SOLVE_VARIABLES + 1;
    let populated = SimultaneousAffineValue { dimension: RelationDimension::default(), constant: 3.0, coefficients: named_entries(count, 0.0) };
    let empty = SimultaneousAffineValue::constant(1.0, RelationDimension::default());
    let work = variable_steps(count);
    for reverse in [false, true] {
        work_boundary(work, 0..=work, |ctx| {
            let result = if reverse { empty.constant_difference_admitted(&populated, ctx)? } else { populated.constant_difference_admitted(&empty, ctx)? };
            assert_eq!(result, Some(if reverse { -2.0 } else { 2.0 }));
            Ok(result)
        });
    }
}

fn copied_tree_work<V>(count: usize) -> u64 {
    // Nine insertions add node bounds at lengths zero and five. These two
    // insertions admit three node passes; the other seven admit one. Lookup
    // and insertion each compare against at most all existing keys in one
    // node. Each one-byte key is also copied once.
    assert_eq!(count, 9);
    let node_bytes = 11 * (std::mem::size_of::<String>() + std::mem::size_of::<V>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<String>().max(std::mem::align_of::<V>()).max(std::mem::align_of::<usize>());
    let key_comparisons = 2 * (0..count).sum::<usize>();
    u64::try_from(13 * node_bytes + key_comparisons + count).expect("small tree") + variable_steps(count)
}

#[test]
fn affine_coefficient_copy_has_no_exhausted_traversal_fee() {
    let count = MAX_NONLINEAR_SOLVE_VARIABLES + 1;
    let source = SimultaneousAffineValue { dimension: RelationDimension::default(), constant: 7.0, coefficients: named_entries(count, 2.0) };
    let work = copied_tree_work::<f64>(count);
    work_boundary(work, [0, work - 1, work], |ctx| {
        let result = source.clone_admitted(ctx)?;
        assert_eq!(result, source);
        Ok(result)
    });
}

#[test]
fn dimension_coefficient_copy_has_no_exhausted_traversal_fee() {
    let count = MAX_NONLINEAR_SOLVE_VARIABLES + 1;
    let source = DimensionForm { constant: DimensionRational::integer(7), variables: named_entries(count, DimensionRational::one()) };
    let work = copied_tree_work::<DimensionRational>(count);
    work_boundary(work, [0, work - 1, work], |ctx| {
        let result = source.copy_admitted(ctx)?;
        assert_eq!(result, source);
        Ok(result)
    });
}

#[test]
fn dimension_scaling_pays_present_variable_and_retention_passes() {
    for count in [0, 1, MAX_NONLINEAR_SOLVE_VARIABLES, MAX_NONLINEAR_SOLVE_VARIABLES + 1] {
        let source = DimensionForm { constant: DimensionRational::integer(3), variables: named_entries(count, DimensionRational::one()) };
        let work = variable_steps(count) + u64::try_from(count).expect("small count");
        work_boundary(work, 0..=work, |ctx| {
            let result = source.clone().scale(2, ctx)?;
            let value = result.as_ref().expect("finite scale");
            assert_eq!(value.constant, DimensionRational::integer(6));
            assert!(value.variables.values().all(|coefficient| *coefficient == DimensionRational::integer(2)));
            assert_eq!(value.variables.len(), count);
            Ok(result)
        });
    }
}

#[test]
fn dimension_division_pays_present_variable_and_retention_passes() {
    for count in [0, 1, MAX_NONLINEAR_SOLVE_VARIABLES, MAX_NONLINEAR_SOLVE_VARIABLES + 1] {
        let source = DimensionForm { constant: DimensionRational::integer(6), variables: named_entries(count, DimensionRational::integer(2)) };
        let work = variable_steps(count) + u64::try_from(count).expect("small count");
        work_boundary(work, 0..=work, |ctx| {
            let result = source.clone().divide_exact(2, ctx)?;
            let value = result.as_ref().expect("finite division");
            assert_eq!(value.constant, DimensionRational::integer(3));
            assert!(value.variables.values().all(|coefficient| *coefficient == DimensionRational::one()));
            assert_eq!(value.variables.len(), count);
            Ok(result)
        });
    }
}
