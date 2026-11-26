// SPDX-License-Identifier: Apache-2.0

use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::{F3dCodec, F3zDecodedMember, F3zMemberKind, F3zSession};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const GENERATED_CUBE: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/generated_unit_cube.f3d");

fn assert_cube_document(ir: &cadmpeg_ir::CadIr) {
    assert_eq!(
        ir.model
            .product_definitions
            .iter()
            .filter(|definition| definition.is_root())
            .count(),
        1
    );
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 6);
    let validation = cadmpeg_ir::validate_neutral(ir, Vec::new()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn independent_archive_members_state_one_document_root() {
    let bytes = f3z_archive(
        "Alpha.f3d",
        &[("Alpha.f3d", GENERATED_CUBE), ("Beta.f3d", GENERATED_CUBE)],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    for id in ["Alpha.f3d", "Beta.f3d"] {
        let decoded = session.decode_member(id).unwrap();
        assert_cube_document(&decoded.decoded().unwrap().unwrap().ir);
    }
    let composed = session
        .compose(
            session
                .members()
                .iter()
                .map(|member| session.decode_member(member.id()).unwrap()),
            &cadmpeg_ir::report::strictness::Strictness::default(),
        )
        .unwrap();
    assert_cube_document(composed.ir());
}

#[test]
fn referenced_archive_member_keeps_root_body_ownership_after_merge() {
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("cube.f3d", XREF_ROLE)]);
    let bytes = f3z_archive(
        "root.f3d",
        &[("root.f3d", &root), ("cube.f3d", GENERATED_CUBE)],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    let member = session.decode_member("cube.f3d").unwrap();
    assert_cube_document(&member.decoded().unwrap().unwrap().ir);
    let composed = session
        .compose(
            session
                .members()
                .iter()
                .map(|member| session.decode_member(member.id()).unwrap()),
            &cadmpeg_ir::report::strictness::Strictness::default(),
        )
        .unwrap();
    assert_cube_document(composed.ir());
    let body = &composed.ir().geometry.bodies[0].id;
    assert_eq!(
        composed
            .ir()
            .model
            .product_definitions
            .iter()
            .filter(|definition| { !definition.is_root() && definition.bodies.contains(body) })
            .count(),
        1
    );
    let traversed = session.compose_in_traversal_for_test(&ctx).unwrap();
    assert_cube_document(&traversed.ir);
}

#[test]
fn archive_session_and_member_results_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<F3zSession<'static>>();
    assert_send_sync::<F3zDecodedMember>();
}

#[test]
fn archive_session_open_lists_stable_paths_roles_sizes_and_references() {
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let leaf = f3d_without_brep("part-design", "leaf.f3d", &[]);
    let bytes = f3z_archive(
        "root.f3d",
        &[
            ("leaf.f3d", &leaf),
            ("root.f3d", &root),
            ("bad.f3d", b"invalid"),
            ("sheet.f2d", b"drawing"),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    assert_eq!(session.members()[0].id(), "root.f3d");
    let member = |id: &str| {
        session
            .members()
            .iter()
            .find(|member| member.id() == id)
            .unwrap()
    };
    assert_eq!(member("root.f3d").kind(), &F3zMemberKind::PlacementOnly);
    assert_eq!(member("root.f3d").references(), &["leaf.f3d"]);
    assert_eq!(member("leaf.f3d").kind(), &F3zMemberKind::Design);
    assert_eq!(
        member("leaf.f3d").byte_size(),
        u64::try_from(leaf.len()).unwrap()
    );
    assert_eq!(member("sheet.f2d").kind(), &F3zMemberKind::Drawing);
    assert!(
        matches!(member("bad.f3d").kind(), F3zMemberKind::Skipped(reason) if !reason.is_empty())
    );
    assert!(session.decode_member("absent.f3d").is_err());
    assert!(session
        .decode_member("sheet.f2d")
        .unwrap()
        .decoded()
        .is_none());
    let other_arena = DecodeArena::new();
    let (other_ctx, other_view) =
        DecodeContext::from_root_bytes(&bytes, &other_arena, &DecodePolicy::default()).unwrap();
    let other = F3dCodec.open_archive(&other_ctx, other_view).unwrap();
    assert_eq!(session.members(), other.members());
}

#[test]
fn archive_session_reference_error_keeps_the_member_decode_loss() {
    let root =
        crate::test_support::assembly_test::f3d_with_redirections_json("part-design", b"{invalid}");
    let bytes = f3z_archive("root.f3d", &[("root.f3d", &root)]);
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    assert!(session.members()[0].reference_error().is_some());
    let member = session.decode_member("root.f3d").unwrap();
    let decoded = member.decoded().unwrap().unwrap();
    assert!(decoded
        .body
        .losses
        .iter()
        .any(crate::test_support::xref_table_given_up));
}

mod memory;

mod preparation;

#[test]
fn archive_session_expanded_member_traces_avoid_reexecution() {
    let leaf = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("left.f3d", XREF_ROLE), ("right.f3d", XREF_ROLE)],
    );
    let bytes = crate::test_support::archive_session::deflated_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("left.f3d", &leaf),
            ("right.f3d", &leaf),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    let compose = || {
        session.compose(
            session
                .members()
                .iter()
                .map(|member| session.decode_member(member.id()).unwrap()),
            &cadmpeg_ir::report::strictness::Strictness::default(),
        )
    };
    session
        .require_original_spaces
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let original = compose();
    assert!(original.is_ok());
    let before = session.composition_reexecutions();
    assert!(before > 0);
    session
        .require_original_spaces
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let composed = compose();
    crate::test_support::archive_session::assert_equal(&original, &composed);
    assert_eq!(session.composition_reexecutions(), before);
    println!("expanded archive: original-space reexecutions={before}, relocated reexecutions=0");
}

