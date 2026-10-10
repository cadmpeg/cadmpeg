// SPDX-License-Identifier: Apache-2.0
//! Exact source exhaustion and fused empty-route admission.

use crate::element_map::{
    bind_topology, element_map_size, mapped_name_count, parse_legacy_records, parse_string_table,
    ParsedMap, TextScanner,
};
use crate::native::element_map::{
    ElementMapGroup, ElementMapNode, ElementMapNodes, ElementMappedName,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimit};
use cadmpeg_core::CodecError;

fn with_work<T>(work: u64, call: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    call(&ctx)
}

fn parsed(groups: Vec<ElementMapGroup>) -> ParsedMap {
    ParsedMap {
        map_id: 0,
        postfixes: Vec::new(),
        maps: ElementMapNodes::try_from(vec![ElementMapNode { map_id: 0, groups }]).unwrap(),
    }
}

fn assert_original<T>(result: Result<T, CodecError>, original: ResourceLimit) {
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn empty_text_and_record_sources_need_no_work() {
    with_work(0, |ctx| {
        let mut scanner = TextScanner::new("");
        scanner.skip_whitespace(ctx).unwrap();
        assert_eq!(scanner.next(ctx).unwrap(), None);
        assert_eq!(scanner.next_field(ctx).unwrap(), Some(""));
        assert_eq!(scanner.next_field(ctx).unwrap(), None);
        assert_eq!(scanner.next_legacy_id(ctx).unwrap(), None);
        assert!(parse_string_table(ctx, b"", 0, false).unwrap().is_empty());
        assert!(
            parse_legacy_records(ctx, &mut TextScanner::new_ascii(""), 0)
                .unwrap()
                .data
                .is_empty()
        );
        assert_eq!(element_map_size(ctx, &parsed(Vec::new())).unwrap(), 0);
        bind_topology(ctx, &mut [], &[]).unwrap();
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn unicode_field_exhaustion_charges_one_scalar_visit() {
    with_work(1, |ctx| {
        let mut scanner = TextScanner::new("é");
        assert_eq!(scanner.next_field(ctx).unwrap(), Some("é"));
        assert_eq!(scanner.next_field(ctx).unwrap(), None);
        assert_eq!(scanner.position, "é".len());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn token_exhaustion_preserves_actual_whitespace_and_delimiter_visits() {
    with_work(6, |ctx| {
        let mut scanner = TextScanner::new("a b");
        // First token: nonspace probe, 'a', delimiter. Second: space,
        // nonspace probe, 'b'. No terminal source step remains.
        assert_eq!(scanner.next(ctx).unwrap(), Some("a"));
        assert_eq!(scanner.next(ctx).unwrap(), Some("b"));
        assert_eq!(scanner.next(ctx).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn legacy_id_exhaustion_preserves_separator_probe_and_token_visit() {
    with_work(2, |ctx| {
        let mut scanner = TextScanner::new("1");
        assert_eq!(scanner.next_legacy_id(ctx).unwrap(), Some("1"));
        assert_eq!(scanner.next_legacy_id(ctx).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn whitespace_exhaustion_preserves_unicode_and_ascii_rules() {
    with_work(2, |ctx| {
        let mut scanner = TextScanner::new(" \u{2003}");
        scanner.skip_whitespace(ctx).unwrap();
        assert!(scanner.is_done());
        assert_eq!(scanner.next(ctx).unwrap(), None);
    });
    with_work(2, |ctx| {
        let mut scanner = TextScanner::new_ascii("\u{2003}");
        assert_eq!(scanner.next(ctx).unwrap(), Some("\u{2003}"));
        assert_eq!(scanner.next(ctx).unwrap(), None);
    });
}

#[test]
fn mapped_count_exhaustion_charges_only_nodes_groups_and_chains() {
    let name = || ElementMappedName {
        encoded: ";name.0".into(),
        resolved: Some("name".into()),
        string_ids: Vec::new(),
        topology_ids: Vec::new(),
    };
    let map = parsed(vec![
        ElementMapGroup {
            indexed_name: "Edge".into(),
            children: Vec::new(),
            names: vec![vec![name(), name()], Vec::new()],
        },
        ElementMapGroup {
            indexed_name: "Face".into(),
            children: Vec::new(),
            names: vec![vec![name()]],
        },
    ]);
    with_work(5, |ctx| {
        // Two groups and three chains; chain lengths are fixed metadata.
        assert_eq!(element_map_size(ctx, &map).unwrap(), 3);
        assert_eq!(ctx.resource_refusal(), None);
    });
    with_work(6, |ctx| {
        // One node, two groups and three chains.
        assert_eq!(mapped_name_count(ctx, &map).unwrap(), 3);
        assert_eq!(ctx.resource_refusal(), None);
    });
    with_work(1, |ctx| {
        assert_eq!(mapped_name_count(ctx, &parsed(Vec::new())).unwrap(), 0);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn field_refusal_preserves_unvisited_suffix_and_original_fuse() {
    with_work(1, |ctx| {
        let mut scanner = TextScanner::new("a.unvisited");
        scanner.next_field(ctx).unwrap_err();
        let original = ctx.resource_refusal().unwrap();
        assert_eq!(original.operation, "FreeCAD element-map field scan");
        assert_eq!((original.used, original.additional), (1, 1));
        assert_eq!(scanner.position, 1);
        assert_original(scanner.next_field(ctx), original);
        assert_eq!(scanner.position, 1);
    });
}

#[test]
fn empty_and_completed_element_helpers_return_original_refusal() {
    with_work(0, |ctx| {
        let mut completed = TextScanner::new("");
        assert_eq!(completed.next_field(ctx).unwrap(), Some(""));
        ctx.charge_work(1, "test original element fuse")
            .unwrap_err();
        let original = ctx.resource_refusal().unwrap();
        let mut scanner = TextScanner::new("");
        assert_original(scanner.skip_whitespace(ctx), original);
        assert_original(scanner.next(ctx), original);
        assert_original(scanner.next_field(ctx), original);
        assert_original(completed.next_field(ctx), original);
        assert_original(scanner.next_legacy_id(ctx), original);
        assert_original(parse_string_table(ctx, b"", 0, false), original);
        assert_original(
            parse_legacy_records(ctx, &mut TextScanner::new_ascii(""), 0),
            original,
        );
        let map = parsed(Vec::new());
        assert_original(element_map_size(ctx, &map), original);
        assert_original(mapped_name_count(ctx, &map), original);
        assert_original(bind_topology(ctx, &mut [], &[]), original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn legacy_record_and_zero_id_ranges_cost_only_actual_records() {
    with_work(17, |ctx| {
        let mut scanner = TextScanner::new_ascii("Edge1 x 0");
        // One record visit; token scans cost 7 + 4 + 3; copying 'x'
        // and parsing '0' cost one each. The zero ID range has no visits.
        let records = parse_legacy_records(ctx, &mut scanner, 1).unwrap();
        assert_eq!(records.data.len(), 1);
        assert_eq!(records.data[0].indexed_name, "Edge1");
        assert_eq!(records.data[0].mapped_name, "x");
        assert!(records.data[0].string_ids.is_empty());
        assert!(scanner.is_done());
        assert_eq!(ctx.resource_refusal(), None);
    });
}
