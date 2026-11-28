// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::test_support::manifest_fixture::api_reference_extension;

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

fn replace_property(bytes: &mut Vec<u8>, field: &str, value: &str) {
    let mut key = Vec::new();
    crate::test_support::lp_utf16(&mut key, field);
    let at = bytes
        .windows(key.len())
        .position(|window| window == key)
        .unwrap()
        + key.len();
    let count = View::u32_le_at(bytes, at).unwrap();
    let mut replacement = Vec::new();
    crate::test_support::lp_utf16(&mut replacement, value);
    bytes.splice(
        at..at + 4 + usize::try_from(count).unwrap() * 2,
        replacement,
    );
}

#[test]
fn manifest_reference_lineage_disagreement_keeps_lineage_urn() {
    let mut bytes = api_reference_extension(&[(
        "11111111-2222-4333-8444-555555555555",
        "Target",
        "target.f3d",
    )]);
    replace_property(&mut bytes, "LineageId", "urn:adsk.wipprod:dm.lineage:Other");
    crate::test_support::with_decode_context(|ctx| {
        let table = parse(ctx, View::over_retained(&bytes)).unwrap();
        assert_eq!(
            table.documents["target.f3d"]
                .document_id
                .as_ref()
                .unwrap()
                .as_str(),
            "urn:adsk.wipprod:dm.lineage:Target"
        );
        assert_eq!(table.identity_losses.len(), 1);
        assert_eq!(
            table.identity_losses[0].code.local_code(),
            "xref.lineage-disagrees"
        );
        assert!(table.identity_losses[0].message.contains("LineageId"));
        assert!(table.identity_losses[0].message.contains("LineageUrn"));
    });
}

#[test]
fn manifest_reference_blank_identity_preserves_table() {
    for field in ["LineageUrn", "URL"] {
        let mut bytes = api_reference_extension(&[
            ("11111111-2222-4333-8444-555555555555", "First", "first.f3d"),
            (
                "66666666-7777-4888-8999-aaaaaaaaaaaa",
                "Second",
                "second.f3d",
            ),
        ]);
        replace_property(&mut bytes, field, "  ");
        crate::test_support::with_decode_context(|ctx| {
            let table = parse(ctx, View::over_retained(&bytes)).unwrap();
            assert_eq!(table.references.len(), 2);
            let first = &table.documents["first.f3d"];
            assert_eq!(first.document_id.is_none(), field == "LineageUrn");
            assert_eq!(first.version.is_none(), field == "URL");
            assert!(table.documents["second.f3d"].document_id.is_some());
            assert!(table.documents["second.f3d"].version.is_some());
            assert!(table
                .identity_losses
                .iter()
                .any(|loss| loss.code.local_code() == "xref.identity-blank"
                    && loss.message.contains(field)));
        });
    }
}

#[test]
fn manifest_reference_zero_run_requires_four_zero_bytes() {
    let mut bytes = api_reference_extension(&[(
        "11111111-2222-4333-8444-555555555555",
        "Target",
        "target.f3d",
    )]);
    for at in 0..prefix::ZERO_RUN_VALUE.len() {
        bytes[at] = 1;
        crate::test_support::with_decode_context(|ctx| {
            let error = parse(ctx, View::over_retained(&bytes)).unwrap_err();
            assert!(matches!(error.0, CodecError::Malformed(_)));
        });
        bytes[at] = 0;
    }
}

#[test]
fn manifest_reference_blank_identity_shares_present_target_identity() {
    for field in ["LineageUrn", "URL"] {
        let mut bytes = api_reference_extension(&[
            (
                "11111111-2222-4333-8444-555555555555",
                "Target",
                "target.f3d",
            ),
            (
                "66666666-7777-4888-8999-aaaaaaaaaaaa",
                "Target",
                "target.f3d",
            ),
        ]);
        replace_property(&mut bytes, field, "");
        crate::test_support::with_decode_context(|ctx| {
            let table = parse(ctx, View::over_retained(&bytes)).unwrap();
            assert_eq!(table.references.len(), 2);
            assert!(table.documents["target.f3d"].document_id.is_some());
            assert!(table.documents["target.f3d"].version.is_some());
        });
    }
}

fn without_classification(bytes: &mut Vec<u8>, count: u32) {
    let mut pair = Vec::new();
    crate::test_support::lp_utf16(&mut pair, "linkClassification");
    crate::test_support::lp_utf16(&mut pair, "OriginalDesign");
    let at = bytes
        .windows(pair.len())
        .position(|window| window == pair)
        .unwrap();
    bytes.drain(at..at + pair.len());
    let prefix: Vec<u8> = [2u32, 4, 1, 0, 6]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let at = bytes
        .windows(prefix.len())
        .position(|window| window == prefix)
        .unwrap()
        + 16;
    bytes[at..at + 4].copy_from_slice(&count.to_le_bytes());
}

#[test]
fn a_five_property_reference_stores_no_classification() {
    let mut bytes = api_reference_extension(&[(
        "11111111-2222-4333-8444-555555555555",
        "Target",
        "target.f3d",
    )]);
    without_classification(&mut bytes, 5);
    crate::test_support::with_decode_context(|ctx| {
        let table = parse(ctx, View::over_retained(&bytes)).unwrap();
        assert_eq!(table.references.len(), 1);
        let identity = &table.documents["target.f3d"];
        assert_eq!(
            identity.document_id.as_ref().unwrap().as_str(),
            "urn:adsk.wipprod:dm.lineage:Target"
        );
        assert_eq!(
            identity.version.as_ref().unwrap().as_str(),
            "urn:adsk.wipprod:fs.file:vf.Target?version=7"
        );
        assert!(table.identity_losses.is_empty());
    });
}

#[test]
fn a_six_property_reference_requires_its_classification() {
    let mut bytes = api_reference_extension(&[(
        "11111111-2222-4333-8444-555555555555",
        "Target",
        "target.f3d",
    )]);
    let mut unclassified = bytes.clone();
    without_classification(&mut unclassified, 6);
    replace_property(&mut bytes, "linkClassification", "Derived");
    crate::test_support::with_decode_context(|ctx| {
        assert!(parse(ctx, View::over_retained(&bytes)).is_err());
        assert!(parse(ctx, View::over_retained(&unclassified)).is_err());
    });
}
