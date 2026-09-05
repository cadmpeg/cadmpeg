// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use super::*;
use crate::chunks::TCODE_CRC;
use crate::test_support::test_dump::*;

fn versioned_anonymous_chunk(
    archive: ArchiveVersion,
    major: i32,
    minor: i32,
    body: &[u8],
) -> Vec<u8> {
    let mut payload = major.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    crate::test_support::test_dump::crc_chunk(archive, ANONYMOUS, &payload)
}

fn anonymous_value(minor: i32, body: &[u8]) -> Vec<u8> {
    versioned_anonymous_chunk(ArchiveVersion::V8, 1, minor, body)
}

fn value(type_code: i32, payload: &[u8]) -> Vec<u8> {
    let mut body = type_code.to_le_bytes().to_vec();
    body.extend(7_i32.to_le_bytes());
    body.extend(payload);
    anonymous_value(0, &body)
}

fn id(value: u8) -> Uuid {
    let mut bytes = [0; 16];
    bytes[15] = value;
    Uuid::from_canonical(bytes)
}

fn record(record_id: u8, command: u8, antecedents: &[u8], descendants: &[u8]) -> HistoryRecord {
    HistoryRecord {
        source_range: usize::from(record_id)..usize::from(record_id) + 1,
        id: id(record_id),
        version: 1,
        command_id: id(command),
        descendants: descendants.iter().copied().map(id).collect(),
        antecedents: antecedents.iter().copied().map(id).collect(),
        values: vec![HistoryValue {
            id: 7,
            value: Value::Doubles(vec![2.5]),
        }],
        record_type: RecordType::FeatureParameters,
        copy_on_replace: false,
    }
}

fn source_band_history_record(archive: ArchiveVersion, minor: i32) -> Vec<u8> {
    source_band_history_record_with_major(archive, 1, minor)
}

fn source_band_history_record_with_major(
    archive: ArchiveVersion,
    major: i32,
    minor: i32,
) -> Vec<u8> {
    let mut values_body = 0_i32.to_le_bytes().to_vec();
    values_body.extend([0xcc, 0xdd]);
    let values = anonymous_chunk(archive, 0, &values_body);
    let empty_list = anonymous_chunk(archive, 0, &0_i32.to_le_bytes());

    let mut body = id(1).to_wire().to_vec();
    body.extend(42_i32.to_le_bytes());
    body.extend(id(2).to_wire());
    body.extend(&empty_list);
    body.extend(&empty_list);
    body.extend(values);
    if minor >= 1 {
        body.extend(1_i32.to_le_bytes());
    }
    if minor >= 2 {
        body.push(1);
    }
    body.extend([0xaa, 0xbb]);

    let payload = versioned_anonymous_chunk(archive, major, minor, &body);
    let class = class_wrapper(archive, HISTORY_CLASS.to_wire(), &payload);
    crc_chunk(archive, 0x2000_807b, &class)
}

#[test]
fn projection_links_unique_prior_producers_and_preserves_native_parameters() {
    let records = [record(1, 11, &[], &[40]), record(2, 12, &[40], &[41])];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    assert_eq!(project(&records, None, &mut ir), (0, 0, 0, 0));

    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(
        ir.model.features[1].dependencies,
        vec![ir.model.features[0].id.clone()]
    );
    let cadmpeg_ir::features::FeatureDefinition::Native { kind, parameters } =
        &ir.model.features[1].definition
    else {
        panic!("native history operation");
    };
    assert_eq!(kind.as_str(), "00000000-0000-0000-0000-00000000000c");
    assert_eq!(parameters["value_7"], "2.5");
    assert_eq!(
        ir.model.features[1].source_properties["antecedent_objects"],
        id(40).to_string()
    );
    assert_eq!(
        ir.model.features[1].native_ref.as_deref(),
        Some("rhino:history:record#00000000-0000-0000-0000-000000000002")
    );
}

#[test]
fn projection_counts_dependency_on_later_producer() {
    let records = [record(1, 11, &[40], &[41]), record(2, 12, &[], &[40])];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    assert_eq!(project(&records, None, &mut ir), (0, 0, 1, 0));
    assert!(ir.model.features[0].dependencies.is_empty());
}

