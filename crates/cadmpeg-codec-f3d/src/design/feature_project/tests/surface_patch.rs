// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::DesignFeatureKind;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignScopePayloadMut;
use crate::records::feature::surface_ops::DesignPatchContinuity;
use crate::records::feature::surface_ops::DesignSurfacePatchBoundary;
use crate::records::feature::surface_ops::DesignSurfacePatchSelection;
use crate::records::identity::Located;
use crate::records::identity::ReferenceRun;
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupDraft;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::FilledSurfaceBoundaryCondition;
use cadmpeg_ir::features::PathRef;
use cadmpeg_ir::features::SurfaceBoundary;
use cadmpeg_ir::report::loss::LossNote;

const STREAM: &str = "f3d:test/Design/BulkStream.dat";

fn group(
    record_index: u32,
    scope_reference_ordinal: u32,
    members: &[u32],
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record_index}"),
        scope_record_index: 50,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
        members: (0_u64..)
            .zip(members)
            .map(|(ordinal, value)| Located {
                value: *value,
                offset: 100 + 11 * ordinal,
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
                opaque_index: 1,
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
        paired_class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn interior_scope(references: Vec<u32>, interior_groups: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::SurfacePatch,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::SurfacePatch(patch) = scope.payload_mut() {
        patch.boundaries = vec![DesignSurfacePatchBoundary {
            scope_reference_ordinal: 6,
            record_index: 300,
            is_seed_selection: true,
            continuity: DesignPatchContinuity::Tangent,
            flip: 2,
            scale: crate::test_support::real(-1.0),
            model_reference: 100,
        }];
        patch.selection = Some(DesignSurfacePatchSelection {
            settings: vec![300],
            interior_groups,
            tool_body: 102,
        });
    }
    scope
}

fn interior_groups() -> Vec<DesignConstructionOperandGroup> {
    vec![
        group(101, 0, &[201, 202], DesignOperandRole::ROLE_0X21),
        group(100, 4, &[200], DesignOperandRole::BODIES_A),
    ]
}

fn project(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> (Option<FeatureDefinition>, Vec<LossNote>) {
    let mut losses = Vec::new();
    let projected = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::project_surface_patch(
            ctx,
            scope,
            groups,
            &[],
            &[],
            &[],
            &mut losses,
        )
    })
    .unwrap();
    (projected, losses)
}

#[test]
fn interior_group_form_projects_boundary_continuity_and_interior_constraints() {
    let scope = interior_scope(vec![101, 201, 202, 102, 100, 200, 300], vec![101]);
    let groups = interior_groups();
    let (projected, losses) = project(&scope, &groups);
    let Some(FeatureDefinition::Operation(FeatureOperation::FilledSurface {
        boundary: SurfaceBoundary::Path(PathRef::Native(boundary)),
        continuity,
        merge_result: Some(false),
        interior_constraints,
        reverse_normal: None,
        ..
    })) = projected
    else {
        panic!("interior-group SurfacePatch projects a filled surface");
    };
    assert_eq!(boundary, groups[1].id);
    assert_eq!(
        interior_constraints,
        vec![PathRef::Native(groups[0].id.clone())]
    );
    let conditions = continuity.resolved().unwrap().conditions.as_slice();
    let [FilledSurfaceBoundaryCondition::Tangent {
        weight: Some(weight),
    }] = conditions
    else {
        panic!("one tangent boundary condition");
    };
    assert!((weight.get() - 0.5).abs() < f64::EPSILON);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::SurfacePatchInteriorConstraintAssumed.kind()
    );
}

#[test]
fn interior_group_form_with_an_unnamed_interior_role_group_stays_native() {
    let scope = interior_scope(vec![101, 201, 202, 102, 100, 200, 300, 103, 203], vec![101]);
    let mut groups = interior_groups();
    groups.push(group(103, 7, &[203], DesignOperandRole::ROLE_0X21));
    let (projected, losses) = project(&scope, &groups);
    assert_eq!(projected, None);
    assert!(losses.is_empty());
}

#[test]
fn interior_group_form_requires_the_prefix_to_name_each_run() {
    let groups = interior_groups();
    for (references, interior) in [
        (vec![101, 201, 202, 102, 100, 200, 300], vec![100]),
        (vec![101, 201, 202, 102, 100, 200, 300], vec![101, 101]),
        (vec![101, 202, 201, 102, 100, 200, 300], vec![101]),
        (vec![101, 201, 202, 100, 200, 300, 102], vec![101]),
        (vec![101, 201, 202, 102, 100, 200, 300, 102], vec![101]),
    ] {
        let scope = interior_scope(references.clone(), interior.clone());
        let (projected, losses) = project(&scope, &groups);
        assert_eq!(projected, None, "{references:?} {interior:?}");
        assert!(losses.is_empty());
    }
}
