// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use super::*;
use crate::codec::{CodecError, DecodeResult};
use crate::document::CadIr;
use crate::report::{DecodeReport, LossCategory, LossCode, LossNote, ProfileVersions, Severity};
use crate::source_fidelity::{
    AddressSpaceLedger, CanonicalSpaceId, LedgerSpan, SerializedOrigin, SerializedRange,
};
use crate::units::Units;

fn desktop() -> DecodePolicy {
    DecodePolicy::default()
}

fn service() -> DecodePolicy {
    DecodePolicy::service()
}

fn strict() -> DecodePolicy {
    DecodePolicy {
        mode: DecodeMode::Strict,
        limits: ResourceLimits::desktop(),
    }
}

fn tight(edit: impl FnOnce(&mut ResourceLimits)) -> DecodePolicy {
    let mut limits = ResourceLimits::desktop();
    edit(&mut limits);
    DecodePolicy {
        mode: DecodeMode::Salvage,
        limits,
    }
}

fn dummy_result() -> DecodeResult {
    result_with_bodies(&[])
}

fn result_with_bodies(ids: &[&str]) -> DecodeResult {
    use crate::ids::BodyId;
    use crate::topology::{Body, BodyKind};

    let mut ir = CadIr::empty(Units::default());
    for id in ids {
        ir.model.bodies.push(Body {
            id: BodyId((*id).to_owned()),
            kind: BodyKind::default(),
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }
    let report = DecodeReport {
        format: "test".to_string(),
        container_only: false,
        geometry_transferred: false,
        losses: Vec::new(),
        notes: Vec::new(),
        retention_degraded: false,
        profile_versions: ProfileVersions::default(),
    };
    DecodeResult::new(ir, report)
}

#[test]
fn window_escape_is_refused_at_the_request_site() {
    let bytes: &[u8] = &[0u8; 200];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (_ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    assert!(root.child(100, 264).is_none());
    let parent = root.child(0, 157).unwrap();
    assert!(parent.child(0, 164).is_none());

    let mut child = root.child(100, 157).unwrap();
    assert_eq!(child.start(), 100);
    assert_eq!(child.seek(50), None, "seek below start is refused");
    assert_eq!(child.position(), 100);
    assert_eq!(child.seek(120), Some(()));
    assert_eq!(child.seek(158), None, "seek past end is refused");
}

#[test]
fn probe_reads_advance_only_on_success() {
    let bytes: &[u8] = &[1u8, 0, 2, 0, 0, 0, 9];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (_ctx, mut view) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    assert_eq!(view.u16_le(), Some(1));
    assert_eq!(view.u32_le(), Some(2));
    assert_eq!(view.remaining(), 1);
    assert_eq!(view.u16_le(), None);
    assert_eq!(view.position(), 6, "a failed read does not advance");

    match view.req_u16_le() {
        Err(ParseError {
            kind: ParseErrorKind::UnexpectedEof { needed, remaining },
            ..
        }) => {
            assert_eq!(needed, 2);
            assert_eq!(remaining, 1);
        }
        other => panic!("expected UnexpectedEof, got {other:?}"),
    }

    assert_eq!(view.counted(1, 1).map(BoundedCount::get), Some(1));
    assert!(view.counted(2, 1).is_none(), "impossible count is refused");
}

#[test]
fn fuse_then_swallow_still_fails_at_finish() {
    let bytes: &[u8] = &[0u8; 16];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_alloc_bytes = 0);
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let first = ctx.exact_vec::<u64>(root.counted(4, 1).unwrap());
    assert!(matches!(first, Err(CodecError::ResourceLimit(_))));
    assert!(ctx.is_fused());

    let swallowed = ctx.exact_vec::<u8>(root.counted(1, 1).unwrap()).ok();
    assert!(swallowed.is_none());

    match ctx.finish(Ok(dummy_result())) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::AllocBytes);
            assert_eq!(limit.reason, ResourceFailure::BudgetExceeded);
        }
        other => panic!("expected fused ResourceLimit, got {other:?}"),
    }
}

#[test]
fn depth_guard_recurses_to_the_limit_and_fuses_beyond() {
    fn recurse(ctx: &DecodeContext<'_>, remaining: u32) -> Result<u32, CodecError> {
        let _guard = ctx.descend()?;
        if remaining == 0 {
            Ok(ctx.current_depth())
        } else {
            recurse(ctx, remaining - 1)
        }
    }

    let bytes: &[u8] = &[0u8; 4];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_depth = 8);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    assert_eq!(recurse(&ctx, 7).unwrap(), 8);
    assert_eq!(ctx.current_depth(), 0);
    assert!(!ctx.is_fused());

    match recurse(&ctx, 8) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::Depth);
        }
        other => panic!("expected Depth ResourceLimit, got {other:?}"),
    }
    assert!(ctx.is_fused());
}

