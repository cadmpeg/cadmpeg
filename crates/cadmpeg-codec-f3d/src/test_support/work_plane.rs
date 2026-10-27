// SPDX-License-Identifier: Apache-2.0

pub(crate) fn scope(
    placement: &crate::records::placement_construction::DesignPlacementConstruction,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::WorkPlane,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![u32::try_from(
                    placement.record_index,
                )
                .unwrap()]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

pub(crate) fn placements(
) -> Vec<crate::records::placement_construction::DesignPlacementConstruction> {
    crate::test_support::placement_construction::frames()
        .iter()
        .map(|fixture| {
            let fixture: serde_json::Value = serde_json::from_str(fixture).unwrap();
            let mut placement: crate::records::placement_construction::DesignPlacementConstruction =
                serde_json::from_value(fixture["native"].clone()).unwrap();
            placement.fields.inferred_x_axis.value = 0;
            placement.fields.inferred_z_axis.value = 0;
            placement.fields.z_flip.value = 0;
            if let Some(value) = &mut placement.fields.axis_flipped {
                value.value = 0;
            }
            if let Some(value) = &mut placement.fields.use_u_direction {
                value.value = 0;
            }
            placement
        })
        .collect()
}
