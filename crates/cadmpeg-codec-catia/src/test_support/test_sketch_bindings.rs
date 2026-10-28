// SPDX-License-Identifier: Apache-2.0
//! Source streams for membership and point-value binding controls.

use super::test_object_graph::{
    catalog_stream, entity_table_record_with_value, object_graph_from_records, object_graph_record,
};

pub(crate) fn finite_pair(first: u64, second: u64) -> Vec<u8> {
    let mut value = vec![0x91, 0x86, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6];
    value.extend_from_slice(&first.to_le_bytes());
    value.push(0xe6);
    value.extend_from_slice(&second.to_le_bytes());
    value.extend_from_slice(&[0xfe, 0xfe]);
    value
}

pub(crate) fn roster() -> Vec<u8> {
    vec![
        0x3b, 0x84, 0x81, 0x82, 0x81, 0x82, 0x81, 0x83, 0x81, 0xcd, 0x81, 0x84, 0xfe,
    ]
}

pub(crate) fn sketch_bindings_source(
    roster: &[u8],
    coordinate_value: &[u8],
    compact_sketch: bool,
    compact_points: bool,
    point_class: &str,
) -> Vec<u8> {
    let sketch_head: &[u8] = if compact_sketch {
        &[0x12, 0x88, 0x84]
    } else {
        &[0x54, 1, 0x88, 0x84, 0x83]
    };
    let point_head: &[u8] = if compact_points {
        &[0x12, 0x83, 0x85]
    } else {
        &[0x14, 1, 0x83, 0x85]
    };
    let objects = [
        object_graph_record(sketch_head, roster),
        object_graph_record(&[0x54, 1, 0x85, 0x86, 0x83], &[0xfe]),
        object_graph_record(&[0x54, 1, 0x85, 0x86, 0x83], &[0xfe]),
        object_graph_record(point_head, &[0xfe]),
        object_graph_record(&[0x14, 1, 0x81, 0x86], &[0xfe]),
        object_graph_record(&[0x14, 1, 0x82, 0x86], &[0xfe]),
        object_graph_record(point_head, &[0xfe]),
        object_graph_record(&[0x14, 1, 0x89, 0x86], &[0xfe]),
    ];
    let mut source = Vec::new();
    for identity in 1..=8 {
        let value = if identity == 3 {
            coordinate_value
        } else {
            &[0xfe]
        };
        source.extend(entity_table_record_with_value(identity, value));
    }
    source.push(0xde);
    source.extend(object_graph_from_records(&objects));
    source.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        point_class,
        "Value",
    ]));
    source
}

pub(crate) fn positive_sketch_bindings_source() -> Vec<u8> {
    sketch_bindings_source(
        &roster(),
        &finite_pair(1.25_f64.to_bits(), (-0.0_f64).to_bits()),
        false,
        false,
        "2DPoint",
    )
}
