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

#[test]
fn edge_flange_scope_projects_a_typed_two_sided_neutral_flange() {
    use crate::records::{
        DesignBendPosition, DesignEdgeFlangeOperation, DesignParameterKind, DesignParameterScope,
        DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, SheetMetalBendPosition, SheetMetalFlangeWidth, SheetMetalHeightDatum,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#900"),
        "EdgeFlange",
        382,
    );
    scope.reference_members = vec![383, 385, 388, 393, 396, 399, 402, 404, 407, 411];
    scope.edge_flange_operation = Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices: vec![383],
        edge_group_record_indices: vec![385],
        edge_operand_record_indices: vec![388],
        aggregate_group_record_index: 404,
        aggregate_operand_record_indices: vec![407],
        height_owner_record_index: 399,
        height_extent: crate::records::DesignEdgeFlangeHeightExtent::Distance,
        angle_owner_record_index: 402,
        width_mode: None,
        width_distance_owner_record_indices: vec![393, 396],
        width_distance_owner_record_indices_by_edge: Vec::new(),
        settings_record_index: 411,
        bend_radius: 0.25,
        bend_radius_offset: 156,
        reference_side_code: 4,
        height_datum: DesignSheetMetalHeightDatum::InnerFaces,
        bend_position: DesignBendPosition::Adjacent,
    });

    let owner =
        |record_index: u32, parameter_record_index: u32| crate::records::DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "000".into(),
            record_index,
            scope_record_index: 382,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 0,
            parameter_record_index,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 0,
        };
    let parameter = |record_index: u32, source_kind: &str, unit: &str, evaluated_value: f64| {
        crate::records::DesignParameter {
            id: format!("{stream}:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            family_discriminator: None,
            family_discriminator_offset: None,
            source_ordinal: 0,
            owner_record_index: None,
            expression: String::new(),
            expression_offset: 0,
            source_kind: source_kind.into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some(unit.into()),
            unit_offset: None,
            name: source_kind.into(),
            name_offset: 0,
            evaluated_value,
            evaluated_value_offset: 0,
        }
    };
    let owners = [
        owner(393, 392),
        owner(396, 395),
        owner(399, 398),
        owner(402, 401),
    ];
    // Stored lengths are centimetres and stored angles are radians.
    let parameters = [
        parameter(392, "EdgeWidth_1", "mm", 3.0),
        parameter(395, "EdgeWidth_2", "mm", 1.5),
        parameter(398, "FlangeHeight", "mm", 2.5),
        parameter(401, "FlangeAngle", "deg", std::f64::consts::FRAC_PI_2),
    ];
    let group = crate::records::DesignConstructionOperandGroup {
        id: format!("{stream}:design-construction-operand-group#385"),
        scope_record_index: 382,
        scope_reference_ordinal: 1,
        record_index: 385,
        byte_offset: 0,
        class_tag: "000".into(),
        members: vec![388],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 0,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "000".into(),
        paired_byte_offset: 0,
    };

    let inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        scopes: &[],
        timelines: &[],
        construction_groups: std::slice::from_ref(&group),
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    let definition = crate::design::feature_project::project_edge_flange(&scope, &inputs)
        .expect("typed EdgeFlange definition");

    let FeatureDefinition::SheetMetalEdgeFlange {
        height,
        angle,
        height_datum,
        bend_position,
        width,
        bend_radius,
        ..
    } = definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    let cadmpeg_ir::features::SheetMetalFlangeHeight::Distance(height) = height else {
        panic!("expected a distance flange height");
    };
    assert!((height.0 - 25.0).abs() < 1e-12);
    assert!((angle.0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(height_datum, SheetMetalHeightDatum::InnerFaces);
    assert_eq!(bend_position, SheetMetalBendPosition::Adjacent);
    assert!((bend_radius.0 - 2.5).abs() < 1e-12);
    let SheetMetalFlangeWidth::TwoSides { first, second } = width else {
        panic!("expected a two-sided flange width");
    };
    assert!((first.0 - 30.0).abs() < 1e-12);
    assert!((second.0 - 15.0).abs() < 1e-12);

    let mut multi_scope = scope.clone();
    let mut multi_operation = multi_scope
        .edge_flange_operation
        .clone()
        .expect("single-edge operation fixture");
    multi_operation.edge_group_record_indices = vec![385, 415];
    multi_operation.edge_operand_record_indices = vec![388, 418];
    multi_operation.aggregate_operand_record_indices = vec![407, 420];
    multi_scope.edge_flange_operation = Some(multi_operation.clone());
    let mut second_group = group.clone();
    second_group.id = format!("{stream}:design-construction-operand-group#415");
    second_group.record_index = 415;
    second_group.members = vec![418];
    let multi_groups = [group, second_group];
    let multi_inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        scopes: &[],
        timelines: &[],
        construction_groups: &multi_groups,
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    let multi_definition =
        crate::design::feature_project::project_edge_flange(&multi_scope, &multi_inputs)
            .expect("typed multi-edge EdgeFlange definition");
    let FeatureDefinition::SheetMetalEdgeFlange { edges, .. } = multi_definition else {
        panic!("expected a sheet-metal edge flange");
    };
    assert_eq!(
        edges,
        cadmpeg_ir::features::EdgeSelection::Native(multi_scope.id.clone())
    );

    let mut per_edge_parameters = parameters.clone();
    per_edge_parameters[0].source_kind = "EdgeWidth".into();
    per_edge_parameters[1].source_kind = "EdgeWidth".into();
    per_edge_parameters[1].evaluated_value = 3.0;
    let mut per_edge_operation = multi_operation;
    per_edge_operation.width_mode = Some(crate::records::DesignEdgeWidthMode::SymmetricPerEdge);
    multi_scope.edge_flange_operation = Some(per_edge_operation);
    let per_edge_inputs = crate::design::feature_project::ProjectInputs {
        native: &per_edge_parameters,
        owners: &owners,
        scopes: &[],
        timelines: &[],
        construction_groups: &multi_groups,
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    let per_edge_definition =
        crate::design::feature_project::project_edge_flange(&multi_scope, &per_edge_inputs)
            .expect("equal per-edge symmetric widths project to one neutral width");
    let FeatureDefinition::SheetMetalEdgeFlange { width, .. } = per_edge_definition else {
        panic!("expected a sheet-metal edge flange");
    };
    assert_eq!(
        width,
        SheetMetalFlangeWidth::Symmetric {
            width: cadmpeg_ir::features::Length(30.0),
        }
    );
    let mut distinct_parameters = per_edge_parameters.clone();
    distinct_parameters[1].evaluated_value = 1.5;
    let distinct_inputs = crate::design::feature_project::ProjectInputs {
        native: &distinct_parameters,
        owners: &owners,
        scopes: &[],
        timelines: &[],
        construction_groups: &multi_groups,
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    assert!(
        crate::design::feature_project::project_edge_flange(&multi_scope, &distinct_inputs)
            .is_none(),
        "distinct per-edge widths must remain source-native"
    );

    let mut two_sided_per_edge_operation = multi_scope
        .edge_flange_operation
        .clone()
        .expect("per-edge width operation fixture");
    two_sided_per_edge_operation.width_mode =
        Some(crate::records::DesignEdgeWidthMode::TwoSidesPerEdge);
    multi_scope.edge_flange_operation = Some(two_sided_per_edge_operation);
    assert!(
        crate::design::feature_project::project_edge_flange(&multi_scope, &per_edge_inputs)
            .is_none(),
        "edge-local two-sided widths remain native until orientation is represented"
    );
}

#[test]
fn edge_flange_scope_projects_a_to_object_height_to_a_work_plane() {
    use crate::records::{
        DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeOperation,
        DesignParameterKind, DesignParameterScope, DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#910"),
        "EdgeFlange",
        382,
    );
    scope.edge_flange_operation = Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices: vec![383],
        edge_group_record_indices: vec![385],
        edge_operand_record_indices: vec![388],
        aggregate_group_record_index: 404,
        aggregate_operand_record_indices: vec![407],
        height_owner_record_index: 399,
        height_extent: DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index: 421,
            target_operand_record_index: 424,
            offset_owner_record_index: 430,
            reference_record_indices: [469, 470],
        },
        angle_owner_record_index: 402,
        width_mode: None,
        width_distance_owner_record_indices: Vec::new(),
        width_distance_owner_record_indices_by_edge: Vec::new(),
        settings_record_index: 411,
        bend_radius: 0.25,
        bend_radius_offset: 156,
        reference_side_code: 4,
        height_datum: DesignSheetMetalHeightDatum::OuterFaces,
        bend_position: DesignBendPosition::Inside,
    });

    let owner =
        |record_index: u32, parameter_record_index: u32| crate::records::DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "000".into(),
            record_index,
            scope_record_index: 382,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 0,
            parameter_record_index,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 0,
        };
    let parameter = |record_index: u32, source_kind: &str, unit: &str, evaluated_value: f64| {
        crate::records::DesignParameter {
            id: format!("{stream}:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            family_discriminator: None,
            family_discriminator_offset: None,
            source_ordinal: 0,
            owner_record_index: None,
            expression: String::new(),
            expression_offset: 0,
            source_kind: source_kind.into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some(unit.into()),
            unit_offset: None,
            name: source_kind.into(),
            name_offset: 0,
            evaluated_value,
            evaluated_value_offset: 0,
        }
    };
    let owners = [owner(399, 398), owner(402, 401), owner(430, 429)];
    let parameters = [
        parameter(398, "FlangeHeight", "mm", 2.5),
        parameter(401, "FlangeAngle", "deg", std::f64::consts::FRAC_PI_2),
        parameter(429, "ToObjectOffset", "mm", 1.5),
    ];

    let mut edge_group = crate::records::DesignConstructionOperandGroup {
        id: format!("{stream}:design-construction-operand-group#385"),
        scope_record_index: 382,
        scope_reference_ordinal: 1,
        record_index: 385,
        byte_offset: 0,
        class_tag: "000".into(),
        members: vec![388],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 0,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "000".into(),
        paired_byte_offset: 0,
    };
    let mut target_group = edge_group.clone();
    target_group.id = format!("{stream}:design-construction-operand-group#421");
    target_group.scope_reference_ordinal = 2;
    target_group.record_index = 421;
    target_group.members = vec![424];
    target_group.role = 0x0000_0021_0000_0000;
    edge_group.member_offsets = vec![0];

    let target_selection = crate::records::DesignEntitySelectionOperand {
        id: format!("{stream}:design-entity-selection-operand#424"),
        scope_record_index: 382,
        group_record_index: 421,
        group_member_ordinal: 0,
        record_index: 424,
        byte_offset: 0,
        class_tag: "377".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: 427,
        identity_record_offset: 0,
        primary_identity: 319,
        primary_identity_offset: 0,
        secondary_identity: None,
        secondary_identity_offset: None,
        curve_secondary_identity: None,
        curve_secondary_identity_offset: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 428,
        next_byte_offset: 0,
    };
    let mut target_scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#920"),
        "WorkPlane",
        320,
    );
    target_scope.work_plane_transform = Some([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    let groups = [edge_group, target_group];
    let target_scopes = [target_scope.clone()];
    let target_selections = [target_selection];
    let inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        scopes: &target_scopes,
        timelines: &[],
        construction_groups: &groups,
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &target_selections,
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    let definition = crate::design::feature_project::project_edge_flange(&scope, &inputs)
        .expect("typed to-object EdgeFlange definition");
    let FeatureDefinition::SheetMetalEdgeFlange { height, .. } = definition else {
        panic!("expected a sheet-metal edge flange");
    };
    let SheetMetalFlangeHeight::ToObject { target, offset } = height else {
        panic!("expected a to-object flange height");
    };
    assert_eq!(
        target,
        SheetMetalFlangeHeightTarget::Feature(crate::ids::neutral_feature_id(&target_scope))
    );
    assert_eq!(offset.0, 15.0);
}

#[test]
fn edge_flange_scope_without_a_width_parameter_keeps_its_native_form() {
    use crate::records::{
        DesignBendPosition, DesignEdgeFlangeOperation, DesignParameterScope,
        DesignSheetMetalHeightDatum,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#901"),
        "EdgeFlange",
        317,
    );
    scope.reference_members = vec![318, 320, 323, 328, 331, 334, 336, 339, 343];
    scope.edge_flange_operation = Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices: vec![318],
        edge_group_record_indices: vec![320],
        edge_operand_record_indices: vec![323],
        aggregate_group_record_index: 336,
        aggregate_operand_record_indices: vec![339],
        height_owner_record_index: 331,
        height_extent: crate::records::DesignEdgeFlangeHeightExtent::Distance,
        angle_owner_record_index: 334,
        width_mode: None,
        width_distance_owner_record_indices: vec![328],
        width_distance_owner_record_indices_by_edge: Vec::new(),
        settings_record_index: 343,
        bend_radius: 0.25,
        bend_radius_offset: 156,
        reference_side_code: 4,
        height_datum: DesignSheetMetalHeightDatum::OuterFaces,
        bend_position: DesignBendPosition::Inside,
    });

    let inputs = crate::design::feature_project::ProjectInputs {
        native: &[],
        owners: &[],
        scopes: &[],
        timelines: &[],
        construction_groups: &[],
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        placements: &[],
        body_bindings: &[],
        histories: &[],
    };
    assert!(crate::design::feature_project::project_edge_flange(&scope, &inputs).is_none());
}

