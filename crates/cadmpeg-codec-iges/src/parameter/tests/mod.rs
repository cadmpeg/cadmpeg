// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::directory::{DirectoryEntry, SourceStatus};

use super::ParameterRecord;
use super::Token;
use super::TokenValue;
use crate::card::{scan, Section};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

mod advanced_entity_boundaries;
mod curve_surface_boundaries;
mod drawing_associativity;
mod entity_table_boundaries;
mod entity_table_forms;
mod envelope_boundaries;
mod envelope_counted_entity_boundaries;
mod envelope_fixed_field_boundaries;
mod fixed_entity_boundaries;
mod implementor_defined;
mod later_entity_boundaries;
mod legacy_entities;
mod legacy_type402;
mod lexical;
mod macros;
mod presentation_forms;
mod solid_entity_boundaries;
mod type_fem;

#[test]
fn parameter_summary_refuses_note_slot_and_text_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let records = [];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::summary_notes(&records, &ctx);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges parameter summary notes"
    ));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = b"parameter_records=0".len() as u64 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::summary_notes(&records, &ctx);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.additional == b"parameter_records=0".len() as u64
                && limit.operation == "iges parameter summary text"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::summary_notes(&records, &ctx).unwrap(),
        [
            "parameter_records=0",
            "parameter_tokens=0",
            "external_references=0"
        ]
    );
}

#[test]
fn trailing_pointer_prefix_refuses_collection_limit_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeMap;

    let record = integer_parameter_record(1, &[999, 0, 0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::analyze_trailing_pointer_groups_for_global_table_with_context(
        &record,
        &BTreeMap::new(),
        crate::global::GlobalTable::V5Later,
        Some(&ctx),
    );
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 4
                && limit.operation == "iges noninteger token prefix"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(
        super::analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &BTreeMap::new(),
            crate::global::GlobalTable::V5Later,
            Some(&ctx),
        )
        .is_ok()
    );
}

#[test]
fn trailing_pointer_entries_refuse_collection_limit_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeMap;

    let record = integer_parameter_record(1, &[999, 3, 5, 0]);
    let candidate = super::PointerGroupCandidate {
        token_start: 0,
        association_start: 1,
        property_count_index: 3,
        property_count: 0,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result =
        super::groups_for_candidate_with_context(&record, &BTreeMap::new(), candidate, Some(&ctx));
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 2
                && limit.operation == "iges trailing pointer entries"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(super::groups_for_candidate_with_context(
        &record,
        &BTreeMap::new(),
        candidate,
        Some(&ctx),
    )
    .unwrap()
    .is_some());
}

#[test]
fn resolved_pointer_groups_refuse_collection_limit_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let groups = super::TrailingPointerGroups {
        token_start: 1,
        association_pointers: [3_u32, 5]
            .into_iter()
            .enumerate()
            .map(|(token_index, sequence)| super::TrailingPointer {
                token_index,
                raw_pointer: i64::from(sequence),
                resolved: Some(sequence),
            })
            .collect(),
        property_pointers: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = groups.fully_valid_with_context(Some(&ctx));
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 2
                && limit.operation == "iges resolved association pointers"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        groups
            .fully_valid_with_context(Some(&ctx))
            .unwrap()
            .unwrap()
            .associations(),
        &[3, 5]
    );
}

#[test]
fn owned_parameter_bytes_refuse_retained_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeMap;

    let bytes = crate::test_support::test_curves_and_surfaces::point_file();
    let scan = scan(&bytes).unwrap();
    let lines = scan.section(Section::Parameter).collect::<BTreeMap<_, _>>();
    let cards = [*lines.keys().next().unwrap()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 63;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let result = super::owned_bytes(&cards, &lines, Some(&ctx));
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.additional == 64
    ));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(super::owned_bytes(&cards, &lines, Some(&ctx)).is_ok());
}

