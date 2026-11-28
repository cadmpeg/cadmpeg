// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::HistoryDrop;

fn sketch_with_a_constrained_line() -> (cadmpeg_ir::CadIr, SketchEntityId, SketchConstraintId) {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch = SketchId::mint("other:model:sketch#profile").unwrap();
    let entity = SketchEntityId::mint("other:model:sketch-entity#line").unwrap();
    let constraint = SketchConstraintId::mint("other:model:constraint#horizontal").unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity.clone(),
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        entity.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch,
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Horizontal {
                entity: entity.clone(),
            },
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    (ir, entity, constraint)
}

#[test]
fn a_failure_that_names_a_constraint_leaves_out_that_constraint_and_keeps_its_operands() {
    let (mut ir, entity, constraint) = sketch_with_a_constrained_line();
    let mut drop = HistoryDrop::default();
    drop.degrade(
        &mut ir,
        CodecError::NotImplemented(format!(
            "relation cannot identify entity {} with one unambiguous marker locus: sketch \
             constraint {}",
            entity.as_str(),
            constraint.as_str()
        )),
    )
    .unwrap();
    assert_eq!(ir.model.sketches.len(), 1);
    assert_eq!(ir.model.sketch_entities.len(), 1);
    assert!(ir.model.sketch_constraints.is_empty());
    let losses = drop.finish();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "writer.sketch-constraints-omitted"
    );
}

#[test]
fn a_second_failure_that_names_no_record_leaves_out_every_parameter() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .parameters
        .push(cadmpeg_ir::features::parameters::DesignParameter {
            product_definition: None,
            id: cadmpeg_ir::features::ParameterId::mint("other:model:parameter#width").unwrap(),
            owner: None,
            ordinal: 0,
            name: "width".into(),
            expression: "10 mm".into(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    let mut drop = HistoryDrop::default();
    let failure = || {
        CodecError::malformed(
            "SLDPRT parameter dependencies are inconsistent with their expressions",
        )
    };
    drop.degrade(&mut ir, failure()).unwrap();
    assert!(ir.model.parameters.is_empty());
    assert!(drop.degrade(&mut ir, failure()).is_err());
    let losses = drop.finish();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("other:model:parameter#width"));
}

/// A stored-geometry feature of `key` whose result is `outputs`.
fn feature_producing(key: &str, outputs: &[&str]) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let mut evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::StoredGeometry {},
    ));
    evaluation.edit(|_, produced| {
        produced.extend(
            outputs
                .iter()
                .map(|body| cadmpeg_ir::ids::BodyId::mint(*body).unwrap()),
        );
    });
    cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint(key).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation,
        native_ref: None,
    }
}

#[test]
fn a_feature_whose_result_body_is_absent_is_left_out_and_named() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.bodies.push(cadmpeg_ir::topology::Body {
        id: cadmpeg_ir::ids::BodyId::mint("other:model:body#held").unwrap(),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible: None,
        unstated: None,
        native_ref: None,
    });
    ir.model.features.extend([
        feature_producing("other:model:feature#held", &["other:model:body#held"]),
        feature_producing(
            "other:model:feature#elsewhere",
            &["other:model:body#elsewhere"],
        ),
    ]);
    let loss = super::features_without_result(&mut ir).unwrap();
    assert_eq!(
        ir.model
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect::<Vec<_>>(),
        ["other:model:feature#held"]
    );
    assert_eq!(loss.code.local_code(), "writer.feature-result-absent");
    assert!(loss.message.contains("other:model:feature#elsewhere"));
    assert!(!loss.message.contains("other:model:feature#held"));
    assert!(super::features_without_result(&mut ir).is_none());
}
