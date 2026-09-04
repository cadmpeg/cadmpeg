// SPDX-License-Identifier: Apache-2.0
//! Configuration-lane membership and inherited-state tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use super::*;

#[test]
fn configuration_lane_loss_uses_stored_ids_not_partition_indices() {
    let configurations = [
        with_configuration_id(design_configuration("first", 0, Some(8), None), 1),
        with_configuration_id(design_configuration("second", 1, Some(9), None), 2),
    ];

    assert_eq!(
        unresolved_configuration_lanes(
            &configurations,
            &[
                feature_input_lane("first", Some("1")),
                feature_input_lane("second", Some("2")),
            ],
        ),
        0
    );
    assert_eq!(
        unresolved_configuration_lanes(
            &configurations,
            &[
                feature_input_lane("duplicate-first", Some("1")),
                feature_input_lane("duplicate-second", Some("1")),
                feature_input_lane("unmatched", Some("3")),
            ],
        ),
        3
    );
}

#[test]
fn unresolved_configuration_body_membership_reuses_model_surface_carriers() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("model-surface").expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        bodies: ConfigurationBodies::Unresolved,
        ..design_configuration("unresolved", 0, Some(0), None)
    });

    assert_eq!(configuration_surface_carriers(&ir, 0), ir.model.surfaces,);
}

#[test]
fn resolved_empty_configuration_body_membership_has_no_surface_carriers() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("model-surface").expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model
        .configurations
        .push(design_configuration("empty", 0, Some(0), None));

    assert!(configuration_surface_carriers(&ir, 0).is_empty());
}

#[test]
fn changing_shadowed_ordinal_does_not_steal_stored_id_lane() {
    let native_configurations = vec![
        native_with_configuration_id(native_configuration("explicit-native", 0, Some(7)), 1),
        native_configuration("fallback-native", 1, None),
    ];
    let mut native = native_with_configuration_lanes(
        native_configurations,
        vec![feature_input_lane("explicit-lane", Some("1"))],
    )
    .into();
    let configurations = [
        with_configuration_id(
            design_configuration("explicit", 0, Some(8), Some("explicit-native")),
            1,
        ),
        design_configuration("fallback", 2, None, Some("fallback-native")),
    ];

    sync_neutral_configurations(&configurations, &mut native);

    assert_eq!(
        native.expect("required invariant").feature_input_lanes[0]
            .configuration
            .as_deref(),
        Some("1")
    );
}

#[test]
fn configuration_lane_index_swaps_are_simultaneous() {
    let mut native = native_with_configuration_lanes(
        vec![
            native_with_configuration_id(native_configuration("first-native", 0, None), 1),
            native_with_configuration_id(native_configuration("second-native", 1, None), 2),
        ],
        vec![
            feature_input_lane("first-lane", Some("1")),
            feature_input_lane("second-lane", Some("2")),
        ],
    )
    .into();
    let configurations = [
        with_configuration_id(
            design_configuration("first", 0, None, Some("first-native")),
            2,
        ),
        with_configuration_id(
            design_configuration("second", 1, None, Some("second-native")),
            1,
        ),
    ];

    sync_neutral_configurations(&configurations, &mut native);

    assert_eq!(
        native
            .expect("required invariant")
            .feature_input_lanes
            .into_iter()
            .map(|lane| lane.configuration)
            .collect::<Vec<_>>(),
        [Some("2".into()), Some("1".into())]
    );
}

#[test]
fn deleting_configuration_removes_its_uniquely_owned_lane() {
    let mut native = native_with_configuration_lanes(
        vec![
            native_with_configuration_id(native_configuration("kept-native", 0, Some(9)), 1),
            native_with_configuration_id(native_configuration("deleted-native", 1, Some(10)), 2),
        ],
        vec![
            feature_input_lane("kept-lane", Some("1")),
            feature_input_lane("deleted-lane", Some("2")),
        ],
    )
    .into();

    sync_neutral_configurations(
        &[with_configuration_id(
            design_configuration("kept", 0, Some(11), Some("kept-native")),
            1,
        )],
        &mut native,
    );

    let native = native.expect("required invariant");
    assert_eq!(native.feature_input_lanes.len(), 1);
    assert_eq!(native.feature_input_lanes[0].id, "kept-lane");

    let mut native = native_with_configuration_lanes(
        vec![native_with_configuration_id(
            native_configuration("deleted-native", 0, Some(1)),
            1,
        )],
        vec![
            feature_input_lane("global-lane", None),
            feature_input_lane("deleted-lane", Some("1")),
        ],
    )
    .into();
    sync_neutral_configurations(&[], &mut native);
    let native = native.expect("required invariant");
    assert!(native.feature_histories[0].configurations.is_empty());
    assert_eq!(native.feature_input_lanes.len(), 1);
    assert_eq!(native.feature_input_lanes[0].id, "global-lane");
}

