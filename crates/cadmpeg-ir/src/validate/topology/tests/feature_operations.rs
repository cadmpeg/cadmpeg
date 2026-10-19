// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::validate::validate_neutral;

#[test]
fn feature_operation_geometry_is_validated() {
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let definitions = vec![
        FeatureDefinition::Operation(FeatureOperation::Form { cages: Vec::new() }),
        FeatureDefinition::Operation(FeatureOperation::Form {
            cages: vec![
                crate::ids::SubdId::mint("synthetic:test:subd#missing").expect("valid identity")
            ],
        }),
    ];
    let expected = ["references missing Form control cage `synthetic:test:subd#missing`"];
    let mut ir = unit_cube().expect("valid unit cube fixture");
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:feature#invalid-{ordinal}"))
                .expect("identity grammar"),
            ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
            name: None,
            suppressed: Some(false),
            dependencies: crate::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),

            needs_recompute: false,

            evaluation: crate::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    let findings = validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings;
    assert!(!findings
        .iter()
        .any(|finding| { finding.entity.as_deref() == Some("synthetic:test:feature#invalid-0") }));
    for message in expected {
        assert!(findings.iter().any(|finding| finding.message == message));
    }
}

fn feature(
    id: &str,
    ordinal: usize,
    definition: crate::features::FeatureDefinition,
) -> crate::features::Feature {
    crate::features::Feature {
        id: crate::features::FeatureId::mint(id).unwrap(),
        ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: crate::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    }
}

fn feature_reference_findings(ir: &crate::CadIr) -> Vec<crate::report::check::Finding> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let index = crate::index::ModelIndex::build(ir);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::super::check_feature_references(&ctx, ir, &index, &mut findings).unwrap();
    ctx.finish_session().unwrap();
    findings
}

#[test]
fn many_block_instances_resolve_their_definitions_in_linear_time() {
    use crate::features::{FeatureDefinition, FeatureOperation};
    // A scan of the feature arena per instance would be 2^32 comparisons.
    const INSTANCES: usize = 1 << 16;
    let block = "synthetic:test:feature#block";
    let plane = "synthetic:test:feature#plane";
    let mut ir = crate::CadIr::empty();
    ir.model.features.push(feature(
        block,
        0,
        FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition { sketch: None }),
    ));
    ir.model.features.push(feature(
        plane,
        1,
        FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame: crate::features::FeatureDatumPlaneFrame::new(
                crate::math::Point3::new(0.0, 0.0, 0.0),
                crate::math::Vector3::new(0.0, 0.0, 1.0),
                crate::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        }),
    ));
    for index in 0..INSTANCES {
        let target = if index == 0 { plane } else { block };
        let mut instance = feature(
            &format!("synthetic:test:feature#instance-{index}"),
            index + 2,
            FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance {
                block: Some(crate::features::FeatureId::mint(target).unwrap()),
                placement: Some(crate::transform::Transform::identity()),
            }),
        );
        instance
            .dependencies
            .insert(crate::features::FeatureId::mint(target).unwrap());
        ir.model.features.push(instance);
    }
    let findings = feature_reference_findings(&ir);
    let not_definitions: Vec<_> = findings
        .iter()
        .filter(|finding| finding.message == "sketch block target is not a block definition")
        .map(|finding| finding.entity.as_deref().unwrap())
        .collect();
    assert_eq!(not_definitions, ["synthetic:test:feature#instance-0"]);
}

#[test]
fn many_datum_plane_cycles_are_each_reported_once_in_linear_time() {
    use crate::features::{DatumPlaneReference, FeatureDefinition, FeatureOperation};
    // A comparison with every reported cycle would be 2^31 steps.
    const CYCLES: usize = 1 << 15;
    let id = |cycle: usize, side: usize| format!("synthetic:test:feature#plane-{cycle}-{side}");
    let mut ir = crate::CadIr::empty();
    for cycle in 0..CYCLES {
        for side in 0..2 {
            ir.model.features.push(feature(
                &id(cycle, side),
                2 * cycle + side,
                FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference: Some(DatumPlaneReference::Feature {
                        feature: crate::features::FeatureId::mint(id(cycle, 1 - side)).unwrap(),
                    }),
                    distance: crate::features::Dimension::Literal(
                        crate::scalar::Length::new(1.0).unwrap(),
                    ),
                }),
            ));
        }
    }
    let findings = feature_reference_findings(&ir);
    let reported: Vec<_> = findings
        .iter()
        .filter(|finding| finding.message.contains("datum-plane reference cycle"))
        .map(|finding| finding.entity.clone().unwrap())
        .collect();
    let expected: Vec<_> = (0..CYCLES).map(|cycle| id(cycle, 0)).collect();
    assert_eq!(reported, expected);
}
