// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

use super::{
    decode_sketch_point_companion, decode_sketch_point_record, SKETCH_CONTAINER_TYPE_GUID,
    SKETCH_POINT_COMPANION_TYPE, SKETCH_POINT_TYPE_GUID,
};
use crate::records::{
    SketchPointClosure, SketchPointCompanion, SketchPointCompanionReferenceEncoding,
    SketchPointRecordForm,
};
use std::collections::HashMap;

const POINT: u32 = 41;
const COMPANION: u32 = 43;
const OWNER: u32 = 17;
const LINE: &str = "DCA267ED-D615-4934-B64F-AD805E8003E2";
const ARC: &str = "F0130424-8B7E-4092-93C9-1CA807482534";

fn push_header(out: &mut Vec<u8>, class_tag: &str, record_index: u32) {
    out.extend_from_slice(&u32::try_from(class_tag.len()).unwrap().to_le_bytes());
    out.extend_from_slice(class_tag.as_bytes());
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn push_ascii(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn push_reference(out: &mut Vec<u8>, target: u32, type_guid: Option<&str>) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    if let Some(type_guid) = type_guid {
        push_ascii(out, type_guid);
    }
    out.extend_from_slice(&[0; 2]);
}

fn tagged_point_payload(
    version: u32,
    inline_typed: bool,
    selector: u64,
    state: u8,
    padded_paired_reference: bool,
) -> Vec<u8> {
    let mut payload = Vec::new();
    push_header(&mut payload, "257", POINT);
    payload.extend_from_slice(&[0; 9]);
    payload.push(1);
    payload.extend_from_slice(&1u32.to_le_bytes());
    push_ascii(&mut payload, "pt_tag");
    push_ascii(&mut payload, "IntrinsicMetaTypeuint64");
    payload.extend_from_slice(&500u64.to_le_bytes());
    let paired_type = inline_typed.then_some(SKETCH_POINT_COMPANION_TYPE.0);
    push_reference(&mut payload, COMPANION, paired_type);
    payload.extend(std::iter::repeat_n(0, if version == 11 { 8 } else { 7 }));
    for value in [1.25f64, -2.5, 0.25] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&selector.to_le_bytes());
    payload.push(state);
    payload.extend(std::iter::repeat_n(0, if version == 8 { 8 } else { 12 }));
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[0, 1, 0, 0, 0]);
    push_reference(&mut payload, COMPANION, paired_type);
    if padded_paired_reference {
        payload.extend_from_slice(&[0; 4]);
    }
    push_reference(
        &mut payload,
        OWNER,
        inline_typed.then_some(SKETCH_CONTAINER_TYPE_GUID),
    );
    payload
}

#[test]
fn point_record_parser_closes_every_versioned_three_coordinate_form() {
    let cases = [
        (8, false, 0, 0, false, SketchPointRecordForm::Version8),
        (10, false, 0, 1, false, SketchPointRecordForm::Version10),
        (
            10,
            true,
            0,
            1,
            false,
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference: OWNER,
            },
        ),
        (
            10,
            true,
            2,
            1,
            false,
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference: OWNER,
            },
        ),
        (
            11,
            true,
            0,
            0,
            false,
            SketchPointRecordForm::Version11InlineTyped {
                trailing_reference: OWNER,
            },
        ),
    ];
    for (version, inline_typed, selector, state, padded, expected_form) in cases {
        let decoded = decode_sketch_point_record(
            &tagged_point_payload(version, inline_typed, selector, state, padded),
            version,
        )
        .expect("synthetic point form");
        assert_eq!(decoded.record_form, expected_form);
        assert_eq!(decoded.persistent_id, Some(500));
        assert_eq!(decoded.paired_reference, COMPANION);
        assert_eq!(decoded.coordinates, [1.25, -2.5, 0.25]);
        assert_eq!(
            decoded.closure,
            Some(SketchPointClosure { selector, state })
        );
    }
    assert!(
        decode_sketch_point_record(&tagged_point_payload(10, false, 2, 1, false), 10,).is_none()
    );
    for (selector, state) in [(0, 0), (0, 1), (1, 0), (2, 1), (4, 0)] {
        for padded_paired_reference in [false, true] {
            let decoded = decode_sketch_point_record(
                &tagged_point_payload(11, false, selector, state, padded_paired_reference),
                11,
            )
            .expect("synthetic version-11 point");
            assert_eq!(
                decoded.record_form,
                SketchPointRecordForm::Version11 {
                    padded_paired_reference,
                }
            );
            assert_eq!(
                decoded.closure,
                Some(SketchPointClosure { selector, state })
            );
        }
    }
    for (selector, state) in [(1, 1), (2, 0), (4, 1), (3, 0), (0, 2)] {
        assert!(decode_sketch_point_record(
            &tagged_point_payload(11, false, selector, state, false),
            11,
        )
        .is_none());
    }
}

#[test]
fn version_zero_point_retains_its_one_flag_and_source_local_identity() {
    let mut payload = Vec::new();
    push_header(&mut payload, "257", POINT);
    payload.extend_from_slice(&[0; 10]);
    push_reference(&mut payload, COMPANION, None);
    payload.push(1);
    payload.extend_from_slice(&1.25f64.to_le_bytes());
    payload.extend_from_slice(&(-2.5f64).to_le_bytes());
    payload.extend_from_slice(&[0; 20]);
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[0; 12]);
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    push_reference(&mut payload, COMPANION, None);
    push_reference(&mut payload, OWNER, None);
    let decoded = decode_sketch_point_record(&payload, 0).expect("version-0 point");
    assert_eq!(decoded.record_form, SketchPointRecordForm::Version0);
    assert_eq!(decoded.persistent_id, None);
    assert_eq!(decoded.flags, [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(decoded.coordinates, [1.25, -2.5, 0.0]);
    assert_eq!(decoded.closure, None);
}

#[test]
fn point_companion_retains_both_prefixes_and_reference_encodings() {
    let types = HashMap::from([
        (POINT, (SKETCH_POINT_TYPE_GUID, 11, "Geometry")),
        (71, (LINE, 2, "Geometry")),
        (72, (ARC, 0, "Geometry")),
    ]);
    for prefix_present_zero in [false, true] {
        for reference_encoding in [
            SketchPointCompanionReferenceEncoding::SameSegment,
            SketchPointCompanionReferenceEncoding::InlineTyped,
        ] {
            if prefix_present_zero
                && reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped
            {
                continue;
            }
            let mut payload = Vec::new();
            push_header(&mut payload, "258", COMPANION);
            if prefix_present_zero {
                payload.extend_from_slice(&[0; 9]);
                payload.extend_from_slice(&[1, 0, 0, 0, 0]);
            } else {
                payload.extend_from_slice(&[0; 10]);
            }
            payload.extend_from_slice(&2u32.to_le_bytes());
            let inline_typed =
                reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped;
            push_reference(&mut payload, 71, inline_typed.then_some(LINE));
            push_reference(&mut payload, 72, inline_typed.then_some(ARC));
            payload.push(0);
            push_reference(
                &mut payload,
                POINT,
                inline_typed.then_some(SKETCH_POINT_TYPE_GUID),
            );
            assert_eq!(
                decode_sketch_point_companion(&payload, POINT, reference_encoding, true, &types,),
                Some(SketchPointCompanion {
                    prefix_present_zero,
                    reference_encoding,
                    incident_curves: vec![71, 72],
                })
            );
        }
    }
}
