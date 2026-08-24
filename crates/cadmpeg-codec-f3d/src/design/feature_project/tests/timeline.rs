// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use crate::design::feature_project::ScopeHistoryGraph;

#[test]
fn work_point_history_state_keys_are_history_qualified() {
    let scope_a = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#a", "Extrude", 1);
    let scope_b = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#b", "Fillet", 2);
    let graph = ScopeHistoryGraph {
        histories_present: true,
        bound_histories: HashMap::from([
            (scope_a.id.clone(), "f3d:history#a".to_owned()),
            (scope_b.id.clone(), "f3d:history#b".to_owned()),
        ]),
        scopes_by_state: HashMap::new(),
    };

    assert_ne!(graph.state_key(&scope_a, 7), graph.state_key(&scope_b, 7));
}

#[test]
fn feature_projection_uses_timeline_items_not_scope_byte_order() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut earlier = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#900"),
        "Extrude",
        100,
    );
    earlier.byte_offset = 900;
    earlier.history_state_id = Some(7);
    let mut later = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        "Fillet",
        200,
    );
    later.byte_offset = 100;
    later.previous_history_state_id = Some(7);
    let scopes = [later.clone(), earlier.clone()];
    let timeline = |items| DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 10),
        byte_offset: 10,
        class_tag: "256".into(),
        record_index: 35,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 17,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: items,
        item_record_index_offsets: vec![0; 3],
    };
    let authored = timeline(vec![100, 150, 200]);
    let project = |timeline: &DesignFeatureTimeline| {
        project_parameter_design_with_edge_identities(
            &crate::design::feature_project::ProjectInputs {
                native: &[],
                owners: &[],
                scopes: &scopes,
                timelines: std::slice::from_ref(timeline),
                construction_groups: &[],
                fillet_radius_groups: &[],
                edge_operands: &[],
                edge_identity_operands: &[],
                edge_treatment_vertex_operands: &[],
                entity_selection_operands: &[],
                curve_identities: &[],
                face_operands: &[],
                body_recipe_operands: &[],
                legacy_loft_body_carriers: &[],
                placements: &[],
                body_bindings: &[],
                histories: &[],
            },
        )
    };
    let (features, _) = project(&authored).expect("exact authored order");
    let earlier_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&earlier.id))
        .expect("earlier feature");
    let later_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&later.id))
        .expect("later feature");
    assert_eq!(earlier_feature.ordinal, 0);
    assert_eq!(later_feature.ordinal, 2);
    assert_eq!(later_feature.dependencies, [earlier_feature.id.clone()]);

    let unrelated = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream("f3d:Other/BulkStream.dat", 10),
        item_record_indices: vec![9000],
        item_record_index_offsets: vec![0],
        ..authored.clone()
    };
    let ordinals = crate::design::feature_project::authored_scope_ordinals(
        &scopes,
        &[unrelated, authored.clone()],
    )
    .expect("an unrelated timeline does not shift this stream");
    assert_eq!(ordinals[&(stream, 100)], 0);
    assert_eq!(ordinals[&(stream, 200)], 2);

    let mut reversed = timeline(vec![200, 150, 100]);
    reversed.item_record_index_offsets = vec![0; reversed.item_record_indices.len()];
    let error = project(&reversed).expect_err("forward history edge must be rejected");
    assert!(error
        .to_string()
        .contains("dependency does not precede its authored timeline position"));

    let second = DesignFeatureTimeline {
        source_ordinal: 1,
        record_index: 36,
        item_record_indices: vec![300],
        item_record_index_offsets: vec![0],
        ..authored.clone()
    };
    let error = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &scopes,
            timelines: &[authored, second],
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        },
    )
    .expect_err("independent nonempty timelines have no total order");
    assert!(error
        .to_string()
        .contains("multiple nonempty Design timelines"));
}

