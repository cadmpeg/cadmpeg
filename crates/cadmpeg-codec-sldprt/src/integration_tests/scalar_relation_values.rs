// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::{
    DesignParameter, DistinctMembers, Feature, FeatureContent, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation, ParameterId, ParameterValue,
    SketchFeatureBinding,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{FiniteReal, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use crate::records::operand_tag::NativeOperandTag;
use crate::records::relation_scalars::RelationScalars;
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputRelationFamily, FeatureInputRelationInstance, FeatureInputScalar,
    FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
};
use crate::resolved_features::projections::type_display_relation_parameters;
use crate::resolved_features::relation_geometry::project_relation_bindings;

struct Fixture {
    feature: Feature,
    parameter: DesignParameter,
    lane: FeatureInputLane,
    sketch: Sketch,
    entities: Vec<SketchEntity>,
}

impl Fixture {
    fn new(value_m: f64) -> Self {
        let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
        let feature = Feature {
            product_definition: None,
            id: FeatureId::mint("synthetic:test:id#feature").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Planar(Some(sketch_id.clone())),
                },
            )),
            native_ref: Some("feature".into()),
        };
        let parameter = DesignParameter {
            product_definition: None,
            id: ParameterId::mint("synthetic:test:id#parameter").unwrap(),
            owner: Some(feature.id.clone()),
            ordinal: 0,
            name: "D1".into(),
            expression: "3.0".into(),
            display: None,
            value: Some(ParameterValue::Real(FiniteReal::new(3.0).unwrap())),
            dependencies: DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let operands = [0, 1]
            .into_iter()
            .map(|entity_index| FeatureInputOperand {
                offset: u64::from(entity_index),
                reference_ref: format!("reference-{entity_index}"),
                kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100),
                entity_index,
                entity_ref: Some(format!("point-{entity_index}")),
            })
            .collect::<Vec<_>>();
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new().into(),
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                value: "D1".into(),
                object_id: None,
            }],
            scalars: vec![FeatureInputScalar {
                id: "driver".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 0,
                offset: 0,
                object_id: 0,
                name: "name".into(),
                value: FiniteReal::new(value_m).unwrap(),
                role: FeatureInputScalarRole::Driving,
                operands: operands.clone(),
                archive_class: None,
            }],
            relation_bindings: Vec::new(),
            relation_instances: vec![FeatureInputRelationInstance {
                id: "relation".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                family: FeatureInputRelationFamily::PointPointDistance,
                class_ref: "class".into(),
                feature_ref: "feature".into(),
                scalars: RelationScalars::from_refs(
                    vec!["driver".into()],
                    Some("driver".into()),
                    None,
                )
                .unwrap(),
                operands,
            }],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: [0.0, 0.02]
                .into_iter()
                .enumerate()
                .map(|(i, x)| {
                    let mut point = SketchInputEntity::new(
                        format!("point-{i}"),
                        "lane",
                        u32::try_from(i).unwrap(),
                        u64::try_from(i).unwrap(),
                        SketchInputKind::Point,
                    );
                    point.feature_ref = Some("feature".into());
                    point.coordinates_m = cadmpeg_ir::units::FiniteVector::new([x, 0.0]);
                    point
                })
                .collect(),
        };
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: SketchProfiles::default(),
            native_ref: Some("lane".into()),
        };
        let entities = [0.0, 20.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                SketchEntity::new(
                    SketchEntityId::mint(format!("synthetic:test:id#point-{i}")).unwrap(),
                    sketch_id.clone(),
                    SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: Point2::new(x, 0.0),
                    })
                    .unwrap(),
                )
                .with_native_ref(Some(format!("point-{i}")))
            })
            .collect();
        Self {
            feature,
            parameter,
            lane,
            sketch,
            entities,
        }
    }

    fn type_parameter(&mut self) {
        type_display_relation_parameters(
            std::slice::from_mut(&mut self.parameter),
            std::slice::from_ref(&self.feature),
            std::slice::from_ref(&self.lane),
        )
        .unwrap();
    }

    fn constraint_kind(&self) -> SketchConstraintDefinitionInput {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut constraints = Vec::new();
        project_relation_bindings(
            &ctx,
            &mut constraints,
            std::slice::from_ref(&self.sketch),
            std::slice::from_ref(&self.feature),
            &self.entities,
            std::slice::from_ref(&self.parameter),
            std::slice::from_ref(&self.lane),
        )
        .unwrap();
        assert_eq!(constraints.len(), 1);
        assert_ne!(constraints[0].active, Some(false));
        constraints.remove(0).definition.kind().clone()
    }
}

