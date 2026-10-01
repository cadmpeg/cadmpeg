// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{AnnotationBuilder, CadIr};

fn material_asset_result(configure: impl FnOnce(&mut DecodePolicy)) -> Result<CadIr, CodecError> {
    let texture = [b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/materialsTif/Material",
        texture.to_vec(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .unwrap();
    let textures = crate::test_support::with_decode_context(|ctx| {
        crate::native::om::material_texture::material_texture_assets(ctx, &container)
    })
    .unwrap();
    assert_eq!(textures.len(), 1);
    let scan = crate::decode::Scan {
        container,
        streams: Vec::new(),
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            let mut ir = CadIr::empty();
            let mut annotations = AnnotationBuilder::new();
            super::super::attach_material_texture_assets(
                ctx,
                &mut ir,
                &textures,
                &scan,
                &mut annotations,
            )?;
            Ok(ir)
        },
    )
}

#[test]
fn material_asset_attachment_preserves_tiff() {
    let ir = material_asset_result(|_| {}).unwrap();
    assert_eq!(ir.model.assets.len(), 1);
}

#[test]
fn material_asset_attachment_refuses_collection_limit() {
    let error = material_asset_result(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn material_asset_attachment_refuses_retained_limit() {
    let error = material_asset_result(|policy| policy.limits.max_retained_bytes = 0).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn material_asset_attachment_refuses_scoped_limit() {
    let error =
        material_asset_result(|policy| policy.limits.max_materialized_bytes = 0).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn material_asset_attachment_refuses_work_limit() {
    let error = material_asset_result(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}
