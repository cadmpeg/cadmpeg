// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::assembly_test::{
    f3d_with_identity_xrefs, f3d_with_redirections_json, f3d_without_brep,
    f3z_archive_with_design_description, redirections_json,
};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::products::{ExternalDocument, Storage};
use std::io::Write;

fn assert_identity(document: &ExternalDocument, key: &str) {
    assert_eq!(
        document.document_id.as_ref().map(NonBlankString::as_str),
        Some(format!("urn:adsk.wipprod:dm.lineage:{key}").as_str())
    );
    assert_eq!(
        document.version.as_ref().map(NonBlankString::as_str),
        Some(format!("urn:adsk.wipprod:fs.file:vf.{key}?version=1").as_str())
    );
}

#[test]
fn redirections_state_self_and_linked_document_urns() {
    let bytes = f3d_with_identity_xrefs("assembly-design", "root.f3d", &[("child.f3d", XREF_ROLE)]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let definitions = &decoded.ir().model.product_definitions;
    let root = definitions.iter().find(|row| row.is_root()).unwrap();
    let Storage::File { document, .. } = &root.storage else {
        panic!("file root")
    };
    assert_identity(document, "RootKey");
    let external = definitions
        .iter()
        .find_map(|row| match &row.storage {
            Storage::External(document) => Some(document),
            _ => None,
        })
        .unwrap();
    assert_identity(external, "Key0");
    assert_eq!(
        external.path.as_ref().map(NonBlankString::as_str),
        Some("child.f3d")
    );
    assert_eq!(
        crate::test_support::native_test::f3d_native(decoded.ir())
            .xref_designs
            .len(),
        2
    );
}

#[test]
fn unreferenced_design_identity_keeps_components_local() {
    let mut table: serde_json::Value = serde_json::from_str(&redirections_json(
        "root.f3d",
        &[("embedded.f3d", XREF_ROLE)],
    ))
    .unwrap();
    table["references"] = serde_json::json!({});
    for retain_table in [false, true] {
        let base = f3d_with_smbh(&synthetic_geometry_smbh());
        let mut zip = zip::ZipWriter::new_append(Cursor::new(base)).unwrap();
        let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        if retain_table {
            zip.start_file("RedirectionsStream.dat", options).unwrap();
            zip.write_all(&serde_json::to_vec(&table).unwrap()).unwrap();
        }
        let mut metadata =
            crate::test_support::streams_test::design_metastream_with_records(&[], &[]);
        metadata.truncate(metadata.len() - 8);
        metadata.extend_from_slice(&2u32.to_le_bytes());
        for key in ["RootKey", "Key0"] {
            metadata.extend_from_slice(&1u64.to_le_bytes());
            for guid in [
                "11111111-2222-3333-4444-555555555555",
                "66666666-7777-8888-9999-aaaaaaaaaaaa",
            ] {
                crate::test_support::lp_utf16(&mut metadata, guid);
            }
            crate::test_support::lp_utf16(
                &mut metadata,
                &format!("urn:adsk.wipprod:fs.file:vf.{key}?version=1"),
            );
            crate::test_support::lp_utf16(&mut metadata, "bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
            metadata.extend_from_slice(&1u32.to_le_bytes());
        }
        metadata.extend_from_slice(&0u32.to_le_bytes());
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", options)
            .unwrap();
        zip.write_all(&metadata).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let decoded = F3dCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(decoded
            .ir()
            .model
            .product_definitions
            .iter()
            .all(|row| !matches!(row.storage, Storage::External(_))));
        assert_eq!(
            crate::test_support::native_test::f3d_native(decoded.ir())
                .xref_designs
                .len(),
            if retain_table { 2 } else { 0 }
        );
        assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    }
}

#[test]
fn archive_description_supplies_member_and_unresolved_link_identities() {
    let mut table: serde_json::Value = serde_json::from_str(&redirections_json(
        "root.f3d",
        &[("child.f3d", XREF_ROLE), ("missing.f3d", "missing-role")],
    ))
    .unwrap();
    for design in table["designs"].as_array_mut().unwrap() {
        design["lineageUrn"] = "".into();
        design["versionUrn"] = "".into();
    }
    let root = f3d_with_redirections_json("assembly-design", &serde_json::to_vec(&table).unwrap());
    let child = include_bytes!("../../tests/golden/fixtures/generated_unit_cube.f3d");
    let objects: Vec<_> = [
        ("root.f3d", "Root"),
        ("child.f3d", "Child"),
        ("missing.f3d", "Missing"),
        ("embedded.f3d", "Embedded"),
    ]
    .iter()
    .map(|(path, key)| {
        serde_json::json!({
            "relativePath": path,
            "lineage": format!("urn:adsk.wipprod:dm.lineage:{key}"),
            "about": format!("urn:adsk.wipprod:fs.file:vf.{key}?version=1")
        })
    })
    .collect();
    let description =
        serde_json::json!({"designDescription":{"designGraphs":[{"designObjects":objects}]}});
    let bytes = f3z_archive_with_design_description(
        "root.f3d",
        &[("root.f3d", &root), ("child.f3d", child)],
        &serde_json::to_vec(&description).unwrap(),
    );
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(bytes),
        &DecodeOptions::default(),
    )
    .unwrap();
    let definitions = &decoded.ir().model.product_definitions;
    for (key, path) in [
        ("Root", None),
        ("Child", Some("child.f3d")),
        ("Missing", Some("missing.f3d")),
    ] {
        let document = definitions
            .iter()
            .find_map(|row| match &row.storage {
                Storage::File { document, .. } if path.is_none() && row.is_root() => Some(document),
                Storage::File { document, .. } | Storage::External(document)
                    if path.is_some()
                        && document.path.as_ref().map(NonBlankString::as_str) == path =>
                {
                    Some(document)
                }
                _ => None,
            })
            .unwrap();
        assert_identity(document, key);
    }
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().geometry.faces.len(), 6);
    assert_eq!(
        definitions
            .iter()
            .filter(|row| matches!(row.storage, Storage::External(_)))
            .count(),
        1
    );
}