#[test]
fn owned_driving_value_supplies_the_measured_length() {
    let mut fixture = Fixture::new(0.02);
    fixture.type_parameter();
    assert_eq!(
        fixture.parameter.value,
        Some(ParameterValue::Length(Length::new(20.0).unwrap()))
    );
    assert_eq!(fixture.parameter.native_ref.as_deref(), Some("driver"));
    assert_eq!(
        fixture
            .parameter
            .properties
            .get("sldprt_relation_parameter_role")
            .map(String::as_str),
        Some("driving")
    );
    assert!(matches!(
        fixture.constraint_kind(),
        SketchConstraintDefinitionInput::DistanceLoci { .. }
    ));
}

#[test]
fn disagreeing_driving_value_keeps_the_relation_native() {
    let mut fixture = Fixture::new(0.03);
    fixture.type_parameter();
    assert_eq!(
        fixture.parameter.value,
        Some(ParameterValue::Length(Length::new(30.0).unwrap()))
    );
    assert!(matches!(
        fixture.constraint_kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
}

#[test]
fn competing_driving_records_keep_the_display_binding() {
    let mut fixture = Fixture::new(0.02);
    let mut scalar = fixture.lane.scalars[0].clone();
    scalar.id = "other-driver".into();
    fixture.lane.scalars.push(scalar);
    let mut relation = fixture.lane.relation_instances[0].clone();
    relation.id = "other-relation".into();
    relation.scalars = RelationScalars::from_refs(
        vec!["other-driver".into()],
        Some("other-driver".into()),
        None,
    )
    .unwrap();
    fixture.lane.relation_instances.push(relation);
    fixture.type_parameter();
    assert!(fixture.parameter.native_ref.is_none());
    assert!(!fixture
        .parameter
        .properties
        .contains_key("sldprt_relation_parameter_role"));
}

#[test]
fn symbolic_parameter_retains_the_display_binding() {
    let mut fixture = Fixture::new(0.02);
    fixture.parameter.expression = "D2 * 2".into();
    fixture.type_parameter();
    assert!(fixture.parameter.native_ref.is_none());
    assert!(!fixture
        .parameter
        .properties
        .contains_key("sldprt_relation_parameter_role"));
}

#[test]
fn owned_point_line_driver_uses_the_solved_line_distance() {
    let mut fixture = Fixture::new(0.02);
    fixture.lane.relation_instances[0].family = FeatureInputRelationFamily::PointLineDistance;
    fixture.lane.relation_instances[0].operands[0].entity_ref = Some("point-0".into());
    fixture.lane.relation_instances[0].operands[1].entity_index = 9;
    fixture.lane.relation_instances[0].operands[1].entity_ref = None;
    fixture.entities[1] = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#line").unwrap(),
        fixture.sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(-10.0, 20.0),
            end: Point2::new(10.0, 20.0),
        })
        .unwrap(),
    );
    fixture.type_parameter();
    assert_eq!(
        fixture.parameter.value,
        Some(ParameterValue::Length(Length::new(20.0).unwrap()))
    );
    assert!(matches!(
        fixture.constraint_kind(),
        SketchConstraintDefinitionInput::DistanceLoci { .. }
    ));
}
