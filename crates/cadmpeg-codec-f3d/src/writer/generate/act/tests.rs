// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{Cursor, Write};

fn document_record<T: Serialize + DeserializeOwned>(record: T) -> T {
    serde_json::from_value(serde_json::to_value(record).unwrap()).unwrap()
}

#[test]
fn current_typed_act_records_reemit_without_source_frames_or_locations() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let original =
        crate::test_support::zip_test::with_scan(&source, |scan| crate::act::decode(scan).unwrap());
    assert_eq!(
        original.untyped_records, 1,
        "the synthetic decoy is source-only"
    );
    let mut native = F3dNative {
        act_tables: original.tables.into_iter().map(document_record).collect(),
        act_types: original.types.into_iter().map(document_record).collect(),
        act_entities: original.entities.into_iter().map(document_record).collect(),
        act_guids: original.guids.into_iter().map(document_record).collect(),
        act_registry_channels: original
            .registry_channels
            .into_iter()
            .map(document_record)
            .collect(),
        act_root_components: original
            .root_components
            .into_iter()
            .map(document_record)
            .collect(),
        act_table_references: original
            .table_references
            .into_iter()
            .map(document_record)
            .collect(),
        ..Default::default()
    };
    let root = &mut native.act_root_components[0];
    root.try_set_strings(
        root.layout().entity_id().to_owned(),
        "Current part label".into(),
    )
    .unwrap();
    native.act_entities[0]
        .set_channel_guid(
            "Appearance",
            "12345678-1234-5678-9abc-123456789abc"
                .to_owned()
                .try_into()
                .unwrap(),
        )
        .unwrap();
    let segments = encode(&native).unwrap();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, options);
    for segment in segments {
        archive
            .start_file(format!("{}/BulkStream.dat", segment.folder), options)
            .unwrap();
        archive.write_all(&segment.bulk).unwrap();
        archive
            .start_file(format!("{}/MetaStream.dat", segment.folder), options)
            .unwrap();
        archive.write_all(&segment.meta).unwrap();
    }
    let bytes = archive.finish().unwrap().into_inner();
    let output =
        crate::test_support::zip_test::with_scan(&bytes, |scan| crate::act::decode(scan).unwrap());
    assert_eq!(output.untyped_records, 0);
    assert_eq!(output.entities.len(), native.act_entities.len());
    assert_eq!(
        output.root_components[0].layout().display_name(),
        "Current part label"
    );
    assert_eq!(
        output.entities[0].channel_group().channels()["Appearance"]
            .value
            .as_str(),
        "12345678-1234-5678-9abc-123456789abc"
    );
    assert_eq!(output.guids[0].guid, native.act_guids[0].guid);
    assert_eq!(
        output.registry_channels.len(),
        native.act_registry_channels.len()
    );
    assert_eq!(
        output.table_references.len(),
        native.act_table_references.len()
    );
    let expected_types = native
        .act_types
        .iter()
        .map(|record| (&record.type_guid, record.version, &record.module))
        .collect::<Vec<_>>();
    let actual_types = output
        .types
        .iter()
        .map(|record| (&record.type_guid, record.version, &record.module))
        .collect::<Vec<_>>();
    assert_eq!(actual_types, expected_types);
}
