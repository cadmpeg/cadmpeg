// SPDX-License-Identifier: Apache-2.0
//! Production searches preserve resource refusal.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn model_brep_basename_lookup_preserves_work_refusal() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let blob_names = crate::container::design_breps(&ctx, &scan)
        .unwrap()
        .map(|brep| brep.map(|brep| brep.name.rsplit('/').next().unwrap().to_owned()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!blob_names.is_empty());
    let (candidates, _storage) =
        super::super::super::model_brep_candidates(&ctx, &scan, &blob_names).unwrap();
    assert_eq!(candidates.len(), blob_names.len());
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D model BREP by basename",
        0,
        |limited| {
            super::super::super::model_brep_candidates(limited, &scan, &blob_names).map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D model BREP by basename")
    );
}

#[test]
fn unique_asset_identity_search_preserves_work_refusal() {
    let asset = crate::test_support::with_decode_context(|ctx| {
        cadmpeg_ir::assets::Asset::try_new(
            ctx,
            cadmpeg_ir::assets::AssetId::mint("f3d:test:asset#same").unwrap(),
            Some("same.png".into()),
            Some("image/png".into()),
            cadmpeg_ir::assets::AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(vec![1]).unwrap(),
            },
            None,
        )
        .unwrap()
    });
    crate::test_support::with_decode_context(|ctx| {
        let mut assets = vec![asset.clone()];
        super::super::super::extend_unique_assets(ctx, &mut assets, vec![asset.clone()]).unwrap();
        assert_eq!(assets, std::slice::from_ref(&asset));
    });
    for operation in [
        "find F3D embedded asset identity",
        "index F3D embedded asset identities",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                super::super::super::extend_unique_assets(
                    ctx,
                    &mut vec![asset.clone()],
                    vec![asset.clone()],
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}
