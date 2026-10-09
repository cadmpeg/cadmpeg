// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::design::decode::sketch::{
    parse_genesis_entity_header, parse_settled_entity_header, NamedEntityHeader,
};
use crate::design::test_support::push_genesis_block;
use crate::test_support::lp_utf16;

fn candidate_header(text: &str, genesis: bool, optional_slot: bool) -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    if genesis {
        bytes.extend_from_slice(&201u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        push_genesis_block(&mut bytes, 4);
    } else {
        bytes.extend_from_slice(&201u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 5]);
        bytes.push(u8::from(optional_slot));
        if optional_slot {
            bytes.extend_from_slice(&[0; 4]);
        }
    }
    let text_offset = bytes.len() + 4;
    lp_utf16(&mut bytes, text);
    (bytes, text_offset)
}

fn parse_candidate(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    genesis: bool,
) -> Result<Option<NamedEntityHeader>, CodecError> {
    if genesis {
        parse_genesis_entity_header(ctx, bytes, 0)
    } else {
        parse_settled_entity_header(ctx, bytes, 0)
    }
}

#[test]
fn rejected_named_header_releases_its_decoded_identity() {
    for (genesis, optional) in [(false, false), (false, true), (true, false)] {
        for text in [
            "invalid", "entity_bad", "entity_18446744073709551616", "0_202", "雪_202",
        ] {
            let (bytes, _) = candidate_header(text, genesis, optional);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_materialized_bytes = u64_from_index(text.len());
            policy.limits.max_retained_bytes = 1;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(parse_candidate(&ctx, &bytes, genesis).unwrap().is_none());
            assert!(parse_candidate(&ctx, &bytes, genesis).unwrap().is_none());
            let text_storage;
            let (held, storage) = ctx
                .utf16le_scoped_text(&bytes[bytes.len() - text.encode_utf16().count() * 2..],
                    text.encode_utf16().count(), false, "after rejected named header")
                .unwrap();
            text_storage = storage;
            assert_eq!(held, text);
            assert_eq!(held.capacity(), text.len());
            let retained = ctx.copy_retained_text("a", "after rejected named header").unwrap();
            assert_eq!(retained, "a");
            assert_eq!(retained.capacity(), 1);
            drop(held);
            drop(text_storage);
        }
    }
}

#[test]
fn accepted_named_header_preserves_identity_locations_and_exact_backing() {
    for (genesis, optional) in [(false, false), (false, true), (true, false)] {
        for text in ["0_201", "雪_201"] {
            let (bytes, text_offset) = candidate_header(text, genesis, optional);
            let backing = u64_from_index(text.len());
            for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes] {
                for ceiling in [backing, backing - 1] {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::default();
                    match dimension {
                        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = ceiling,
                        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = ceiling,
                        _ => unreachable!(),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let result = parse_candidate(&ctx, &bytes, genesis);
                    if ceiling == backing {
                        let header = result.unwrap().unwrap();
                        assert_eq!(header.entity_id.as_str(), text);
                        assert_eq!(header.entity_id.suffix(), 201);
                        assert_eq!(header.entity_id_offset, text_offset);
                        assert_eq!(header.end, bytes.len());
                        assert_eq!(header.optional_slot_present, optional);
                        if dimension == ResourceDimension::MaterializedBytes {
                            let storage;
                            let (held, held_storage) = ctx
                                .utf16le_scoped_text(&bytes[text_offset..],
                                    text.encode_utf16().count(), false, "after named header transfer")
                                .unwrap();
                            storage = held_storage;
                            assert_eq!(held, text);
                            assert_eq!(held.capacity(), text.len());
                            assert_eq!(header.entity_id.as_str(), text);
                            drop(held);
                            drop(storage);
                        } else {
                            let CodecError::ResourceLimit(limit) = ctx
                                .copy_retained_text("a", "after named header transfer").unwrap_err()
                            else { panic!("accepted identity backing must remain retained"); };
                            assert_eq!(limit.dimension, dimension);
                            assert_eq!(limit.operation, "after named header transfer");
                            assert_eq!(limit.used, backing);
                            assert_eq!(limit.additional, 1);
                            assert_eq!(limit.limit, ceiling);
                            assert_eq!(header.entity_id.as_str(), text);
                            let CodecError::ResourceLimit(later) = ctx
                                .charge_work(0, "after named header retained refusal").unwrap_err()
                            else { panic!("the original refusal must remain fused"); };
                            assert_eq!(later, limit);
                        }
                    } else {
                        let Err(CodecError::ResourceLimit(limit)) = result else {
                            panic!("actual decoded identity storage must refuse");
                        };
                        assert_eq!(limit.dimension, dimension);
                        assert_eq!(limit.operation, "f3d Design UTF-16 text");
                        assert_eq!(limit.used, 0);
                        assert_eq!(limit.additional, backing);
                        assert_eq!(limit.limit, ceiling);
                        let CodecError::ResourceLimit(later) = ctx
                            .charge_work(0, "after named header backing refusal").unwrap_err()
                        else { panic!("the original refusal must remain fused"); };
                        assert_eq!(later, limit);
                    }
                }
            }
        }
    }
}

#[test]
fn named_header_keeps_original_utf16_refusal_before_candidate_allocation() {
    for (genesis, optional) in [(false, false), (false, true), (true, false)] {
        let (bytes, _) = candidate_header("0_201", genesis, optional);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = parse_candidate(&ctx, &bytes, genesis) else {
            panic!("the original UTF-16 validation must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "validate F3D UTF-16 text");
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, 1);
        assert_eq!(original.limit, 0);
        let Err(CodecError::ResourceLimit(later)) = parse_candidate(&ctx, &bytes, genesis) else {
            panic!("the original refusal must escape a repeated header parse");
        };
        assert_eq!(later, original);
    }
}
