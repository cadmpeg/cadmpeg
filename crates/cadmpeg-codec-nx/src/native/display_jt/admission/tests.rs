// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::native::NativeNamespace;
use serde_json::{json, Value};

use super::{DisplayJtGraph, DisplayJtGraphWire};

fn graph_wire() -> Value {
    json!({
        "display_jt_documents": [{
            "id": "nx:display-jt:document#0", "index_row": "nx:display-jt:row#0",
            "version_field": format!("{:<80}", "Version 9.5"),
            "format_major": 9, "format_minor": 5, "byte_order": 0,
            "toc_offset": 105, "lsg_segment_id": vec![0; 16],
            "toc_entries": [
                {"id": "nx:display-jt:toc-entry#0", "ordinal": 0, "segment_id": vec![7; 16],
                 "segment_offset": 200, "segment_byte_len": 64, "attributes": [0, 0, 0, 7], "source_offset": 209},
                {"id": "nx:display-jt:toc-entry#1", "ordinal": 1, "segment_id": vec![31; 16],
                 "segment_offset": 264, "segment_byte_len": 64, "attributes": [0, 0, 0, 31], "source_offset": 237}
            ],
            "physical_byte_len": 328, "source_offset": 100
        }],
        "display_jt_segments": [
            {"id": "nx:display-jt:segment#0", "document": "nx:display-jt:document#0", "toc_entry": "nx:display-jt:toc-entry#0",
             "segment_id": vec![7; 16], "segment_type": 7, "segment_byte_len": 64, "payload_sha256": "payload", "compression": null, "source_offset": 300},
            {"id": "nx:display-jt:segment#1", "document": "nx:display-jt:document#0", "toc_entry": "nx:display-jt:toc-entry#1",
             "segment_id": vec![31; 16], "segment_type": 31, "segment_byte_len": 64, "payload_sha256": "payload",
             "compression": {"flag": 2, "compressed_data_byte_len": 32, "algorithm": 2, "compressed_byte_len": 31, "inflated_sha256": "inflated"}, "source_offset": 364}
        ],
        "display_jt_shape_lod_elements": [{
            "id": "nx:display-jt:shape-element#0", "segment": "nx:display-jt:segment#0", "ordinal": 0,
            "object_type_id": vec![0; 16], "object_base_type": 4, "object_id": 1,
            "body_byte_len": 1, "body_sha256": "body", "source_offset": 324
        }],
        "display_jt_compressed_elements": [{
            "id": "nx:display-jt:compressed-element#0", "segment": "nx:display-jt:segment#1", "segment_type": 31,
            "ordinal": 0, "object_type_id": vec![0; 16], "object_base_type": 1, "object_id": 2,
            "body_byte_len": 1, "body_sha256": "body", "inflated_offset": 0, "source_offset": 388
        }],
        "display_jt_compressed_element_sequences": [{
            "id": "nx:display-jt:sequence#0", "segment": "nx:display-jt:segment#1", "segment_type": 31,
            "elements": ["nx:display-jt:compressed-element#0"], "framed_byte_len": 46,
            "tail": [], "tail_sha256": cadmpeg_ir::hash::sha256_hex(&[]), "source_offset": 388
        }]
    })
}

#[test]
fn aggregate_admission_preserves_native_json() {
    let wire = graph_wire();
    let graph: DisplayJtGraph = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&graph).unwrap(), wire);
    let namespace: NativeNamespace = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(namespace.admit::<DisplayJtGraph>().unwrap(), graph);
    assert_eq!(serde_json::to_value(&namespace).unwrap(), wire);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native.0.insert("nx".into(), namespace);
    assert!(crate::NxCodec::validate_native(&ir).is_empty());
}