#[test]
fn surface_patch_continuity_needs_every_boundary_to_agree() {
    use crate::records::{DesignParameterScope, DesignPatchContinuity, DesignSurfacePatchBoundary};
    use cadmpeg_ir::features::SurfaceContinuity;

    let boundary = |continuity: DesignPatchContinuity| DesignSurfacePatchBoundary {
        scope_reference_ordinal: 0,
        record_index: 0,
        is_seed_selection: false,
        continuity,
        flip: 2,
        scale: -1.0,
        model_reference: 0,
    };
    let scope_with = |boundaries: Vec<DesignSurfacePatchBoundary>| {
        let mut scope = DesignParameterScope::empty("f3d:test:scope#1", "SurfacePatch", 1);
        scope.surface_patch_boundaries = boundaries;
        scope
    };

    for (code, expected) in [
        (DesignPatchContinuity::Connected, SurfaceContinuity::Contact),
        (DesignPatchContinuity::Tangent, SurfaceContinuity::Tangent),
        (
            DesignPatchContinuity::Curvature,
            SurfaceContinuity::Curvature,
        ),
    ] {
        let scope = scope_with(vec![boundary(code), boundary(code)]);
        assert_eq!(
            crate::design::feature_project::surface_patch_continuity(&scope),
            Some(expected)
        );
    }

    // A patch whose boundaries impose different conditions has no single neutral
    // continuity, and one with no boundary record has none to report.
    let mixed = scope_with(vec![
        boundary(DesignPatchContinuity::Tangent),
        boundary(DesignPatchContinuity::Connected),
    ]);
    assert_eq!(
        crate::design::feature_project::surface_patch_boundary_continuities(&mixed),
        vec![SurfaceContinuity::Tangent, SurfaceContinuity::Contact]
    );
    assert!(crate::design::feature_project::surface_patch_continuity(&mixed).is_none());
    assert!(
        crate::design::feature_project::surface_patch_continuity(&scope_with(Vec::new())).is_none()
    );
    assert!(
        crate::design::feature_project::surface_patch_continuity(&scope_with(vec![boundary(
            DesignPatchContinuity::Unknown(9)
        )]))
        .is_none()
    );
    assert!(
        crate::design::feature_project::surface_patch_boundary_continuities(&scope_with(vec![
            boundary(DesignPatchContinuity::Unknown(9))
        ]))
        .is_empty()
    );
}

