// SPDX-License-Identifier: Apache-2.0
//! History records synthesized from neutral features re-decode through the
//! table grammar; values without structured content stay omitted.

use super::records;
use crate::container;
use crate::test_support::test_dump;
use crate::writer::tests::table;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
};
use cadmpeg_ir::{CadIr, NativeRecord};
use std::collections::BTreeMap;

fn history_feature() -> Feature {
    let properties: &[(&str, &str)] = &[
        ("record_version", "20180320"),
        ("record_type", "history_parameters"),
        ("copy_on_replace", "false"),
        (
            "antecedent_objects",
            "43cff9a6-c650-440c-af75-addf5585d7a0,e06de8c1-f86e-4f2f-a4d8-06d491888fdc",
        ),
        ("descendant_objects", "07c90e28-cc28-4108-9408-df5a968f4114"),
        ("value_1.count", "2"),
        ("value_1.0.component", "0,-1"),
        ("value_1.0.evaluation.component", "0,-1"),
        (
            "value_1.0.evaluation.interval_0",
            "0.06297731394623449,32.3432405472889",
        ),
        ("value_1.0.evaluation.interval_1", "1.0,2.0"),
        ("value_1.0.evaluation.interval_2", "3.0,4.0"),
        ("value_1.0.evaluation.parameters", "5.0,6.0,7.0,8.0"),
        ("value_1.0.evaluation.type", "11"),
        ("value_1.0.geometry_type", "4"),
        ("value_1.0.instance_count", "1"),
        (
            "value_1.0.instance_0.definition_id",
            "11111111-2222-3333-4444-555555555555",
        ),
        ("value_1.0.instance_0.component", "3,4"),
        ("value_1.0.instance_0.evaluation.component", "1,2"),
        ("value_1.0.instance_0.evaluation.interval_0", "0.0,1.0"),
        ("value_1.0.instance_0.evaluation.interval_1", "2.0,3.0"),
        ("value_1.0.instance_0.evaluation.interval_2", "4.0,5.0"),
        (
            "value_1.0.instance_0.evaluation.parameters",
            "9.0,10.0,11.0,12.0",
        ),
        ("value_1.0.instance_0.evaluation.type", "13"),
        ("value_1.0.instance_0.geometry_index", "7"),
        (
            "value_1.0.instance_0.reference_id",
            "22222222-3333-4444-5555-666666666666",
        ),
        (
            "value_1.0.instance_0.transform",
            "1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1",
        ),
        (
            "value_1.0.object_id",
            "43cff9a6-c650-440c-af75-addf5585d7a0",
        ),
        ("value_1.0.osnap_mode", "0"),
        ("value_1.0.point", "1.5,2.5,3.5"),
        ("value_1.1.component", "-1,4"),
        ("value_1.1.evaluation.component", "5,6"),
        ("value_1.1.evaluation.parameters", "13.0,14.0,15.0,16.0"),
        ("value_1.1.evaluation.type", "17"),
        ("value_1.1.geometry_type", "1"),
        ("value_1.1.instance_count", "0"),
        (
            "value_1.1.object_id",
            "e06de8c1-f86e-4f2f-a4d8-06d491888fdc",
        ),
        ("value_1.1.osnap_mode", "0"),
        ("value_1.1.point", "4.5,5.5,6.5"),
        ("value_2.count", "1"),
        ("value_2.0.component", "0,-1"),
        ("value_2.0.evaluation.component", "0,-1"),
        ("value_2.0.evaluation.interval_0", "0,191.41592653589794"),
        ("value_2.0.evaluation.interval_1", "1.0,2.0"),
        ("value_2.0.evaluation.interval_2", "3.0,4.0"),
        ("value_2.0.evaluation.parameters", "20.0,21.0,22.0,23.0"),
        ("value_2.0.evaluation.type", "24"),
        ("value_2.0.geometry_type", "2"),
        ("value_2.0.instance_count", "0"),
        (
            "value_2.0.object_id",
            "8c1e5fda-fc2b-4026-8954-b6cb6dcbd4a5",
        ),
        ("value_2.0.osnap_mode", "3"),
        ("value_2.0.point", "7.5,8.5,9.5"),
    ];
    let mut source_properties = BTreeMap::new();
    for (key, value) in properties {
        source_properties.insert(
            cadmpeg_core::text::NonBlankString::new(*key).expect("property name"),
            (*value).to_owned(),
        );
    }
    Feature {
        product_definition: None,
        id: FeatureId::mint("rhino:history:feature#test".to_owned()).expect("feature identity"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        needs_recompute: false,
        dependencies: Default::default(),
        source_properties,
        source_tag: Some("HistoryRecord".to_owned()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "06d2b948-7388-4634-9f7b-6bd6543d0b44".to_owned().into(),
                parameters: Default::default(),
            }),
            Default::default(),
        ),
        native_ref: Some("rhino:history:record#4a9c4974-0863-40a8-84c7-4f27609c187c".to_owned()),
    }
}

