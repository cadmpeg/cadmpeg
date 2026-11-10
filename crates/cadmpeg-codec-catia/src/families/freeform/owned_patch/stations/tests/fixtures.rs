// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;

const EXTENDED: &[u8] =
    include_bytes!("../../../../../../tests/fixtures/endpoint_stations/extended-span.bin");

fn resolve(index: usize, reference: AllocationReference) -> Option<usize> {
    let value = usize::try_from(reference.value).ok()?;
    match reference.encoding {
        AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value),
        AllocationReferenceEncoding::BackwardDistance => index.checked_sub(value),
        _ => None,
    }
}

fn assert_stations(bytes: &[u8], expected: &[f64]) {
    let records = consolidated_records(bytes);
    assert_eq!(records.len(), 3 + expected.len());
    for (lane, &expected) in expected.iter().enumerate() {
        assert_station(bytes, 3 + lane, Some(expected));
    }
}

fn assert_station(bytes: &[u8], curve: usize, expected: Option<f64>) {
    let records = consolidated_records(bytes);
    let spline = owned_pcurve_from_record(bytes, &records[curve]).expect("fixture pcurve");
    let knots = spline
        .sites
        .iter()
        .map(|site| site.knot.get())
        .collect::<Vec<_>>();
    let first_arc = spline_first_arc(payload(bytes, &records[curve]).expect("payload"), &knots)
        .expect("first arc");
    let extrapolation = leading_extrapolation_sites(&knots, spline.extrapolation_sites);
    let domain = Domain {
        range: spline.range.endpoints(),
        knots: Some((&knots, first_arc, extrapolation)),
        line: false,
    };
    assert_eq!(
        parameter(bytes, &records, 0, curve, domain, &resolve),
        expected
    );
    assert_eq!(
        edge_station(bytes, &records, 2, curve, domain, &resolve).map(|station| station.0),
        expected
    );
}

#[test]
fn framed_vertex_graph_resolves_compact_and_extended_span_tags() {
    assert_stations(EXTENDED, &[2.5]);
    assert_stations(
        include_bytes!("../../../../../../tests/fixtures/endpoint_stations/compact-span.bin"),
        &[2.5],
    );
}

#[test]
fn framed_vertex_graph_skips_encoded_leading_extrapolation_sites() {
    assert_stations(
        include_bytes!(
            "../../../../../../tests/fixtures/endpoint_stations/leading-extrapolation.bin"
        ),
        &[0.25],
    );
}

#[test]
fn framed_vertex_graph_preserves_mixed_lane_gauges() {
    assert_stations(
        include_bytes!("../../../../../../tests/fixtures/endpoint_stations/mixed-final-tag.bin"),
        &[0.5, 2.75],
    );
    assert_stations(
        include_bytes!("../../../../../../tests/fixtures/endpoint_stations/mixed-first-tag.bin"),
        &[2.5, 12.25],
    );
    assert_stations(
        include_bytes!(
            "../../../../../../tests/fixtures/endpoint_stations/extended-three-lanes.bin"
        ),
        &[2.5, 12.25, -0.25],
    );
}

#[test]
fn framed_extended_station_rejects_invalid_spans_and_local_parameters() {
    let records = consolidated_records(EXTENDED);
    let tail = records[2].payload().expect("incidence payload").end;
    let knots = [-1.0, 2.0, 4.0];
    let domain = Domain {
        range: [0.0, 4.0],
        knots: Some((&knots, 1, 0)),
        line: false,
    };
    for code in [0x43, 0x46, 0xff] {
        let mut bytes = EXTENDED.to_vec();
        bytes[tail - 1] = code;
        let records = consolidated_records(&bytes);
        assert!(parameter(&bytes, &records, 0, 3, domain, &resolve).is_none());
    }
    let mut duplicate = EXTENDED.to_vec();
    let incidence = records[2].payload().expect("incidence payload").start;
    duplicate[incidence + 1] = duplicate[incidence + 2];
    let duplicate_records = consolidated_records(&duplicate);
    assert!(parameter(&duplicate, &duplicate_records, 0, 3, domain, &resolve).is_none());

    let mut unknown_tag = EXTENDED.to_vec();
    unknown_tag[tail - 2] = 0x08;
    let unknown_records = consolidated_records(&unknown_tag);
    assert!(parameter(&unknown_tag, &unknown_records, 0, 3, domain, &resolve).is_none());

    for value in [-0.5_f64, 2.5, f64::INFINITY, f64::NAN] {
        let mut bytes = EXTENDED.to_vec();
        bytes[tail - 10..tail - 2].copy_from_slice(&value.to_le_bytes());
        let records = consolidated_records(&bytes);
        assert!(parameter(&bytes, &records, 0, 3, domain, &resolve).is_none());
    }
}

#[test]
fn framed_extended_station_preserves_selected_and_unassigned_lanes() {
    let bytes = include_bytes!(
        "../../../../../../tests/fixtures/endpoint_stations/extended-first-lane.bin"
    );
    assert_station(bytes, 3, Some(2.5));
    assert_station(bytes, 4, None);
    assert_stations(
        include_bytes!(
            "../../../../../../tests/fixtures/endpoint_stations/extended-tag-and-scalar.bin"
        ),
        &[2.5, 2.75],
    );
}

#[test]
fn framed_companion_station_uses_its_own_span_and_affine_map() {
    let bytes =
        include_bytes!("../../../../../../tests/fixtures/endpoint_stations/mapped-companion.bin");
    let records = consolidated_records(bytes);
    assert!(mapped_station(bytes, &records, [0, 5, 3, 2], false, &resolve).is_none());
    assert_eq!(
        mapped_station(bytes, &records, [0, 5, 3, 2], true, &resolve),
        Some((5.0, false))
    );
    let mut invalid = bytes.to_vec();
    let map = records[5].payload().expect("parameter map").start;
    invalid[map + 2] = invalid[map + 1];
    let records = consolidated_records(&invalid);
    assert!(mapped_station(&invalid, &records, [0, 5, 3, 2], true, &resolve).is_none());
}
