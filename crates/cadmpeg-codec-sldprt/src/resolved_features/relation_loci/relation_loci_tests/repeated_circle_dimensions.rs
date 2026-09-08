use super::super::*;
use super::*;

#[test]
fn repeated_circle_dimension_binds_generated_circles_by_parameter_identity() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let circle = |id: &str, center: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::Circle {
                center,
                radius: Length(2.5),
            },
        )
        .with_geometry_ref(Some("driver".into()))
    };
    let entities = vec![
        circle("synthetic:test:id#first", Point2::new(-12.0, -12.0)),
        circle("synthetic:test:id#second", Point2::new(12.0, -12.0)),
    ];
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec!["display".into(), "driver".into()],
            Some("driver".into()),
            Some("display".into()),
        )
        .unwrap(),
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::Native(33065),
            entity_index: 1,
            entity_ref: None,
        }],
    };
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>5".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("driver".into()),
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &HashMap::new(),
            &HashMap::new(),
        ),
        Some(cadmpeg_ir::sketches::SketchConstraintDefinition::RepeatedDiameter {
            entities: repeated,
            parameter: parameter_id,
        }) if repeated == vec![SketchEntityId::mint("synthetic:test:id#first").unwrap(), SketchEntityId::mint("synthetic:test:id#second").unwrap()]
            && parameter_id == parameter.id
    ));
}

#[test]
fn repeated_circle_dimension_binds_reference_display_run_by_radius() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let circle = |id: &str, radius| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(radius),
            },
        )
        .with_geometry_ref(Some(format!("geometry-{id}")))
    };
    let entities = vec![
        circle("synthetic:test:id#first", 2.0),
        circle("synthetic:test:id#second", 2.0),
        circle("synthetic:test:id#third", 2.0),
    ];
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec!["scalar-0".into(), "scalar-1".into(), "scalar-2".into()],
            None,
            None,
        )
        .unwrap(),
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::Native(0x8207),
            entity_index: 0,
            entity_ref: None,
        }],
    };
    let mut parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>4".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(4.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    parameter
        .properties
        .insert("sldprt_relation_parameter_role".into(), "reference".into());
    parameter
        .properties
        .insert("sldprt_relation_id".into(), relation.id.clone());

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &HashMap::new(),
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::RepeatedDiameter {
            entities: entities.iter().map(|entity| entity.id().clone()).collect(),
            parameter: parameter.id,
        })
    );
}
#[test]
fn repeated_circle_dimension_is_inactive_when_any_radius_differs() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entity = |id: &str, radius| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(radius),
            },
        )
        .with_geometry_ref(Some("driver".into()))
    };
    let entities = vec![
        entity("synthetic:test:id#first", 2.5),
        entity("synthetic:test:id#second", 2.5),
    ];
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>5".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("driver".into()),
    };
    let definition = cadmpeg_ir::sketches::SketchConstraintDefinition::RepeatedDiameter {
        entities: entities.iter().map(|entity| entity.id().clone()).collect(),
        parameter: parameter.id.clone(),
    };
    assert!(!relation_constraint_is_inactive(
        Some(&parameter),
        &definition,
        &entities,
    ));

    let mut mismatched = entities;
    mismatched[1].geometry = SketchGeometry::Circle {
        center: Point2::new(0.0, 0.0),
        radius: Length(2.0),
    };
    assert!(relation_constraint_is_inactive(
        Some(&parameter),
        &definition,
        &mismatched,
    ));
}
