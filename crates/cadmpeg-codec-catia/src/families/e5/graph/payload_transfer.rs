// SPDX-License-Identifier: Apache-2.0
//! Actual E5 parser payload transfers and sticky retained refusals.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{
    parse_bodies, parse_body_root, parse_bounds, parse_jet_pcurve, parse_nurbs_pcurve, E5Pcurve,
    Record,
};

fn refuses_actual_payload<T>(
    input: &[u8],
    operation: &'static str,
    parse: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let error = crate::test_support::with_retained_refusal(input, operation, |ctx| {
        let result = parse(ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, operation);
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            let sticky = ctx.charge_work(1, "after E5 payload refusal");
            assert!(matches!(sticky, Err(CodecError::ResourceLimit(next)) if next == *limit));
        }
        result
    }).err().expect("selected payload must retain its backing");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation));
}

#[test]
fn e5_bounds_transfer_actual_entries_and_preserve_retained_refusal() {
    let mut payload = vec![0x81, 0x85, 0x81];
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(7_u32.to_le_bytes());
    let record = Record { class: 0x0e, id: 1, payload: &payload };
    refuses_actual_payload(&payload, "catia_e5_parse_bounds_output", |ctx| parse_bounds(ctx, &record));
    let value = crate::test_support::with_service_context(|ctx| parse_bounds(ctx, &record))
        .expect("bounds admitted").expect("complete bound");
    assert_eq!(value.entries.len(), 1);
    assert_eq!(value.entries[0].representation, 5);
    assert_eq!(value.entries[0].parameter.get(), 1.0);
    assert_eq!(value.entries[0].code, 7);
}

#[test]
fn e5_nurbs_transfer_actual_lanes_and_preserve_retained_refusal() {
    let mut payload = vec![0, 0];
    for value in [1_u32, 0, 0, 2, 0] { payload.extend(value.to_le_bytes()); }
    for value in [0.0_f64, 1.0] { payload.extend(value.to_le_bytes()); }
    for value in [2_u32, 2] { payload.extend(value.to_le_bytes()); }
    for value in [0.0_f64, 0.0, 1.0, 1.0] { payload.extend(value.to_le_bytes()); }
    payload.extend([0; 37]);
    refuses_actual_payload(&payload, "catia_e5_parse_nurbs_pcurve_output", |ctx| {
        parse_nurbs_pcurve(ctx, &payload, 0, 7)
    });
    let value = crate::test_support::with_service_context(|ctx| parse_nurbs_pcurve(ctx, &payload, 0, 7))
        .expect("NURBS admitted").expect("complete NURBS");
    assert!(matches!(value, E5Pcurve::Nurbs { surface: 7, degree: 1, knots, control_points, range }
        if knots.iter().map(|knot| knot.get()).collect::<Vec<_>>() == [0.0, 0.0, 1.0, 1.0]
            && control_points.iter().map(|point| point.map(|value| value.get())).collect::<Vec<_>>() == [[0.0, 0.0], [1.0, 1.0]]
            && range.map(|value| value.get()) == [0.0, 1.0]));
}

#[test]
fn e5_jet_transfer_actual_sites_and_preserve_retained_refusal() {
    let mut payload = Vec::new();
    for value in [5_u32, 0, 0, 2, 0, 0, 0] { payload.extend(value.to_le_bytes()); }
    payload.extend(1.0_f64.to_le_bytes());
    for value in [6_u32, 6, 2] { payload.extend(value.to_le_bytes()); }
    for _ in 0..8 { payload.extend(0.0_f64.to_le_bytes()); }
    payload.extend(1_u16.to_le_bytes());
    for _ in 0..4 { payload.extend(0.0_f64.to_le_bytes()); }
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    refuses_actual_payload(&payload, "catia_e5_jet_sites", |ctx| parse_jet_pcurve(ctx, &payload, 0, 7));
    let value = crate::test_support::with_service_context(|ctx| parse_jet_pcurve(ctx, &payload, 0, 7))
        .expect("jet admitted").expect("complete jet");
    assert!(matches!(value, E5Pcurve::Jet { surface: 7, sites, range }
        if sites.iter().map(|site| (site.knot.get(), site.multiplicity)).collect::<Vec<_>>() == [(0.0, 6), (1.0, 6)]
            && range.map(|value| value.get()) == [0.0, 1.0]));
}

#[test]
fn e5_body_root_transfer_actual_faces_and_preserve_retained_refusal() {
    let payload = [0x81, 0x85, 0x81, 1, 0, 1, 0, 1, 0];
    refuses_actual_payload(&payload, "catia_e5_parse_body_root_output", |ctx| parse_body_root(ctx, &payload));
    assert_eq!(crate::test_support::with_service_context(|ctx| parse_body_root(ctx, &payload))
        .expect("roster admitted").expect("complete roster"), [5]);
}

#[test]
fn e5_bodies_transfer_nested_faces_and_preserve_retained_refusal() {
    let root_payload = [0x81, 0x85, 0x81, 1, 0, 1, 0, 1, 0];
    let records = [
        Record { class: 0x01, id: 9, payload: &[0x81, 0x84] },
        Record { class: 0x08, id: 4, payload: &root_payload },
        Record { class: 0x00, id: 5, payload: &[] },
    ];
    let by_id = records.iter().map(|record| (record.id, *record)).collect();
    refuses_actual_payload(&root_payload, "catia_e5_parse_bodies_output", |ctx| parse_bodies(ctx, &records, &by_id));
    let value = crate::test_support::with_service_context(|ctx| parse_bodies(ctx, &records, &by_id))
        .expect("bodies admitted").expect("complete bodies");
    assert_eq!(value.len(), 1);
    assert_eq!(value[0].record_id, 9);
    assert_eq!(value[0].faces, [5]);
}
