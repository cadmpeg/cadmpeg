// SPDX-License-Identifier: Apache-2.0

use super::*;

fn feature(id: &str, ordinal: u64) -> Feature {
    Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "test".into(),
            parameters: std::collections::BTreeMap::new(),
            properties: std::collections::BTreeMap::new(),
        },
        native_ref: None,
    }
}

#[test]
fn component_feature_history_follows_the_parent_without_losing_relative_order() {
    let parent = Model {
        features: vec![
            feature("f3d:feature#parent-0", 4),
            feature("f3d:feature#parent-1", 8),
        ],
        ..Model::default()
    };
    let mut component = Model {
        features: vec![
            feature("f3d:feature#component-0", 10),
            feature("f3d:feature#component-1", 12),
        ],
        ..Model::default()
    };

    super::append_feature_history(&parent, &mut component).unwrap();

    assert_eq!(
        component
            .features
            .iter()
            .map(|feature| feature.ordinal)
            .collect::<Vec<_>>(),
        vec![9, 11]
    );
}

#[test]
fn component_feature_history_refuses_an_exhausted_ordinal_domain() {
    let parent = Model {
        features: vec![feature("f3d:feature#parent", u64::MAX)],
        ..Model::default()
    };
    let mut component = Model {
        features: vec![feature("f3d:feature#component", 0)],
        ..Model::default()
    };

    let error = super::append_feature_history(&parent, &mut component).unwrap_err();

    assert!(error
        .to_string()
        .contains("merged F3Z feature ordinal exceeds u64::MAX"));
}

/// The rescoping round-trip carries an entity through an untyped value tree.
/// A coordinate is an `f64`, and a decoded one is not guaranteed finite, so
/// the tree must hold the value itself rather than a decimal rendering of it.
#[test]
fn rescoping_a_model_entity_preserves_a_non_finite_coordinate() {
    use cadmpeg_ir::document::EntityRewrite;
    use cadmpeg_ir::ids::PointId;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Point;

    let point = Point {
        id: PointId("f3d:model:point#1".into()),
        position: Point3 {
            x: f64::NAN,
            y: f64::INFINITY,
            z: f64::NEG_INFINITY,
        },
        source_object: None,
    };

    let rescoped = super::OccurrenceScope {
        occurrence: "role/occurrence-0",
    }
    .rewrite(point)
    .expect("a model entity rescopes through the value tree");

    assert_eq!(rescoped.id.0, "f3d:xref/role/occurrence-0/model:point#1");
    assert!(rescoped.position.x.is_nan());
    assert_eq!(rescoped.position.y, f64::INFINITY);
    assert_eq!(rescoped.position.z, f64::NEG_INFINITY);
}

#[test]
fn occurrence_transform_composes_outside_existing_body_transform() {
    let outer = Transform {
        rows: [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 30.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let inner = Transform {
        rows: [
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    assert_eq!(
        super::compose_transforms(outer, inner).rows,
        [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 35.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
}

#[test]
fn repeated_occurrence_merge_remaps_typed_graphs_disjointly() {
    let mut merged = Model::default();
    let component = Model {
        bodies: vec![Body {
            id: BodyId("f3d:brep:entity#1".into()),
            kind: BodyKind::Solid,
            regions: vec![RegionId("f3d:brep:entity#2".into())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        }],
        regions: vec![Region {
            id: RegionId("f3d:brep:entity#2".into()),
            body: BodyId("f3d:brep:entity#1".into()),
            shells: Vec::new(),
        }],
        ..Model::default()
    };
    for ordinal in 0..2 {
        let occurrence = format!("role/occurrence-{ordinal}");
        let mut scope = super::OccurrenceScope {
            occurrence: &occurrence,
        };
        merged
            .extend_rewritten(component.clone(), &mut scope)
            .expect("merge component arenas");
    }

    for ordinal in 0..2 {
        let prefix = format!("f3d:xref/role/occurrence-{ordinal}/brep:entity#");
        assert_eq!(merged.bodies[ordinal].id.0, format!("{prefix}1"));
        assert_eq!(merged.bodies[ordinal].regions[0].0, format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].id.0, format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].body.0, format!("{prefix}1"));
    }
}

#[test]
fn occurrence_merge_remaps_and_retains_native_records() {
    let placement = DesignSketchPlacement {
        id: "f3d:Design/BulkStream.dat:design-sketch-placement#42".into(),
        scope_record_index: None,
        entity_id: "Sketch_1".into(),
        entity_suffix: 1,
        visibility: None,
        byte_offset: 42,
        class_tag: "001".into(),
        record_index: 7,
        frame_length: 34,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: None,
        paired_class_tag: "002".into(),
        paired_byte_offset: 76,
        member_run_head: true,
    };
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena("design_sketch_placements", &[placement])
        .expect("store component native");
    let mut root = Native::default();
    super::extend_native(&mut root, component, "role/occurrence-0");

    let merged: Vec<DesignSketchPlacement> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_sketch_placements")
        .expect("read merged arena");
    assert_eq!(
        merged[0].id,
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:design-sketch-placement#42"
    );
}

#[test]
fn occurrence_merge_remaps_native_record_map_keys_and_nested_payloads() {
    let record = NativeRecord::new(
        "f3d:Design/Configurations.json:design-configuration#1",
        serde_json::json!({
            "channels": {
                "f3d:brep:entity#2": "kept",
                "plain": "f3d:brep:entity#3",
            },
            "payload": [{"link": "f3d:brep:entity#4"}, "not-an-id"],
        })
        .as_object()
        .expect("object payload")
        .clone(),
    );

    let rescoped = super::rescope_record(&record, "role/occurrence-0");

    assert_eq!(
        rescoped.id(),
        "f3d:xref/role/occurrence-0/Design/Configurations.json:design-configuration#1"
    );
    assert_eq!(
        serde_json::Value::Object(rescoped.fields()),
        serde_json::json!({
            "channels": {
                "f3d:xref/role/occurrence-0/brep:entity#2": "kept",
                "plain": "f3d:xref/role/occurrence-0/brep:entity#3",
            },
            "payload": [
                {"link": "f3d:xref/role/occurrence-0/brep:entity#4"},
                "not-an-id",
            ],
        })
    );
}
