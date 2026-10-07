//! Variable fillet radius and endpoint ownership tests.

use super::super::variable_fillet_radius_groups;
use super::with_projection_context;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputComponentPathEntry,
    FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId,
};
use cadmpeg_ir::features::{
    edge_treatments::{RadiusSpec, VariableRadius},
    FeatureDefinition, FeatureId, FeatureOperation,
};
use std::collections::BTreeMap;

#[test]
fn variable_fillet_radii_join_control_vertices_to_edge_endpoints() {
    let signature = |serial: u32| {
        let mut value = [0u8; 12];
        value[..4].copy_from_slice(&[0x38, 0x80, 0x3b, 0]);
        value[4..8].copy_from_slice(&serial.to_le_bytes());
        value[8..].copy_from_slice(&(serial + 100).to_le_bytes());
        value
    };
    let first_vertex = signature(40);
    let second_vertex = signature(50);
    let mut payload = vec![0; 400];
    let class_name = "moVertDim_c";
    let class_offset = 60;
    payload[56..60].copy_from_slice(&[0x20, 0x81, 0x08, 0]);
    payload[class_offset..class_offset + 4].copy_from_slice(super::super::super::CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&u16::try_from(class_name.len()).unwrap().to_le_bytes());
    payload[class_offset + 6..class_offset + 6 + class_name.len()]
        .copy_from_slice(class_name.as_bytes());
    payload[class_offset + 6 + class_name.len()..class_offset + 8 + class_name.len()]
        .copy_from_slice(&0x87d3_u16.to_le_bytes());
    let write_control = |payload: &mut [u8], marker: usize, vertex: [u8; 12]| {
        payload[marker - 12..marker - 8].copy_from_slice(&3u32.to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
        payload[marker..marker + 16]
            .copy_from_slice(&super::super::super::selections::COMPACT_EDGE_VECTOR_MARKER);
        let mut cursor = marker + 18;
        for (instance, type_signature, local_id) in [
            (0x8521_u16, signature(20), 7_u32),
            (0x8521_u16, signature(20), 6_u32),
            (0x8083_u16, vertex, 1_u32),
        ] {
            payload[cursor..cursor + 2].copy_from_slice(&instance.to_le_bytes());
            payload[cursor + 4..cursor + 16].copy_from_slice(&type_signature);
            payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
            cursor += 20;
        }
    };
    write_control(&mut payload, 130, first_vertex);
    write_control(&mut payload, 240, second_vertex);

    let feature = Feature {
        id: "variable".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(10),
        ordinal: 0,
        name: "Variable fillet".into(),
        kind: "VarFillet".into(),
        input_class: Some("VarFillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([
            (cadmpeg_core::nonblank_literal!("D0"), "R2mm".into()),
            (cadmpeg_core::nonblank_literal!("D01"), "R3mm".into()),
        ]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut next = feature.clone();
    next.id = "next".into();
    next.source_id = FeatureSource::from_value(11);
    next.ordinal = 1;
    next.name = "Next".into();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature, next],
    };
    let name = |id: &str, offset, object_id, value: &str| FeatureInputName {
        id: id.into(),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        object_id: ObjectId::try_from(object_id).ok(),
        value: value.into(),
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "vertex-class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: cadmpeg_core::decode::u64_from_index(class_offset),
            name: class_name.into(),
        }],
        names: vec![
            name("feature-name", 20, 10, "Variable fillet"),
            name("d0-name", 100, 100, "D0"),
            name("d01-name", 210, 101, "D01"),
            name("next-name", 350, 11, "Next"),
        ],
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
    let endpoint = |type_signature| {
        vec![FeatureInputComponentPathEntry {
            instance: Some(0x8083),
            type_signature,
            local_id: Some(1),
        }]
    };
    let selection = FeatureInputEdgeSelection {
        id: "edge".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 80,
        object_name_ref: "feature-name".into(),
        feature_ref: "variable".into(),
        local_edge_ids: vec![7, 6, 1, 0],
        components: Vec::new(),
        references: vec![endpoint(first_vertex), endpoint(second_vertex)],
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    };

    let groups_ctx = cadmpeg_test_support::service_decode_context();
    let groups = variable_fillet_radius_groups(
        &groups_ctx,
        "variable",
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
        &[&selection],
    )
    .expect("fillet resource limits")
    .expect("vertex join");
    assert!(matches!(
        groups.as_slice(),
        [super::super::RadiusSelectionGroup(RadiusSpec::Variable { points }, selections, _)]
            if matches!(points.as_slice(), [
                VariableRadius { parameter: first_parameter, radius: actual_radius },
                VariableRadius { parameter: second_parameter, radius: actual_radius_2 },
            ] if first_parameter.get() == 0.0 && second_parameter.get() == 1.0 && actual_radius.get() == 2.0 && actual_radius_2.get() == 3.0) && selections.len() == 1
    ));
    drop(groups);
    lane.edge_selections.push(selection);
    let mut projected = [cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#variable").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups: cadmpeg_ir::features::NonEmptyMembers::one(
                    cadmpeg_ir::features::edge_treatments::FilletGroup {
                        edges: cadmpeg_ir::features::EdgeSelection::Native("native-edges".into()),
                        radius: RadiusSpec::Unresolved { form: None },
                        tangency_weight: None,
                    },
                ),
            }),
        ),
        native_ref: Some("variable".into()),
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("fillet fixture context");
    super::super::project_compact_edge_selections(
        &ctx,
        &mut projected,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    )
    .expect("fillet projection");
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(groups.as_slice(),
                [cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: cadmpeg_ir::features::EdgeSelection::Native(native),
                    radius: RadiusSpec::Variable { .. },
                    ..
                }] if native == "native-edges")
    ));
}

