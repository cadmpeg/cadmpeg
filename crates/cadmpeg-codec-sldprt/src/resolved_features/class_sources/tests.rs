// SPDX-License-Identifier: Apache-2.0
//! Source-only declaration indexes preserve identity, provenance, and budget.

use super::ClassSources;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn payload() -> Vec<u8> {
    let mut bytes = vec![0xa5; 9];
    let name = "sgEntHandle";
    bytes.extend(super::super::CLASS_MARKER);
    bytes.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
    bytes.extend(name.as_bytes());
    bytes
}

#[test]
fn draft_address_is_lane_relative_and_does_not_claim_record_reads() {
    let mut bytes = vec![0xcc; 37];
    bytes.extend(payload());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(37, bytes.len()).unwrap();
    let classes = super::super::names::class_declarations(source.window(), "lane").unwrap();
    let positions = ClassSources::from_source(&ctx, source, &classes).unwrap();
    assert_eq!(positions.position(&classes[0]), Some(9));
    assert!(serde_json::to_value(&classes[0])
        .unwrap()
        .get("offset")
        .is_none());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn staged_class_identity_and_content_must_match_the_actual_declaration() {
    let bytes = payload();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let classes = super::super::names::class_declarations(root.window(), "lane").unwrap();
    for field in ["id", "parent", "ordinal", "name"] {
        let mut altered = classes.clone();
        match field {
            "id" => altered[0].id = "another-class".into(),
            "parent" => altered[0].parent = "another-lane".into(),
            "ordinal" => altered[0].ordinal = 7,
            "name" => altered[0].name = "sgOtherHandle".into(),
            _ => unreachable!(),
        }
        assert!(
            ClassSources::from_source(&ctx, root, &altered).is_err(),
            "{field}"
        );
    }
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_foreign_session_cannot_supply_a_class_draft() {
    let bytes = payload();
    let arena = DecodeArena::new();
    let (_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let foreign_arena = DecodeArena::new();
    let (foreign_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
    let classes = super::super::names::class_declarations(root.window(), "lane").unwrap();
    assert!(ClassSources::from_source(&foreign_ctx, root, &classes).is_err());
    assert!(foreign_ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn pointer_storage_obeys_the_actual_session_retained_budget() {
    let bytes = payload();
    let framing_storage = {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let before = ctx.retained_bytes_available();
        ctx.capture_part_extent(root).unwrap();
        before - ctx.retained_bytes_available()
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = framing_storage;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let classes = super::super::names::class_declarations(root.window(), "lane").unwrap();
    let CodecError::ResourceLimit(limit) = ClassSources::from_source(&ctx, root, &classes)
        .err()
        .unwrap()
    else {
        panic!("class pointers must use the session budget");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "class source pointer bank");
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn a_shared_pointer_copy_is_charged_but_a_unique_move_is_not_copied() {
    let bytes = payload();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let classes = super::super::names::class_declarations(root.window(), "lane").unwrap();
    let positions = ClassSources::from_source(&ctx, root, &classes).unwrap();
    let shared = positions.clone();
    let before_copy = ctx.retained_bytes_available();
    let copied = positions.into_pointers(&ctx).unwrap();
    assert!(ctx.retained_bytes_available() < before_copy);
    assert_eq!(shared.position(&classes[0]), Some(9));
    assert_eq!(copied[&classes[0].id]["offset"], 9);
    let before_move = ctx.retained_bytes_available();
    let moved = shared.into_pointers(&ctx).unwrap();
    assert_eq!(ctx.retained_bytes_available(), before_move);
    assert_eq!(moved, copied);
}

#[test]
fn an_empty_bank_needs_no_retained_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let empty = ClassSources::default();
    let shared = empty.clone();
    assert!(empty.into_pointers(&ctx).unwrap().is_empty());
    assert!(shared.into_pointers(&ctx).unwrap().is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}