#[test]
fn configuration_lane_follows_stored_id_or_ordinal_changes() {
    for (previous_ordinal, previous_id, previous_lane, ordinal, id, expected) in [
        (2, Some(7), "7", 3, None, "3"),
        (2, None, "2", 4, None, "4"),
    ] {
        let native_configuration =
            native_configuration("native-configuration", previous_ordinal, Some(19));
        let native_configuration = previous_id.map_or(native_configuration.clone(), |id| {
            native_with_configuration_id(native_configuration, id)
        });
        let mut native = native_with_configuration_lanes(
            vec![native_configuration],
            vec![feature_input_lane("lane", Some(previous_lane))],
        )
        .into();
        let mut configuration = design_configuration(
            "configuration",
            ordinal,
            Some(23),
            Some("native-configuration"),
        );
        if let Some(id) = id {
            configuration = with_configuration_id(configuration, id);
        }
        configuration.active = true.into();
        sync_neutral_configurations(&[configuration], &mut native);

        assert_eq!(
            native.expect("required invariant").feature_input_lanes[0]
                .configuration
                .as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn configuration_sketch_state_reuses_projected_neutral_sketch() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, DesignConfiguration, Feature as NeutralFeature,
        FeatureDefinition,
    };
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
        SpatialSketch, SpatialSketchId,
    };

    let native_feature = feature("sketch-native", Some("7"), 0);
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native_feature],
    };
    let feature_id = cadmpeg_ir::features::FeatureId("sketch".into());
    let unresolved = FeatureDefinition::Sketch { sketch: None };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("sketch-native".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: unresolved.clone(),
        native_ref: Some("sketch-native".into()),
    });
    let spatial_feature_id = cadmpeg_ir::features::FeatureId("sldprt:model:feature#spatial".into());
    let spatial_sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#spatial".into());
    ir.model.features.push(NeutralFeature {
        id: spatial_feature_id.clone(),
        ordinal: 1,
        name: Some("spatial-native".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(spatial_sketch_id.clone()),
        },
        native_ref: Some("spatial-native".into()),
    });
    let sketch_id = SketchId("projected-sketch".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("sketch-native".into()),
        configuration: Some("0".into()),
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    });
    ir.model.sketch_entities.push(
        SketchEntity::new(
            SketchEntityId("configuration-line".into()),
            sketch_id.clone(),
            SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
        )
        .with_native_ref(Some("line-marker".into())),
    );
    ir.model.spatial_sketches.push(SpatialSketch {
        id: spatial_sketch_id.clone(),
        name: Some("spatial-native".into()),
        configuration: Some("0".into()),
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    });
    ir.model.configurations.push(DesignConfiguration {
        id: cadmpeg_ir::features::ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                feature_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: unresolved,
                },
            ),
            (
                spatial_feature_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::SpatialSketch { sketch: None },
                },
            ),
        ]),
        native_ref: None,
    });
    let mut lane = feature_input_lane("lane", Some("0"));
    lane.sketch_entities = vec![
        crate::records::SketchInputEntity {
            id: "line-marker".into(),
            parent: lane.id.clone(),
            feature_ref: Some("sketch-native".into()),
            ordinal: 0,
            offset: 10,
            object_index: Some(1),
            local_id: Some(1),
            kind: crate::records::SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: None,
            links: Vec::new(),
            link_selector: None,
        },
        crate::records::SketchInputEntity {
            id: "relation-marker".into(),
            parent: lane.id.clone(),
            feature_ref: Some("sketch-native".into()),
            ordinal: 1,
            offset: 20,
            object_index: Some(2),
            local_id: Some(2),
            kind: crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Horizontal,
            ),
            state_value: None,
            coordinates_m: None,
            links: vec![crate::records::SketchInputLink {
                local_id: 1,
                entity_ref: "line-marker".into(),
            }],
            link_selector: None,
        },
    ];

    let mut annotations = cadmpeg_ir::Annotations::default();
    project_configuration_sketch_states(&mut ir, &[history], &[lane], &mut annotations);

    assert_eq!(ir.model.sketches.len(), 1);
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Sketch {
            sketch: Some(sketch),
        } if sketch == &sketch_id
    ));
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&spatial_feature_id].definition,
        FeatureDefinition::SpatialSketch {
            sketch: Some(sketch),
        } if sketch == &spatial_sketch_id
    ));
    assert!(ir.model.sketch_constraints.iter().any(|constraint| {
        constraint.native_ref.as_deref() == Some("relation-marker")
            && matches!(
                constraint.definition,
                SketchConstraintDefinition::Horizontal { ref entity }
                    if entity.0 == "configuration-line"
            )
    }));
}

