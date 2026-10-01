// SPDX-License-Identifier: Apache-2.0
//! Intersection and auxiliary delta-record fixtures.

use super::{
    blend_bound_charted_intersection_curve_stream, bspline_partition_stream,
    charted_intersection_curve_topology_partition_stream, deltas_body_revision,
    deltas_intersection_curve_stream, encoded_xmt, ext11_charted_intersection_curve_stream,
    put_ref, put_vec3, status_framed_deltas_intersection_stream, status_framed_deltas_point_stream,
};

#[test]
fn deltas_point_normalizes_to_partition_record_framing() {
    let record = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::census::walk(ctx, &status_framed_deltas_point_stream())
    })
    .unwrap()
    .into_events()
    .records
    .remove(0);
    let mut expected = crate::test_support::test_bytes::record(29, 40);
    put_ref(&mut expected, 2, 50);
    expected[4..8].copy_from_slice(&900u32.to_be_bytes());
    for at in [8, 10, 12, 14] {
        put_ref(&mut expected, at, 1);
    }
    put_vec3(&mut expected, 16, [0.0125, -0.002, 0.004]);
    assert_eq!(record.canonical_bytes, expected);
}

#[test]
fn deltas_intersection_normalizes_during_full_record_merge() {
    let mut stream = status_framed_deltas_intersection_stream();
    stream[10] = 0;
    let record_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 38);
    assert_eq!(census.bytes_decoded(), record_len);

    let merged = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::merge_full_records(ctx, &[], &stream)
    })
    .unwrap();
    let intersections = crate::test_support::with_decode_context(|ctx| {
        crate::topology::composite_curves(ctx, &merged)
    })
    .unwrap();
    assert_eq!(intersections.len(), 1);
    assert_eq!(intersections[0].xmt, 12);
    assert_eq!(
        intersections[0]
            .references
            .map(crate::framing::xmt_reference::XmtTarget::to_wire),
        [6, 7, 20, 21, 22, 23]
    );
}

#[test]
fn merge_replaces_a_partition_intersection_by_exact_xmt() {
    let partition = charted_intersection_curve_topology_partition_stream();
    let mut replacement = status_framed_deltas_intersection_stream();
    let sense = replacement
        .iter()
        .position(|byte| *byte == b'+')
        .expect("intersection sense");
    replacement[sense] = b'-';

    let merged = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::merge_full_records(ctx, &partition, &replacement)
    })
    .unwrap();
    let [intersection] = crate::test_support::with_decode_context(|ctx| {
        crate::topology::composite_curves(ctx, &merged)
    })
    .unwrap()
    .try_into()
    .expect("one current intersection");

    assert_eq!(intersection.xmt, 12);
    assert!(!intersection.sense);
}

#[test]
fn deltas_walks_complete_single_byte_intersection_data_records() {
    let mut stream = crate::topology::TYPE_38_SCHEMA_HEADER.to_vec();
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    for reference in [6u16, 7] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for reference in [15u16, 14, 13] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(0);
    }
    stream.extend_from_slice(&[0, 1, 1]);
    let schema_end = stream.len();
    stream.extend_from_slice(&[0xa5; 100]);

    let record_offset = stream.len();
    stream.extend_from_slice(&[0x5a]);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4, 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    let record_end = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 90);
    assert_eq!(census.records[0].family_name(), "INTERSECTION_DATA");
    assert_eq!(census.records[0].xmt, 12);
    assert_eq!(census.records[0].offset, record_offset);
    assert_eq!(
        census.records[0].family.references(),
        [1, 2, 3, 4, 5, 6, 6, 1, 1, 1, 1]
    );
    assert_eq!(
        census.records[0].canonical_bytes,
        stream[record_offset..record_end]
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
            ["INTERSECTION_DATA"],
        1
    );
    assert_eq!(
        census.bytes_decoded(),
        schema_end + (record_end - record_offset)
    );
    let curves = crate::test_support::with_decode_context(|ctx| {
        crate::topology::intersection_data_curves(ctx, &stream)
    })
    .unwrap();
    assert_eq!(curves.len(), 1);
    assert_eq!(
        curves[0]
            .references
            .map(crate::framing::xmt_reference::XmtTarget::to_wire),
        [6, 6, 1, 1, 1, 1]
    );

    let residual = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::semantic_residual(ctx, &stream)
    })
    .unwrap();
    assert!(residual[record_offset..record_end]
        .iter()
        .all(|byte| *byte == 0xff));
    let prefix_len = crate::topology::TYPE_38_SCHEMA_HEADER.len() - 1;
    let appended_start = residual.len() - prefix_len - (record_end - record_offset);
    assert_eq!(
        &residual[appended_start..appended_start + prefix_len],
        &crate::topology::TYPE_38_SCHEMA_HEADER[..prefix_len]
    );
    assert_eq!(
        &residual[appended_start + prefix_len..],
        &stream[record_offset..record_end]
    );
}