#[test]
fn feature_projection_collapses_internal_scope_history_chains() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut predecessor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        "Extrude",
        100,
    );
    predecessor.history_state_id = Some(7);
    let mut internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#150"),
        "Base Feature",
        150,
    );
    internal.history_state_id = Some(8);
    internal.previous_history_state_id = Some(7);
    let mut successor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        "Fillet",
        200,
    );
    successor.history_state_id = Some(9);
    successor.previous_history_state_id = Some(8);
    let scopes = vec![successor.clone(), internal.clone(), predecessor.clone()];
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 10),
        byte_offset: 10,
        class_tag: "256".into(),
        record_index: 35,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 17,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![100, 200],
        item_record_index_offsets: vec![0, 0],
    };
    let mut parameter = parse_design_parameter(&parameter_record(
        Some(40),
        "1 mm",
        "FeatureInput",
        Some("mm"),
        "InternalValue",
        0.1,
    ))
    .expect("synthetic internal parameter");
    parameter.id = format!("{stream}:design-parameter#41");
    parameter.record_index = 41;
    parameter.owner_record_index = Some(40);
    let owner = DesignParameterOwner {
        id: format!("{stream}:design-parameter-owner#40"),
        byte_offset: 0,
        frame_length: 0,
        class_tag: "292".into(),
        record_index: 40,
        scope_record_index: internal.record_index,
        local_ordinal: 0,
        evaluated_value: 0.1,
        evaluated_value_offset: 0,
        parameter_record_index: parameter.record_index,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: 42,
    };
    let (features, parameters) = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            scopes: &scopes,
            timelines: std::slice::from_ref(&timeline),
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        },
    )
    .expect("timeline-listed feature projection through one internal scope");

    assert_eq!(features.len(), 2);
    assert!(features
        .iter()
        .all(|feature| feature.native_ref.as_deref() != Some(internal.id.as_str())));
    let predecessor_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(predecessor.id.as_str()))
        .expect("projected predecessor");
    let successor_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(successor.id.as_str()))
        .expect("projected successor");
    assert_eq!(
        successor_feature.dependencies,
        [predecessor_feature.id.clone()]
    );
    assert_eq!(parameters.len(), 1);
    assert!(parameters[0].owner.is_none());
    assert_eq!(
        parameters[0].properties.get("owner_record_index"),
        Some(&owner.record_index.to_string())
    );
}

#[test]
fn feature_projection_uses_the_timeline_position_of_an_assembly_datum_envelope() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut assembly = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        "Assemble",
        10,
    );
    assembly.assembly_alignment = Some(DesignAssemblyAlignment {
        angle: 0.0,
        offset: [0.0; 3],
        owner_record_indices: Vec::new(),
        value_offsets: Vec::new(),
        operand_frames: None,
        legacy_operand_carriers: None,
        solved_frame: None,
        operand_paths: None,
        axial_operand_targets: None,
        limits: None,
        joint_origin_scope_record_index: Some(20),
    });
    let mut origin = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#20"),
        "JointOrigin",
        20,
    );
    origin.joint_origin_transform = Some(identity_matrix());
    let mut internal_origin = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#30"),
        "JointOrigin",
        30,
    );
    internal_origin.joint_origin_transform = Some(identity_matrix());
    let scopes = vec![assembly, origin.clone(), internal_origin.clone()];
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 2,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![10],
        item_record_index_offsets: vec![0],
    };
    let (features, _) = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &scopes,
            timelines: std::slice::from_ref(&timeline),
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        },
    )
    .expect("one authored datum envelope");

    let [feature] = features.as_slice() else {
        panic!("expected one projected datum feature");
    };
    assert_eq!(feature.ordinal, 0);
    assert_eq!(feature.native_ref.as_deref(), Some(origin.id.as_str()));
    assert!(matches!(
        feature.definition,
        FeatureDefinition::DatumCoordinateSystem { .. }
    ));
    assert_ne!(
        feature.native_ref.as_deref(),
        Some(internal_origin.id.as_str())
    );

    let mut directly_listed = timeline;
    directly_listed
        .item_record_indices
        .push(origin.record_index.into());
    directly_listed.item_record_index_offsets.push(0);
    let (features, _) = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &scopes,
            timelines: std::slice::from_ref(&directly_listed),
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        },
    )
    .expect("directly listed datum target");
    let [feature] = features.as_slice() else {
        panic!("expected one directly listed datum feature");
    };
    assert_eq!(feature.ordinal, 1);
}

