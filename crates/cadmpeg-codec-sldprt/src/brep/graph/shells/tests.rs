// SPDX-License-Identifier: Apache-2.0
//! Actual shell-table, member and assignment lifetimes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Face, FaceLoops, Sense};

use super::{ShellFaceIndex, ShellMembers};

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;
const FINAL_SHELL: &str = "test:model:shell#replacement";

fn faces() -> Vec<Face> {
    vec![Face {
        id: FaceId::mint("test:model:face#1").unwrap(),
        shell: ShellId::mint("test:model:shell#1").unwrap(),
        surface: SurfaceId::mint("test:model:surface#1").unwrap(),
        sense: Sense::Forward, loops: FaceLoops::unspecified(Vec::new()),
        name: None, color: None, tolerance: None,
    }]
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

fn node_bytes<K, V>() -> u64 {
    let alignment = std::mem::align_of::<K>().max(std::mem::align_of::<V>()).max(std::mem::align_of::<usize>());
    u64::try_from(11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>()) + 16 * std::mem::size_of::<usize>() + 2 * alignment).unwrap()
}

#[test]
fn shell_index_holds_only_live_members_and_table_after_build_maps_die() {
    let faces = faces();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let index = ShellFaceIndex::new(&ctx, &faces, &[], &[]).unwrap();
    // First shell member uses collection_vec(1), separate from its table node.
    // No coedge creates a neighbor; the initial face hash map has already died.
    let expected = node_bytes::<&str, ShellMembers<'_>>() + u64::try_from(std::mem::size_of::<usize>()).unwrap();
    let additional = SCRATCH_BYTES - expected + 1;
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(additional, "test live shell index") else { panic!("live shell backing"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional), (expected, additional));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(index);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn component_callback_keeps_only_its_member_and_iterator_backing_live() {
    let faces = faces();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut index = ShellFaceIndex::new(&ctx, &faces, &[], &[]).unwrap();
    let components = index.take_components("test:model:shell#1").unwrap();
    drop(index);
    // Each push_vec starts with minimum capacity4. Build sets/pending and the
    // removed one-slot candidate member list are dead before this callback.
    let expected = u64::try_from(4 * std::mem::size_of::<ShellMembers<'_>>() + 4 * std::mem::size_of::<usize>()).unwrap();
    let additional = SCRATCH_BYTES - expected + 1;
    let result = components.visit(|ordinal, members| {
        assert_eq!(ordinal, 0);
        assert_eq!(members, [0]);
        ctx.reserve_scoped(additional, "test live component callback").map(drop)
    });
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("live component backing"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional), (expected, additional));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn component_callback_error_releases_all_child_and_iterator_backing() {
    let faces = faces();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut index = ShellFaceIndex::new(&ctx, &faces, &[], &[]).unwrap();
    let components = index.take_components("test:model:shell#1").unwrap();
    drop(index);
    let result = components.visit(|_, _| Err(CodecError::NotImplemented("test component callback".into())));
    assert!(matches!(result, Err(CodecError::NotImplemented(message)) if message == "test component callback"));
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test component callback teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn retained_component_fixture_publishes_actual_child_and_new_outer_vector() {
    let faces = faces();
    let arena = DecodeArena::new();
    let mut policy = policy();
    let expected = u64::try_from(4 * std::mem::size_of::<usize>() + 4 * std::mem::size_of::<Vec<usize>>()).unwrap();
    policy.limits.max_retained_bytes = expected;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = ShellFaceIndex::new(&ctx, &faces, &[], &[]).unwrap();
    let components = index.take_components("test:model:shell#1").unwrap();
    drop(index);
    let output = components.into_retained().unwrap();
    assert_eq!(output, [vec![0]]);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test retained component source teardown").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test retained component backing") else { panic!("exact component output"); };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (expected, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn face_assignment_publishes_only_the_actual_identity_text() {
    let mut faces = faces();
    let shell = ShellId::mint(FINAL_SHELL).unwrap();
    let arena = DecodeArena::new();
    let mut policy = policy();
    let expected = u64::try_from(FINAL_SHELL.len()).unwrap();
    policy.limits.max_retained_bytes = expected;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = ShellFaceIndex::new(&ctx, &faces, &[], &[]).unwrap();
    index.assign(0, &shell).unwrap();
    let assignments = index.into_assignments();
    assignments.apply(&mut faces).unwrap();
    assert_eq!(faces[0].shell, shell);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test final face assignment teardown").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test final face shell text") else { panic!("exact assignment output"); };
    assert_eq!((limit.used, limit.additional), (expected, 1));
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn missing_face_releases_its_assignment_identity_with_zero_retained_bytes() {
    let mut faces = faces();
    let shell = ShellId::mint(FINAL_SHELL).unwrap();
    let original = faces[0].shell.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut index = ShellFaceIndex::new(&ctx, &faces, &[], &[]).unwrap();
    index.assign(1, &shell).unwrap();
    index.into_assignments().apply(&mut faces).unwrap();
    assert_eq!(faces[0].shell, original);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test missing face assignment teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn face_assignment_publication_preserves_the_exact_sticky_refusal() {
    let mut faces = faces();
    let shell = ShellId::mint(FINAL_SHELL).unwrap();
    let original = faces[0].shell.clone();
    let arena = DecodeArena::new();
    let mut policy = policy();
    let expected = u64::try_from(FINAL_SHELL.len()).unwrap();
    policy.limits.max_retained_bytes = expected - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = ShellFaceIndex::new(&ctx, &faces, &[], &[]).unwrap();
    index.assign(0, &shell).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = index.into_assignments().apply(&mut faces) else { panic!("assignment publication refused"); };
    assert_eq!(faces[0].shell, original);
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (0, expected));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn empty_shell_routes_preserve_original_refusal_before_any_allocation() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = ShellFaceIndex::new(&ctx, &[], &[], &[]).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(1, "test original shell refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| index.take_components("missing"));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    assert_eq!(allocations, 0);
    drop(result);
    drop(index);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