#[test]
fn dissected_sketch_alias_inherits_an_omitted_class_without_solved_geometry() {
    use cadmpeg_ir::features::{Feature as NeutralFeature, FeatureDefinition};

    let mut owner = feature("owner-native", Some("63"), 0);
    owner.xml_tag = "Sketch".into();
    owner.name = "Sketch1".into();
    owner.kind = "Sketch".into();
    owner.input_class = Some("moProfileFeature_c".into());
    owner.parameters.insert("D1".into(), "10".into());
    owner.content.push(FeatureContent::Dimension("D1".into()));
    let mut alias = feature("alias-native", Some("85"), 1);
    alias.xml_tag = "Sketch".into();
    alias.name = "Sketch1<3>".into();
    alias.kind = alias.name.clone();
    alias
        .properties
        .insert("Description".into(), alias.name.clone());
    alias.parameters = owner.parameters.clone();
    alias.content = owner.content.clone();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner, alias],
    };
    let neutral = |id: &str, name: &str, native_ref: &str, ordinal| NeutralFeature {
        id: cadmpeg_ir::features::FeatureId(id.into()),
        ordinal,
        name: Some(name.into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Sketch".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch { sketch: None },
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        neutral("owner", "Sketch1", "owner-native", 0),
        neutral("alias", "Sketch1<3>", "alias-native", 1),
    ];
    bind_unique_sketch_feature(&mut features, &[], std::slice::from_ref(&history));
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(features[1].dependencies, [features[0].id.clone()]);

    crate::resolved_features::component_paths::project_dissected_sketches(
        &mut features,
        &[],
        &[history],
    );
    assert!(matches!(
        features[1].definition,
        FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
            ..
        }
    ));
}

#[test]
fn configuration_sketch_states_reuse_shared_geometry_across_lanes() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, DesignConfiguration, Feature as NeutralFeature,
        FeatureDefinition, FeatureId,
    };
    use cadmpeg_ir::sketches::{SpatialSketch, SpatialSketchId};

    let feature_id = FeatureId("sldprt:model:feature#spatial".into());
    let sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#spatial".into());
    let planar_state_id = FeatureId("sldprt:model:feature#planar-state".into());
    let planar_sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#planar-state".into());
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("spatial".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("spatial-native".into()),
    });
    ir.model.features.push(NeutralFeature {
        id: planar_state_id.clone(),
        ordinal: 1,
        name: Some("planar-state".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(planar_sketch_id.clone()),
        },
        native_ref: Some("planar-state-native".into()),
    });
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: Some("spatial".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("first-lane".into()),
    });
    ir.model.spatial_sketches.push(SpatialSketch {
        id: planar_sketch_id.clone(),
        name: Some("planar-state".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("first-lane".into()),
    });
    for ordinal in 0..2 {
        ir.model.configurations.push(DesignConfiguration {
            id: cadmpeg_ir::features::ConfigurationId(format!("configuration-{ordinal}")),
            ordinal,
            active: ordinal == 0,
            source_index: Some(ordinal),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::from([
                (
                    feature_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: FeatureDefinition::SpatialSketch { sketch: None },
                    },
                ),
                (
                    planar_state_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: FeatureDefinition::Sketch { sketch: None },
                    },
                ),
            ]),
            native_ref: None,
        });
    }
    let lanes = [
        feature_input_lane("first-lane", Some("0")),
        feature_input_lane("second-lane", Some("1")),
    ];

    let mut annotations = cadmpeg_ir::Annotations::default();
    project_configuration_sketch_states(&mut ir, &[], &lanes, &mut annotations);

    assert!(ir.model.configurations.iter().all(|configuration| matches!(
        &configuration.feature_states[&feature_id].definition,
        FeatureDefinition::SpatialSketch {
            sketch: Some(projected),
        } if projected == &sketch_id
    )));
    assert!(ir.model.configurations.iter().all(|configuration| matches!(
        &configuration.feature_states[&planar_state_id].definition,
        FeatureDefinition::SpatialSketch {
            sketch: Some(projected),
        } if projected == &planar_sketch_id
    )));
}

