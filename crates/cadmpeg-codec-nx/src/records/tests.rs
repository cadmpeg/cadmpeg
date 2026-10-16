// SPDX-License-Identifier: Apache-2.0

use super::{positive, read, DataType, SchemaField};
use crate::test_support::with_decode_context;
use cadmpeg_core::decode::View;

fn descriptor(bytes: &mut Vec<u8>, op: u8, name: &str, class: u16, scalar: u8) {
    bytes.push(op);
    bytes.push(u8::try_from(name.len()).unwrap());
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(&class.to_be_bytes());
    bytes.extend_from_slice(&[0, 1]); // Positive integer zero: scalar.
    if class == 0 {
        bytes.extend_from_slice(&[1, scalar]);
    }
}

fn world(mesh: bool, lattice: bool) -> Vec<u8> {
    assert!(!lattice || mesh);
    let mut bytes = vec![
        0,
        101,
        if lattice {
            19
        } else if mesh {
            18
        } else {
            15
        },
    ];
    bytes.extend_from_slice(b"CCCCCCC");
    if mesh {
        descriptor(&mut bytes, b'I', "mesh", 1006, 0);
        descriptor(&mut bytes, b'I', "polyline", 1008, 0);
    }
    if lattice {
        descriptor(&mut bytes, b'I', "lattice", 222, 0);
    }
    bytes.extend_from_slice(b"CC");
    descriptor(&mut bytes, b'I', "attdef_list", 74, 0);
    bytes.extend_from_slice(b"CC");
    descriptor(&mut bytes, b'A', "index_map_offset", 0, b'd');
    descriptor(&mut bytes, b'A', "index_map", 82, 0);
    descriptor(&mut bytes, b'A', "schema_embedding_map", 82, 0);
    if mesh {
        descriptor(&mut bytes, b'A', "mesh_offset_data", 206, 0);
    }
    bytes.push(b'Z');
    bytes
}

#[test]
fn world_variants_come_from_edit_fields_and_end_before_the_instance() {
    with_decode_context(|ctx| {
        for (mesh, lattice, count) in [(false, false, 15), (true, false, 18), (true, true, 19)] {
            let mut bytes = world(mesh, lattice);
            let end = bytes.len();
            bytes.extend_from_slice(&[0, 2, 0, 1, 1]);
            let (schema, actual_end) = read(ctx, &bytes, 0).unwrap().unwrap();
            assert_eq!(actual_end, end);
            assert_eq!(schema.plan.kind, 101);
            assert_eq!(schema.plan.fields.len(), count);
            let mut names = vec![
                "assembly",
                "attribute",
                "body",
                "transform",
                "surface",
                "curve",
                "point",
            ];
            if mesh {
                names.extend(["mesh", "polyline"]);
            }
            if lattice {
                names.push("lattice");
            }
            names.extend([
                "alive",
                "attrib_def",
                "attdef_list",
                "highest_id",
                "current_id",
                "index_map_offset",
                "index_map",
                "schema_embedding_map",
            ]);
            if mesh {
                names.push("mesh_offset_data");
            }
            assert_eq!(
                schema
                    .plan
                    .fields
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>(),
                names
            );
            for len in 0..end {
                assert!(read(ctx, &bytes[..len], 0).unwrap().is_none());
            }
            for changed in [count - 1, count + 1] {
                let mut invalid = bytes.clone();
                invalid[2] = u8::try_from(changed).unwrap();
                assert!(read(ctx, &invalid, 0).unwrap().is_none());
            }
            let mut invalid = bytes.clone();
            invalid[end - 1] = b'X';
            assert!(read(ctx, &invalid, 0).unwrap().is_none());
        }
    });
}

#[test]
fn region_insert_and_append_preserve_class_constraints() {
    with_decode_context(|ctx| {
        for (frame, owner_class) in [(false, 12), (true, 65_000)] {
            let mut bytes = vec![0, 19, if frame { 9 } else { 8 }];
            bytes.extend_from_slice(b"CCCCCC");
            if frame {
                descriptor(&mut bytes, b'I', "frame", 230, 0);
            }
            bytes.push(b'C');
            descriptor(&mut bytes, b'A', "owner", owner_class, 0);
            bytes.push(b'Z');
            let (schema, end) = read(ctx, &bytes, 0).unwrap().unwrap();
            assert_eq!(end, bytes.len());
            assert_eq!(
                schema.plan.fields.last().unwrap().pointer_class,
                owner_class
            );
            assert_eq!(
                schema
                    .plan
                    .fields
                    .iter()
                    .any(|f| f.name.as_str() == "frame"),
                frame
            );
            let wire = serde_json::to_value(&schema).unwrap();
            assert_eq!(
                wire["fields"][schema.plan.fields.len() - 1]["pointer_class"],
                owner_class
            );
        }
    });
}

#[test]
fn first_use_match_is_distinct_from_a_missing_edit_or_a_zero_field_candidate() {
    with_decode_context(|ctx| {
        let (schema, end) = read(ctx, &[0, 101, 255, 0, 2], 0).unwrap().unwrap();
        assert_eq!(end, 3);
        assert_eq!(schema.plan.fields.len(), 11);
        for bytes in [
            &[0, 101, 0, b'Z'][..],
            &[0, 101, 1, b'A', b'Z'],
            &[0, 250, 255],
        ] {
            assert!(read(ctx, bytes, 0).unwrap().is_none());
        }
    });
}

#[test]
fn descriptor_positive_integer_and_wire_invariants_are_checked() {
    for (bytes, expected) in [
        (&[0, 1][..], 0),
        (&[127, 255], 32766),
        (&[255, 255, 0, 1], 32767),
    ] {
        assert_eq!(positive(&mut View::over_retained(bytes)), Some(expected));
    }
    for bytes in [&[0, 0][..], &[255, 255, 0, 0]] {
        assert_eq!(positive(&mut View::over_retained(bytes)), None);
    }
    let field = serde_json::json!({"name":"child", "data_type":"p", "pointer_class":12, "n_elements":0, "transmit":true});
    let valid: SchemaField = serde_json::from_value(field.clone()).unwrap();
    assert_eq!(valid.data_type, DataType::Pointer);
    for (name, value) in [
        ("name", serde_json::json!("")),
        ("data_type", serde_json::json!("d")),
        ("transmit", serde_json::json!(false)),
    ] {
        let mut invalid = field.clone();
        invalid[name] = value;
        assert!(serde_json::from_value::<SchemaField>(invalid).is_err());
    }
}

#[test]
fn unestablished_primary_scalar_codes_cannot_enter_status_metadata() {
    with_decode_context(|ctx| {
        for scalar in *b"qt" {
            let mut bytes = vec![0, 41, 1, b'D', b'D'];
            descriptor(&mut bytes, b'A', "opaque", 0, scalar);
            bytes.push(b'Z');
            assert!(read(ctx, &bytes, 0).unwrap().is_none());
        }
    });
}
