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
use super::project_rectangular_pattern_scalars;
use crate::records::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignParameterScope,
    DesignRectangularPatternConstruction,
};
use cadmpeg_ir::features::{
    BodySelection, FaceSelection, FeatureDefinition, PatternKind, PatternSeed,
};

const EPS_SPACING: f64 = 1.0e-12;

fn group(scope_record_index: u32, record_index: u32, role: u64) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup {
        id: format!("f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"),
        scope_record_index,
        scope_reference_ordinal: 1,
        record_index,
        byte_offset: 0,
        class_tag: "313".into(),
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
            opaque_index: 99,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role,
        extrude_role: None,
        role_offset: 0,
        paired_class_tag: "263".into(),
        paired_byte_offset: 0,
    }
}

fn rectangular_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::DesignFeatureKind::RPattern,
        10,
    );
    scope.set_rectangular_pattern_construction(Some(DesignRectangularPatternConstruction {
        u_count: 3,
        v_count: 1,
        u_extent: 10.0,
        v_extent: 0.0,
        owner_record_indices: [11, 12, 13, 14],
        value_offsets: [101, 102, 103, 104],
        instances: None,
    }));
    scope
}

fn assert_linear_seed(definition: FeatureDefinition, expected_seed: PatternSeed) {
    let FeatureDefinition::Pattern { seeds, pattern } = definition else {
        panic!("rectangular pattern definition");
    };
    assert_eq!(seeds, vec![expected_seed]);
    let PatternKind::Linear {
        direction,
        spacing,
        count,
        second,
    } = pattern
    else {
        panic!("linear rectangular pattern");
    };
    assert!(direction.is_none());
    assert!((spacing.0 - 50.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    assert!(second.is_none());
}

#[test]
fn rectangular_pattern_seed_role_selects_body_or_face() {
    let body_scope = rectangular_scope();
    let body_group = group(10, 20, 0x0000_0008_0000_0000);
    let body_definition = project_rectangular_pattern_scalars(&body_scope, &[body_group], &[])
        .expect("body rectangular pattern");
    assert_linear_seed(
        body_definition,
        PatternSeed::Bodies(BodySelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
        )),
    );

    let face_scope = rectangular_scope();
    let face_group = group(10, 30, 0x0000_0004_0000_0000);
    let face_definition = project_rectangular_pattern_scalars(&face_scope, &[face_group], &[])
        .expect("face rectangular pattern");
    assert_linear_seed(
        face_definition,
        PatternSeed::Faces(FaceSelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
        )),
    );
}