#[test]
fn projection_counts_dependency_with_ambiguous_producers() {
    let records = [
        record(1, 11, &[], &[40]),
        record(2, 12, &[], &[40]),
        record(3, 13, &[40], &[41]),
    ];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    assert_eq!(project(&records, None, &mut ir), (0, 0, 1, 0));
    assert!(ir.model.features[2].dependencies.is_empty());
}

#[test]
fn unstored_evaluation_intervals_remain_absent() {
    let mut bytes = 7_i32.to_le_bytes().to_vec();
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(2_i32.to_le_bytes());
    bytes.extend(
        [0.0_f64, 1.0, 2.0, 3.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let value = evaluation(&mut reader, 0).expect("evaluation");
    assert_eq!(value.intervals, [None, None, None]);
    let mut properties = BTreeMap::new();
    evaluation_properties("evaluation", &value, &mut properties);
    assert!(!properties.contains_key("evaluation.interval_0"));
}

#[test]
fn history_value_accepts_a_future_minor_and_skips_its_suffix() {
    let mut body = 2_i32.to_le_bytes().to_vec();
    body.extend(7_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(42_i32.to_le_bytes());
    body.extend([0xaa, 0xbb]);
    let bytes = anonymous_value(9, &body);
    let (parsed, next) = parse_value(&bytes, 0, bytes.len(), ArchiveVersion::V8)
        .expect("future history minor is bounded");
    assert_eq!(next, bytes.len());
    assert!(matches!(parsed.value, Value::Integers(values) if values == [42]));
}

#[test]
fn history_record_writer_bands_follow_archive_version() {
    for (version, archive, minor, copy_on_replace) in [
        ("50", ArchiveVersion::V5, 1, false),
        ("60", ArchiveVersion::V6, 2, true),
    ] {
        let history_record = source_band_history_record(archive, minor);
        let bytes = minimal_document(
            version,
            &[
                crc_table(archive, 0x1000_0014, &[]),
                crc_table(archive, 0x1000_0015, &[]),
                crc_table(archive, 0x1000_0013, &[]),
                crc_table(archive, 0x1000_0026, &[history_record]),
            ],
        );

        let scan = crate::container::scan_owned(bytes).expect("source-shaped history record");
        assert_eq!(scan.history.len(), 1);
        let history = &scan.history[0];
        assert_eq!(history.values.len(), 0);
        assert_eq!(history.record_type, RecordType::FeatureParameters);
        assert_eq!(history.copy_on_replace, copy_on_replace);
    }
}

#[test]
fn future_history_major_is_retained_as_a_complete_record() {
    let archive = ArchiveVersion::V5;
    let future_record = source_band_history_record_with_major(archive, 2, 1);
    let bytes = minimal_document(
        "50",
        &[
            crc_table(archive, 0x1000_0014, &[]),
            crc_table(archive, 0x1000_0015, &[]),
            crc_table(archive, 0x1000_0013, &[]),
            crc_table(archive, 0x1000_0026, std::slice::from_ref(&future_record)),
        ],
    );

    let scan = crate::container::scan_owned(bytes).expect("future history record");
    assert!(scan.history.is_empty());
    let retained = scan
        .opaque_records
        .iter()
        .find(|record| record.table_typecode & !TCODE_CRC == 0x1000_0026)
        .expect("future history record is retained");
    assert_eq!(retained.record.typecode, 0x2000_807b);
    assert_eq!(
        &scan.data[retained.record.range.clone()],
        future_record.as_slice()
    );
}

#[test]
fn projection_preserves_duplicate_values_and_same_record_descendants() {
    let mut producer = record(1, 11, &[], &[40, 40]);
    producer.values.push(HistoryValue {
        id: 7,
        value: Value::Doubles(vec![3.5]),
    });
    let records = [producer, record(2, 12, &[40], &[41])];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    assert_eq!(project(&records, None, &mut ir), (0, 0, 0, 0));

    assert_eq!(
        ir.model.features[1].dependencies,
        vec![ir.model.features[0].id.clone()]
    );
    let cadmpeg_ir::features::FeatureDefinition::Native { parameters, .. } =
        &ir.model.features[0].definition
    else {
        panic!("native history operation");
    };
    assert_eq!(parameters["value_7"], "2.5");
    assert_eq!(parameters["value_7_1"], "3.5");
}

#[test]
fn decoded_history_geometry_is_counted_as_untyped_while_it_stays_stringified() {
    for (class, payload) in [
        (
            crate::test_support::LINE_CLASS,
            crate::test_support::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        ),
        (
            crate::test_support::POINT_CLASS,
            crate::test_support::point_payload([1.0, 2.0, 3.0]),
        ),
    ] {
        let mut geometry_payload = 1_i32.to_le_bytes().to_vec();
        geometry_payload.extend(crate::test_support::class_wrapper(class, &payload));
        let geometry_value = value(10, &anonymous_value(0, &geometry_payload));
        let (parsed, _) = parse_value(&geometry_value, 0, geometry_value.len(), ArchiveVersion::V8)
            .expect("embedded geometry");
        let mut properties = BTreeMap::new();
        let mut sink = GeometrySink {
            untyped: 0,
            failed: 0,
            redundant_repairs: 0,
        };
        crate::decode::with_expand_bytes(&geometry_value, |expand| {
            structured_value_properties(
                "value_7",
                &parsed.value,
                Some((expand, ArchiveVersion::V8, None, 2.0)),
                &mut properties,
                &mut sink,
            );
        });
        assert_eq!(sink.untyped, 1);
        assert!(properties.contains_key("value_7.0.geometry"));
    }
}

#[test]
fn embedded_geometry_polyedge_and_subd_chain_values_are_typed() {
    let geometry = crate::test_support::class_wrapper(
        crate::test_support::POINT_CLASS,
        &crate::test_support::point_payload([1.0, 2.0, 3.0]),
    );
    let mut geometry_payload = 1_i32.to_le_bytes().to_vec();
    geometry_payload.extend(geometry);
    let geometry_value = value(10, &anonymous_value(0, &geometry_payload));
    let (parsed, next) = parse_value(&geometry_value, 0, geometry_value.len(), ArchiveVersion::V8)
        .expect("embedded geometry");
    assert_eq!(next, geometry_value.len());
    assert!(matches!(&parsed.value, Value::Geometries(values)
        if values.len() == 1
            && values[0].class_id == Uuid::from_wire(crate::test_support::POINT_CLASS)));
    let mut properties = BTreeMap::new();
    let mut sink = GeometrySink {
        untyped: 0,
        failed: 0,
        redundant_repairs: 0,
    };
    crate::decode::with_expand_bytes(&geometry_value, |expand| {
        structured_value_properties(
            "value_7",
            &parsed.value,
            Some((expand, ArchiveVersion::V8, None, 2.0)),
            &mut properties,
            &mut sink,
        );
    });
    // A point has no neutral carrier, so it stays a stringified coordinate
    // and is counted as untyped.
    assert_eq!(sink.untyped, 1);
    assert_eq!(
        properties["value_7.0.geometry"],
        r#"{"x":2.0,"y":4.0,"z":6.0}"#
    );

    let mut polyedge = 0_i32.to_le_bytes().to_vec();
    polyedge.extend(2_i32.to_le_bytes());
    polyedge.extend(0.25_f64.to_le_bytes());
    polyedge.extend(0.75_f64.to_le_bytes());
    polyedge.extend(3_i32.to_le_bytes());
    let mut polyedges = 1_i32.to_le_bytes().to_vec();
    polyedges.extend(anonymous_value(0, &polyedge));
    let polyedge_value = value(13, &anonymous_value(0, &polyedges));
    let (parsed, _) = parse_value(&polyedge_value, 0, polyedge_value.len(), ArchiveVersion::V8)
        .expect("polyedge");
    let Value::PolyEdges(values) = parsed.value else {
        panic!("expected polyedges");
    };
    assert_eq!(values.len(), 1);
    assert!(values[0].segments.is_empty());
    assert_eq!(values[0].parameters, [0.25, 0.75]);
    assert_eq!(values[0].evaluation_mode, Some(3));

    let subd_id = id(42);
    let mut chain = [0_u8; 16].to_vec();
    chain[15] = 42;
    chain.extend(2_i32.to_le_bytes());
    chain.extend(2_i32.to_le_bytes());
    chain.extend(11_u32.to_le_bytes());
    chain.extend(12_u32.to_le_bytes());
    chain.extend(2_i32.to_le_bytes());
    chain.extend([0, 1]);
    let mut chains = 1_i32.to_le_bytes().to_vec();
    chains.extend(anonymous_value(1, &chain));
    let chain_value = value(14, &anonymous_value(1, &chains));
    let (parsed, _) = parse_value(&chain_value, 0, chain_value.len(), ArchiveVersion::V8)
        .expect("SubD edge chain");
    assert!(matches!(parsed.value, Value::SubdEdgeChains(values)
        if values.len() == 1
            && values[0].subd_id == subd_id
            && values[0].edge_ids == [11, 12]
            && values[0].orientations == [0, 1]));
}

#[test]
fn subd_edge_chain_count_mismatch_drops_dependent_arrays_with_a_diagnostic() {
    let mut chain = [0_u8; 16].to_vec();
    chain[15] = 42;
    chain.extend(2_i32.to_le_bytes());
    chain.extend(2_i32.to_le_bytes());
    chain.extend(11_u32.to_le_bytes());
    chain.extend(12_u32.to_le_bytes());
    chain.extend(1_i32.to_le_bytes());
    chain.push(0);
    let mut chains = 1_i32.to_le_bytes().to_vec();
    chains.extend(anonymous_value(1, &chain));
    let value = value(14, &anonymous_value(1, &chains));
    let mut warnings = Vec::new();
    let (parsed, _) =
        parse_value_with_warnings(&value, 0, value.len(), ArchiveVersion::V8, &mut warnings)
            .expect("mismatched redundant arrays remain bounded");
    assert!(matches!(
        parsed.value,
        Value::SubdEdgeChains(values)
            if values.len() == 1
                && values[0].edge_ids.is_empty()
                && values[0].orientations.is_empty()
    ));
    assert_eq!(warnings.len(), 1);
}

#[test]
fn embedded_cage_projects_exact_construction_semantics() {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend(3_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    for _ in 0..6 {
        body.extend(2_i32.to_le_bytes());
    }
    for axis in 0..3 {
        body.extend(0.0_f64.to_le_bytes());
        body.extend((axis as f64 + 1.0).to_le_bytes());
    }
    for index in 0..8 {
        for coordinate in [index as f64, 0.0, 0.0] {
            body.extend(coordinate.to_le_bytes());
        }
    }
    let bytes = crate::test_support::crc_chunk(ANONYMOUS, &body);
    let geometry = EmbeddedGeometry {
        class_id: crate::cage::CLASS,
        class_data_range: 0..bytes.len(),
        userdata: Vec::new(),
    };
    let semantic = crate::decode::with_expand_bytes(&bytes, |expand| {
        extended_geometry_json(expand, &geometry, ArchiveVersion::V8, None, 10.0)
    })
    .expect("cage semantics");
    let semantic: serde_json::Value = serde_json::from_str(&semantic).expect("required invariant");
    assert_eq!(semantic["kind"], "nurbs_cage");
    assert_eq!(semantic["orders"], serde_json::json!([2, 2, 2]));
    assert_eq!(
        semantic["control_points"][7],
        serde_json::json!([70.0, 0.0, 0.0])
    );

    let empty_subd = [0_u8];
    let geometry = EmbeddedGeometry {
        class_id: crate::subd::ON_SUBD,
        class_data_range: 0..1,
        userdata: Vec::new(),
    };
    let semantic = crate::decode::with_expand_bytes(&empty_subd, |expand| {
        extended_geometry_json(expand, &geometry, ArchiveVersion::V8, None, 1.0)
    })
    .expect("empty SubD semantics");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&semantic).expect("required invariant"),
        serde_json::json!({"kind": "subd", "empty": true})
    );

    let brep = crate::test_support::brep_payload(false);
    let geometry = EmbeddedGeometry {
        class_id: crate::brep::ON_BREP,
        class_data_range: 0..brep.len(),
        userdata: Vec::new(),
    };
    let semantic = crate::decode::with_expand_bytes(&brep, |expand| {
        extended_geometry_json(expand, &geometry, ArchiveVersion::V8, None, 10.0)
    })
    .expect("Brep topology semantics");
    let semantic: serde_json::Value = serde_json::from_str(&semantic).expect("required invariant");
    assert_eq!(semantic["kind"], "brep");
    assert_eq!(
        semantic["bodies"]
            .as_array()
            .expect("required invariant")
            .len(),
        1
    );
    assert_eq!(
        semantic["faces"]
            .as_array()
            .expect("required invariant")
            .len(),
        1
    );
    assert_eq!(
        semantic["vertices"]
            .as_array()
            .expect("required invariant")
            .len(),
        3
    );
}

#[test]
fn scan_retains_history_record_source_boundaries() {
    let archive = ArchiveVersion::V5;
    let history_record = crc_chunk(archive, 0x2000_807b, &[1, 2, 3, 4]);
    let bytes = minimal_document(
        "50",
        &[
            crc_table(archive, 0x1000_0014, &[]),
            crc_table(archive, 0x1000_0015, &[]),
            crc_table(archive, 0x1000_0013, &[]),
            crc_table(archive, 0x1000_0026, &[history_record]),
        ],
    );

    let scan = crate::container::scan_owned(bytes).expect("history table");
    let history = scan
        .tables
        .iter()
        .find(|table| table.typecode & !TCODE_CRC == 0x1000_0026)
        .expect("history table descriptor");
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].typecode, 0x2000_807b);
    assert_eq!(&scan.data[history.records[0].body.clone()], &[1, 2, 3, 4]);
}