#[test]
fn quarantined_parameter_bytes_refuse_retained_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeMap;

    let bytes = crate::test_support::test_curves_and_surfaces::point_file();
    let scan = scan(&bytes).unwrap();
    let lines = scan.section(Section::Parameter).collect::<BTreeMap<_, _>>();
    let cards = [*lines.keys().next().unwrap()];
    let (directory, _) =
        crate::directory::parse(&scan, crate::global::GlobalTable::V5Later, None).unwrap();
    let entry = directory.first().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 79;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let result = super::quarantine(
        entry,
        &cards,
        &lines,
        super::ParameterDefect::NoOwnedCards,
        None,
        Some(&ctx),
    );
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.additional == 80
    ));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(super::quarantine(
        entry,
        &cards,
        &lines,
        super::ParameterDefect::NoOwnedCards,
        None,
        Some(&ctx),
    )
    .is_ok());
}

#[test]
fn parameter_ownership_refuses_nested_owner_map_before_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeMap;

    let bytes = crate::test_support::test_curves_and_surfaces::point_file();
    let scan = scan(&bytes).unwrap();
    let (directory, _) =
        crate::directory::parse(&scan, crate::global::GlobalTable::V5Later, None).unwrap();
    let lines = scan.section(Section::Parameter).collect::<BTreeMap<_, _>>();
    let back_pointers = lines
        .iter()
        .map(|(sequence, line)| (*sequence, super::back_pointer(line)))
        .collect::<BTreeMap<_, _>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let result = super::resolve_ownership(
        &directory,
        &lines,
        &back_pointers,
        &mut crate::card::FramingRecoveries::default(),
        Some(&ctx),
    );
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 2
                && limit.additional == 1
                && limit.operation == "iges named parameter owners"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(super::resolve_ownership(
        &directory,
        &lines,
        &back_pointers,
        &mut crate::card::FramingRecoveries::default(),
        Some(&ctx),
    )
    .is_ok());
}

fn parameter_owner(field: [u8; 8]) -> Option<u32> {
    let mut bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00010000",
        parameters: "116,1,2,3,0;".into(),
    }]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      1")
        .expect("Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(&field);
    let scan = scan(&bytes).unwrap();
    let line = scan
        .section(Section::Parameter)
        .map(|(_, line)| line)
        .next()
        .expect("Parameter Data line");
    super::back_pointer(line)
}

impl From<i64> for TokenValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for TokenValue {
    fn from(value: f64) -> Self {
        Self::real(value)
    }
}

#[test]
fn finite_real_token_keeps_native_json_shape() {
    assert_eq!(
        serde_json::to_string(&TokenValue::real(1.5)).expect("token JSON"),
        r#"{"kind":"real","value":1.5}"#
    );
}

#[test]
fn parameter_owner_field_uses_blank_column_65_and_right_aligned_seven_digits() {
    for (field, expected) in [
        (*b"       1", Some(1)),
        (*b" 0000001", Some(1)),
        (*b" 9999999", Some(9_999_999)),
        (*b"1       ", None),
        (*b" 123456 ", None),
        (*b"        ", None),
        (*b" 0000000", None),
        (*b"  123456", Some(123_456)),
    ] {
        assert_eq!(parameter_owner(field), expected, "{field:?}");
    }
}

fn integer_parameter_record(sequence: u32, values: &[i64]) -> ParameterRecord {
    ParameterRecord {
        directory_sequence: sequence,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .iter()
            .copied()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    }
}

fn token_parameter_record(sequence: u32, values: Vec<TokenValue>) -> ParameterRecord {
    let parameter_end = values.len();
    ParameterRecord {
        directory_sequence: sequence,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect(),
        parameter_end,
        comment: Vec::new(),
    }
}

pub(super) fn directory_target_with_form(
    sequence: u32,
    entity_type: i64,
    form: i64,
) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence,
        entity_type,
        parameter_start: 1,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: SourceStatus::from_codes([0, 0, 0, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 1,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}
