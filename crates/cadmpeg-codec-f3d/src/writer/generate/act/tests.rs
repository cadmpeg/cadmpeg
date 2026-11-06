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
        original.untyped_records, 0,
        "the unindexed decoy is outside the ACT records"
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
        act_component_links: original
            .component_links
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
    let root = &mut native.act_component_links[0];
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
    let mut displaced = native.clone();
    let (prefix, _) = displaced.act_tables[0].id.rsplit_once('#').unwrap();
    displaced.act_tables[0].id = format!("{prefix}#1024");
    let bounded_arena = cadmpeg_core::decode::DecodeArena::new();
    let mut bounded_policy = cadmpeg_core::decode::DecodePolicy::default();
    bounded_policy.limits.max_retained_bytes = 1;
    let bounded = cadmpeg_core::decode::DecodeContext::for_encode(&bounded_arena, &bounded_policy);
    assert!(
        matches!(encode(&displaced, &bounded), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
    let count = u64::try_from(
        native.act_tables.len() + native.act_component_links.len() + native.act_entities.len(),
    )
    .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_entities = count - 1;
    let limited = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    assert!(
        matches!(encode(&native, &limited), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
        && limit.limit == count - 1)
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_entities = count;
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let segments = encode(&native, &ctx).unwrap();
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
    assert_eq!(output.tables[0].id, native.act_tables[0].id);
    assert_eq!(
        serde_json::to_value(&output.component_links[0]).unwrap()["byte_offset"],
        serde_json::to_value(&native.act_component_links[0]).unwrap()["byte_offset"]
    );
    assert_eq!(
        output.component_links[0].layout().display_name(),
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
