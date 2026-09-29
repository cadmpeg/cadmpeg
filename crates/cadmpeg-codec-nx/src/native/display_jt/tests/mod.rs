// SPDX-License-Identifier: Apache-2.0

fn with_jt_context<T>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test decode context");
    f(&ctx)
}

fn with_jt_budget<T>(
    container: &crate::container::Container,
    run: impl FnOnce(
        (
            &cadmpeg_core::decode::DecodeContext<'_>,
            cadmpeg_core::decode::View<'_>,
        ),
    ) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        container.data.as_ref(),
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test DisplayJT root");
    run((&ctx, root))
}

fn high_degree_lane_count(representation: &[u8], bindings: u64) -> Option<usize> {
    with_jt_context(|ctx| {
        super::jt9_topology_high_degree_lane_count(ctx, representation, bindings)
            .expect("service JT budget")
    })
}

#[test]
fn transformed_jt_geometry_holds_checked_points_and_normals() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let point = super::transform_jt_point(identity, [1.0, -2.0, 3.0]).unwrap();
    assert_eq!(
        point.get(),
        cadmpeg_ir::math::Point3::new(1000.0, -2000.0, 3000.0)
    );
    let normal = super::transform_jt_normal(identity, [3.0, 4.0, 0.0]).unwrap();
    assert_eq!(
        *normal.as_raw(),
        cadmpeg_ir::math::Vector3::new(0.6, 0.8, 0.0)
    );
    assert!(super::transform_jt_normal(identity, [0.0, 0.0, 0.0]).is_none());
    let mut overflow = identity;
    overflow[3][0] = f64::MAX;
    assert!(super::transform_jt_point(overflow, [1.0, 0.0, 0.0]).is_none());
}

#[test]
fn numerical_followup_jt_transform_accepts_extreme_finite_scales() {
    for scale in [1.0_f32, 1e20, 1e-30] {
        let mut body = 1_u16.to_le_bytes().to_vec();
        body.push(0);
        body.extend(0_u32.to_le_bytes());
        body.extend(1_u16.to_le_bytes());
        body.extend(0x8420_u16.to_le_bytes());
        for _ in 0..3 {
            body.extend(scale.to_le_bytes());
        }
        let (_, _, _, matrix) = super::parse_jt9_geometric_transform_body(&body).unwrap();
        let matrix = matrix.get();
        assert_eq!([matrix[0][0], matrix[1][1], matrix[2][2]], [scale; 3]);
    }
}

#[test]
fn range_limits_admit_only_finite_nonnegative_increasing_values() {
    for valid in [vec![], vec![0.0], vec![0.0, 1.0, 2.0]] {
        let admitted = super::JtRangeLimits::try_from(valid.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&admitted).unwrap(),
            serde_json::json!(valid)
        );
        assert_eq!(
            serde_json::to_vec(&admitted).unwrap(),
            serde_json::to_vec(&Vec::<f32>::from(admitted.clone())).unwrap()
        );
        assert_eq!(
            serde_json::from_value::<super::JtRangeLimits>(serde_json::json!(valid)).unwrap(),
            admitted
        );
    }
    for invalid in [
        vec![-1.0],
        vec![1.0, 1.0],
        vec![2.0, 1.0],
        vec![f32::NAN],
        vec![f32::INFINITY],
    ] {
        assert!(super::JtRangeLimits::try_from(invalid).is_err());
    }
    assert!(serde_json::from_str::<super::JtRangeLimits>("[2,1]").is_err());
}

#[test]
fn jt_range_limits_native_limit_refuses_before_owned_wire_conversion() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        limits: &'a super::JtRangeLimits,
    }
    let limits = super::JtRangeLimits::try_from(vec![0.0, 1.0, 2.0]).unwrap();
    let record = Record {
        id: "nx:jt:range-limits#0",
        limits: &limits,
    };
    super::JT_RANGE_LIMITS_INTO_WIRE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id":"nx:jt:range-limits#0", "limits":[0.0,1.0,2.0]}),
    );
    super::JT_RANGE_LIMITS_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn compression_wire_checks_constants_and_length_without_changing_evidence() {
    let valid = serde_json::json!({
        "flag": 2, "algorithm": 2, "compressed_data_byte_len": 4,
        "compressed_byte_len": 3, "inflated_sha256": cadmpeg_ir::hash::sha256_hex(b"hash")
    });
    let value: super::DisplayJtCompression = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!(serde_json::to_value(&value).unwrap(), valid);
    assert_eq!(
        serde_json::to_vec(&value).unwrap(),
        serde_json::to_vec(&super::DisplayJtCompressionWire::from(value.clone())).unwrap()
    );
    for (field, invalid) in [
        ("flag", 0),
        ("algorithm", 1),
        ("compressed_data_byte_len", 3),
        ("compressed_byte_len", u32::MAX),
    ] {
        let mut wire = valid.clone();
        wire[field] = invalid.into();
        assert!(serde_json::from_value::<super::DisplayJtCompression>(wire).is_err());
    }
}