#[test]
fn configuration_sketch_state_reuses_scoped_spatial_sketch() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, Feature as NeutralFeature, FeatureDefinition, FeatureId,
    };
    use cadmpeg_ir::sketches::{SpatialSketch, SpatialSketchId};

    let feature_id = FeatureId("sldprt:model:feature#scoped-spatial".into());
    let sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#scoped-spatial".into());
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("scoped-spatial".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("scoped-spatial-native".into()),
    });
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: Some("scoped-spatial".into()),
        configuration: Some("1".into()),
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("supplemental-lane".into()),
    });
    let mut configuration =
        with_configuration_id(design_configuration("configuration", 0, Some(1), None), 1);
    configuration.feature_states.insert(
        feature_id.clone(),
        ConfigurationFeatureState {
            suppressed: false,
            dependencies: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::SpatialSketch { sketch: None },
        },
    );
    ir.model.configurations.push(configuration);

    let lanes = [feature_input_lane("resolved-lane", Some("1"))];
    let mut annotations = cadmpeg_ir::Annotations::default();
    project_configuration_sketch_states(&mut ir, &[], &lanes, &mut annotations);

    assert!(matches!(
        &ir.model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::SpatialSketch {
            sketch: Some(projected),
        } if projected == &sketch_id
    ));
}

#[test]
fn supplemental_edge_paths_project_into_matching_configuration_state() {
    use cadmpeg_ir::features::{
        ChamferGroup, ChamferSpec, ConfigurationFeatureState, DesignConfiguration, EdgeSelection,
        Feature as NeutralFeature, FeatureDefinition, FeatureId, Length,
    };

    let producer_id = FeatureId("producer".into());
    let consumer_id = FeatureId("consumer".into());
    let unresolved = FeatureDefinition::Chamfer {
        groups: vec![ChamferGroup {
            edges: EdgeSelection::Unresolved,
            spec: ChamferSpec::Distance {
                distance: Length(1.0),
            },
        }],
        flip_direction: false,
    };
    let neutral_feature = |id: FeatureId, ordinal, native_ref: &str, definition| NeutralFeature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features = vec![
        neutral_feature(
            producer_id.clone(),
            0,
            "producer-native",
            FeatureDefinition::StoredGeometry,
        ),
        neutral_feature(
            consumer_id.clone(),
            1,
            "consumer-native",
            unresolved.clone(),
        ),
    ];
    ir.model.configurations.push(DesignConfiguration {
        id: cadmpeg_ir::features::ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true,
        source_index: Some(1),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::from([("id".into(), "1".into())]),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                producer_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::StoredGeometry,
                },
            ),
            (
                consumer_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: unresolved,
                },
            ),
        ]),
        native_ref: None,
    });
    let mut lane = feature_input_lane("sldprt:feature-input:config-objects#1", Some("1"));
    lane.edge_selections
        .push(crate::records::FeatureInputEdgeSelection {
            id: "selection".into(),
            parent: lane.id.clone(),
            ordinal: 0,
            offset: 100,
            object_name_ref: "name".into(),
            feature_ref: "consumer-native".into(),
            local_edge_ids: vec![7],
            components: Vec::new(),
            references: Vec::new(),
            producer_feature_refs: vec!["producer-native".into()],
            terminal_feature_ref: Some("producer-native".into()),
        });

    project_configuration_supplemental_edge_selections(&mut ir, &[lane]);

    let state = &ir.model.configurations[0].feature_states[&consumer_id];
    assert_eq!(state.dependencies, vec![producer_id.clone()]);
    assert!(matches!(
        &state.definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(
                &groups[0].edges,
                EdgeSelection::Generated { edges, .. }
                    if edges.len() == 1
                        && edges[0].feature == producer_id
                        && edges[0].local_id == "7"
            )
    ));
}

