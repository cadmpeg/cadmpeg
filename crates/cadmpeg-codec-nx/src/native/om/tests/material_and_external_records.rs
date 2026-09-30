use crate::container;
use crate::om::reference_value::{DirectReference, RecordReference};
use crate::test_support::test_bytes::zlib_compress;
use crate::test_support::test_prt::assembly_with_external_paths;
use crate::test_support::test_prt::prt_with_arrangements;
use crate::test_support::test_prt::prt_with_indexed_om_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_prt::prt_with_size_framed_om_section;
use crate::test_support::test_prt::prt_with_two_bodies_and_rmfastload;
use crate::test_support::test_prt::rmfastload_prt;
use crate::test_support::test_streams::partition_stream;
use crate::NxCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeOptions;
use std::io::Cursor;

fn native_fastload_result(
    policy: DecodePolicy,
) -> Result<
    Option<(
        super::super::RmFastLoadObjectIdTable,
        Vec<super::super::RmFastLoadObjectId>,
    )>,
    CodecError,
> {
    let file = rmfastload_prt();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&file, &arena, &policy)?;
    let container = container::scan_bytes(&ctx, file.as_slice())?;
    super::super::rmfastload_object_id_table(&ctx, &container)
}

fn store_header_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let file = prt_with_indexed_om_section();
    let scan_arena = DecodeArena::new();
    let scan_policy = DecodePolicy::service();
    let (scan_ctx, _) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy).unwrap();
    let container = container::scan_bytes(&scan_ctx, file.as_slice()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::store_headers(&ctx, &container).unwrap_err()
}