#[test]
fn sequential_siblings_do_not_exhaust_the_depth_gauge() {
    let bytes: &[u8] = &[0u8; 4];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_depth = 4);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    for _ in 0..1000 {
        let guard = ctx.descend().unwrap();
        assert_eq!(ctx.current_depth(), 1);
        drop(guard);
        assert_eq!(ctx.current_depth(), 0);
    }
    assert!(!ctx.is_fused());
}

#[test]
fn exact_vec_enforces_capacity_and_full_population() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let three = root.counted(3, 1).unwrap();
    let mut partial = ctx.exact_vec::<u32>(three).unwrap();
    assert!(partial.is_empty());
    partial.push(10).unwrap();
    assert!(partial.finish_exact().is_err(), "1 of 3 is not exact");

    let mut full = ctx.exact_vec::<u32>(three).unwrap();
    full.push(1).unwrap();
    full.push(2).unwrap();
    full.push(3).unwrap();
    assert!(full.push(4).is_err(), "push past capacity is refused");
    assert_eq!(full.finish_exact().unwrap(), vec![1, 2, 3]);

    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 24);
}

#[test]
fn grow_vec_charges_per_element_and_fuses_on_refusal() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_alloc_bytes = 3);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let mut grow = ctx.grow_vec::<u16>();
    grow.try_push(1).unwrap();
    assert_eq!(grow.len(), 1);
    assert!(grow.try_push(2).is_err(), "second element exceeds 3 bytes");
    assert!(ctx.is_fused());
}

#[test]
fn read_counted_reads_and_charges() {
    let bytes: &[u8] = &[1u8, 0, 0, 0, 2, 0, 0, 0];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, mut view) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let count = view.counted(2, 4).unwrap();
    let values = ctx.read_counted(&mut view, count, |v| v.u32_le()).unwrap();
    assert_eq!(values, vec![1u32, 2]);
    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 8);
}

#[test]
fn read_counted_classifies_exhausted_input_as_truncated() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, mut view) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let count = view.counted(2, 4).unwrap();
    let result = ctx.read_counted(&mut view, count, |v| v.u64_le());
    assert!(matches!(result, Err(CodecError::Truncated { .. })));
}

#[test]
fn read_counted_classifies_in_window_rejection_as_malformed() {
    let bytes: &[u8] = &[0x01, 0xFF, 0x02, 0x03, 0x04, 0x05];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, mut view) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let count = view.counted(4, 1).unwrap();
    let result = ctx.read_counted(&mut view, count, |v| v.u8().filter(|tag| *tag < 0x80));
    assert!(matches!(result, Err(CodecError::Malformed(_))));
}

#[test]
fn expand_exact_mismatch_is_rejected() {
    let bytes: &[u8] = &[0u8; 100];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let mut under = ctx.begin_expand(root, ExpandSpec::Exact(10)).unwrap();
    under.write(&[0u8; 6]).unwrap();
    assert!(under.finalize().is_err());

    let mut over = ctx.begin_expand(root, ExpandSpec::Exact(10)).unwrap();
    assert!(over.write(&[0u8; 12]).is_err());
}

#[test]
fn expand_finalize_registers_space_and_grows_input_basis() {
    let bytes: &[u8] = &[0u8; 100];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    assert_eq!(ctx.input_basis(), 100);
    assert_eq!(ctx.spaces_len(), 1);
    let alloc_before = ctx.allowance_of(ResourceDimension::AllocBytes);

    let mut writer = ctx.begin_expand(root, ExpandSpec::Exact(10)).unwrap();
    writer.write(&[1u8; 4]).unwrap();
    writer.write(&[2u8; 6]).unwrap();
    assert_eq!(writer.written(), 10);
    let (space, view) = writer.finalize().unwrap();

    assert_eq!(space.index(), 1);
    assert_eq!(view.space(), space);
    assert_eq!(view.window().len(), 10);
    assert_eq!(ctx.spaces_len(), 2);

    assert_eq!(ctx.input_basis(), 110);
    let alloc_after = ctx.allowance_of(ResourceDimension::AllocBytes);
    assert!(alloc_after > alloc_before, "allowance grows with the basis");
}

