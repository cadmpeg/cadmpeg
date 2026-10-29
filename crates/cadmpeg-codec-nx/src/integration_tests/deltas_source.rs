// SPDX-License-Identifier: Apache-2.0

use crate::test_support::{owned_deltas, test_pmarks, test_prt, with_decode_context};
use crate::NxCodec;
use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::{CadIr, DecodeSidecar, RetainedBytes, RetainedSourceRecord, SourceFidelity};
use serde_json::{json, Value};

const STREAM: &str = "nx:container:parasolid#0";
const SCHEMAS: &str = "parasolid_deltas_schemas";
const INSTANCES: &str = "parasolid_deltas_instances";

fn stream(payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"PS".to_vec();
    for text in [
        b": TRANSMIT FILE (deltas)".as_slice(),
        b"SCH_3501171_35102_13006",
    ] {
        bytes.extend(u32::try_from(text.len()).unwrap().to_be_bytes());
        bytes.extend(text);
    }
    bytes.extend(500u16.to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    bytes.extend(&test_pmarks::prefix(1)[test_pmarks::header().len()..]);
    bytes.extend(payload);
    test_pmarks::complete(bytes)
}

fn body_stream() -> Vec<u8> {
    let (mut payload, ordinary, _) = owned_deltas::body(36);
    payload.extend(&ordinary[2..]);
    payload.extend(ordinary);
    stream(&payload)
}

fn custom_stream() -> Vec<u8> {
    stream(b"\0\xf0\x01\x07MY_KIND\x06custom\x05value\0\0\0\x01\x01d\0\x02\0\0\0\x07")
}

fn decoded(
    streams: &[Vec<u8>],
) -> (
    CadIr,
    cadmpeg_ir::report::decode::DecodeReport,
    SourceFidelity,
) {
    let refs = streams.iter().map(Vec::as_slice).collect::<Vec<_>>();
    NxCodec
        .decode(
            &mut std::io::Cursor::new(test_prt::prt_with_streams(&refs)),
            &DecodeOptions::default(),
        )
        .expect("Deltas source fixture decodes")
        .into_parts()
}

fn findings(ir: &CadIr, source: Option<&SourceFidelity>) -> Vec<Finding> {
    with_decode_context(|ctx| <NxCodec as CodecBackend>::validate_native(ctx, ir, source)).unwrap()
}

fn edit(ir: &mut CadIr, arena: &str, change: impl FnOnce(&mut Vec<Value>)) {
    let namespace = ir.native.namespace_mut("nx");
    let mut records = namespace.arena_as::<Value>(arena).unwrap();
    change(&mut records);
    with_decode_context(|ctx| namespace.set_arena(ctx, arena, &records)).unwrap();
}

fn source_error(ir: &CadIr, fidelity: &SourceFidelity) {
    let result = findings(ir, Some(fidelity));
    assert_eq!(result.len(), 1, "{result:?}");
    assert_eq!(result[0].check, Check::NativeLinks);
    assert_eq!(result[0].severity, Severity::Error);
    assert!(result[0].message.contains("retained source"), "{result:?}");
}

fn structurally_valid(ir: &CadIr) {
    let result = findings(ir, None);
    assert_eq!(result.len(), 1, "{result:?}");
    assert_eq!(result[0].severity, Severity::Info);
    assert_eq!(result[0].entity.as_deref(), Some(STREAM));
    assert!(result[0].message.contains("structural admission only"));
}

fn replace_bytes(fidelity: &mut SourceFidelity, bytes: RetainedBytes) {
    let old = fidelity.remove_retained_record(STREAM).unwrap();
    let replacement =
        RetainedSourceRecord::from_bytes(old.stream().to_owned(), old.offset(), bytes).unwrap();
    fidelity
        .insert_retained_record(
            cadmpeg_ir::ids::UnknownId::mint(STREAM).unwrap(),
            replacement,
        )
        .unwrap();
}

#[test]
fn deltas_round_trip_compares_inflated_source_at_nonzero_container_offset() {
    let bytes = body_stream();
    let (ir, report, fidelity) = decoded(std::slice::from_ref(&bytes));
    let retained = fidelity.retained_record(STREAM).unwrap();
    assert!(retained.offset() > 0);
    assert_eq!(retained.data(), Some(bytes.as_slice()));
    assert_eq!(
        ir.native.namespace("nx").unwrap().arenas()[INSTANCES].len(),
        2
    );
    let json = ir.to_canonical_json().unwrap();
    let sidecar =
        DecodeSidecar::bind_sha256(Sha256Digest::digest(json.as_bytes()), report, fidelity);
    let sidecar = DecodeSidecar::from_json(&sidecar.to_canonical_json().unwrap()).unwrap();
    assert!(sidecar.matches(json.as_bytes()));
    let reopened = CadIr::from_json(&json).unwrap();
    assert!(findings(&reopened, Some(&sidecar.fidelity)).is_empty());
}

#[test]
fn canonical_identity_order_does_not_replace_physical_declaration_and_instance_order() {
    let (mut payload, ordinary, _) = owned_deltas::body(36);
    payload.extend(&ordinary[2..]);
    for _ in 0..10 {
        payload.extend(&ordinary);
    }
    let (ir, _, fidelity) = decoded(&[stream(&payload)]);
    let reopened = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    let namespace = reopened.native.namespace("nx").unwrap();
    for arena in [SCHEMAS, INSTANCES] {
        let offsets = namespace.arenas()[arena]
            .iter()
            .map(|record| {
                record.fields().unwrap()["inflated_offset"]
                    .as_u64()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(
            offsets.windows(2).any(|pair| pair[0] > pair[1]),
            "fixture must cross an identity-width boundary in {arena}: {offsets:?}"
        );
    }
    structurally_valid(&reopened);
    assert!(findings(&reopened, Some(&fidelity)).is_empty());
}

#[test]
fn edited_values_and_declaration_or_instance_digests_fail_source_comparison() {
    let (original, _, fidelity) = decoded(&[body_stream()]);
    for arena in [SCHEMAS, INSTANCES] {
        let mut ir = original.clone();
        edit(&mut ir, arena, |records| {
            records[0]["sha256"] = json!("0".repeat(64));
        });
        structurally_valid(&ir);
        source_error(&ir, &fidelity);
    }
    let mut ir = original;
    edit(&mut ir, INSTANCES, |records| {
        records[0]["fields"][0]["values"][0]["value"] = json!(0);
    });
    structurally_valid(&ir);
    source_error(&ir, &fidelity);
}

#[test]
fn coherent_schema_edits_preserving_declaration_digest_fail_source_comparison() {
    let (original, _, fidelity) = decoded(&[body_stream()]);
    for field in ["name", "n_elements", "transmit"] {
        let mut ir = original.clone();
        edit(&mut ir, SCHEMAS, |records| {
            let prefix = records
                .iter_mut()
                .find(|record| record["definition"]["kind"] == 3)
                .unwrap();
            let field_definition = &mut prefix["definition"]["fields"][0];
            match field {
                "name" => field_definition["name"] = json!("edited_name"),
                "n_elements" => field_definition["n_elements"] = json!(2),
                "transmit" => {
                    field_definition["n_elements"] = json!(1);
                    field_definition["transmit"] = json!(false);
                }
                _ => unreachable!(),
            }
        });
        structurally_valid(&ir);
        source_error(&ir, &fidelity);
    }
    let mut ir = original;
    let mut byte_field = 0;
    edit(&mut ir, SCHEMAS, |records| {
        let schema = records
            .iter_mut()
            .find(|record| record["definition"]["kind"] == 12)
            .unwrap();
        let fields = schema["definition"]["fields"].as_array_mut().unwrap();
        byte_field = fields
            .iter()
            .position(|field| field["data_type"] == "u")
            .unwrap();
        fields[byte_field]["data_type"] = json!("c");
    });
    edit(&mut ir, INSTANCES, |records| {
        for record in records {
            record["fields"][byte_field]["values"][0]["type"] = json!("char");
        }
    });
    structurally_valid(&ir);
    source_error(&ir, &fidelity);
}

#[test]
fn custom_schema_name_description_and_extra_fields_are_authenticated() {
    let (original, _, fidelity) = decoded(&[custom_stream()]);
    assert_eq!(
        original.native.namespace("nx").unwrap().arenas()[INSTANCES].len(),
        1
    );
    for key in ["custom_name", "custom_description", "extra"] {
        let mut ir = original.clone();
        edit(&mut ir, SCHEMAS, |records| {
            let schema = records
                .iter_mut()
                .find(|record| record["definition"]["kind"] == 240)
                .unwrap();
            assert_eq!(schema["custom_name"], "MY_KIND");
            assert_eq!(schema["custom_description"], "custom");
            schema[key] = json!("edited");
        });
        structurally_valid(&ir);
        source_error(&ir, &fidelity);
    }
}

#[test]
fn deleting_an_ordinary_instance_or_standalone_prefix_schema_is_detected() {
    let (original, _, fidelity) = decoded(&[body_stream()]);
    for arena in [INSTANCES, SCHEMAS] {
        let mut ir = original.clone();
        edit(&mut ir, arena, |records| {
            let index = if arena == INSTANCES {
                1
            } else {
                records
                    .iter()
                    .position(|record| record["definition"]["kind"] == 3)
                    .unwrap()
            };
            records.remove(index);
        });
        structurally_valid(&ir);
        source_error(&ir, &fidelity);
    }
}

#[test]
fn deleting_every_record_for_one_stream_is_detected_from_retained_streams() {
    let (mut ir, _, fidelity) = decoded(&[body_stream(), custom_stream()]);
    let namespace = ir.native.namespace_mut("nx");
    for (arena, records) in namespace.arenas_mut() {
        if arena.starts_with("parasolid_deltas_") {
            records.retain(|record| record.fields().unwrap()["stream_ordinal"] != 1);
        }
    }
    source_error(&ir, &fidelity);
    assert!(findings(&ir, Some(&fidelity))[0]
        .message
        .contains("parasolid#1"));
}

#[test]
fn source_absence_digest_only_and_missing_record_have_distinct_results() {
    let (ir, _, mut fidelity) = decoded(&[body_stream()]);
    structurally_valid(&ir);
    let retained = fidelity.retained_record(STREAM).unwrap();
    let digest = RetainedBytes::Digest {
        byte_len: retained.byte_len(),
        sha256: Sha256Digest::digest(retained.data().unwrap()),
    };
    replace_bytes(&mut fidelity, digest);
    let result = findings(&ir, Some(&fidelity));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].severity, Severity::Info);
    assert_eq!(result[0].entity.as_deref(), Some(STREAM));
    assert!(result[0].message.contains("only a digest"));
    fidelity.remove_retained_record(STREAM).unwrap();
    let result = findings(&ir, Some(&fidelity));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].severity, Severity::Error);
    assert!(result[0]
        .message
        .contains("missing retained stream nx:container:parasolid#0"));
}

