// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureEvaluation, FeatureOperation};
use cadmpeg_ir::report::Severity;

use crate::{loss::InventorLossCode, InventorCodec};

#[test]
fn thicken_bytes_reach_the_admitted_partial_operation_and_native_refusal() {
    for slot_two in [true, false] {
        let bytes = crate::test_support::thicken::fixture(slot_two, true, 3);
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .expect("synthetic Thicken decode");
        let (ir, report, _) = decoded.into_parts();
        assert_eq!(ir.model.features.len(), 1);
        let feature = &ir.model.features[0];
        if slot_two {
            let FeatureDefinition::Operation(FeatureOperation::Thicken {
                thickness, side, ..
            }) = feature.evaluation.definition()
            else {
                panic!("admitted synthetic Thicken")
            };
            assert_eq!(thickness.expect("positive thickness").get(), 2.5);
            assert_eq!(*side, None);
            assert_eq!(ir.model.feature_result_topologies.len(), 1);
        } else {
            assert!(matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                    if kind.as_str() == "Thicken"
            ));
            assert!(ir.model.feature_result_topologies.is_empty());
            assert!(report.losses.iter().any(|loss| {
                loss.code == InventorLossCode::FeatureThickenConstructionNative.kind()
                    && loss.message == "Retained Inventor Thicken natively: slot 0 has unresolved construction operands."
            }));
        }
        assert!(report
            .losses
            .iter()
            .any(|loss| { loss.code == InventorLossCode::FeatureThickenOperandsNative.kind() }));
    }
}

#[test]
fn thicken_validation_checks_decoded_operands_and_retains_the_native_form() {
    for slot_two in [true, false] {
        let bytes = crate::test_support::thicken::fixture(slot_two, true, 3);
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .expect("synthetic Thicken decode");
        let (mut ir, _, _) = decoded.into_parts();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("synthetic validation context");
        let findings = InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("synthetic Thicken validation");
        assert!(
            findings
                .iter()
                .all(|finding| finding.severity < Severity::Error),
            "{findings:?}"
        );
        if slot_two {
            let mut definition = ir.model.features[0].evaluation.definition().clone();
            let FeatureDefinition::Operation(FeatureOperation::Thicken { thickness, .. }) =
                &mut definition
            else {
                panic!("admitted synthetic Thicken")
            };
            *thickness = cadmpeg_ir::scalar::PositiveLength::new(3.5);
            ir.model.features[0].evaluation = FeatureEvaluation::from_definition(definition);
            let findings = InventorCodec
                .validate_native(&ctx, &ir, None)
                .expect("changed synthetic Thicken validation");
            assert!(findings.iter().any(|finding| {
                finding.message == "Inventor neutral thicken operands do not match their PmDc slots"
            }));
        }
    }
}

#[test]
fn thicken_selection_validation_checks_both_references() {
    let bytes = crate::test_support::thicken::offset_fixture();
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("synthetic selected Thicken decode");
    let ir = decoded.into_parts().0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("synthetic validation context");
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("synthetic selection validation");
    assert!(
        findings
            .iter()
            .all(|finding| finding.severity < Severity::Error),
        "{findings:?}"
    );
    for field in ["faces", "parameter"] {
        let mut changed = ir.clone();
        let namespace = changed.native.namespace_mut("inventor");
        let mut properties: Vec<serde_json::Value> = namespace
            .arena_as("pm_dc_feature_properties")
            .expect("native properties");
        let selection = properties
            .iter_mut()
            .find(|property| property["kind"]["form"] == "thicken_selection")
            .expect("typed selection");
        selection["kind"][field]["index"] = 1000.into();
        namespace
            .set_arena(&ctx, "pm_dc_feature_properties", &properties)
            .expect("changed native properties");
        let findings = InventorCodec
            .validate_native(&ctx, &changed, None)
            .expect("dangling selection validation");
        assert!(findings.iter().any(|finding| {
            finding.message == "Inventor PmDc feature-property record or reference does not resolve"
        }));
    }
}

#[test]
fn thicken_refusal_reports_the_first_checked_operand() {
    for (slot_two, slot_sixteen, operation, slot) in [
        (false, false, 2, 1),
        (false, false, 3, 16),
        (true, false, 3, 16),
    ] {
        let bytes = crate::test_support::thicken::fixture(slot_two, slot_sixteen, operation);
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthetic refused Thicken decode");
        let (ir, report, _) = decoded.into_parts();
        assert!(matches!(
            ir.model.features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                if kind.as_str() == "Thicken"
        ));
        assert!(report.losses.iter().any(|loss| {
            loss.code == InventorLossCode::FeatureThickenConstructionNative.kind()
                && loss.message == format!(
                    "Retained Inventor Thicken natively: slot {slot} has unresolved construction operands."
                )
        }));
    }
}

