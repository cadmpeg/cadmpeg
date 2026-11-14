use crate::resolved_features::compact_reference_planes::principal_top_component_frame;
use crate::resolved_features::transforms::{
    component_sketch_marker_transform, feature_sketch_marker_transform,
    record_top_marker_coordinates,
};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

fn sketch() -> Sketch {
    let (origin, normal, u_axis) = principal_top_component_frame();
    Sketch {
        id: SketchId::mint("synthetic:test:id#top-component").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(origin, normal, u_axis).unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

#[test]
fn top_component_rectangle_and_blind_travel_use_intrinsic_coordinates() {
    let sketch = sketch();
    let (origin, normal, u_axis) = principal_top_component_frame();
    let v_axis = normal.cross(u_axis);
    assert_eq!(normal, Vector3::new(0.0, 1.0, 0.0));
    let transform = component_sketch_marker_transform(&sketch, 1.0, true).unwrap();
    for (stored, expected) in [
        ((2, 3), Point3::new(2.0, 0.0, -3.0)),
        ((7, 3), Point3::new(7.0, 0.0, -3.0)),
        ((7, 11), Point3::new(7.0, 0.0, -11.0)),
        ((2, 11), Point3::new(2.0, 0.0, -11.0)),
    ] {
        let (u, v) = transform.apply(stored).unwrap();
        let point = origin
            .translated(u_axis, cadmpeg_core::convert::f64_from_i64(u).unwrap())
            .translated(v_axis, cadmpeg_core::convert::f64_from_i64(v).unwrap());
        assert_eq!(point, expected);
        let end = point.translated(normal, 13.0);
        assert_eq!(end.y, 13.0);
        assert_ne!(point.translated(normal.scale(-1.0), 13.0).y, end.y);
        let mirrored = origin
            .translated(u_axis, cadmpeg_core::convert::f64_from_i64(u).unwrap())
            .translated(v_axis, -cadmpeg_core::convert::f64_from_i64(v).unwrap());
        assert_ne!(mirrored, expected);
    }
}

#[test]
fn custom_top_frame_keeps_ascending_model_axis_projection() {
    let sketch = sketch();
    assert_eq!(
        component_sketch_marker_transform(&sketch, 1.0, false)
            .unwrap()
            .apply((2, 3)),
        Some((2, -3))
    );
    assert_eq!(
        component_sketch_marker_transform(&sketch, 1.0, true)
            .unwrap()
            .apply((2, 3)),
        Some((2, 3))
    );
}

#[test]
fn top_coordinate_provenance_follows_the_sketch_binding() {
    let sketch = sketch();
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#top-feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            },
        )),
    };
    record_top_marker_coordinates(&mut feature, true);
    assert_eq!(
        feature_sketch_marker_transform(&sketch, 1.0, &[feature])
            .unwrap()
            .apply((2, 3)),
        Some((2, 3))
    );
    assert_eq!(
        feature_sketch_marker_transform(&sketch, 1.0, &[])
            .unwrap()
            .apply((2, 3)),
        Some((2, -3))
    );
}
