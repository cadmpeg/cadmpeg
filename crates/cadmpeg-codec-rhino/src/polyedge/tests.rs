// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports, dead_code, clippy::disallowed_methods)]

use super::*;
use crate::test_support::test_dump::*;
use crate::test_support::{class_wrapper, crc_chunk};

const OPENNURBS_UNSET_VALUE: f64 = -1.234_321_012_343_21e308;

fn polyedge_payload_with_domains(edge_domain: [f64; 2], trim_domain: [f64; 2]) -> Vec<u8> {
    let mut segment = 1_i32.to_le_bytes().to_vec();
    segment.extend(0_i32.to_le_bytes());
    segment.extend([0_u8; 15]);
    segment.push(9);
    segment.extend(2_i32.to_le_bytes());
    segment.extend(17_i32.to_le_bytes());
    for value in [
        edge_domain[0],
        edge_domain[1],
        trim_domain[0],
        trim_domain[1],
    ] {
        segment.extend(value.to_le_bytes());
    }
    segment.push(1);
    for value in [10.0_f64, 20.0, 2.0, 6.0] {
        segment.extend(value.to_le_bytes());
    }
    let segment = crc_chunk(ANONYMOUS, &segment);
    let segment_class = [
        0x87, 0x7a, 0xf4, 0x42, 0x1b, 0x5b, 0x31, 0x4e, 0xab, 0x87, 0x46, 0x39, 0xd7, 0x83, 0x25,
        0xd6,
    ];

    let mut payload = vec![0x10];
    payload.extend(1_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 48]);
    payload.extend(2_i32.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(10.0_f64.to_le_bytes());
    payload.extend(class_wrapper(segment_class, &segment));
    payload
}

pub(crate) fn polyedge_payload() -> Vec<u8> {
    polyedge_payload_with_domains([0.0, 4.0], [1.0, 3.0])
}

#[test]
fn decodes_persistent_polyedge_segment_construction() {
    let payload = polyedge_payload();
    let decoded = crate::decode::with_expand_bytes(&payload, |expand| {
        decode(expand, 0..payload.len(), ArchiveVersion::V8)
    })
    .expect("required invariant");
    assert_eq!(decoded.parameters, [0.0, 10.0]);
    assert_eq!(
        decoded.segments[0].object_id,
        Uuid::from_wire(POLYEDGE_SEGMENT_TARGET)
    );
    assert_eq!(decoded.segments[0].component, [2, 17]);
    assert_eq!(decoded.segments[0].edge_domain, [0.0, 4.0]);
    assert_eq!(decoded.segments[0].trim_domain, [1.0, 3.0]);
    assert!(decoded.segments[0].reversed);
    assert_eq!(decoded.segments[0].domain, [10.0, 20.0]);
    assert_eq!(decoded.segments[0].proxy_domain, [2.0, 6.0]);
}

#[test]
fn accepts_empty_edge_and_trim_domains_for_a_source_curve_segment() {
    let payload =
        polyedge_payload_with_domains([OPENNURBS_UNSET_VALUE; 2], [OPENNURBS_UNSET_VALUE; 2]);
    let decoded = crate::decode::with_expand_bytes(&payload, |expand| {
        decode(expand, 0..payload.len(), ArchiveVersion::V8)
    })
    .expect("required invariant");
    assert_eq!(decoded.segments[0].edge_domain, [OPENNURBS_UNSET_VALUE; 2]);
    assert_eq!(decoded.segments[0].trim_domain, [OPENNURBS_UNSET_VALUE; 2]);
}

#[test]
fn truncating_the_segment_child_is_rejected_at_the_record_boundary() {
    // Drop the trailing bytes of the child segment record so the
    // count-framed segment loop runs past the body's proven window.
    let mut payload = polyedge_payload();
    payload.truncate(payload.len() - 16);
    assert!(crate::decode::with_expand_bytes(&payload, |expand| decode(
        expand,
        0..payload.len(),
        ArchiveVersion::V8
    ))
    .is_err());
}

#[test]
fn polyedge_segment_uuid_resolves_to_the_single_record_that_owns_it() {
    let mut scan = scan_with_objects(&polyedge_scan_objects());
    set_identity(&mut scan, 0, POLYEDGE_SEGMENT_TARGET, "target", None, true);
    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(
        polyedge_segment_parameter(&result).as_deref(),
        Some("rhino:object:record#000000")
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.starts_with("reference.")));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn polyedge_segment_uuid_that_names_no_record_is_charged_and_left_unbound() {
    let scan = scan_with_objects(&polyedge_scan_objects());
    let result = crate::decode::decode_for_test(&scan);
    assert!(polyedge_segment_parameter(&result).is_none());
    let charged = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::RhinoLossCode::ReferenceMemberUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(charged.len(), 1);
    assert_eq!(
        charged[0].code,
        crate::loss::RhinoLossCode::ReferenceMemberUnresolved.kind()
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn polyedge_segment_uuid_owned_by_two_records_is_charged_as_ambiguous() {
    let mut objects = polyedge_scan_objects();
    objects.insert(
        1,
        object_record_with_payload(
            ArchiveVersion::V5,
            1,
            POINT_CLASS,
            &point_payload([4.0, 5.0, 6.0]),
        ),
    );
    let mut scan = scan_with_objects(&objects);
    set_identity(&mut scan, 0, POLYEDGE_SEGMENT_TARGET, "first", None, true);
    set_identity(&mut scan, 1, POLYEDGE_SEGMENT_TARGET, "second", None, true);
    let result = crate::decode::decode_for_test(&scan);
    assert!(polyedge_segment_parameter(&result).is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == crate::loss::RhinoLossCode::ReferenceMemberAmbiguous.kind() }));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}