#[test]
fn derived_space_concats_multiple_extents_and_charges_alloc() {
    let bytes: &[u8] = &[10, 11, 12, 13, 20, 21, 22, 23, 24, 25];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let first = root.child(0, 4).unwrap();
    let second = root.child(6, 10).unwrap();
    let basis_before = ctx.input_basis();

    let writer = ctx
        .begin_derived_space(&[first, second], DerivedKind::Concat)
        .unwrap();
    assert_eq!(writer.written(), 8);
    let (space, view) = writer.finalize().unwrap();

    assert_eq!(space.index(), 1);
    assert_eq!(view.space(), space);
    assert_eq!(view.window(), &[10, 11, 12, 13, 22, 23, 24, 25]);
    assert_eq!(ctx.spaces_len(), 2);

    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 8);

    assert_eq!(ctx.input_basis(), basis_before);

    match ctx.space_origin(space).unwrap() {
        SpaceOrigin::Concat { segments } => {
            assert_eq!(segments.len(), 2);
            assert_eq!(segments[0].space, SpaceId::ROOT);
            assert_eq!(segments[0].range, ByteRange { start: 0, end: 4 });
            assert_eq!(segments[1].range, ByteRange { start: 6, end: 10 });
        }
        other => panic!("expected Concat origin, got {other:?}"),
    }
}

#[test]
fn derived_space_records_a_multi_input_transform() {
    let bytes: &[u8] = &[0xAA, 0xBB, 0xCC, 0xDD, 0x01, 0x02, 0x03, 0x04];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let dictionary = root.child(0, 4).unwrap();
    let data = root.child(4, 8).unwrap();
    let basis_before = ctx.input_basis();

    let mut writer = ctx
        .begin_derived_space(
            &[dictionary, data],
            DerivedKind::Transform(TransformKind::Decompress),
        )
        .unwrap();
    writer.write(&[1, 2, 3, 4, 5, 6]).unwrap();
    let (space, view) = writer.finalize().unwrap();

    assert_eq!(view.window(), &[1, 2, 3, 4, 5, 6]);
    assert_eq!(ctx.charged(ResourceDimension::DecompressedBytes), 6);
    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 0);
    assert_eq!(ctx.input_basis(), basis_before + 6);
    match ctx.space_origin(space).unwrap() {
        SpaceOrigin::Transform { inputs, kind } => {
            assert_eq!(kind, TransformKind::Decompress);
            assert_eq!(inputs.len(), 2);
            assert_eq!(inputs[0].range, ByteRange { start: 0, end: 4 });
            assert_eq!(inputs[1].range, ByteRange { start: 4, end: 8 });
        }
        other => panic!("expected Transform origin, got {other:?}"),
    }
}

#[test]
fn derived_space_registers_only_on_finalize_and_fuses_on_refusal() {
    let bytes: &[u8] = &[0u8; 16];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    {
        let extent = root.child(0, 3).unwrap();
        let _abandoned = ctx
            .begin_derived_space(&[extent], DerivedKind::Concat)
            .unwrap();
    }
    assert_eq!(ctx.spaces_len(), 1, "an unfinalized space never registers");

    let arena2 = DecodeArena::new();
    let tight = tight(|limits| limits.max_alloc_bytes = 4);
    let (ctx2, root2) = DecodeContext::from_root_bytes(bytes, &arena2, &tight).unwrap();
    let head = root2.child(0, 4).unwrap();
    let tail = root2.child(4, 5).unwrap();
    match ctx2.begin_derived_space(&[head, tail], DerivedKind::Concat) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::AllocBytes);
        }
        other => panic!("expected AllocBytes ResourceLimit, got {other:?}"),
    }
    assert!(ctx2.is_fused());
}

#[test]
fn register_slice_aliases_a_stored_entry_without_copying() {
    let bytes: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let basis_before = ctx.input_basis();

    let (space, view) = ctx
        .register_slice(root, ByteRange { start: 2, end: 6 })
        .unwrap();

    assert_eq!(space.index(), 1);
    assert_eq!(view.space(), space);
    assert_eq!(view.window(), &[2, 3, 4, 5]);
    assert_eq!(view.position(), 0);
    assert_eq!(view.start(), 0);
    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 0);
    assert_eq!(ctx.input_basis(), basis_before);
    match ctx.space_origin(space).unwrap() {
        SpaceOrigin::Slice { parent, range } => {
            assert_eq!(parent, SpaceId::ROOT);
            assert_eq!(range, ByteRange { start: 2, end: 6 });
        }
        other => panic!("expected Slice origin, got {other:?}"),
    }
}

#[test]
fn register_slice_refuses_a_range_escaping_the_parent() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    match ctx.register_slice(root, ByteRange { start: 4, end: 9 }) {
        Err(CodecError::Malformed(_)) => {}
        other => panic!("expected Malformed refusal, got {other:?}"),
    }
    assert_eq!(ctx.spaces_len(), 1, "a refused slice registers no space");
}

#[test]
fn derived_concat_refuses_transform_writes() {
    let bytes: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let extent = root.child(0, 4).unwrap();
    let mut writer = ctx
        .begin_derived_space(&[extent], DerivedKind::Concat)
        .unwrap();
    match writer.write(&[99, 99]) {
        Err(CodecError::Malformed(_)) => {}
        other => panic!("expected Malformed refusal, got {other:?}"),
    }
    let (space, view) = writer.finalize().unwrap();
    assert_eq!(view.window(), &[1, 2, 3, 4]);
    match ctx.space_origin(space).unwrap() {
        SpaceOrigin::Concat { segments } => {
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0].range, ByteRange { start: 0, end: 4 });
        }
        other => panic!("expected Concat origin, got {other:?}"),
    }
}

