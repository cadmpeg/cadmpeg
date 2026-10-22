// SPDX-License-Identifier: Apache-2.0
//! Whole-arena source facts, strict wire admission, and composition lifecycle.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use serde_json::{json, Value};

use super::FreecadSourceLedgers;
use crate::{provenance::CodecFormat, SourceFidelity};

fn wire() -> Value {
    json!({
        "physical_ledger":[
            {"id":"fcstd:native:span#1","start":4,"end":8,"role":"compressed-payload","entry":"模型.bin"},
            {"id":"fcstd:native:span#0","start":0,"end":4,"role":"local-signature","entry":"模型.bin"}
        ],
        "logical_ledger":[
            {"id":"fcstd:native:logical-span#0","entry":"模型.bin","start":0,"end":3,"classification":"named_opaque","owner":"fcstd:native:property#Pad:Payload"}
        ],
        "byte_coverage":{
            "id":"fcstd:native:coverage#0","physical_byte_len":8,"physical_span_count":2,
            "logical_entry_count":1,"logical_byte_len":3,"logical_span_count":1,
            "classification_bytes":{"named_opaque":3},"named_opaque_entries":["模型.bin"],"exact":true
        }
    })
}

#[test]
fn complete_original_facts_and_record_order_survive_without_source_images_or_reads() {
    let original = wire();
    let ledgers: FreecadSourceLedgers = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(&ledgers).unwrap(), original);
    let mut fidelity = SourceFidelity::default();
    fidelity
        .insert_freecad_ledgers("file".into(), ledgers)
        .unwrap();
    assert!(fidelity.retained_records().is_empty());
    assert!(fidelity.record_layout().is_empty());
    assert!(fidelity.parts().is_empty());
    assert!(fidelity.unrepresented().is_empty());
    assert_eq!(
        serde_json::to_value(&fidelity).unwrap()["freecad_ledgers"]["file"],
        original
    );
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).unwrap()).unwrap();
    assert_eq!(restored, fidelity);
    let (_, _, _, _, _, _, actual) = restored.into_parts();
    assert_eq!(serde_json::to_value(&actual["file"]).unwrap(), original);
}

#[test]
fn every_closed_ledger_object_rejects_an_unknown_field() {
    for path in ["root", "physical", "logical", "coverage"] {
        let mut invalid = wire();
        let node = match path {
            "root" => &mut invalid,
            "physical" => &mut invalid["physical_ledger"][0],
            "logical" => &mut invalid["logical_ledger"][0],
            "coverage" => &mut invalid["byte_coverage"],
            _ => unreachable!(),
        };
        node["invented"] = json!(true);
        assert!(
            serde_json::from_value::<FreecadSourceLedgers>(invalid).is_err(),
            "{path}"
        );
    }
    let mut invalid = wire();
    invalid["physical_ledger"][0]["end"] = json!(4);
    assert!(serde_json::from_value::<FreecadSourceLedgers>(invalid).is_err());
    let mut invalid = wire();
    invalid["logical_ledger"][0]["owner"] = Value::Null;
    assert!(serde_json::from_value::<FreecadSourceLedgers>(invalid).is_err());
}

