// SPDX-License-Identifier: Apache-2.0
#[test]
fn chamfer_sets_name_scope_members_and_owners_of_their_namespace() {
    use crate::records::feature::chamfer_set::{
        DesignChamferSet, DesignChamferSetList, DesignChamferSetListForm, DesignChamferSetSpec,
    };
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    let findings = |spec: DesignChamferSetSpec, group_member: bool, sets: usize| {
        let mut native = super::fillet_group::radius_native(true);
        native.design_parameter_scopes[0] = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Chamfer,
            10,
        );
        super::fillet_group::add_radius_parameter(&mut native, 205, "Distance", 2.0, Some("mm"));
        super::fillet_group::add_radius_parameter(&mut native, 207, "Distance 1", 2.0, Some("mm"));
        super::fillet_group::add_radius_parameter(
            &mut native,
            209,
            "rightDistance",
            3.0,
            Some("mm"),
        );
        if group_member {
            crate::design::test_support::name_scope_references(
                &mut native.design_parameter_scopes[0],
                [100],
            );
        }
        let DesignScopePayloadMut::Chamfer(slot) = native.design_parameter_scopes[0].payload_mut()
        else {
            panic!("Chamfer fixture");
        };
        slot.sets = Some(DesignChamferSetList {
            form: DesignChamferSetListForm::Current,
            entry_count: 1,
            sets: vec![
                DesignChamferSet {
                    record_index: 11,
                    byte_offset: 500,
                    frame_length: 83,
                    group_record_index: 100,
                    spec,
                    auxiliary_record_index: 12,
                    flag: true,
                };
                sets
            ],
        });
        crate::test_support::with_decode_context(|service_ctx| {
            let ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::chamfer_sets::validate(&ctx, &mut findings);
            findings.len()
        })
    };
    let equal = |distance_owner_record_index| DesignChamferSetSpec::EqualDistance {
        distance_owner_record_index,
    };
    assert_eq!(findings(equal(204), true, 1), 0);
    assert_eq!(findings(equal(204), false, 1), 1);
    assert_eq!(findings(equal(204), true, 2), 1);
    assert_eq!(findings(equal(200), true, 1), 1);
    assert_eq!(findings(equal(999), true, 1), 1);
    let mixed = DesignChamferSetSpec::TwoDistances {
        first_owner_record_index: 206,
        second_owner_record_index: 208,
    };
    assert_eq!(findings(mixed, true, 1), 1);
}