#[test]
fn store_header_route_refuses_collection_limit() {
    let error = store_header_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn store_header_route_refuses_retained_limit() {
    let error = store_header_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn store_header_route_refuses_work_limit() {
    let error = store_header_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

fn indexed_om_projection_error(
    configure: impl FnOnce(&mut DecodePolicy),
    project: impl FnOnce(&DecodeContext<'_>, &container::Container) -> Result<(), CodecError>,
) -> CodecError {
    let file = prt_with_indexed_om_section();
    let scan_arena = DecodeArena::new();
    let scan_policy = DecodePolicy::service();
    let (scan_ctx, _) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy).unwrap();
    let container = container::scan_bytes(&scan_ctx, file.as_slice()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    project(&ctx, &container).unwrap_err()
}

#[test]
fn native_string_value_route_refuses_collection_limit() {
    let error = indexed_om_projection_error(
        |policy| policy.limits.max_collection_items = 0,
        |ctx, container| super::super::string_values(ctx, container).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn native_string_value_route_refuses_retained_limit() {
    let error = indexed_om_projection_error(
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx, container| super::super::string_values(ctx, container).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn native_string_value_route_refuses_work_limit() {
    let error = indexed_om_projection_error(
        |policy| policy.limits.max_work_units = 0,
        |ctx, container| super::super::string_values(ctx, container).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn native_object_reference_route_refuses_collection_limit() {
    let error = indexed_om_projection_error(
        |policy| policy.limits.max_collection_items = 0,
        |ctx, container| super::super::object_references(ctx, container).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn native_object_reference_route_refuses_retained_limit() {
    let error = indexed_om_projection_error(
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx, container| super::super::object_references(ctx, container).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn native_object_reference_route_refuses_work_limit() {
    let error = indexed_om_projection_error(
        |policy| policy.limits.max_work_units = 0,
        |ctx, container| super::super::object_references(ctx, container).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

fn assert_fastload_limit(error: &CodecError, dimension: ResourceDimension, operation: &str) {
    let CodecError::ResourceLimit(limit) = error else {
        panic!("FastLoad must return a resource refusal: {error}");
    };
    assert_eq!(limit.dimension, dimension);
    assert_eq!(limit.operation, operation);
}

#[test]
fn decode_retains_strict_tiff_material_texture_assets() {
    let texture = [b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
    let malformed = [b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/AISI Steel 4340", texture.to_vec()),
        ("/Root/materialsTif/Truncated", malformed.to_vec()),
    ]);

    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let assets = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<crate::native::om::material_texture::MaterialTextureAsset>(
            "material_texture_assets",
        )
        .expect("required invariant");

    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].name(), "AISI Steel 4340");
    assert_eq!(
        serde_json::to_value(assets[0].byte_order).unwrap(),
        "little_endian"
    );
    assert_eq!(serde_json::to_value(&assets[0]).unwrap()["version"], 42);
    assert_eq!(assets[0].first_ifd_offset(), 8);
    assert_eq!(
        assets[0].byte_len(),
        cadmpeg_core::decode::u64_from_index(texture.len())
    );
    assert_eq!(
        assets[0].sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&texture)
    );
    assert_eq!(
        assets[0].source_entry(),
        "/Root/materialsTif/AISI Steel 4340"
    );
}

#[test]
fn decode_joins_qaf_material_names_to_texture_assets() {
    let texture = [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0];
    let qaf = br#"<?xml version="1.0" encoding="UTF-8"?>
<folderContents>
<folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
<folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
</folderContents>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/unmap$1", texture.to_vec()),
        ("/Root/qafmetadata", qaf.to_vec()),
    ]);

    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let namespace = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant");
    let assets = namespace
        .arena_as::<crate::native::om::material_texture::MaterialTextureAsset>(
            "material_texture_assets",
        )
        .expect("required invariant");
    let catalog = namespace
        .arena_as::<super::super::MaterialTextureCatalogEntry>("material_texture_catalog_entries")
        .expect("required invariant");

    assert_eq!(assets.len(), 1);
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].texture_asset, assets[0].id);
    assert_eq!(catalog[0].storage_path, "materialsTif/unmap$1");
    assert_eq!(
        catalog[0].material_path,
        "materialsTif/Carbon Fiber Harness Satin Coated"
    );
    assert_eq!(catalog[0].create_time, "2026-07-15T08:01:00");
    assert_eq!(catalog[0].modify_time, "2026-07-15T08:02:00");
    assert_eq!(catalog[0].source_entry, "/Root/qafmetadata");
}

#[test]
fn decode_rejects_ambiguous_nx_arrangement_table_atomically() {
    for arrangements in [
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="YES" Name="Exploded"/></Arrangements>"#.as_slice(),
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Model"/></Arrangements>"#.as_slice(),
    ] {
        let file = prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/part/arrangements", arrangements.to_vec()),
        ]);
        let mut cur = Cursor::new(file);
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).expect("required invariant");
        assert!(result.ir().native.namespace("nx").is_none_or(|namespace| {
            namespace
                .arena_as::<super::super::Configuration>("configurations")
                .expect("required invariant")
                .is_empty()
        }));
        assert!(result.ir().model.configurations.is_empty());
    }
}

#[test]
fn decode_rejects_duplicate_nx_configuration_stream_paths_atomically() {
    let arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    let attributes = br#"<UgAttributes version="4"><Attribute owner="part" pdmBased="false" utf8title="NX_Arrangement" utf8value="Model" version="3" type="StringAttributeType"/></UgAttributes>"#.to_vec();
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements.clone()),
        ("/Root/part/arrangements", arrangements.clone()),
        ("/Root/part/attrs", attributes.clone()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    assert!(result.ir().model.configurations.is_empty());

    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
        ("/Root/part/attrs", attributes.clone()),
        ("/Root/part/attrs", attributes),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    assert_eq!(result.ir().model.configurations.len(), 1);
    assert!(!result.ir().model.configurations[0].active);
    assert!(result.ir().model.configurations[0].bodies.is_none());
    assert!(result.ir().native.namespace("nx").is_none_or(|namespace| {
        namespace
            .arena_as::<super::super::PartAttribute>("part_attributes")
            .expect("required invariant")
            .is_empty()
    }));
}

#[test]
fn assembly_metadata_lists_external_child_paths() {
    let mut cur = Cursor::new(assembly_with_external_paths());
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let attrs = &result.ir().source.as_ref().expect("source").attributes;
    assert_eq!(
        attrs.get("external_reference.0").map(String::as_str),
        Some("child.prt")
    );
    assert_eq!(
        attrs.get("external_reference.1").map(String::as_str),
        Some("nested/b.prt")
    );
    let references = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX native namespace")
        .arena_as::<super::super::ExternalReference>("external_references")
        .expect("typed external references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].ordinal, 0);
    assert_eq!(references[0].path, "child.prt");
    assert_eq!(references[1].ordinal, 1);
    assert_eq!(references[1].path, "nested/b.prt");
    assert!(references[0].source_offset < references[1].source_offset);
}

#[test]
fn external_reference_extraction_refuses_record_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let file = assembly_with_external_paths();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
            .expect("external reference container");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two strings enter both the parsed table and the container result before
    // extraction admits the two native records.
    policy.limits.max_collection_items = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    let error = super::super::external_references(&ctx, &container)
        .expect_err("two native records exceed the remaining collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx external references"
    ));
}

fn native_external_record_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceRecord>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::external_reference_records(&ctx, &container)
}

#[test]
fn native_external_record_route_preserves_indexed_record() {
    let records = native_external_record_result(|_| {}).expect("native indexed record");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
}

#[test]
fn native_external_record_route_refuses_collection_limit() {
    let error = native_external_record_result(|policy| policy.limits.max_collection_items = 10)
        .expect_err("native record exceeds the parsed collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference records"),
        "{error:?}"
    );
}

#[test]
fn native_external_record_route_refuses_retained_limit() {
    let error = native_external_record_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("indexed record exceeds the retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

fn native_external_indexed_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceIndexedRecord>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let decoded = crate::test_support::with_decode_context(|ctx| {
        super::super::external_reference_records(ctx, &container)
    })
    .expect("handle-set record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::external_reference_indexed_records(&ctx, &container, &decoded)
}

#[test]
fn native_external_indexed_route_preserves_record_links() {
    let records = native_external_indexed_result(|_| {}).expect("native indexed records");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].record_id, 7);
    assert_eq!(records[1].record_id, 6);
    assert!(records[0].handle_set_record.is_none());
    assert_eq!(
        records[1].handle_set_record.as_deref(),
        Some("nx:external-reference-record:/Root/ExternalReferences#6")
    );
}

#[test]
fn native_external_indexed_route_refuses_collection_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_collection_items = 9)
        .expect_err("native indexed record exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference indexed records"),
        "{error:?}"
    );
}

#[test]
fn native_external_indexed_route_refuses_retained_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("indexed record exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn native_external_indexed_route_refuses_scoped_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("decoded index exceeds scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx external reference decoded index"),
        "{error:?}"
    );
}