#[test]
fn duplicate_stream_keys_and_classification_keys_are_rejected() {
    let ledger = wire().to_string();
    let duplicate = format!(r#"{{"freecad_ledgers":{{"file":{ledger},"file":{ledger}}}}}"#);
    assert!(serde_json::from_str::<SourceFidelity>(&duplicate).is_err());
    let duplicate = ledger.replace(
        r#""named_opaque":3"#,
        r#""named_opaque":3,"named_opaque":3"#,
    );
    assert!(serde_json::from_str::<FreecadSourceLedgers>(&duplicate).is_err());
    let invalid = format!(r#"{{"freecad_ledgers":{{"":{ledger}}}}}"#);
    assert!(serde_json::from_str::<SourceFidelity>(&invalid).is_err());
}

#[test]
fn insertion_and_append_collisions_leave_all_existing_fidelity_unchanged() {
    let mut fidelity = SourceFidelity::default();
    let ledgers: FreecadSourceLedgers = serde_json::from_value(wire()).unwrap();
    fidelity
        .insert_freecad_ledgers("file".into(), ledgers.clone())
        .unwrap();
    let original = fidelity.clone();
    assert!(fidelity
        .insert_freecad_ledgers("file".into(), ledgers.clone())
        .is_err());
    assert_eq!(fidelity, original);
    assert!(fidelity
        .insert_freecad_ledgers(String::new(), ledgers.clone())
        .is_err());
    assert_eq!(fidelity, original);
    let mut other = SourceFidelity::default();
    other
        .insert_freecad_ledgers("another".into(), ledgers.clone())
        .unwrap();
    other
        .insert_freecad_ledgers("file".into(), ledgers)
        .unwrap();
    assert!(fidelity.append(other).is_err());
    assert_eq!(fidelity, original);
}

#[test]
fn part_tree_rescoping_leaves_whole_source_ledger_facts_for_the_composition_owner() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(b"12345678", &arena, &policy).unwrap();
    let actual = ctx.capture_part_extent(root).unwrap();
    let mut fidelity = SourceFidelity::default();
    let ledgers: FreecadSourceLedgers = serde_json::from_value(wire()).unwrap();
    fidelity
        .insert_freecad_ledgers(actual.stream().into(), ledgers)
        .unwrap();
    fidelity
        .rescope_part_tree(&ctx, CodecFormat::Fcstd, "host/0/")
        .unwrap();
    assert_eq!(fidelity.freecad_ledgers().len(), 1);
    assert_eq!(
        serde_json::to_value(&fidelity.freecad_ledgers()["file"]).unwrap(),
        wire()
    );
    assert!(fidelity.retained_records().is_empty());
    assert!(fidelity.record_layout().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn ownerless_ledger_rows_require_the_explicit_null_written_by_their_serializers() {
    let physical =
        json!({"id":"fcstd:native:span#end","start":0,"end":22,"role":"end-record","entry":null});
    let row: super::ArchiveSpan = serde_json::from_value(physical.clone()).unwrap();
    assert_eq!(serde_json::to_value(row).unwrap(), physical);
    let mut missing = physical;
    missing.as_object_mut().unwrap().remove("entry");
    assert!(serde_json::from_value::<super::ArchiveSpan>(missing).is_err());

    let logical = json!({"id":"fcstd:native:logical-span#framing","entry":"Document.xml","start":0,"end":1,"classification":"structural","owner":null});
    let row: super::LogicalSpan = serde_json::from_value(logical.clone()).unwrap();
    assert_eq!(serde_json::to_value(row).unwrap(), logical);
    let mut missing = logical;
    missing.as_object_mut().unwrap().remove("owner");
    assert!(serde_json::from_value::<super::LogicalSpan>(missing).is_err());
}

#[test]
fn entry_owned_and_typed_ledger_rows_reject_null_owners_and_missing_keys() {
    let mut physical = wire()["physical_ledger"][0].clone();
    physical["entry"] = Value::Null;
    assert!(serde_json::from_value::<super::ArchiveSpan>(physical.clone()).is_err());
    physical.as_object_mut().unwrap().remove("entry");
    assert!(serde_json::from_value::<super::ArchiveSpan>(physical).is_err());

    for classification in ["typed", "named_opaque"] {
        let mut logical = wire()["logical_ledger"][0].clone();
        logical["classification"] = json!(classification);
        logical["owner"] = Value::Null;
        assert!(serde_json::from_value::<super::LogicalSpan>(logical.clone()).is_err());
        logical.as_object_mut().unwrap().remove("owner");
        assert!(serde_json::from_value::<super::LogicalSpan>(logical).is_err());
    }
}
