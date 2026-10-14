// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::{AnnotationBuilder, CadIr};

#[test]
fn material_asset_attachment_preserves_tiff() {
    let texture = [b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/materialsTif/Material",
        texture.to_vec(),
    )]);
    let container = crate::container::scan_bytes(file.as_slice()).unwrap();
    let textures = crate::test_support::with_decode_context(|ctx| {
        crate::native::om::material_texture::material_texture_assets(ctx, &container)
    })
    .unwrap();
    assert_eq!(textures.len(), 1);
    let scan = crate::decode::Scan {
        container,
        streams: Vec::new(),
    };

    let ir = crate::test_support::with_decode_context(|ctx| {
        let mut ir = CadIr::empty();
        let mut annotations = AnnotationBuilder::new();
        super::super::attach_material_texture_assets(
            ctx,
            &mut ir,
            &textures,
            &scan,
            &mut annotations,
        )
        .unwrap();
        ir
    });
    assert_eq!(ir.model.assets.len(), 1);
}