#[test]
fn variable_fillet_legacy_edge_controls_apply_one_profile_to_endpointless_edges() {
    let signature = |serial: u32| {
        let mut value = [0u8; 12];
        value[..4].copy_from_slice(&[0x38, 0x80, 0x3b, 0]);
        value[4..8].copy_from_slice(&serial.to_le_bytes());
        value[8..].copy_from_slice(&(serial + 100).to_le_bytes());
        value
    };
    let mut payload = vec![0; 400];
    let class_name = "moEdgeDim_c";
    let class_offset = 60;
    payload[56..60].copy_from_slice(&[0x20, 0x81, 0x08, 0]);
    payload[class_offset..class_offset + 4].copy_from_slice(super::super::super::CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&u16::try_from(class_name.len()).unwrap().to_le_bytes());
    payload[class_offset + 6..class_offset + 6 + class_name.len()]
        .copy_from_slice(class_name.as_bytes());
    payload[class_offset + 6 + class_name.len()..class_offset + 8 + class_name.len()]
        .copy_from_slice(&0x87d3_u16.to_le_bytes());
    let write_control = |payload: &mut [u8], marker: usize, edge_ids: &[u32]| {
        payload[marker - 12..marker - 8]
            .copy_from_slice(&u32::try_from(edge_ids.len()).unwrap().to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
        payload[marker..marker + 16]
            .copy_from_slice(&super::super::super::selections::COMPACT_EDGE_VECTOR_MARKER);
        let mut cursor = marker + 18;
        for local_id in edge_ids {
            payload[cursor..cursor + 2].copy_from_slice(&0x81a5_u16.to_le_bytes());
            payload[cursor + 4..cursor + 16].copy_from_slice(&signature(20));
            payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
            cursor += 20;
        }
    };
    write_control(&mut payload, 130, &[28, 29, 33]);
    write_control(&mut payload, 240, &[28, 29, 4]);

    let feature = Feature {
        id: "variable".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(10),
        ordinal: 0,
        name: "Variable fillet".into(),
        kind: "VarFillet".into(),
        input_class: Some("VarFillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([
            (cadmpeg_core::nonblank_literal!("D0"), "R2mm".into()),
            (cadmpeg_core::nonblank_literal!("D1"), "R3mm".into()),
        ]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut next = feature.clone();
    next.id = "next".into();
    next.source_id = FeatureSource::from_value(11);
    next.ordinal = 1;
    next.name = "Next".into();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature, next],
    };
    let name = |id: &str, offset, object_id, value: &str| FeatureInputName {
        id: id.into(),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        object_id: ObjectId::try_from(object_id).ok(),
        value: value.into(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "edge-class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: cadmpeg_core::decode::u64_from_index(class_offset),
            name: class_name.into(),
        }],
        names: vec![
            name("feature-name", 20, 10, "Variable fillet"),
            name("d0-name", 100, u32::MAX, "D0"),
            name("d1-name", 210, u32::MAX, "D1"),
            name("next-name", 350, 11, "Next"),
        ],
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
    let edge_reference = |local_id| {
        vec![FeatureInputComponentPathEntry {
            instance: Some(0x81a5),
            type_signature: signature(20),
            local_id: Some(local_id),
        }]
    };
    let selection = FeatureInputEdgeSelection {
        id: "edge".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 80,
        object_name_ref: "feature-name".into(),
        feature_ref: "variable".into(),
        local_edge_ids: vec![28, 29, 33, 4],
        components: Vec::new(),
        references: vec![
            edge_reference(28),
            edge_reference(29),
            edge_reference(33),
            edge_reference(4),
        ],
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    };

    let groups_ctx = cadmpeg_test_support::service_decode_context();
    let groups =
        variable_fillet_radius_groups(&groups_ctx, "variable", &[history], &[lane], &[&selection])
            .expect("fillet resource limits")
            .expect("legacy edge-control join");
    assert!(matches!(
        groups.as_slice(),
        [super::super::RadiusSelectionGroup(RadiusSpec::Variable { points }, selections, _)]
            if matches!(points.as_slice(), [
                VariableRadius { parameter: first_parameter, radius: actual_radius },
                VariableRadius { parameter: second_parameter, radius: actual_radius_2 },
            ] if first_parameter.get() == 0.0 && second_parameter.get() == 1.0 && actual_radius.get() == 2.0 && actual_radius_2.get() == 3.0) && selections.len() == 1
    ));
}

