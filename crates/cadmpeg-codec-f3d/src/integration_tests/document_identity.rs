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