#[test]
fn unavailable_source_is_reported_once_for_each_claimed_stream() {
    let (ir, _, mut fidelity) = decoded(&[body_stream(), custom_stream()]);
    let result = findings(&ir, None);
    assert_eq!(result.len(), 2, "{result:?}");
    for (ordinal, finding) in result.iter().enumerate() {
        assert_eq!(finding.severity, Severity::Info);
        assert_eq!(
            finding.entity.as_deref(),
            Some(format!("nx:container:parasolid#{ordinal}").as_str())
        );
    }
    let retained = fidelity.retained_record(STREAM).unwrap();
    let digest = RetainedBytes::Digest {
        byte_len: retained.byte_len(),
        sha256: Sha256Digest::digest(retained.data().unwrap()),
    };
    replace_bytes(&mut fidelity, digest);
    let result = findings(&ir, Some(&fidelity));
    assert_eq!(result.len(), 1, "{result:?}");
    assert_eq!(result[0].severity, Severity::Info);
    assert_eq!(result[0].entity.as_deref(), Some(STREAM));
    fidelity
        .remove_retained_record("nx:container:parasolid#1")
        .unwrap();
    let result = findings(&ir, Some(&fidelity));
    assert_eq!(result.len(), 1, "{result:?}");
    assert_eq!(result[0].severity, Severity::Error);
    assert!(result[0]
        .message
        .contains("missing retained stream nx:container:parasolid#1"));
}

