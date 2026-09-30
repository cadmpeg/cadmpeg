// SPDX-License-Identifier: Apache-2.0

use crate::records::dimensions::{DesignDimensionLocus, DesignDimensionLocusGroup};
use crate::records::identity::Located;

fn locus_group_with_saturated_offsets(base: u64, count: usize) -> DesignDimensionLocusGroup {
    let loci_start = base.saturating_add(24);
    let owner_start = loci_start.saturating_add(cadmpeg_core::decode::u64_from_index(count) * 15);
    let returns_start = owner_start.saturating_add(24);
    let end = returns_start.saturating_add(cadmpeg_core::decode::u64_from_index(count) * 11).saturating_add(1);
    DesignDimensionLocusGroup {
        id: "f3d:Design/BulkStream.dat:locus-group#9".into(),
        companion_record_index: 8,
        byte_offset: base,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 9,
        frame_length: end - base,
        loci: (0..count).map(|ordinal| {
            let ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
            let start = loci_start.saturating_add(ordinal * 15);
            DesignDimensionLocus {
                returned: Located { value: 10, offset: returns_start.saturating_add(ordinal * 11).saturating_add(1) },
                geometry_record_index: 10,
                geometry_reference_offset: start.saturating_add(1),
                role: 1,
                role_offset: start.saturating_add(11),
            }
        }).collect(),
        owner_reference: 4,
        owner_reference_offset: owner_start.saturating_add(2),
        owner_role: 1,
        owner_role_offset: owner_start.saturating_add(12),
        state: 1,
        state_offset: owner_start.saturating_add(16),
        next_class_tag: "259".to_owned().try_into().unwrap(),
        next_record_index: 11,
        next_byte_offset: end,
    }
}

fn locus_findings(group: DesignDimensionLocusGroup) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|decode| {
        use crate::records::entity_header::{DesignEntityHeader, DesignEntityRegistration, DESIGN_MODULE_SKETCH};
        use crate::records::identity::{DesignEntityId, ReferenceRun};
        use crate::records::parameters::{DesignParameter, DesignParameterDraft, DesignParameterSource, DesignParameterOwner, DesignParameterOwnerWire, DesignParameterCompanion};
        let parameter = DesignParameter::try_from(DesignParameterDraft {
            id: "f3d:Design/BulkStream.dat:parameter#7".into(),
            byte_offset: 20, class_tag: "305".to_owned().try_into().unwrap(), record_index: 7,
            source_ordinal: 0,
            source: DesignParameterSource::new("Dimension".into(), Some(6), None).unwrap(),
            expression: "1".into(), expression_offset: 32, source_kind_offset: 52,
            unit: None, name: "Dimension".into(), name_offset: 82,
            evaluated_value: 1.0, evaluated_value_offset: 92,
        }).unwrap();
        let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:owner#6".into(), byte_offset: 0,
            frame_length: 99, class_tag: "268".to_owned().try_into().unwrap(), record_index: 6,
            scope_record_index: 4, local_ordinal: 0, evaluated_value: 1.0,
            evaluated_value_offset: 40, parameter_record_index: 7, owned_ordinal: 0,
            variant: None, companion_record_index: 8,
        }).unwrap();
        let companion = DesignParameterCompanion::unbound(
            "f3d:Design/BulkStream.dat:companion#8".into(), 0,
            "258".to_owned().try_into().unwrap(), 8, 6,
            std::num::NonZeroU64::new(1).unwrap(), 42,
        );
        let entity = DesignEntityHeader {
            id: "f3d:Design/BulkStream.dat:entity#4".into(), byte_offset: 0,
            entity_id: DesignEntityId::from_parts("sketch", 4),
            class_tag: "112".to_owned().try_into().unwrap(), optional_slot_present: false,
            registration: DesignEntityRegistration::new(Some(DESIGN_MODULE_SKETCH.into()), None, ReferenceRun::unlocated(Vec::new())).unwrap(),
        };
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_dimension_locus_groups.push(group);
        let mut ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        let stream = super::super::design_stream(&native.design_dimension_locus_groups[0].id);
        ctx.parameters_by_index.insert((stream, 7), &parameter);
        ctx.owners_by_index.insert((stream, 6), &owner);
        ctx.companions_by_index.insert((stream, 8), &companion);
        ctx.entities_by_suffix.insert((stream, 4), &entity);
        ctx.sketch_geometry_indices.insert((stream, 10));
        let mut findings = Vec::new();
        super::super::validate_dimension_locus_groups(&ctx, &mut findings).unwrap();
        findings
    })
}

#[test]
fn locus_group_accepts_representable_layout() {
    assert!(locus_findings(locus_group_with_saturated_offsets(100, 2)).is_empty());
}

macro_rules! overflow_case {
    ($name:ident, $base:expr, $count:expr) => {
        #[test]
        fn $name() {
            let findings = locus_findings(locus_group_with_saturated_offsets($base, $count));
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].message, "Fusion Design dimension locus group has an invalid counted frame or geometry link");
        }
    };
}

overflow_case!(locus_group_rejects_overflowed_origin, u64::MAX, 1);
overflow_case!(locus_group_rejects_overflowed_geometry_slot, u64::MAX - 24, 1);
overflow_case!(locus_group_rejects_overflowed_role_slot, u64::MAX - 34, 1);
overflow_case!(locus_group_rejects_overflowed_locus_stride, u64::MAX - 24, 2);
overflow_case!(locus_group_rejects_overflowed_owner_origin, u64::MAX - 38, 1);
overflow_case!(locus_group_rejects_overflowed_owner_reference, u64::MAX - 40, 1);
overflow_case!(locus_group_rejects_overflowed_owner_role, u64::MAX - 49, 1);
overflow_case!(locus_group_rejects_overflowed_state, u64::MAX - 53, 1);
overflow_case!(locus_group_rejects_overflowed_return_origin, u64::MAX - 60, 1);
overflow_case!(locus_group_rejects_overflowed_return_slot, u64::MAX - 63, 1);
overflow_case!(locus_group_rejects_overflowed_return_stride, u64::MAX - 87, 2);
overflow_case!(locus_group_rejects_overflowed_end_stride, u64::MAX - 93, 2);