#[test]
fn configuration_hole_inherits_shared_construction_and_placement() {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureId, HoleKind, HolePlacement, Length, LinearTermination,
    };

    let id = FeatureId("test:model:feature#hole".into());
    let base = cadmpeg_ir::features::Feature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            placements: Some(vec![HolePlacement::Axis {
                origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            }]),
            construction: cadmpeg_ir::features::HoleConstruction::form(HoleKind::Counterbore {
                diameter: Length(8.0),
                depth: Length(4.0),
            }),
            exit_kind: None,
            diameter: Some(Length(5.0)),
            extent: Some(LinearTermination::Blind {
                length: Length(12.0),
            }),
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    };
    let mut configured = base.clone();
    configured.definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        placements: None,
        construction: cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
        exit_kind: None,
        diameter: None,
        extent: None,
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    };

    inherit_configuration_shared_semantics(&mut configured.definition, &base.definition);

    assert_eq!(configured.definition, base.definition);
}

#[test]
fn configuration_lane_inherits_hole_construction_without_replacing_positions() {
    use cadmpeg_ir::features::{
        FeatureDefinition, HoleKind, HolePlacement, Length, LinearTermination,
    };

    let placement = HolePlacement::Axis {
        origin: cadmpeg_ir::math::Point3::new(9.0, 8.0, 7.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
    };
    let base = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        placements: Some(vec![HolePlacement::Axis {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }]),
        construction: cadmpeg_ir::features::HoleConstruction::form(HoleKind::Counterbore {
            diameter: Length(8.0),
            depth: Length(4.0),
        }),
        exit_kind: None,
        diameter: Some(Length(5.0)),
        extent: Some(LinearTermination::Blind {
            length: Length(12.0),
        }),
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    };
    let mut local = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        placements: Some(vec![placement.clone()]),
        construction: cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
        exit_kind: None,
        diameter: None,
        extent: None,
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    };

    inherit_configuration_hole_semantics(&mut local, &base, false);

    let FeatureDefinition::Hole {
        placements,
        construction,
        diameter,
        extent,
        ..
    } = local
    else {
        panic!("hole definition changed variant");
    };
    assert_eq!(placements, Some(vec![placement]));
    assert!(matches!(
        construction,
        cadmpeg_ir::features::HoleConstruction::Form {
            kind: HoleKind::Counterbore {
                diameter: Length(8.0),
                depth: Length(4.0),
            },
            ..
        }
    ));
    assert_eq!(diameter, Some(Length(5.0)));
    assert_eq!(
        extent,
        Some(LinearTermination::Blind {
            length: Length(12.0),
        })
    );
}

#[test]
fn configuration_lane_does_not_inherit_shared_hole_semantics() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, Feature as NeutralFeature, FeatureDefinition, FeatureId,
        HoleKind, Length, LinearTermination,
    };

    let id = FeatureId("test:model:feature#hole-lane".into());
    let base_definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        placements: Some(vec![cadmpeg_ir::features::HolePlacement::Axis {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }]),
        construction: cadmpeg_ir::features::HoleConstruction::form(HoleKind::Counterbore {
            diameter: Length(8.0),
            depth: Length(4.0),
        }),
        exit_kind: None,
        diameter: Some(Length(5.0)),
        extent: Some(LinearTermination::Blind {
            length: Length(12.0),
        }),
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    };
    let local_definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        placements: None,
        construction: cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
        exit_kind: None,
        diameter: None,
        extent: None,
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(NeutralFeature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: base_definition,
        native_ref: None,
    });
    let mut configuration = design_configuration("configuration", 0, Some(0), None);
    configuration.active = true.into();
    configuration.feature_states.insert(
        id.clone(),
        ConfigurationFeatureState {
            suppressed: false,
            dependencies: Vec::new(),
            outputs: Vec::new(),
            definition: local_definition,
        },
    );
    ir.model.configurations.push(configuration);

    let mut annotations = cadmpeg_ir::Annotations::default();
    project_configuration_sketch_states(
        &mut ir,
        &[],
        &[feature_input_lane("lane", Some("0"))],
        &mut annotations,
    );

    assert!(matches!(
        &ir.model.configurations[0].feature_states[&id].definition,
        FeatureDefinition::Hole {
            placements,
            construction,
            diameter: None,
            extent: None,
            ..
        } if placements.is_none()
            && matches!(construction, cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            })
    ));
}

