// SPDX-License-Identifier: Apache-2.0
//! #224 comment6376510137 canonical flat polygon and W1 controls.
use super::*;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use serde_json::Value;

fn source(binary: bool) -> Vec<u8> {
    let xml = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='Shape' id='1'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let mut member = if binary {
        b"Open CASCADE Topology V4, (c) Open Cascade\n".to_vec()
    } else {
        b"CASCADE Topology V3, (c) Open Cascade\n".to_vec()
    };
    member.extend_from_slice(b"Locations 0\nCurve2ds 0\nCurves 0\nPolygon3D 2\n");
    for (deflection, x) in [(0.125_f64, 1.0_f64), (0.25, 4.0)] {
        if binary {
            member.extend_from_slice(&2_i32.to_le_bytes());
            member.push(1);
            for value in [deflection, x, 2.0, 3.0, x + 1.0, 5.0, 6.0, 0.0, 1.0] {
                member.extend_from_slice(&value.to_le_bytes());
            }
        } else {
            member.extend_from_slice(
                format!("2 1 {deflection} {x} 2 3 {} 5 6 0 1\n", x + 1.0).as_bytes(),
            );
        }
    }
    member.extend_from_slice(b"\nPolygonOnTriangulations 2\n");
    for deflection in [0.5_f64, 0.75] {
        if binary {
            for index in [2_i32, 1, 2] {
                member.extend_from_slice(&index.to_le_bytes());
            }
            member.extend_from_slice(&deflection.to_le_bytes());
            member.push(1);
            for value in [0.0_f64, 1.0] {
                member.extend_from_slice(&value.to_le_bytes());
            }
        } else {
            member.extend_from_slice(format!("2 1 2 p {deflection} 1 0 1\n").as_bytes());
        }
    }
    member.extend_from_slice(b"\nSurfaces 0\nTriangulations 0\nTShapes 0\n");
    if binary {
        for value in [-1_i32, -1, -1] {
            member.extend_from_slice(&value.to_le_bytes());
        }
    } else {
        member.extend_from_slice(b"*\n");
    }
    let filename = if binary { "Shape.bin" } else { "Shape.brp" };
    let xml = if binary {
        String::from_utf8(xml.to_vec())
            .unwrap()
            .replace("Shape.brp", "Shape.bin")
            .into_bytes()
    } else {
        xml.to_vec()
    };
    crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml.as_slice()),
        (filename, member.as_slice()),
    ])
}

fn decode_container(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap()
}

fn ordered(ir: &cadmpeg_ir::CadIr, arena: &str) -> Vec<Value> {
    let namespace = ir.native.namespace("fcstd").unwrap();
    let payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let payload = serde_json::to_value(&payloads[0]).unwrap();
    let carrier = if payload["form"] == "binary" {
        &payload["binary"]
    } else {
        &payload["text"]
    };
    let rows = namespace.arena_as::<Value>(arena).unwrap();
    carrier[arena]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            let mut row = rows.iter().find(|row| row["id"] == *id).unwrap().clone();
            row.as_object_mut().unwrap().remove("id");
            row
        })
        .collect()
}

fn write(ir: &cadmpeg_ir::CadIr) -> (Vec<u8>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    (bytes, losses)
}

#[test]
fn text_and_binary_two_by_two_polygon_tables_have_flat_native_homes_and_exact_source_free_roundtrips(
) {
    for binary in [false, true] {
        let before = decode_container(&source(binary));
        let namespace = before.ir().native.namespace("fcstd").unwrap();
        let payload = namespace
            .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
            .unwrap()
            .remove(0);
        let key = cadmpeg_ir::ids::Identity::new(&payload.id)
            .unwrap()
            .key()
            .to_string();
        for arena in ["polygons3d", "polygons_on_triangulations"] {
            let rows = namespace.arena_as::<Value>(arena).unwrap();
            assert_eq!(rows.len(), 2);
            for (position, row) in rows.iter().enumerate() {
                assert_eq!(
                    row["id"],
                    crate::native::native_id(arena, &format!("{key}:{}", position + 1))
                );
                assert_eq!(
                    row.as_object()
                        .unwrap()
                        .keys()
                        .map(String::as_str)
                        .collect::<std::collections::BTreeSet<_>>(),
                    std::collections::BTreeSet::from(["id", "deflection", "nodes", "parameters"])
                );
                let layout = &before.source_fidelity().record_layout()[row["id"].as_str().unwrap()];
                assert!(!layout.reads().is_empty());
            }
        }
        assert!(crate::test_support::validate_native(before.ir()).is_empty());
        let (bytes, _) = write(before.ir());
        let after = decode_container(&bytes);
        for arena in ["polygons3d", "polygons_on_triangulations"] {
            assert_eq!(
                serde_json::to_vec(&ordered(before.ir(), arena)).unwrap(),
                serde_json::to_vec(&ordered(after.ir(), arena)).unwrap(),
                "{arena} binary={binary}"
            );
        }
    }
}

#[test]
fn canonical_polygon_list_order_and_distinct_equal_rows_survive_source_free_write() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for binary in [false, true] {
        let before = decode_container(&source(binary));
        let mut ir = before.ir().clone();
        let mut payloads = ir
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<Value>("shape_payloads")
            .unwrap();
        let carrier = if binary { "binary" } else { "text" };
        for arena in ["polygons3d", "polygons_on_triangulations"] {
            payloads[0][carrier][arena]
                .as_array_mut()
                .unwrap()
                .reverse();
        }
        ir.native
            .namespace_mut("fcstd")
            .set_arena(&ctx, "shape_payloads", &payloads)
            .unwrap();
        let (bytes, _) = write(&ir);
        let after = decode_container(&bytes);
        for arena in ["polygons3d", "polygons_on_triangulations"] {
            assert_eq!(ordered(&ir, arena), ordered(after.ir(), arena));
        }
        for arena in ["polygons3d", "polygons_on_triangulations"] {
            let namespace = ir.native.namespace_mut("fcstd");
            let mut rows = namespace.arena_as::<Value>(arena).unwrap();
            let id = rows[1]["id"].clone();
            rows[1] = rows[0].clone();
            rows[1]["id"] = id;
            namespace.set_arena(&ctx, arena, &rows).unwrap();
        }
        let (bytes, _) = write(&ir);
        let after = decode_container(&bytes);
        for arena in ["polygons3d", "polygons_on_triangulations"] {
            assert_eq!(ordered(&ir, arena).len(), 2);
            assert_eq!(ordered(&ir, arena), ordered(after.ir(), arena));
        }
    }
}

#[test]
fn deleted_polygon_rows_are_not_resurrected_and_both_surviving_tables_keep_their_values() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for binary in [false, true] {
        for arena in ["polygons3d", "polygons_on_triangulations"] {
            let before = decode_container(&source(binary));
            let mut ir = before.ir().clone();
            let namespace = ir.native.namespace_mut("fcstd");
            let mut rows = namespace.arena_as::<Value>(arena).unwrap();
            let removed = rows.remove(0)["id"].as_str().unwrap().to_owned();
            namespace.set_arena(&ctx, arena, &rows).unwrap();
            let mut expected = ordered(before.ir(), arena);
            expected.remove(0);
            let (bytes, losses) = write(&ir);
            assert!(losses.iter().any(|loss| loss.message.contains(&removed)));
            let after = decode_container(&bytes);
            assert_eq!(ordered(after.ir(), arena), expected);
            let other = if arena == "polygons3d" {
                "polygons_on_triangulations"
            } else {
                "polygons3d"
            };
            assert_eq!(ordered(after.ir(), other), ordered(before.ir(), other));
        }
    }
}
