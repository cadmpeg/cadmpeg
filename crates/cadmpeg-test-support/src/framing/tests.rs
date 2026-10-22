// SPDX-License-Identifier: Apache-2.0
//! Table identity, source containment and nearest-owner coverage controls.

use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, View};
use cadmpeg_ir::source_fidelity::parts::{PartFraming, PartLevel};

use super::{FrameParent, FramedPart, PartParent, PartTable};

const TABLE: &str = include_str!("../../../../scripts/part-framing.tsv");

fn level(name: &str) -> PartLevel {
    PartLevel::parse(name).unwrap()
}

fn frame<'a>(
    ctx: &DecodeContext<'_>,
    name: &str,
    view: View<'a>,
    parent: FrameParent,
) -> FramedPart<'a> {
    FramedPart::new(ctx, level(name), view, parent, Vec::new(), Vec::new()).unwrap()
}

#[test]
fn immutable_table_has_all_exact_registered_rows_and_parent_alternatives() {
    let table = PartTable::load().unwrap();
    assert_eq!(table.len(), 448);
    assert!(!table.is_empty());
    assert_eq!(
        table.rule(level("nx.om_id_store")).unwrap().parents(),
        [
            PartParent::Level(level("nx.entry_stream")),
            PartParent::Level(level("nx.om_section")),
        ]
    );
    assert!(table
        .rule(level("catia.object_stream_record"))
        .unwrap()
        .parents()
        .contains(&PartParent::Level(level("catia.object_stream_record"))));
    for name in [
        "nx.om_store_control_block",
        "nx.om_store_column_block",
        "nx.rmfastload_physical_span",
    ] {
        let rule = table.rule(level(name)).unwrap();
        assert!(rule.extent_only());
        assert_eq!(rule.level().framing(), PartFraming::Sequential);
    }
}

#[test]
fn table_rejects_changed_headers_duplicate_rows_and_unregistered_levels() {
    assert!(PartTable::parse(&TABLE.replacen("codec\tlevel", "format\tlevel", 1)).is_err());
    let row = TABLE.lines().nth(1).unwrap();
    assert!(PartTable::parse(&format!("{TABLE}{row}\n")).is_err());
    assert!(PartTable::parse(&TABLE.replacen("zip\tarchive\t", "zip\tinvented\t", 1)).is_err());
    assert!(
        PartTable::parse(&TABLE.replacen("zip.archive\tlocated", "zip.missing\tlocated", 1))
            .is_err()
    );
}

#[test]
fn table_rejects_missing_parents_and_registry_framing_disagreement() {
    let missing = TABLE
        .lines()
        .filter(|line| !line.starts_with("zip\tarchive\t"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(PartTable::parse(&missing).is_err());
    assert!(PartTable::parse(&TABLE.replacen(
        "zip\tarchive\tfile\tlocated",
        "zip\tarchive\tfile\tterminated",
        1
    ))
    .is_err());
    assert!(PartTable::parse(&TABLE.replacen(
        "zip\tarchive\tfile\tlocated\t",
        "zip\tarchive\tfile\tlocated\textent only:",
        1
    ))
    .is_err());
}

#[test]
fn framing_and_dispatch_use_absolute_ordered_bounded_source_ranges() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789", &arena, &DecodePolicy::default()).unwrap();
    let view = root.child(2, 9).unwrap();
    let part = FramedPart::new(
        &ctx,
        level("step.exchange"),
        view,
        FrameParent::File,
        vec![2..3, 8..9],
        vec![3..5],
    )
    .unwrap();
    assert_eq!(part.capture().offset(), 2);
    assert_eq!(part.capture().byte_len(), 7);
    assert_eq!(part.framing(), [2..3, 8..9]);
    assert_eq!(part.dispatch(), [3..5]);
    assert_eq!(part.content_ranges(), [3..8]);
    for invalid in [
        vec![1..3],
        vec![8..10],
        vec![3..3],
        vec![5..7, 4..5],
        vec![3..6, 5..7],
    ] {
        assert!(FramedPart::new(
            &ctx,
            level("step.exchange"),
            view,
            FrameParent::File,
            invalid,
            Vec::new()
        )
        .is_err());
    }
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn detached_and_mixed_session_inventories_are_rejected() {
    let arena = DecodeArena::new();
    let other_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default()).unwrap();
    let (other_ctx, other) =
        DecodeContext::from_root_bytes(b"source", &other_arena, &DecodePolicy::default()).unwrap();
    assert!(FramedPart::new(
        &ctx,
        level("step.exchange"),
        View::over_retained(b"source"),
        FrameParent::File,
        Vec::new(),
        Vec::new()
    )
    .is_err());
    let frames = [frame(&other_ctx, "step.exchange", other, FrameParent::File)];
    assert!(PartTable::load().unwrap().validate(&ctx, &frames).is_err());
    let part = frame(&ctx, "step.exchange", root, FrameParent::File);
    assert!(part.with_external_dispatch(&ctx, vec![other]).is_err());
}

#[test]
fn both_declared_parent_alternatives_are_admitted_without_a_guessed_parent() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"01234567890123456789", &arena, &DecodePolicy::default())
            .unwrap();
    let table = PartTable::load().unwrap();
    let direct = [
        frame(&ctx, "nx.entry_stream", root, FrameParent::File),
        frame(
            &ctx,
            "nx.om_id_store",
            root.child(2, 8).unwrap(),
            FrameParent::Part(0),
        ),
    ];
    table.validate(&ctx, &direct).unwrap();
    let nested = [
        frame(&ctx, "nx.entry_stream", root, FrameParent::File),
        frame(&ctx, "nx.om_section", root, FrameParent::Part(0)),
        frame(
            &ctx,
            "nx.om_id_store",
            root.child(2, 8).unwrap(),
            FrameParent::Part(1),
        ),
    ];
    table.validate(&ctx, &nested).unwrap();
    let wrong = [
        frame(&ctx, "nx.entry_stream", root, FrameParent::File),
        frame(
            &ctx,
            "step.header_record",
            root.child(2, 8).unwrap(),
            FrameParent::Part(0),
        ),
    ];
    assert!(table.validate(&ctx, &wrong).is_err());
}