#[test]
pub(crate) fn scan_decodes_history_identity_dependencies_and_typed_values() {
    let archive = ArchiveVersion::V5;
    let record_id = [1, 0, 0, 0, 2, 0, 3, 0, 4, 5, 6, 7, 8, 9, 10, 11];
    let command_id = [12, 0, 0, 0, 13, 0, 14, 0, 15, 16, 17, 18, 19, 20, 21, 22];
    let descendant = [23, 0, 0, 0, 24, 0, 25, 0, 26, 27, 28, 29, 30, 31, 32, 33];
    let antecedent = [34, 0, 0, 0, 35, 0, 36, 0, 37, 38, 39, 40, 41, 42, 43, 44];
    let uuid_list = |uuid: [u8; 16]| {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(uuid);
        anonymous_chunk(archive, 0, &body)
    };
    let value = |kind: i32, id: i32, payload: &[u8]| {
        let mut body = kind.to_le_bytes().to_vec();
        body.extend(id.to_le_bytes());
        body.extend(payload);
        anonymous_chunk(archive, 0, &body)
    };
    let mut integers = 2_i32.to_le_bytes().to_vec();
    integers.extend(7_i32.to_le_bytes());
    integers.extend((-9_i32).to_le_bytes());
    let mut text = 1_i32.to_le_bytes().to_vec();
    text.extend(utf16_bytes("distance"));
    let referenced_object = [45, 0, 0, 0, 46, 0, 47, 0, 48, 49, 50, 51, 52, 53, 54, 55];
    let mut object_reference = referenced_object.to_vec();
    object_reference.extend(7_i32.to_le_bytes());
    object_reference.extend(8_i32.to_le_bytes());
    object_reference.extend(4_i32.to_le_bytes());
    for coordinate in [1.0_f64, 2.0, 3.0] {
        object_reference.extend(coordinate.to_le_bytes());
    }
    object_reference.extend(9_i32.to_le_bytes());
    object_reference.extend(10_i32.to_le_bytes());
    object_reference.extend(11_i32.to_le_bytes());
    for parameter in [0.1_f64, 0.2, 0.3, 0.4] {
        object_reference.extend(parameter.to_le_bytes());
    }
    object_reference.extend(0_i32.to_le_bytes());
    for bound in [0.0_f64, 1.0, 2.0, 3.0, 4.0, 5.0] {
        object_reference.extend(bound.to_le_bytes());
    }
    object_reference.extend(12_i32.to_le_bytes());
    let object_reference = anonymous_chunk(archive, 3, &object_reference);
    let mut object_references = 1_i32.to_le_bytes().to_vec();
    object_references.extend(object_reference);
    let values = [
        value(2, 10, &integers),
        value(8, 20, &text),
        value(9, 25, &object_references),
        value(99, 30, &[0xaa, 0xbb]),
    ];
    let mut values_body = 4_i32.to_le_bytes().to_vec();
    values_body.extend(values.concat());
    let mut body = record_id.to_vec();
    body.extend(202_607_130_i32.to_le_bytes());
    body.extend(command_id);
    body.extend(uuid_list(descendant));
    body.extend(uuid_list(antecedent));
    body.extend(anonymous_chunk(archive, 0, &values_body));
    body.extend(1_i32.to_le_bytes());
    body.push(1);
    let payload = anonymous_chunk(archive, 2, &body);
    let history_class = [
        0x2f, 0xfd, 0xd0, 0xec, 0x88, 0x20, 0xdc, 0x49, 0x96, 0x41, 0x9c, 0xf7, 0xa2, 0x8f, 0xfa,
        0x6b,
    ];
    let record = crc_chunk(
        archive,
        0x2000_807b,
        &class_wrapper(archive, history_class, &payload),
    );
    let bytes = minimal_document(
        "50",
        &[
            crc_table(archive, 0x1000_0014, &[]),
            crc_table(archive, 0x1000_0015, &[]),
            crc_table(archive, 0x1000_0013, &[]),
            crc_table(archive, 0x1000_0026, &[record]),
        ],
    );

    let scan = crate::container::scan_owned(bytes).expect("typed history record");
    let history = &scan.history[0];
    assert_eq!(
        history.id.to_string(),
        "00000001-0002-0003-0405-060708090a0b"
    );
    assert_eq!(history.version, 202_607_130);
    assert_eq!(
        history.command_id.to_string(),
        "0000000c-000d-000e-0f10-111213141516"
    );
    assert_eq!(history.descendants.len(), 1);
    assert_eq!(history.antecedents.len(), 1);
    assert_eq!(history.values.len(), 4);
    assert!(matches!(
        &history.values[0].value,
        crate::history::Value::Integers(values) if values == &[7, -9]
    ));
    assert!(matches!(
        &history.values[1].value,
        crate::history::Value::Strings(values) if values == &["distance"]
    ));
    assert!(matches!(
        &history.values[2].value,
        crate::history::Value::ObjectReferences(values)
            if values.len() == 1
                && values[0].object_id.to_string() == "0000002d-002e-002f-3031-323334353637"
                && values[0].component == [7, 8]
                && values[0].geometry_type == 4
                && values[0].point.0 == [1.0, 2.0, 3.0]
                && values[0].evaluation.parameter_type == 9
                && values[0].evaluation.component == [10, 11]
                && values[0].evaluation.parameters == [0.1, 0.2, 0.3, 0.4]
                && values[0].evaluation.intervals
                    == [Some([0.0, 1.0]), Some([2.0, 3.0]), Some([4.0, 5.0])]
                && values[0].instance_path.is_empty()
                && values[0].osnap_mode == 12
    ));
    assert!(matches!(
        history.values[3].value,
        crate::history::Value::Opaque { type_code: 99, .. }
    ));
    assert_eq!(
        history.record_type,
        crate::history::RecordType::FeatureParameters
    );
    assert!(history.copy_on_replace);

    let decoded = crate::decode::decode_for_test(&scan);
    assert_eq!(decoded.ir().model.features.len(), 1);
    assert_eq!(
        decoded.ir().model.features[0].native_ref.as_deref(),
        Some("rhino:history:record#00000001-0002-0003-0405-060708090a0b")
    );
}
