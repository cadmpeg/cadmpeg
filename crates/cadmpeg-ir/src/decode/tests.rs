// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the decode ownership model.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use super::*;
use crate::codec::{CodecError, DecodeResult};
use crate::document::CadIr;
use crate::report::{DecodeReport, LossCategory, Severity};
use crate::units::Units;

fn desktop() -> DecodePolicy {
    DecodePolicy::default()
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
