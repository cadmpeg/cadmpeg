// SPDX-License-Identifier: Apache-2.0
use crate::test_support::native_test::f3d_native_mut;

#[test]
fn validation_accepts_identity_insert_frame_and_rejects_wrong_pair() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            assembly_features::DesignComponentInsertConstruction, scope::DesignParameterScope,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#100");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
        169,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 100;
            draft.class_tag =
                crate::records::references::DesignClassTag::try_from("357".to_owned()).unwrap();
            draft.frame_length = 272;
            draft.kind_offset = 252;
            draft.reference_count_offset = 229;
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![167],
                vec![234],
                "reference_members",
            )
            .unwrap();
            draft.paired_class_tag =
                crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
            draft.paired_byte_offset = 372;
            draft.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
            draft.feature_ordinal_offset = 284;

            draft.previous_history_state_id_offset = None;
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignComponentInsertConstruction {
            relation_record_index: 167,
            carrier_record_index: 166,
            occurrence_identity: Some(17),
            neutron_role: "cccccccc-dddd-eeee-ffff-000000000000".into(),
            neutron_role_offset: 259,
            placement: None,
        });
    }

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#167"),
                record_index: 167,
                class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned())
                    .unwrap(),
                byte_offset: 0,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#169"),
                record_index: 169,
                class_tag: crate::records::references::DesignClassTag::try_from("357".to_owned())
                    .unwrap(),
                byte_offset: 100,
            },
        ]);
        native.design_parameter_scopes.push(scope);
    }

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(!findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));

    f3d_native_mut(&mut ir).design_parameter_scopes[0].paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    assert!(findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));
}
