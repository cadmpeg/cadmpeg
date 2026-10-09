// SPDX-License-Identifier: Apache-2.0

use super::super::read_property_block;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn property_block(properties: &[(&str, u64)]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&u32::try_from(properties.len()).unwrap().to_le_bytes());
    for &(name, value) in properties {
        for field in [name, "IntrinsicMetaTypeuint64"] {
            bytes.extend_from_slice(&u32::try_from(field.len()).unwrap().to_le_bytes());
            bytes.extend_from_slice(field.as_bytes());
        }
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn sketch_property_session_preserves_native_first_winner_pair_order_and_refunds_backing() {
    let duplicates = property_block(&[("pt_tag", 7), ("pt_tag", 9), ("EntityGenesis", 11)]);
    let pair = property_block(&[("EntityGenesis", 11), ("pt_tag", 7)]);
    let singleton = property_block(&[("pt_tag", 7)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cursor = 0;
    let properties = read_property_block(&ctx, &duplicates, &mut cursor).unwrap().unwrap();
    assert_eq!(cursor, duplicates.len());
    assert_eq!(properties.find("pt_tag").unwrap(), Some(7));
    assert_eq!(properties.find("EntityGenesis").unwrap(), Some(11));
    assert_eq!(properties.find("missing").unwrap(), None);
    assert!(properties.first_two().unwrap().is_none());
    // The property name borrows its own original serialized field.
    assert!(std::ptr::eq(properties.entries[0].name.as_ptr(), duplicates[9..].as_ptr()));
    let mut cursor = 0;
    let paired = read_property_block(&ctx, &pair, &mut cursor).unwrap().unwrap();
    assert_eq!(cursor, pair.len());
    let (first, second) = paired.first_two().unwrap().unwrap();
    let second = second.unwrap();
    assert_eq!((first.name, first.value), ("EntityGenesis", 11));
    assert_eq!((second.name, second.value), ("pt_tag", 7));
    let mut cursor = 0;
    let single = read_property_block(&ctx, &singleton, &mut cursor).unwrap().unwrap();
    assert_eq!(cursor, singleton.len());
    let (first, second) = single.first_two().unwrap().unwrap();
    assert_eq!((first.name, first.value), ("pt_tag", 7));
    assert!(second.is_none());
    drop(properties);
    drop(paired);
    drop(single);
    drop(ctx.reserve_scoped(4096, "after actual sketch property backing destruction").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_property_session_existing_missing_pair_and_construction_keep_original_refusal() {
    let payloads = [vec![0], property_block(&[("EntityGenesis", 11), ("pt_tag", 7)])];
    for payload in &payloads {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (other, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cursor = 0;
        let properties = read_property_block(&ctx, payload, &mut cursor).unwrap().unwrap();
        assert_eq!(cursor, payload.len());
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
            policy.limits.max_work_units + 1, "original native sketch property refusal")
        else { panic!("creating property session refuses excess work"); };
        for key in ["EntityGenesis", "pt_tag", "missing"] {
            assert!(matches!(properties.find(key), Err(CodecError::ResourceLimit(later)) if later == original));
        }
        assert!(matches!(properties.first_two(), Err(CodecError::ResourceLimit(later)) if later == original));
        let mut cursor = 0;
        assert!(matches!(read_property_block(&ctx, payload, &mut cursor),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(cursor, 0);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert_eq!(other.resource_refusal(), None);
        drop(properties);
        other.finish_session().unwrap();
    }
}

#[test]
fn sketch_property_session_empty_presence_forms_need_no_work_items_or_backing_then_keep_original_fuse() {
    for payload in [&[0][..], &[1, 0, 0, 0, 0][..]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cursor = 0;
        let properties = read_property_block(&ctx, payload, &mut cursor).unwrap().unwrap();
        assert_eq!(cursor, payload.len());
        assert_eq!(properties.find("EntityGenesis").unwrap(), None);
        assert!(properties.first_two().unwrap().is_none());
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty property refusal")
        else { panic!("zero-work creating session refuses positive work"); };
        assert!(matches!(properties.find("EntityGenesis"), Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(properties.first_two(), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        drop(properties);
    }
}
