use crate::container;
use crate::test_support::test_bytes::zlib_compress;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_streams::partition_stream;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn material_catalog_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let qaf = br#"<folderContents><folderProperties location="materialsTif/sample" unmappedLocation="materialsTif/Steel"><createTime>2026-01-01</createTime><modifyTime>2026-01-02</modifyTime></folderProperties></folderContents>"#;
    let texture = [b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/sample", texture.to_vec()),
        ("/Root/qafmetadata", qaf.to_vec()),
    ]);
    let scan_arena = DecodeArena::new();
    let scan_policy = DecodePolicy::service();
    let (scan_ctx, _) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy).unwrap();
    let container = container::scan_bytes(&scan_ctx, file.as_slice()).unwrap();
    let assets = super::super::material_texture::material_texture_assets(&scan_ctx, &container).unwrap();
    assert_eq!(assets.len(), 1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::material_texture_catalog_entries(&ctx, &container, &assets).unwrap_err()
}

#[test]
fn material_catalog_route_refuses_collection_limit() {
    let error = material_catalog_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn material_catalog_route_refuses_retained_limit() {
    let error = material_catalog_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn material_catalog_route_refuses_scoped_limit() {
    let error = material_catalog_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn material_catalog_route_refuses_work_limit() {
    let error = material_catalog_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}