#[test]
fn jt_compression_native_limit_refuses_before_owned_wire_conversion() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        compression: &'a super::DisplayJtCompression,
    }
    let value: super::DisplayJtCompression = serde_json::from_value(serde_json::json!({
        "flag": 2, "algorithm": 2, "compressed_data_byte_len": 4,
        "compressed_byte_len": 3, "inflated_sha256": cadmpeg_ir::hash::sha256_hex(b"hash")
    }))
    .unwrap();
    let record = Record {
        id: "nx:jt:compression#0",
        compression: &value,
    };
    super::JT_COMPRESSION_INTO_WIRE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id":"nx:jt:compression#0", "compression": value}),
    );
    super::JT_COMPRESSION_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn jt_shape_lod_element_borrowed_wire_and_native_limit() {
    let wire = serde_json::json!({
        "id": "nx:jt:shape-lod-element#0", "segment": "nx:jt:segment#0",
        "ordinal": 0, "object_type_id": vec![7; 16], "object_base_type": 4,
        "object_id": 9, "body_byte_len": 3,
        "body_sha256": cadmpeg_ir::hash::sha256_hex(b"body"), "source_offset": 10
    });
    let value: super::DisplayJtShapeLodElement = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(&value).unwrap(),
        serde_json::to_vec(&super::DisplayJtShapeLodElementWire::from(value.clone())).unwrap()
    );
    super::JT_SHAPE_LOD_INTO_WIRE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(&value, wire);
    super::JT_SHAPE_LOD_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn jt_compressed_element_borrowed_wire_and_native_limit() {
    let wire = serde_json::json!({
        "id": "nx:jt:compressed-element#0", "segment": "nx:jt:segment#0",
        "segment_type": 7, "ordinal": 0, "object_type_id": vec![7; 16],
        "object_base_type": 4, "object_id": 9, "body_byte_len": 3,
        "body_sha256": cadmpeg_ir::hash::sha256_hex(b"body"),
        "inflated_offset": 0, "source_offset": 10
    });
    let value: super::DisplayJtCompressedElement = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(&value).unwrap(),
        serde_json::to_vec(&super::DisplayJtCompressedElementWire::from(value.clone())).unwrap()
    );
    super::JT_COMPRESSED_ELEMENT_INTO_WIRE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(&value, wire);
    super::JT_COMPRESSED_ELEMENT_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn jt_compressed_sequence_borrowed_wire_and_native_limit() {
    let wire = serde_json::json!({
        "id": "nx:jt:compressed-sequence#0", "segment": "nx:jt:segment#0",
        "segment_type": 7, "elements": ["nx:jt:compressed-element#0"],
        "framed_byte_len": 48, "tail": [6, 5],
        "tail_sha256": cadmpeg_ir::hash::sha256_hex(&[6, 5]),
        "source_offset": 10
    });
    let value: super::DisplayJtCompressedElementSequence =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(&value).unwrap(),
        serde_json::to_vec(&super::DisplayJtCompressedElementSequenceWire::from(
            value.clone()
        ))
        .unwrap()
    );
    super::JT_COMPRESSED_SEQUENCE_INTO_WIRE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(&value, wire);
    super::JT_COMPRESSED_SEQUENCE_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn index_wire_preserves_count_and_rejects_invalid_rows() {
    let wire = r#"{"id":"index","version":9,"declared_count":1,"rows":[{"id":"row","ordinal":0,"header_offset":28,"value":100,"source_offset":8}],"source_offset":0}"#;
    let index: super::DisplayJtIndex = serde_json::from_str(wire).unwrap();
    assert_eq!(index.declared_count(), 1);
    assert_eq!(serde_json::to_string(&index).unwrap(), wire);
    assert_eq!(
        serde_json::to_vec(&index).unwrap(),
        serde_json::to_vec(&super::DisplayJtIndexWire::from(index.clone())).unwrap()
    );
    for (invalid, field) in [
        (
            wire.replace("\"declared_count\":1", "\"declared_count\":7"),
            "declared_count",
        ),
        (wire.replace("\"value\":100", "\"value\":0"), "value"),
        (
            r#"{"id":"index","version":9,"declared_count":0,"rows":[],"source_offset":0}"#
                .to_string(),
            "rows",
        ),
    ] {
        let error = serde_json::from_str::<super::DisplayJtIndex>(&invalid).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn index_native_limit_refuses_before_clone() {
    let wire = r#"{"id":"nx:jt:index#1","version":9,"declared_count":1,"rows":[{"id":"nx:jt:row#1","ordinal":0,"header_offset":28,"value":100,"source_offset":8}],"source_offset":0}"#;
    let index: super::DisplayJtIndex = serde_json::from_str(wire).unwrap();
    super::JT_INDEX_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &index,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
    super::JT_INDEX_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn document_wire_derives_version_numbers_and_rejects_disagreement() {
    let mut wire = serde_json::json!({
        "id": "document", "index_row": "row",
        "version_field": format!("{:<80}", "Version +0009.005"),
        "format_major": 9, "format_minor": 5, "byte_order": 0,
        "toc_offset": 105, "lsg_segment_id": vec![0; 16], "toc_entries": [],
        "physical_byte_len": 105, "source_offset": 0
    });
    let document: super::DisplayJtDocument = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&document).unwrap(), wire);
    assert_eq!(
        serde_json::to_vec(&document).unwrap(),
        serde_json::to_vec(&super::DisplayJtDocumentWire::from(document.clone())).unwrap()
    );
    let mut invalid = wire.clone();
    invalid["byte_order"] = serde_json::json!(1);
    assert!(serde_json::from_value::<super::DisplayJtDocument>(invalid).is_err());
    wire["format_minor"] = serde_json::json!(6);
    assert!(serde_json::from_value::<super::DisplayJtDocument>(wire).is_err());
}

#[test]
fn document_native_limit_refuses_before_clone() {
    let wire = serde_json::json!({
        "id": "nx:jt:document#1", "index_row": "nx:jt:row#1",
        "version_field": format!("{:<80}", "Version +0009.005"),
        "format_major": 9, "format_minor": 5, "byte_order": 0,
        "toc_offset": 105, "lsg_segment_id": vec![0; 16], "toc_entries": [],
        "physical_byte_len": 105, "source_offset": 0
    });
    let document: super::DisplayJtDocument = serde_json::from_value(wire.clone()).unwrap();
    super::JT_DOCUMENT_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(&document, wire);
    super::JT_DOCUMENT_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn string_property_wire_derives_exact_utf16_and_rejects_disagreement() {
    let wire = r#"{"id":"atom","element":"element","object_id":1,"code_units":[78,88,55357,56960],"value":"NX🚀","source_offset":0}"#;
    let record: super::DisplayJtStringPropertyAtom = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&super::DisplayJtStringPropertyAtomWire::from(
            record.clone()
        ))
        .unwrap()
    );
    let inconsistent = wire.replace("[78,88,55357,56960]", "[78,88,55357]");
    assert!(serde_json::from_str::<super::DisplayJtStringPropertyAtom>(&inconsistent).is_err());
}

#[test]
fn jt_string_property_native_limit_refuses_before_code_unit_allocation() {
    let wire = serde_json::json!({
        "id": "nx:jt:string-property#0", "element": "nx:jt:compressed-element#0",
        "object_id": 1, "code_units": [78, 88, 55357, 56960],
        "value": "NX🚀", "source_offset": 0
    });
    let record: super::DisplayJtStringPropertyAtom = serde_json::from_value(wire.clone()).unwrap();
    super::JT_STRING_PROPERTY_INTO_WIRE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire);
    super::JT_STRING_PROPERTY_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::super::hex::Sha256Hex;
use super::{DisplayJtMaterialAttribute, DisplayJtPartitionBounds, FiniteBinary32, UnitBinary32};
use cadmpeg_ir::topology::Color;

const EPS_JT_TRANSFORMED_VERTEX: f64 = 1.0e-6;

mod framing;
mod wires;

fn finite<const N: usize>(values: [f32; N]) -> [FiniteBinary32; N] {
    values.map(|value| FiniteBinary32::new(value).expect("fixture values are finite"))
}