#[test]
fn native_external_indexed_route_refuses_work_limit() {
    let error = native_external_indexed_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("indexed scan exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

fn native_external_empty_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceEmptyRecord>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let indexed = crate::test_support::with_decode_context(|ctx| {
        let records = super::super::external_reference_records(ctx, &container)?;
        super::super::external_reference_indexed_records(ctx, &container, &records)
    })
    .expect("native indexed records");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::external_reference_empty_records(&ctx, &container, &indexed)
}

#[test]
fn native_external_empty_route_preserves_record() {
    let records = native_external_empty_result(|_| {}).expect("native empty record");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].id,
        "nx:external-reference-empty-record:/Root/ExternalReferences#7"
    );
    assert!(!records[0].closing_marker);
}

#[test]
fn native_external_empty_route_refuses_collection_limit() {
    let error = native_external_empty_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("native empty record exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference empty records"),
        "{error:?}"
    );
}

#[test]
fn native_external_empty_route_refuses_retained_limit() {
    let error = native_external_empty_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("native empty record exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native external reference empty records"),
        "{error:?}"
    );
}

fn native_external_tail_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceTailReferencePair>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let records = crate::test_support::with_decode_context(|ctx| {
        super::super::external_reference_records(ctx, &container)
    })
    .expect("handle-set record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::external_reference_tail_reference_pairs(&ctx, &container, &records)
}

#[test]
fn native_external_tail_route_preserves_pair() {
    let pairs = native_external_tail_result(|_| {}).expect("external tail pair");
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].ordinal, 0);
    assert_eq!(pairs[0].persistent_handle, 5);
}

#[test]
fn native_external_tail_route_refuses_collection_limit() {
    let error = native_external_tail_result(|policy| policy.limits.max_collection_items = 1)
        .expect_err("native pair exceeds parsed collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native external reference tail pairs"),
        "{error:?}"
    );
}

#[test]
fn native_external_tail_route_refuses_retained_limit() {
    let error = native_external_tail_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("external pair exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn native_external_tail_route_refuses_work_limit() {
    let error = native_external_tail_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("external pair scan exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

fn native_external_string_uses_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceRecordStringUse>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let (records, references) =
        crate::test_support::with_decode_context(|ctx| -> Result<_, CodecError> {
            Ok((
                super::super::external_reference_records(ctx, &container)?,
                super::super::external_references(ctx, &container)?,
            ))
        })
        .expect("native external-reference inputs");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::external_reference_record_string_uses(&ctx, &records, &references)
}

#[test]
fn native_external_string_uses_route_preserves_slots() {
    let uses = native_external_string_uses_result(|_| {}).expect("external string uses");
    assert_eq!(uses.len(), 4);
    assert_eq!(
        uses.iter()
            .map(|use_| use_.string_index)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
}

#[test]
fn native_external_string_uses_route_refuses_collection_limit() {
    let error = native_external_string_uses_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("slot index exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx external reference slot index"),
        "{error:?}"
    );
}

#[test]
fn native_external_string_uses_route_refuses_retained_limit() {
    let error = native_external_string_uses_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("slot use exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native external reference string uses"),
        "{error:?}"
    );
}

#[test]
fn native_external_string_uses_route_refuses_scoped_limit() {
    let error =
        native_external_string_uses_result(|policy| policy.limits.max_materialized_bytes = 0)
            .expect_err("slot index exceeds scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx external reference slot index"),
        "{error:?}"
    );
}

#[test]
fn native_external_string_uses_route_refuses_work_limit() {
    let error = native_external_string_uses_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("slot index exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "nx external reference slot index"),
        "{error:?}"
    );
}

fn native_external_children_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ExternalReferenceRecordChild>, CodecError> {
    let file = prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("indexed external-reference container");
    let (records, references, uses) =
        crate::test_support::with_decode_context(|ctx| -> Result<_, CodecError> {
            let records = super::super::external_reference_records(ctx, &container)?;
            let references = super::super::external_references(ctx, &container)?;
            let uses =
                super::super::external_reference_record_string_uses(ctx, &records, &references)?;
            Ok((records, references, uses))
        })
        .expect("complete external-reference child inputs");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::external_reference_record_children(&ctx, &records, &references, &uses)
}

#[test]
fn native_external_children_route_preserves_child() {
    let children = native_external_children_result(|_| {}).expect("external child");
    assert_eq!(children.len(), 1);
    assert_eq!(
        children[0].id,
        "nx:external-reference-record:/Root/ExternalReferences#6:child"
    );
}

#[test]
fn native_external_children_route_refuses_collection_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("child index exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx external reference child index"),
        "{error:?}"
    );
}

#[test]
fn native_external_children_route_refuses_retained_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("child record exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native external reference children"),
        "{error:?}"
    );
}

#[test]
fn native_external_children_route_refuses_scoped_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("child index exceeds scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx external reference child index"),
        "{error:?}"
    );
}

#[test]
fn native_external_children_route_refuses_work_limit() {
    let error = native_external_children_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("child index exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "nx external reference child index"),
        "{error:?}"
    );
}

fn active_configuration_join_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ConfigurationAttributeUse>, CodecError> {
    let file = prt_with_arrangements();
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("arrangement container");
    let configurations = crate::test_support::with_decode_context(|ctx| {
        super::super::configurations(ctx, &container)
    })
    .expect("arrangement table");
    let attributes = crate::test_support::with_decode_context(|ctx| {
        super::super::part_attributes(ctx, &container)
    })
    .expect("part attribute table");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::configuration_attribute_uses(&ctx, &configurations, &attributes)
}

#[test]
fn active_configuration_join_route_preserves_relation() {
    let uses = active_configuration_join_result(|_| {}).expect("active configuration use");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "Model");
}

#[test]
fn active_configuration_join_route_refuses_collection_limit() {
    let error = active_configuration_join_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("active relation exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx active configuration attribute uses"),
        "{error:?}"
    );
}