#[test]
fn semantic_residual_does_not_reemit_historical_intersection_data() {
    let mut stream = deltas_intersection_curve_stream();
    stream.extend_from_slice(&deltas_body_revision(2));

    let residual = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::semantic_residual(ctx, &stream)
    })
    .unwrap();

    assert_eq!(residual.len(), stream.len());
}

#[test]
fn deltas_rejects_single_byte_intersection_data_before_its_schema_anchor() {
    let mut stream = vec![0x5a];
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert!(census.records.iter().all(|record| record.kind() != 90));
    assert!(
        !crate::test_support::with_decode_context(|ctx| census.full_counts(ctx))
            .unwrap()
            .contains_key("INTERSECTION_DATA")
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::topology::intersection_data_curves(ctx, &stream)
    })
    .unwrap()
    .is_empty());
}

#[test]
fn deltas_rejects_denormal_topology_tolerance_payload_coincidences() {
    fn edge(tolerance: f64) -> Vec<u8> {
        let mut bytes = 16u16.to_be_bytes().to_vec();
        bytes.extend(encoded_xmt(20));
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend(encoded_xmt(1));
        bytes.push(1);
        bytes.extend_from_slice(&tolerance.to_be_bytes());
        for reference in [2u32, 3, 4, 5, 6, 7, 8] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(1);
        }
        bytes
    }

    let valid = edge(1.0e-8);
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &valid))
            .unwrap();
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 16);
    assert_eq!(census.bytes_decoded(), valid.len());

    let denormal = edge(1.0e-120);
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &denormal))
            .unwrap()
            .records
            .is_empty()
    );

    let mut vertex = 18u16.to_be_bytes().to_vec();
    vertex.extend(encoded_xmt(20));
    vertex.extend_from_slice(&1u32.to_be_bytes());
    for reference in [2u32, 3, 4, 5, 6] {
        vertex.extend(encoded_xmt(reference));
        vertex.push(1);
    }
    let tolerance_at = vertex.len();
    vertex.extend_from_slice(&1.0e-8f64.to_be_bytes());
    vertex.extend(encoded_xmt(7));
    vertex.push(1);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &vertex))
            .unwrap();
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 18);

    vertex[tolerance_at..tolerance_at + 8].copy_from_slice(&1.0e-120f64.to_be_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &vertex))
            .unwrap()
            .records
            .is_empty()
    );
}

#[test]
fn deltas_rejects_denormal_point_payload_coincidences() {
    let mut point = status_framed_deltas_point_stream();
    let position = point.len() - 24;
    for (ordinal, value) in [f64::from_bits(1), f64::from_bits(2), f64::from_bits(3)]
        .into_iter()
        .enumerate()
    {
        point[position + ordinal * 8..position + (ordinal + 1) * 8]
            .copy_from_slice(&value.to_be_bytes());
    }
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &point))
            .unwrap()
            .records
            .iter()
            .all(|record| record.kind() != 29)
    );

    point[position..position + 8].copy_from_slice(&1.0e-200f64.to_be_bytes());
    point[position + 8..].fill(0);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::test_support::with_decode_context(
            |ctx| crate::deltas::census::walk(ctx, &point)
        )
        .unwrap()
        .full_counts(ctx))
        .unwrap()["POINT"],
        1
    );
}

