// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::f3z::merge::{
    append_feature_history, compose_transforms, extend_native, occurrence_key,
    reparent_component_roots, OccurrenceScope,
};
use crate::records::XrefReference;
use cadmpeg_ir::features::FeatureOperation;

fn feature(id: &str, ordinal: u64) -> Feature {
    Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "test".into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: None,
    }
}

#[test]
fn component_feature_history_follows_the_parent_without_losing_relative_order() {
    let mut parent = Model::default();
    parent.features = vec![
        feature("f3d:test:feature#parent-0", 4),
        feature("f3d:test:feature#parent-1", 8),
    ];
    let mut component = Model::default();
    component.features = vec![
        feature("f3d:test:feature#component-0", 10),
        feature("f3d:test:feature#component-1", 12),
    ];

    append_feature_history(&parent, &mut component).unwrap();

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
    let mut parent = Model::default();
    parent.features = vec![feature("f3d:test:feature#parent", u64::MAX)];
    let mut component = Model::default();
    component.features = vec![feature("f3d:test:feature#component", 0)];

    let error = append_feature_history(&parent, &mut component).unwrap_err();

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
        id: PointId::mint("f3d:model:point#1").expect("identity grammar"),
        position: Point3 {
            x: f64::NAN,
            y: f64::INFINITY,
            z: f64::NEG_INFINITY,
        },
        source_object: None,
    };

    let rescoped = OccurrenceScope {
        occurrence: "role/occurrence-0",
    }
    .rewrite(point)
    .expect("a model entity rescopes through the value tree");

    assert_eq!(
        rescoped.id.as_str(),
        "f3d:xref/role/occurrence-0/model:point#1"
    );
    assert!(rescoped.position.x.is_nan());
    assert_eq!(rescoped.position.y, f64::INFINITY);
    assert_eq!(rescoped.position.z, f64::NEG_INFINITY);
}

