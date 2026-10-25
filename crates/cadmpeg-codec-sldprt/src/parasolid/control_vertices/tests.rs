// SPDX-License-Identifier: Apache-2.0
use super::control_vertices;
use crate::parasolid::stream_header;
use crate::test_support::parasolid::construction_curve::stream;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::math::Point3;

fn points() -> [[f64; 3]; 3] {
    [
        [1000.0, 2000.0, 3000.0],
        [2500.0, 3500.0, 4500.0],
        [4000.0, 5000.0, 6000.0],
    ]
}
fn decode(bytes: &[u8]) -> Option<Vec<Point3>> {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("context");
    let header = stream_header(bytes)
        .expect("valid header")
        .expect("header present");
    control_vertices(&ctx, bytes, &header).expect("bounded control decode")
}
fn record(bytes: &[u8], kind: u8) -> usize {
    bytes
        .windows(3)
        .position(|b| b == [0, kind, 255])
        .expect("independent synthetic record")
}
fn append(bytes: &mut Vec<u8>, extra: &[u8]) {
    let end = bytes.len() - 4;
    bytes.splice(end..end, extra.iter().copied());
}
#[test]
fn references_select_control_vertices_and_convert_metres_to_millimetres() {
    let bytes = stream(&points());
    let result = decode(&bytes).expect("complete path");
    assert_eq!(result, points().map(|p| Point3::new(p[0], p[1], p[2])));
}
#[test]
fn missing_wrong_type_count_dimension_and_nonfinite_targets_abstain() {
    for case in 0..9 {
        let mut bytes = stream(&points());
        let n = record(&bytes, 136) + 3;
        let v = record(&bytes, 45) + 3;
        match case {
            0 => bytes[n + 19..n + 21].copy_from_slice(&5000_u16.to_be_bytes()),
            1 => bytes[v - 2] = 46,
            2 => bytes[v..v + 4].copy_from_slice(&8_u32.to_be_bytes()),
            3 => bytes[n + 8..n + 10].copy_from_slice(&4_u16.to_be_bytes()),
            4 => bytes[v + 6..v + 14].copy_from_slice(&f64::NAN.to_be_bytes()),
            5 => bytes[v + 6..v + 14].copy_from_slice(&f64::INFINITY.to_be_bytes()),
            6 => bytes[n + 17] = 1,
            7 => bytes[v + 6..v + 14].copy_from_slice(&f64::MAX.to_be_bytes()),
            8 => bytes[n + 2..n + 4].copy_from_slice(&8_u16.to_be_bytes()),
            _ => unreachable!(),
        }
        assert!(decode(&bytes).is_none(), "case {case}");
    }
}
#[test]
fn duplicate_reachable_identity_and_boundary_cycle_abstain() {
    let mut duplicate = stream(&points());
    let start = record(&duplicate, 45);
    let data_end = start + 3 + 6 + points().len() * 24;
    let mut copy = duplicate[start..data_end].to_vec();
    copy.remove(2);
    append(&mut duplicate, &copy);
    assert!(decode(&duplicate).is_none());
    let mut cycle = stream(&points());
    let c = record(&cycle, 134) + 3;
    cycle[c + 10..c + 12].copy_from_slice(&31_u16.to_be_bytes());
    assert!(decode(&cycle).is_none());
}
#[test]
fn two_complete_boundary_paths_abstain() {
    let mut bytes = stream(&points());
    let start = record(&bytes, 134);
    let mut second = bytes[start..start + 24].to_vec();
    second.remove(2);
    second[2..4].copy_from_slice(&32_u16.to_be_bytes());
    bytes[start + 3 + 10..start + 3 + 12].copy_from_slice(&32_u16.to_be_bytes());
    append(&mut bytes, &second);
    assert!(decode(&bytes).is_none());
}
#[test]
fn later_fixed_and_variable_records_need_no_schema_marker() {
    let mut bytes = stream(&points());
    let c = record(&bytes, 134);
    let n = record(&bytes, 136);
    let v = record(&bytes, 45);
    let mut later_curve = bytes[c..c + 24].to_vec();
    later_curve.remove(2);
    let mut later_nurbs = bytes[n..n + 28].to_vec();
    later_nurbs.remove(2);
    let mut later_vertices = bytes[v..v + 9 + points().len() * 24].to_vec();
    later_vertices.remove(2);
    bytes[c + 3..c + 5].copy_from_slice(&32_u16.to_be_bytes());
    bytes[n + 3..n + 5].copy_from_slice(&44_u16.to_be_bytes());
    bytes[v + 7..v + 9].copy_from_slice(&58_u16.to_be_bytes());
    append(&mut bytes, &later_curve);
    append(&mut bytes, &later_nurbs);
    append(&mut bytes, &later_vertices);
    assert_eq!(
        decode(&bytes).expect("later forms"),
        points().map(|p| Point3::new(p[0], p[1], p[2]))
    );
}
#[test]
fn reference_relocation_and_unreferenced_larger_decoys_do_not_rank_arrays() {
    let mut bytes = stream(&points());
    let n = record(&bytes, 136) + 3;
    let v = record(&bytes, 45);
    let mut relocated = bytes[v..v + 9 + points().len() * 24].to_vec();
    relocated.remove(2);
    relocated[6..8].copy_from_slice(&30000_u16.to_be_bytes());
    bytes[n + 19..n + 21].copy_from_slice(&30000_u16.to_be_bytes());
    append(&mut bytes, &relocated);
    let mut decoy = vec![0, 45];
    decoy.extend(12_u32.to_be_bytes());
    decoy.extend(34_u16.to_be_bytes());
    for _ in 0..12 {
        decoy.extend(42_f64.to_be_bytes());
    }
    append(&mut bytes, &decoy);
    assert_eq!(
        decode(&bytes).expect("referenced relocated record"),
        points().map(|p| Point3::new(p[0], p[1], p[2]))
    );
}
#[test]
fn curve_form_is_an_unsigned_native_byte() {
    for value in [0, 1, 3, 255] {
        let mut bytes = stream(&points());
        let n = record(&bytes, 136) + 3;
        bytes[n + 18] = value;
        assert!(decode(&bytes).is_some(), "curve form {value}");
    }
}
#[test]
fn nonwire_body_and_unsupported_schema_edits_abstain() {
    let mut bytes = stream(&points());
    let header = stream_header(&bytes).unwrap().unwrap();
    let body = header.body_offset + 6;
    bytes[body + 3 + 43] = 3;
    assert!(decode(&bytes).is_none());
    let mut bytes = stream(&points());
    bytes[body + 2] = 0;
    assert!(decode(&bytes).is_none());
}

