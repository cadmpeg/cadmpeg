// SPDX-License-Identifier: Apache-2.0
use super::*;

fn row(state: u32, numbered: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if numbered {
        bytes.extend([5, 0, 1, 0, 0, 0, b'1', 0, 0]);
    }
    bytes.extend(state.to_le_bytes());
    if state != 1 {
        bytes.extend(0x133_u32.to_le_bytes());
    }
    bytes.extend([1, 0, 4, 0]);
    bytes.extend(0x1234_u32.to_le_bytes());
    bytes.extend([0, 3, 0]);
    bytes.extend(281_u32.to_le_bytes());
    bytes.extend([0x21, 0, 0, 0, 0x17]);
    bytes
}

fn members(bytes: &[u8], kind: u32) -> Vec<CatiaStoragePropertyMember> {
    crate::test_support::with_service_context(|ctx| {
        parse(
            ctx,
            bytes,
            &[FinjplSegment {
                range: 0..bytes.len(),
                type_word: kind,
                name: None,
            }],
        )
    })
    .expect("valid storage member fixture")
}

#[test]
fn nullable_state_requires_exact_word_and_bounded_storage_segment() {
    for (state, expected) in [
        (0x0001_0001, MemberState::Present),
        (0x0001_0000, MemberState::Absent),
    ] {
        let bytes = row(state, false);
        let parsed = members(&bytes, 0x8e);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].state, expected);
        assert_eq!(parsed[0].label, None);
        assert_eq!(parsed[0].slot, 281);
        assert!(members(&bytes, 0x8f).is_empty());
        assert!(members(&bytes[..bytes.len() - 1], 0x8e).is_empty());
    }
    assert!(members(&row(0x0001_0002, true), 0x8e).is_empty());
}

#[test]
fn direct_members_require_complete_numbered_label() {
    assert!(members(&row(1, false), 0x8e).is_empty());
    let bytes = row(1, true);
    assert_eq!(members(&bytes, 0x8e)[0].state, MemberState::Direct);
    for index in [0, 2, 6, 7, 8] {
        let mut damaged = bytes.clone();
        damaged[index] = 0xff;
        assert!(members(&damaged, 0x8e).is_empty(), "label byte {index}");
    }
}

#[test]
fn member_inventory_obeys_collection_and_work_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let bytes = row(1, true);
    let segment = FinjplSegment {
        range: 0..bytes.len(),
        type_word: 0x8e,
        name: None,
    };
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("valid storage member fixture");
        let error = parse(&ctx, &bytes, std::slice::from_ref(&segment))
            .expect_err("invalid storage member must be refused");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }
}

#[test]
fn wire_rejects_unknown_keys_and_explicit_null_labels() {
    let member = members(&row(0x0001_0001, false), 0x8e)
        .pop()
        .expect("valid storage member fixture");
    let wire = serde_json::to_value(&member).expect("valid storage member fixture");
    assert!(wire.get("label").is_none());
    assert_eq!(
        serde_json::from_value::<CatiaStoragePropertyMember>(wire.clone())
            .expect("valid storage member fixture"),
        member
    );
    for (key, value) in [
        ("label", serde_json::Value::Null),
        ("unknown", serde_json::json!(1)),
    ] {
        let mut damaged = wire.clone();
        damaged[key] = value;
        let error = serde_json::from_value::<CatiaStoragePropertyMember>(damaged)
            .expect_err("invalid storage member must be refused");
        assert!(error.to_string().contains(key), "{error}");
    }
}
