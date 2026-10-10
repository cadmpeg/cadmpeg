// SPDX-License-Identifier: Apache-2.0
//! Native inputs for the direct query owner's controls.

use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use crate::records::topology::construction::{DesignConstructionOperandFlag, DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft};
use crate::records::topology::extrude_selection::DesignOperandRole;

pub(super) fn history_scope() -> DesignParameterScope {
    let mut scope = super::super::scope();
    scope.try_edit(|draft| {
        draft.previous_history_state_id = Some(7);
        draft.layout_fixture_tail();
    }).unwrap();
    scope
}

pub(super) fn full_round_group() -> DesignConstructionOperandGroup {
    let mut group = super::super::group(77, 0, 100, vec![101], DesignOperandRole::BODIES_A);
    group.frame = DesignConstructionOperandGroupFrame::try_from(DesignConstructionOperandGroupFrameDraft {
        member_count_offset: 0,
        auxiliary_records: Vec::new(),
        auxiliary_paths: Vec::new(),
        trailing_records: vec![Located { value: 102, offset: 0 }],
        trailing_transforms: Vec::new(),
        trailing_dual_transforms: Vec::new(),
        trailing_flags: vec![DesignConstructionOperandFlag {
            record_index: 102,
            byte_offset: 0,
            class_tag: "262".to_owned().try_into().unwrap(),
            value: true,
            value_offset: 0,
        }],
        opaque_index: 1,
        opaque_index_offset: 18,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 22,
        variant: false,
    }).unwrap();
    group
}

pub(super) fn legacy_carrier() -> crate::records::topology::entity_selection::DesignLoftLegacyBodyCarrier {
    crate::records::topology::entity_selection::DesignLoftLegacyBodyCarrier {
        id: "f3d:Design/BulkStream.dat:loft-legacy-body#150".into(),
        scope_record_index: 77,
        record_index: 150,
        byte_offset: 0,
        class_tag: "322".to_owned().try_into().unwrap(),
        owner_scope_record_index_offset: 0,
        member: 101,
        member_offset: 0,
        member_count_offset: 0,
        opaque_index: std::num::NonZeroU8::new(1).unwrap(),
        opaque_index_offset: 0,
        opaque_scalar: cadmpeg_ir::scalar::FiniteReal::new(0.0).unwrap(),
        opaque_scalar_offset: 0,
        repeated_opaque_index_offset: 0,
        next_next_record_index: 152,
        next_next_reference_offset: 0,
        flags_offset: 0,
        next_record_index: 151,
        next_reference_offset: 0,
        trailing_scope_reference_offset: None,
        paired_class_tag: "262".to_owned().try_into().unwrap(),
        paired_byte_offset: 0,
    }
}

pub(super) fn ruled_parameters() -> ([crate::records::parameters::DesignParameterOwner; 2], [crate::records::parameters::DesignParameter; 2]) {
    use crate::records::parameters::{DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire, DesignParameterSource};
    let owner = |record: u32, parameter: u32| DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("f3d:Design/BulkStream.dat:owner#{record}"),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "000".to_owned().try_into().unwrap(),
        record_index: record,
        scope_record_index: 77,
        local_ordinal: 0,
        evaluated_value: 0.0,
        evaluated_value_offset: 40,
        parameter_record_index: parameter,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: record + 1,
    }).unwrap();
    let parameter = |record: u32, kind: &str, unit: &str| DesignParameter::try_from(DesignParameterDraft::<String> {
        id: format!("f3d:Design/BulkStream.dat:parameter#{record}"),
        byte_offset: 0,
        class_tag: "000".to_owned().try_into().unwrap(),
        record_index: record,
        source_ordinal: 0,
        source: DesignParameterSource::new::<String>(kind.into(), Some(0), None).unwrap(),
        expression: "1".into(),
        expression_offset: 40,
        source_kind_offset: 60,
        unit: Some(crate::records::identity::RecordedValue { value: unit.into(), offset: 70 }),
        name: kind.into(),
        name_offset: 80,
        evaluated_value: 1.0,
        evaluated_value_offset: 90,
    }).unwrap();
    ([owner(90, 89), owner(93, 92)], [parameter(89, "ruledDistance", "mm"), parameter(92, "ruledAngle", "rad")])
}
