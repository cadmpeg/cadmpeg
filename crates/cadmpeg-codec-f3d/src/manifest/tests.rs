use std::collections::BTreeMap;

use super::{
    encode_asset_header, encode_design_asset, encode_top_level, generated_design_asset,
    parse_asset_header, parse_top_level, push_ascii, push_u32, push_utf16, resolve_design_folder,
    AssetKind, DESIGN_ASSET_TYPE, GENERATED_DESIGN_ASSET_BASE, GENERATED_DOCUMENT_ASSET_GUID,
    GENERATED_DOCUMENT_GUID, TOP_LEVEL_MANIFEST_VERSION,
};
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const DESIGN_GUID: &str = "10000000-0000-4000-8000-000000000001";
const OTHER_GUID: &str = "20000000-0000-4000-8000-000000000002";
const SECONDARY_GUID: &str = "30000000-0000-4000-8000-000000000003";

fn top_level_limit(items: u64, retained: u64) -> CodecError {
    let bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    parse_top_level(&ctx, &bytes).unwrap_err()
}

#[test]
fn manifest_ascii_refuses_retained_limit() {
    let error = top_level_limit(u64::MAX, 6);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D manifest ASCII"));
}

#[test]
fn manifest_utf16_refuses_retained_limit() {
    let error = top_level_limit(u64::MAX, 20);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D manifest UTF-16"));
}

#[test]
fn manifest_registry_index_refuses_collection_limit() {
    let error = top_level_limit(0, u64::MAX);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D manifest registry names"));
}

#[test]
fn manifest_asset_folder_run_refuses_collection_limit() {
    let error = top_level_limit(7, u64::MAX);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D asset folders"));
}

#[test]
fn manifest_entry_name_index_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let manifest = super::TopLevelManifest {
        version: TOP_LEVEL_MANIFEST_VERSION.to_owned(),
        asset_folder_bases: vec!["Design Base".to_owned()],
    };
    let error = resolve_design_folder(&ctx, &manifest, ["Design Base/Manifest.dat"], |_| None)
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D manifest entry names"));
}

#[test]
fn manifest_capability_index_refuses_collection_limit() {
    let bytes = generated_design_asset().unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = parse_asset_header(&ctx, &bytes).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D asset capability names"));
}

#[test]
fn manifest_active_name_refuses_materialization_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 13;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let manifest = super::TopLevelManifest {
        version: TOP_LEVEL_MANIFEST_VERSION.to_owned(),
        asset_folder_bases: vec!["Design".to_owned()],
    };
    let error = resolve_design_folder(&ctx, &manifest, ["Design/Manifest.dat"], |_| None)
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "name F3D active asset"));
}

#[test]
fn manifest_member_name_refuses_materialization_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 14;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let manifest = super::TopLevelManifest {
        version: TOP_LEVEL_MANIFEST_VERSION.to_owned(),
        asset_folder_bases: vec!["Design".to_owned()],
    };
    let error = resolve_design_folder(&ctx, &manifest, ["Design/Manifest.dat"], |_| None)
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "name F3D asset manifest"));
}

#[test]
fn manifest_error_text_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::parse_malformed(&ctx, "asset manifest", "invalid record");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "describe malformed F3D manifest"));
}

#[test]
fn current_top_level_manifest_round_trips() {
    let bytes = encode_top_level(DESIGN_GUID, &["Design Base", "Simulation"]).unwrap();
    let manifest = parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Design Base", "Simulation"]);
}

#[test]
fn legacy_top_level_manifest_accepts_both_terminal_forms() {
    let mut bytes = Vec::new();
    push_ascii(&mut bytes, "3-2-0-0").unwrap();
    push_ascii(&mut bytes, "FusionDocType").unwrap();
    push_utf16(&mut bytes, ".f3d").unwrap();
    push_utf16(&mut bytes, "Fusion Document").unwrap();
    push_utf16(&mut bytes, "A Fusion Document").unwrap();
    push_utf16(&mut bytes, GENERATED_DOCUMENT_GUID).unwrap();
    push_utf16(&mut bytes, GENERATED_DOCUMENT_ASSET_GUID).unwrap();
    push_u32(&mut bytes, 0x0800_06e1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 1);
    push_utf16(&mut bytes, OTHER_GUID).unwrap();
    push_utf16(&mut bytes, DESIGN_GUID).unwrap();
    push_u32(&mut bytes, 2);
    push_utf16(&mut bytes, "Simulation").unwrap();
    push_utf16(&mut bytes, "Design Base").unwrap();
    push_u32(&mut bytes, 0);

    let word_only = parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(word_only.asset_folder_bases, ["Simulation", "Design Base"]);

    bytes.push(0);
    push_utf16(&mut bytes, "Legacy Document").unwrap();

    let manifest = parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Simulation", "Design Base"]);

    push_utf16(&mut bytes, "urn:synthetic:lineage").unwrap();
    let with_lineage = parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(
        with_lineage.asset_folder_bases,
        ["Simulation", "Design Base"]
    );
}

