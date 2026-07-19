// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use super::*;
use crate::codec::{CodecError, DecodeResult};
use crate::document::CadIr;
use crate::report::{DecodeReport, Severity};
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
fn fuse_then_swallow_still_fails_at_inspection_finish() {
    let bytes: &[u8] = &[0u8; 16];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_work = 0);
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let swallowed = ctx.charge_work(1, "test", None).ok();
    assert!(swallowed.is_none());
    assert!(matches!(
        ctx.finish_inspection(Ok(())),
        Err(CodecError::ResourceLimit(_))
    ));
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
    let values = ctx
        .read_counted(&mut view, count, super::view::View::u32_le)
        .unwrap();
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
    let result = ctx.read_counted(&mut view, count, super::view::View::u64_le);
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
}

#[test]
fn derived_space_charges_a_multi_input_transform() {
    let bytes: &[u8] = &[0xAA, 0xBB, 0xCC, 0xDD, 0x01, 0x02, 0x03, 0x04];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let dictionary = root.child(0, 4).unwrap();
    let data = root.child(4, 8).unwrap();
    let basis_before = ctx.input_basis();

    let mut writer = ctx
        .begin_derived_space(&[dictionary, data], DerivedKind::Transform)
        .unwrap();
    writer.write(&[1, 2, 3, 4, 5, 6]).unwrap();
    let (_space, view) = writer.finalize().unwrap();

    assert_eq!(view.window(), &[1, 2, 3, 4, 5, 6]);
    assert_eq!(ctx.charged(ResourceDimension::DecompressedBytes), 6);
    assert_eq!(ctx.charged(ResourceDimension::AllocBytes), 0);
    assert_eq!(ctx.input_basis(), basis_before + 6);
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
    let (_space, view) = writer.finalize().unwrap();
    assert_eq!(view.window(), &[1, 2, 3, 4]);
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
fn retain_charges_once_per_digest() {
    let bytes: &[u8] = &[7u8; 64];
    let arena = DecodeArena::new();
    let policy = desktop();
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let payload_a = arena.alloc(vec![1u8, 2, 3, 4, 5, 6, 7, 8].into_boxed_slice());
    let retention = ctx.retain(payload_a).unwrap();
    let Retention::Retained { digest } = retention else {
        panic!("expected retention");
    };
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 8);

    let payload_b = arena.alloc(vec![1u8, 2, 3, 4, 5, 6, 7, 8].into_boxed_slice());
    let again = ctx.retain(payload_b).unwrap();
    assert_eq!(again, Retention::Retained { digest });
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 8);

    let other = arena.alloc(vec![9u8; 4].into_boxed_slice());
    let _ = ctx.retain(other).unwrap();
    assert_eq!(ctx.charged(ResourceDimension::RetainedBytes), 12);
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
fn salvage_degrades_retained_exhaustion_to_digest_only() {
    let bytes: &[u8] = &[0u8; 8];
    let arena = DecodeArena::new();
    let policy = tight(|limits| limits.max_retained_bytes = 8);
    let (ctx, _root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();

    let small = arena.alloc(vec![1u8; 8].into_boxed_slice());
    assert!(matches!(
        ctx.retain(small).unwrap(),
        Retention::Retained { .. }
    ));

    let big = arena.alloc(vec![2u8; 16].into_boxed_slice());
    match ctx.retain(big).unwrap() {
        Retention::DigestOnly { digest } => assert_eq!(digest.len(), 64),
        Retention::Retained { .. } => panic!("expected digest-only retention"),
    }
    assert!(!ctx.is_fused(), "retention alone never fuses in salvage");
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
fn retention_is_deterministic_across_repeat_decodes() {
    fn run(mode: DecodeMode) -> String {
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
        let Retention::Retained { digest } = ctx.retain(fits).unwrap() else {
            panic!("expected retention");
        };
        digest
    }
    assert_eq!(run(DecodeMode::Salvage), run(DecodeMode::Salvage));
    assert_eq!(run(DecodeMode::Strict), run(DecodeMode::Salvage));
}