#[test]
fn archive_merge_keeps_redirections_member_identity() {
    let root = f3d_with_identity_xrefs("assembly-design", "root.f3d", &[("child.f3d", XREF_ROLE)]);
    let child = f3d_without_brep("part-design", "child.f3d", &[]);
    let bytes = crate::test_support::assembly_test::f3z_archive(
        "root.f3d",
        &[("root.f3d", &root), ("child.f3d", &child)],
    );
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(bytes),
        &DecodeOptions::default(),
    )
    .unwrap();
    let document = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .find_map(|row| match &row.storage {
            Storage::File { document, .. }
                if document.path.as_ref().map(NonBlankString::as_str) == Some("child.f3d") =>
            {
                Some(document)
            }
            _ => None,
        })
        .unwrap();
    assert_identity(document, "RootKey");
}

fn api_export_with_placements(linked: bool, terminal: Option<(u8, &str)>) -> Vec<u8> {
    api_export_with_identity_fields(linked, terminal, None)
}

fn api_export_with_identity_fields(
    linked: bool,
    terminal: Option<(u8, &str)>,
    blank: Option<&str>,
) -> Vec<u8> {
    api_export_build(linked, terminal, blank, false, "child.f3d")
}

fn api_export_build(
    linked: bool,
    terminal: Option<(u8, &str)>,
    blank: Option<&str>,
    keep_redirections: bool,
    child_path: &str,
) -> Vec<u8> {
    use std::io::Read;
    let bytes = f3d_with_identity_xrefs("assembly-design", "root.f3d", &[("child.f3d", XREF_ROLE)]);
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        if name == "RedirectionsStream.dat" && !keep_redirections {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == "Manifest.dat" {
            let manifest = crate::manifest::parse_top_level(&bytes).unwrap();
            let target = [(XREF_ROLE, "ChildKey", child_path)];
            let mut extension =
                crate::test_support::manifest_test::api_reference_extension(if linked {
                    &target
                } else {
                    &[]
                });
            if let Some(field) = blank {
                let mut key = Vec::new();
                crate::test_support::lp_utf16(&mut key, field);
                let at = extension
                    .windows(key.len())
                    .position(|window| window == key)
                    .unwrap()
                    + key.len();
                let count = cadmpeg_core::decode::View::u32_le_at(&extension, at).unwrap();
                extension.splice(at..at + 4 + usize::try_from(count).unwrap() * 2, [0; 4]);
            }
            bytes.splice(manifest.reference_extension, extension);
            if let Some((flag, lineage)) = terminal {
                let mut marker = vec![1];
                crate::test_support::lp_utf16(&mut marker, "NA_EXPORT");
                assert!(bytes.ends_with(&marker));
                bytes.truncate(bytes.len() - marker.len());
                bytes.push(flag);
                if flag == 0 {
                    crate::test_support::lp_utf16(&mut bytes, "Root");
                }
                crate::test_support::lp_utf16(&mut bytes, lineage);
            }
        }
        zip.start_file(name, options).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn api_export_manifest_export_marker_keeps_root_lineage_unassigned() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(api_export_with_placements(true, None)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let definitions = &decoded.ir().model.product_definitions;
    let child = definitions
        .iter()
        .find_map(|row| match &row.storage {
            Storage::External(document) => Some(document),
            _ => None,
        })
        .unwrap();
    assert_eq!(child.path.as_ref().unwrap().as_str(), "child.f3d");
    assert_eq!(
        child.document_id.as_ref().unwrap().as_str(),
        "urn:adsk.wipprod:dm.lineage:ChildKey"
    );
    assert_eq!(
        child.version.as_ref().unwrap().as_str(),
        "urn:adsk.wipprod:fs.file:vf.ChildKey?version=7"
    );
    let Storage::File { document, .. } = &definitions
        .iter()
        .find(|row| row.is_root())
        .unwrap()
        .storage
    else {
        panic!("file root")
    };
    assert_eq!(document.path.as_ref().unwrap().as_str(), "root.f3d");
    assert!(document.document_id.is_none());
    assert!(document.version.is_none());
    assert_eq!(decoded.ir().model.occurrences.len(), 1);
    assert_eq!(f3d_native(decoded.ir()).xref_references.len(), 1);
    assert!(f3d_native(decoded.ir()).xref_designs.is_empty());
}

#[test]
fn api_export_empty_reference_extension_has_no_external_documents() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(api_export_with_placements(false, None)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .all(|row| !matches!(row.storage, Storage::External(_))));
    assert!(f3d_native(decoded.ir()).xref_references.is_empty());
}

#[test]
fn api_export_manifest_terminal_lineage_identifies_root() {
    const ROOT_LINEAGE: &str = "urn:adsk.wipprod:dm.lineage:RootKey";
    for flag in [0, 1] {
        for linked in [false, true] {
            let bytes = api_export_with_placements(linked, Some((flag, ROOT_LINEAGE)));
            let decoded = F3dCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let definitions = &decoded.ir().model.product_definitions;
            let root = definitions.iter().find(|row| row.is_root()).unwrap();
            let Storage::File { document, .. } = &root.storage else {
                panic!("file root")
            };
            assert_eq!(
                document.document_id.as_ref().map(NonBlankString::as_str),
                Some(ROOT_LINEAGE)
            );
            assert!(document.version.is_none());
            if linked {
                let child = definitions
                    .iter()
                    .find_map(|row| match &row.storage {
                        Storage::External(document) => Some(document),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    child.document_id.as_ref().map(NonBlankString::as_str),
                    Some("urn:adsk.wipprod:dm.lineage:ChildKey")
                );
            }
        }
    }
}

#[test]
fn api_export_terminal_lineage_wins_over_design_zero_and_charges_loss() {
    const TERMINAL: &str = "urn:adsk.wipprod:dm.lineage:TerminalKey";
    let bytes = api_export_build(false, Some((1, TERMINAL)), None, true, "child.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let root = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|row| row.is_root())
        .unwrap();
    let Storage::File { document, .. } = &root.storage else {
        panic!("file root")
    };
    assert_eq!(
        document.document_id.as_ref().map(NonBlankString::as_str),
        Some(TERMINAL)
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.local_code() == "xref.lineage-disagrees"
            && loss.message.contains(TERMINAL)
            && loss.message.contains("urn:adsk.wipprod:dm.lineage:RootKey")
    }));
}

#[test]
fn api_export_without_terminal_takes_design_zero_lineage() {
    let bytes = api_export_build(false, None, None, true, "child.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let root = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|row| row.is_root())
        .unwrap();
    let Storage::File { document, .. } = &root.storage else {
        panic!("file root")
    };
    assert_identity(document, "RootKey");
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "xref.lineage-disagrees"));
}