fn pointer_field(schema: &mut Vec<u8>, name: &str, class: u16) {
    schema.push(u8::try_from(name.len()).unwrap());
    schema.extend(name.as_bytes());
    schema.extend(class.to_be_bytes());
    schema.extend(1_u16.to_be_bytes());
}

#[test]
fn inserted_replaced_and_appended_body_fields_keep_the_named_boundary() {
    let bytes = stream(&points());
    let body = stream_header(&bytes).unwrap().unwrap().body_offset + 6;
    let curve = record(&bytes, 134);
    let payload = &bytes[body + 3..curve];
    let mut schema = vec![26];
    schema.extend([b'C'; 7]);
    schema.push(b'I');
    pointer_field(&mut schema, "mesh", 1006);
    schema.push(b'I');
    pointer_field(&mut schema, "polyline", 1008);
    schema.extend([b'C'; 6]);
    schema.push(b'D');
    schema.push(b'I');
    pointer_field(&mut schema, "owner", 1040);
    schema.extend([b'C'; 9]);
    schema.push(b'A');
    pointer_field(&mut schema, "child", 12);
    schema.push(b'Z');
    let mut edited = bytes[..body + 2].to_vec();
    edited.extend(&schema);
    edited.extend(&payload[..18]);
    edited.extend([0, 1, 0, 1]);
    edited.extend(&payload[18..]);
    edited.extend([0, 1]);
    edited.extend(&bytes[curve..]);
    assert_eq!(
        decode(&edited).expect("named boundary after schema edits"),
        points().map(|p| Point3::new(p[0], p[1], p[2]))
    );
    edited[body + 2] = 27;
    assert!(decode(&edited).is_none());
}

