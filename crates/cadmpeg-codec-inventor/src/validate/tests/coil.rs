// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::coil::{definition_matches, CLASS_ID};
use crate::feature::tests::{test_feature, test_label};

#[test]
fn coil_validation_requires_native_definition_for_each_refused_vector() -> TestResult {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    for (marker, count) in [(2, 19), (2, 20), (2, 21), (8, 20)] {
        let source = test_feature(0, count, &[]);
        let mut wire = serde_json::to_value(&source)?;
        wire["properties"]["marker"] = serde_json::json!(marker);
        if marker == 8 {
            wire["properties"]["metadata"]["width"] = serde_json::json!("u16");
        }
        let source: crate::feature::PmDcFeature = serde_json::from_value(wire)?;
        let label = test_label(0, 1, CLASS_ID, &[]);
        let mut data = empty_data();
        for identity in [&source.identity, &label.identity] {
            data.records.push(serde_json::from_value(serde_json::json!({
                "id": format!("inventor:rse:record#{}-{}", identity.segment_token, identity.record_ordinal),
                "token": identity.segment_token.as_str(), "ordinal": identity.record_ordinal,
                "selector": 0, "type_index": 0, "type_id": identity.type_id.as_str(),
                "payload_offset": 0, "payload_len": 0, "payload_sha256": "0".repeat(64),
                "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": "0".repeat(64)
            }))?);
        }
        let mut feature = annotation(source.id());
        feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("Coil".into()),
                parameters: std::collections::BTreeMap::new(),
            },
        ));
        data.pm_dc_features.push(source.clone());
        data.pm_dc_feature_labels.push(label.clone());
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.features = vec![feature.clone()];
        let mut findings = Vec::new();
        super::super::validate_features(&ctx, &ir, &data, &mut findings)?;
        assert!(findings.is_empty(), "{findings:?}");
        assert!(definition_matches(
            &feature,
            &source,
            false,
            std::slice::from_ref(&label),
            &[]
        ));
        assert!(!definition_matches(
            &feature,
            &source,
            true,
            std::slice::from_ref(&label),
            &[]
        ));
        feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            },
        ));
        ir.model.features = vec![feature];
        findings.clear();
        super::super::validate_features(&ctx, &ir, &data, &mut findings)?;
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].message, "Inventor Coil requires its native definition while construction operands are unresolved");
    }
    Ok(())
}

#[test]
fn coil_validation_keeps_ambiguous_labels_native_and_rejects_invented_parameters() {
    let source = test_feature(0, 20, &[]);
    let label = test_label(0, 1, CLASS_ID, &[]);
    let labels = [label.clone(), label];
    let mut feature = annotation(source.id());
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("InventorFeature".into()),
            parameters: std::collections::BTreeMap::new(),
        },
    ));
    assert!(definition_matches(&feature, &source, false, &labels, &[]));
    assert!(!definition_matches(&feature, &source, true, &labels, &[]));
    assert!(!definition_matches(
        &feature,
        &source,
        false,
        &labels[..1],
        &[]
    ));
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("Coil".into()),
            parameters: std::collections::BTreeMap::from([(
                cadmpeg_core::text::NonBlankString::new("left_handed").expect("key"),
                "false".into(),
            )]),
        },
    ));
    assert!(!definition_matches(
        &feature,
        &source,
        false,
        &labels[..1],
        &[]
    ));
}

#[test]
fn coil_validation_refuses_both_hands_for_each_neutral_extent() {
    use cadmpeg_ir::features::{
        CoilConstruction, CoilExtent, CoilPlacement, CoilResult, CoilSection, CoilSectionPlacement,
        FeatureUnitPlaneFrame,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{Angle, Length, NonZeroLength, PositiveLength, PositiveReal};
    let source = crate::feature::coil::tests::source();
    let properties = crate::feature::coil::tests::properties();
    let label = test_label(0, 1, CLASS_ID, &[]);
    let mut feature = annotation(source.id());
    let revolutions = PositiveReal::new(2.0).expect("turns");
    let pitch = NonZeroLength::new(3.0).expect("pitch");
    let extents = [
        CoilExtent::RevolutionsPitch { revolutions, pitch },
        CoilExtent::RevolutionsHeight {
            revolutions,
            height: Length::new(6.0).expect("height"),
        },
        CoilExtent::HeightPitch {
            height: NonZeroLength::new(6.0).expect("height"),
            pitch,
        },
        CoilExtent::Spiral {
            revolutions,
            radial_pitch: pitch,
        },
    ];
    for extent in extents {
        for clockwise in [false, true] {
            feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Coil {
                    construction: CoilConstruction {
                        placement: CoilPlacement::Explicit {
                            frame: FeatureUnitPlaneFrame::new(
                                Point3::new(0.0, 0.0, 0.0),
                                Vector3::new(0.0, 0.0, 1.0),
                                Vector3::new(1.0, 0.0, 0.0),
                            )
                            .expect("frame"),
                        },
                        diameter: PositiveLength::new(10.0).expect("diameter"),
                        extent,
                        section: CoilSection::Circular {
                            diameter: PositiveLength::new(1.0).expect("section"),
                        },
                        section_placement: CoilSectionPlacement::Center,
                        clockwise,
                        taper: Angle::ZERO,
                    },
                    result: CoilResult::NewBody {},
                },
            ));
            assert!(!definition_matches(
                &feature,
                &source,
                false,
                std::slice::from_ref(&label),
                &properties
            ));
        }
    }
}