#[test]
fn display_jt_index_requires_every_declared_header() {
    use crate::container::{Container, DirEntry, Region};

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let mut inflated = Vec::new();
    inflated.extend_from_slice(&24_u32.to_le_bytes());
    inflated.extend_from_slice(&[3; 16]);
    inflated.push(1);
    inflated.extend_from_slice(&5_u32.to_le_bytes());
    inflated.extend_from_slice(&[9, 8, 7]);
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    inflated.extend_from_slice(&[6, 5]);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&inflated).expect("required invariant");
    let compressed = encoder.finish().expect("required invariant");
    let segment_byte_len = 24 + 9 + compressed.len() as u32;
    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&2_u32.to_le_bytes());
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2);
    data.extend_from_slice(&compressed);
    let physical_size = data.len() as u64;
    let data_len = data.len() as u64;
    let container = Container {
        data: data.clone().into(),
        physical_size,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_string(),
            region: Region::Footer,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: data_len,
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let indices = super::display_jt_indices(&ctx, &container).unwrap();
    assert_eq!(indices[0].version, 9);
    assert_eq!(indices[0].declared_count(), 1);
    assert_eq!(indices[0].rows.first().header_offset, 28);
    assert_eq!(indices[0].rows.first().value.get(), 100);
    let documents = super::display_jt_documents(&ctx, &container, &indices).unwrap();
    assert_eq!(
        (documents[0].version.major(), documents[0].version.minor()),
        (9, 4)
    );
    assert_eq!(documents[0].toc_offset, 105);
    assert_eq!(
        documents[0].physical_byte_len,
        137 + u64::from(segment_byte_len)
    );
    assert_eq!(documents[0].toc_entries.len(), 1);
    assert_eq!(documents[0].toc_entries[0].segment_offset, 137);
    assert_eq!(
        documents[0].toc_entries[0].segment_byte_len,
        segment_byte_len
    );
    assert_eq!(documents[0].toc_entries[0].attributes, [0, 0, 0, 1]);
    let segments = with_jt_budget(&container, |budget| {
        super::display_jt_segments(budget, &container, &documents)
    })
    .unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].id.matches('#').count(), 1);
    assert!(!segments[0].id.contains(&documents[0].id));
    assert_eq!(segments[0].segment_type, 1);
    assert_eq!(segments[0].segment_byte_len, segment_byte_len);
    let compression = segments[0]
        .compression
        .as_ref()
        .expect("required invariant");
    assert_eq!(
        super::DisplayJtCompressionWire::from(compression.clone()).compressed_data_byte_len,
        compressed.len() as u32 + 1
    );
    assert_eq!(
        compression.envelope.compressed_byte_len,
        compressed.len() as u32
    );
    assert_eq!(
        compression.inflated_sha256,
        crate::native::hex::Sha256Hex::digest(&inflated)
    );

    let mut cross_entry = container.clone();
    cross_entry.entries[0].body = crate::container::DirEntryBody::File {
        offset: 0,
        len: data_len - 1,
    };
    cross_entry.entries.push(DirEntry {
        name: "/Root/other".to_string(),
        region: Region::Header,
        body: crate::container::DirEntryBody::File {
            offset: data_len - 1,
            len: 1,
        },
    });
    assert!(
        with_jt_budget(&cross_entry, |budget| super::display_jt_segments(
            budget,
            &cross_entry,
            &documents
        ))
        .unwrap()
        .is_empty()
    );

    let (compressed_elements, sequences) = with_jt_budget(&container, |budget| {
        super::display_jt_compressed_element_sequences(budget, &container, &segments)
    })
    .unwrap();
    assert_eq!(compressed_elements.len(), 1);
    assert_eq!(compressed_elements[0].segment_type, 1);
    assert_eq!(compressed_elements[0].object_type_id, [3; 16]);
    assert_eq!(compressed_elements[0].object_id, 5);
    assert_eq!(compressed_elements[0].object_base_type, 1);
    assert_eq!(compressed_elements[0].body_byte_len(), 3);
    assert_eq!(sequences.len(), 1);
    assert_eq!(sequences[0].framed_byte_len(), 48);
    assert_eq!(sequences[0].tail, [6, 5]);

    let mut malformed_compression = container.clone();
    malformed_compression.data.to_mut()[193..197]
        .copy_from_slice(&(compressed.len() as u32 + 2).to_le_bytes());
    assert!(
        with_jt_budget(&malformed_compression, |budget| super::display_jt_segments(
            budget,
            &malformed_compression,
            &documents
        ))
        .unwrap()
        .is_empty()
    );

    let mut malformed = container;
    malformed.data.to_mut()[28] = b'X';
    assert!(super::display_jt_indices(&ctx, &malformed)
        .unwrap()
        .is_empty());
}

#[test]
fn display_jt_shape_lod_requires_canonical_end_marker_and_tail() {
    use super::DisplayJtSegment;
    use crate::container::{Container, DirEntry, Region};

    let object_type_id = [0x5a; 16];
    let body = [9, 8, 7];
    let mut data = Vec::new();
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&7_u32.to_le_bytes());
    data.extend_from_slice(&78_u32.to_le_bytes());
    data.extend_from_slice(&24_u32.to_le_bytes());
    data.extend_from_slice(&object_type_id);
    data.push(4);
    data.extend_from_slice(&42_u32.to_le_bytes());
    data.extend_from_slice(&body);
    data.extend_from_slice(&16_u32.to_le_bytes());
    data.extend_from_slice(&[0xff; 16]);
    data.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    let physical_size = data.len() as u64;
    let data_len = data.len() as u64;
    let container = Container {
        data: data.into(),
        physical_size,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_string(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: data_len,
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let segment = DisplayJtSegment {
        id: "segment".to_string(),
        document: "document".to_string(),
        toc_entry: "entry".to_string(),
        segment_id: [1; 16],
        segment_type: 7,
        segment_byte_len: 78,
        payload_sha256: Sha256Hex::digest(&[]),
        compression: None,
        source_offset: 0,
    };
    let elements = with_jt_budget(&container, |budget| {
        super::display_jt_shape_lod_elements(budget, &container, std::slice::from_ref(&segment))
    })
    .unwrap();
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].object_type_id, object_type_id);
    assert_eq!(elements[0].object_id, 42);
    assert_eq!(
        serde_json::to_value(&elements[0]).unwrap()["object_base_type"],
        4
    );
    assert_eq!(elements[0].body_byte_len, 3);
    let wire = serde_json::to_value(&elements[0]).unwrap();
    for length in [15, 17] {
        let mut invalid = wire.clone();
        invalid["object_type_id"] = serde_json::json!(vec![0; length]);
        assert!(serde_json::from_value::<super::DisplayJtShapeLodElement>(invalid).is_err());
    }

    let mut malformed = container;
    *malformed
        .data
        .to_mut()
        .last_mut()
        .expect("required invariant") = 1;
    assert!(
        with_jt_budget(&malformed, |budget| super::display_jt_shape_lod_elements(
            budget,
            &malformed,
            &[segment]
        ))
        .unwrap()
        .is_empty()
    );
}

#[test]
fn display_jt_shape_lod_binding_resolves_property_table_segment_reference() {
    use super::DisplayJtSegment;
    use crate::container::{Container, DirEntry, Region};

    let mut inflated = Vec::new();
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let mut late_body = vec![1, 0];
    late_body.extend_from_slice(&0x4000_0000_u32.to_le_bytes());
    late_body.extend_from_slice(&1_u16.to_le_bytes());
    late_body.extend_from_slice(&[9; 16]);
    late_body.extend_from_slice(&7_u32.to_le_bytes());
    late_body.extend_from_slice(&12_u32.to_le_bytes());
    late_body.extend_from_slice(&1_u32.to_le_bytes());
    inflated.extend_from_slice(&57_u32.to_le_bytes());
    inflated.extend_from_slice(&[
        0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0x00, 0xaa, 0x00, 0xd1, 0x09,
        0x54,
    ]);
    inflated.push(8);
    inflated.extend_from_slice(&3_u32.to_le_bytes());
    inflated.extend_from_slice(&late_body);

    let key = "JT_LLPROP_SHAPEIMPL";
    let mut string_body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    string_body.extend_from_slice(&(key.len() as u32).to_le_bytes());
    for unit in key.encode_utf16() {
        string_body.extend_from_slice(&unit.to_le_bytes());
    }
    inflated.extend_from_slice(&(21_u32 + string_body.len() as u32).to_le_bytes());
    inflated.extend_from_slice(&[
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ]);
    inflated.push(5);
    inflated.extend_from_slice(&4_u32.to_le_bytes());
    inflated.extend_from_slice(&string_body);
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    inflated.extend_from_slice(&1_u16.to_le_bytes());
    inflated.extend_from_slice(&1_u32.to_le_bytes());
    inflated.extend_from_slice(&2_u32.to_le_bytes());
    inflated.extend_from_slice(&4_u32.to_le_bytes());
    inflated.extend_from_slice(&3_u32.to_le_bytes());
    inflated.extend_from_slice(&0_u32.to_le_bytes());

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&inflated).expect("required invariant");
    let compressed = encoder.finish().expect("required invariant");
    let mut data = vec![0; 33];
    data.extend_from_slice(&compressed);
    let physical_size = data.len() as u64;
    let data_len = data.len() as u64;
    let container = Container {
        data: data.into(),
        physical_size,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_string(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: data_len,
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let scene = DisplayJtSegment {
        id: "scene".into(),
        document: "document".into(),
        toc_entry: "scene-entry".into(),
        segment_id: [1; 16],
        segment_type: 1,
        segment_byte_len: (33 + compressed.len()) as u32,
        payload_sha256: Sha256Hex::digest(&[]),
        compression: None,
        source_offset: 0,
    };
    let shape = DisplayJtSegment {
        id: "shape".into(),
        document: "document".into(),
        toc_entry: "shape-entry".into(),
        segment_id: [9; 16],
        segment_type: 7,
        segment_byte_len: 0,
        payload_sha256: Sha256Hex::digest(&[]),
        compression: None,
        source_offset: 0,
    };
    let bindings = with_jt_budget(&container, |budget| {
        super::display_jt_shape_lod_bindings(budget, &container, &[scene, shape])
    })
    .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].shape_node_object_id, 2);
    assert_eq!(bindings[0].shape_segment, "shape");
    assert_eq!(bindings[0].payload_object_id, 12);
    assert_eq!(bindings[0].key, key);
}

