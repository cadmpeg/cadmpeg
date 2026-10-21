// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::rse::expand_rse_body;

use crate::test_support::rse_compression::encoded;

fn metadata(method: u8) -> (Vec<u8>, usize) {
    let body = crate::records::synthetic_meta_table_body();
    let mut bytes = meta_fixture(false);
    let offset = bytes.len() - encoded(1, &body).len();
    bytes.truncate(offset);
    bytes[offset - 1] = method;
    bytes.extend_from_slice(&encoded(method, &body));
    (bytes, offset)
}

fn bulk(method: u8) -> Vec<u8> {
    let mut bytes = vec![0x3c; 16];
    bytes.extend_from_slice(&[4, method]);
    bytes.extend_from_slice(&encoded(method, b"framed bulk records"));
    bytes
}

#[test]
fn both_methods_preserve_forms_and_expanded_spaces() {
    for method in [1, 2] {
        let (bytes, _) = metadata(method);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("metadata context");
        let SegmentMetaState::Parsed(meta) = parse_meta_stream(&ctx, root).expect("metadata")
        else {
            panic!("complete metadata must decode")
        };
        assert_eq!(meta.body_form, method);
        assert_eq!(
            meta.body.window(),
            crate::records::synthetic_meta_table_body()
        );
        assert_eq!(meta.tables.blocks.len(), 2);
        assert_eq!(meta.tables.types.len(), 1);
        assert_ne!(meta.body.location().space, root.location().space);

        let bytes = bulk(method);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("bulk context");
        let envelope = parse_bulk_stream(&ctx, root).expect("bulk");
        assert_eq!(envelope.form.value(), (u16::from(method) << 8) | 4);
        assert_eq!(envelope.prefix, [0x3c; 16]);
        assert_eq!(envelope.compressed.window(), &bytes[18..]);
        assert_eq!(envelope.compressed.location(), root.location_at(18));
        assert_ne!(envelope.expanded.location().space, root.location().space);
        assert_eq!(envelope.expanded.window(), b"framed bulk records");
    }
}

#[test]
fn dispatch_rejects_unknown_methods_and_mismatched_payloads() {
    for method in [0, 3, 255] {
        let (mut bytes, offset) = metadata(1);
        bytes[offset - 1] = method;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("context");
        assert!(
            matches!(parse_meta_stream(&ctx, root).expect("declaration"),
            SegmentMetaState::Malformed { declared: Some(_), detail }
                if detail.contains("unsupported RSe compression method"))
        );
        let mut bytes = bulk(1);
        bytes[17] = method;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("context");
        assert!(
            matches!(parse_bulk_stream(&ctx, root), Err(CodecError::Malformed(detail))
            if detail.contains("unsupported RSe compression method"))
        );
    }
    for method in [1, 2] {
        let bytes = encoded(method, b"payload");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("context");
        assert!(expand_rse_body(&ctx, root, 3 - method).is_err());
    }
}

#[test]
fn both_streams_reject_truncated_members_and_trailing_garbage() {
    for method in [1, 2] {
        let (bytes, offset) = metadata(method);
        for end in offset..bytes.len() {
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes[..end], &arena, &DecodePolicy::service())
                    .expect("context");
            assert!(matches!(
                parse_meta_stream(&ctx, root).expect("declaration"),
                SegmentMetaState::Malformed {
                    declared: Some(_),
                    ..
                }
            ));
        }
        let mut suffixed = bytes;
        suffixed.push(0xff);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&suffixed, &arena, &DecodePolicy::service())
                .expect("context");
        assert!(matches!(
            parse_meta_stream(&ctx, root).expect("declaration"),
            SegmentMetaState::Malformed { .. }
        ));

        let bytes = bulk(method);
        for end in 18..bytes.len() {
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes[..end], &arena, &DecodePolicy::service())
                    .expect("context");
            assert!(parse_bulk_stream(&ctx, root).is_err());
        }
        let mut suffixed = bytes;
        suffixed.push(0xff);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&suffixed, &arena, &DecodePolicy::service())
                .expect("context");
        assert!(parse_bulk_stream(&ctx, root).is_err());
    }
}

#[test]
fn both_streams_reject_expansion_limit_violations() {
    for method in [1, 2] {
        for limit in 1..3 {
            let mut policy = DecodePolicy::service();
            match limit {
                1 => policy.limits.max_decompressed_bytes_per_expand = 1,
                _ => policy.limits.max_decompressed_bytes_total = 1,
            }
            let (bytes, _) = metadata(method);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
            let result = parse_meta_stream(&ctx, root);
            if method == 2 && limit == 1 {
                // Window admission rejects this stream before allocating a decoder.
                assert!(matches!(result, Ok(SegmentMetaState::Malformed { .. })));
            } else {
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(_))),
                    "metadata method={method}, limit={limit}: {result:?}"
                );
            }
            let bytes = bulk(method);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
            let result = parse_bulk_stream(&ctx, root);
            if method == 2 && limit == 1 {
                assert!(matches!(result, Err(CodecError::NotImplemented(_))));
            } else {
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(_))),
                    "bulk method={method}, limit={limit}: {result:?}"
                );
            }
        }
    }
}