#[test]
fn configuration_offset_plane_inherits_shared_reference() {
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, FeatureDefinition, Length};

    let base = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face(FaceSelection::Faces(vec![
            "test:model:face#1".into(),
        ]))),
        distance: Length(5.0),
    };
    let mut configured = FeatureDefinition::DatumOffsetPlane {
        reference: None,
        distance: Length(8.0),
    };

    inherit_configuration_shared_semantics(&mut configured, &base);

    let FeatureDefinition::DatumOffsetPlane {
        reference,
        distance,
    } = configured
    else {
        panic!("offset-plane definition retained its variant");
    };
    assert!(reference.is_some());
    assert_eq!(distance, Length(8.0));
}

#[test]
fn configuration_offset_plane_does_not_merge_a_resolved_plane_with_a_face() {
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, FeatureDefinition, Length};

    let base = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face(FaceSelection::Faces(vec![
            "test:model:face#1".into(),
        ]))),
        distance: Length(5.0),
    };
    let configured_origin = cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0);
    let mut configured = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::ResolvedPlane {
            origin: configured_origin,
            normal: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }),
        distance: Length(8.0),
    };

    inherit_configuration_shared_semantics(&mut configured, &base);

    let FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::ResolvedPlane { origin, .. }),
        distance,
    } = configured
    else {
        panic!("offset-plane definition retained its face reference");
    };
    assert_eq!(origin, configured_origin);
    assert_eq!(distance, Length(8.0));
}

#[test]
fn scoped_offset_plane_inherits_only_a_frame_matching_reference() {
    use cadmpeg_ir::features::{
        DatumPlaneReference, Feature as NeutralFeature, FeatureDefinition, FeatureId, Length,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let plane_id = FeatureId("test:model:feature#plane".into());
    let offset_id = FeatureId("test:model:feature#offset".into());
    let neutral_feature = |id: FeatureId, ordinal, definition| NeutralFeature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    let base_plane = neutral_feature(
        plane_id.clone(),
        0,
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
    );
    let base_offset = neutral_feature(
        offset_id.clone(),
        1,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(plane_id.clone())),
            distance: Length(12.0),
        },
    );
    let unresolved_reference = || DatumPlaneReference::ResolvedPlane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(1.0, 0.0, 0.0),
        u_axis: Vector3::new(0.0, 0.0, -1.0),
    };
    let mut configured = neutral_feature(
        offset_id.clone(),
        1,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(unresolved_reference()),
            distance: Length(12.0),
        },
    );

    inherit_configuration_reference_plane_semantics(
        std::slice::from_mut(&mut configured),
        &[base_plane.clone(), base_offset.clone()],
    );

    assert_eq!(configured.dependencies, vec![plane_id.clone()]);
    assert!(matches!(
        configured.definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            ..
        } if reference == plane_id
    ));

    let mut mismatched = neutral_feature(
        offset_id,
        1,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            distance: Length(12.0),
        },
    );
    inherit_configuration_reference_plane_semantics(
        std::slice::from_mut(&mut mismatched),
        &[base_plane, base_offset],
    );
    assert!(matches!(
        mismatched.definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane { .. }),
            ..
        }
    ));
}