#[test]
fn append_before_base_end_and_insert_after_base_end_abstain() {
    let bytes = stream(&points());
    let body = stream_header(&bytes).unwrap().unwrap().body_offset + 6;
    let curve = record(&bytes, 134);
    let payload = &bytes[body + 3..curve];
    for early_append in [true, false] {
        let mut schema = vec![24];
        if !early_append {
            schema.extend([b'C'; 23]);
        }
        schema.push(if early_append { b'A' } else { b'I' });
        pointer_field(&mut schema, "child", 12);
        if early_append {
            schema.extend([b'C'; 23]);
        }
        schema.push(b'Z');
        let mut edited = bytes[..body + 2].to_vec();
        edited.extend(&schema);
        edited.extend(&payload[..2]);
        if early_append {
            edited.extend([0, 1]);
        }
        edited.extend(&payload[2..]);
        if !early_append {
            edited.extend([0, 1]);
        }
        edited.extend(&bytes[curve..]);
        assert!(decode(&edited).is_none());
    }
}

#[test]
fn wide_pointer_encoding_requires_canonical_negative_remainder_and_positive_quotient() {
    for (bytes, expected) in [
        (&[0, 1][..], Some(1)),
        (&[0, 0][..], None),
        (&[255, 255, 0, 1][..], Some(32768)),
        (&[128, 1, 0, 1][..], Some(65534)),
        (&[128, 0, 0, 1][..], None),
        (&[255, 255, 0, 0][..], None),
        (&[255, 255, 128, 0][..], None),
    ] {
        let mut cursor = super::Cursor { bytes, at: 0 };
        assert_eq!(cursor.reference(), expected);
    }
}

#[test]
fn duplicated_selector_names_with_different_types_abstain() {
    let bytes = stream(&points());
    let body = stream_header(&bytes).unwrap().unwrap().body_offset + 6;
    let curve = record(&bytes, 134);
    let payload = &bytes[body + 3..curve];
    for wrong_body_kind in [true, false] {
        let mut schema = vec![24];
        schema.extend([b'C'; 23]);
        schema.push(b'A');
        let extra = if wrong_body_kind {
            pointer_field(&mut schema, "body_type", 12);
            vec![0, 1]
        } else {
            let name = b"boundary_curve";
            schema.push(u8::try_from(name.len()).unwrap());
            schema.extend(name);
            schema.extend([0, 0, 0, 1, 1, b'u']);
            vec![1]
        };
        schema.push(b'Z');
        let mut edited = bytes[..body + 2].to_vec();
        edited.extend(schema);
        edited.extend(payload);
        edited.extend(extra);
        edited.extend(&bytes[curve..]);
        assert!(decode(&edited).is_none());
    }
}

#[test]
fn another_wire_body_with_a_complete_curve_path_abstains() {
    let bytes = stream(&points());
    let body = stream_header(&bytes).unwrap().unwrap().body_offset + 6;
    let c = record(&bytes, 134);
    let n = record(&bytes, 136);
    let v = record(&bytes, 45);
    for kind in [2_u8, 3] {
        let mut edited = bytes.clone();
        let mut extra_body = vec![0, 12];
        let mut body_payload = bytes[body + 3..c].to_vec();
        body_payload[..2].copy_from_slice(&4_u16.to_be_bytes());
        body_payload[43] = kind;
        body_payload[49..51].copy_from_slice(&65_u16.to_be_bytes());
        extra_body.extend(body_payload);
        let mut extra_curve = bytes[c..c + 24].to_vec();
        extra_curve.remove(2);
        extra_curve[2..4].copy_from_slice(&65_u16.to_be_bytes());
        extra_curve[2 + 8..2 + 10].copy_from_slice(&4_u16.to_be_bytes());
        extra_curve[2 + 17..2 + 19].copy_from_slice(&66_u16.to_be_bytes());
        let mut extra_nurbs = bytes[n..n + 28].to_vec();
        extra_nurbs.remove(2);
        extra_nurbs[2..4].copy_from_slice(&66_u16.to_be_bytes());
        extra_nurbs[2 + 19..2 + 21].copy_from_slice(&67_u16.to_be_bytes());
        let mut extra_vertices = bytes[v..v + 9 + points().len() * 24].to_vec();
        extra_vertices.remove(2);
        extra_vertices[6..8].copy_from_slice(&67_u16.to_be_bytes());
        append(&mut edited, &extra_body);
        append(&mut edited, &extra_curve);
        append(&mut edited, &extra_nurbs);
        append(&mut edited, &extra_vertices);
        if kind == 2 {
            assert!(decode(&edited).is_none());
        } else {
            assert_eq!(
                decode(&edited).expect("one eligible wire body"),
                points().map(|p| Point3::new(p[0], p[1], p[2]))
            );
        }
    }
}
