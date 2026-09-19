// SPDX-License-Identifier: Apache-2.0
use crate::report::export::{CensusBasis, EntityCensus, ExportReport};
use cadmpeg_core::dialect::DialectId;
use std::collections::BTreeMap;

#[test]
fn a_native_export_report_states_its_payload_and_names_its_target() {
    let report = ExportReport::native(
        cadmpeg_core::dialect_id!("step:ap242-e3"),
        EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts: BTreeMap::new(),
        },
        crate::report::export::WritePath::Synthesized {
            fidelity: crate::report::export::SynthesisFidelity::NotProvided {},
        },
        Vec::new(),
        Vec::new(),
    );

    assert_eq!(report.format(), "step");
    assert_eq!(
        report.target().map(DialectId::as_str),
        Some("step:ap242-e3")
    );
    let rendered = serde_json::to_value(&report).unwrap();
    assert_eq!(rendered["identity"]["payload"], "native");
    assert_eq!(rendered["identity"]["target"], "step:ap242-e3");
    assert!(rendered.get("format").is_none());
    assert_eq!(
        serde_json::from_value::<ExportReport>(rendered).unwrap(),
        report
    );
}

#[test]
fn an_export_payload_carries_only_the_target_key_its_own_arm_owns() {
    let native = serde_json::json!({
        "identity": {"payload": "native", "target": "step:ap242-e3"},
        "census": { "basis": "target_records", "counts": {} },
        "write_path": { "path": "synthesized", "fidelity": { "status": "not_provided" } },
        "losses": [],
        "notes": [],
    });
    serde_json::from_value::<ExportReport>(native.clone()).expect("a native export report");

    let mut without_target = native.clone();
    without_target["identity"]
        .as_object_mut()
        .expect("map")
        .remove("target");
    let error = serde_json::from_value::<ExportReport>(without_target)
        .expect_err("a native export report requires a target");
    assert!(error.to_string().contains("target"), "{error}");

    let mut cadir_with_target = native;
    cadir_with_target["identity"]["payload"] = serde_json::json!("cadir");
    cadir_with_target["census"]["basis"] = serde_json::json!("ir_arenas");
    let error = serde_json::from_value::<ExportReport>(cadir_with_target)
        .expect_err("CADIR has no native dialect target");
    assert!(error.to_string().contains("target"), "{error}");
}
