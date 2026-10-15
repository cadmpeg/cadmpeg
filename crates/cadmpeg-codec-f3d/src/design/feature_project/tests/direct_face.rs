// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{
    DesignOffsetFacesOperation, DesignShellOperation, DesignThickenOperation,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::CodecError;

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", kind, 12);
    match scope.payload_mut() {
        DesignScopePayloadMut::OffsetFaces(slot) => {
            *slot = Some(DesignOffsetFacesOperation {
                distance: cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap(),
                distance_record_index: 300,
                distance_offset: 0,
            });
        }
        DesignScopePayloadMut::Thicken(slot) => {
            *slot = Some(DesignThickenOperation {
                signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: 0,
            });
        }
        DesignScopePayloadMut::Shell(slot) => {
            *slot = Some(DesignShellOperation {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: 0,
                outward: true,
                outward_offset: 0,
            });
        }
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
    }))
    .unwrap()
}

fn assert_projects(projected: Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError>) {
    assert!(projected.unwrap().is_some());
}

#[test]
fn offset_faces_projects_native_group() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    assert_projects(super::super::project_offset_faces(
        &scope,
        &[],
        &[],
        std::slice::from_ref(&group),
    ));
}

#[test]
fn thicken_projects_native_group() {
    let scope = scope(DesignFeatureKind::Thicken);
    let group = group(DesignOperandRole::ROLE_0X5);
    assert_projects(super::super::project_thicken(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ));
}

#[test]
fn shell_projects_native_face_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    assert_projects(super::super::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ));
}

#[test]
fn shell_projects_native_body_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::BODIES_A);
    assert_projects(super::super::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ));
}