#[test]
fn duplicate_overlapping_outside_and_forward_children_are_rejected() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"01234567890123456789", &arena, &DecodePolicy::default())
            .unwrap();
    let table = PartTable::load().unwrap();
    for (first, second) in [(3..7, 3..7), (3..7, 6..9)] {
        let frames = [
            frame(&ctx, "nx.entry_stream", root, FrameParent::File),
            frame(
                &ctx,
                "nx.om_section",
                root.child(first.start, first.end).unwrap(),
                FrameParent::Part(0),
            ),
            frame(
                &ctx,
                "nx.om_section",
                root.child(second.start, second.end).unwrap(),
                FrameParent::Part(0),
            ),
        ];
        assert!(table.validate(&ctx, &frames).is_err());
    }
    let outside = [
        frame(
            &ctx,
            "nx.entry_stream",
            root.child(2, 8).unwrap(),
            FrameParent::File,
        ),
        frame(
            &ctx,
            "nx.om_section",
            root.child(3, 9).unwrap(),
            FrameParent::Part(0),
        ),
    ];
    assert!(table.validate(&ctx, &outside).is_err());
    let forward = [frame(&ctx, "nx.om_section", root, FrameParent::Part(0))];
    assert!(table.validate(&ctx, &forward).is_err());
}

#[test]
fn shared_root_uses_the_actual_host_alias_and_keeps_external_dispatch() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        b"0123456789012345678901234567890123456789",
        &arena,
        &DecodePolicy::default(),
    )
    .unwrap();
    let member = ctx
        .register_named_slice(root, ByteRange { start: 8, end: 32 }, "member/model.smb")
        .unwrap();
    let host = frame(&ctx, "f3d.brep_blob", member, FrameParent::Part(1))
        .with_external_dispatch(&ctx, vec![root.child(2, 7).unwrap()])
        .unwrap();
    let frames = [
        frame(&ctx, "zip.archive", root, FrameParent::File),
        frame(
            &ctx,
            "zip.entry",
            root.child(8, 32).unwrap(),
            FrameParent::Part(0),
        ),
        host,
        frame(&ctx, "asm.sab_stream", member, FrameParent::Host(2)),
    ];
    let table = PartTable::load().unwrap();
    table.validate(&ctx, &frames).unwrap();
    assert_eq!(frames[2].capture().stream(), "member/model.smb");
    assert_eq!(frames[2].external_dispatch()[0].start(), 2);
    assert_eq!(
        table.owned_ranges(&ctx, &frames, 0).unwrap(),
        [0..8, 32..40]
    );
    assert!(table.owned_ranges(&ctx, &frames, 2).unwrap().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_shared_root_cannot_be_filed_as_a_physical_root_or_an_unrelated_host() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789", &arena, &DecodePolicy::default()).unwrap();
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 2, end: 8 }, "member")
        .unwrap();
    let table = PartTable::load().unwrap();
    assert!(table
        .validate(
            &ctx,
            &[frame(&ctx, "asm.sab_stream", alias, FrameParent::File)]
        )
        .is_err());
    let frames = [
        frame(&ctx, "step.exchange", root, FrameParent::File),
        frame(&ctx, "asm.sab_stream", alias, FrameParent::Host(0)),
    ];
    assert!(table.validate(&ctx, &frames).is_err());
}

