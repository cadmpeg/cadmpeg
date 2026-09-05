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

use super::project_mirror;
use crate::records::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignMirrorConstruction,
    DesignParameterScope,
};
use cadmpeg_ir::features::{
    BodySelection, FaceSelection, FeatureDefinition, PatternKind, PatternSeed,
};
use cadmpeg_ir::math::{Point3, Vector3};

fn group(scope_record_index: u32, record_index: u32, role: u64) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup {
        id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
        scope_record_index,
        scope_reference_ordinal: 0,
        record_index,
        byte_offset: 0,
        class_tag: "282".into(),
        members: vec![record_index + 1],
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
        role_offset: 0,
        paired_class_tag: "261".into(),
        paired_byte_offset: 0,
    }
}

fn mirror_scope(seed_group_record_index: u32) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#10",
        crate::records::DesignFeatureKind::Mirror,
        10,
    );
    if let crate::records::DesignScopePayload::Mirror(slot)
    | crate::records::DesignScopePayload::SymetrieMiroir(slot) = &mut scope.payload
    {
        *slot = Some(DesignMirrorConstruction {
            count: 2,
            count_record_index: 11,
            count_offset: 0,
            stitch_tolerance: 0.001,
            stitch_tolerance_record_index: Some(12),
            stitch_tolerance_offset: 0,
            stitch_tolerance_scope: None,
            seed_group_record_index,
            plane_group_record_index: 30,
            seed_feature_scope_record_index: None,
            plane_scope_record_index: None,
            plane_selection_record_index: None,
            plane: Some(
                crate::records::DesignPlane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                }
                .into(),
            ),
        });
    }
    scope
}

#[test]
fn mirror_seed_role_selects_body_or_face_semantics() {
    let body_scope = mirror_scope(20);
    let body_groups = [
        group(10, 20, 0x0000_0008_0000_0000),
        group(10, 30, 0x0000_0005_0000_0000),
    ];
    let FeatureDefinition::Pattern { seeds, pattern } =
        project_mirror(&body_scope, &body_groups, &[], &[]).expect("body mirror")
    else {
        panic!("mirror projects a pattern");
    };
    assert!(matches!(pattern, PatternKind::Mirror { .. }));
    assert!(matches!(
        seeds.as_slice(),
        [PatternSeed::Bodies(BodySelection::Native(native))]
            if native == "f3d:Design/BulkStream.dat:group#20"
    ));

    let face_scope = mirror_scope(40);
    let face_groups = [
        group(10, 40, 0x0000_0004_0000_0000),
        group(10, 30, 0x0000_0005_0000_0000),
    ];
    let FeatureDefinition::Pattern { seeds, .. } =
        project_mirror(&face_scope, &face_groups, &[], &[]).expect("face mirror")
    else {
        panic!("mirror projects a pattern");
    };
    assert!(matches!(
        seeds.as_slice(),
        [PatternSeed::Faces(FaceSelection::Native(native))]
            if native == "f3d:Design/BulkStream.dat:group#40"
    ));
}