#[test]
fn feature_projection_rejects_multiple_datum_envelope_positions() {
    let stream = "f3d:Design/BulkStream.dat";
    let envelope = |record_index| {
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{record_index}"),
            "Assemble",
            record_index,
        );
        scope.assembly_alignment = Some(DesignAssemblyAlignment {
            angle: 0.0,
            offset: [0.0; 3],
            owner_record_indices: Vec::new(),
            value_offsets: Vec::new(),
            operand_frames: None,
            legacy_operand_carriers: None,
            solved_frame: None,
            operand_paths: None,
            axial_operand_targets: None,
            limits: None,
            joint_origin_scope_record_index: Some(20),
        });
        scope
    };
    let mut origin = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#20"),
        "JointOrigin",
        20,
    );
    origin.joint_origin_transform = Some(identity_matrix());
    let scopes = vec![envelope(10), envelope(11), origin];
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 2,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![10, 11],
        item_record_index_offsets: vec![0, 0],
    };
    let result = crate::design::feature_project::authored_scope_ordinals(
        &scopes,
        std::slice::from_ref(&timeline),
    );
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn feature_projection_rejects_a_cyclic_internal_scope_history() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut first_internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        "Base Feature",
        10,
    );
    first_internal.history_state_id = Some(1);
    first_internal.previous_history_state_id = Some(2);
    let mut second_internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#20"),
        "Base Feature",
        20,
    );
    second_internal.history_state_id = Some(2);
    second_internal.previous_history_state_id = Some(1);
    let mut consumer =
        DesignParameterScope::empty(&format!("{stream}:design-parameter-scope#30"), "Move", 30);
    consumer.history_state_id = Some(3);
    consumer.previous_history_state_id = Some(1);
    let scopes = vec![first_internal, second_internal, consumer];
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 2,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![30],
        item_record_index_offsets: vec![0],
    };
    let result = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &scopes,
            timelines: std::slice::from_ref(&timeline),
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        },
    );
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn feature_projection_does_not_invent_an_ambiguous_internal_dependency() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut predecessor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        "Extrude",
        100,
    );
    predecessor.history_state_id = Some(7);
    let internal = |record_index| {
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{record_index}"),
            "Base Feature",
            record_index,
        );
        scope.history_state_id = Some(8);
        scope.previous_history_state_id = Some(7);
        scope
    };
    let mut successor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        "Fillet",
        200,
    );
    successor.history_state_id = Some(9);
    successor.previous_history_state_id = Some(8);
    let scopes = vec![predecessor, internal(150), internal(160), successor.clone()];
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 2,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![100, 200],
        item_record_index_offsets: vec![0, 0],
    };
    let (features, _) = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &scopes,
            timelines: std::slice::from_ref(&timeline),
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        },
    )
    .expect("ambiguous internal state chain remains unresolved");

    let successor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(successor.id.as_str()))
        .expect("projected successor");
    assert!(successor.dependencies.is_empty());
}

#[test]
fn timeline_less_feature_family_uses_complete_family_ordinals() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut first = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        "Extrude",
        100,
    );
    first.feature_ordinal = 1;
    let mut second = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        "Extrude",
        200,
    );
    second.feature_ordinal = 2;
    let scopes = [second.clone(), first.clone()];
    let ordinals = crate::design::feature_project::authored_scope_ordinals(&scopes, &[])
        .expect("complete family ordinals carry exact order");
    assert_eq!(ordinals[&(stream, first.record_index)], 0);
    assert_eq!(ordinals[&(stream, second.record_index)], 1);

    let mut mixed = second;
    mixed.kind = "Fillet".into();
    let error = crate::design::feature_project::authored_scope_ordinals(&[first, mixed], &[])
        .expect_err("mixed families have no timeline-independent total order");
    assert!(error
        .to_string()
        .contains("no complete authored timeline order"));
}

