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

#[test]
fn material_asset_attachment_retains_the_tiff_copy() {
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
    let scan = crate::decode::Scan {
        container,
        streams: Vec::new(),
    };
    let copy = cadmpeg_core::decode::u64_from_index(texture.len());
    for (limit, admitted) in [(copy, true), (copy - 1, false)] {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_retained_bytes = limit,
            |ctx| {
                let result = super::super::attach_material_texture_assets(
                    ctx,
                    &mut CadIr::empty(),
                    &textures,
                    &scan,
                    &mut AnnotationBuilder::new(),
                );
                if admitted {
                    result.unwrap();
                } else {
                    assert!(matches!(result.unwrap_err(),
                        cadmpeg_core::CodecError::ResourceLimit(limit)
                            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                                && limit.operation == "retain NX TIFF material asset"));
                }
            },
        );
    }
}
