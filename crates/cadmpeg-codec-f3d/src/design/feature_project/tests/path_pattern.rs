// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_parameter_owner;
use crate::design::feature_project::project_path_pattern;
use crate::design::test_support::parameter_owner_frame;
use crate::records::{
    feature::scope::{DesignFeatureKind, DesignParameterScope},
    identity::{Located, ReferenceRun},
    parameters::{DesignParameterOwner, DesignParameterOwnerWire},
    topology::{
        construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
        extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::features::{
    patterns::{PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const SCOPE: u32 = 315;
const SEED_GROUP: u32 = 323;
const PATH_GROUP: u32 = 333;

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#38353"),
        DesignFeatureKind::PathPattern,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            let members = [319, SEED_GROUP, 326, 331, PATH_GROUP, 336, 341, 343, 346];
            let shift = 11 * (u64::try_from(members.len()).unwrap() - 1);
            draft.reference_members = ReferenceRun::located(
                members
                    .into_iter()
                    .zip((14..).step_by(11))
                    .map(|(value, offset)| Located { value, offset })
                    .collect(),
            );
            draft.kind_offset += shift;
            draft.feature_ordinal_offset += shift;
            draft.frame_length += shift;
            draft.paired_byte_offset += shift;
        })
        .unwrap();
    scope
}

fn group(
    record_index: u32,
    scope_reference_ordinal: u32,
    role: DesignOperandRole,
    members: &[u32],
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record_index}"),
        scope_record_index: SCOPE,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned()).unwrap(),
        members: members
            .iter()
            .map(|value| Located {
                value: *value,
                offset: 0,
            })
            .collect(),
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
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
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn owner(record_index: u32, local_ordinal: u32, value: f64) -> DesignParameterOwner {
    let owner = parse_parameter_owner(&parameter_owner_frame())
        .expect("generated parameter owner is canonical")
        .into_record("Design/BulkStream.dat", 0)
        .unwrap();
    let mut wire = DesignParameterOwnerWire::from(owner);
    wire.id = format!("{STREAM}:design-parameter-owner#{record_index}");
    wire.scope_record_index = SCOPE;
    wire.local_ordinal = local_ordinal;
    wire.evaluated_value = value;
    DesignParameterOwner::try_from(wire).unwrap()
}

fn groups(seed_role: DesignOperandRole) -> Vec<DesignConstructionOperandGroup> {
    vec![
        group(SEED_GROUP, 1, seed_role, &[326]),
        group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
    ]
}

fn pattern(definition: FeatureDefinition) -> (Vec<PatternSeed>, PatternTransform) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("path pattern definition");
    };
    (seeds, pattern.definition().clone())
}

#[test]
fn path_pattern_reads_seed_path_count_and_spacing() {
    let owners = [owner(331, 0, 3.0), owner(341, 1, 3.0)];
    for (role, seed) in [
        (
            DesignOperandRole::BODIES_B,
            PatternSeed::Bodies(BodySelection::Native(format!(
                "{STREAM}:design-construction-operand-group#{SEED_GROUP}"
            ))),
        ),
        (
            DesignOperandRole::ROLE_0X10,
            PatternSeed::Faces(FaceSelection::Native(format!(
                "{STREAM}:design-construction-operand-group#{SEED_GROUP}"
            ))),
        ),
    ] {
        let projected =
            project_path_pattern(&scope(), &groups(role), &owners).expect("path pattern");
        assert!(projected.scalars_assumed);
        let (seeds, transform) = pattern(projected.definition);
        assert_eq!(seeds, vec![seed]);
        let PatternTransform::CurveDriven {
            path,
            spacing,
            count,
        } = transform
        else {
            panic!("curve-driven transform");
        };
        assert_eq!(
            path,
            Some(PathRef::Native(format!(
                "{STREAM}:design-construction-operand-group#{PATH_GROUP}"
            )))
        );
        assert!((spacing.get() - 30.0).abs() <= 1.0e-12);
        assert_eq!(count, 3);
    }
}

#[test]
fn path_pattern_without_usable_owners_keeps_an_unresolved_curve_driven_form() {
    for owners in [
        vec![owner(341, 1, 3.0)],
        vec![owner(331, 0, 2.5), owner(341, 1, 3.0)],
        vec![owner(331, 0, 3.0), owner(341, 1, 0.0)],
        vec![owner(331, 0, 3.0), owner(332, 0, 3.0), owner(341, 1, 3.0)],
    ] {
        let projected =
            project_path_pattern(&scope(), &groups(DesignOperandRole::BODIES_B), &owners)
                .expect("path pattern");
        assert!(!projected.scalars_assumed);
        let (seeds, transform) = pattern(projected.definition);
        assert_eq!(seeds.len(), 1);
        assert!(matches!(
            transform,
            PatternTransform::Unresolved {
                form: Some(cadmpeg_ir::features::patterns::PatternForm::CurveDriven)
            }
        ));
    }
}

#[test]
fn path_pattern_rejects_groups_outside_its_form() {
    let owners = [owner(331, 0, 3.0), owner(341, 1, 3.0)];
    for groups in [
        vec![group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326])],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_A, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X10, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[331]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 2, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
    ] {
        assert!(project_path_pattern(&scope(), &groups, &owners).is_none());
    }
}