#[test]
fn api_export_manifest_save_markers_keep_root_lineage_unassigned() {
    for flag in [0, 1] {
        for marker in ["NA_EXPORT", "NA_OFFLINESAVE"] {
            let bytes = api_export_with_placements(true, Some((flag, marker)));
            let decoded = F3dCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let root = decoded
                .ir()
                .model
                .product_definitions
                .iter()
                .find(|row| row.is_root())
                .unwrap();
            let Storage::File { document, .. } = &root.storage else {
                panic!("file root")
            };
            assert!(document.document_id.is_none());
            assert!(document.version.is_none());
        }
    }
}

#[test]
fn api_export_blank_document_identity_keeps_placements_and_occurrences() {
    for field in ["LineageUrn", "URL"] {
        let bytes = api_export_with_identity_fields(true, None, Some(field));
        let decoded = F3dCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.occurrences.len(), 1);
        assert_eq!(f3d_native(decoded.ir()).xref_references.len(), 1);
        let child = decoded
            .ir()
            .model
            .product_definitions
            .iter()
            .find_map(|row| match &row.storage {
                Storage::External(document) => Some(document),
                _ => None,
            })
            .unwrap();
        assert_eq!(child.document_id.is_none(), field == "LineageUrn");
        assert_eq!(child.version.is_none(), field == "URL");
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "xref.identity-blank"
                && loss.message.contains(field)));
    }
}

