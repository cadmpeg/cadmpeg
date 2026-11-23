//! Hole dependency tests.

use super::{lane_with_position_reference, model_hole, native_history};
use std::collections::BTreeMap;

use cadmpeg_ir::features::{holes::HolePlacement, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::SketchId;

use crate::records::FeatureSource;
use crate::resolved_features::holes::project_hole_position_sketches;

#[test]
fn a_hole_with_placements_depends_on_its_position_sketch() {
    let placement = HolePlacement::Axis {
        origin: cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .unwrap(),
        axis: cadmpeg_ir::features::geometry::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
            .unwrap(),
    };
    let mut hole = model_hole();
    hole.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) = definition
        {
            *placements = Some(vec![placement]);
        }
    });
    let placed = hole.evaluation.definition().clone();
    let position_sketch =
        FeatureId::mint("synthetic:test:id#position-sketch").expect("identity grammar");
    let sketch_feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: position_sketch.clone(),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                    SketchId::mint("synthetic:test:id#position-geometry").unwrap(),
                )),
            }),
        ),
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(6),
        ordinal: 1,
        name: "Position".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut features = vec![hole, sketch_feature];

    project_hole_position_sketches(
        &mut features,
        &[],
        &[],
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane_with_position_reference(6)),
    )
    .unwrap();

    assert_eq!(features[0].evaluation.definition(), &placed);
    assert_eq!(features[0].dependencies.as_slice(), [position_sketch]);
}