#[test]
fn display_jt_string_property_body_requires_exact_utf16_frame() {
    let mut body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    body.extend_from_slice(&3_u32.to_le_bytes());
    body.extend_from_slice(&[b'N', 0, b'X', 0, 0xa9, 0x03]);
    let value = with_jt_context(|ctx| super::parse_jt_string_property_atom_body(ctx, &body))
        .unwrap()
        .expect("required invariant");
    assert_eq!(
        value.encode_utf16().collect::<Vec<_>>(),
        [0x4e, 0x58, 0x3a9]
    );
    assert_eq!(value, "NXΩ");

    body.push(0);
    assert!(
        with_jt_context(|ctx| super::parse_jt_string_property_atom_body(ctx, &body))
            .unwrap()
            .is_none()
    );
}

fn assert_jt_string_resource_limit(
    policy: cadmpeg_core::decode::DecodePolicy,
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    body.extend_from_slice(&3_u32.to_le_bytes());
    body.extend_from_slice(&[b'N', 0, b'X', 0, 0xa9, 0x03]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::parse_jt_string_property_atom_body(&ctx, &body).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation)
    );

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::parse_jt_string_property_atom_body(&service, &body)
            .unwrap()
            .as_deref(),
        Some("NXΩ")
    );
}

#[test]
fn jt_string_code_units_refuse_before_collection_growth() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    assert_jt_string_resource_limit(
        policy,
        ResourceDimension::CollectionItems,
        "decode DisplayJT string code units",
    );
}

#[test]
fn jt_string_code_units_refuse_before_scoped_allocation() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 5;
    assert_jt_string_resource_limit(
        policy,
        ResourceDimension::MaterializedBytes,
        "decode DisplayJT string code units",
    );
}

#[test]
fn jt_string_value_refuses_before_retained_allocation() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    assert_jt_string_resource_limit(
        policy,
        ResourceDimension::RetainedBytes,
        "retain DisplayJT string property",
    );
}

#[test]
fn jt_string_scan_refuses_before_utf16_work() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    assert_jt_string_resource_limit(
        policy,
        ResourceDimension::WorkUnits,
        "decode DisplayJT string code units",
    );
}

#[test]
fn display_jt9_tri_strip_header_requires_supported_versions() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x4a_u64.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&0x1234_u32.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&[9, 8, 7]);
    let (bindings, mesh_version, records_id, compressed_version, compressed) =
        super::parse_jt9_tri_strip_lod_header(&body).expect("required invariant");
    assert_eq!(bindings, 0x4a);
    assert_eq!(mesh_version, 2);
    assert_eq!(records_id, 0x1234);
    assert_eq!(compressed_version, 2);
    assert_eq!(compressed, [9, 8, 7]);

    body[12..14].copy_from_slice(&3_u16.to_le_bytes());
    assert!(super::parse_jt9_tri_strip_lod_header(&body).is_none());
}

#[test]
fn polygon_mesh_wire_preserves_corner_pairs_and_rejects_unequal_rings() {
    let wire = serde_json::json!({
        "id": "mesh",
        "topology": "topology",
        "coordinate_header": "coordinates",
        "polygons": [[0, 1, 2], [2, 1, 0, 2]],
        "vertex_attribute_indices": [[0, null, 2], [null, null, null, null]],
        "polygon_groups": [4, -1],
        "polygon_flags": [0, 7],
        "source_offset": 80
    });
    let mesh = serde_json::from_value::<super::DisplayJtPolygonMesh>(wire.clone())
        .expect("matched polygon corner arrays");
    assert_eq!(serde_json::to_value(mesh).unwrap(), wire);
    for field in ["polygons", "vertex_attribute_indices"] {
        let mut invalid = wire.clone();
        invalid[field][0].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<super::DisplayJtPolygonMesh>(invalid).is_err());
    }
}