#[test]
fn transaction_rolls_back_position_but_not_charges() {
    let bytes: &[u8] = &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, mut root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let charged_before = ctx.charged(ResourceDimension::AllocBytes);
    let start = root.position();

    let failed: Option<()> = root.transaction(|view| {
        let count = view.counted(4, 1)?;
        let _reserved = ctx.exact_vec::<u8>(count).ok()?;
        view.take(1000)?;
        Some(())
    });
    assert!(failed.is_none());
    assert_eq!(root.position(), start, "position rolled back");
    assert!(
        ctx.charged(ResourceDimension::AllocBytes) > charged_before,
        "charges do not roll back"
    );

    let ok = root.transaction(|view| {
        let a = view.u32_le()?;
        let b = view.u32_le()?;
        Some((a, b))
    });
    assert_eq!(ok, Some((1, 2)));
    assert_eq!(root.position(), 8);
}

#[test]
fn read_root_enforces_the_input_limit() {
    let arena = DecodeArena::new();
    let over = tight(|limits| limits.max_input_bytes = 50);
    let mut reader = Cursor::new(vec![0u8; 100]);
    match DecodeContext::read_root(&mut reader, &arena, &over) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::InputBytes);
        }
        other => panic!("expected InputBytes ResourceLimit, got {other:?}"),
    }

    let arena2 = DecodeArena::new();
    let policy = desktop();
    let mut reader = Cursor::new(vec![7u8; 40]);
    let (ctx, root) = DecodeContext::read_root(&mut reader, &arena2, &policy).unwrap();
    assert_eq!(root.window().len(), 40);
    assert_eq!(ctx.charged(ResourceDimension::InputBytes), 40);
    assert_eq!(ctx.input_basis(), 40);
    assert_eq!(root.space(), SpaceId::ROOT);
}

#[test]
fn arena_borrows_stay_valid_across_later_allocations() {
    let arena = DecodeArena::new();
    let first = arena.alloc(vec![1u8, 2, 3].into_boxed_slice());
    for value in 0u16..1000 {
        let _ = arena.alloc(vec![value as u8; 32].into_boxed_slice());
    }
    assert_eq!(
        first,
        &[1u8, 2, 3][..],
        "an early borrow survives many pushes"
    );
}

#[test]
fn unresolved_tickets_fail_strict_and_auto_drop_in_salvage() {
    let bytes: &[u8] = &[0u8; 8];

    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("entity"));
    assert_eq!(ctx.unresolved_tickets(), 1);
    ctx.resolve(ticket, RecordDisposition::Structural);
    assert_eq!(ctx.unresolved_tickets(), 0);
    assert!(ctx.finish(Ok(dummy_result())).is_ok());

    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let _ticket = ctx.commit_record(root.location(), RecordKind("entity"));
    match ctx.finish(Ok(dummy_result())) {
        Err(CodecError::Malformed(_)) => {}
        other => panic!("expected strict unresolved-ticket error, got {other:?}"),
    }

    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let _ticket = ctx.commit_record(root.location(), RecordKind("entity"));
    let result = ctx.finish(Ok(dummy_result())).unwrap();
    assert_eq!(result.report.losses.len(), 1);
    let loss = &result.report.losses[0];
    assert_eq!(loss.category, LossCategory::Other);
    assert_eq!(loss.severity, Severity::Warning);
    assert!(loss.message.contains("`entity`"), "{}", loss.message);
    assert!(
        loss.message.contains("auto-dropped in salvage mode"),
        "{}",
        loss.message
    );
}

#[test]
fn transfer_accounting_passes_trivially_without_tickets() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    assert!(ctx.finish(Ok(dummy_result())).is_ok());
}

#[test]
fn transfer_accounting_accepts_consistent_dispositions() {
    let bytes: &[u8] = &[7u8; 16];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let typed = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(
        typed,
        RecordDisposition::Typed {
            outputs: vec!["body/0".to_owned()],
        },
    );

    let retention = ctx.retain(&bytes[..8]).unwrap();
    let record = retention.range().unwrap().blob().as_str().to_owned();
    let retained = ctx.commit_record(root.location(), RecordKind("blob"));
    ctx.resolve(
        retained,
        RecordDisposition::Retained {
            records: vec![record],
        },
    );

    let structural = ctx.commit_record(root.location(), RecordKind("header"));
    ctx.resolve(structural, RecordDisposition::Structural);

    assert!(ctx.finish(Ok(result_with_bodies(&["body/0"]))).is_ok());
}