#[test]
fn surface_patch_projection_accepts_boundary_groups_at_either_reference_endpoint() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignParameterScope,
        DesignPatchContinuity, DesignSurfacePatchBoundary,
    };
    use cadmpeg_ir::features::{FeatureDefinition, SurfaceContinuity};

    let mut scope = DesignParameterScope::empty("f3d:test:scope#1", "SurfacePatch", 1);
    scope.frame_length = 442;
    scope.reference_members = vec![900, 100, 101, 102, 110, 111, 112, 120, 121, 122];
    scope.surface_patch_boundaries = vec![
        DesignSurfacePatchBoundary {
            scope_reference_ordinal: 3,
            record_index: 102,
            is_seed_selection: false,
            continuity: DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference: 100,
        },
        DesignSurfacePatchBoundary {
            scope_reference_ordinal: 6,
            record_index: 112,
            is_seed_selection: true,
            continuity: DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference: 110,
        },
        DesignSurfacePatchBoundary {
            scope_reference_ordinal: 9,
            record_index: 122,
            is_seed_selection: false,
            continuity: DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference: 120,
        },
    ];
    let group = |record_index, ordinal, member| DesignConstructionOperandGroup {
        id: format!("f3d:test:construction-group#{record_index}"),
        scope_record_index: scope.record_index,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 0,
        class_tag: "277".into(),
        members: vec![member],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0004_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "260".into(),
        paired_byte_offset: 0,
    };
    let shifted_groups = [group(100, 1, 101), group(110, 4, 111), group(120, 7, 121)];
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &scope,
            &shifted_groups,
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            continuity: Some(SurfaceContinuity::Contact),
            ref boundary_continuities,
            ..
        }) if boundary_continuities == &[
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
        ]
    ));

    scope.reference_members = vec![100, 101, 102, 110, 111, 112, 120, 121, 122, 900];
    for (boundary, ordinal) in scope.surface_patch_boundaries.iter_mut().zip([2_u32, 5, 8]) {
        boundary.scope_reference_ordinal = ordinal;
    }
    let endpoint_groups = [group(100, 0, 101), group(110, 3, 111), group(120, 6, 121)];
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &scope,
            &endpoint_groups,
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            continuity: Some(SurfaceContinuity::Contact),
            ref boundary_continuities,
            ..
        }) if boundary_continuities == &[
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
        ]
    ));
}

