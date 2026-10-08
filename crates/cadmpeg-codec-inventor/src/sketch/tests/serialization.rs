// SPDX-License-Identifier: Apache-2.0
//! Borrowed sketch native wire checks.

use super::super::{
    parse_transform, PmDcSketchEntityKind, PmDcTransformPayload, PmDcTransformPayloadWire,
    PointTail, PointTailWire,
};
use crate::compact_matrix::CompactMatrix;
use crate::pmdc::{
    PmDcContentHeader, PmDcListMetadata, PmDcReference, PmDcReferenceList, PMDC_LIST_CLONE_COUNT,
};
use crate::test_support::test_fixtures::{content, parse};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use serde::Serialize;

#[derive(Serialize)]
struct Record<'a, T> {
    id: &'static str,
    value: &'a T,
}

#[test]
fn point_tail_borrowed_wire_refuses_retained_limit_before_clone() {
    let tail = PointTail::Present {
        state: 7,
        associations: PmDcReferenceList::new(
            8,
            Some(PmDcListMetadata::U16([1, 2])),
            vec![PmDcReference::new(3, false).expect("test reference index fits 31 bits")],
        )
        .expect("paired point associations"),
    };
    let wire = PointTailWire::from(tail.clone());
    assert_eq!(
        serde_json::to_vec(&tail).expect("borrowed tail"),
        serde_json::to_vec(&wire).expect("owned tail")
    );
    let record = Record {
        id: "inventor:pmdc:point-tail#1",
        value: &tail,
    };
    PMDC_LIST_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": wire}),
    );
    PMDC_LIST_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));

    assert_eq!(
        serde_json::to_vec(&PointTail::Absent).expect("borrowed absent tail"),
        serde_json::to_vec(&PointTailWire::from(PointTail::Absent)).expect("owned absent tail")
    );
}

#[test]
fn transform_payload_streams_flat_matrix_under_retained_limit() {
    let reference = PmDcReference::new(3, false).expect("test reference index fits 31 bits");
    let transform = PmDcTransformPayload {
        save_version_major: 16,
        header: PmDcContentHeader {
            header_value: 0,
            header_id: 18,
            next: reference,
            flags: 0,
            context: reference,
            source_index: 1,
        },
        prefix_present: true,
        matrix: CompactMatrix::try_from_rows(0, 0, [[2.0; 4]; 4])
            .expect("finite compact matrix"),
    };
    let wire = PmDcTransformPayloadWire::from(transform.clone());
    assert_eq!(
        serde_json::to_vec(&transform).expect("borrowed transform"),
        serde_json::to_vec(&wire).expect("owned transform")
    );
    let record = Record {
        id: "inventor:pmdc:transform#1",
        value: &transform,
    };
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": wire}),
    );
}

#[test]
fn point_tail_wire_requires_both_fields() {
    let list = serde_json::json!({"marker": 2, "metadata": null, "references": []});
    let mut wire = serde_json::json!({
        "form": "point", "position": [0.0, 0.0],
        "endpoint_of": list.clone(), "center_of": list.clone(),
        "state": null, "associations": null
    });
    let point: PmDcSketchEntityKind =
        serde_json::from_value(wire.clone()).expect("paired point fixture round-trips");
    assert_eq!(
        serde_json::to_value(point).expect("paired point fixture round-trips"),
        wire
    );
    wire["state"] = serde_json::json!(0);
    assert!(serde_json::from_value::<PmDcSketchEntityKind>(wire.clone()).is_err());
    wire["associations"] = list;
    let point: PmDcSketchEntityKind =
        serde_json::from_value(wire.clone()).expect("paired point fixture round-trips");
    assert_eq!(
        serde_json::to_value(point).expect("paired point fixture round-trips"),
        wire
    );
    wire["state"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<PmDcSketchEntityKind>(wire).is_err());
}

#[test]
fn transform_prefix_error_needs_no_retained_text() {
    let mut bytes = content(1);
    bytes.extend_from_slice(&0x8421u16.to_le_bytes());
    bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
    let transform = parse(&bytes, |_ctx, source| {
        parse_transform(source, 22).expect("transform")
    });
    let mut wire = serde_json::to_value(transform).expect("wire");
    wire["prefix"] = serde_json::json!(516);
    let wire = serde_json::from_value::<PmDcTransformPayloadWire>(wire).expect("wire fields");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        wire.into_payload(),
        Err(CodecError::Malformed(_))
    ));
    ctx.finish_session().expect("fixed prefix error needs no budget");
}

#[test]
fn transform_prefix_wire_is_constant_or_absent() {
    let mut bytes = content(1);
    bytes.extend_from_slice(&0x8421u16.to_le_bytes());
    bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
    let transform = parse(&bytes, |_ctx, source| {
        parse_transform(source, 22).expect("constant-prefix transform fixture is valid")
    });
    let mut wire =
        serde_json::to_value(transform).expect("constant-prefix transform fixture is valid");
    for (prefix, present) in [
        (serde_json::Value::Null, false),
        (serde_json::json!(515), true),
    ] {
        wire["prefix"] = prefix;
        let parsed: PmDcTransformPayload =
            serde_json::from_value::<PmDcTransformPayloadWire>(wire.clone())
                .expect("constant-prefix transform fixture is valid")
                .into_payload()
                .expect("constant-prefix transform fixture is valid");
        assert_eq!(parsed.prefix_present, present);
        assert_eq!(
            serde_json::to_value(parsed).expect("constant-prefix transform fixture is valid"),
            wire
        );
    }
    wire["prefix"] = serde_json::json!(516);
    assert!(
        serde_json::from_value::<PmDcTransformPayloadWire>(wire.clone())
            .expect("transform wire fixture is valid")
            .into_payload()
            .is_err()
    );
    wire.as_object_mut()
        .expect("constant-prefix transform fixture is valid")
        .remove("prefix");
    assert!(
        !serde_json::from_value::<PmDcTransformPayloadWire>(wire)
            .expect("constant-prefix transform fixture is valid")
            .into_payload()
            .expect("constant-prefix transform fixture is valid")
            .prefix_present
    );
}