fn source_ledger(length: u64, class: crate::source_fidelity::SpanClass) -> DecodeResult {
    use crate::source_fidelity::{
        AddressSpaceLedger, CanonicalSpaceId, LedgerCapability, LedgerLevel, LedgerSpan,
        SerializedOrigin, SerializedRange, SourceFidelity,
    };

    let mut result = dummy_result();
    result.source_fidelity = SourceFidelity::new(
        LedgerLevel::L1,
        LedgerCapability::Accounted,
        vec![AddressSpaceLedger {
            id: CanonicalSpaceId::source(),
            length,
            origin: SerializedOrigin::Root,
            spans: vec![LedgerSpan {
                range: SerializedRange {
                    start: 0,
                    end: length,
                },
                class,
                owner: "test".to_string(),
                meaning: "test".to_string(),
                digest: String::new(),
                retained: None,
            }],
        }],
    );
    result
}

#[test]
fn transfer_accounting_accepts_disposition_tiled_by_the_ledger() {
    let bytes: &[u8] = &[0u8; 16];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let typed = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(
        typed,
        RecordDisposition::Typed {
            outputs: vec!["body/0".to_owned()],
        },
    );

    let mut result = source_ledger(16, crate::source_fidelity::SpanClass::Opaque);
    result.ir = result_with_bodies(&["body/0"]).ir;
    assert!(ctx.finish(Ok(result)).is_ok());
}

#[test]
fn transfer_accounting_requires_every_derived_ticket_space_in_the_ledger() {
    let bytes: &[u8] = &[0u8; 16];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let (_space, derived) = ctx
        .register_slice(root, ByteRange { start: 4, end: 8 })
        .unwrap();
    let ticket = ctx.commit_record(derived.location(), RecordKind("body"));
    ctx.resolve(
        ticket,
        RecordDisposition::Typed {
            outputs: vec!["body/0".to_owned()],
        },
    );

    let mut result = source_ledger(16, crate::source_fidelity::SpanClass::Opaque);
    result.ir = result_with_bodies(&["body/0"]).ir;
    let error = ctx.finish(Ok(result)).expect_err("derived space is absent");
    assert!(error
        .to_string()
        .contains("runtime address space is absent"));
}

#[test]
fn transfer_accounting_matches_derived_space_by_origin() {
    let bytes: &[u8] = &[0u8; 16];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let (_space, derived) = ctx
        .register_slice(root, ByteRange { start: 4, end: 8 })
        .unwrap();
    let ticket = ctx.commit_record(derived.location(), RecordKind("body"));
    ctx.resolve(
        ticket,
        RecordDisposition::Typed {
            outputs: vec!["body/0".to_owned()],
        },
    );

    let mut result = source_ledger(16, crate::source_fidelity::SpanClass::Opaque);
    result.source_fidelity.spaces.push(AddressSpaceLedger {
        id: CanonicalSpaceId::entry("part"),
        length: 4,
        origin: SerializedOrigin::Slice {
            parent: CanonicalSpaceId::source(),
            range: SerializedRange { start: 4, end: 8 },
        },
        spans: vec![LedgerSpan {
            range: SerializedRange { start: 0, end: 4 },
            class: crate::source_fidelity::SpanClass::Opaque,
            owner: "test".to_string(),
            meaning: "test".to_string(),
            digest: String::new(),
            retained: None,
        }],
    });
    result.ir = result_with_bodies(&["body/0"]).ir;
    assert!(ctx.finish(Ok(result)).is_ok());
}

#[test]
fn transfer_accounting_rejects_disposition_absent_from_the_ledger_in_strict() {
    let bytes: &[u8] = &[0u8; 64];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(
        root.child(32, 33)
            .map_or_else(|| root.location(), View::location),
        RecordKind("body"),
    );
    ctx.resolve(
        ticket,
        RecordDisposition::Typed {
            outputs: vec!["body/0".to_owned()],
        },
    );

    let mut result = source_ledger(16, crate::source_fidelity::SpanClass::Opaque);
    result.ir = result_with_bodies(&["body/0"]).ir;
    match ctx.finish(Ok(result)) {
        Err(CodecError::Malformed(message)) => {
            assert!(
                message.contains("absent from the serialized ledger"),
                "{message}"
            );
        }
        other => panic!("expected strict ledger-reconciliation error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_ledger_violation_degrades_to_loss_in_salvage() {
    let bytes: &[u8] = &[0u8; 64];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(
        root.child(32, 33)
            .map_or_else(|| root.location(), View::location),
        RecordKind("body"),
    );
    ctx.resolve(
        ticket,
        RecordDisposition::Typed {
            outputs: vec!["body/0".to_owned()],
        },
    );

    let mut result = source_ledger(16, crate::source_fidelity::SpanClass::Opaque);
    result.ir = result_with_bodies(&["body/0"]).ir;
    let finished = ctx.finish(Ok(result)).unwrap();
    assert!(
        finished
            .report
            .losses
            .iter()
            .any(|loss| loss.message.contains("absent from the serialized ledger")),
        "expected a ledger-reconciliation loss note"
    );
}

fn result_with_unknown(id: &str) -> DecodeResult {
    use crate::ids::UnknownId;
    use crate::unknown::UnknownRecord;

    let mut result = dummy_result();
    result
        .ir
        .push_native_unknown(
            "test",
            crate::NativeUnknownRecord::from(&UnknownRecord {
                id: UnknownId(id.to_owned()),
                offset: 0,
                byte_len: 4,
                sha256: String::new(),
                data: None,
                links: Vec::new(),
            }),
        )
        .unwrap();
    result
}

#[test]
fn transfer_accounting_accepts_preserved_when_unknown_is_emitted() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("section"));
    ctx.resolve(
        ticket,
        RecordDisposition::Preserved {
            records: vec!["u/0".to_owned()],
        },
    );
    assert!(ctx.finish(Ok(result_with_unknown("u/0"))).is_ok());
}