#[test]
fn deltas_walks_complete_intersection_auxiliary_records() {
    let source = ext11_charted_intersection_curve_stream();
    let blend_source = blend_bound_charted_intersection_curve_stream();
    let chart_pos = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_records(
            ctx,
            &source,
            crate::intersection::ChartPointLayout::Ext11,
        )
    })
    .unwrap()[0]
        .pos;
    let (_, chart_end) = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_record_at(
            ctx,
            &source,
            chart_pos,
            crate::intersection::ChartPointLayout::Ext11,
        )
    })
    .unwrap()
    .expect("chart");
    let term_pos = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::term_use_records(ctx, &source)
    })
    .unwrap()[0]
        .pos;
    let (_, term_end) = crate::intersection::term_use_at(&source, term_pos).expect("term use");
    let support_uv_pos = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::support_uv_records(ctx, &source)
    })
    .unwrap()[0]
        .pos;
    let (_, support_uv_end) = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::support_uv_record_at(ctx, &source, support_uv_pos)
    })
    .unwrap()
    .expect("support UV");
    let blend_bound_pos = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::blend_bounds(ctx, &blend_source)
    })
    .unwrap()[0]
        .pos;
    let (_, blend_bound_end) =
        crate::intersection::blend_bound_at(&blend_source, blend_bound_pos).expect("blend bound");

    for (bytes, kind, family) in [
        (&source[chart_pos..chart_end], 40, "CHART"),
        (&source[term_pos..term_end], 41, "TERM_USE"),
        (
            &blend_source[blend_bound_pos..blend_bound_end],
            59,
            "BLEND_BOUND",
        ),
        (&source[support_uv_pos..support_uv_end], 204, "SUPPORT_UV"),
    ] {
        let mut stream = bytes.to_vec();
        stream.extend_from_slice(&[0xfe, 0xdc]);
        let census = crate::test_support::with_decode_context(|ctx| {
            crate::deltas::census::walk(ctx, &stream)
        })
        .unwrap();
        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind(), kind);
        assert_eq!(census.records[0].canonical_bytes, bytes);
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
                [family],
            1
        );
        assert_eq!(census.bytes_decoded(), bytes.len());

        let residual = crate::test_support::with_decode_context(|ctx| {
            crate::deltas::semantic_residual(ctx, &stream)
        })
        .unwrap();
        assert!(residual[..bytes.len()].iter().all(|byte| *byte == 0xff));
        assert!(residual.ends_with(bytes));
    }
}

#[test]
fn deltas_support_uv_route_refuses_scoped_limit() {
    let source = ext11_charted_intersection_curve_stream();
    let record = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::support_uv_records(ctx, &source)
    })
    .unwrap()
    .remove(0);
    let (_, end) = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::support_uv_record_at(ctx, &source, record.pos)
    })
    .unwrap()
    .expect("support UV");
    let stream = &source[record.pos..end];

    crate::test_support::with_decode_context_over(
        stream,
        |policy| {
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            assert!(matches!(crate::deltas::census::walk(ctx, stream),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && limit.operation == "NX support-UV scalar lane"));
        },
    );
}

#[test]
fn deltas_walks_status_framed_blend_bound_records() {
    fn record(escape: bool, xmt: u32, surface: u32) -> Vec<u8> {
        let mut bytes = 59u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.extend_from_slice(&17u32.to_be_bytes());
        for (reference, status) in [(1u32, 1u8), (3, 1), (40_001, 0), (1, 1), (40_002, 0)] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(status);
        }
        bytes.push(b'+');
        bytes.extend(encoded_xmt(0));
        bytes.extend(encoded_xmt(surface));
        bytes.push(1);
        bytes
    }

    let direct = record(false, 24, 40_003);
    let escaped = record(true, 40_004, 40_005);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();

    assert_eq!(
        crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
            ["BLEND_BOUND"],
        2
    );
    assert_eq!(census.bytes_decoded(), stream.len());
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(
        census.records[0].family.references(),
        [1, 3, 40_001, 1, 40_002, 0, 40_003]
    );
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::intersection::blend_bounds(
            ctx, &stream
        ))
        .unwrap()
        .into_iter()
        .map(|record| record.framing)
        .collect::<Vec<_>>(),
        [
            crate::intersection::BlendBoundFraming::DeltasDirect,
            crate::intersection::BlendBoundFraming::DeltasEscaped,
        ]
    );

    let mut invalid_status = record(false, 24, 40_003);
    *invalid_status.last_mut().expect("terminal status") = 0;
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx,
            &invalid_status
        ))
        .unwrap()
        .records
        .is_empty()
    );
}