#[test]
fn active_configuration_join_route_refuses_retained_limit() {
    let error = active_configuration_join_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("active relation exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx active configuration attribute uses"),
        "{error:?}"
    );
}

#[test]
fn active_configuration_join_route_refuses_work_limit() {
    let error = active_configuration_join_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("active relation exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "nx active configuration join"),
        "{error:?}"
    );
}

fn arrangement_configuration_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::Configuration>, CodecError> {
    let file = prt_with_arrangements();
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("arrangement container");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::configurations(&ctx, &container)
}

#[test]
fn arrangement_configuration_route_preserves_order() {
    let configurations = arrangement_configuration_result(|_| {}).expect("arrangement table");
    assert_eq!(configurations.len(), 2);
    assert_eq!(configurations[0].name, "Model");
    assert_eq!(configurations[1].name, "Exploded");
}

#[test]
fn arrangement_configuration_route_refuses_collection_limit() {
    let error = arrangement_configuration_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("arrangement names exceed collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx arrangement names"),
        "{error:?}"
    );
}

#[test]
fn arrangement_configuration_route_refuses_retained_limit() {
    let error = arrangement_configuration_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("arrangement records exceed retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx arrangement configurations"),
        "{error:?}"
    );
}

#[test]
fn arrangement_configuration_route_refuses_scoped_limit() {
    let error = arrangement_configuration_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("arrangement names exceed scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "nx arrangement names"),
        "{error:?}"
    );
}

#[test]
fn arrangement_configuration_route_refuses_work_limit() {
    let error = arrangement_configuration_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("arrangement XML exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "nx arrangement XML scan"),
        "{error:?}"
    );
}

fn part_attribute_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::PartAttribute>, CodecError> {
    let file = prt_with_arrangements();
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("part attribute container");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::part_attributes(&ctx, &container)
}

#[test]
fn part_attribute_route_preserves_typed_value() {
    let attributes = part_attribute_result(|_| {}).expect("typed part attribute");
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].title, "NX_Arrangement");
    assert_eq!(attributes[0].value, "Model");
}

#[test]
fn part_attribute_route_refuses_collection_limit() {
    let error = part_attribute_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("part attribute exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "nx native part attributes"),
        "{error:?}"
    );
}

#[test]
fn part_attribute_route_refuses_retained_limit() {
    let error = part_attribute_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("part attribute exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx native part attributes"),
        "{error:?}"
    );
}

#[test]
fn part_attribute_route_refuses_work_limit() {
    let error = part_attribute_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("part attribute XML exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "nx part attribute XML scan"),
        "{error:?}"
    );
}

fn class_definition_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::ClassDefinition>, CodecError> {
    let file = prt_with_indexed_om_section();
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("class definition container");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::class_definitions(&ctx, &container)
}

fn field_definition_result(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<Vec<super::super::FieldDefinition>, CodecError> {
    let file = prt_with_size_framed_om_section();
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("field definition container");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::field_definitions(&ctx, &container)
}

#[test]
fn registry_class_route_preserves_definition() {
    let classes = class_definition_result(|_| {}).expect("class definition");
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "UGS::EXP_expression");
}

#[test]
fn registry_class_route_refuses_collection_limit() {
    let error = class_definition_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("class definition exceeds collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems),
        "{error:?}"
    );
}