#[test]
fn authored_scope_validation_orders_independent_streams_separately() {
    let mut first = DesignParameterScope::empty(
        "f3d:DesignA/BulkStream.dat:design-parameter-scope#10",
        "Extrude",
        10,
    );
    first.feature_ordinal = 1;
    let mut second = DesignParameterScope::empty(
        "f3d:DesignB/BulkStream.dat:design-parameter-scope#10",
        "Fillet",
        10,
    );
    second.feature_ordinal = 1;
    let scopes = [first, second];

    let ordinals = crate::design::feature_project::authored_scope_ordinals_per_stream(&scopes, &[])
        .expect("independent stream-local orders");
    assert_eq!(ordinals.len(), 2);
    assert!(ordinals.values().all(|ordinal| *ordinal == 0));
    assert!(matches!(
        crate::design::feature_project::authored_scope_ordinals(&scopes, &[]),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
fn move_matrix_decomposes_to_translation_and_axis_angle() {
    let angle = std::f64::consts::PI / 3.0;
    let transform: [[f64; 4]; 4] = [
        [angle.cos(), 0.0, angle.sin(), -14.0],
        [0.0, 1.0, 0.0, 2.0],
        [-angle.sin(), 0.0, angle.cos(), 9.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rotation = crate::design::feature_project::matrix_axis_angle(&transform)
        .expect("nonidentity rotation");
    assert!((rotation.angle.0 - angle).abs() <= 1.0e-12);
    assert!((rotation.direction.x - 0.0).abs() <= 1.0e-12);
    assert!((rotation.direction.y - 1.0).abs() <= 1.0e-12);
    assert!((rotation.direction.z - 0.0).abs() <= 1.0e-12);
    assert_eq!(
        crate::design::feature_project::matrix_axis_angle(
            &crate::design::decode::sketch::identity_matrix()
        ),
        None
    );
}

#[test]
fn history_state_identity_orders_cross_family_feature_dependencies() {
    let scope = |record_index, byte_offset, kind: &str, current, previous| DesignParameterScope {
        id: format!("f3d:native:scope#{record_index}"),
        byte_offset,
        class_tag: "301".into(),
        record_index,
        frame_length: 200,
        kind: kind.into(),
        kind_offset: byte_offset + 100,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        coil_placement: None,
        coil_transform: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: current,
        history_state_id_offset: byte_offset + 60,
        previous_history_state_id: previous,
        previous_history_state_id_offset: byte_offset + 120,
        reference_count_offset: byte_offset + 80,
        reference_members: Vec::new(),
        reference_member_offsets: Vec::new(),
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_plane_construction: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_construction: None,
        unclosed_construction_operand_groups: Vec::new(),
        hole_construction: None,
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        derived_instance_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: byte_offset + 200,
    };
    let predecessor = scope(12, 200, "Fillet", Some(10), Some(9));
    let successor = scope(22, 100, "Chamfer", Some(11), Some(10));
    let parameter = |owner_record_index, record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(owner_record_index),
            expression,
            "FeatureInput",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated history-ordered parameter");
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, scope_record_index| DesignParameterOwner {
        id: format!("f3d:native:owner#{record_index}"),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "292".into(),
        record_index,
        scope_record_index,
        local_ordinal: parameter_record_index,
        evaluated_value: 1.0,
        evaluated_value_offset: 0,
        parameter_record_index,
        owned_ordinal: parameter_record_index,
        variant: Some(0),
        companion_record_index: record_index + 1,
    };
    let parameters = [
        parameter(44, 45, "10 mm", "Width"),
        parameter(54, 55, "Width / 2", "Depth"),
    ];
    let owners = [owner(44, 45, 12), owner(54, 55, 22)];
    let scopes = [successor, predecessor];
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream("f3d:native", 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 1,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![12, 22],
        item_record_index_offsets: vec![0, 0],
    };
    let (features, parameters) = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &parameters,
            owners: &owners,
            scopes: &scopes,
            timelines: std::slice::from_ref(&timeline),
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        },
    )
    .expect("authored cross-family timeline");
    let predecessor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("f3d:native:scope#12"))
        .expect("predecessor feature");
    let successor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("f3d:native:scope#22"))
        .expect("successor feature");
    assert_eq!(successor.dependencies, [predecessor.id.clone()]);
    assert!(predecessor.ordinal < successor.ordinal);
    let width = parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .expect("predecessor Width parameter");
    let depth = parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("successor Depth parameter");
    assert_eq!(depth.dependencies, [width.id.clone()]);
}