#[test]
fn transfer_accounting_rejects_preserved_unknown_absent_from_arena_in_strict() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("section"));
    ctx.resolve(
        ticket,
        RecordDisposition::Preserved {
            records: vec!["u/999".to_owned()],
        },
    );
    match ctx.finish(Ok(result_with_unknown("u/0"))) {
        Err(CodecError::Malformed(message)) => {
            assert!(message.contains("transfer accounting"), "{message}");
            assert!(
                message.contains("absent from the native unknowns arena"),
                "{message}"
            );
        }
        other => panic!("expected strict native-unknowns error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_rejects_preserved_without_records_in_strict() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("section"));
    ctx.resolve(
        ticket,
        RecordDisposition::Preserved {
            records: Vec::new(),
        },
    );
    match ctx.finish(Ok(dummy_result())) {
        Err(CodecError::Malformed(message)) => {
            assert!(message.contains("names no unknown records"), "{message}");
        }
        other => panic!("expected strict empty-Preserved error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_rejects_typed_output_absent_from_ir_in_strict() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(
        ticket,
        RecordDisposition::Typed {
            outputs: vec!["body/999".to_owned()],
        },
    );
    match ctx.finish(Ok(dummy_result())) {
        Err(CodecError::Malformed(message)) => {
            assert!(message.contains("transfer accounting"), "{message}");
            assert!(message.contains("absent from the IR model"), "{message}");
        }
        other => panic!("expected strict IR-model error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_rejects_typed_without_outputs_in_strict() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(
        ticket,
        RecordDisposition::Typed {
            outputs: Vec::new(),
        },
    );
    match ctx.finish(Ok(dummy_result())) {
        Err(CodecError::Malformed(message)) => {
            assert!(message.contains("transfer accounting"), "{message}");
            assert!(message.contains("names no output entities"), "{message}");
        }
        other => panic!("expected strict transfer-accounting error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_rejects_unknown_retained_record_in_strict() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("blob"));
    ctx.resolve(
        ticket,
        RecordDisposition::Retained {
            records: vec!["not-a-real-digest".to_owned()],
        },
    );
    match ctx.finish(Ok(dummy_result())) {
        Err(CodecError::Malformed(message)) => {
            assert!(
                message.contains("absent from the retained ledger"),
                "{message}"
            );
        }
        other => panic!("expected strict retained-ledger error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_rejects_dropped_without_loss_note_in_strict() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(
        ticket,
        RecordDisposition::Dropped {
            loss: LossNote {
                code: LossCode::DecodeDiagnostic,
                category: LossCategory::Other,
                severity: Severity::Warning,
                message: "unhandled body variant".to_owned(),
                provenance: None,
            },
        },
    );
    match ctx.finish(Ok(dummy_result())) {
        Err(CodecError::Malformed(message)) => {
            assert!(message.contains("loss note is absent"), "{message}");
        }
        other => panic!("expected strict dropped-loss error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_accepts_dropped_when_loss_is_reflected() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let loss = LossNote {
        code: LossCode::DecodeDiagnostic,
        category: LossCategory::Other,
        severity: Severity::Warning,
        message: "unhandled body variant".to_owned(),
        provenance: None,
    };
    let ticket = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(ticket, RecordDisposition::Dropped { loss: loss.clone() });
    let mut result = dummy_result();
    result.report.losses.push(loss);
    assert!(ctx.finish(Ok(result)).is_ok());
}

#[test]
fn transfer_accounting_rejects_shared_loss_note_for_multiple_drops() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let loss = LossNote {
        code: LossCode::DecodeDiagnostic,
        category: LossCategory::Other,
        severity: Severity::Warning,
        message: "unhandled body variant".to_owned(),
        provenance: None,
    };
    let first = ctx.commit_record(root.location(), RecordKind("body"));
    let second = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(first, RecordDisposition::Dropped { loss: loss.clone() });
    ctx.resolve(second, RecordDisposition::Dropped { loss: loss.clone() });
    let mut result = dummy_result();
    result.report.losses.push(loss);
    match ctx.finish(Ok(result)) {
        Err(CodecError::Malformed(message)) => {
            assert!(message.contains("loss note is absent"), "{message}");
        }
        other => panic!("expected strict dropped-loss error, got {other:?}"),
    }
}

#[test]
fn transfer_accounting_accepts_one_loss_note_per_drop() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = strict();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let loss = LossNote {
        code: LossCode::DecodeDiagnostic,
        category: LossCategory::Other,
        severity: Severity::Warning,
        message: "unhandled body variant".to_owned(),
        provenance: None,
    };
    let first = ctx.commit_record(root.location(), RecordKind("body"));
    let second = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(first, RecordDisposition::Dropped { loss: loss.clone() });
    ctx.resolve(second, RecordDisposition::Dropped { loss: loss.clone() });
    let mut result = dummy_result();
    result.report.losses.push(loss.clone());
    result.report.losses.push(loss);
    assert!(ctx.finish(Ok(result)).is_ok());
}

#[test]
fn transfer_accounting_violations_degrade_to_losses_in_salvage() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ticket = ctx.commit_record(root.location(), RecordKind("blob"));
    ctx.resolve(
        ticket,
        RecordDisposition::Retained {
            records: vec!["not-a-real-digest".to_owned()],
        },
    );
    let result = ctx.finish(Ok(dummy_result())).unwrap();
    assert_eq!(result.report.losses.len(), 1);
    let loss = &result.report.losses[0];
    assert_eq!(loss.severity, Severity::Warning);
    assert!(
        loss.message.contains("transfer accounting")
            && loss.message.contains("absent from the retained ledger"),
        "{}",
        loss.message
    );
}

#[test]
fn work_charge_exhaustion_fuses() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_work = 10);
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    ctx.charge_work(10, "probe_scan", Some(root.location()))
        .unwrap();
    assert_eq!(ctx.charged(ResourceDimension::Work), 10);
    match ctx.charge_work(1, "probe_scan", None) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::Work);
        }
        other => panic!("expected Work ResourceLimit, got {other:?}"),
    }
    assert!(ctx.is_fused());
}