#[test]
fn jt_scene_binding_transfers_visible_triangles_in_document_units() {
    use super::{
        DisplayJtBaseNodeData, DisplayJtCompressedElement, DisplayJtCompressedVertexRecordsHeader,
        DisplayJtGeometricTransformAttribute, DisplayJtGroupNodeData, DisplayJtInstanceNode,
        DisplayJtPolygonMesh, DisplayJtShapeLodBinding, DisplayJtShapeLodElement,
        DisplayJtTriStripShapeNode, DisplayJtVertexColors, DisplayJtVertexCoordinateArrayHeader,
        DisplayJtVertexCoordinates, DisplayJtVertexFlags, DisplayJtVertexNormals,
        DisplayJtVertexTextureCoordinates,
    };

    let mesh = DisplayJtPolygonMesh::try_from(super::DisplayJtPolygonMeshWire {
        id: "native-mesh".into(),
        topology: "topology".into(),
        coordinate_header: "coordinate-header".into(),
        polygons: vec![vec![0, 1, 2], vec![2, 1, 0, 2]],
        vertex_attribute_indices: vec![vec![Some(0), Some(1), Some(2)], vec![None; 4]],
        polygon_groups: vec![4, -1],
        polygon_flags: vec![0, 0],
        source_offset: 80,
    })
    .unwrap();
    let wire = serde_json::to_value(&mesh).unwrap();
    assert_eq!(
        serde_json::from_value::<DisplayJtPolygonMesh>(wire.clone()).unwrap(),
        mesh
    );
    for field in [
        "polygons",
        "vertex_attribute_indices",
        "polygon_groups",
        "polygon_flags",
    ] {
        let mut invalid = wire.clone();
        invalid[field].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<DisplayJtPolygonMesh>(invalid).is_err());
    }
    let coordinates = DisplayJtVertexCoordinates {
        id: "coordinates".into(),
        header: "coordinate-header".into(),
        points_m: vec![
            finite([0.0, 0.0, 0.0]),
            finite([0.001, 0.0, 0.0]),
            finite([0.0, 0.002, 0.0]),
        ],
        coordinate_hash: 0,
        byte_len: 4,
        source_offset: 90,
    };
    let header = DisplayJtVertexCoordinateArrayHeader {
        id: "coordinate-header".into(),
        element: "shape-element".into(),
        unique_vertex_count: 3,
        component_count: 3,
        component_ranges: [super::QuantizedRange::ZERO; 3],
        component_quantization_bits: [0; 3],
        compressed_components_byte_len: 4,
        compressed_components_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 60,
    };
    let shape_element = DisplayJtShapeLodElement {
        id: "shape-element".into(),
        segment: "shape-segment".into(),
        ordinal: 0,
        object_type_id: [0; 16],
        object_id: 7,
        body_byte_len: 0,
        body_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 100,
    };
    let binding = DisplayJtShapeLodBinding {
        id: "binding".into(),
        scene_segment: "scene-segment".into(),
        table_version: 1,
        shape_node_object_id: 9,
        key_object_id: 1,
        key: "JT_LLPROP_SHAPEIMPL".into(),
        value_object_id: 2,
        state_flags: 0,
        property_version: 1,
        shape_segment: "shape-segment".into(),
        payload_object_id: 7,
        reserved_value: 1,
        source_offset: 110,
    };
    let base = DisplayJtBaseNodeData {
        id: "base".into(),
        element: "scene-element".into(),
        object_type_id: [0; 16],
        object_id: 9,
        version: 1,
        flags: 0,
        attribute_object_ids: vec![10, 13],
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 120,
    };
    let compressed: DisplayJtCompressedElement = super::DisplayJtCompressedElementWire {
        id: "scene-element".into(),
        segment: "scene-segment".into(),
        segment_type: 1,
        ordinal: 0,
        object_type_id: [0; 16],
        object_base_type: 2,
        object_id: 9,
        body_byte_len: 0,
        body_sha256: "00".repeat(32).try_into().unwrap(),
        inflated_offset: 0,
        source_offset: 120,
    }
    .try_into()
    .unwrap();
    let instance_base = DisplayJtBaseNodeData {
        id: "instance-base".into(),
        element: "instance-element".into(),
        object_type_id: [0; 16],
        object_id: 11,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 6,
        family_data_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 122,
    };
    let instance_element: DisplayJtCompressedElement = super::DisplayJtCompressedElementWire {
        id: "instance-element".into(),
        segment: "scene-segment".into(),
        segment_type: 1,
        ordinal: 1,
        object_type_id: [0; 16],
        object_base_type: 0,
        object_id: 11,
        body_byte_len: 0,
        body_sha256: "00".repeat(32).try_into().unwrap(),
        inflated_offset: 0,
        source_offset: 122,
    }
    .try_into()
    .unwrap();
    let instance = DisplayJtInstanceNode {
        id: "instance-node".into(),
        base_node: "instance-base".into(),
        object_id: 11,
        version: 1,
        child_object_id: 9,
        source_offset: 122,
    };
    let mut second_instance_base = instance_base.clone();
    second_instance_base.id = "second-instance-base".into();
    second_instance_base.element = "second-instance-element".into();
    second_instance_base.object_id = 12;
    second_instance_base.source_offset = 123;
    let mut second_instance_element = instance_element.clone();
    second_instance_element.id = "second-instance-element".into();
    second_instance_element.ordinal = 2;
    second_instance_element.object_id = 12;
    second_instance_element.source_offset = 123;
    let second_instance = DisplayJtInstanceNode {
        id: "second-instance-node".into(),
        base_node: "second-instance-base".into(),
        object_id: 12,
        version: 1,
        child_object_id: 9,
        source_offset: 123,
    };
    let group_base = DisplayJtBaseNodeData {
        id: "group-base".into(),
        element: "group-element".into(),
        object_type_id: [0; 16],
        object_id: 20,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 14,
        family_data_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 124,
    };
    let group_element: DisplayJtCompressedElement = super::DisplayJtCompressedElementWire {
        id: "group-element".into(),
        segment: "scene-segment".into(),
        segment_type: 1,
        ordinal: 3,
        object_type_id: [0; 16],
        object_base_type: 1,
        object_id: 20,
        body_byte_len: 0,
        body_sha256: "00".repeat(32).try_into().unwrap(),
        inflated_offset: 0,
        source_offset: 124,
    }
    .try_into()
    .unwrap();
    let group = DisplayJtGroupNodeData {
        id: "group-node".into(),
        base_node: "group-base".into(),
        object_id: 20,
        version: 1,
        child_object_ids: vec![11, 12],
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 124,
    };
    let mut ignored_group_base = group_base.clone();
    ignored_group_base.id = "ignored-group-base".into();
    ignored_group_base.element = "ignored-group-element".into();
    ignored_group_base.object_id = 21;
    ignored_group_base.flags = 1;
    ignored_group_base.source_offset = 125;
    let mut ignored_group_element = group_element.clone();
    ignored_group_element.id = "ignored-group-element".into();
    ignored_group_element.ordinal = 4;
    ignored_group_element.object_id = 21;
    ignored_group_element.source_offset = 125;
    let ignored_group = DisplayJtGroupNodeData {
        id: "ignored-group-node".into(),
        base_node: "ignored-group-base".into(),
        object_id: 21,
        version: 1,
        child_object_ids: vec![9],
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 125,
    };
    let transform = DisplayJtGeometricTransformAttribute {
        id: "transform".into(),
        element: "scene-element".into(),
        object_id: 10,
        state_flags: 0,
        field_inhibit_flags: 0,
        stored_values_mask: 0xffff,
        matrix: super::JtTransformMatrix::try_from([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [0.01, 0.02, 0.03, 1.0],
        ])
        .unwrap(),
        source_offset: 121,
    };
    let material_element: DisplayJtCompressedElement = super::DisplayJtCompressedElementWire {
        id: "material-element".into(),
        segment: "scene-segment".into(),
        segment_type: 1,
        ordinal: 5,
        object_type_id: [0; 16],
        object_base_type: 3,
        object_id: 13,
        body_byte_len: 0,
        body_sha256: "00".repeat(32).try_into().unwrap(),
        inflated_offset: 0,
        source_offset: 126,
    }
    .try_into()
    .unwrap();
    let material = DisplayJtMaterialAttribute {
        id: "material".into(),
        element: "material-element".into(),
        object_id: 13,
        state_flags: 0,
        field_inhibit_flags: 0,
        version: super::JtMaterialVersion::One,
        data_flags: 0x20,
        ambient: super::jt_rgba_from_wire([0.1, 0.1, 0.1, 1.0]).unwrap(),
        diffuse: super::jt_rgba_from_wire([0.2, 0.3, 0.4, 0.5]).unwrap(),
        specular: super::jt_rgba_from_wire([0.0, 0.0, 0.0, 1.0]).unwrap(),
        emission: super::jt_rgba_from_wire([0.0, 0.0, 0.0, 1.0]).unwrap(),
        shininess: super::JtShininess::new(1.0).unwrap(),
        source_offset: 126,
    };
    let material_wire = serde_json::to_value(&material).unwrap();
    assert_eq!(
        serde_json::from_value::<DisplayJtMaterialAttribute>(material_wire.clone()).unwrap(),
        material
    );
    for (version, reflectivity) in [(1, Some(0.5)), (2, None), (3, None), (2, Some(-0.5))] {
        let mut wire = material_wire.clone();
        wire["version"] = version.into();
        wire["reflectivity"] = serde_json::json!(reflectivity);
        assert!(serde_json::from_value::<DisplayJtMaterialAttribute>(wire).is_err());
    }
    let node = DisplayJtTriStripShapeNode {
        id: "shape-node".into(),
        base_node: "base".into(),
        object_id: 9,
        reserved_bounds: super::JtBounds::try_from([[0.0; 3]; 2]).unwrap(),
        untransformed_bounds: super::JtBounds::try_from([[0.0; 3]; 2]).unwrap(),
        area: super::JtArea::new(0.0).unwrap(),
        vertex_count_range: [0, 0],
        node_count_range: [0, 0],
        polygon_count_range: [0, 0],
        memory_byte_len: 0,
        compression_level: 0.0.try_into().unwrap(),
        vertex_version: super::JtVertexVersion::One,
        vertex_bindings: 2,
        vertex_quantization_bits: 0,
        normal_quantization_factor: 0,
        texture_quantization_bits: 0,
        color_quantization_bits: 0,
        source_offset: 120,
    };
    let node_wire = serde_json::to_value(&node).unwrap();
    assert_eq!(
        serde_json::from_value::<DisplayJtTriStripShapeNode>(node_wire.clone()).unwrap(),
        node
    );
    for level in [0.0, 1.0] {
        let mut wire = node_wire.clone();
        wire["compression_level"] = serde_json::json!(level);
        assert!(serde_json::from_value::<DisplayJtTriStripShapeNode>(wire).is_ok());
    }
    for level in [-0.25, 1.25] {
        let mut wire = node_wire.clone();
        wire["compression_level"] = serde_json::json!(level);
        assert!(serde_json::from_value::<DisplayJtTriStripShapeNode>(wire)
            .unwrap_err()
            .to_string()
            .contains("compression_level"));
    }
    for level in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(super::UnitBinary32::try_from(level).is_err());
    }
    for (version, bindings) in [(1, Some(4)), (2, None), (3, None)] {
        let mut wire = node_wire.clone();
        wire["vertex_version"] = version.into();
        wire["version_2_vertex_bindings"] = serde_json::json!(bindings);
        assert!(serde_json::from_value::<DisplayJtTriStripShapeNode>(wire).is_err());
    }
    let vertex_header = DisplayJtCompressedVertexRecordsHeader {
        id: "vertex-header".into(),
        element: "shape-element".into(),
        vertex_bindings: 0x15a,
        vertex_quantization_bits: 0,
        normal_quantization_factor: 0,
        texture_quantization_bits: 0,
        color_quantization_bits: 0,
        topological_vertex_count: 3,
        vertex_attribute_count: 3,
        compressed_arrays_byte_len: 0,
        compressed_arrays_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 80,
    };
    let normals = DisplayJtVertexNormals {
        id: "normals".into(),
        vertex_records_header: "vertex-header".into(),
        normals: vec![
            finite([1.0, 0.0, 0.0]),
            finite([0.0, 1.0, 0.0]),
            finite([0.0, 0.0, 1.0]),
        ],
        normal_hash: 0,
        byte_len: 4,
        source_offset: 94,
    };
    let colors = DisplayJtVertexColors {
        id: "colors".into(),
        vertex_records_header: "vertex-header".into(),
        colors: vec![
            finite([1.0, 0.0, 0.0, 1.0]),
            finite([0.0, 1.0, 0.0, 0.5]),
            finite([0.0, 0.0, 1.0, 0.25]),
        ],
        color_hash: 0,
        byte_len: 4,
        source_offset: 98,
    };
    let texture_coordinates = DisplayJtVertexTextureCoordinates {
        id: "texture".into(),
        vertex_records_header: "vertex-header".into(),
        channel: 0,
        values: vec![
            finite([0.0, 0.0]).to_vec(),
            finite([1.0, 0.0]).to_vec(),
            finite([0.0, 1.0]).to_vec(),
        ],
        texture_coordinate_hash: 0,
        byte_len: 4,
        source_offset: 102,
    };
    let vertex_flags = DisplayJtVertexFlags {
        id: "flags".into(),
        vertex_records_header: "vertex-header".into(),
        values: vec![0, 1, 0],
        byte_len: 4,
        source_offset: 106,
    };

    let tessellations = with_jt_context(|ctx| {
        super::display_jt_tessellations(
            ctx,
            &super::DisplayJtTessellationInputs {
                meshes: &[mesh],
                coordinates: &[coordinates],
                normals: &[normals],
                colors: &[colors],
                texture_coordinates: &[texture_coordinates],
                vertex_flags: &[vertex_flags],
                vertex_headers: &[vertex_header],
                coordinate_headers: &[header],
                shape_elements: &[shape_element],
                bindings: &[binding],
                shape_nodes: &[node],
                base_nodes: &[
                    base,
                    instance_base,
                    second_instance_base,
                    group_base,
                    ignored_group_base,
                ],
                group_nodes: &[group, ignored_group],
                instance_nodes: &[instance, second_instance],
                transforms: &[transform],
                materials: &[material],
                compressed_elements: &[
                    compressed,
                    instance_element,
                    second_instance_element,
                    group_element,
                    ignored_group_element,
                    material_element,
                ],
            },
        )
    })
    .expect("complete scene binding");
    assert_eq!(tessellations.len(), 2);
    assert!((tessellations[0].0.vertices()[1].x - 12.0).abs() < EPS_JT_TRANSFORMED_VERTEX);
    assert!((tessellations[0].0.vertices()[2].y - 26.0).abs() < EPS_JT_TRANSFORMED_VERTEX);
    assert_eq!(tessellations[0].0.triangles(), vec![[0, 1, 2]]);
    assert_eq!(
        tessellations[0].0.vertex_normals()[1],
        cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    );
    assert_eq!(
        tessellations[0]
            .0
            .source_object
            .as_ref()
            .expect("required invariant")
            .object_id,
        "shape-node"
    );
    assert_eq!(
        tessellations[0]
            .0
            .source_object
            .as_ref()
            .expect("required invariant")
            .instance_path,
        ["instance-node"]
    );
    assert_eq!(
        tessellations[0]
            .0
            .source_object
            .as_ref()
            .expect("required invariant")
            .color,
        Some(Color::new(0.2, 0.3, 0.4, 0.5).expect("valid color"))
    );
    assert_eq!(
        tessellations[1]
            .0
            .source_object
            .as_ref()
            .expect("required invariant")
            .instance_path,
        ["second-instance-node"]
    );
    assert_eq!(tessellations[0].0.channels().len(), 3);
    assert_eq!(tessellations[0].0.channels()[0].kind(), 0x4e58_0001);
    assert_eq!(tessellations[0].0.channels()[0].item_size(), 16);
    assert_eq!(tessellations[0].0.channels()[0].flags(), 1);
    assert_eq!(tessellations[0].0.channels()[0].count(), 3);
    assert_eq!(
        &tessellations[0].0.channels()[0].data()[16..32],
        &[
            0.0_f32.to_le_bytes(),
            1.0_f32.to_le_bytes(),
            0.0_f32.to_le_bytes(),
            0.5_f32.to_le_bytes(),
        ]
        .concat()
    );
    assert_eq!(tessellations[0].0.channels()[1].kind(), 0x4e58_0100);
    assert_eq!(tessellations[0].0.channels()[1].item_size(), 8);
    assert_eq!(tessellations[0].0.channels()[1].flags(), 0x100);
    assert_eq!(tessellations[0].0.channels()[1].count(), 3);
    assert_eq!(
        &tessellations[0].0.channels()[1].data()[16..24],
        &[0.0_f32.to_le_bytes(), 1.0_f32.to_le_bytes()].concat()
    );
    assert_eq!(tessellations[0].0.channels()[2].kind(), 0x4e58_0002);
    assert_eq!(tessellations[0].0.channels()[2].item_size(), 4);
    assert_eq!(tessellations[0].0.channels()[2].count(), 3);
    assert_eq!(
        tessellations[0].0.channels()[2].data(),
        [
            0_u32.to_le_bytes(),
            1_u32.to_le_bytes(),
            0_u32.to_le_bytes(),
        ]
        .concat()
    );
    assert_eq!(tessellations[0].1, 120);
}

