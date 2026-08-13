// SPDX-License-Identifier: Apache-2.0
//! Object-graph, entity-table, catalog, and value-block CATPart builders.

#![allow(clippy::unwrap_used)]
use super::{be32, outer_container_catpart, standard_catpart};

pub(crate) fn outer_container_object_graph_catpart() -> (Vec<u8>, u64) {
    outer_container_catpart(&object_graph_stream())
}

pub(crate) fn object_graph_record(head: &[u8], payload: &[u8]) -> Vec<u8> {
    let child_len = 6 + payload.len();
    let total_len = 6 + head.len() + child_len;
    let mut bytes = vec![0x7c, 0x09];
    bytes.extend_from_slice(&(total_len as u32).to_le_bytes());
    bytes.extend_from_slice(head);
    bytes.extend_from_slice(&[0x7c, 0x0a]);
    bytes.extend_from_slice(&(child_len as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

pub(crate) fn inline_object_graph_record(body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x09];
    bytes.extend_from_slice(&(6_u32 + body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

pub(crate) fn object_graph_from_records(records: &[Vec<u8>]) -> Vec<u8> {
    let total_len = 6 + records.iter().map(Vec::len).sum::<usize>();
    let mut bytes = vec![0x7c, 0x08];
    bytes.extend_from_slice(&(total_len as u32).to_le_bytes());
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes
}

pub(crate) fn entity_table_record(entity_id: u32) -> Vec<u8> {
    entity_table_record_with_value(entity_id, &[])
}

pub(crate) fn entity_table_record_with_value(entity_id: u32, value: &[u8]) -> Vec<u8> {
    entity_table_record_with_definition_and_value(entity_id, &[0x01], value)
}

pub(crate) fn entity_table_record_with_definition_and_value(
    entity_id: u32,
    definition_prefix: &[u8],
    value: &[u8],
) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x05, 0, 0, 0, 0, 0x00, 0x7c, 0x06];
    bytes.extend_from_slice(
        &u32::try_from(definition_prefix.len() + 11)
            .expect("generated 7C06 length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(definition_prefix);
    bytes.push(0xea);
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&[0x7c, 0x07]);
    bytes.extend_from_slice(
        &u32::try_from(value.len() + 6)
            .expect("generated 7C07 length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value);
    let total_len = u32::try_from(bytes.len()).expect("generated 7C05 length");
    bytes[2..6].copy_from_slice(&total_len.to_le_bytes());
    bytes
}

pub(crate) fn entity_backed_object_graph(records: &[Vec<u8>], entity_ids: &[u32]) -> Vec<u8> {
    assert_eq!(records.len(), entity_ids.len());
    let mut bytes = entity_ids
        .iter()
        .flat_map(|entity_id| entity_table_record(*entity_id))
        .collect::<Vec<_>>();
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(records));
    bytes
}

pub(crate) fn sequential_entity_backed_object_graph(records: &[Vec<u8>]) -> Vec<u8> {
    let entity_ids = (1..=u32::try_from(records.len()).expect("bounded generated entity count"))
        .collect::<Vec<_>>();
    entity_backed_object_graph(records, &entity_ids)
}

pub(crate) fn object_graph_stream() -> Vec<u8> {
    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x82, 0x83, 0x84],
            &[0x81, 0x85, 0x3a, 0x87, 0xfe],
        ),
        object_graph_record(
            &[0x14, 0x01, 0x82, 0x84],
            &[0xe5, 0x02, 0, 0, 0, 0xaa, 0xbb, 0xfe],
        ),
    ];
    object_graph_from_records(&records)
}

pub(crate) fn object_graph_vm_stream() -> Vec<u8> {
    object_graph_from_records(&[
        object_graph_record(
            &[0x1c, 0x01, 0x82, 0x80, 0xff, 0xff, 0xff, 0xff, 0x83],
            &[0x3b, 0x83, 0x81, 0x85, 0x80, 0x86, 0xd1, 0x09, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x83], &[0xfe]),
    ])
}

pub(crate) fn object_graph_ambiguous_3c_stream() -> Vec<u8> {
    object_graph_from_records(&[object_graph_record(
        &[0x1c, 0x01, 0x82, 0x80, 0xff, 0xff, 0xff, 0xff, 0x83],
        &[
            0x3c, 0x80, 0x01, 0x00, 0x00, 0x00, 0x81, 0x80, 0x80, 0x00, 0x00, 0x00, 0x80, 0x2f,
            0x69, 0x00, 0x00, 0xfe,
        ],
    )])
}

pub(crate) fn object_graph_bulk_table_stream() -> Vec<u8> {
    object_graph_from_records(&[object_graph_record(
        &[0x1c, 0x01, 0x82, 0x80, 0xff, 0xff, 0xff, 0xff, 0x83],
        &[
            0x3c, 0x80, 0x03, 0x00, 0x00, 0x00, 0x81, 0x91, 0x80, 0x2f, 0x69, 0x00, 0x00, 0x81,
            0xd2, 0x00, 0x80, 0x31, 0x69, 0x00, 0x00, 0x81, 0x80, 0x01, 0x14, 0x00, 0x00, 0x80,
            0x33, 0x69, 0x00, 0x00, 0x3a, 0x85, 0xfe,
        ],
    )])
}

pub(crate) fn catalog_stream(entries: &[&str]) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x02, 0, 0, 0, 0];
    bytes.push(0x80 + u8::try_from(entries.len() + 1).unwrap());
    for entry in entries {
        bytes.push(u8::try_from(entry.len() + 1).unwrap());
        bytes.extend_from_slice(entry.as_bytes());
    }
    let total_len = u32::try_from(bytes.len()).unwrap();
    bytes[2..6].copy_from_slice(&total_len.to_le_bytes());
    bytes
}

