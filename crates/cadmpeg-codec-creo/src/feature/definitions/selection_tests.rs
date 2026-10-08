// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

#[test]
fn placement_reader_stops_before_an_unvisited_tail() {
    let row = b"\xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6";
    let read_first = |ctx: &DecodeContext<'_>| {
        PlacementInstructions { payload: row, definition_offset: 1000,
            table_class: Some(11), markers: 0..row.len() }.next(ctx)
    };
    let CodecError::ResourceLimit(refusal) = crate::test_support::last_refusal_at(
        row, ResourceDimension::WorkUnits, "creo placement instruction byte traversal", read_first)
    else { panic!("work refusal"); };
    let mut extended = row.to_vec();
    extended.resize(row.len() + 4096, 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = refusal.used + refusal.additional;
    let (ctx, _) = DecodeContext::from_root_bytes(&extended, &arena, &policy).expect("root");
    let instruction = PlacementInstructions { payload: &extended, definition_offset: 1000,
        table_class: Some(11), markers: 0..extended.len() }.next(&ctx)
        .expect("first row uses the same work with a longer tail").expect("first row");
    assert_eq!(instruction.offset, 1000);
    assert_eq!(instruction.kind, 20_127);
    assert_eq!(instruction.geometry1_id, Some(2));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn selected_definition_keeps_inherited_schema_and_identical_output() {
    let mut payload = b"var_arr\0\xf8\x01\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2\
        \xe0\x01type\0\x01\xe0\x01key\0\x07\xe0\x01value\0\x18\
        \xe0\x01guess\0\x18\xe0\x01known\0\x01\xe0\x01homo\0\x00\xe0\x01uvar_id\0\x09".to_vec();
    let replay_offset = payload.len();
    payload.extend_from_slice(b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
        \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2\
        \x02\x07\x18\x18\x01\x00\x0a");
    let starts = [DefinitionStart { offset: 0, id: NonZeroU32::new(1), owner_override: None, positional: false },
        DefinitionStart { offset: replay_offset, id: NonZeroU32::new(1), owner_override: None, positional: true }];
    let selected = BTreeSet::from([replay_offset]);
    let all = crate::decode::with_test_decode_ctx(|ctx| definitions_in_ranges(ctx, &payload, &starts, None)).expect("all definitions");
    let result = crate::decode::with_test_decode_ctx(|ctx| definitions_in_ranges(ctx, &payload, &starts, Some(&selected))).expect("selected definition");
    assert_eq!(result, all[1..]);
    let rows = &result[0].variables.as_ref().expect("inherited table class").rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].key, 7);
    assert_eq!(rows[1].uvar_id, Some(10));
}

#[test]
fn excluded_definition_has_no_retained_output() {
    let payload = b"feat_defs_1\0local_sys\0\xf9\x04\x03\x18\x18\x18";
    let starts = [DefinitionStart { offset: 0, id: NonZeroU32::new(1), owner_override: None, positional: false }];
    let selected = BTreeSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root");
    assert!(definitions_in_ranges(&ctx, payload, &starts, Some(&selected)).expect("no output copies").is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn definition_searches_refuse_at_actual_visited_steps() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
        \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00";
    let result = crate::test_support::assert_work_boundaries(
        &["creo positional array header search", "creo positional array header uniqueness",
          "creo positional feature skamps cursor traversal", "creo class close search", "creo skamp item array search"],
        |ctx| positional_feature_skamps(ctx, payload, 0, payload.len(), 88));
    let Some(SolverSubtable::Declared { rows, header }) = result else { panic!("declared table"); };
    assert_eq!(header.declared_count, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].items[0].entity_id, 6);
}