#[test]
fn an_empty_deltas_claim_has_no_source_finding() {
    let (mut ir, _, fidelity) = decoded(&[body_stream()]);
    ir.native
        .namespace_mut("nx")
        .arenas_mut()
        .retain(|arena, _| !arena.starts_with("parasolid_deltas_"));
    assert!(findings(&ir, None).is_empty());
    assert!(findings(&ir, Some(&fidelity)).is_empty());
}

#[test]
fn changed_retained_bytes_and_complete_to_stopped_edits_fail_comparison() {
    let bytes = body_stream();
    let (ir, _, fidelity) = decoded(std::slice::from_ref(&bytes));
    let mut changed = bytes.clone();
    *changed.last_mut().unwrap() ^= 1;
    let mut edited_fidelity = fidelity.clone();
    replace_bytes(
        &mut edited_fidelity,
        RetainedBytes::Inline { data: changed },
    );
    source_error(&ir, &edited_fidelity);

    let mut edited_ir = ir;
    let mut stop = 0;
    edit(&mut edited_ir, INSTANCES, |records| {
        stop = usize::try_from(records[1]["inflated_offset"].as_u64().unwrap()).unwrap();
        records.remove(1);
    });
    let mut changed = bytes;
    changed[stop..stop + 2].copy_from_slice(&[0, 1]);
    let mut edited_fidelity = fidelity;
    replace_bytes(
        &mut edited_fidelity,
        RetainedBytes::Inline { data: changed },
    );
    source_error(&edited_ir, &edited_fidelity);
}