#[test]
fn registry_class_route_refuses_retained_limit() {
    let error = class_definition_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("class definition exceeds retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn registry_class_route_refuses_scoped_limit() {
    let error = class_definition_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("class definition exceeds scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes),
        "{error:?}"
    );
}

#[test]
fn registry_class_route_refuses_work_limit() {
    let error = class_definition_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("class definition exceeds work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

#[test]
fn registry_field_route_preserves_definitions() {
    let fields = field_definition_result(|_| {}).expect("field definitions");
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "m_target");
}

#[test]
fn registry_field_route_refuses_collection_limit() {
    let error = field_definition_result(|policy| policy.limits.max_collection_items = 0)
        .expect_err("field definitions exceed collection budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems),
        "{error:?}"
    );
}

#[test]
fn registry_field_route_refuses_retained_limit() {
    let error = field_definition_result(|policy| policy.limits.max_retained_bytes = 0)
        .expect_err("field definitions exceed retained budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn registry_field_route_refuses_scoped_limit() {
    let error = field_definition_result(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("field definitions exceed scoped budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes),
        "{error:?}"
    );
}

#[test]
fn registry_field_route_refuses_work_limit() {
    let error = field_definition_result(|policy| policy.limits.max_work_units = 0)
        .expect_err("field definitions exceed work budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

#[test]
fn persistent_handle_identity_bridges_om_and_external_records() {
    let reference = super::super::ObjectReference {
        id: "nx:test:reference#0".into(),
        record: "nx:test:om-record#0".into(),
        object_id: 1,
        ordinal: 0,
        reference: RecordReference::Direct(DirectReference::PersistentHandle(0x1020_3040)),
        source_entry: "om".into(),
        source_offset: 0,
    };
    let external = super::super::ExternalReferenceRecord {
        id: "nx:test:external-record#6".into(),
        record_id: 6,
        declared_count: 1,
        id_slots: [0; 4],
        handles: crate::container::extref_handles::ExtrefHandles::new(vec![
            0x1020_3040,
            0x1020_3040,
        ])
        .unwrap(),
        tail_byte_len: 0,
        source_entry: "external".into(),
        source_offset: 10,
    };
    let control = super::super::DataBlockControlReference {
        id: "nx:test:control-reference#0".into(),
        data_block: "nx:test:control-block#0".into(),
        ordinal: 0,
        reference: DirectReference::PersistentHandle(0x1020_3040),
        source_offset: 20,
    };

    let tail_pair = super::super::ExternalReferenceTailReferencePair {
        id: "nx:test:tail-pair#0".into(),
        handle_set_record: external.id.clone(),
        ordinal: 0,
        persistent_handle: 0x5060_7080,
        tagged_reference: crate::om::reference_value::Tagged28::try_from(7).unwrap(),
        source_offset: 30,
    };

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let handles =
        super::super::persistent_handles(&ctx, &[reference], &[control], &[external], &[tail_pair])
            .unwrap();

    assert_eq!(handles.len(), 2);
    assert_eq!(handles[0].records, ["nx:test:om-record#0"]);
    assert_eq!(handles[0].occurrence_count, 2);
    assert_eq!(handles[0].data_blocks, ["nx:test:control-block#0"]);
    assert_eq!(handles[0].external_records, ["nx:test:external-record#6"]);
    assert_eq!(handles[0].external_occurrence_count, 2);
    assert_eq!(handles[1].value, 0x5060_7080);
    assert_eq!(handles[1].external_records, ["nx:test:external-record#6"]);
    assert_eq!(handles[1].external_occurrence_count, 1);
}

fn persistent_handle_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let reference = super::super::ObjectReference {
        id: "reference".into(),
        record: "record".into(),
        object_id: 1,
        ordinal: 0,
        reference: RecordReference::Direct(DirectReference::PersistentHandle(17)),
        source_entry: "om".into(),
        source_offset: 0,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::persistent_handles(&ctx, &[reference], &[], &[], &[]).unwrap_err()
}

#[test]
fn persistent_handle_route_refuses_collection_limit() {
    let error = persistent_handle_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn persistent_handle_route_refuses_scoped_limit() {
    let error = persistent_handle_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn persistent_handle_route_refuses_retained_limit() {
    let error = persistent_handle_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn persistent_handle_route_refuses_work_limit() {
    let error = persistent_handle_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn nx_control_handle_pairs_require_maximal_runs_of_exactly_two() {
    let reference = |ordinal: u32, offset: u64| super::super::DataBlockControlReference {
        id: format!("reference#{ordinal}"),
        data_block: "block#0".into(),
        ordinal,
        reference: DirectReference::PersistentHandle(ordinal + 100),
        source_offset: offset,
    };
    let references = [
        reference(0, 10),
        reference(1, 15),
        reference(2, 30),
        reference(3, 35),
        reference(4, 40),
    ];
    let pairs = crate::test_support::with_decode_context(|ctx| {
        super::super::data_block_control_handle_pairs(ctx, &references)
    })
    .unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].id, "nx:om-data-block-control:handle-pair#10");
    assert_eq!(pairs[0].first_reference, "reference#0");
    assert_eq!(pairs[0].second_reference, "reference#1");
    assert_eq!(pairs[0].first_handle, 100);
    assert_eq!(pairs[0].second_handle, 101);
}

fn control_handle_pair_refusal(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let reference = |ordinal: u32, source_offset: u64| super::super::DataBlockControlReference {
        id: format!("reference#{ordinal}"),
        data_block: "block#0".into(),
        ordinal,
        reference: DirectReference::PersistentHandle(ordinal + 100),
        source_offset,
    };
    let references = [reference(0, 10), reference(1, 15)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::data_block_control_handle_pairs(&ctx, &references).unwrap_err()
}

#[test]
fn control_handle_pair_route_refuses_collection_limit() {
    let error = control_handle_pair_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "NX control handle pair blocks"));
}

#[test]
fn control_handle_pair_route_refuses_scoped_limit() {
    let error = control_handle_pair_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "NX control handle pair index"));
}

#[test]
fn control_handle_pair_route_refuses_retained_limit() {
    let error = control_handle_pair_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain NX control handle pair id"));
}

#[test]
fn control_handle_pair_route_refuses_work_limit() {
    let error = control_handle_pair_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index NX control handle pair references"));
}

#[test]
fn nx_object_record_handle_pairs_do_not_cross_records_or_long_runs() {
    let reference = |record: &str, ordinal: u32, offset: u64| super::super::ObjectReference {
        id: format!("{record}:reference#{ordinal}"),
        record: record.into(),
        object_id: 7,
        ordinal,
        reference: RecordReference::Direct(DirectReference::PersistentHandle(ordinal + 100)),
        source_entry: "om".into(),
        source_offset: offset,
    };
    let references = [
        reference("record#0", 0, 10),
        reference("record#0", 1, 15),
        reference("record#0", 2, 30),
        reference("record#0", 3, 35),
        reference("record#0", 4, 40),
        reference("record#1", 5, 20),
        reference("record#1", 6, 25),
    ];

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let pairs = super::super::object_record_handle_pairs(&ctx, &references).unwrap();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].record, "record#0");
    assert_eq!(pairs[0].first_reference, "record#0:reference#0");
    assert_eq!(pairs[0].second_reference, "record#0:reference#1");
    assert_eq!(pairs[0].object_id, 7);
    assert_eq!(pairs[1].record, "record#1");
    assert_eq!(pairs[1].source_offset, 20);
}

fn record_handle_pair_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let reference = |ordinal, source_offset| super::super::ObjectReference {
        id: format!("reference#{ordinal}"),
        record: "record".into(),
        object_id: 7,
        ordinal,
        reference: RecordReference::Direct(DirectReference::PersistentHandle(ordinal + 100)),
        source_entry: "om".into(),
        source_offset,
    };
    let references = [reference(0, 10), reference(1, 15)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::object_record_handle_pairs(&ctx, &references).unwrap_err()
}

#[test]
fn record_handle_pair_route_refuses_collection_limit() {
    let error = record_handle_pair_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn record_handle_pair_route_refuses_scoped_limit() {
    let error = record_handle_pair_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn record_handle_pair_route_refuses_retained_limit() {
    let error = record_handle_pair_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn record_handle_pair_route_refuses_work_limit() {
    let error = record_handle_pair_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn native_retains_rmfastload_table_and_member_words() {
    let (entry_offset, table, object_ids) = crate::test_support::with_decode_context(|ctx| {
        let container = container::scan_bytes(ctx, rmfastload_prt()).expect("required invariant");
        let entry_offset = container
            .entries
            .iter()
            .find(|entry| entry.name == "/Root/FastLoad/RMFastLoad")
            .and_then(crate::container::DirEntry::file_span)
            .expect("RMFastLoad span")
            .0;
        let (table, object_ids) = super::super::rmfastload_object_id_table(ctx, &container)
            .expect("native RMFastLoad admission")
            .expect("native RMFastLoad table");
        (entry_offset, table, object_ids)
    });

    assert_eq!(table.id, "nx:rmfastload:object-id-table#0");
    assert_eq!(table.members.as_slice().len(), 50);
    assert_eq!(table.raw_count(), 50u32.to_le_bytes());
    assert_eq!(table.registry_source_offset, entry_offset);
    assert_eq!(
        table.source_offset,
        entry_offset + cadmpeg_core::decode::u64_from_index(b"UGS::Solid::Topol".len())
    );
    assert_eq!(object_ids[0].table, table.id);
    assert_eq!(object_ids[0].value, 1);
    assert_eq!(
        object_ids[0].stable_identity.as_deref(),
        Some("nx:rmfastload:object-id-table#0:value#1")
    );
    assert_eq!(object_ids[0].raw(), 1u32.to_le_bytes());
    assert_eq!(object_ids[0].source_offset, table.source_offset + 4);
    assert_eq!(object_ids[49].ordinal, 49);
    assert_eq!(object_ids[49].value, 50);
    assert_eq!(object_ids[49].raw(), 50u32.to_le_bytes());
    assert_eq!(table.members.as_slice()[49], object_ids[49].id);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::super::rmfastload_target_object_id(ctx, &object_ids, 0).unwrap(),
            Some(object_ids[0].id.clone())
        );
        assert_eq!(
            super::super::rmfastload_target_object_id(ctx, &object_ids, 49).unwrap(),
            Some(object_ids[49].id.clone())
        );
        assert_eq!(
            super::super::rmfastload_target_object_id(ctx, &object_ids, 50).unwrap(),
            None
        );
    });
}

#[test]
fn rmfastload_target_identity_refuses_retained_limit() {
    let (_, object_ids) = native_fastload_result(DecodePolicy::service())
        .expect("RMFastLoad input is valid")
        .expect("RMFastLoad table is present");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::rmfastload_target_object_id(&ctx, &object_ids, 0).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn service_profile_admits_fastload_native_members() {
    let (table, object_ids) = native_fastload_result(DecodePolicy::service())
        .expect("service FastLoad admission")
        .expect("FastLoad table");
    assert_eq!(table.members.as_slice().len(), 50);
    assert_eq!(object_ids.len(), 50);
}

#[test]
fn fastload_identity_map_refuses_collection_limit_before_reserve() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 100;
    let error = native_fastload_result(policy).expect_err("identity map needs fifty more items");
    assert_fastload_limit(
        &error,
        ResourceDimension::CollectionItems,
        "admit NX FastLoad identity counts",
    );
}

#[test]
fn fastload_identity_map_refuses_materialized_limit_before_reserve() {
    let mut policy = DecodePolicy::default();
    let map_entry_bytes = std::mem::size_of::<(u32, usize)>() + 4 * std::mem::size_of::<usize>();
    policy.limits.max_materialized_bytes =
        cadmpeg_core::decode::u64_from_index(50 * map_entry_bytes - 1);
    let error = native_fastload_result(policy).expect_err("identity map needs one more byte");
    assert_fastload_limit(
        &error,
        ResourceDimension::MaterializedBytes,
        "count NX FastLoad identities",
    );
}

#[test]
fn fastload_identity_map_refuses_work_limit_before_counting() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 100;
    let error = native_fastload_result(policy).expect_err("fifty IDs need one hundred work units");
    assert_fastload_limit(
        &error,
        ResourceDimension::WorkUnits,
        "count NX FastLoad identities",
    );
}

#[test]
fn fastload_native_records_refuse_collection_limit_before_reserve() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 201;
    let error =
        native_fastload_result(policy).expect_err("native records and links need one more item");
    assert_fastload_limit(
        &error,
        ResourceDimension::CollectionItems,
        "admit NX FastLoad native collections",
    );
}

#[test]
fn fastload_native_records_refuse_entity_limit_before_creation() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 50;
    let error = native_fastload_result(policy)
        .expect_err("fifty members and table need fifty-one entities");
    assert_fastload_limit(
        &error,
        ResourceDimension::Entities,
        "admit NX FastLoad native entities",
    );
}

#[test]
fn fastload_native_copies_refuse_retained_limit_before_creation() {
    let mut policy = DecodePolicy::default();
    let table_id = "nx:rmfastload:object-id-table#0";
    let member_id_len = "nx:rmfastload:object-id#".len() + 10;
    let stable_bytes = (1..=50)
        .map(|value| format!("{table_id}:value#{value}").len())
        .sum::<usize>();
    let directory_bytes =
        std::mem::size_of::<crate::container::DirEntry>() + "/Root/FastLoad/RMFastLoad".len();
    let parsed_id_bytes = 50 * std::mem::size_of::<u32>();
    let native_bytes = 50
        * (std::mem::size_of::<super::super::RmFastLoadObjectId>()
            + std::mem::size_of::<String>()
            + 2 * member_id_len
            + table_id.len())
        + table_id.len()
        + std::mem::size_of::<super::super::RmFastLoadObjectIdTable>()
        + "/Root/FastLoad/RMFastLoad".len()
        + stable_bytes;
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(directory_bytes + parsed_id_bytes + native_bytes - 1);
    let error = native_fastload_result(policy).expect_err("native copies need one more byte");
    assert_fastload_limit(
        &error,
        ResourceDimension::RetainedBytes,
        "retain NX FastLoad native copies",
    );
}

#[test]
fn decode_selects_dominant_rmfastload_body() {
    let mut cur = Cursor::new(prt_with_two_bodies_and_rmfastload());
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let namespace = result.ir().native.namespace("nx").expect("NX namespace");
    let tables = namespace
        .arena_as::<super::super::RmFastLoadObjectIdTable>("rmfastload_object_id_tables")
        .expect("RMFastLoad tables");
    let object_ids = namespace
        .arena_as::<super::super::RmFastLoadObjectId>("rmfastload_object_ids")
        .expect("RMFastLoad object IDs");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].members.as_slice().len(), 50);
    assert_eq!(object_ids.len(), 50);
    assert_eq!(object_ids[0].value, 1_000);
    assert_eq!(object_ids[49].value, 1_049);
    assert!(result.ir().model.bodies[0]
        .id
        .as_str()
        .starts_with("nx:s0:"));
    assert_eq!(result.ir().model.faces.len(), 50);
    assert_eq!(result.ir().model.surfaces.len(), 50);
    assert!(result
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.id.as_str().starts_with("nx:s0:")));
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str().starts_with("nx:s0:")));
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("active_body_selector"))
            .map(String::as_str),
        Some("rmfastload_object_id_membership")
    );
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn data_block_column_index_tables_require_complete_mode_and_target_sequence() {
    use super::super::data_block_column_index_tables;
    use crate::native::om::column_row::{DataBlockLinkedIndexRow, DataBlockTargetIndexRow};
    use crate::om::column_row::{LinkedRow, TargetRow};
    use crate::om::compact::CompactIndexTarget;

    let linked = |id: &str, target: u32, mode, offset: u64| DataBlockLinkedIndexRow {
        id: id.into(),
        section_ordinal: 2,
        ordinal: 0,
        frame: LinkedRow::<String, u64>::new(
            crate::om::compact::CompactIndexAtom::from_wire(20, &[128, 20]).unwrap(),
            crate::om::discriminators::LinkedIndexDiscriminator::Form16,
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::from_wire(
                    target,
                    &[u8::try_from(target).expect("fixture value fits u8")],
                )
                .unwrap(),
                target: format!("block#{target}"),
            },
            [5, 6, 7].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            crate::om::discriminators::LinkedIndexFlag::Form03,
            mode,
            offset,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: format!("opening-block-{id}"),
        opening_block_offset: 8,
    };
    let target = |id: &str, index: u32, mode, offset: u64| DataBlockTargetIndexRow {
        id: id.into(),
        section_ordinal: 2,
        ordinal: 0,
        frame: TargetRow::<String, u64>::new(
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::from_wire(
                    index,
                    &[u8::try_from(index).expect("fixture value fits u8")],
                )
                .unwrap(),
                target: format!("block#{index}"),
            },
            [5, 6, 7].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            mode,
            offset,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: format!("opening-block-{id}"),
        opening_block_offset: 8,
    };
    let linked_rows = [
        linked(
            "opening",
            63,
            crate::om::discriminators::IndexRowMode::Form07,
            100,
        ),
        linked(
            "linked-59",
            59,
            crate::om::discriminators::IndexRowMode::Form04,
            200,
        ),
        linked(
            "linked-58",
            58,
            crate::om::discriminators::IndexRowMode::Form04,
            225,
        ),
    ];
    let target_rows = [
        target(
            "target-62",
            62,
            crate::om::discriminators::IndexRowMode::Form07,
            125,
        ),
        target(
            "target-61",
            61,
            crate::om::discriminators::IndexRowMode::Form07,
            150,
        ),
        target(
            "target-60",
            60,
            crate::om::discriminators::IndexRowMode::Form04,
            175,
        ),
    ];

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let tables = data_block_column_index_tables(&ctx, &linked_rows, &target_rows).unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].id, "nx:om-data-block-column-index-tables:table#2");
    assert_eq!(tables[0].opening_linked_row, "opening");
    assert_eq!(
        tables[0].rows.target_rows(),
        ["target-62", "target-61", "target-60"]
    );
    assert_eq!(tables[0].rows.linked_rows(), ["linked-59", "linked-58"]);
    assert_eq!(
        serde_json::to_value(&tables[0]).unwrap()["first_target_index"],
        63
    );
    assert_eq!(tables[0].rows.last_target_index(), 58);
    assert_eq!(tables[0].source_offset, 100);

    let mut gap = target_rows.clone();
    gap[1] = target(
        "target-61",
        60,
        crate::om::discriminators::IndexRowMode::Form07,
        150,
    );
    assert!(data_block_column_index_tables(&ctx, &linked_rows, &gap)
        .unwrap()
        .is_empty());
    let mut incomplete_mode = target_rows.clone();
    incomplete_mode[2] = target(
        "target-60",
        60,
        crate::om::discriminators::IndexRowMode::Form07,
        175,
    );
    assert!(
        data_block_column_index_tables(&ctx, &linked_rows, &incomplete_mode)
            .unwrap()
            .is_empty()
    );
}

