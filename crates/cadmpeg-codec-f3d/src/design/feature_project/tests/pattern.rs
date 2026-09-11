// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::project_rectangular_pattern_scalars;
use crate::records::feature::{DesignParameterScope, DesignRectangularPatternConstruction};
use crate::records::topology::DesignOperandRole;
use crate::records::topology::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
};
use cadmpeg_ir::features::{
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, PatternSeed,
    PatternTransform,
};

const EPS_SPACING: f64 = 1.0e-12;

fn group(
    scope_record_index: u32,
    record_index: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: format!(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"
            ),
            scope_record_index,
            scope_reference_ordinal: 1,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("313".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: record_index + 1,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 99,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::DesignClassTag::try_from("263".to_owned()).unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn rectangular_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 1,
                    u_extent: 10.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

fn assert_linear_seed(definition: FeatureDefinition, expected_seed: PatternSeed) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("rectangular pattern definition");
    };
    assert_eq!(seeds, vec![expected_seed]);
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(direction.is_none());
    assert!((spacing.get() - 50.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    assert!(second.is_none());
}

#[test]
fn rectangular_pattern_seed_role_selects_body_or_face() {
    let body_scope = rectangular_scope();
    let body_group = group(10, 20, DesignOperandRole::BODIES_B);
    let body_definition = project_rectangular_pattern_scalars(&body_scope, &[body_group], &[])
        .expect("body rectangular pattern");
    assert_linear_seed(
        body_definition,
        PatternSeed::Bodies(BodySelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
        )),
    );

    let face_scope = rectangular_scope();
    let face_group = group(10, 30, DesignOperandRole::BODIES_A);
    let face_definition = project_rectangular_pattern_scalars(&face_scope, &[face_group], &[])
        .expect("face rectangular pattern");
    assert_linear_seed(
        face_definition,
        PatternSeed::Faces(FaceSelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
        )),
    );
}
