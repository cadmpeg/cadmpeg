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
            product_definition: None,
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
        product_definition: None,
        id: crate::features::FeatureId::mint(id).unwrap(),
        ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
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
                    distance: crate::scalar::Length::new(1.0).unwrap(),
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

#[test]
fn attached_plate_and_bend_validate_profiles_edges_and_history() {
    use crate::features::{
        EdgeSelection, FeatureDefinition, FeatureId, FeatureOperation, PlanarProfileRef,
        SheetMetalRelief, SheetMetalReliefForm, SheetMetalThicknessSide, SheetMetalTransition,
        SheetMetalTransitionForm,
    };
    use crate::scalar::{NonNegativeLength, PositiveLength};
    let relief = SheetMetalRelief {
        form: SheetMetalReliefForm::StyleDefault,
        width: PositiveLength::new(1.0).unwrap(),
        depth: PositiveLength::new(2.0).unwrap(),
        minimum_remnant: NonNegativeLength::new(0.0).unwrap(),
        seam_gap: NonNegativeLength::new(0.0).unwrap(),
    };
    let transition = SheetMetalTransition {
        form: SheetMetalTransitionForm::StyleDefault,
        radius: NonNegativeLength::new(0.0).unwrap(),
    };
    let plate_id = FeatureId::mint("test:model:feature#plate").unwrap();
    let plate = FeatureOperation::SheetMetalAttachedPlate {
        profile: PlanarProfileRef::Sketch(
            crate::sketches::SketchId::mint("test:model:sketch#missing").unwrap(),
        ),
        thickness: PositiveLength::new(1.0).unwrap(),
        side: SheetMetalThicknessSide::Forward,
        attachment: EdgeSelection::Edges(vec![
            crate::ids::EdgeId::mint("test:model:edge#missing").unwrap()
        ]),
    };
    let bend = FeatureOperation::SheetMetalBend {
        plate: plate_id.clone(),
        attachment: EdgeSelection::All,
        inside_radius: PositiveLength::new(1.0).unwrap(),
        relief,
        transition,
    };
    let mut ir = crate::CadIr::empty();
    ir.model.features.push(feature(
        plate_id.as_str(),
        0,
        FeatureDefinition::Operation(plate),
    ));
    ir.model.features.push(feature(
        "test:model:feature#bend",
        1,
        FeatureDefinition::Operation(bend),
    ));
    let findings = validate_neutral(&ir, Vec::new()).unwrap().findings;
    for missing in ["test:model:sketch#missing", "test:model:edge#missing"] {
        assert!(findings.iter().any(|finding| finding.check
            == crate::report::check::Check::ReferentialIntegrity
            && finding.entity.as_deref() == Some(plate_id.as_str())
            && finding.message.contains(missing)));
    }
    assert!(findings.iter().any(|finding| finding.entity.as_deref()
        == Some("test:model:feature#bend")
        && finding.message.contains("dependencies")));
    ir.model.features[1].dependencies = vec![plate_id.clone()].try_into().unwrap();
    assert!(!feature_reference_findings(&ir)
        .iter()
        .any(|finding| finding.entity.as_deref() == Some("test:model:feature#bend")));
    ir.model.features[0].evaluation.edit(|definition, _| {
        *definition = FeatureDefinition::Operation(FeatureOperation::StoredGeometry {});
    });
    assert!(feature_reference_findings(&ir)
        .iter()
        .any(
            |finding| finding.entity.as_deref() == Some("test:model:feature#bend")
                && finding.message.contains("attached plate")
        ));
    ir.model.features.remove(0);
    assert!(feature_reference_findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("missing attached plate")));
}

#[test]
fn an_attached_plate_requires_one_bend() {
    use crate::features::*;
    use crate::scalar::{NonNegativeLength, PositiveLength};
    let mut ir = crate::CadIr::empty();
    let id = FeatureId::mint("test:model:feature#plate").unwrap();
    let plate = FeatureOperation::SheetMetalAttachedPlate {
        profile: PlanarProfileRef::Native("test:profile".into()),
        thickness: PositiveLength::new(1.0).unwrap(),
        side: SheetMetalThicknessSide::Forward,
        attachment: EdgeSelection::All,
    };
    ir.model
        .features
        .push(feature(id.as_str(), 0, FeatureDefinition::Operation(plate)));
    let missing_bend = |ir: &crate::CadIr| {
        feature_reference_findings(ir)
            .iter()
            .any(|finding| finding.message == "attached plate requires exactly one bend")
    };
    assert!(missing_bend(&ir));
    let bend = FeatureOperation::SheetMetalBend {
        plate: id.clone(),
        attachment: EdgeSelection::All,
        inside_radius: PositiveLength::new(1.0).unwrap(),
        relief: SheetMetalRelief {
            form: SheetMetalReliefForm::StyleDefault,
            width: PositiveLength::new(1.0).unwrap(),
            depth: PositiveLength::new(1.0).unwrap(),
            minimum_remnant: NonNegativeLength::new(0.0).unwrap(),
            seam_gap: NonNegativeLength::new(0.0).unwrap(),
        },
        transition: SheetMetalTransition {
            form: SheetMetalTransitionForm::StyleDefault,
            radius: NonNegativeLength::new(0.0).unwrap(),
        },
    };
    let mut bend = feature(
        "test:model:feature#bend",
        1,
        FeatureDefinition::Operation(bend),
    );
    bend.dependencies.insert(id);
    ir.model.features.push(bend.clone());
    assert!(!missing_bend(&ir));
    bend.id = FeatureId::mint("test:model:feature#second-bend").unwrap();
    bend.ordinal = 2;
    ir.model.features.push(bend);
    assert!(missing_bend(&ir));
}

#[test]
fn corner_requires_a_preceding_bend_dependency() {
    use crate::features::*;
    let bend_id = FeatureId::mint("test:model:feature#bend").unwrap();
    let mut ir = crate::CadIr::empty();
    ir.model.features.push(feature(
        "test:model:feature#corner",
        1,
        FeatureDefinition::Operation(FeatureOperation::SheetMetalCorner {
            bend: bend_id.clone(),
            effect: SheetMetalCornerEffect::Unchanged,
        }),
    ));
    assert!(feature_reference_findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("missing corner bend")));
    ir.model.features.push(feature(
        bend_id.as_str(),
        0,
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
    ));
    assert!(feature_reference_findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("corner bend")));
    let bend: FeatureOperation = serde_json::from_value(serde_json::json!({
        "definition": "sheet_metal_bend", "plate": "test:model:feature#plate",
        "attachment": {"kind": "all"}, "inside_radius": 1.0,
        "relief": {"form": "style_default", "width": 1.0, "depth": 1.0,
            "minimum_remnant": 0.0, "seam_gap": 0.0},
        "transition": {"form": "style_default", "radius": 0.0}
    }))
    .unwrap();
    ir.model.features[1].evaluation.edit(|definition, _| {
        *definition = FeatureDefinition::Operation(bend);
    });
    ir.model.features[0].dependencies.insert(bend_id);
    let corner_findings = |ir: &crate::CadIr| {
        feature_reference_findings(ir)
            .into_iter()
            .filter(|finding| finding.entity.as_deref() == Some("test:model:feature#corner"))
            .collect::<Vec<_>>()
    };
    assert!(corner_findings(&ir).is_empty());
    ir.model.features[1].ordinal = 2;
    assert!(corner_findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("precede")));
    ir.model.features[1].ordinal = 0;
    ir.model.features[0].dependencies = DistinctMembers::default();
    assert!(corner_findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("dependencies")));
}
