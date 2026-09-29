// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{
    DesignOffsetFacesOperation, DesignShellOperation, DesignThickenOperation,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", kind, 12);
    match scope.payload_mut() {
        DesignScopePayloadMut::OffsetFaces(slot) => *slot = Some(DesignOffsetFacesOperation {
            distance: cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap(),
            distance_record_index: 300,
            distance_offset: 0,
        }),
        DesignScopePayloadMut::Thicken(slot) => *slot = Some(DesignThickenOperation {
            signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
            thickness_record_index: 300,
            thickness_offset: 0,
        }),
        DesignScopePayloadMut::Shell(slot) => *slot = Some(DesignShellOperation {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
            thickness_record_index: 300,
            thickness_offset: 0,
            outward: true,
            outward_offset: 0,
        }),
        _ => panic!("unsupported direct-face scope fixture"),
    }
    scope
}

fn group(role: DesignOperandRole) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:group#100",
        "scope_record_index": 12,
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [200],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    })).unwrap()
}

fn assert_retained_refusal(
    operation: &'static str,
    project: impl for<'a> Fn(Option<&'a DecodeContext<'a>>) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError>,
) {
    assert!(project(None).unwrap().is_some());
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(project(Some(&ctx)), Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == operation));
}

#[test]
fn offset_faces_native_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    assert_retained_refusal("f3d OffsetFaces native group id", |ctx| {
        super::super::project_offset_faces(ctx, &scope, &[], &[], std::slice::from_ref(&group))
    });
}

#[test]
fn thicken_native_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::Thicken);
    let group = group(DesignOperandRole::ROLE_0X5);
    assert_retained_refusal("f3d Thicken native group id", |ctx| {
        super::super::project_thicken(ctx, &scope, &[], std::slice::from_ref(&group))
    });
}

#[test]
fn shell_native_face_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    assert_retained_refusal("f3d Shell native face group id", |ctx| {
        super::super::project_shell(ctx, &scope, &[], std::slice::from_ref(&group))
    });
}

#[test]
fn shell_native_body_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::BODIES_A);
    assert_retained_refusal("f3d Shell native body group id", |ctx| {
        super::super::project_shell(ctx, &scope, &[], std::slice::from_ref(&group))
    });
}
