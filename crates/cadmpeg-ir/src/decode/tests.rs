// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the decode ownership model.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use super::*;
use crate::codec::{CodecError, DecodeResult};
use crate::document::CadIr;
use crate::report::{DecodeReport, LossCategory, ProfileVersions, Severity};
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
    let ir = CadIr::empty(Units::default());
    let report = DecodeReport {
        format: "test".to_string(),
        container_only: false,
        geometry_transferred: false,
        losses: Vec::new(),
        notes: Vec::new(),
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

    // A child that would exceed the parent is refused, never clamped.
    assert!(root.child(100, 264).is_none());
    let parent = root.child(0, 157).unwrap();
    assert!(parent.child(0, 164).is_none());

    // seek honors the stored lower bound.
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

    // Code that swallows the failure through Option starves rather than spins:
    // every subsequent charge fails immediately.
    let swallowed = ctx.exact_vec::<u8>(root.counted(1, 1).unwrap()).ok();
    assert!(swallowed.is_none());

    // finish refuses to return Ok from a fused context, returning the
    // original resource error even though the decode result was Ok.
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

    // Exactly the limit succeeds; guards release on unwind.
    assert_eq!(recurse(&ctx, 7).unwrap(), 8);
    assert_eq!(ctx.current_depth(), 0);
    assert!(!ctx.is_fused());

    // One level too deep is unswallowable and fuses.
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

    // The count proof comes from the view; exact_vec accepts nothing else.
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

    // Each element charged size_of::<u32>() = 4; two builders of three = 24.
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
    // The floor is proven for two 4-byte elements, but the reader consumes
    // eight bytes per element: the second read finds fewer bytes remaining
    // than one element's minimum size, which is a truncation.
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
    // The reader rejects a value while at least one more element's bytes are
    // still present: an inconsistency wholly inside the view, malformed.
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

    // Underrun: declared 10, produced 6, finalize rejects.
    let mut under = ctx.begin_expand(root, ExpandSpec::Exact(10)).unwrap();
    under.write(&[0u8; 6]).unwrap();
    assert!(under.finalize().is_err());

    // Overrun: a write past the declared exact size is rejected per-write.
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

    // The input basis grew by the finalized length, raising allowances.
    assert_eq!(ctx.input_basis(), 110);
    let alloc_after = ctx.allowance_of(ResourceDimension::AllocBytes);
    assert!(alloc_after > alloc_before, "allowance grows with the basis");
}