#[test]
fn top_level_manifest_accepts_a_terminal_export_flag_without_a_marker() {
    let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut marker = Vec::new();
    push_utf16(&mut marker, "NA_EXPORT").unwrap();
    assert!(bytes.ends_with(&marker));
    bytes.truncate(bytes.len() - marker.len());

    let manifest = parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Design Base"]);
}

#[test]
fn design_folder_uses_root_fusion_asset_not_active_guid_or_run_order() {
    let manifest =
        parse_top_level(&cadmpeg_test_support::service_decode_context(), &encode_top_level(OTHER_GUID, &["Simulation", "Design Base"]).unwrap())
            .unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        "Simulation/Manifest.dat".to_string(),
        encode_fusion_subtype_asset("Simulation", OTHER_GUID, "Simulation").unwrap(),
    );
    entries.insert("Simulation/Breps.BlobParts/decoy.smbh".to_string(), vec![0]);
    entries.insert(
        "Design Base[Active]/Manifest.dat".to_string(),
        encode_design_asset("Design Base", DESIGN_GUID).unwrap(),
    );
    entries.insert(
        "Design Base[Active]/Design1/BulkStream.dat".to_string(),
        vec![1],
    );

    let folder = resolve_design_folder(&cadmpeg_test_support::service_decode_context(), &manifest, entries.keys().map(String::as_str), |name| {
        entries.get(name).map(Vec::as_slice)
    })
    .unwrap();
    assert_eq!(folder, "Design Base[Active]");
}

#[test]
fn active_guid_can_be_shared_by_a_non_design_asset() {
    let manifest =
        parse_top_level(&cadmpeg_test_support::service_decode_context(), &encode_top_level(DESIGN_GUID, &["Simulation", "Design Base"]).unwrap())
            .unwrap();
    let entries = BTreeMap::from([
        (
            "Simulation/Manifest.dat".to_string(),
            encode_asset_header(
                "Simulation",
                DESIGN_GUID,
                SECONDARY_GUID,
                "SimulationAssetType",
            )
            .unwrap(),
        ),
        (
            "Design Base/Manifest.dat".to_string(),
            encode_design_asset("Design Base", DESIGN_GUID).unwrap(),
        ),
        ("Design Base/Design1/BulkStream.dat".to_string(), vec![1]),
    ]);
    let folder = resolve_design_folder(&cadmpeg_test_support::service_decode_context(), &manifest, entries.keys().map(String::as_str), |name| {
        entries.get(name).map(Vec::as_slice)
    })
    .unwrap();
    assert_eq!(folder, "Design Base");
}

/// Replace the leading version field, keeping every later byte.
fn with_version(bytes: &[u8], version: &str) -> Vec<u8> {
    let mut supported = Vec::new();
    push_ascii(&mut supported, TOP_LEVEL_MANIFEST_VERSION).unwrap();
    assert!(bytes.starts_with(&supported));
    let mut replacement = Vec::new();
    push_ascii(&mut replacement, version).unwrap();
    [replacement.as_slice(), &bytes[supported.len()..]].concat()
}

#[test]
fn an_unknown_version_is_parsed_with_the_known_layout() {
    // Version-only drift: the attempt runs the same parse the known version
    // runs, and the reading of the version survives to the classifier.
    let known = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let bytes = with_version(&known, "3-3-0-0");

    let manifest = parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Design Base"]);
    assert_eq!(manifest.declared_version(), "3-3-0-0");
}

#[test]
fn a_broken_layout_is_malformed_for_every_declared_version() {
    let known = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut anchor = Vec::new();
    push_ascii(&mut anchor, "FusionDocType").unwrap();
    let at = known
        .windows(anchor.len())
        .position(|window| window == anchor)
        .expect("the kind anchor is present");
    let mut moved = Vec::new();
    push_ascii(&mut moved, "FusionDocTypX").unwrap();
    for version in [TOP_LEVEL_MANIFEST_VERSION, "3-3-0-0"] {
        let mut bytes = with_version(&known, version);
        bytes.splice(at..at + anchor.len(), moved.clone());
        let error = parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap_err();
        assert!(
            matches!(&error, CodecError::Malformed(message)
                    if message.contains(version) && message.contains("probable cause")),
            "expected a structural failure naming version {version}, found {error:?}"
        );
    }
}