#[test]
fn archive_session_changed_allowance_decision_reexecutes_member() {
    let leaf = f3d_without_brep("part-design", "leaf.f3d", &[]);
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let bytes = f3z_archive("root.f3d", &[("root.f3d", &root), ("leaf.f3d", &leaf)]);
    let policy = DecodePolicy {
        limits: cadmpeg_core::decode::ResourceLimits {
            max_retained_bytes: 100,
            ..cadmpeg_core::decode::ResourceLimits::default()
        },
    };
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    let mut member = session.decode_member("leaf.f3d").unwrap();
    let budget_arena = DecodeArena::new();
    let budget = DecodeContext::new(
        &budget_arena,
        &policy,
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let task_arena = DecodeArena::new();
    let task = budget.checkpoint().resume(&task_arena);
    let available = task.retained_allowance();
    task.observe_retained_allowance(available, 70);
    member.trace = task.take_trace();
    budget.retain_bytes(40, "earlier member output").unwrap();
    let mut results = std::collections::BTreeMap::from([("leaf.f3d".to_owned(), member)]);
    let _result = session.take_member(&budget, "leaf.f3d", &mut results);
    assert_eq!(session.composition_reexecutions(), 1);
}

#[test]
fn archive_session_formatted_space_diagnostic_reexecutes_after_relocation() {
    let leaf = f3d_without_brep("part-design", "leaf.f3d", &[]);
    let bytes = f3z_archive("leaf.f3d", &[("leaf.f3d", &leaf)]);
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    let mut member = session.decode_member("leaf.f3d").unwrap();
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    let range = cadmpeg_core::decode::ByteRange { start: 0, end: 1 };
    task.register_slice(view, range).unwrap();
    member.trace = task.take_trace();
    let body = &mut member.decoded.as_mut().unwrap().as_mut().unwrap().body;
    body.notes
        .push("truncated input at space 7 offset 11".to_owned());
    body.losses
        .push(crate::F3dLossCode::XrefMemberUndecoded.note("stored slice escapes parent space 7"));
    ctx.register_slice(view, range).unwrap();
    let mut results = std::collections::BTreeMap::from([("leaf.f3d".to_owned(), member)]);
    let (decoded, _, _) = session.take_member(&ctx, "leaf.f3d", &mut results).unwrap();
    assert_eq!(session.composition_reexecutions(), 1);
    assert!(!decoded
        .body
        .notes
        .iter()
        .any(|note| note.contains("at space 7")));
}

#[test]
fn archive_session_rebinds_typed_member_truncation_location() {
    let leaf = f3d_without_brep("part-design", "leaf.f3d", &[]);
    let bytes = f3z_archive("leaf.f3d", &[("leaf.f3d", &leaf)]);
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    let mut member = session.decode_member("leaf.f3d").unwrap();
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    let range = cadmpeg_core::decode::ByteRange { start: 0, end: 1 };
    let mut expanded = task.register_slice(view, range).unwrap();
    member.decoded = Some(Err(expanded.req_u64_le().unwrap_err().into()));
    member.trace = task.take_trace();
    let mut prior = ctx.register_slice(view, range).unwrap();
    let expected = prior.req_u64_le().unwrap_err().location.space;
    let mut results = std::collections::BTreeMap::from([("leaf.f3d".to_owned(), member)]);
    let error = session
        .take_member(&ctx, "leaf.f3d", &mut results)
        .unwrap_err();
    let cadmpeg_core::CodecError::Truncated { location, .. } = error else {
        panic!("expected typed truncation");
    };
    assert_eq!(location.space.index(), expected.index() + 1);
    assert_eq!(location.offset, 0);
    assert_eq!(session.composition_reexecutions(), 0);
}

#[test]
#[ignore = "requires CADMPEG_F3Z_PUBLIC_ARCHIVE and the public-archive resource scope"]
fn archive_session_public_archive_trace_counts() {
    let path = std::env::var_os("CADMPEG_F3Z_PUBLIC_ARCHIVE").expect("public archive path");
    let bytes = std::fs::read(path).unwrap();
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let session = F3dCodec.open_archive(&ctx, view).unwrap();
    let decode = || {
        let mut decoded = Vec::new();
        for group in session.members().chunks(16) {
            decoded.extend(std::thread::scope(|scope| {
                let handles = group
                    .iter()
                    .map(|member| {
                        let session = &session;
                        scope.spawn(move || session.decode_member(member.id()).unwrap())
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect::<Vec<_>>()
            }));
        }
        decoded
    };
    session
        .require_original_spaces
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let original = session.compose(
        decode(),
        &cadmpeg_ir::report::strictness::Strictness::default(),
    );
    assert!(original.is_ok());
    let before = session.composition_reexecutions();
    session
        .require_original_spaces
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let composed = session.compose(
        decode(),
        &cadmpeg_ir::report::strictness::Strictness::default(),
    );
    let after = session.composition_reexecutions() - before;
    crate::test_support::archive_session::assert_equal(&original, &composed);
    println!("public archive: members={}, original-space reexecutions={before}, relocated reexecutions={after}", session.members().len());
}
