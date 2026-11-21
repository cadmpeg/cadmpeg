// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::test_support::manifest_test::api_reference_extension;

#[test]
fn manifest_reference_roles_select_named_document_identity() {
    let roles = [
        "11111111-2222-4333-8444-555555555555",
        "66666666-7777-4888-8999-aaaaaaaaaaaa",
    ];
    let bytes = api_reference_extension(&[
        (roles[0], "First", "first.f3d"),
        (roles[1], "Second", "second.f3d"),
    ]);
    assert_eq!(&bytes[..13], &[0, 0, 0, 0, 1, 2, 0, 0, 0, 36, 0, 0, 0]);
    crate::test_support::with_decode_context(|ctx| {
        let table = parse(ctx, View::over_retained(&bytes)).unwrap();
        assert!(table.designs.is_empty());
        assert_eq!(table.references.len(), 2);
        for (ordinal, (path, key)) in [("first.f3d", "First"), ("second.f3d", "Second")]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                table.references[ordinal].neutron_role.as_str(),
                roles[ordinal]
            );
            assert_eq!(table.references[ordinal].relative_path.as_str(), path);
            assert_eq!(table.references[ordinal].from.as_str(), "root.f3d");
            let identity = &table.documents[path];
            assert_eq!(
                identity.document_id.as_ref().unwrap().as_str(),
                format!("urn:adsk.wipprod:dm.lineage:{key}")
            );
            assert_eq!(
                identity.version.as_ref().unwrap().as_str(),
                format!("urn:adsk.wipprod:fs.file:vf.{key}?version=7")
            );
        }
    });
}

#[test]
fn manifest_reference_count_is_bounded_before_allocation() {
    let mut bytes = vec![0, 0, 0, 0, 1];
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    crate::test_support::with_decode_context(|ctx| {
        let error = parse(ctx, View::over_retained(&bytes)).unwrap_err();
        assert!(matches!(error.0, CodecError::Malformed(_)));
    });
}

#[test]
fn manifest_reference_truncation_keeps_source_location() {
    let bytes = [0; 16];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let view = ctx.concat_views(&[root]).unwrap().child(13, 16).unwrap();
    assert_ne!(view.location().space, root.location().space);
    assert_eq!(view.location().offset, 13);
    let error = parse(&ctx, view).unwrap_err();
    assert!(
        matches!(error.0, CodecError::Truncated { location, .. } if location == view.location())
    );
}