#[test]
fn f3z_conflicting_description_identity_keeps_member_geometry_and_identity() {
    let targets = [
        ("child.f3d", XREF_ROLE),
        ("missing.f3d", "66666666-7777-4888-8999-aaaaaaaaaaaa"),
    ];
    let mut table: serde_json::Value =
        serde_json::from_str(&redirections_json("root.f3d", &targets)).unwrap();
    table["designs"][2]["lineageUrn"] = "".into();
    table["designs"][2]["versionUrn"] = "".into();
    let root = crate::test_support::assembly_test::f3d_with_reference_table_and_identity_xrefs(
        "assembly-design",
        "root.f3d",
        &targets,
        &serde_json::to_vec(&table).unwrap(),
    );
    let child = include_bytes!("../../tests/golden/fixtures/generated_unit_cube.f3d");
    let description = serde_json::json!({"designDescription":{"designGraphs":[{"designObjects":[
        {"relativePath":"root.f3d", "lineage":"urn:first", "about":"urn:version:first"},
        {"relativePath":"root.f3d", "lineage":"urn:second", "about":"urn:version:second"},
        {"relativePath":"missing.f3d", "lineage":"", "about":""},
        {"relativePath":"missing.f3d", "lineage":"urn:present", "about":"urn:version:present"}
    ]}]}});
    let bytes = f3z_archive_with_design_description(
        "root.f3d",
        &[("root.f3d", &root), ("child.f3d", child)],
        &serde_json::to_vec(&description).unwrap(),
    );
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(bytes),
        &DecodeOptions::default(),
    )
    .unwrap();
    let root = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|row| row.is_root())
        .unwrap();
    let Storage::File { document, .. } = &root.storage else {
        panic!("file root")
    };
    assert_identity(document, "RootKey");
    let missing = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .find_map(|row| match &row.storage {
            Storage::External(document)
                if document.path.as_ref().map(NonBlankString::as_str) == Some("missing.f3d") =>
            {
                Some(document)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        missing.document_id.as_ref().unwrap().as_str(),
        "urn:present"
    );
    assert_eq!(
        missing.version.as_ref().unwrap().as_str(),
        "urn:version:present"
    );
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().geometry.faces.len(), 6);
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "xref.archive-identity-disagrees")
            .count(),
        2
    );
}