#[test]
fn stopped_owner_reports_prefix_scope_and_authenticates_the_residual_suffix() {
    let mut bytes = body_stream();
    let stop = bytes.len() - test_pmarks::TERMINATOR.len();
    bytes.extend([0, 1, 0xde, 0xad]);
    let (ir, _, mut fidelity) = decoded(&[bytes.clone()]);
    let result = findings(&ir, Some(&fidelity));
    assert_eq!(result.len(), 1, "{result:?}");
    assert_eq!(result[0].severity, Severity::Info);
    assert_eq!(result[0].entity.as_deref(), Some(STREAM));
    assert!(result[0]
        .message
        .contains("only the bounded prefix was parsed"));
    assert!(result[0]
        .message
        .contains(&format!("offset {stop} (UnsupportedPacket)")));
    *bytes.last_mut().unwrap() ^= 1;
    replace_bytes(&mut fidelity, RetainedBytes::Inline { data: bytes });
    source_error(&ir, &fidelity);
}

#[test]
fn header_prefix_and_projected_record_values_are_authenticated() {
    let payload = owned_deltas::region(7, 10, b'S');
    let (original, _, fidelity) = decoded(&[stream(&payload)]);
    for (arena, key, value) in [
        (
            "parasolid_deltas_transmit_headers",
            "description",
            json!(": TRANSMIT FILE (deltas) edited"),
        ),
        (
            "parasolid_deltas_partition_prefixes",
            "sha256",
            json!("0".repeat(64)),
        ),
        ("parasolid_deltas_records", "node_id", json!(0)),
    ] {
        let mut ir = original.clone();
        edit(&mut ir, arena, |records| {
            assert_eq!(records.len(), 1);
            records[0][key] = value;
        });
        structurally_valid(&ir);
        source_error(&ir, &fidelity);
    }
    let mut reordered = original;
    edit(&mut reordered, "parasolid_deltas_records", |records| {
        let references = records[0]["references"].as_array_mut().unwrap();
        assert_ne!(references[1], references[3]);
        references.swap(1, 3);
    });
    structurally_valid(&reordered);
    source_error(&reordered, &fidelity);
}

