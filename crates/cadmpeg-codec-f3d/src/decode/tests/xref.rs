// SPDX-License-Identifier: Apache-2.0
//! Decode-owner external-reference projection tests.

use super::super::report_xref_placement_overrides;
use crate::loss::F3dLossCode;
use cadmpeg_ir::codec::Codec as _;
use std::io::Write as _;

fn placement_table() -> crate::xref::XrefTable {
    crate::xref::XrefTable {
        designs: Vec::new(),
        references: vec![crate::records::xref::XrefReference {
            current_pose: None,
            operation_pose: None,
            contained_poses: Vec::new(),
            owned_poses: Vec::new(),
            id: "f3d:xref:reference#4-occurrence-0".into(),
            ordinal: 4,
            occurrence_ordinal: 0,
            placement_source: None,
            from: "root.f3d".to_owned().try_into().unwrap(),
            relative_path: "part.f3d".to_owned().try_into().unwrap(),
            neutron_role: "role-guid".to_owned().try_into().unwrap(),
            neutron_data: "data-guid".into(),
            transform: None,
        }],
        placement_failures: Vec::new(),
        placement_metadata_missing: false,
        placement_overrides: vec![crate::xref::PlacementOverride {
            ordinal: 4,
            count: 2,
        }],
        ..Default::default()
    }
}

#[test]
fn superseded_xref_placements_have_a_distinct_loss_note() {
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    let table = placement_table();

    report_xref_placement_overrides(&mut report, &table);

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefPlacementSuperseded.kind())
        .expect("superseded placement loss");
    assert_eq!(loss.message, "2 structured placement record(s) for external occurrence part.f3d and role role-guid were superseded by scope-bound Component Insert carrier(s)");
}

#[test]
fn missing_placement_metadata_has_a_distinct_loss_note() {
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    let mut table = placement_table();
    table.placement_metadata_missing = true;
    super::super::report_xref_placement_failures(&mut report, &table);
    assert_eq!(report.losses.len(), 1);
    assert_eq!(
        report.losses[0].code,
        F3dLossCode::XrefPlacementMetadataMissing.kind()
    );
}

#[test]
fn assembly_property_note_preserves_role_and_data_text() {
    let table = placement_table();
    assert_eq!(
        super::super::XrefPropertyNote(&table.references[0]).to_string(),
        "neutronRole role-guid, neutronData data-guid"
    );
}

/// A failed table without `RedirectionsStream.dat` or a manifest reference
/// extension fails in `ComponentReferenceData.json`, whose entry is given up.
#[test]
fn an_invalid_component_reference_table_without_redirections_gives_up_its_own_entry() {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    let properties = r#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    zip.write_all(&u32::try_from(properties.len()).unwrap().to_le_bytes())
        .unwrap();
    zip.write_all(properties.as_bytes()).unwrap();
    zip.start_file("ComponentReferenceData.json", stored)
        .unwrap();
    zip.write_all(b"[]").unwrap();
    let document = zip.finish().unwrap().into_inner();
    let decoded = crate::F3dCodec
        .decode(
            &mut std::io::Cursor::new(document),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .expect("an invalid reference table remains a reported decode");
    let losses = &decoded.report().losses;
    let given_up = losses
        .iter()
        .filter(|loss| crate::test_support::xref_table_given_up(loss))
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 1, "{losses:?}");
    assert_eq!(
        given_up[0].code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(
        given_up[0].message.starts_with(&format!(
            "part {} \"ComponentReferenceData.json\" ",
            crate::container::ENTRY_PART
        )),
        "{losses:?}"
    );
    assert!(
        !losses
            .iter()
            .any(|loss| loss.message.contains("RedirectionsStream.dat")),
        "{losses:?}"
    );
}
