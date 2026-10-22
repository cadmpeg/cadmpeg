// SPDX-License-Identifier: Apache-2.0
//! Structural expansion preserves the source lineage without semantic read credit.

use super::*;

#[test]
fn opening_and_decoding_zstd_add_no_semantic_reads() {
    let bytes = single_segment(b"payload");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    ctx.record("frame", root, || {
        let mut decoder = ctx.open_zstd(root)?;
        let mut output = [0_u8; 7];
        assert_eq!(decoder.read_chunk(&mut output)?, 7);
        assert_eq!(&output, b"payload");
        assert_eq!(decoder.read_chunk(&mut output)?, 0);
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn a_semantic_expanded_read_still_consumes_the_actual_compressed_frame() {
    let bytes = single_segment(b"payload");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    ctx.record("value", root, || {
        let mut decoder = ctx.open_zstd(root)?;
        let mut writer = ctx.begin_expand(ExpandSpec::Exact(7))?;
        let mut output = [0_u8; 7];
        let count = decoder.read_chunk(&mut output)?;
        writer.write(&output[..count])?;
        let mut expanded = writer.finalize_from(root, "expanded")?;
        assert_eq!(expanded.req_u8()?, b'p');
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &std::collections::BTreeMap::from([
            ("expanded".to_owned(), vec![0..1]),
            (
                "file".to_owned(),
                vec![0..u64::try_from(bytes.len()).unwrap()]
            ),
        ])
    );
}
