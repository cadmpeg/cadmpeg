// SPDX-License-Identifier: Apache-2.0
//! Thread sizing validation against the native definition.

use super::*;
use crate::test_support::thread::definition_bytes;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn thread_validation_accepts_metric_and_inch_coarse_and_fine_and_rejects_forged_sizes() -> TestResult
{
    for (version, nominal, designation, standard, form, pitch, diameter_mm, pitch_mm) in [
        (
            19,
            "6",
            "M6x1",
            "ANSI Metric M Profile",
            0x00ca,
            "1",
            6.0,
            1.0,
        ),
        (
            19,
            "6",
            "M6x1",
            "ANSI Metric M Profile",
            0x10ca,
            "1",
            6.0,
            1.0,
        ),
        (
            19,
            "6",
            "M6x0.75",
            "ANSI Metric M Profile",
            0x10ce,
            "0.75",
            6.0,
            0.75,
        ),
        (
            19,
            "6",
            "M6x1",
            "ANSI Metric M Profile",
            0x10ea,
            "1",
            6.0,
            1.0,
        ),
        (19, "6", "M6x1", "ISO Metric profile", 0x00ca, "1", 6.0, 1.0),
        (
            19,
            "6",
            "M6x0.75",
            "ANSI Metric M Profile",
            0x10ee,
            "0.75",
            6.0,
            0.75,
        ),
        (
            19,
            "6",
            "M6x0.75",
            "ISO Metric profile",
            0x10ee,
            "0.75",
            6.0,
            0.75,
        ),
        (
            19,
            "0.25",
            "1/4-20 UNC",
            "ANSI Unified Screw Threads",
            0x18f2,
            "20",
            6.35,
            1.27,
        ),
        (
            19,
            "0.25",
            "1/4-28 UNF",
            "ANSI Unified Screw Threads",
            0x18f2,
            "28",
            6.35,
            25.4 / 28.0,
        ),
        (
            19,
            "1",
            "1-8 UNC",
            "ANSI Unified Screw Threads",
            0x18f2,
            "8",
            25.4,
            3.175,
        ),
        (
            21,
            "6",
            "M6x1",
            "ANSI Metric M Profile",
            0x10ea,
            "1",
            6.0,
            1.0,
        ),
        (
            21,
            "6",
            "M6x0.75",
            "ANSI Metric M Profile",
            0x10ee,
            "0.75",
            6.0,
            0.75,
        ),
    ] {
        let bytes = definition_bytes(5, 9, nominal, designation, standard, form, pitch);
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())?;
        let mut data = thread_data()?;
        let mut raw = serde_json::to_value(&data.pm_dc_features[0])?;
        raw["save_version_major"] = serde_json::json!(version);
        data.pm_dc_features[0] = serde_json::from_value(raw)?;
        let mut label = serde_json::to_value(&data.pm_dc_feature_labels[0])?;
        label["save_version_major"] = serde_json::json!(version);
        label["header"]["owner"]["qualified"] = serde_json::json!(true);
        data.pm_dc_feature_labels[0] = serde_json::from_value(label)?;
        data.pm_dc_thread_definitions
            .push(crate::record_identity::Located::new(
                crate::feature::thread::parse(&ctx, view, version)?,
                crate::record_identity::RecordTypeId::from_bytes(crate::feature::thread::TYPE),
                cadmpeg_ir::identity_key!("generated"),
                9,
            ));
        data.records.push(serde_json::from_value(serde_json::json!({
            "id": "inventor:rse:record#generated-9", "token": "generated", "ordinal": 9,
            "selector": 0, "type_index": 0, "type_id": crate::pmdc::type_id_string(crate::feature::thread::TYPE),
            "payload_offset": 0, "payload_len": 0, "payload_sha256": "0".repeat(64),
            "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": "0".repeat(64)
        }))?);
        let mut feature = serde_json::to_value(annotation(data.pm_dc_features[0].id()))?;
        feature["definition"]["diameter"] = serde_json::json!(diameter_mm);
        feature["definition"]["pitch"] = serde_json::json!(pitch_mm);
        feature["definition"]["standard"] = serde_json::json!(standard);
        feature["definition"]["designation"] = serde_json::json!(designation);
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model
            .features
            .push(serde_json::from_value(feature.clone())?);
        let mut findings = Vec::new();
        crate::validate::validate_features(&ctx, &ir, &data, &mut findings)?;
        assert!(findings.is_empty(), "{findings:?}");
        for (field, invalid) in [
            ("diameter", serde_json::json!(diameter_mm * 2.0)),
            ("pitch", serde_json::json!(pitch_mm * 2.0)),
            ("standard", serde_json::json!("other")),
            ("designation", serde_json::json!("other")),
            ("modeled", serde_json::json!(true)),
            ("diameter", serde_json::Value::Null),
        ] {
            let mut invalid_feature = feature.clone();
            if invalid.is_null() {
                invalid_feature["definition"]
                    .as_object_mut()
                    .ok_or("missing definition object")?
                    .remove(field);
            } else {
                invalid_feature["definition"][field] = invalid;
            }
            ir.model.features[0] = serde_json::from_value(invalid_feature)?;
            findings.clear();
            crate::validate::validate_features(&ctx, &ir, &data, &mut findings)?;
            assert_eq!(findings.len(), 1, "{field}: {findings:?}");
        }
        ir.model.features[0] = serde_json::from_value(feature.clone())?;
        let mut wire = serde_json::to_value(&data.pm_dc_thread_definitions[0])?;
        wire["controls"][0] = serde_json::json!(1);
        data.pm_dc_thread_definitions[0] = serde_json::from_value(wire)?;
        findings.clear();
        crate::validate::validate_features(&ctx, &ir, &data, &mut findings)?;
        assert_eq!(findings.len(), 1);
        let fallback = feature["definition"]
            .as_object_mut()
            .ok_or("missing definition")?;
        fallback.remove("diameter");
        fallback.remove("pitch");
        ir.model.features[0] = serde_json::from_value(feature)?;
        findings.clear();
        crate::validate::validate_features(&ctx, &ir, &data, &mut findings)?;
        assert!(findings.is_empty(), "{findings:?}");
    }
    Ok(())
}