#[test]
fn deltas_walks_complete_nurbs_auxiliary_records() {
    let source = bspline_partition_stream();
    for (kind, family) in [
        (125u16, "B_SURFACE_DATA"),
        (126, "B_SURFACE_DESCRIPTOR"),
        (127, "MULTIPLICITIES"),
        (128, "KNOTS"),
        (135, "B_CURVE_DATA"),
        (136, "B_CURVE_DESCRIPTOR"),
    ] {
        let (pos, auxiliary) = (0..source.len())
            .find_map(|pos| {
                let auxiliary = crate::test_support::with_decode_context(|ctx| crate::nurbs::auxiliary_record_at(ctx, &source, pos)).unwrap()?;
                (auxiliary.family.kind() == kind).then_some((pos, auxiliary))
            })
            .expect("complete NURBS auxiliary record");
        let bytes = &source[pos..auxiliary.end];
        let mut stream = bytes.to_vec();
        stream.extend_from_slice(&[0xfe, 0xdc]);

        let census = crate::test_support::with_decode_context(|ctx| {
            crate::deltas::census::walk(ctx, &stream)
        })
        .unwrap();
        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind(), kind);
        assert_eq!(census.records[0].canonical_bytes, bytes);
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
                [family],
            1
        );
        assert_eq!(census.bytes_decoded(), bytes.len());

        let residual = crate::test_support::with_decode_context(|ctx| {
            crate::deltas::semantic_residual(ctx, &stream)
        })
        .unwrap();
        assert!(residual[..bytes.len()].iter().all(|byte| *byte == 0xff));
        assert!(residual.ends_with(bytes));
    }
}

#[test]
fn deltas_walks_complete_status_framed_surface_descriptors() {
    let mut descriptor = 126u16.to_be_bytes().to_vec();
    descriptor.push(0xff);
    descriptor.extend(encoded_xmt(98));
    descriptor.extend_from_slice(&5u32.to_be_bytes());
    descriptor.extend_from_slice(&3u16.to_be_bytes());
    descriptor.extend_from_slice(&30u32.to_be_bytes());
    descriptor.extend_from_slice(&4u32.to_be_bytes());
    descriptor.extend_from_slice(&[6, 5]);
    descriptor.extend_from_slice(&10u32.to_be_bytes());
    descriptor.extend_from_slice(&2u32.to_be_bytes());
    descriptor.extend_from_slice(&1u32.to_be_bytes());
    descriptor.extend_from_slice(&3u16.to_be_bytes());
    for reference in [106u32, 107, 108, 109, 110] {
        descriptor.extend(encoded_xmt(reference));
        descriptor.push(0);
    }
    let descriptor_len = descriptor.len();
    descriptor.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::census::walk(ctx, &descriptor)
    })
    .unwrap();
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 126);
    assert_eq!(census.records[0].xmt, 98);
    assert_eq!(census.records[0].end, descriptor_len);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
            ["B_SURFACE_DESCRIPTOR"],
        1
    );
    assert_eq!(census.bytes_decoded(), descriptor_len);

    let mut invalid_status = descriptor[..descriptor_len].to_vec();
    *invalid_status.last_mut().expect("final reference status") = 1;
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx,
            &invalid_status
        ))
        .unwrap()
        .records
        .is_empty()
    );
}

