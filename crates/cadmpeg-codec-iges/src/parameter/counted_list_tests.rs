// SPDX-License-Identifier: Apache-2.0
//! Counted fixed-width lists and the IGES 5.3 §2.2.3 defaulted final item.

use super::{DefaultTailCount, ParameterRecord, Token, TokenValue};

fn integer_record(values: &[i64], parameter_end: usize) -> ParameterRecord {
    ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .iter()
            .map(|value| Token {
                value: TokenValue::Integer(*value),
                span: 0..0,
            })
            .collect(),
        parameter_end,
        comment: Vec::new(),
    }
}

fn counted_record(declared: usize, item_tokens: usize) -> ParameterRecord {
    let mut values = vec![0, declared as i64];
    values.extend(std::iter::repeat_n(0, item_tokens));
    let parameter_end = values.len();
    integer_record(&values, parameter_end)
}

fn counted_record_with_suffix(
    declared: usize,
    item_tokens: usize,
    suffix_tokens: usize,
) -> (ParameterRecord, usize) {
    let mut values = vec![0, declared as i64];
    values.extend(std::iter::repeat_n(0, item_tokens));
    let list_end = values.len();
    values.extend(std::iter::repeat_n(0, suffix_tokens));
    let parameter_end = values.len();
    (integer_record(&values, parameter_end), list_end)
}

#[test]
fn a_list_that_runs_to_the_record_end_holds_its_final_item_in_whole_or_in_part() {
    for stride in [2, 12, 20] {
        for complete in 0..4 {
            for partial in 0..stride {
                let item_tokens = complete * stride + partial;
                let present = complete + usize::from(partial > 0);
                let record = counted_record(present, item_tokens);
                assert_eq!(
                    record.items_before_default_tail_at(2, stride, record.parameter_end()),
                    Some(present),
                    "stride {stride}, {complete} complete items, {partial} trailing tokens"
                );
                assert_eq!(
                    record.count_with_stride_before_default_tail(1, stride, record.parameter_end()),
                    DefaultTailCount::Held(present),
                    "stride {stride}, {complete} complete items, {partial} trailing tokens"
                );
            }
        }
    }
}

#[test]
fn a_complete_item_before_a_partial_item_admits_both() {
    let stride = 20;
    let record = counted_record(2, stride + 9);

    assert_eq!(
        record.items_before_default_tail_at(2, stride, record.parameter_end()),
        Some(2)
    );
    assert_eq!(
        record.count_with_stride_before_default_tail(1, stride, record.parameter_end()),
        DefaultTailCount::Held(2)
    );
}

#[test]
fn a_declared_count_above_the_items_present_in_whole_or_in_part_is_not_admitted() {
    for stride in [2, 12, 20] {
        for complete in 0..4 {
            for partial in 0..stride {
                let item_tokens = complete * stride + partial;
                let present = complete + usize::from(partial > 0);
                let record = counted_record(present + 1, item_tokens);
                assert_eq!(
                    record.items_before_default_tail_at(2, stride, record.parameter_end()),
                    Some(present),
                    "stride {stride}, {complete} complete items, {partial} trailing tokens"
                );
                assert_eq!(
                    record.count_with_stride_before_default_tail(1, stride, record.parameter_end()),
                    DefaultTailCount::Overdeclared {
                        declared: present + 1,
                        present
                    },
                    "stride {stride}, {complete} complete items, {partial} trailing tokens"
                );
            }
        }
    }
}

#[test]
fn a_count_below_the_items_present_leaves_the_surplus_tokens_unread() {
    let stride = 12;
    let record = counted_record(1, 3 * stride);

    assert_eq!(
        record.items_before_default_tail_at(2, stride, record.parameter_end()),
        Some(3)
    );
    assert_eq!(
        record.count_with_stride_before_default_tail(1, stride, record.parameter_end()),
        DefaultTailCount::Held(1)
    );
}

#[test]
fn a_list_that_a_suffix_follows_holds_only_its_complete_items() {
    let stride = 20;
    let declared = 2;
    let (record, list_end) = counted_record_with_suffix(declared, stride + 9, 2);
    let complete = (stride + 9) / stride;

    assert_eq!(
        record.items_before_default_tail_at(2, stride, list_end),
        Some(complete)
    );
    assert_eq!(
        record.count_with_stride_before_default_tail(1, stride, list_end),
        DefaultTailCount::Overdeclared {
            declared,
            present: complete
        }
    );

    let (exact, exact_end) = counted_record_with_suffix(1, stride, 2);
    assert_eq!(
        exact.items_before_default_tail_at(2, stride, exact_end),
        Some(1)
    );
    assert_eq!(
        exact.count_with_stride_before_default_tail(1, stride, exact_end),
        DefaultTailCount::Held(1)
    );
}

