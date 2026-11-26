// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashSet};

use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::parameters::{DesignParameter, DimensionDisplay, ParameterValue};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId, SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use crate::records::{FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};

use super::{bind_sketch_profiles, marker_sketch_profiles, ExactProfiles, SketchArenas};

const LANE: &str = "lane";
const RADIUS: f64 = 25.0;

fn native_feature(id: &str, ordinal: u32, name: &str) -> crate::records::Feature {
    crate::records::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(ordinal),
        ordinal,
        name: name.into(),
        kind: "ProfileFeature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn lane_name(ordinal: u32, offset: u64, value: &str) -> FeatureInputName {
    FeatureInputName {
        id: format!("name-{ordinal}"),
        parent: LANE.into(),
        ordinal,
        offset,
        object_id: ObjectId::from_value(ordinal),
        value: value.into(),
    }
}

fn feature(native: &str, name: &str, sketch: SketchFeatureBinding) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(format!("synthetic:test:id#feature-{native}")).unwrap(),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        native_ref: Some(native.into()),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch { sketch },
        )),
    }
}

fn circle(sketch: &SketchId, name: &str) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#circle-{name}")).unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(RADIUS).unwrap(),
        })
        .unwrap(),
    )
}

fn sketch(id: &SketchId, name: &str, profiles: Vec<Vec<SketchEntityUse>>) -> Sketch {
    Sketch {
        id: id.clone(),
        name: Some(name.into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::try_from(profiles).unwrap(),
        native_ref: Some(LANE.into()),
    }
}

/// A configuration pass binds the sketches again in an arena that already
/// holds the marker sketches. Sketch feature A dimensions the circle that a
/// profile stream gives, and the name span of sketch feature B holds that
/// stream. B's marker sketch draws the same circle twice, once in a curve
/// record of state `0`, so its profile is the one circle. The stream sketch
/// keeps A's name and binds to A, and B's marker sketch keeps B's name.
#[test]
fn a_configuration_pass_binds_the_dimensioned_stream_circle_to_its_feature() {
    let stream_id = SketchId::mint("synthetic:test:id#stream-sketch").unwrap();
    let marker_id = SketchId::mint("synthetic:test:id#markers-b").unwrap();
    let stream_circle = circle(&stream_id, "stream");
    let marker_circle = circle(&marker_id, "b");
    let duplicate = circle(&marker_id, "b-duplicate");
    let marker_profiles = marker_sketch_profiles(
        &[marker_circle.clone(), duplicate.clone()],
        |_| true,
        &HashSet::from([duplicate.id().clone()]),
    );
    assert_eq!(marker_profiles.len(), 1);
    let mut sketches = vec![
        sketch(
            &stream_id,
            "Sketch1",
            vec![vec![SketchEntityUse {
                entity: stream_circle.id().clone(),
                reversed: false,
            }]],
        ),
        sketch(&marker_id, "Sketch2", marker_profiles),
    ];
    let mut entities = vec![stream_circle, marker_circle, duplicate];

    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("a", 27, "Sketch1"),
            native_feature("b", 28, "Sketch2"),
        ],
    };
    let lane = FeatureInputLane {
        id: LANE.into(),
        configuration: None,
        native_payload: vec![0; 32].into(),
        classes: Vec::new(),
        names: vec![lane_name(27, 0, "Sketch1"), lane_name(28, 1, "Sketch2")],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut features = vec![
        feature("a", "Sketch1", SketchFeatureBinding::Unresolved),
        feature(
            "b",
            "Sketch2",
            SketchFeatureBinding::Planar(Some(marker_id.clone())),
        ),
    ];
    let parameters = [DesignParameter {
        product_definition: None,
        id: ParameterId::mint("synthetic:test:id#a-diameter").unwrap(),
        owner: Some(features[0].id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: format!("<MOD-DIAM>{}", 2.0 * RADIUS),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length::new(2.0 * RADIUS).unwrap())),
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }];
    let mut builder = cadmpeg_ir::AnnotationBuilder::new();
    let stream =
        cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("test:profile"));
    builder.note(&stream_id, &stream, 2, Some("profile"));
    let mut annotations = builder.build();

    bind_sketch_profiles(
        &mut features,
        SketchArenas {
            sketches: &mut sketches,
            sketch_entities: &mut entities,
            sketch_constraints: &mut Vec::new(),
            annotations: &mut annotations,
        },
        &parameters,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
        &ExactProfiles::default(),
    );

    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(id)),
        }) if id == &stream_id
    ));
    let names = sketches
        .iter()
        .map(|sketch| (sketch.id.clone(), sketch.name.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [(stream_id, Some("Sketch1")), (marker_id, Some("Sketch2"))]
    );
}
