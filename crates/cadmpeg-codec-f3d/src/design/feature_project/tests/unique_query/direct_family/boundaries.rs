// SPDX-License-Identifier: Apache-2.0
//! Actual shape and binding callers exercise their unique-source phases.

use super::{assert_refusal, fixtures};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::topology::extrude_selection::DesignOperandRole;

#[test]
fn direct_unique_base_flange_group_source_preserves_original_refusal() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        DesignFeatureKind::BaseFlange,
        77,
    );
    let DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() else {
        panic!("BaseFlange payload");
    };
    *slot = Some(crate::records::feature::scope::DesignBaseFlangeScope {
        base_flange_operation: Some(
            crate::records::feature::sheet_metal::DesignBaseFlangeOperation {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(0.25).unwrap(),
                thickness_offset: 0,
                profile_group_record_index: 100,
                profile_record_index: 101,
                thickness_record_index: 90,
                settings_record_index: 91,
            },
        ),
        base_flange_profile: None,
    });
    let groups = [super::super::group(
        77,
        0,
        100,
        vec![101],
        DesignOperandRole::PROFILE,
    )];
    assert_refusal("visit F3D base flange group source", |ctx| {
        super::super::super::super::project_base_flange(ctx, &scope, &groups, &[])
    });
}

#[test]
fn direct_unique_ruled_surface_group_source_preserves_original_refusal() {
    let mut scope = super::super::scope();
    let operation = crate::records::feature::surface_ops::DesignRuledSurfaceOperation {
        method: crate::records::feature::surface_ops::DesignRuledSurfaceMethod::Tangent,
        method_offset: 0,
        corner: crate::records::feature::surface_ops::DesignRuledSurfaceCorner::Rounded,
        corner_offset: 0,
        alternate_face: false,
        alternate_face_offset: 0,
        angle_owner_record_index: 93,
        distance_owner_record_index: 90,
        edge_group_record_indices: vec![100],
        auxiliary_record_indices: Vec::new(),
        direction_entity_id: None,
    };
    scope
        .try_edit(|draft| {
            draft.payload =
                crate::records::feature::scope::DesignScopePayload::SurfaceRuled(operation);
        })
        .unwrap();
    let groups = [super::super::group(
        77,
        0,
        100,
        vec![101],
        DesignOperandRole::BODIES_B,
    )];
    let (owners, parameters) = fixtures::ruled_parameters();
    assert_refusal("visit F3D ruled surface edge group source", |ctx| {
        super::super::super::super::project_ruled_surface(
            &scope,
            &owners,
            &parameters,
            &groups,
            &[],
            &[],
            ctx,
        )
    });
}

#[test]
fn direct_unique_legacy_loft_body_group_source_preserves_original_refusal() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        DesignFeatureKind::Loft,
        77,
    );
    let DesignScopePayloadMut::Loft(slot) = scope.payload_mut() else {
        panic!("Loft payload");
    };
    *slot = Some(
        crate::records::feature::path_features::DesignLoftConstruction {
            operation: crate::records::feature::extrude::DesignExtrudeOperation::Join,
            operation_offset: 0,
        },
    );
    let groups = [super::super::group(
        77,
        1,
        100,
        vec![101],
        DesignOperandRole::BODIES_B,
    )];
    let carriers = [fixtures::legacy_carrier()];
    assert_refusal("visit F3D legacy Loft body group source", |ctx| {
        super::super::super::super::project_fixed_loft(
            &scope,
            &groups,
            &carriers,
            &[],
            &[],
            &[],
            ctx,
        )
    });
}

#[test]
fn direct_unique_sketch_binding_end_probe_preserves_original_refusal_and_feature() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        DesignFeatureKind::Sketch,
        77,
    );
    let (mut features, _) = crate::test_support::with_decode_context(|ctx| {
        super::super::super::project_single_scope_with_context(ctx, &scope)
    })
    .unwrap();
    let original = features.clone();
    assert_refusal("visit F3D sketch binding placement source", |ctx| {
        super::super::super::super::bind_sketch_feature_geometry(
            ctx,
            &mut features,
            std::slice::from_ref(&scope),
            &[],
            &[],
            &[],
        )
    });
    assert_eq!(features, original);
}
