// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use std::io::{self, Cursor, Read, Seek, SeekFrom};

use crate::CodecError;

use super::*;

fn policy_with(mut edit: impl FnMut(&mut ResourceLimits)) -> DecodePolicy {
    let mut policy = DecodePolicy::default();
    edit(&mut policy.limits);
    policy
}

#[test]
fn root_limit_is_enforced() {
    let bytes = [0_u8; 5];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_input_bytes = 4);
    assert!(matches!(
        DecodeContext::from_root_bytes(&bytes, &arena, &policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::InputBytes
    ));
}

#[test]
fn read_root_uses_sized_and_fallback_read_paths() {
    let bytes = vec![0_u8; 32];
    let policy = policy_with(|limits| limits.max_input_bytes = bytes.len() as u64);

    let arena = DecodeArena::new();
    let mut seekable = Cursor::new(bytes.clone());
    let (_, root) = DecodeContext::read_root(&mut seekable, &arena, &policy, false).unwrap();
    assert_eq!(root.window(), bytes.as_slice());

    let arena = DecodeArena::new();
    let mut fallback = Unseekable::new(bytes.clone());
    let (_, root) = DecodeContext::read_root(&mut fallback, &arena, &policy, false).unwrap();
    assert_eq!(root.window(), bytes.as_slice());
}

struct Unseekable {
    input: Cursor<Vec<u8>>,
}

impl Unseekable {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            input: Cursor::new(bytes),
        }
    }
}

impl Read for Unseekable {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.input.read(bytes)
    }
}

impl Seek for Unseekable {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::other("seek unavailable"))
    }
}

#[test]
fn views_bound_reads_and_children() {
    let bytes = [0, 1, 2, 3, 4];
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let mut child = root.child(1, 4).unwrap();
    assert_eq!(child.take(3), Some(&bytes[1..4]));
    assert!(child.take(1).is_none());
    assert!(root.child(3, 6).is_none());
}

#[test]
fn counted_requires_a_physically_possible_count() {
    let bytes = [0_u8; 8];
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    assert!(root.counted(3, 4).is_none());
    assert_eq!(root.counted(2, 4).unwrap().get(), 2);
}