#[test]
fn jt9_topology_bounds_variable_high_degree_lane_count() {
    fn representation(high_degree_lanes: usize, topological_vertices: u32) -> Vec<u8> {
        let mut bytes = vec![0; (21 + high_degree_lanes + 2) * 4];
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes.extend_from_slice(&10_u64.to_le_bytes());
        bytes.extend_from_slice(&[24, 13, 16, 8]);
        bytes.extend_from_slice(&topological_vertices.to_le_bytes());
        if topological_vertices != 0 {
            bytes.extend_from_slice(&(topological_vertices + 1).to_le_bytes());
        }
        bytes
    }

    let empty = representation(1, 0);
    assert_eq!(high_degree_lane_count(&empty, 10), Some(1));
    let populated = representation(13, 20);
    assert_eq!(high_degree_lane_count(&populated, 10), Some(13));
    let beyond_legacy_ceiling = representation(65, 20);
    assert_eq!(high_degree_lane_count(&beyond_legacy_ceiling, 10), Some(65));
    assert_eq!(high_degree_lane_count(&populated, 11), None);
}

#[test]
fn jt9_topology_lookahead_returns_packet_nesting_refusal() {
    let representation = vec![0; 21 * 4];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&representation, &arena, &policy)
            .expect("bounded JT lookahead input");
    let error = super::jt9_topology_high_degree_lane_count(&ctx, &representation, 10)
        .expect_err("the packet frame exceeds the nesting limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                && limit.operation == "frame JT integer packet"
    ));
}