#[test]
fn a_broken_top_level_manifest_version_field_stays_malformed() {
    let complete = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();

    let truncated = parse_top_level(&cadmpeg_test_support::service_decode_context(), &complete[..6]).unwrap_err();
    assert!(
        matches!(truncated, CodecError::Malformed(_)),
        "expected a malformed truncation, found {truncated:?}"
    );

    let mut non_ascii = complete.clone();
    non_ascii[4] = 0x01;
    let error = parse_top_level(&cadmpeg_test_support::service_decode_context(), &non_ascii).unwrap_err();
    assert!(
        matches!(error, CodecError::Malformed(_)),
        "expected a malformed non-ASCII version, found {error:?}"
    );
}

#[test]
fn top_level_manifest_rejects_trailing_bytes() {
    let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    bytes.push(0);
    assert!(parse_top_level(&cadmpeg_test_support::service_decode_context(), &bytes).is_err());
}

#[test]
fn generated_asset_manifest_has_a_joinable_header() {
    let bytes = generated_design_asset().unwrap();
    let header = parse_asset_header(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(header.base_name, GENERATED_DESIGN_ASSET_BASE);
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn revision_zero_design_asset_has_no_named_capability_registry() {
    let mut bytes = encode_asset_header(
        "Legacy Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 3);
    push_u32(&mut bytes, 1);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 6);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Design").unwrap();
    push_ascii(&mut bytes, "Design").unwrap();

    let header = parse_asset_header(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(header.base_name, "Legacy Design");
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn revision_ten_design_asset_carries_linked_document_triples() {
    let mut bytes = encode_asset_header(
        "Linked Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 10);
    push_u32(&mut bytes, 1);
    push_ascii(&mut bytes, "Application").unwrap();
    push_u32(&mut bytes, 52);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_u32(&mut bytes, 2);
    push_utf16(&mut bytes, "synthetic_urn:synthetic:version:1").unwrap();
    push_utf16(&mut bytes, DESIGN_GUID).unwrap();
    push_utf16(&mut bytes, OTHER_GUID).unwrap();
    push_u32(&mut bytes, 2);
    push_u32(&mut bytes, 5);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Design").unwrap();
    push_ascii(&mut bytes, "Design").unwrap();

    let header = parse_asset_header(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(header.base_name, "Linked Design");
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn revision_fourteen_uses_the_ascii_subtype_header() {
    let mut bytes =
        encode_asset_header("Design 14", DESIGN_GUID, SECONDARY_GUID, DESIGN_ASSET_TYPE).unwrap();
    push_u32(&mut bytes, 14);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_ascii(&mut bytes, "").unwrap();

    let header = parse_asset_header(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(header.base_name, "Design 14");
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn current_revisions_use_the_current_asset_header() {
    for revision in [11, 12, 13, 14, 15, 19, 20] {
        let mut bytes = encode_asset_header(
            "Intermediate Design",
            DESIGN_GUID,
            SECONDARY_GUID,
            DESIGN_ASSET_TYPE,
        )
        .unwrap();
        push_u32(&mut bytes, revision);
        push_u32(&mut bytes, 1);
        push_ascii(&mut bytes, "Application").unwrap();
        push_u32(&mut bytes, 139);
        push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
        bytes.push(0);
        push_ascii(&mut bytes, "").unwrap();

        let header = parse_asset_header(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
        assert_eq!(header.base_name, "Intermediate Design");
        assert_eq!(
            header.kind,
            AssetKind::Design {
                fusion_subtype: None
            }
        );
    }
}

#[test]
fn an_unknown_revision_uses_the_current_asset_header() {
    let mut bytes = encode_asset_header(
        "Future Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 99);
    push_u32(&mut bytes, 1);
    push_ascii(&mut bytes, "Application").unwrap();
    push_u32(&mut bytes, 139);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_ascii(&mut bytes, "").unwrap();

    let header = parse_asset_header(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(header.base_name, "Future Design");
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn a_broken_unknown_revision_names_the_revision_as_the_probable_cause() {
    let mut bytes = encode_asset_header(
        "Future Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 99);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "MovedAssetType").unwrap();

    let error = parse_asset_header(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap_err();
    assert!(
        matches!(&error, CodecError::Malformed(message)
                if message.contains("declared revision 99 is the probable cause")),
        "expected a structural failure naming the revision, found {error:?}"
    );
}

fn encode_fusion_subtype_asset(
    base_name: &str,
    primary_guid: &str,
    subtype: &str,
) -> Result<Vec<u8>, CodecError> {
    let mut bytes =
        encode_asset_header(base_name, primary_guid, SECONDARY_GUID, DESIGN_ASSET_TYPE)?;
    push_u32(&mut bytes, 20);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Neutron3DAssetType")?;
    bytes.push(0);
    push_ascii(&mut bytes, subtype)?;
    Ok(bytes)
}