#[test]
fn thicken_validation_rejects_control_families_on_the_wrong_classes() {
    let bytes = crate::test_support::thicken::offset_fixture();
    let ir = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("synthetic selected Thicken decode")
        .into_parts()
        .0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("synthetic validation context");
    for family in ["thicken_control13", "thicken_control14"] {
        let mut changed = ir.clone();
        let namespace = changed.native.namespace_mut("inventor");
        let mut properties: Vec<serde_json::Value> = namespace
            .arena_as("pm_dc_feature_properties")
            .expect("native properties");
        let control = properties
            .iter_mut()
            .find(|property| property["kind"]["family"] == family)
            .expect("typed control");
        control["kind"]["family"] = "auxiliary".into();
        namespace
            .set_arena(&ctx, "pm_dc_feature_properties", &properties)
            .expect("changed native properties");
        let findings = InventorCodec
            .validate_native(&ctx, &changed, None)
            .expect("changed control validation");
        assert!(findings.iter().any(|finding| {
            finding.message == "Inventor PmDc feature-property record or reference does not resolve"
        }));
    }
}

#[test]
fn offset_bytes_use_the_selection_dimension_and_validate_signed_motion() {
    let bytes = crate::test_support::thicken::offset_fixture();
    let (mut ir, report, fidelity) = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("synthetic offset decode")
        .into_parts();
    let FeatureDefinition::Operation(FeatureOperation::MoveFace {
        motion: cadmpeg_ir::features::FaceMotion::Offset { distance },
        ..
    }) = ir.model.features[0].evaluation.definition()
    else {
        panic!("neutral offset")
    };
    assert_eq!(distance.get(), 4.0);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == InventorLossCode::FeatureThickenOperandsNative.kind()));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == InventorLossCode::FeatureThickenConstructionNative.kind()));
    let neutral =
        cadmpeg_ir::validate::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses)
            .expect("neutral offset validation");
    assert!(
        neutral
            .findings
            .iter()
            .all(|finding| finding.severity < Severity::Error),
        "{:?}",
        neutral.findings
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("validation context");
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("offset validation");
    assert!(
        findings
            .iter()
            .all(|finding| finding.severity < Severity::Error),
        "{findings:?}"
    );
    let mut changed = ir.clone();
    let namespace = changed.native.namespace_mut("inventor");
    let mut sources: Vec<serde_json::Value> =
        namespace.arena_as("pm_dc_features").expect("native source");
    sources[0]["save_version_major"] = 18.into();
    namespace
        .set_arena(&ctx, "pm_dc_features", &sources)
        .expect("changed version");
    let findings = InventorCodec
        .validate_native(&ctx, &changed, None)
        .expect("version validation");
    assert!(findings.iter().any(|finding| finding.message
        == "Inventor neutral thicken operands do not match their PmDc slots"));
    let mut definition = ir.model.features[0].evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::MoveFace {
        motion: cadmpeg_ir::features::FaceMotion::Offset { distance },
        ..
    }) = &mut definition
    else {
        panic!("neutral offset")
    };
    *distance = cadmpeg_ir::scalar::Length::new(-4.0).expect("reversed distance");
    ir.model.features[0].evaluation = FeatureEvaluation::from_definition(definition);
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("changed offset validation");
    assert!(findings.iter().any(|finding| finding.message
        == "Inventor neutral thicken operands do not match their PmDc slots"));
    let FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, .. }) =
        ir.model.features[0].evaluation.definition()
    else {
        panic!("offset faces")
    };
    ir.model.features[0].evaluation = FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces: faces.clone(),
            distance: cadmpeg_ir::scalar::Length::new(4.0),
        }),
    );
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("changed operation validation");
    assert!(findings.iter().any(|finding| finding.message
        == "Inventor neutral thicken operands do not match their PmDc slots"));
}

#[test]
fn selected_thicken_forms_with_unresolved_mode_or_direction_stay_native() {
    for (slot_two, slot_sixteen, operation, refused_slot) in [
        (false, false, 3, 16),
        (true, false, 2, 1),
        (false, true, 0, 1),
        (false, true, 1, 1),
        (false, true, 4, 1),
        (false, true, 5, 1),
    ] {
        let bytes = crate::test_support::thicken::offset_fixture_with_controls(
            slot_two,
            slot_sixteen,
            operation,
        );
        let (ir, report, _) = InventorCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthetic selected form")
            .into_parts();
        assert!(matches!(ir.model.features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if kind.as_str() == "Thicken"));
        assert!(report.losses.iter().any(|loss| {
            loss.code == InventorLossCode::FeatureThickenConstructionNative.kind()
                && loss.message == format!("Retained Inventor Thicken natively: slot {refused_slot} has unresolved construction operands.")
        }));
    }
}

#[test]
fn unproved_offset_version_emits_the_pinned_native_loss() {
    let bytes = crate::test_support::thicken::offset_fixture_at_version(18);
    let (ir, report, _) = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("synthetic unsupported version")
        .into_parts();
    assert!(
        matches!(ir.model.features[0].evaluation.definition(), FeatureDefinition::Operation(
        FeatureOperation::Native { kind, .. }) if kind.as_str() == "Thicken")
    );
    assert!(report.losses.iter().any(|loss| loss.code == InventorLossCode::FeatureThickenConstructionNative.kind()
        && loss.message == "Retained Inventor Thicken natively: segment version has unresolved construction semantics."));
}