#[test]
fn scoped_offset_plane_inherits_an_omitted_resolved_reference() {
    use cadmpeg_ir::features::{
        DatumPlaneReference, Feature as NeutralFeature, FeatureDefinition, FeatureId, Length,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let plane_id = FeatureId("test:model:feature#plane".into());
    let offset_id = FeatureId("test:model:feature#offset".into());
    let neutral_feature = |id: FeatureId, definition| NeutralFeature {
        id,
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    let base_plane = neutral_feature(
        plane_id.clone(),
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
    );
    let base_offset = neutral_feature(
        offset_id.clone(),
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(plane_id.clone())),
            distance: Length(6.0),
        },
    );
    let mut configured = neutral_feature(
        offset_id.clone(),
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(6.0),
        },
    );

    inherit_configuration_reference_plane_semantics(
        std::slice::from_mut(&mut configured),
        &[base_plane.clone(), base_offset.clone()],
    );

    assert_eq!(configured.dependencies, vec![plane_id]);
    assert!(matches!(
        configured.definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == FeatureId("test:model:feature#plane".into())
    ));

    let unresolved_base = neutral_feature(
        offset_id.clone(),
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            distance: Length(6.0),
        },
    );
    let mut remains_unresolved = neutral_feature(
        offset_id,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(6.0),
        },
    );
    inherit_configuration_reference_plane_semantics(
        std::slice::from_mut(&mut remains_unresolved),
        std::slice::from_ref(&unresolved_base),
    );
    assert_eq!(remains_unresolved.definition, unresolved_base.definition);
}

#[test]
fn scoped_offset_plane_does_not_merge_a_resolved_plane_with_a_face() {
    use cadmpeg_ir::features::{
        DatumPlaneReference, FaceSelection, Feature as NeutralFeature, FeatureDefinition,
        FeatureId, Length,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let id = FeatureId("test:model:feature#face-offset".into());
    let neutral_feature = |definition| NeutralFeature {
        id: id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    let resolved_plane = || DatumPlaneReference::ResolvedPlane {
        origin: Point3::new(2.0, 3.0, 4.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let base = neutral_feature(FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face(FaceSelection::Faces(vec![
            "face#1".into(),
        ]))),
        distance: Length(7.0),
    });
    let mut configured = neutral_feature(FeatureDefinition::DatumOffsetPlane {
        reference: Some(resolved_plane()),
        distance: Length(7.0),
    });

    inherit_configuration_reference_plane_semantics(
        std::slice::from_mut(&mut configured),
        std::slice::from_ref(&base),
    );

    assert!(matches!(
        configured.definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane { .. }),
            ..
        }
    ));
}

#[test]
fn configuration_numeric_override_inherits_parameter_dimension() {
    use cadmpeg_ir::features::{
        ConfigurationId, DesignConfiguration, DesignParameter, FeatureId, ParameterId,
        ParameterValue,
    };

    let mut ir = cadmpeg_ir::CadIr::empty();
    let parameter_id = ParameterId("test:model:parameter#depth".into());
    let count_id = ParameterId("test:model:parameter#count".into());
    ir.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: Some(FeatureId("test:model:feature#extrude".into())),
        ordinal: 0,
        name: "Depth".into(),
        expression: "7mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(7.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: count_id.clone(),
        owner: Some(FeatureId("test:model:feature#pattern".into())),
        ordinal: 0,
        name: "Count".into(),
        expression: "7".into(),
        display: None,
        value: Some(ParameterValue::Integer(7)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("test:model:configuration#default".into()),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::from([
            (parameter_id.clone(), ParameterValue::Integer(7)),
            (count_id.clone(), ParameterValue::Length(Length(0.007))),
        ]),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });

    align_configuration_parameter_kinds(&mut ir);

    assert_eq!(
        ir.model.configurations[0].parameter_values[&parameter_id],
        ParameterValue::Length(Length(7.0))
    );
    assert!(!ir.model.configurations[0]
        .parameter_values
        .contains_key(&count_id));

    ir.model.configurations[0]
        .parameter_values
        .insert(count_id.clone(), ParameterValue::Length(Length(7.0)));
    align_configuration_parameter_kinds(&mut ir);
    assert_eq!(
        ir.model.configurations[0].parameter_values[&count_id],
        ParameterValue::Integer(7)
    );

    ir.model.configurations[0]
        .parameter_values
        .insert(count_id.clone(), ParameterValue::Real(7.0));
    align_configuration_parameter_kinds(&mut ir);
    assert_eq!(
        ir.model.configurations[0].parameter_values[&count_id],
        ParameterValue::Integer(7)
    );

    ir.model.configurations[0]
        .parameter_values
        .insert(count_id.clone(), ParameterValue::Real(7.5));
    align_configuration_parameter_kinds(&mut ir);
    assert!(!ir.model.configurations[0]
        .parameter_values
        .contains_key(&count_id));
}