#[test]
fn an_empty_list_admits_a_zero_count_and_no_item() {
    let stride = 20;
    let record = counted_record(0, 0);

    assert_eq!(
        record.items_before_default_tail_at(2, stride, record.parameter_end()),
        Some(0)
    );
    assert_eq!(
        record.count_with_stride_before_default_tail(1, stride, record.parameter_end()),
        DefaultTailCount::Held(0)
    );

    let declared = 1;
    let overdeclared = counted_record(declared, 0);
    assert_eq!(
        overdeclared.count_with_stride_before_default_tail(1, stride, overdeclared.parameter_end()),
        DefaultTailCount::Overdeclared {
            declared,
            present: 0
        }
    );
}

#[test]
fn an_item_start_past_the_count_excludes_the_fields_that_precede_the_list() {
    let stride = 2;
    let record = integer_record(&[0, 3, 0, 0, 0, 0, 0, 0, 0], 9);

    assert_eq!(record.items_before_default_tail_at(7, stride, 9), Some(1));
    assert_eq!(
        record.count_with_stride_before_default_tail_at(1, 7, stride, 9),
        DefaultTailCount::Overdeclared {
            declared: 3,
            present: 1
        }
    );
    assert_eq!(
        record.count_with_stride_before_default_tail(1, stride, 9),
        DefaultTailCount::Held(3)
    );
}

#[test]
fn an_item_start_of_index_plus_one_agrees_with_the_implicit_form() {
    for stride in [1, 2, 12] {
        for item_tokens in 0..3 * stride {
            for declared in 0..4 {
                let record = counted_record(declared, item_tokens);
                let end = record.parameter_end();
                assert_eq!(
                    record.count_with_stride_before_default_tail(1, stride, end),
                    record.count_with_stride_before_default_tail_at(1, 2, stride, end),
                    "stride {stride}, {item_tokens} item tokens, declared {declared}"
                );
            }
        }
    }
}

#[test]
fn a_zero_stride_has_no_item_count_and_admits_no_declared_count() {
    let record = counted_record(1, 20);

    assert_eq!(
        record.items_before_default_tail_at(2, 0, record.parameter_end()),
        None
    );
    assert_eq!(
        record.count_with_stride_before_default_tail(1, 0, record.parameter_end()),
        DefaultTailCount::Unreadable
    );
}

#[test]
fn a_negative_or_missing_count_admits_no_list() {
    let stride = 12;
    let negative = integer_record(&[0, -1, 0, 0], 4);
    assert_eq!(
        negative.count_with_stride_before_default_tail(1, stride, negative.parameter_end()),
        DefaultTailCount::Unreadable
    );

    let absent = integer_record(&[0], 1);
    assert_eq!(
        absent.count_with_stride_before_default_tail(1, stride, absent.parameter_end()),
        DefaultTailCount::Unreadable
    );
    assert_eq!(
        absent.items_before_default_tail_at(2, stride, absent.parameter_end()),
        Some(0)
    );
}

#[test]
fn a_bounded_count_admits_exactly_the_counts_that_fit_the_available_items() {
    for stride in [1, 2, 3, 7] {
        for item_tokens in 0..4 * stride {
            for declared in 0..6_usize {
                let record = counted_record(declared, item_tokens);
                let end = record.parameter_end();
                let available = end.saturating_sub(2);
                let fits = declared <= available / stride;
                assert_eq!(
                    record.count_with_stride_before(1, stride, end).is_some(),
                    fits,
                    "stride {stride}, {item_tokens} item tokens, declared {declared}"
                );
            }
        }
    }
}

#[test]
fn a_bounded_count_rejects_a_product_that_overflows_without_panicking() {
    let record = integer_record(&[0, i64::MAX, 0, 0], 4);

    assert_eq!(record.count_with_stride_before(1, 7, 4), None);
    assert_eq!(record.count_with_stride_before(1, 1, 4), None);
}