const CHILD_LINEAGE: &str = "urn:adsk.wipprod:dm.lineage:ChildKey";
const CHILD_VERSION: &str = "urn:adsk.wipprod:fs.file:vf.ChildKey?version=7";

/// A cache-path-named reference of an F3Z root without `RedirectionsStream.dat`,
/// decoded against a description that states `objects`.
fn decode_lineage_bundle(objects: &[(&str, &str, &str)]) -> cadmpeg_ir::codec::DecodeResult {
    let root = api_export_build(true, None, None, false, "cache/original-child.f3d");
    let child = include_bytes!("../../tests/golden/fixtures/generated_unit_cube.f3d");
    let objects: Vec<_> = objects
        .iter()
        .map(|(path, lineage, version)| {
            serde_json::json!({"relativePath": path, "lineage": lineage, "about": version})
        })
        .collect();
    let description =
        serde_json::json!({"designDescription":{"designGraphs":[{"designObjects":objects}]}});
    let bytes = f3z_archive_with_design_description(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("child.f3d", child),
            ("other.f3d", child),
        ],
        &serde_json::to_vec(&description).unwrap(),
    );
    crate::test_support::archive_session::decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn loss_count(decoded: &cadmpeg_ir::codec::DecodeResult, code: &str) -> usize {
    decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == code)
        .count()
}

#[test]
fn f3z_reference_without_redirections_resolves_by_lineage_and_version() {
    let decoded = decode_lineage_bundle(&[
        ("root.f3d", "urn:lineage:root", "urn:version:root"),
        ("child.f3d", CHILD_LINEAGE, CHILD_VERSION),
    ]);
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().geometry.faces.len(), 6);
    assert_eq!(loss_count(&decoded, "xref.redirection-from-lineage"), 1);
    assert_eq!(loss_count(&decoded, "xref.member-missing"), 0);
    let references = &f3d_native(decoded.ir()).xref_references;
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].relative_path.as_str(), "child.f3d");
}

#[test]
fn f3z_reference_without_unique_lineage_match_keeps_the_path() {
    let cases: [&[(&str, &str, &str)]; 4] = [
        // No entry states the reference's lineage.
        &[("child.f3d", "urn:lineage:other", CHILD_VERSION)],
        // The lineage matches, the version differs.
        &[("child.f3d", CHILD_LINEAGE, "urn:version:other")],
        // Two members state the same pair.
        &[
            ("child.f3d", CHILD_LINEAGE, CHILD_VERSION),
            ("other.f3d", CHILD_LINEAGE, CHILD_VERSION),
        ],
        // The one match names no archive member.
        &[("absent.f3d", CHILD_LINEAGE, CHILD_VERSION)],
    ];
    for objects in cases {
        let decoded = decode_lineage_bundle(objects);
        assert_eq!(loss_count(&decoded, "xref.redirection-from-lineage"), 0);
        assert_eq!(loss_count(&decoded, "xref.member-missing"), 1);
        assert_eq!(decoded.ir().geometry.bodies.len(), 0);
        let references = &f3d_native(decoded.ir()).xref_references;
        assert_eq!(
            references[0].relative_path.as_str(),
            "cache/original-child.f3d"
        );
    }
}