const METADATA: &str = "parasolid_deltas_metadata";

fn metadata_stream() -> Vec<u8> {
    let mut bytes = stream(&[]);
    bytes.truncate(bytes.len() - 14);
    bytes.extend([0, 7, 0, 19, 0, 1, 0, 0, 0, 21]);
    bytes.extend(test_pmarks::TERMINATOR);
    bytes.extend(test_pmarks::node(65_535, 1, 1, 0x0001_0000));
    bytes.extend(test_pmarks::node(9, 1, 1, 0));
    bytes.extend(owned_deltas::region(7, 3, b'S'));
    bytes.extend(test_pmarks::TERMINATOR);
    bytes
}

#[test]
fn metadata_values_digests_identity_and_complete_record_sets_are_authenticated() {
    let (original, _, fidelity) = decoded(&[metadata_stream()]);
    let reopened = CadIr::from_json(&original.to_canonical_json().unwrap()).unwrap();
    assert!(findings(&reopened, Some(&fidelity)).is_empty());
    for change in [
        "map_xmt",
        "map_kind",
        "word",
        "pmark",
        "closes",
        "section_offset",
        "digest",
        "identity",
        "span",
        "delete",
        "delete_all",
    ] {
        let mut ir = reopened.clone();
        edit(&mut ir, METADATA, |records| {
            if change == "delete_all" {
                records.clear();
                return;
            }
            if change == "delete" {
                records.remove(0);
                return;
            }
            if change == "span" {
                for record in records {
                    let offset = record["inflated_offset"].as_u64().unwrap() + 10_000;
                    record["inflated_offset"] = json!(offset);
                    record["id"] = json!(format!("nx:s0:deltas-metadata#{offset}"));
                }
                return;
            }
            let production = match change {
                "map_xmt" | "map_kind" | "word" => "map",
                "pmark" | "identity" => "pmark",
                _ => "terminator",
            };
            let record = records
                .iter_mut()
                .find(|record| record["production"] == production)
                .unwrap();
            match change {
                "map_xmt" => record["entries"][0][0] = json!(8),
                "map_kind" => record["entries"][0][1] = json!(18),
                "word" => record["terminal_word"] = json!(22),
                "pmark" => record["state"]["identifier"] = json!(42),
                "closes" => record["closes"] = json!("pmark"),
                "section_offset" => record["section_offset"] = json!(0),
                "digest" => record["sha256"] = json!("0".repeat(64)),
                "identity" => {
                    record["stream_ordinal"] = json!(1);
                    record["id"] = json!(format!(
                        "nx:s1:deltas-metadata#{}",
                        record["inflated_offset"].as_u64().unwrap()
                    ));
                }
                _ => unreachable!(),
            }
        });
        if change != "identity" {
            structurally_valid(&ir);
        }
        source_error(&ir, &fidelity);
    }
}

#[test]
fn metadata_claim_alone_enrolls_the_stream_and_all_seven_arenas() {
    let (mut ir, _, fidelity) = decoded(&[metadata_stream()]);
    ir.native
        .namespace_mut("nx")
        .arenas_mut()
        .retain(|arena, _| !arena.starts_with("parasolid_deltas_") || arena == METADATA);
    structurally_valid(&ir);
    source_error(&ir, &fidelity);
}