pub(crate) fn value_block_stream(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x0b, 0, 0, 0, 0];
    bytes.extend_from_slice(payload);
    let declared_len = u32::try_from(bytes.len()).expect("generated 7C0B length");
    bytes[2..6].copy_from_slice(&declared_len.to_le_bytes());
    bytes.push(0xfe);
    bytes
}

pub(crate) fn standard_catpart_with_object_graph() -> Vec<u8> {
    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x82, 0x83, 0x84],
            &[0x81, 0x85, 0x3a, 0x87, 0xfe],
        ),
        object_graph_record(
            &[0x14, 0x01, 0x82, 0x84],
            &[0xe5, 0x02, 0, 0, 0, 0xaa, 0xbb, 0xfe],
        ),
    ];
    let graph = entity_backed_object_graph(&records, &[1, 2]);
    let mut file = standard_catpart();
    file.splice(16..16, graph);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_nested_design_objects() -> Vec<u8> {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x83, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x83, 0x84], &[0xfe]),
    ];
    let graph = entity_backed_object_graph(&records, &[1, 2, 3]);
    let mut file = standard_catpart();
    file.splice(16..16, graph);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_catalog() -> Vec<u8> {
    let catalog = catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        "Pad",
        "GSMLoft",
        "GSMPointBetweenValues",
        "GSMPlaneAngle",
    ]);
    let mut file = standard_catpart();
    file.splice(16..16, catalog);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_value_block() -> Vec<u8> {
    let mut stream = object_graph_stream();
    stream.extend(value_block_stream(&[
        0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82,
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "VPGlobal",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_repeated_reference_schema_selection() -> Vec<u8> {
    let mut payload = vec![0xac, 0xe5];
    payload.extend_from_slice(&59_u32.to_le_bytes());
    payload.extend_from_slice(&[0; 59]);
    payload.extend_from_slice(&[
        0x85, 0xae, 0x84, 0xb0, 0x82, 0x81, 0x81, 0x81, 0x82, 0x82, 0x81, 0x81, 0xd1, 0x80, 0xfe,
    ]);
    let mut stream =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x84], &payload)]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "TargetSchema",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_entity_value_schema_selection() -> Vec<u8> {
    let mut value = vec![0x32, 4, 0, 0, 0, 0x87, 0xe6];
    value.extend_from_slice(&12.5_f64.to_bits().to_le_bytes());
    value.extend_from_slice(&[0xe8, 0xe0, 0x0a, 0x37, 0xfe, 0xfe]);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_value(1, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "TargetValue",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_crossing_entity_value_packet() -> Vec<u8> {
    let value = [
        0x32, 4, 0, 0, 0, 0x81, 0x82, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84, 0xe6, 0x32, 4, 0, 0, 0,
        0, 0, 0, 0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_value(1, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "TargetValue",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_numeric_entity_value_pair() -> Vec<u8> {
    let value = [
        0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6, 0, 0, 0, 0, 0, 0, 0x12, 0x40, 0xe8,
        0xfe, 0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut stream = entity_table_record_with_value(1, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_visualization_values_only() -> Vec<u8> {
    let mut stream = value_block_stream(&[0x32, 4, 0, 0, 0, 0x83]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "VPGlobal",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_design_class(class: &str) -> Vec<u8> {
    let mut stream = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    stream.extend(value_block_stream(&[0x81]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "CurrentFeature",
        class,
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}
