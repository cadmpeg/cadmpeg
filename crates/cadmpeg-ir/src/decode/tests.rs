// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the decode ownership model.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use super::*;
use crate::codec::{CodecError, DecodeResult};
use crate::document::CadIr;
use crate::report::{DecodeReport, LossCategory, LossCode, LossNote, ProfileVersions, Severity};
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
        source_fidelity: None,
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
fn transfer_accounting_passes_trivially_without_tickets() {
    // A codec that issues no tickets has an empty disposition table; the check
    // must not manufacture a violation, so L0 codecs stay unaffected.
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

    // Typed naming an entity actually emitted into the IR model is consistent.
    let typed = ctx.commit_record(root.location(), RecordKind("body"));
    ctx.resolve(
        typed,
        RecordDisposition::Typed {
            outputs: vec!["body/0".to_owned()],
        },
    );

    // Retained naming a record actually held in the retained store is consistent.
    let retention = ctx.retain(&bytes[..8]).unwrap();
    let record = retention.range().unwrap().blob().as_str().to_owned();
    let retained = ctx.commit_record(root.location(), RecordKind("blob"));
    ctx.resolve(
        retained,
        RecordDisposition::Retained {
            records: vec![record],
        },
    );

    // Structural framing is unconstrained.
    let structural = ctx.commit_record(root.location(), RecordKind("header"));
    ctx.resolve(structural, RecordDisposition::Structural);

    assert!(ctx.finish(Ok(result_with_bodies(&["body/0"]))).is_ok());
}

/// Builds a single-space `source` ledger tiling `[0, length)` with one span of
/// `class`, for the ledger-reconciliation tests.
fn source_ledger(length: u64, class: crate::source_fidelity::SpanClass) -> DecodeResult {
    use crate::source_fidelity::{
        AddressSpaceLedger, CanonicalSpaceId, LedgerCapability, LedgerLevel, LedgerSpan,
        SerializedOrigin, SerializedRange, SourceFidelity,
    };

    let mut result = dummy_result();
    result.report.source_fidelity = Some(SourceFidelity::new(
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
    ));
    result
}

#[test]
fn transfer_accounting_accepts_disposition_tiled_by_the_ledger() {
    // A ticket committed in the root (`source`) space at an offset the ledger
    // tiles reconciles: its bytes are corroborated by the serialized §6.1
    // ledger, not merely by the disposition table (§6.2). At L1 the span class
    // is coarse (`Opaque`) yet backs a `Typed` disposition — only span
    // existence is asserted, never class coherence.
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
fn transfer_accounting_rejects_disposition_absent_from_the_ledger_in_strict() {
    // The disposition table validates in isolation, but the ticket's byte
    // location lies past the serialized ledger's tiling: the two artifacts claim
    // disjoint accounts, the cross-artifact conservation §6.2 assigns the check
    // forbids. The offset 32 falls outside a `source` ledger of length 16.
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
    // In salvage mode the same disjoint account never fails the decode: it is
    // appended as an accountable loss note, matching the mode's discipline.
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
    // A Preserved disposition naming a record actually pushed into the native
    // unknowns arena is consistent: §6.2 identity holds over the arena.
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
    // A Preserved disposition naming an unknown id never pushed into the arena
    // is an unaccounted emission; §6.2 requires each named record to resolve.
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
    // A Typed disposition naming an entity id never emitted into the IR is a
    // phantom emission; §6.2 requires each named span to resolve in the model.
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
    // The disposition claims a drop, but the loss never reaches the report.
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
    // Two records dropped under one key, but only one matching note reaches
    // the report: the second drop must not borrow the first's accounting.
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
    assert_eq!(report.profile_versions.envelope, "envelope-v3");
    assert!(report.profile_versions.overrides.is_empty());

    // Service profile: its own version tag, still no overrides.
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

    // A custom ceiling reports the `custom` profile and names the deviation
    // from the desktop default; the envelope version is unchanged.
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

    // Identical bytes in a distinct buffer deduplicate to the same blob and do
    // not charge again.
    let payload_b = arena.alloc(vec![1u8, 2, 3, 4, 5, 6, 7, 8].into_boxed_slice());
    let again = ctx.retain(payload_b).unwrap();
    assert_eq!(again.range().unwrap().blob(), range.blob());
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 8);

    // Distinct bytes are a new blob and charge again.
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

    // Escaping or inverted subranges are refused.
    assert!(whole.subrange(0, 101).is_none());
    assert!(whole.subrange(60, 40).is_none());

    // Serialized reference names the blob and its subrange.
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
    // The refusal fused the context: finish cannot return Ok.
    assert!(ctx.is_fused());
    assert!(ctx.finish(Ok(dummy_result())).is_err());
}

#[test]
fn salvage_degrades_retained_exhaustion_to_accounting() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_retained_bytes = 8);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    // A blob that fits is retained recoverably.
    let small = arena.alloc(vec![1u8; 8].into_boxed_slice());
    assert!(ctx.retain(small).unwrap().is_retained());

    // The next blob exhausts the retained budget: it degrades to accounting
    // rather than failing or fusing.
    let big = arena.alloc(vec![2u8; 16].into_boxed_slice());
    match ctx.retain(big).unwrap() {
        Retention::Accounted { digest } => assert_eq!(digest.len(), 64),
        Retention::Retained(_) => panic!("expected degradation to accounting"),
    }
    assert!(!ctx.is_fused(), "retention alone never fuses in salvage");
    assert!(ctx.retention_degraded());

    // finish succeeds, flags the degradation, and records one loss note.
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
    // Collect the egress before finishing; the borrows address the arena.
    let egress = ctx.retained_blobs();
    let recovered = egress[0].bytes;

    // finish consumes the context; the arena outlives it, so the retained bytes
    // remain readable with no copy at teardown.
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