#[test]
fn equal_pmark_values_with_changed_xmt_spelling_fail_the_metadata_digest_comparison() {
    let mut bytes = metadata_stream();
    let (ir, _, mut fidelity) = decoded(&[bytes.clone()]);
    let metadata = ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<Value>(METADATA)
        .unwrap();
    let row = metadata
        .iter()
        .find(|record| record["production"] == "pmark" && record["state"]["xmt"] == 65_535)
        .unwrap();
    let at = usize::try_from(row["inflated_offset"].as_u64().unwrap()).unwrap() + 2;
    assert_eq!(&bytes[at..at + 4], &[255, 255, 0, 2]);
    bytes[at..at + 4].copy_from_slice(&[128, 0, 0, 1]);
    let (changed, _, _) = decoded(&[bytes.clone()]);
    let changed_rows = changed
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<Value>(METADATA)
        .unwrap();
    let changed_row = changed_rows
        .iter()
        .find(|record| record["id"] == row["id"])
        .unwrap();
    assert_eq!(changed_row["state"], row["state"]);
    assert_ne!(changed_row["sha256"], row["sha256"]);
    replace_bytes(&mut fidelity, RetainedBytes::Inline { data: bytes });
    let reopened = CadIr::from_json(&changed.to_canonical_json().unwrap()).unwrap();
    structurally_valid(&reopened);
    assert!(findings(&reopened, Some(&fidelity)).is_empty());
    source_error(&ir, &fidelity);
}

#[test]
fn workspace_nodes_extract_compare_and_reload_for_plain_and_embedded_bases() {
    for embedded in [false, true] {
        let mut bytes = if embedded {
            let mut bytes = stream(&[]);
            bytes.truncate(bytes.len() - 18);
            bytes
        } else {
            let prefix = test_pmarks::prefix(1);
            let header_end = test_pmarks::header().len();
            let pmark = header_end + 15;
            let mut bytes = b"PS".to_vec();
            for text in [b": TRANSMIT FILE (deltas)".as_slice(), b"SCH_3501171_13006"] {
                bytes.extend(u32::try_from(text.len()).unwrap().to_be_bytes());
                bytes.extend(text);
            }
            bytes.extend(0u32.to_be_bytes());
            bytes.extend([0, 3]);
            bytes.extend(&prefix[header_end + 3..pmark]);
            bytes.extend([0, 4]);
            bytes.extend(&prefix[pmark + 3..]);
            bytes
        };
        bytes.extend([0, 2]);
        if embedded {
            bytes.push(255);
        }
        let payload = [0, 1, 0, 1, 0, 4, 0, 1, 0, 0, 0, 0];
        bytes.extend(u32::try_from(payload.len()).unwrap().to_be_bytes());
        bytes.extend([0, 8]);
        bytes.extend(payload);
        bytes.extend([0, 2, 0, 0, 0, 1, 0, 9, b'W']);
        let bytes = test_pmarks::complete(bytes);
        let (ir, _, fidelity) = decoded(&[bytes]);
        let namespace = ir.native.namespace("nx").unwrap();
        let schemas = namespace.arena_as::<Value>(SCHEMAS).unwrap();
        let schema = schemas
            .iter()
            .find(|schema| schema["definition"]["kind"] == 2)
            .unwrap();
        assert_eq!(schema["definition"]["fields"][0]["name"], "ws");
        let instances = namespace.arena_as::<Value>(INSTANCES).unwrap();
        assert_eq!(instances.len(), 2);
        assert!(instances
            .iter()
            .all(|instance| instance["schema"] == schema["id"]));
        assert_eq!(
            instances[0]["fields"][0]["values"]
                .as_array()
                .unwrap()
                .len(),
            payload.len()
        );
        assert_eq!(instances[1]["fields"][0]["values"][0]["value"], b'W');
        let reopened = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
        assert!(findings(&reopened, Some(&fidelity)).is_empty());
        structurally_valid(&reopened);
        let mut deleted = reopened;
        edit(&mut deleted, INSTANCES, |records| {
            records.clear();
        });
        let result = findings(&deleted, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].severity, Severity::Error);
        assert!(result[0].message.contains("transactional first instance"));
    }
}