#[test]
fn aggregate_admission_rejects_missing_owners_and_repeated_field_disagreement() {
    for (path, value, field) in [
        (
            "/display_jt_segments/0/document",
            json!("nx:display-jt:document#missing"),
            "document",
        ),
        (
            "/display_jt_segments/0/toc_entry",
            json!("nx:display-jt:toc-entry#missing"),
            "toc_entry",
        ),
        (
            "/display_jt_segments/0/segment_id",
            json!(vec![9; 16]),
            "segment_id",
        ),
        (
            "/display_jt_segments/0/segment_type",
            json!(31),
            "segment_type",
        ),
        (
            "/display_jt_segments/0/segment_byte_len",
            json!(65),
            "segment_byte_len",
        ),
        (
            "/display_jt_segments/0/source_offset",
            json!(301),
            "source_offset",
        ),
        (
            "/display_jt_shape_lod_elements/0/segment",
            json!("nx:display-jt:segment#missing"),
            "segment",
        ),
        (
            "/display_jt_shape_lod_elements/0/segment",
            json!("nx:display-jt:segment#1"),
            "type-7",
        ),
        (
            "/display_jt_compressed_elements/0/segment",
            json!("nx:display-jt:segment#missing"),
            "segment",
        ),
        (
            "/display_jt_compressed_elements/0/segment",
            json!("nx:display-jt:segment#0"),
            "compression",
        ),
        (
            "/display_jt_compressed_elements/0/segment_type",
            json!(7),
            "segment_type",
        ),
        (
            "/display_jt_compressed_elements/0/source_offset",
            json!(389),
            "source_offset",
        ),
        (
            "/display_jt_compressed_element_sequences/0/segment",
            json!("nx:display-jt:segment#missing"),
            "segment",
        ),
        (
            "/display_jt_compressed_element_sequences/0/segment_type",
            json!(7),
            "segment_type",
        ),
        (
            "/display_jt_compressed_element_sequences/0/elements/0",
            json!("nx:display-jt:compressed-element#missing"),
            "elements",
        ),
        (
            "/display_jt_compressed_elements/0/ordinal",
            json!(1),
            "ordinal",
        ),
        (
            "/display_jt_compressed_elements/0/body_byte_len",
            json!(2),
            "body_byte_len",
        ),
        (
            "/display_jt_compressed_elements/0/inflated_offset",
            json!(1),
            "inflated_offset",
        ),
        (
            "/display_jt_compressed_element_sequences/0/framed_byte_len",
            json!(47),
            "framed_byte_len",
        ),
    ] {
        let mut wire = graph_wire();
        *wire.pointer_mut(path).unwrap() = value;
        let raw: DisplayJtGraphWire = serde_json::from_value(wire.clone()).unwrap();
        assert!(
            DisplayJtGraph::try_from(raw)
                .unwrap_err()
                .to_string()
                .contains(field),
            "{path}"
        );
        assert!(
            serde_json::from_value::<DisplayJtGraph>(wire.clone())
                .unwrap_err()
                .to_string()
                .contains(field),
            "{path}"
        );
        let namespace: NativeNamespace = serde_json::from_value(wire).unwrap();
        assert!(
            namespace
                .admit::<DisplayJtGraph>()
                .unwrap_err()
                .to_string()
                .contains(field),
            "{path}"
        );
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native.0.insert("nx".into(), namespace);
        let findings = crate::NxCodec::validate_native(&ir);
        assert_eq!(findings.len(), 1, "{path}");
        assert!(findings[0].message.contains(field), "{path}");
    }
}

#[test]
fn compressed_element_wire_rejects_invalid_lengths_and_tail_hash() {
    for (path, value, field) in [
        (
            "/display_jt_compressed_elements/0/body_byte_len",
            json!(u32::MAX),
            "body_byte_len",
        ),
        (
            "/display_jt_compressed_element_sequences/0/framed_byte_len",
            json!(19),
            "framed_byte_len",
        ),
        (
            "/display_jt_compressed_element_sequences/0/tail_sha256",
            json!(cadmpeg_ir::hash::sha256_hex(&[1])),
            "tail_sha256",
        ),
    ] {
        let mut wire = graph_wire();
        *wire.pointer_mut(path).unwrap() = value;
        assert!(serde_json::from_value::<DisplayJtGraph>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}