#[test]
fn occurrence_transform_composes_outside_existing_body_transform() {
    let outer = Transform::affine([
        [0.0, -1.0, 0.0, 20.0],
        [1.0, 0.0, 0.0, 30.0],
        [0.0, 0.0, 1.0, 40.0],
    ])
    .expect("affine transform");
    let inner = Transform::affine([
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("affine transform");

    assert_eq!(
        compose_transforms(outer, inner).unwrap().rows(),
        [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 35.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
}

#[test]
fn merged_component_root_occurrences_become_children_of_the_outer_instance() {
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};

    let outer = OccurrenceId::mint("f3d:model:occurrence#xref-0-0").unwrap();
    let root_child = OccurrenceId::mint("f3d:xref/outer/model:occurrence#xref-0-0").unwrap();
    let nested_child = OccurrenceId::mint("f3d:xref/outer/model:occurrence#xref-1-0").unwrap();
    let mut occurrences = vec![
        Occurrence {
            id: root_child.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        },
        Occurrence {
            id: nested_child.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Occurrence {
                occurrence: root_child.clone(),
            },
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        },
    ];

    reparent_component_roots(&mut occurrences, &outer);

    assert!(matches!(
        occurrences[0].parent,
        OccurrenceParent::Occurrence { ref occurrence } if occurrence == &outer
    ));
    assert!(matches!(
        occurrences[1].parent,
        OccurrenceParent::Occurrence { ref occurrence } if occurrence == &root_child
    ));
}

#[test]
fn repeated_occurrence_merge_remaps_typed_graphs_disjointly() {
    let mut merged = Model::default();
    let mut component = Model::default();
    component.bodies = vec![Body {
        id: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("f3d:brep:entity#2").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    }];
    component.regions = vec![Region {
        id: RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
        body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        shells: Vec::new(),
    }];
    for ordinal in 0..2 {
        let occurrence = format!("role/occurrence-{ordinal}");
        let mut scope = OccurrenceScope {
            occurrence: &occurrence,
        };
        merged
            .extend_rewritten(component.clone(), &mut scope)
            .expect("merge component arenas");
    }

    for ordinal in 0..2 {
        let prefix = format!("f3d:xref/role/occurrence-{ordinal}/brep:entity#");
        assert_eq!(merged.bodies[ordinal].id.as_str(), format!("{prefix}1"));
        assert_eq!(
            merged.bodies[ordinal].regions[0].as_str(),
            format!("{prefix}2")
        );
        assert_eq!(merged.regions[ordinal].id.as_str(), format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].body.as_str(), format!("{prefix}1"));
    }
}

#[test]
fn occurrence_merge_preserves_a_body_name_that_spells_its_identity() {
    use cadmpeg_ir::document::EntityRewrite;
    let source_id = "f3d:model:body#source";
    let body = Body {
        id: BodyId::mint(source_id).unwrap(),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("f3d:model:region#source").unwrap()],
        transform: None,
        name: Some(source_id.into()),
        color: None,
        visible: None,
    };
    let scoped = OccurrenceScope {
        occurrence: "component-0",
    }
    .rewrite(body)
    .unwrap();
    assert_eq!(scoped.id.as_str(), "f3d:xref/component-0/model:body#source");
    assert_eq!(
        scoped.regions[0].as_str(),
        "f3d:xref/component-0/model:region#source"
    );
    assert_eq!(scoped.name.as_deref(), Some(source_id));
}

#[test]
fn occurrence_merge_remaps_and_retains_native_records() {
    let placement = DesignSketchPlacement {
        frame: crate::records::DesignSketchFrame::new(
            42,
            crate::records::DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: 76,
            },
        )
        .unwrap(),
        id: "f3d:Design/BulkStream.dat:design-sketch-placement#42".into(),
        scope_record_index: None,
        entity_id: crate::records::DesignEntityId::try_from("Sketch_1".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::DesignClassTag::try_from("001".to_owned()).unwrap(),
        record_index: 7,

        paired_class_tag: crate::records::DesignClassTag::try_from("002".to_owned()).unwrap(),
    };
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena("design_sketch_placements", &[placement])
        .expect("store component native");
    let mut root = Native::default();
    extend_native(&mut root, component, "role/occurrence-0").unwrap();

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
fn occurrence_configuration_survives_document_and_typed_native_admission() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    let configuration = DesignConfiguration::try_new(
        "Design/table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        Vec::new(),
        serde_json::Map::new(),
    )
    .unwrap();
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena("design_configurations", &[configuration])
        .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    extend_native(&mut ir.native, component, "component-0").unwrap();
    let wire = ir.to_canonical_json().unwrap();
    let admitted = cadmpeg_ir::CadIr::from_json(&wire).unwrap();
    let configurations = admitted
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as::<DesignConfiguration>("design_configurations")
        .unwrap();
    assert_eq!(configurations.len(), 1);
    assert_eq!(configurations[0].entry_name(), "Design/table.dsgcfg");
    assert_eq!(
        configurations[0].id(),
        "f3d:xref/component-0/configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(configurations[0].payload(), serde_json::Map::new());
}

#[test]
fn occurrence_merge_scopes_admitted_native_references_and_preserves_configuration_text() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    use crate::records::BodyVisibility;

    let configuration_payload = serde_json::json!({
        "raw_text": "f3d:brep:entity#3",
        "nested": {
            "f3d:brep:entity#4": "f3d:brep:entity#5",
        },
    })
    .as_object()
    .expect("object configuration payload")
    .clone();
    let configuration = DesignConfiguration::try_new(
        "Design/table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        Vec::new(),
        configuration_payload.clone(),
    )
    .expect("admitted configuration payload");
    let visibility = BodyVisibility {
        id: "f3d:Design/BulkStream.dat:body-visibility#1".into(),
        body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        stream: "Design/BulkStream.dat".into(),
        byte_offset: 10,
        asm_body_key_offset: 20,
        asm_body_key: 3,
        entity_suffix: 1,
        visible: true,
    };
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena("design_configurations", &[configuration])
        .expect("store configuration");
    component
        .namespace_mut("f3d")
        .set_arena("body_visibilities", &[visibility])
        .expect("store typed native reference");

    let mut root = Native::default();
    extend_native(&mut root, component, "role/occurrence-0").unwrap();

    let merged_visibility: Vec<BodyVisibility> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("body_visibilities")
        .expect("read merged visibility arena");
    assert_eq!(
        merged_visibility[0].id,
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:body-visibility#1"
    );
    assert_eq!(
        merged_visibility[0].body.as_str(),
        "f3d:xref/role/occurrence-0/brep:entity#1"
    );
    assert_eq!(merged_visibility[0].stream, "Design/BulkStream.dat");

    let merged_configurations: Vec<DesignConfiguration> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_configurations")
        .expect("read merged configuration arena");
    assert_eq!(
        merged_configurations[0].id(),
        "f3d:xref/role/occurrence-0/configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(
        merged_configurations[0].payload(),
        configuration_payload,
        "configuration text and extension map keys are source payload, not identities"
    );
}

#[test]
fn occurrence_key_separates_fallback_and_authored_roles() {
    let reference = |role: &str, ordinal: u32| XrefReference {
        id: "f3d:xref:reference#1".into(),
        ordinal,
        occurrence_ordinal: 0,
        from: "root.f3d".into(),
        relative_path: "part.f3d".into(),
        neutron_role: role.into(),
        neutron_data: String::new(),
        transform: None,
    };

    assert_eq!(occurrence_key(&reference("", 7)), "ordinal-7/occurrence-0");
    assert_eq!(
        occurrence_key(&reference("ordinal-7", 7)),
        "role-ordinal-7/reference-7/occurrence-0"
    );
    assert_eq!(
        occurrence_key(&reference("role /#: value", 7)),
        "role-role%20%2F%23%3A%20value/reference-7/occurrence-0"
    );
    let key = occurrence_key(&reference("role /#: value", 7));
    cadmpeg_ir::ids::Identity::new(format!("f3d:xref/{key}/native:record#1"))
        .expect("encoded occurrence key remains an admitted identity scope");
}

#[test]
fn occurrence_key_separates_same_role_references_with_reset_ordinals() {
    let reference = |ordinal| XrefReference {
        id: format!("f3d:xref:reference#{ordinal}"),
        ordinal,
        occurrence_ordinal: 0,
        from: "root.f3d".into(),
        relative_path: format!("part-{ordinal}.f3d"),
        neutron_role: "same-role".into(),
        neutron_data: String::new(),
        transform: None,
    };

    assert_ne!(
        occurrence_key(&reference(0)),
        occurrence_key(&reference(1)),
        "reference ordinal is part of the occurrence owner when role ordinals reset"
    );
}