#[test]
fn deltas_walks_complete_surface_data_headers() {
    fn record(escape: bool, xmt: u32, marker: u8) -> Vec<u8> {
        let mut bytes = 125u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        for value in [0.0f64, 1.0, -0.25, 0.5, 0.0, 1.0, -0.25, 0.5] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.push(marker);
        bytes.extend(std::iter::repeat_n(b'B', usize::from(marker) * 4));
        bytes.extend(std::iter::repeat_n(b'?', 12 - usize::from(marker) * 4));
        for reference in [1u32, 20, 21, 1] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(1);
        }
        bytes
    }

    let direct = record(false, 20, 1);
    let escaped = record(true, 40_000, 2);
    let mut extended_marker_one = record(false, 21, 1);
    extended_marker_one[73..77].fill(b'B');
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&extended_marker_one);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
            ["B_SURFACE_DATA"],
        3
    );
    assert_eq!(census.bytes_decoded(), decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[2].canonical_bytes, extended_marker_one);

    let mut invalid_marker = record(false, 20, 2);
    invalid_marker[68] = 3;
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx,
            &invalid_marker
        ))
        .unwrap()
        .records
        .is_empty()
    );

    let mut invalid_status = record(false, 20, 1);
    *invalid_status.last_mut().expect("final status") = 0;
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx,
            &invalid_status
        ))
        .unwrap()
        .records
        .is_empty()
    );
}

#[test]
fn deltas_walks_complete_curve_data_headers() {
    fn record(escape: bool, xmt: u32, mode: u8, reference: u32) -> Vec<u8> {
        let mut bytes = 135u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.push(mode);
        bytes.extend(encoded_xmt(reference));
        bytes.push(1);
        bytes
    }

    let direct = record(false, 20, 2, 1);
    let escaped = record(true, 40_000, 1, 21);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
            ["B_CURVE_DATA"],
        2
    );
    assert_eq!(census.bytes_decoded(), decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, escaped);

    let mut invalid_marker = record(false, 20, 2, 1);
    invalid_marker[4] = 3;
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx,
            &invalid_marker
        ))
        .unwrap()
        .records
        .is_empty()
    );

    let mut invalid_status = record(false, 20, 2, 1);
    *invalid_status.last_mut().expect("final status") = 0;
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx,
            &invalid_status
        ))
        .unwrap()
        .records
        .is_empty()
    );
}

#[test]
fn deltas_walks_complete_type_141_records() {
    fn record(escape: bool, xmt: u32, references: [u32; 4], boundary_statuses: [u8; 2]) -> Vec<u8> {
        let mut bytes = 141u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        for (reference, status) in
            references
                .into_iter()
                .zip([boundary_statuses[0], 0, 0, boundary_statuses[1]])
        {
            bytes.extend(encoded_xmt(reference));
            bytes.push(status);
        }
        bytes
    }

    let direct = record(false, 3158, [646, 3943, 3165, 131], [0, 1]);
    let direct_extended = record(false, 33_000, [646, 3943, 3165, 131], [1, 0]);
    let escaped = record(true, 40_000, [40_001, 1, 0, 40_002], [1, 1]);
    let ambiguous_escaped = record(true, 325, [317, 44, 44, 8], [1, 1]);
    let mut stream = direct.clone();
    stream.extend_from_slice(&direct_extended);
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&ambiguous_escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| census.full_counts(ctx)).unwrap()
            ["TYPE_141"],
        4
    );
    assert_eq!(census.bytes_decoded(), decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, direct_extended);
    assert_eq!(census.records[1].xmt, 33_000);
    assert_eq!(census.records[2].canonical_bytes, escaped);
    assert_eq!(census.records[2].xmt, 40_000);
    assert_eq!(
        census.records[2].family.references(),
        [40_001, 1, 0, 40_002]
    );
    assert_eq!(census.records[3].canonical_bytes, ambiguous_escaped);
    assert_eq!(census.records[3].xmt, 325);
    assert_eq!(census.records[3].family.references(), [317, 44, 44, 8]);

    let residual = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::semantic_residual(ctx, &stream)
    })
    .unwrap();
    assert!(residual[..decoded_len].iter().all(|byte| *byte == 0xff));
    assert!(residual.ends_with(&[direct, direct_extended, escaped, ambiguous_escaped].concat()));
}