#[test]
fn display_jt_base_node_body_bounds_ordered_attribute_ids() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x20_u32.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&[4, 3, 2, 1]);
    let (version, flags, attributes, family) =
        super::parse_jt_base_node_body(&body, 9).expect("required invariant");
    assert_eq!(version, 1);
    assert_eq!(flags, 0x20);
    assert_eq!(attributes, [7, 9]);
    assert_eq!(family, [4, 3, 2, 1]);

    body.truncate(17);
    assert!(super::parse_jt_base_node_body(&body, 9).is_none());

    let mut modern = vec![2];
    modern.extend_from_slice(&0x40_u32.to_le_bytes());
    modern.extend_from_slice(&1_u32.to_le_bytes());
    modern.extend_from_slice(&11_u32.to_le_bytes());
    modern.push(0xaa);
    let (version, flags, attributes, family) =
        super::parse_jt_base_node_body(&modern, 10).expect("required invariant");
    assert_eq!((version, flags), (2, 0x40));
    assert_eq!(attributes, [11]);
    assert_eq!(family, [0xaa]);
}

#[test]
fn display_jt9_instance_node_requires_one_exact_child_reference() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x20_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());

    assert_eq!(super::parse_jt9_instance_node_body(&body), Some((1, 9)));
    body.push(0);
    assert!(super::parse_jt9_instance_node_body(&body).is_none());
    body.pop();
    body[14..16].copy_from_slice(&2_u16.to_le_bytes());
    assert!(super::parse_jt9_instance_node_body(&body).is_none());
}

#[test]
fn display_jt9_group_node_bounds_ordered_children_and_family_tail() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&[4, 3, 2, 1]);

    let (version, children, family) =
        super::parse_jt9_group_node_body(&body).expect("required invariant");
    assert_eq!(version, 1);
    assert_eq!(children, [7, 9]);
    assert_eq!(family, [4, 3, 2, 1]);
    body.truncate(body.len() - 5);
    assert!(super::parse_jt9_group_node_body(&body).is_none());
}

#[test]
fn display_jt9_tri_strip_shape_node_requires_exact_shape_data() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x20_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    for value in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0_f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.extend_from_slice(&6.0_f32.to_le_bytes());
    for value in [7_i32, 8, 9, 10, 11, 12] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.extend_from_slice(&4096_u32.to_le_bytes());
    body.extend_from_slice(&0.75_f32.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&0x102_u64.to_le_bytes());
    body.extend_from_slice(&[24, 13, 16, 8]);
    body.extend_from_slice(&0x304_u64.to_le_bytes());

    let node = super::parse_jt9_tri_strip_shape_node_body(&body).expect("required invariant");
    assert_eq!(
        node.reserved_bounds.get(),
        [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]
    );
    assert_eq!(
        node.untransformed_bounds.get(),
        [[-3.0, -2.0, -1.0], [0.0, 1.0, 2.0]]
    );
    assert_eq!(node.area.get(), 6.0);
    assert_eq!(node.vertex_count_range, [7, 8]);
    assert_eq!(node.node_count_range, [9, 10]);
    assert_eq!(node.polygon_count_range, [11, 12]);
    assert_eq!(node.memory_byte_len, 4096);
    assert_eq!(f32::from(node.compression_level), 0.75);
    assert_eq!(node.vertex_version.into_wire().0, 2);
    assert_eq!(node.vertex_bindings, 0x102);
    assert_eq!(node.vertex_quantization_bits, 24);
    assert_eq!(node.normal_quantization_factor, 13);
    assert_eq!(node.texture_quantization_bits, 16);
    assert_eq!(node.color_quantization_bits, 8);
    assert_eq!(node.vertex_version.into_wire().1, Some(0x304));

    let mut malformed = body.clone();
    malformed[60..64].copy_from_slice(&(-1.0_f32).to_le_bytes());
    assert!(super::parse_jt9_tri_strip_shape_node_body(&malformed).is_none());
    let mut malformed = body.clone();
    malformed[109] = 25;
    assert!(super::parse_jt9_tri_strip_shape_node_body(&malformed).is_none());
    body.truncate(body.len() - 8);
    assert!(super::parse_jt9_tri_strip_shape_node_body(&body).is_none());
}