#[test]
fn variable_fillet_two_control_roster_rejects_endpoint_collision() {
    let feature = Feature {
        id: "variable".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(10),
        ordinal: 0,
        name: "Variable fillet".into(),
        kind: "VarFillet".into(),
        input_class: Some("VarFillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([
            (cadmpeg_core::nonblank_literal!("D0"), "R50".into()),
            (cadmpeg_core::nonblank_literal!("D1"), "R4".into()),
        ]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    };
    let component = |instance, local_id| FeatureInputComponentPathEntry {
        instance: Some(instance),
        type_signature: [0x38, 0x80, 0x3b, 0, 20, 0, 0, 0, 100, 0, 0, 0],
        local_id: Some(local_id),
    };
    let selection = FeatureInputEdgeSelection {
        id: "edge".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 80,
        object_name_ref: "name".into(),
        feature_ref: "variable".into(),
        local_edge_ids: vec![28, 36, 4],
        components: Vec::new(),
        references: vec![
            vec![component(0x81a5, 28)],
            vec![component(0x81a5, 36)],
            vec![component(0x81a5, 4)],
        ],
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    };

    let groups_ctx = cadmpeg_test_support::service_decode_context();
    let groups = variable_fillet_radius_groups(
        &groups_ctx,
        "variable",
        std::slice::from_ref(&history),
        &[],
        &[&selection],
    )
    .expect("fillet resource limits")
    .expect("endpoint-less two-control roster");
    assert!(matches!(
        groups.as_slice(),
        [super::super::RadiusSelectionGroup(RadiusSpec::Variable { points }, selections, _)]
            if matches!(points.as_slice(), [
                VariableRadius { parameter: first_parameter, radius: actual_radius },
                VariableRadius { parameter: second_parameter, radius: actual_radius_2 },
            ] if first_parameter.get() == 0.0 && second_parameter.get() == 1.0 && actual_radius.get() == 50.0 && actual_radius_2.get() == 4.0) && selections.len() == 1
    ));

    drop(groups);
    let mut collision = selection;
    collision.references[0][0].instance = Some(0x8083);
    assert!(with_projection_context(|ctx| variable_fillet_radius_groups(
        ctx,
        "variable",
        &[history],
        &[],
        &[&collision]
    )
    .map(|groups| groups.is_none()))
    .expect("fillet resource limits"));
}
