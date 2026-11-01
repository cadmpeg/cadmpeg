// SPDX-License-Identifier: Apache-2.0

use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::{F3dCodec, F3zDecodedMember, F3zMemberKind, F3zSession};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
        .any(|loss| loss.code == crate::F3dLossCode::XrefTableUndecoded.kind()));
}

mod memory;
