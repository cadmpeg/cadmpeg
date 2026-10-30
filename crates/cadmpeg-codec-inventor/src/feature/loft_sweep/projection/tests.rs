// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::loft_sweep::{LOFT_MODE_TYPE, SECTION_CONDITION_TYPE};
use crate::feature::loft_sweep::{SWEEP_ORIENTATION_TYPE, SWEEP_SCALING_TYPE};
use crate::feature::PmDcFeaturePropertyPayload;
use crate::feature::{
    PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
};
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_ir::ids::IdentityKey;

fn control(type_id: [u8; 16], type_value: i16, value: u16) -> PmDcFeatureProperty {
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: 29,
            header: crate::feature::tests::test_header(),
            kind: PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value,
                value,
            },
        },
        RecordTypeId::from_bytes(type_id),
        IdentityKey::try_new("synthetic").expect("segment"),
        0,
    )
}

#[test]
fn unknown_section_conditions_and_control_discriminators_do_not_admit_defaults() {
    for type_id in [
        SECTION_CONDITION_TYPE,
        LOFT_MODE_TYPE,
        SWEEP_ORIENTATION_TYPE,
        SWEEP_SCALING_TYPE,
    ] {
        assert!(zero_control(&control(type_id, 2, 0), type_id, 2));
        assert!(!zero_control(&control(type_id, 2, 1), type_id, 2));
        assert!(!zero_control(&control(type_id, 1, 0), type_id, 2));
        assert!(!zero_control(
            &control(super::super::SECTION_LIST_TYPE, 2, 0),
            type_id,
            2
        ));
    }
}

#[test]
fn unresolved_loft_section_scalars_emit_the_pinned_native_loss() {
    use crate::feature::loft_sweep::{slots::LOFT_CLASS, SECTION_TYPE};
    use crate::feature::tests::{
        reference, reference_list, test_feature, test_label, test_projection_index, test_property,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let enumeration = |family, type_value, value| PmDcFeaturePropertyKind::Enumeration {
        family,
        type_value,
        value,
    };
    let boolean = PmDcFeaturePropertyKind::Boolean {
        name: String::new(),
        name_value: 0,
        value: false,
    };
    let section = |entity| PmDcFeaturePropertyKind::LoftSection {
        entity_link: reference(entity),
        value: 0,
        condition: reference(9),
        parameters: [reference(10), reference(11)],
        map_value: 0,
        source_value: 1,
        option: reference(0),
    };
    let mut properties = vec![
        test_property(
            0,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::LoftSections,
                items: reference_list(&[7, 8]),
            },
        ),
        test_property(1, enumeration(PmDcFeatureEnumFamily::PartOperation, 5, 1)),
        test_property(2, boolean.clone()),
        test_property(3, boolean),
        test_property(4, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
        test_property(
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[100]),
            },
        ),
        test_property(6, section(100)),
        test_property(7, section(101)),
        test_property(8, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
    ];
    for (ordinal, type_id) in [
        (4, LOFT_MODE_TYPE),
        (6, SECTION_TYPE),
        (7, SECTION_TYPE),
        (8, SECTION_CONDITION_TYPE),
    ] {
        properties[ordinal].identity.type_id = RecordTypeId::from_bytes(type_id);
    }
    let source = test_feature(20, 14, &[(0, 0), (1, 1), (2, 2), (9, 3), (10, 4), (13, 5)]);
    let label = test_label(20, 0, LOFT_CLASS, &[]);
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut losses = Vec::new();
    assert!(project(
        &ctx,
        &source,
        &label,
        &index,
        FeatureFamily::Loft,
        &mut losses
    )
    .is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::FeatureLoftSectionScalarsNative.kind()
    );
}
