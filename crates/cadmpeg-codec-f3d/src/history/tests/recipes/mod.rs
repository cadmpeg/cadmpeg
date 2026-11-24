// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::records::topology::{
    construction::DesignConstructionOperandGroup,
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};

mod body_history;
mod body_selections;
mod external_body;
mod hem_carriers;
mod record_framing;
mod surface_patch;
mod work_geometry;

/// A coil scope whose body recipe operand 21 selects model body
/// `f3d:brep:body#1` through its face `f3d:brep:entity#7`.
fn direct_body_fixture() -> (
    crate::records::feature::scope::DesignParameterScope,
    &'static str,
    DesignConstructionOperandGroup,
    crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    cadmpeg_ir::topology::Body,
    cadmpeg_ir::topology::Region,
    cadmpeg_ir::topology::Shell,
    cadmpeg_ir::topology::Face,
) {
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};

    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CoilPrimitive,
        10,
    );
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id.into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let operand = crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#21".into(),
            scope_record_index: 10,
            owner: crate::records::topology::body_recipe::DesignOperandOwner::Group {
                group_record_index: 20,
                group_member_ordinal: 0,
            },
            record_index: 21,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("384".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![
                crate::records::topology::body_recipe::DesignBodyRecipeReference {
                    design_reference: 301,
                    design_reference_offset: 25,
                    form: 33,
                    form_offset: 33,
                    candidate_faces: vec![
                        FaceId::mint("f3d:brep:entity#7").expect("identity grammar")
                    ],
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: Vec::new(),
                },
            ],
            nested_record_index: 24,
            nested_record_index_offset: 38,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#23".into(),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 25,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    let body = Body {
        id: BodyId::mint("f3d:brep:body#1").expect("identity grammar"),
        kind: BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
        native_ref: None,
        unstated: None,
    };
    let region = Region::new(
        RegionId::mint("test:model:region#1").expect("identity grammar"),
        body.id.clone(),
        vec![ShellId::mint("test:model:shell#1").expect("identity grammar")],
    );
    let shell = Shell::new(
        ShellId::mint("test:model:shell#1").expect("identity grammar"),
        region.id.clone(),
    );
    let face = crate::test_support::bare_face("f3d:brep:entity#7", "test:model:shell#1");
    (scope, group_id, group, operand, body, region, shell, face)
}