#[test]
fn uncovered_bytes_belong_to_the_enclosing_part_not_to_framing() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"01234567890123456789", &arena, &DecodePolicy::default())
            .unwrap();
    let frames = [
        FramedPart::new(
            &ctx,
            level("step.exchange"),
            root,
            FrameParent::File,
            vec![0..2, 18..20],
            Vec::new(),
        )
        .unwrap(),
        frame(
            &ctx,
            "step.header_section",
            root.child(4, 9).unwrap(),
            FrameParent::Part(0),
        ),
    ];
    let table = PartTable::load().unwrap();
    assert_eq!(table.owned_ranges(&ctx, &frames, 0).unwrap(), [2..4, 9..18]);
    assert_eq!(frames[0].framing(), [0..2, 18..20]);
}

#[test]
fn extent_only_sequential_failure_consumes_the_parent_tail() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        b"012345678901234567890123",
        &arena,
        &DecodePolicy::default(),
    )
    .unwrap();
    let frames = [
        frame(&ctx, "nx.entry_stream", root, FrameParent::File),
        frame(
            &ctx,
            "nx.om_offset_store",
            root.child(2, 22).unwrap(),
            FrameParent::Part(0),
        ),
        frame(
            &ctx,
            "nx.om_store_control_block",
            root.child(4, 8).unwrap(),
            FrameParent::Part(1),
        ),
        frame(
            &ctx,
            "nx.om_store_column_block",
            root.child(8, 12).unwrap(),
            FrameParent::Part(1),
        ),
    ];
    let table = PartTable::load().unwrap();
    let failed = table.failure_extent(&ctx, &frames, 3).unwrap();
    assert_eq!((failed.start(), failed.end()), (8, 22));
    assert_eq!(frames[3].view().window(), b"8901");
    let located = table.failure_extent(&ctx, &frames, 1).unwrap();
    assert_eq!((located.start(), located.end()), (2, 22));
}

#[test]
fn an_expanded_host_is_proven_by_its_registered_compressed_extent() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        b"0123456789012345678901234567890123456789",
        &arena,
        &DecodePolicy::default(),
    )
    .unwrap();
    let compressed = root.child(8, 32).unwrap();
    let mut writer = ctx.begin_expand(ExpandSpec::Exact(8)).unwrap();
    writer.write(b"ASMBYTES").unwrap();
    let expanded = writer
        .finalize_from(compressed, "expanded/model.smb")
        .unwrap();
    let frames = [
        frame(&ctx, "zip.archive", root, FrameParent::File),
        frame(&ctx, "zip.entry", compressed, FrameParent::Part(0)),
        frame(&ctx, "f3d.brep_blob", expanded, FrameParent::Part(1)),
        frame(&ctx, "asm.sab_stream", expanded, FrameParent::Host(2)),
    ];
    let table = PartTable::load().unwrap();
    table.validate(&ctx, &frames).unwrap();
    assert_eq!(frames[2].capture().byte_len(), 8);
    assert_eq!(frames[1].capture().byte_len(), 24);
    assert!(table.owned_ranges(&ctx, &frames, 1).unwrap().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_composite_host_maps_only_its_actual_source_segments() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        b"01234567890123456789012345678901",
        &arena,
        &DecodePolicy::default(),
    )
    .unwrap();
    let joined = ctx
        .concat_named_views(
            &[root.child(4, 8).unwrap(), root.child(12, 16).unwrap()],
            "joined/parasolid",
        )
        .unwrap();
    let frames = [
        frame(&ctx, "sldprt.block", root, FrameParent::File),
        frame(
            &ctx,
            "sldprt.parasolid_chain_section",
            root,
            FrameParent::Part(0),
        ),
        frame(
            &ctx,
            "sldprt.chained_parasolid_stream",
            joined,
            FrameParent::Part(1),
        ),
        frame(&ctx, "parasolid.stream", joined, FrameParent::Host(2)),
    ];
    let table = PartTable::load().unwrap();
    table.validate(&ctx, &frames).unwrap();
    assert_eq!(
        table.owned_ranges(&ctx, &frames, 1).unwrap(),
        [0..4, 8..12, 16..32]
    );
    assert_eq!(frames[2].view().window(), b"45672345");
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn empty_physical_parts_share_boundaries_without_owning_sibling_bytes() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::service()).expect("source");
    let frames = vec![
        frame(&ctx, "zip.archive", root, FrameParent::File),
        frame(
            &ctx,
            "zip.central_directory",
            root.child(0, 0).expect("actual empty directory"),
            FrameParent::Part(0),
        ),
        frame(&ctx, "zip.end_record", root, FrameParent::Part(0)),
    ];
    PartTable::load()
        .expect("table")
        .validate(&ctx, &frames)
        .expect("an empty extent owns no bytes");
}