#[test]
fn configuration_topology_binding_updates_snapshot_face_selection() {
    use cadmpeg_ir::features::{
        DatumPlaneReference, FaceSelection, Feature as NeutralFeature, FeatureDefinition,
        FeatureId, Length,
    };
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Face, Sense};

    let feature_id = FeatureId("test:model:feature#offset".into());
    let feature_ref = "test:history:feature#offset";
    let mut type_signature = [0_u8; 12];
    type_signature[4..8].copy_from_slice(&7_u32.to_le_bytes());
    let components = vec![crate::records::FeatureInputComponentPathEntry {
        instance: Some(0x8001),
        type_signature,
        local_id: Some(11),
    }];
    let native =
        crate::resolved_features::terminations::compact_surface_selection_value(&components);
    let selection = || crate::records::FeatureInputSurfaceSelection {
        id: "selection".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        selector: 0,
        endpoint_selector: None,
        object_name_ref: String::new(),
        feature_ref: feature_ref.into(),
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
        components: components.clone(),
    };
    let definition = || FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face(FaceSelection::Native(
            native.clone(),
        ))),
        distance: Length(4.0),
    };
    let feature = NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: definition(),
        native_ref: Some(feature_ref.into()),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(feature);
    ir.model.faces.push(Face {
        id: FaceId::mint("face").expect("identity grammar"),
        shell: ShellId::mint("shell").expect("identity grammar"),
        surface: SurfaceId::mint("surface").expect("identity grammar"),
        sense: Sense::Forward,
        loops: vec![LoopId::mint("loop").expect("identity grammar")].into(),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.configurations.push(with_configuration_id(
        design_configuration("config", 0, Some(0), None),
        1,
    ));
    ir.model.configurations[0].feature_states.insert(
        feature_id,
        cadmpeg_ir::features::ConfigurationFeatureState {
            suppressed: false,
            dependencies: Vec::new(),
            outputs: Vec::new(),
            definition: definition(),
        },
    );
    let mut lane = feature_input_lane("lane", Some("1"));
    lane.surface_selections.push(selection());

    bind_configuration_topology_selections(&mut ir, &[], &[lane], &[("face".into(), 7, 11)]);

    assert!(matches!(
        &ir.model.configurations[0].feature_states.values().next().unwrap().definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face(FaceSelection::Resolved {
                faces,
                native: resolved_native,
            })),
            ..
        } if faces == &[FaceId::mint("face").expect("identity grammar")] && resolved_native == &native
    ));
}

#[test]
fn configuration_frame_alias_binds_without_body_membership() {
    use cadmpeg_ir::features::{
        ConfigurationBodies, DatumPlaneReference, FaceSelection, Feature as NeutralFeature,
        FeatureDefinition, FeatureId, Length,
    };
    use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Face, Sense};

    let feature_id = FeatureId("test:model:feature#offset".into());
    let definition = || FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::ResolvedPlane {
            origin: Point3::new(0.0, 0.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }),
        distance: Length(4.0),
    };
    let feature = NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: definition(),
        native_ref: None,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(feature);
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("surface").expect("identity grammar"),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.faces.push(Face {
        id: FaceId::mint("face").expect("identity grammar"),
        shell: ShellId::mint("shell").expect("identity grammar"),
        surface: SurfaceId::mint("surface").expect("identity grammar"),
        sense: Sense::Forward,
        loops: vec![LoopId::mint("loop").expect("identity grammar")].into(),
        name: None,
        color: None,
        tolerance: None,
    });
    let mut configuration = design_configuration("config", 0, None, None);
    configuration.bodies = ConfigurationBodies::Unresolved;
    configuration.properties.insert("id".into(), "3".into());
    ir.model.configurations.push(configuration);
    ir.model.configurations[0].feature_states.insert(
        feature_id,
        cadmpeg_ir::features::ConfigurationFeatureState {
            suppressed: false,
            dependencies: Vec::new(),
            outputs: Vec::new(),
            definition: definition(),
        },
    );

    let lane = feature_input_lane("lane", Some("3"));
    bind_configuration_topology_selections(&mut ir, &[], &[lane], &[]);

    assert!(matches!(
        &ir.model.configurations[0].feature_states.values().next().unwrap().definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face(FaceSelection::Faces(faces))),
            ..
        } if faces == &[FaceId::mint("face").expect("identity grammar")]
    ));
}
