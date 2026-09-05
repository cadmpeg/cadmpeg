// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::prelude::*;
use crate::design::decode::scopes::exact_derived_instance_construction;
use crate::layout::{
    derived_instance_relation_310_57 as relation_310, derived_instance_scope_279_261 as scope_279,
};
use crate::records::{DesignComponentOccurrence, DesignParameterScope};

const COMPONENT: &str = "3ad5b67c-2bc5-4ccd-bac9-26ac75616116";
const OCCURRENCE: &str = "f867facf-edec-4109-9553-b3703c4e0caf";

#[test]
fn derived_instance_requires_exact_relation_carrier_and_transform_join() {
    let (mut bytes, mut scope, occurrence) = fixture();
    let records = IndexedRecordOffsets::build(&bytes);

    let construction = exact_derived_instance_construction(
        &bytes,
        &records,
        &scope,
        std::slice::from_ref(&occurrence),
    )
    .expect("exact DerivedInstance construction");
    assert_eq!(construction.reference_record_index, 305);
    assert_eq!(construction.relation_record_index, 383);
    assert_eq!(construction.carrier_record_index, 382);
    assert_eq!(construction.component_guid, COMPONENT);
    assert_eq!(construction.occurrence_guid, OCCURRENCE);
    assert_eq!(
        construction.transform,
        occurrence.transform().as_ref().copied().unwrap().value
    );
    assert_eq!(
        construction.transform_offset,
        425 + scope_279::TRANSFORM as u64
    );

    scope.paired_class_tag = "262".into();
    assert!(exact_derived_instance_construction(
        &bytes,
        &records,
        &scope,
        std::slice::from_ref(&occurrence),
    )
    .is_none());

    scope.paired_class_tag = "261".into();
    bytes[425 + scope_279::TRANSFORM + 6] = 0;
    let records = IndexedRecordOffsets::build(&bytes);
    assert!(exact_derived_instance_construction(
        &bytes,
        &records,
        &scope,
        std::slice::from_ref(&occurrence),
    )
    .is_none());
}

fn fixture() -> (Vec<u8>, DesignParameterScope, DesignComponentOccurrence) {
    const RELATION_AT: usize = 357;
    const RELATION_PAIRED_AT: usize = RELATION_AT + relation_310::LEN;
    const SCOPE_AT: usize = RELATION_PAIRED_AT + 11;
    const SCOPE_PAIRED_AT: usize = SCOPE_AT + scope_279::LEN;

    let mut bytes = vec![0; SCOPE_PAIRED_AT + 11];
    header(&mut bytes, 0, b"380", 382);
    header(&mut bytes, RELATION_AT, b"310", 383);
    header(&mut bytes, RELATION_PAIRED_AT, b"261", 384);
    header(&mut bytes, SCOPE_AT, b"279", 385);
    header(&mut bytes, SCOPE_PAIRED_AT, b"261", 385);

    bytes[RELATION_AT + relation_310::CARRIER_MARKER] = 1;
    bytes[RELATION_AT + relation_310::CARRIER_RECORD_INDEX
        ..RELATION_AT + relation_310::CARRIER_RECORD_INDEX + 4]
        .copy_from_slice(&382u32.to_le_bytes());
    bytes[RELATION_AT + relation_310::MIDDLE_MARKER] = 1;
    bytes[RELATION_AT + relation_310::MIDDLE_RECORD_INDEX
        ..RELATION_AT + relation_310::MIDDLE_RECORD_INDEX + 4]
        .copy_from_slice(&384u32.to_le_bytes());
    bytes[RELATION_AT + relation_310::SCOPE_MARKER] = 1;
    bytes[RELATION_AT + relation_310::SCOPE_RECORD_INDEX
        ..RELATION_AT + relation_310::SCOPE_RECORD_INDEX + 4]
        .copy_from_slice(&385u32.to_le_bytes());

    bytes[SCOPE_AT + scope_279::REFERENCE_MARKER] = 1;
    bytes[SCOPE_AT + scope_279::REFERENCE_RECORD_INDEX
        ..SCOPE_AT + scope_279::REFERENCE_RECORD_INDEX + 4]
        .copy_from_slice(&305u32.to_le_bytes());
    bytes[SCOPE_AT + scope_279::REFERENCE_COUNT..SCOPE_AT + scope_279::REFERENCE_COUNT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[SCOPE_AT + scope_279::RELATION_REFERENCE] = 1;
    bytes[SCOPE_AT + scope_279::RELATION_REFERENCE + 1
        ..SCOPE_AT + scope_279::RELATION_REFERENCE + 5]
        .copy_from_slice(&383u32.to_le_bytes());
    let transform = identity_matrix();
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = SCOPE_AT + scope_279::TRANSFORM + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#425",
        crate::records::DesignFeatureKind::DerivedInstance,
        385,
    );
    scope.byte_offset = SCOPE_AT as u64;
    scope.class_tag = "279".into();
    scope.frame_length = scope_279::LEN as u64;
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![383]);
    scope.paired_class_tag = "261".into();

    let occurrence = DesignComponentOccurrence {
        id: "f3d:Design/BulkStream.dat:design-component-occurrence#0".into(),
        class_tag: "380".into(),
        record_index: 382,
        byte_offset: 0,
        component_record_index: 305,
        component_guid: COMPONENT.into(),
        component_guid_offset: 0,
        occurrence_guid: OCCURRENCE.into(),
        occurrence_guid_offset: 0,
        placement: crate::records::DesignComponentOccurrencePlacement::Explicit {
            ordinal: std::num::NonZeroU32::MIN,
            transform: crate::records::Located { value: transform, offset: 209 },
        },
    };
    (bytes, scope, occurrence)
}

fn header(bytes: &mut [u8], at: usize, class_tag: &[u8; 3], record_index: u32) {
    bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[at + 4..at + 7].copy_from_slice(class_tag);
    bytes[at + 7..at + 11].copy_from_slice(&record_index.to_le_bytes());
}

fn identity_matrix() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