fn held_record() -> NativeRecord {
    NativeRecord::new(
        "rhino:history:record#4a9c4974-0863-40a8-84c7-4f27609c187c"
            .try_into()
            .expect("history identity"),
        serde_json::json!({
            "source_uuid": "4a9c4974-0863-40a8-84c7-4f27609c187c",
            "command_uuid": "06d2b948-7388-4634-9f7b-6bd6543d0b44",
            "record_version": 20180320,
            "record_type": "history_parameters",
            "copy_on_replace": false,
            "antecedent_object_uuids": ["43cff9a6-c650-440c-af75-addf5585d7a0", "e06de8c1-f86e-4f2f-a4d8-06d491888fdc"],
            "descendant_object_uuids": ["07c90e28-cc28-4108-9408-df5a968f4114"],
            "value_count": 2,
        })
        .as_object()
        .expect("history fields")
        .clone(),
    )
    .expect("history record")
}

fn document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.features.push(history_feature());
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("history_records".to_owned(), vec![held_record()]);
    ir
}

fn scan_history(records: &[Vec<u8>]) -> Vec<crate::history::HistoryRecord> {
    let archive = test_dump::minimal_document(
        "80",
        &[
            table(crate::writer::TCODE_SETTINGS_TABLE, &[]).expect("settings"),
            table(crate::writer::TCODE_HISTORY_RECORD_TABLE, records).expect("history"),
            table(crate::writer::TCODE_OBJECT_TABLE, &[]).expect("objects"),
        ],
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _view) = DecodeContext::from_root_bytes(&archive, &arena, &policy).expect("context");
    let scan = container::scan(&ctx, &archive).expect("scan history archive");
    assert_eq!(scan.history.len(), 1, "scan parses one history record");
    scan.history
}

#[test]
fn history_feature_with_object_references_round_trips() {
    let source = document();
    let exported = records(&source).expect("typed history");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.records.len(), 1);
    assert!(exported
        .written
        .contains("rhino:history:record#4a9c4974-0863-40a8-84c7-4f27609c187c"));
    // The table wrapper is production framing; the scan below parses it.
    let parsed = scan_history(&exported.records);
    assert_eq!(parsed.len(), 1);
    let record = &parsed[0];
    assert_eq!(record.version, 20180320);
    assert_eq!(
        record.command_id.to_string(),
        "06d2b948-7388-4634-9f7b-6bd6543d0b44"
    );
    assert_eq!(
        record
            .descendants
            .iter()
            .map(|uuid| uuid.to_string())
            .collect::<Vec<_>>(),
        ["07c90e28-cc28-4108-9408-df5a968f4114"]
    );
    assert_eq!(
        record
            .antecedents
            .iter()
            .map(|uuid| uuid.to_string())
            .collect::<Vec<_>>(),
        [
            "43cff9a6-c650-440c-af75-addf5585d7a0",
            "e06de8c1-f86e-4f2f-a4d8-06d491888fdc"
        ]
    );
    assert!(!record.copy_on_replace);
    assert!(matches!(
        record.record_type,
        crate::history::RecordType::HistoryParameters
    ));
    assert_eq!(record.values.len(), 2);
    assert_eq!(record.values[0].id, 1);
    assert_eq!(record.values[1].id, 2);
    let mut counts = Vec::new();
    for value in &record.values {
        match &value.value {
            crate::history::Value::ObjectReferences(references) => {
                counts.push(references.len());
            }
            _ => panic!("history value is not object references"),
        }
    }
    assert_eq!(counts, [2, 1]);
}

#[test]
fn archive_writes_each_history_record_once() {
    let source = document();
    let history = records(&source).expect("history records").records;
    assert_eq!(history.len(), 1);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let session = cadmpeg_core::encode::EncodeSession::from_context(&ctx).expect("session");
    let (archive, _) =
        crate::writer::write(&ctx, &source, crate::RhinoArchiveVersion::V8, &session)
            .expect("archive");
    let bytes = archive.into_vec();
    assert_eq!(
        bytes
            .windows(history[0].len())
            .filter(|window| *window == history[0])
            .count(),
        1
    );
    let (read_ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let scan = container::scan(&read_ctx, &bytes).expect("archive scan");
    assert_eq!(scan.history.len(), 1);
    assert!(scan.objects.is_empty());
}

#[test]
fn history_feature_without_structured_values_stays_omitted() {
    let mut ir = document();
    // A value with no reconstructible marker (neither references,
    // poly-edge segments nor SubD chains) cannot be carried back.
    ir.model.features[0].source_properties.insert(
        cadmpeg_core::text::NonBlankString::new("value_9.note").expect("property name"),
        "a scalar with no type tag".to_owned(),
    );
    ir.model.features[0].source_properties.retain(|key, _| {
        !key.as_str().starts_with("value_1") && !key.as_str().starts_with("value_2")
    });
    let exported = records(&ir).expect("typed history");
    assert!(exported.records.is_empty());
    assert_eq!(exported.losses.len(), 1);
    assert!(exported.losses[0]
        .message
        .contains("has no reconstructible content"));
}
