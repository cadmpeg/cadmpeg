// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignLoftConstruction;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;

fn loft_findings(section_records: Vec<u32>) -> usize {
    crate::test_support::with_decode_context(|decode| {
        let mut native = super::construction_group::native(true, false);
        native.design_parameter_scopes[0]
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Loft(Some(DesignLoftConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 25,
                    reference_target: None,
                    section_records,
                    rail_groups: Vec::new(),
                }));
            })
            .unwrap();
        let mut second = native.design_construction_operand_groups[0].clone();
        second.id = "f3d:Design/BulkStream.dat:design-construction-operand-group#102".to_owned();
        second.record_index = 102;
        native.design_construction_operand_groups.push(second);
        for group in &mut native.design_construction_operand_groups {
            group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
        }
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        let mut findings = Vec::new();
        super::super::validate_path_feature_operand_roles(&ctx, &mut findings);
        findings.len()
    })
}

#[test]
fn loft_lists_that_partition_the_groups_validate() {
    assert_eq!(loft_findings(vec![99, 101]), 0);
}

#[test]
fn loft_lists_that_do_not_partition_the_groups_give_one_finding() {
    assert_eq!(loft_findings(vec![99, 99]), 1);
}

#[test]
fn a_loft_without_lists_validates() {
    assert_eq!(loft_findings(Vec::new()), 0);
}
