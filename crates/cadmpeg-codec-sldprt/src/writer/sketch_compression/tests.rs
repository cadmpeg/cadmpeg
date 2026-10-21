// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn fresh_compressed_sketch_frames_preserve_streams_and_interleaved_markers() {
    let body = crate::test_support::parasolid::triangle_body();
    let first =
        crate::writer::parasolid_stream_named(&body, "SCH_SW_33103_11000", "current first sketch")
            .unwrap();
    let second =
        crate::writer::parasolid_stream_named(&body, "SCH_SW_33103_11000", "current second sketch")
            .unwrap();
    let first_frame = wrap(&first).unwrap();
    let second_frame = wrap(&second).unwrap();
    let mut payload = b"first marker prefix".to_vec();
    payload.extend_from_slice(&first_frame);
    let middle = payload.len();
    payload.extend_from_slice(b"second marker prefix");
    payload.extend_from_slice(&second_frame);
    assert_eq!(
        &payload[middle..middle + b"second marker prefix".len()],
        b"second marker prefix"
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::desktop()).unwrap();
    let extracted = crate::parasolid::extract_streams_with_offsets(&payload, &ctx).unwrap();
    assert!(!extracted.probe_capped);
    assert_eq!(extracted.streams.len(), 2);
    assert_eq!(extracted.streams[0].payload, first);
    assert_eq!(extracted.streams[1].payload, second);
    assert_eq!(extracted.streams[0].offset, b"first marker prefix".len());
    assert_eq!(
        extracted.streams[1].offset,
        middle + b"second marker prefix".len()
    );
}

#[test]
fn layout_hint_selects_only_validated_compressed_sketch_sections() {
    let body = crate::test_support::parasolid::triangle_body();
    let compressed =
        crate::test_support::history::sldprt_with_compressed_nested_sketch_profile(&body);
    let direct = crate::test_support::history::sldprt_with_nested_sketch_profile(&body);
    for (bytes, expected) in [
        (compressed, BTreeSet::from(["0".to_owned()])),
        (direct, BTreeSet::new()),
    ] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        assert_eq!(from_scan(Some(&scan)), expected);
    }
    assert!(from_scan(None).is_empty());
    assert!(from_fidelity(None).is_empty());
}