#[test]
fn hem_scope_projects_each_decoded_owner_layout() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignHemOperation,
        DesignHemParameterOwners, DesignParameter, DesignParameterKind, DesignParameterOwner,
        DesignParameterScope,
    };
    use cadmpeg_ir::features::{FeatureDefinition, SheetMetalHemDirection, SheetMetalHemForm};

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let owner = |scope_record_index: u32,
                 record_index: u32,
                 parameter_record_index: u32|
     -> DesignParameterOwner {
        DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "000".into(),
            record_index,
            scope_record_index,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 0,
            parameter_record_index,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 0,
        }
    };
    let parameter =
        |record_index: u32, source_kind: &str, unit: &str, value: f64| DesignParameter {
            id: format!("{stream}:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index,
            family_discriminator: None,
            family_discriminator_offset: None,
            source_ordinal: 0,
            owner_record_index: None,
            expression: String::new(),
            expression_offset: 0,
            source_kind: source_kind.into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some(unit.into()),
            unit_offset: None,
            name: source_kind.into(),
            name_offset: 0,
            evaluated_value: value,
            evaluated_value_offset: 0,
        };
    let group = |scope_record_index: u32, record_index: u32, member: u32, role: u64| {
        DesignConstructionOperandGroup {
            id: format!("{stream}:design-construction-operand-group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal: 0,
            record_index,
            byte_offset: 0,
            class_tag: "000".into(),
            members: vec![member],
            lost_edge_references: Vec::new(),
            member_offsets: vec![0],
            frame: DesignConstructionOperandGroupFrame {
                member_count_offset: 0,
                auxiliary_record_indices: Vec::new(),
                auxiliary_record_offsets: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_record_indices: Vec::new(),
                trailing_record_offsets: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 0,
                opaque_index_offset: 0,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 0,
                variant: false,
            },
            role,
            extrude_role: None,
            extrude_face_role: None,
            role_offset: 0,
            paired_class_tag: "000".into(),
            paired_byte_offset: 0,
        }
    };
    let operation = |parameter_owners| DesignHemOperation {
        edge_wrapper_record_index: 708,
        edge_group_record_index: 710,
        edge_operand_record_index: 713,
        aggregate_group_record_index: 717,
        aggregate_operand_record_index: 720,
        parameter_owners,
        settings_record_index: 724,
        bend_radius: 0.25,
        bend_radius_offset: 100,
        form_code: 3,
        direction_code: 1,
        direction_reversal_byte: 0,
        reference_side_code: 4,
    };
    let project = |record_index: u32,
                   operation: DesignHemOperation,
                   owners: Vec<DesignParameterOwner>,
                   parameters: Vec<DesignParameter>| {
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{record_index}"),
            "Hem",
            record_index,
        );
        scope.hem_operation = Some(operation);
        let groups = vec![
            group(record_index, 710, 713, 0x0000_0008_0000_0000),
            group(record_index, 717, 720, 0x0000_0043_0000_0000),
        ];
        let inputs = crate::design::feature_project::ProjectInputs {
            native: &parameters,
            owners: &owners,
            scopes: &[],
            timelines: &[],
            construction_groups: &groups,
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            placements: &[],
            body_bindings: &[],
            histories: &[],
        };
        crate::design::feature_project::project_hem(&scope, &inputs).expect("typed Hem definition")
    };

    let gap_length = project(
        900,
        operation(DesignHemParameterOwners::GapLength {
            gap_owner_record_index: 901,
            length_owner_record_index: 902,
        }),
        vec![owner(900, 901, 903), owner(900, 902, 904)],
        vec![
            parameter(903, "HemGap", "mm", 0.02),
            parameter(904, "HemLength", "mm", 10.0),
        ],
    );
    let rolled = project(
        910,
        operation(DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index: 911,
            angle_owner_record_index: 912,
        }),
        vec![owner(910, 911, 913), owner(910, 912, 914)],
        vec![
            parameter(913, "HemRadius", "mm", 0.5),
            parameter(914, "HemAngle", "deg", std::f64::consts::FRAC_PI_2),
        ],
    );
    let teardrop = project(
        920,
        operation(DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index: 921,
            length_owner_record_index: 922,
            radius_owner_record_index: 923,
        }),
        vec![
            owner(920, 921, 924),
            owner(920, 922, 925),
            owner(920, 923, 926),
        ],
        vec![
            parameter(924, "HemGap", "mm", 0.25),
            parameter(925, "HemLength", "mm", 10.0),
            parameter(926, "HemRadius", "mm", 0.5),
        ],
    );

    let FeatureDefinition::SheetMetalHem {
        form,
        direction,
        bend_radius,
        ..
    } = gap_length
    else {
        panic!("expected a gap-length Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::GapLength {
            gap: cadmpeg_ir::features::Length(0.2),
            length: cadmpeg_ir::features::Length(100.0),
        }
    );
    assert_eq!(direction, SheetMetalHemDirection::Unresolved);
    assert_eq!(bend_radius, cadmpeg_ir::features::Length(2.5));

    let FeatureDefinition::SheetMetalHem { form, .. } = rolled else {
        panic!("expected a rolled Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::Rolled {
            radius: cadmpeg_ir::features::Length(5.0),
            angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        }
    );

    let FeatureDefinition::SheetMetalHem { form, .. } = teardrop else {
        panic!("expected a teardrop Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::Teardrop {
            gap: cadmpeg_ir::features::Length(2.5),
            length: cadmpeg_ir::features::Length(100.0),
            radius: cadmpeg_ir::features::Length(5.0),
        }
    );
}
