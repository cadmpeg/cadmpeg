// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn delete_face_validation_keeps_unresolved_controls_native() -> TestResult {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    for (marker, count, detail) in [
        (2, 4, "slots 1 and 2 Boolean construction roles are unresolved"),
        (2, 3, "property vector differs from the four-slot form"),
        (2, 5, "property vector differs from the four-slot form"),
        (8, 4, "property vector marker differs from type 2"),
    ] {
        let source = crate::feature::tests::test_feature(0, count, &[]);
        let mut wire = serde_json::to_value(&source)?;
        wire["properties"]["marker"] = serde_json::json!(marker);
        if marker == 8 {
            wire["properties"]["metadata"]["width"] = serde_json::json!("u16");
        }
        let source: crate::feature::PmDcFeature = serde_json::from_value(wire)?;
        let identity = &source.identity;
        let mut data = empty_data();
        data.records.push(serde_json::from_value(serde_json::json!({
            "id": format!("inventor:rse:record#{}-0", identity.segment_token),
            "token": identity.segment_token.as_str(), "ordinal": 0,
            "selector": 0, "type_index": 0, "type_id": identity.type_id.as_str(),
            "payload_offset": 0, "payload_len": 0, "payload_sha256": "0".repeat(64),
            "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": "0".repeat(64)
        }))?);
        let mut feature = annotation(source.id());
        data.pm_dc_features.push(source);
        let mut ir = cadmpeg_ir::CadIr::empty();
        for heal in [false, true] {
            feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::DeleteFace {
                    faces: FaceSelection::Native("synthetic-face-selection".into()),
                    heal,
                },
            ));
            ir.model.features = vec![feature.clone()];
            let mut findings = Vec::new();
            super::super::validate_features(&ctx, &ir, &data, &mut findings)?;
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(
                findings[0].message,
                format!("Inventor neutral DeleteFace refused: {detail}.")
            );
        }
        feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("DeleteFace".into()),
                parameters: std::collections::BTreeMap::new(),
            },
        ));
        ir.model.features = vec![feature];
        let mut findings = Vec::new();
        super::super::validate_features(&ctx, &ir, &data, &mut findings)?;
        assert!(findings.is_empty(), "{findings:?}");
    }
    Ok(())
}