fn column_index_table_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    use crate::native::om::column_row::{DataBlockLinkedIndexRow, DataBlockTargetIndexRow};
    use crate::om::column_row::{LinkedRow, TargetRow};
    use crate::om::compact::{CompactIndexAtom, CompactIndexTarget};
    use crate::om::discriminators::{IndexRowMode, LinkedIndexDiscriminator, LinkedIndexFlag};

    let atom = |value: u32| {
        CompactIndexAtom::from_wire(
            value,
            &[u8::try_from(value).expect("fixture value fits u8")],
        )
        .unwrap()
    };
    let target = |value| CompactIndexTarget {
        atom: atom(value),
        target: format!("block#{value}"),
    };
    let linked = |id: &str, value, mode, offset| DataBlockLinkedIndexRow {
        id: id.into(),
        section_ordinal: 0,
        ordinal: 0,
        frame: LinkedRow::<String, u64>::new(
            atom(20),
            LinkedIndexDiscriminator::Form16,
            target(value),
            [5, 6, 7].map(target),
            LinkedIndexFlag::Form03,
            mode,
            offset,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening".into(),
        opening_block_offset: 0,
    };
    let target_row = |value, mode, offset| DataBlockTargetIndexRow {
        id: "target".into(),
        section_ordinal: 0,
        ordinal: 0,
        frame: TargetRow::<String, u64>::new(target(value), [5, 6, 7].map(target), mode, offset)
            .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening".into(),
        opening_block_offset: 0,
    };
    let linked_rows = [
        linked("opening", 63, IndexRowMode::Form07, 100),
        linked("linked", 61, IndexRowMode::Form04, 150),
    ];
    let target_rows = [target_row(62, IndexRowMode::Form04, 125)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::data_block_column_index_tables(&ctx, &linked_rows, &target_rows).unwrap_err()
}

#[test]
fn column_index_table_route_refuses_collection_limit() {
    let error = column_index_table_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn column_index_table_route_refuses_scoped_limit() {
    let error = column_index_table_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn column_index_table_route_refuses_retained_limit() {
    let error = column_index_table_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn column_index_table_route_refuses_work_limit() {
    let error = column_index_table_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn external_reference_record_slots_resolve_atomically_in_the_same_stream() {
    use super::super::{
        external_reference_record_children, external_reference_record_string_uses,
        ExternalReference, ExternalReferenceRecord,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");

    let references = (0..4)
        .map(|ordinal| ExternalReference {
            id: format!("reference#{ordinal}"),
            ordinal,
            path: format!("value-{ordinal}"),
            source_entry: "stream".into(),
            source_offset: 100 + u64::from(ordinal),
        })
        .collect::<Vec<_>>();
    let record = ExternalReferenceRecord {
        id: "record#7".into(),
        record_id: 7,
        declared_count: 2,
        id_slots: [0, 3, 1, 2],
        handles: crate::container::extref_handles::ExtrefHandles::new(vec![10, 20, 20]).unwrap(),
        tail_byte_len: 5,
        source_entry: "stream".into(),
        source_offset: 20,
    };
    let uses =
        external_reference_record_string_uses(&ctx, std::slice::from_ref(&record), &references)
            .expect("complete string use lane");
    assert_eq!(uses.len(), 4);
    assert_eq!(uses[0].id, "nx:external-reference:record-string-use#7-0");
    assert_eq!(
        uses.iter()
            .map(|use_| u8::from(use_.slot))
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(
        uses.iter()
            .map(|use_| use_.string_index)
            .collect::<Vec<_>>(),
        [0, 3, 1, 2]
    );
    assert_eq!(uses[1].external_reference, "reference#3");
    assert_eq!(uses[1].source_offset, 31);
    let mut child_references = references.clone();
    child_references[0].path = "child.prt".into();
    let child_uses = external_reference_record_string_uses(
        &ctx,
        std::slice::from_ref(&record),
        &child_references,
    )
    .expect("complete child string use lane");
    let children = external_reference_record_children(
        &ctx,
        std::slice::from_ref(&record),
        &child_references,
        &child_uses,
    )
    .expect("complete child record");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].external_record, record.id);
    assert_eq!(children[0].name_reference, "reference#0");
    assert_eq!(children[0].directory_reference, "reference#1");
    assert!(external_reference_record_children(
        &ctx,
        std::slice::from_ref(&record),
        &references,
        &uses
    )
    .expect("non-child record")
    .is_empty());

    let mut out_of_range = record.clone();
    out_of_range.id_slots[2] = 4;
    assert!(
        external_reference_record_string_uses(&ctx, &[out_of_range], &references)
            .expect("unresolved slot")
            .is_empty()
    );
    let mut duplicate = references.clone();
    duplicate.push(references[0].clone());
    assert!(
        external_reference_record_string_uses(&ctx, &[record], &duplicate)
            .expect("duplicate slot")
            .is_empty()
    );
}
