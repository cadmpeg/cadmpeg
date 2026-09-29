// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{AnnotationBuilder, CadIr};

const PREVIEW: [u8; 29] = [
    0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xb9, 0x00,
    0xf7, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
];

fn preview_attachment_result(configure: impl FnOnce(&mut DecodePolicy)) -> Result<(), CodecError> {
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/images/preview",
        PREVIEW.to_vec(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .unwrap();
    let scan = crate::decode::Scan {
        container,
        streams: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    super::super::attach_jpeg_preview_assets(&ctx, &mut ir, &scan, &mut annotations, &mut unknowns)
}

#[test]
fn preview_attachment_refuses_asset_collection_limit() {
    let error =
        preview_attachment_result(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "NX JPEG preview assets"));
}

#[test]
fn preview_attachment_refuses_asset_retained_limit() {
    let error = preview_attachment_result(|policy| {
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(PREVIEW.len());
    })
    .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "NX JPEG preview assets"));
}
