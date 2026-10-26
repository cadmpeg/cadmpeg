// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::PmDcFeaturePropertyPayload;
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
