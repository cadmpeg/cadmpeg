// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::default_trait_access,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::prelude::*;
use crate::design::feature_project::project_fixed_revolve_with_entities;
use crate::records::{
    DesignConstructionOperandGroupFrame, DesignEntitySelectionOperand,
    DesignPathFeatureConstruction,
};

fn group(
    stream: &str,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    member: u32,
    role: u64,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup {
        id: format!("{stream}:construction-group#{record_index}"),
        scope_record_index,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: "264".into(),
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
        role,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "261".into(),
        paired_byte_offset: 0,
    }
}

fn profile_operand() -> DesignSketchProfileOperand {
    DesignSketchProfileOperand {
        scope_reference_ordinal: 2,
        record_index: 11,
        byte_offset: 0,
        class_tag: "308".into(),
        asset_id: "11111111-2222-4333-8444-555555555555".into(),
        asset_id_offset: 0,
        entity_id: "0_7".into(),
        entity_suffix: 7,
        entity_reference_offset: 0,
        region_selection: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: 0,
    }
}

fn placement(stream: &str, record_index: u32) -> DesignSketchPlacement {
    DesignSketchPlacement {
        id: format!("{stream}:placement#{record_index}"),
        scope_record_index: None,
        entity_id: "0_7".into(),
        entity_suffix: 7,
        visibility: None,
        byte_offset: 0,
        class_tag: "300".into(),
        record_index,
        frame_length: 0,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "260".into(),
        paired_byte_offset: 0,
        member_run_head: false,
    }
}

fn axis_selection(stream: &str) -> DesignEntitySelectionOperand {
    DesignEntitySelectionOperand {
        id: format!("{stream}:entity-selection#20"),
        scope_record_index: 5,
        group_record_index: 12,
        group_member_ordinal: 0,
        record_index: 20,
        byte_offset: 0,
        class_tag: "375".into(),
        asset_id: "11111111-2222-4333-8444-555555555555".into(),
        asset_id_offset: 0,
        context_id: "66666666-7777-4888-8999-aaaaaaaaaaaa".into(),
        context_id_offset: 0,
        identity_record_index: 21,
        identity_record_offset: 0,
        primary_identity: 7,
        primary_identity_offset: 0,
        secondary_identity: Some(42),
        secondary_identity_offset: Some(0),
        curve_secondary_identity: None,
        curve_secondary_identity_offset: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 22,
        next_byte_offset: 0,
    }
}

fn axis_curve(stream: &str) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: format!("{stream}:curve#30"),
        record_index: 30,
        owner_reference: Some(7),
        class_tag: "275".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: 42,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(1.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }),
    }
}

fn revolve_scope(stream: &str) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(&format!("{stream}:scope#5"), "Revolve", 5);
    scope.reference_members = vec![10, 12];
    scope.revolve_profile = Some(profile_operand());
    scope.path_feature_construction = Some(DesignPathFeatureConstruction::Revolve {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: 0,
        angle: 1.25,
        angle_record_index: 40,
        angle_offset: 0,
        opposite_angle_record_index: None,
        opposite_angle_offset: None,
    });
    scope
}

#[test]
fn revolve_projects_a_unique_sketch_profile_and_rejects_ambiguous_placement() {
    let stream = "f3d:Design/BulkStream.dat";
    let groups = [
        group(stream, 5, 0, 10, 11, 0x0000_0041_0000_0000),
        group(stream, 5, 1, 12, 20, 0x0000_0021_0000_0000),
    ];
    let selection = axis_selection(stream);
    let curve = axis_curve(stream);
    let scope = revolve_scope(stream);
    let primary_placement = placement(stream, 7);
    let definition = project_fixed_revolve_with_entities(
        &scope,
        &groups,
        &[],
        std::slice::from_ref(&selection),
        &[],
        std::slice::from_ref(&primary_placement),
        std::slice::from_ref(&curve),
    )
    .expect("exact Revolve construction");
    let FeatureDefinition::Revolve { construction, .. } = definition else {
        panic!("expected typed Revolve definition");
    };
    assert_eq!(
        construction.profile,
        Some(ProfileRef::Sketch(neutral_sketch_id(&primary_placement)))
    );

    let mut duplicate = placement(stream, 8);
    duplicate.entity_suffix = 8;
    let definition = project_fixed_revolve_with_entities(
        &scope,
        &groups,
        &[],
        std::slice::from_ref(&selection),
        &[],
        &[primary_placement, duplicate],
        std::slice::from_ref(&curve),
    )
    .expect("exact Revolve construction with native fallback");
    let FeatureDefinition::Revolve { construction, .. } = definition else {
        panic!("expected typed Revolve definition");
    };
    assert_eq!(
        construction.profile,
        Some(ProfileRef::Native(groups[0].id.clone()))
    );
}
