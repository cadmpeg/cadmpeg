// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::{
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        topology::{
            construction::DesignConstructionOperandRole, extrude_selection::DesignOperandRole,
        },
    };
    let mut native = super::construction_group::native(true, false);
    native.design_parameter_scopes.clear();
    native
        .design_parameter_scopes
        .push(DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Fillet,
            10,
        ));
    native.design_construction_operand_groups[0].operand_role =
        DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B);
    native
}

fn add_radius_parameter(
    native: &mut crate::native::F3dNative,
    record_index: u32,
    source_kind: &str,
    value: f64,
    unit: Option<&str>,
) {
    use crate::records::{
        identity::RecordedValue,
        parameters::{
            DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
            DesignParameterSource,
        },
    };
    let owner_index = record_index - 1;
    let base = 10_000 + u64::from(record_index) * 100;
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: format!("f3d:Design/BulkStream.dat:design-parameter#{record_index}"),
        byte_offset: base,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index,
        source_ordinal: 0,
        source: DesignParameterSource::new(source_kind.to_owned(), Some(owner_index), None)
            .unwrap(),
        expression: format!("{value}"),
        expression_offset: base + 12,
        source_kind_offset: base + 32,
        unit: unit.map(|unit| RecordedValue {
            value: unit.into(),
            offset: base + 52,
        }),
        name: source_kind.into(),
        name_offset: base + 62,
        evaluated_value: value,
        evaluated_value_offset: base + 72,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{owner_index}"),
        byte_offset: base - 99,
        frame_length: 99,
        class_tag: "268".to_owned().try_into().unwrap(),
        record_index: owner_index,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: value,
        evaluated_value_offset: base - 59,
        parameter_record_index: record_index,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: record_index + 1,
    })
    .unwrap();
    native.design_parameters.push(parameter);
    native.design_parameter_owners.push(owner);
}

fn radius_native(valid: bool) -> crate::native::F3dNative {
    use crate::records::topology::fillet::{DesignFilletRadiusGroup, DesignFilletRadiusLaw};
    let mut native = native();
    if valid {
        add_radius_parameter(&mut native, 201, "Radius", 2.0, Some("mm"));
    }
    native
        .design_fillet_radius_groups
        .push(DesignFilletRadiusGroup {
            id: "f3d:Design/BulkStream.dat:design-fillet-radius-group#100".into(),
            scope_record_index: 10,
            group_ordinal: 0,
            group_record_index: 100,
            edge_operand_record_indices: vec![101],
            law: DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index: 201,
            },
            tangency_weight_parameter_record_index: None,
        });
    native
}

#[test]
fn fillet_radius_valid_assignment_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = radius_native(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_fillet_radius_groups(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}

fn variable_radius_native(increasing: bool) -> crate::native::F3dNative {
    use crate::records::topology::fillet::{DesignFilletMidpoint, DesignFilletRadiusLaw};
    let mut native = radius_native(true);
    for (index, kind, value, unit) in [
        (202, "StartRadius", 0.0, Some("mm")),
        (203, "EndRadius", 0.0, Some("mm")),
        (204, "MidRadius", 1.0, Some("mm")),
        (205, "MidParams", 0.25, None),
        (206, "MidRadius", 1.0, Some("mm")),
        (207, "MidParams", if increasing { 0.75 } else { 0.1 }, None),
    ] {
        add_radius_parameter(&mut native, index, kind, value, unit);
    }
    native.design_fillet_radius_groups[0].law = DesignFilletRadiusLaw::Variable {
        start_radius_parameter_record_index: 202,
        end_radius_parameter_record_index: 203,
        middle: vec![
            DesignFilletMidpoint {
                radius_parameter_record_index: 204,
                parameter_record_index: 205,
            },
            DesignFilletMidpoint {
                radius_parameter_record_index: 206,
                parameter_record_index: 207,
            },
        ],
    };
    native
}

#[test]
fn fillet_variable_radius_midpoints_keep_order_validation() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let valid_native = variable_radius_native(true);
        let valid_ctx = super::super::Ctx::new(&ir, &valid_native, service_ctx);
        let mut valid_findings = Vec::new();
        super::super::validate_fillet_radius_groups(&valid_ctx, &mut valid_findings);
        assert!(valid_findings.is_empty());

        let invalid_native = variable_radius_native(false);
        let invalid_ctx = super::super::Ctx::new(&ir, &invalid_native, service_ctx);
        let mut invalid_findings = Vec::new();
        super::super::validate_fillet_radius_groups(&invalid_ctx, &mut invalid_findings);
        assert!(invalid_findings.iter().any(|finding| finding.message
            == "Fusion Design Fillet radius group has an invalid parameter assignment"));
    })
}