#[test]
fn retained_charge_exhaustion_fuses() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_retained_bytes = 4);
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    ctx.charge_retained(4, "retain_record", Some(root.location()))
        .unwrap();
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 4);
    match ctx.charge_retained(1, "retain_record", None) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        }
        other => panic!("expected RetainedBytes ResourceLimit, got {other:?}"),
    }
    assert!(ctx.is_fused());
}

#[test]
fn allowance_is_the_smaller_of_ceiling_and_envelope() {
    let bytes: &[u8] = &[0u8; 64];
    let arena = DecodeArena::new();

    let capped = tight(|limits| limits.max_alloc_bytes = 128);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &capped).unwrap();
    assert_eq!(ctx.allowance_of(ResourceDimension::AllocBytes), 128);

    let arena2 = DecodeArena::new();
    let policy = tight(|limits| limits.max_input_bytes = 4096);
    let (ctx2, _root) = DecodeContext::from_root_bytes(bytes, &arena2, &policy).unwrap();
    assert_eq!(ctx2.allowance_of(ResourceDimension::InputBytes), 4096);
}

#[test]
fn finish_records_profile_and_envelope_versions_under_both_profiles() {
    let bytes: &[u8] = &[0u8; 8];

    let arena = DecodeArena::new();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &desktop()).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "desktop-v1");
    assert_eq!(report.profile_versions.envelope, "envelope-v3");
    assert!(report.profile_versions.overrides.is_empty());

    let arena = DecodeArena::new();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &service()).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "service-v1");
    assert_eq!(report.profile_versions.envelope, "envelope-v3");
    assert!(report.profile_versions.overrides.is_empty());
}

#[test]
fn finish_records_custom_ceiling_overrides_against_the_default() {
    let bytes: &[u8] = &[0u8; 8];

    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_work = 10);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "custom");
    assert_eq!(report.profile_versions.envelope, "envelope-v3");
    assert_eq!(
        report.profile_versions.overrides,
        vec!["max_work=10".to_string()]
    );

    let arena = DecodeArena::new();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &strict()).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "desktop-v1");
    assert!(report.profile_versions.overrides.is_empty());
}