#[test]
fn display_jt9_geometric_transform_reconstructs_sparse_affine_matrix() {
    let mut body = 1_u16.to_le_bytes().to_vec();
    body.push(0x08);
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x000e_u16.to_le_bytes());
    for value in [1.25_f32, -2.5, 4.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let (state, inhibit, mask, matrix) =
        super::parse_jt9_geometric_transform_body(&body).expect("required invariant");
    assert_eq!(state, 0x08);
    assert_eq!(inhibit, 0);
    assert_eq!(mask, 0x000e);
    assert_eq!(matrix.get()[0], [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(matrix.get()[3], [1.25, -2.5, 4.0, 1.0]);

    body[2] = 0x10;
    assert!(super::parse_jt9_geometric_transform_body(&body).is_none());

    let mut shear = 1_u16.to_le_bytes().to_vec();
    shear.push(0);
    shear.extend_from_slice(&0_u32.to_le_bytes());
    shear.extend_from_slice(&1_u16.to_le_bytes());
    shear.extend_from_slice(&0x4800_u16.to_le_bytes());
    shear.extend_from_slice(&0.5_f32.to_le_bytes());
    shear.extend_from_slice(&0.5_f32.to_le_bytes());
    assert!(super::parse_jt9_geometric_transform_body(&shear).is_none());
}

#[test]
fn display_jt9_material_requires_complete_bounded_components() {
    let mut body = 1_u16.to_le_bytes().to_vec();
    body.push(0x02);
    body.extend_from_slice(&0x41_u32.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&0x3990_u16.to_le_bytes());
    for color in [
        [0.1_f32, 0.2, 0.3, 1.0],
        [0.4_f32, 0.5, 0.6, 0.75],
        [0.7_f32, 0.8, 0.9, 1.0],
        [0.0_f32, 0.1, 0.2, 1.0],
    ] {
        for component in color {
            body.extend_from_slice(&component.to_le_bytes());
        }
    }
    body.extend_from_slice(&64.0_f32.to_le_bytes());
    body.extend_from_slice(&0.25_f32.to_le_bytes());
    let (state, inhibit, version, flags, colors, shininess) =
        super::parse_jt9_material_body(&body).expect("required invariant");
    let (version, reflectivity) = version.into_wire();
    assert_eq!(state, 0x02);
    assert_eq!(inhibit, 0x41);
    assert_eq!(version, 2);
    assert_eq!(flags, 0x3990);
    assert_eq!(colors[1].map(UnitBinary32::get), [0.4, 0.5, 0.6, 0.75]);
    assert_eq!(shininess.get(), 64.0);
    assert_eq!(reflectivity, Some(0.25));

    let mut invalid = body.clone();
    invalid[27..31].copy_from_slice(&1.1_f32.to_le_bytes());
    assert!(super::parse_jt9_material_body(&invalid).is_none());
    let mut invalid = body.clone();
    invalid[75..79].copy_from_slice(&0.0_f32.to_le_bytes());
    assert!(super::parse_jt9_material_body(&invalid).is_none());
    let mut invalid = body.clone();
    invalid[9..11].copy_from_slice(&0x0001_u16.to_le_bytes());
    assert!(super::parse_jt9_material_body(&invalid).is_none());
    body.pop();
    assert!(super::parse_jt9_material_body(&body).is_none());
}

#[test]
fn display_jt_material_accumulation_respects_inhibit_final_and_force() {
    let material = |diffuse, state_flags, field_inhibit_flags| DisplayJtMaterialAttribute {
        id: "material".into(),
        element: "element".into(),
        object_id: 1,
        state_flags,
        field_inhibit_flags,
        version: super::JtMaterialVersion::try_from((2, Some(0.0))).unwrap(),
        data_flags: 0,
        ambient: super::jt_rgba_from_wire([0.0, 0.0, 0.0, 1.0]).unwrap(),
        diffuse: super::jt_rgba_from_wire(diffuse).unwrap(),
        specular: super::jt_rgba_from_wire([0.0, 0.0, 0.0, 1.0]).unwrap(),
        emission: super::jt_rgba_from_wire([0.0, 0.0, 0.0, 1.0]).unwrap(),
        shininess: super::JtShininess::new(1.0).unwrap(),
        source_offset: 0,
    };
    let mut path = super::DisplayJtPath {
        matrix: [[0.0; 4]; 4],
        final_transform: false,
        diffuse: [None; 4],
        override_vertex_colors: None,
        final_material: false,
        node_path: Vec::new(),
        instance_path: Vec::new(),
    };
    super::accumulate_display_jt_material(&mut path, &material([0.1, 0.2, 0.3, 0.4], 0x01, 1 << 8));
    assert_eq!(
        path.diffuse.map(|value| value.map(UnitBinary32::get)),
        [Some(0.1), Some(0.2), Some(0.3), None]
    );
    assert!(path.final_material);

    super::accumulate_display_jt_material(&mut path, &material([0.5, 0.6, 0.7, 0.8], 0, 0));
    assert_eq!(
        path.diffuse.map(|value| value.map(UnitBinary32::get)),
        [Some(0.1), Some(0.2), Some(0.3), None]
    );

    super::accumulate_display_jt_material(&mut path, &material([0.5, 0.6, 0.7, 0.8], 0x02, 1 << 7));
    assert_eq!(
        path.diffuse.map(|value| value.map(UnitBinary32::get)),
        [Some(0.1), Some(0.2), Some(0.3), Some(0.8)]
    );
    assert_eq!(
        super::display_jt_path_color(&path),
        Some(Color::new(0.1, 0.2, 0.3, 0.8).expect("valid color"))
    );

    super::accumulate_display_jt_material(&mut path, &material([1.0; 4], 0x06, 0));
    assert_eq!(
        path.diffuse.map(|value| value.map(UnitBinary32::get)),
        [Some(0.1), Some(0.2), Some(0.3), Some(0.8)]
    );
}

#[test]
fn display_jt9_partition_node_requires_complete_bounds_and_ranges() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&u16::from(b'x').to_le_bytes());
    for value in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.extend_from_slice(&6.0_f32.to_le_bytes());
    for value in [1_i32, 2, 3, 4, 5, 6] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0_f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let node = super::parse_jt9_partition_node_body(&body).expect("required invariant");
    assert_eq!(node.group_version, 1);
    assert_eq!(node.child_object_ids, [2]);
    assert_eq!(node.file_name, "x");
    assert_eq!(
        node.transformed_bounds.get(),
        [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]
    );
    assert_eq!(node.area.get(), 6.0);
    assert_eq!(node.vertex_count_range, [1, 2]);
    assert_eq!(node.node_count_range, [3, 4]);
    assert_eq!(node.polygon_count_range, [5, 6]);
    assert_eq!(
        node.bounds,
        DisplayJtPartitionBounds::Untransformed(
            super::JtBounds::try_from([[-3.0, -2.0, -1.0], [0.0, 1.0, 2.0]]).unwrap()
        )
    );

    body.pop();
    assert!(super::parse_jt9_partition_node_body(&body).is_none());
}

#[test]
fn display_jt9_range_lod_requires_ordered_finite_limits() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&0.25_f32.to_le_bytes());
    body.extend_from_slice(&(-2_i32).to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&10.0_f32.to_le_bytes());
    body.extend_from_slice(&20.0_f32.to_le_bytes());
    for value in [1.0_f32, 2.0, 3.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let node = super::parse_jt9_range_lod_node_body(&body).expect("required invariant");
    assert_eq!(node.group_version, 1);
    assert_eq!(node.child_object_ids, [7, 9]);
    assert_eq!(node.lod_version, 1);
    assert_eq!(
        node.reserved_values
            .iter()
            .copied()
            .map(super::FiniteBinary32::get)
            .collect::<Vec<_>>(),
        [0.25]
    );
    assert_eq!(node.reserved_value, -2);
    assert_eq!(node.range_version, 1);
    assert_eq!(
        node.range_limits
            .0
            .iter()
            .copied()
            .map(super::FiniteBinary32::get)
            .collect::<Vec<_>>(),
        [10.0, 20.0]
    );
    assert_eq!(node.center.map(super::FiniteBinary32::get), [1.0, 2.0, 3.0]);

    let range_offset = body.len() - 20;
    body[range_offset..range_offset + 4].copy_from_slice(&5.0_f32.to_le_bytes());
    body[range_offset + 4..range_offset + 8].copy_from_slice(&4.0_f32.to_le_bytes());
    assert!(super::parse_jt9_range_lod_node_body(&body).is_none());
}

#[test]
fn jt9_topology_packets_retain_decoded_primal_values() {
    use super::{display_jt_topology_packet_sequences, DisplayJtShapeLodElement};

    let mut representation = vec![0; 24 * 4];
    representation.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    representation.extend_from_slice(&10_u64.to_le_bytes());
    representation.extend_from_slice(&[24, 13, 16, 8]);
    representation.extend_from_slice(&0_u32.to_le_bytes());

    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&10_u64.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&representation);
    let source_offset = 64_u64;
    let mut data = vec![0; source_offset as usize + 25];
    data.extend_from_slice(&body);
    let physical_size = data.len() as u64;
    let data_len = data.len() as u64;
    let container = crate::container::Container {
        data: data.into(),
        physical_size,
        layout: crate::container::test_modern_layout(1),
        entries: vec![crate::container::DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_string(),
            region: crate::container::Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: data_len,
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let elements = [DisplayJtShapeLodElement {
        id: "shape-lod".into(),
        segment: "segment".into(),
        ordinal: 0,
        object_type_id: [
            0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb,
            0x59, 0x97,
        ],
        object_id: 1,
        body_byte_len: body.len() as u32,
        body_sha256: Sha256Hex::digest(&[]),
        source_offset,
    }];

    let sequences = with_jt_context(|ctx| {
        display_jt_topology_packet_sequences(ctx, &container, &elements).expect("service JT budget")
    })
    .sequences;
    assert_eq!(sequences.len(), 1);
    assert_eq!(sequences[0].packets.len(), 24);
    assert!(sequences[0]
        .packets
        .iter()
        .all(|packet| packet.values == Some(Vec::new())));
}

mod scene_admission;
