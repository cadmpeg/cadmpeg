//! Locus identity for reused point handles.

use super::super::marker;
use crate::records::{FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputRelationFamily, FeatureInputRelationInstance};
use crate::resolved_features::relation_geometry::{project_relation_point_geometry,
    project_relation_solved_point_geometry};
use crate::resolved_features::relation_loci::{profile_loci_by_marker, typed_relation_definition};
use cadmpeg_ir::features::{DesignParameter, Feature, FeatureDefinition, FeatureId,
    FeatureOperation, ParameterId, ParameterValue};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntity, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus};
use std::collections::{BTreeMap, HashMap};

#[test]
fn reused_point_handle_gets_one_solved_locus_per_dimension_relation() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        b"relation test",
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch.clone())),
            }),
        ),
        native_ref: Some("feature-native".into()),
    };
    let point = |id: &str, marker: Option<&str>, u: f64| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            })
            .unwrap(),
        )
        .with_native_ref(marker.map(str::to_owned))
    };
    let mut entities = vec![
        point("synthetic:test:id#origin", Some("known-a"), 0.0),
        point("synthetic:test:id#middle", Some("known-b"), 5.0),
        point("synthetic:test:id#far", None, 12.0),
    ];
    let known_a = marker("known-a", Some([0.0, 0.0]));
    let known_b = marker("known-b", Some([0.005, 0.0]));
    let missing = marker("missing", None);
    let operand = |index: usize, marker: &str| FeatureInputOperand {
        offset: cadmpeg_core::decode::u64_from_index(index),
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::D6,
        entity_index: u16::try_from(index).expect("test index fits u16"),
        entity_ref: Some(marker.into()),
    };
    let relation =
        |id: &str, offset: u64, family: FeatureInputRelationFamily, known: &str, scalar: &str| {
            FeatureInputRelationInstance {
                id: id.into(),
                parent: "lane".into(),
                ordinal: 0,
                offset,
                family,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                    vec![scalar.into()],
                    Some(scalar.into()),
                    None,
                )
                .unwrap(),
                operands: vec![operand(0, known), operand(1, "missing")],
            }
        };
    let relations = vec![
        relation(
            "relation-a",
            10,
            FeatureInputRelationFamily::PointPointDistance,
            "known-a",
            "scalar-a",
        ),
        relation(
            "relation-b",
            20,
            FeatureInputRelationFamily::PointPointDistance,
            "known-b",
            "scalar-b",
        ),
        relation(
            "relation-c",
            30,
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            "known-b",
            "scalar-c",
        ),
    ];
    let lane = FeatureInputLane {
        id: "lane#test".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: relations.clone(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![known_a, known_b, missing],
    };
    let parameter = |id: &str, scalar: &str, distance: f64| DesignParameter {
        id: ParameterId::mint(id).expect("identity grammar"),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: id.to_string(),
        expression: format!("{distance}mm"),
        display: None,
        value: Some(ParameterValue::Length(Length::new(distance).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(scalar.into()),
    };
    let parameters = vec![
        parameter("synthetic:test:id#distance-a", "scalar-a", 5.0),
        parameter("synthetic:test:id#distance-b", "scalar-b", 7.0),
        parameter("synthetic:test:id#distance-c", "scalar-c", 7.0),
    ];

    project_relation_point_geometry(
        &ctx,
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    )
    .unwrap();
    project_relation_solved_point_geometry(
        &ctx,
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        &parameters,
        std::slice::from_ref(&lane),
    )
    .unwrap();

    let solved = entities
        .iter()
        .filter(|entity| entity.id().as_str().contains("dimension-point:"))
        .collect::<Vec<_>>();
    assert_eq!(solved.len(), 3);
    assert!(matches!(*solved[0].geometry.definition(),
        SketchGeometryDefinition::Point { position } if position == Point2::new(5.0, 0.0)
    ));
    assert!(matches!(*solved[1].geometry.definition(),
        SketchGeometryDefinition::Point { position } if position == Point2::new(12.0, 0.0)
    ));
    assert!(matches!(*solved[2].geometry.definition(),
        SketchGeometryDefinition::Point { position } if position == Point2::new(12.0, 0.0)
    ));
    assert_ne!(solved[0].geometry_ref, solved[1].geometry_ref);
    assert_ne!(solved[1].geometry_ref, solved[2].geometry_ref);

    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id(), marker))
        .collect::<HashMap<_, _>>();
    let loci = profile_loci_by_marker(
        &ctx,
        std::slice::from_ref(&feature),
        &[],
        &entities,
        std::slice::from_ref(&lane),
    )
    .expect("transform resource admission");
    for (index, relation) in relations.iter().enumerate() {
        let definition = typed_relation_definition(
            &cadmpeg_test_support::service_decode_context(),
            relation,
            Some(&parameters[index]),
            &sketch,
            &entities,
            &markers,
            &loci,
        )
        .unwrap();
        let second = match definition {
            Some(
                SketchConstraintDefinitionInput::DistanceLoci { second, .. }
                | SketchConstraintDefinitionInput::HorizontalDistance { second, .. },
            ) => second,
            other => panic!("unexpected relation definition: {other:?}"),
        };
        assert_eq!(second, SketchLocus::Entity(solved[index].id().clone()));
    }
}