#[test]
fn finish_records_every_deviating_dimension_in_sorted_order() {
    let bytes: &[u8] = &[0u8; 8];

    let arena = DecodeArena::new();
    let policy = tight(|limits| {
        limits.max_work = 10;
        limits.max_depth = 4;
    });
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "custom");
    assert_eq!(
        report.profile_versions.overrides,
        vec!["max_depth=4".to_string(), "max_work=10".to_string()]
    );
}

#[test]
fn retain_charges_dedups_and_yields_a_whole_blob_range() {
    let bytes: &[u8] = &[7u8; 64];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let payload_a = arena.alloc(vec![1u8, 2, 3, 4, 5, 6, 7, 8].into_boxed_slice());
    let retention = ctx.retain(payload_a).unwrap();
    let range = retention.range().expect("retained recoverable");
    assert_eq!(range.start(), 0);
    assert_eq!(range.end(), 8);
    assert_eq!(range.len(), 8);
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 8);

    let payload_b = arena.alloc(vec![1u8, 2, 3, 4, 5, 6, 7, 8].into_boxed_slice());
    let again = ctx.retain(payload_b).unwrap();
    assert_eq!(again.range().unwrap().blob(), range.blob());
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 8);

    let other = arena.alloc(vec![9u8; 4].into_boxed_slice());
    let _ = ctx.retain(other).unwrap();
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 12);
    assert_eq!(ctx.retained_blobs().len(), 2);
}

#[test]
fn subranges_reference_one_blob_under_containment() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let blob = arena.alloc(vec![0u8; 100].into_boxed_slice());
    let whole = ctx.retain(blob).unwrap().range().unwrap().clone();

    let head = whole.subrange(0, 40).expect("contained");
    let tail = whole.subrange(40, 100).expect("contained");
    assert_eq!(head.blob(), tail.blob());
    assert_eq!(head.blob(), whole.blob());
    assert_eq!(head.len(), 40);
    assert_eq!(tail.len(), 60);

    assert!(whole.subrange(0, 101).is_none());
    assert!(whole.subrange(60, 40).is_none());

    let serialized = tail.to_serialized();
    assert_eq!(serialized.blob, whole.blob().as_str());
    assert_eq!(serialized.range.start, 40);
    assert_eq!(serialized.range.end, 100);
}

#[test]
fn strict_fails_with_resource_limit_on_retained_exhaustion() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let mut policy = strict();
    policy.limits.max_retained_bytes = 8;
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let payload = arena.alloc(vec![3u8; 16].into_boxed_slice());
    match ctx.retain(payload) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.reason, ResourceFailure::BudgetExceeded);
        }
        other => panic!("expected ResourceLimit, got {other:?}"),
    }
    assert!(ctx.is_fused());
    assert!(ctx.finish(Ok(dummy_result())).is_err());
}

#[test]
fn salvage_degrades_retained_exhaustion_to_accounting() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_retained_bytes = 8);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let small = arena.alloc(vec![1u8; 8].into_boxed_slice());
    assert!(ctx.retain(small).unwrap().is_retained());

    let big = arena.alloc(vec![2u8; 16].into_boxed_slice());
    match ctx.retain(big).unwrap() {
        Retention::Accounted { digest } => assert_eq!(digest.len(), 64),
        Retention::Retained(_) => panic!("expected degradation to accounting"),
    }
    assert!(!ctx.is_fused(), "retention alone never fuses in salvage");
    assert!(ctx.retention_degraded());

    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert!(report.retention_degraded);
    let notes: Vec<_> = report
        .losses
        .iter()
        .filter(|note| note.message.contains("retained-byte budget exhausted"))
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].severity, Severity::Warning);
}

#[test]
fn retained_blobs_survive_context_teardown_without_recopy() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let payload = arena.alloc(vec![5u8; 12].into_boxed_slice());
    let _ = ctx.retain(payload).unwrap();
    let egress = ctx.retained_blobs();
    let recovered = egress[0].bytes;

    let _ = ctx.finish(Ok(dummy_result())).unwrap();
    assert_eq!(recovered, &[5u8; 12]);
}

#[test]
fn retention_is_deterministic_across_repeat_decodes() {
    fn run(mode: DecodeMode) -> (String, bool) {
        let bytes: &[u8] = &[0u8; 8];
        let arena = DecodeArena::new();
        let policy = DecodePolicy {
            mode,
            limits: {
                let mut limits = ResourceLimits::desktop();
                limits.max_retained_bytes = 8;
                limits
            },
        };
        let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
        let fits = arena.alloc(vec![1u8; 8].into_boxed_slice());
        let id = ctx
            .retain(fits)
            .unwrap()
            .range()
            .unwrap()
            .blob()
            .as_str()
            .to_string();
        (id, ctx.retention_degraded())
    }
    assert_eq!(run(DecodeMode::Salvage), run(DecodeMode::Salvage));
    assert_eq!(run(DecodeMode::Strict).0, run(DecodeMode::Salvage).0);
}
