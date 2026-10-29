// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn physical_lines_keep_actual_nonzero_source_extents_without_semantic_read_credit() {
    let bytes = b"prefix:D1@14_1;\r\n116,1,2,3;\rD3;";
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("the source bytes fit the service policy");
    let source = root
        .child(7, bytes.len())
        .expect("the source window lies inside the root");
    let lines = super::lines(source, &ctx).expect("the source window splits into physical lines");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].payload.window(), b"D1@14_1;");
    assert_eq!(lines[0].extent.window(), b"D1@14_1;\r\n");
    assert_eq!(lines[0].payload.start(), 7);
    assert_eq!(lines[1].extent.start(), lines[0].extent.end());
    assert_eq!(lines[1].extent.window(), b"116,1,2,3;\r");
    assert_eq!(lines[2].extent.window(), b"D3;");
    assert_eq!(lines[2].extent.end(), bytes.len());
    assert_eq!(
        lines[0].extent.location(),
        root.child(7, 17)
            .expect("the first line extent lies inside the root")
            .location()
    );
}