#[test]
fn exact_expansion_enforces_size_and_limit() {
    let bytes = [0_u8; 4];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_decompressed_bytes_total = 8;
        limits.max_decompressed_bytes_per_expand = 8;
    });
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut writer = ctx.begin_expand(root, ExpandSpec::Exact(4)).unwrap();
    writer.write(&[1, 2, 3, 4]).unwrap();
    let view = writer.finalize().unwrap();
    assert_eq!(view.window(), &[1, 2, 3, 4]);

    let mut writer = ctx.begin_expand(root, ExpandSpec::Unknown).unwrap();
    assert!(matches!(
        writer.write(&[0; 9]),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn concatenation_and_stored_slices_have_distinct_spaces() {
    let bytes = [0, 1, 2, 3, 4, 5];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let first = root.child(0, 2).unwrap();
    let second = root.child(4, 6).unwrap();
    assert!(matches!(
        ctx.concat_views(&[]),
        Err(CodecError::Malformed(_))
    ));
    let concat = ctx.concat_views(&[first, second]).unwrap();
    assert_eq!(concat.window(), &[0, 1, 4, 5]);
    let slice = ctx
        .register_slice(root, ByteRange { start: 1, end: 3 })
        .unwrap();
    assert_eq!(slice.window(), &[1, 2]);
    assert_ne!(concat.space(), slice.space());
}

#[test]
fn scoped_reservations_release_and_commit_without_double_counting() {
    let bytes = [0_u8; 1];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_materialized_bytes = 5;
        limits.max_retained_bytes = 7;
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    {
        let mut reservation = ctx.reserve_scoped(3, "temporary", None).unwrap();
        reservation.grow(2).unwrap();
        assert_eq!(reservation.bytes(), 5);
    }
    ctx.reserve_scoped(5, "released", None).unwrap();
    let reservation = ctx.reserve_scoped(2, "retained", None).unwrap();
    reservation.commit().unwrap();
    ctx.charge_retained(5, "retained", None).unwrap();
}

#[test]
fn every_session_dimension_refuses_and_fuses() {
    fn assert_dimension(
        edit: impl FnMut(&mut ResourceLimits),
        charge: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>,
        expected: ResourceDimension,
    ) {
        let arena = DecodeArena::new();
        let policy = policy_with(edit);
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
        assert!(
            matches!(charge(&ctx), Err(CodecError::ResourceLimit(limit)) if limit.dimension == expected)
        );
        assert!(matches!(
            ctx.charge_entities(0, "after_fuse"),
            Err(CodecError::ResourceLimit(_))
        ));
    }

    assert_dimension(
        |limits| limits.max_materialized_bytes = 1,
        |ctx| ctx.reserve_scoped(2, "materialize", None).map(drop),
        ResourceDimension::MaterializedBytes,
    );
    assert_dimension(
        |limits| limits.max_retained_bytes = 1,
        |ctx| ctx.charge_retained(2, "retain", None),
        ResourceDimension::RetainedBytes,
    );
    assert_dimension(
        |limits| limits.max_entities = 1,
        |ctx| ctx.charge_entities(2, "entities"),
        ResourceDimension::Entities,
    );
    assert_dimension(
        |limits| limits.max_entities = 1,
        |ctx| {
            let mut admitted = 0;
            ctx.admit_entities(1, &mut admitted, "admit")?;
            ctx.admit_entities(2, &mut admitted, "admit")
        },
        ResourceDimension::Entities,
    );
    assert_dimension(
        |limits| limits.max_collection_items = 1,
        |ctx| ctx.charge_collection_items(2, "items"),
        ResourceDimension::CollectionItems,
    );
    assert_dimension(
        |limits| limits.max_recursion_depth = 0,
        |ctx| ctx.enter_nested("nested", None).map(drop),
        ResourceDimension::RecursionDepth,
    );
    assert_dimension(
        |limits| limits.max_work_units = 1,
        |ctx| ctx.charge_work(2, "work"),
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn tiny_resource_limits_refuse_entities_and_materialized_bytes() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_entities = 1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.charge_entities(2, "entities"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
    ));

    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_materialized_bytes = 1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.reserve_scoped(2, "materialize", None),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn alloc_filled_charges_collection_items_and_reserves() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_collection_items = 1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.alloc_filled(2, 0u8, "filled"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default()).unwrap();
    assert_eq!(ctx.alloc_filled(3, 7u8, "filled").unwrap(), vec![7, 7, 7]);
}

#[test]
fn depth_is_scoped_and_work_budget_is_sticky() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_recursion_depth = 1;
        limits.max_work_units = 3;
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    {
        let _guard = ctx.enter_nested("first", None).unwrap();
    }
    ctx.enter_nested("second", None).unwrap();

    let budget = ctx.work_budget(10);
    assert!(budget.charge_by(2));
    let child = budget.child_slice(1);
    assert!(child.charge());
    assert!(budget.consume_child(&child));
    assert_eq!(budget.consumed(), 3);
    assert!(!budget.charge());
    assert!(budget.exhausted());
    assert!(!budget.charge_by(0));
    assert!(
        matches!(budget.refuse("solver"), CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
    );
}

#[test]
fn local_limit_refusal_uses_codec_dimension() {
    assert!(matches!(
        refuse_local_limit("records", 4, 5, None),
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("records")
                && limit.context.operation == "records"
    ));
}

#[test]
fn codec_limit_refusal_fuses_the_decode_session() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default()).unwrap();
    assert!(matches!(
        ctx.refuse_codec_limit("nested_records", 4, 5, None),
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("nested_records")
                && limit.limit == 4
                && limit.used == 4
                && limit.additional == 1
    ));
    assert!(matches!(
        ctx.charge_work(0, "after_codec_limit"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Codec("nested_records")
    ));
}

#[test]
fn committed_reads_preserve_truncation_location_and_operation() {
    let arena = DecodeArena::new();
    let (_, mut root) =
        DecodeContext::from_root_bytes(&[1, 2], &arena, &DecodePolicy::default()).unwrap();
    let error: CodecError = root
        .req_u32_le()
        .unwrap_err()
        .during("read header size")
        .into();
    assert!(matches!(
        error,
        CodecError::Truncated {
            location,
            operation
        }
            if location == root.location_at(0)
                && operation == "read header size"
    ));
}

#[test]
fn unresolved_address_does_not_invent_a_root_step() {
    let address = resolve_address(
        &[],
        SourceLocation {
            space: SpaceId::ROOT,
            offset: 7,
        },
    );
    assert!(address.steps.is_empty());
    assert_eq!(
        address.inspect_commands("part.FCStd"),
        ["cadmpeg inspect hex part.FCStd --offset 7 --len 64"]
    );
}

#[test]
fn nested_member_address_is_inspect_replayable() {
    let descriptors = vec![
        SpaceDescriptor {
            label: "root".into(),
            derivation: SpaceDerivation::Root,
        },
        SpaceDescriptor {
            label: "GuiDocument.xml".into(),
            derivation: SpaceDerivation::Expanded {
                parent: SpaceId::ROOT,
                source_range: ByteRange { start: 30, end: 90 },
            },
        },
    ];
    let address = resolve_address(
        &descriptors,
        SourceLocation {
            space: SpaceId::from_index(1),
            offset: 120,
        },
    );
    assert_eq!(address.path(), "root/GuiDocument.xml@120");
    assert_eq!(address.steps[1].kind, AddressStepKind::ExpandedMember);
    let commands = address.inspect_commands("part.FCStd");
    assert_eq!(
        commands,
        [
            "cadmpeg inspect extract part.FCStd GuiDocument.xml -o part.FCStd.member".to_string(),
            "cadmpeg inspect hex part.FCStd.member --offset 120 --len 64".to_string(),
        ]
    );
}

#[test]
fn forced_child_exhaustion_charges_its_full_slice() {
    let parent = WorkBudget::new(10);
    let child = parent.child_slice(4);
    assert!(child.charge());
    child.exhaust();
    assert!(parent.consume_child(&child));
    assert_eq!(parent.remaining(), 6);
    assert!(!child.charge_by(0));
}