#[test]
fn derived_space_concats_multiple_extents_and_charges_alloc() {
    // Two disjoint extents of the root are reassembled into one logical stream.
    let bytes: &[u8] = &[10, 11, 12, 13, 20, 21, 22, 23, 24, 25];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let first = root.child(0, 4).unwrap();
    let second = root.child(6, 10).unwrap();
    let basis_before = ctx.input_basis();

    // The builder assembles the output from the input extents itself, so the
    // recorded segments cannot diverge from the bytes the space holds.
    let writer = ctx
        .begin_derived_space(&[first, second], DerivedKind::Concat)
        .unwrap();
    assert_eq!(writer.written(), 8);
    let (space, view) = writer.finalize().unwrap();

    assert_eq!(space.index(), 1);
    assert_eq!(view.space(), space);
    assert_eq!(view.window(), &[10, 11, 12, 13, 22, 23, 24, 25]);
    assert_eq!(ctx.spaces_len(), 2);

    // Each extent charges alloc_bytes for the assembled copy.
    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 8);

    // Reassembling existing bytes does not grow the input basis: the source
    // extents are already accounted for in their own spaces.
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
    // A dictionary-preset decompression is a genuine multi-input transform:
    // one extent is the dictionary, another the compressed data, and the
    // finalized space records both inputs under a Transform origin. Its output
    // is new decompressed content, so it is charged as decompressed_bytes under
    // the decompression ceilings and grows the input basis — never routed
    // through alloc_bytes, which would evade those ceilings.
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
    // Decompressed output is charged as decompressed_bytes, not alloc_bytes, so
    // the calibrated per-expand and cumulative ceilings apply.
    assert_eq!(ctx.charged(ResourceDimension::DecompressedBytes), 6);
    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 0);
    // Finalizing new decompressed content grows the input basis.
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

    // A writer dropped without finalizing registers no space.
    {
        let extent = root.child(0, 3).unwrap();
        let _abandoned = ctx
            .begin_derived_space(&[extent], DerivedKind::Concat)
            .unwrap();
    }
    assert_eq!(ctx.spaces_len(), 1, "an unfinalized space never registers");

    // An extent past the alloc allowance fuses the context during assembly: the
    // first 4-byte extent fits the tight ceiling, the second pushes over it.
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
    // A stored archive entry becomes a Slice space that borrows the parent bytes
    // and re-bases their coordinates to zero, charging nothing: the bytes were
    // already accounted for when the parent space was admitted.
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
    // The slice window is the parent subrange, re-based so offset zero is its
    // first byte.
    assert_eq!(view.window(), &[2, 3, 4, 5]);
    assert_eq!(view.position(), 0);
    assert_eq!(view.start(), 0);
    // Aliasing input neither charges the allocation budget nor grows the basis.
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
    // A slice past the parent window is refused at the request site, never
    // clamped, and registers no space.
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
    // A Concat space assembles from its declared inputs; a caller cannot inject
    // bytes unrelated to the recorded segments through `write`.
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
    // The refusal does not corrupt the assembled buffer: it still concatenates
    // only the declared extent.
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

    // A transaction that charges the budget, then fails a read.
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

    // A successful transaction commits the advance.
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

    // Strict: an unresolved committed ticket is a contract error at finish.
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

    // Salvage: unresolved tickets are auto-resolved as Dropped, and each
    // drop surfaces as an accountable loss note on the result's report.
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

    // A tight absolute ceiling wins over the envelope term.
    let capped = tight(|limits| limits.max_alloc_bytes = 128);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &capped).unwrap();
    assert_eq!(ctx.allowance_of(ResourceDimension::AllocBytes), 128);

    // The input-bytes allowance is the absolute ceiling directly.
    let arena2 = DecodeArena::new();
    let policy = tight(|limits| limits.max_input_bytes = 4096);
    let (ctx2, _root) = DecodeContext::from_root_bytes(bytes, &arena2, &policy).unwrap();
    assert_eq!(ctx2.allowance_of(ResourceDimension::InputBytes), 4096);
}

#[test]
fn finish_records_profile_and_envelope_versions_under_both_profiles() {
    let bytes: &[u8] = &[0u8; 8];

    // Desktop (the default) profile: version tags present, no overrides.
    let arena = DecodeArena::new();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &desktop()).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "desktop-v1");
    assert_eq!(report.profile_versions.envelope, "envelope-v2");
    assert!(report.profile_versions.overrides.is_empty());

    // Service profile: its own version tag, still no overrides.
    let arena = DecodeArena::new();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &service()).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "service-v1");
    assert_eq!(report.profile_versions.envelope, "envelope-v2");
    assert!(report.profile_versions.overrides.is_empty());
}

#[test]
fn finish_records_custom_ceiling_overrides_against_the_default() {
    let bytes: &[u8] = &[0u8; 8];

    // A custom ceiling reports the `custom` profile and names the deviation
    // from the desktop default; the envelope version is unchanged.
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_work = 10);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "custom");
    assert_eq!(report.profile_versions.envelope, "envelope-v2");
    assert_eq!(
        report.profile_versions.overrides,
        vec!["max_work=10".to_string()]
    );

    // Strict mode does not change the versioned ceilings: the desktop version
    // stands with no overrides, since mode is orthogonal policy, not a
    // calibration constant.
    let arena = DecodeArena::new();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &strict()).unwrap();
    let report = ctx.finish(Ok(dummy_result())).unwrap().report;
    assert_eq!(report.profile_versions.profile, "desktop-v1");
    assert!(report.profile_versions.overrides.is_empty());
}

#[test]
fn finish_records_every_deviating_dimension_in_sorted_order() {
    let bytes: &[u8] = &[0u8; 8];

    // Two deviations set out of alphabetical order: the record names each one
    // and sorts them, so the report reads deterministically regardless of the
    // order the caller changed ceilings.
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
