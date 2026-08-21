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
    let unresolved = FeatureDefinition::Sketch {
        space: SketchSpace::Planar,
        sketch: None,
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("sketch-native".into()),
        suppressed: Some(false),
        parent: None,
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
        parent: None,
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
    ir.model.sketch_entities.push(SketchEntity {
        id: SketchEntityId("configuration-line".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: Some("line-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        },
    });
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
        active: true.into(),
        source_index: Some(0),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
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
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch),
            ..
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
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Sketch".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: None,
        },
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
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("spatial".into()),
        suppressed: Some(false),
        parent: None,
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
        parent: None,
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
            active: (ordinal == 0).into(),
            source_index: Some(ordinal),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
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
                        definition: FeatureDefinition::Sketch {
                            space: SketchSpace::Planar,
                            sketch: None,
                        },
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
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
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
        active: true.into(),
        source_index: Some(1),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::from([("id".into(), "1".into())]),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
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
        FeatureDefinition, FeatureId, HoleKind, HolePlacement, Length, Termination,
    };

    let id = FeatureId("test:model:feature#hole".into());
    let base = cadmpeg_ir::features::Feature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        parent: None,
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
            position: None,
            direction: None,
            placements: vec![HolePlacement::Axis {
                origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            }],
            kind: HoleKind::Counterbore {
                diameter: Length(8.0),
                depth: Length(4.0),
            },
            exit_kind: None,
            diameter: Some(Length(5.0)),
            extent: Some(Termination::Blind {
                length: Length(12.0),
            }),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    };
    let mut configured = base.clone();
    configured.definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: Vec::new(),
        kind: HoleKind::Simple,
        exit_kind: None,
        diameter: None,
        extent: None,
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    };

    inherit_configuration_shared_semantics(&mut configured.definition, &base.definition);

    assert_eq!(configured.definition, base.definition);
}

#[test]
fn configuration_lane_does_not_inherit_shared_hole_semantics() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, Feature as NeutralFeature, FeatureDefinition, FeatureId,
        HoleKind, Length, Termination,
    };

    let id = FeatureId("test:model:feature#hole-lane".into());
    let base_definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: vec![cadmpeg_ir::features::HolePlacement::Axis {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }],
        kind: HoleKind::Counterbore {
            diameter: Length(8.0),
            depth: Length(4.0),
        },
        exit_kind: None,
        diameter: Some(Length(5.0)),
        extent: Some(Termination::Blind {
            length: Length(12.0),
        }),
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    };
    let local_definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: Vec::new(),
        kind: HoleKind::Simple,
        exit_kind: None,
        diameter: None,
        extent: None,
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(NeutralFeature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        parent: None,
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
            kind: HoleKind::Simple,
            diameter: None,
            extent: None,
            ..
        } if placements.is_empty()
    ));
}

#[test]
fn configuration_offset_plane_inherits_shared_reference() {
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, FeatureDefinition, Length};

    let base = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face {
            face: FaceSelection::Faces(vec!["test:model:face#1".into()]),
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }),
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
fn configuration_offset_plane_replaces_only_an_unresolved_face() {
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, FeatureDefinition, Length};

    let base = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face {
            face: FaceSelection::Faces(vec!["test:model:face#1".into()]),
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }),
        distance: Length(5.0),
    };
    let configured_origin = cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0);
    let mut configured = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face {
            face: FaceSelection::Unresolved,
            origin: configured_origin,
            normal: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }),
        distance: Length(8.0),
    };

    inherit_configuration_shared_semantics(&mut configured, &base);

    let FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face { face, origin, .. }),
        distance,
    } = configured
    else {
        panic!("offset-plane definition retained its face reference");
    };
    assert_eq!(face, FaceSelection::Faces(vec!["test:model:face#1".into()]));
    assert_eq!(origin, configured_origin);
    assert_eq!(distance, Length(8.0));
}

#[test]
fn configuration_numeric_override_inherits_parameter_dimension() {
    use cadmpeg_ir::features::{
        ConfigurationId, DesignConfiguration, DesignParameter, FeatureId, ParameterId,
        ParameterValue,
    };

    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
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
        active: true.into(),
        source_index: Some(0),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::from([
            (parameter_id.clone(), ParameterValue::Integer(7)),
            (count_id.clone(), ParameterValue::Length(Length(0.007))),
        ]),
        suppressed_features: Vec::new(),
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
