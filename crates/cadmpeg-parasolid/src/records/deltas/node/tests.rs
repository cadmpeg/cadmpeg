// SPDX-License-Identifier: Apache-2.0
use crate::records::deltas::DeltasRecords;
use crate::records::instance::FieldValue;
use crate::test_support::{owned_deltas, test_pmarks, with_decode_context};

#[test]
fn deltas_nodes_expose_their_fields_by_schema_name() {
    let (declaration, ordinary, _) = owned_deltas::body(27);
    let mut bytes = test_pmarks::prefix(1);
    let first_start = bytes.len();
    bytes.extend(&declaration);
    bytes.extend_from_slice(&ordinary[2..]);
    bytes.extend(test_pmarks::TERMINATOR);
    with_decode_context(|_ctx| {
        let records = DeltasRecords::read(&bytes).unwrap();
        let body = records
            .nodes()
            .find(|node| node.kind() == 12)
            .expect("BODY instance");
        assert_eq!(body.range().start, first_start);
        assert_eq!(body.index(), 3);
        assert_eq!(body.integer("highest_node_id"), Some(0x71e4));
        assert_eq!(
            body.real("res_size").map(f64::to_bits),
            Some(0x408f_4000_0000_0001)
        );
        let values = body.values("attributes_features").unwrap();
        let [FieldValue::Pointer { target, status }] = values.as_slice() else {
            panic!("attributes_features pointer");
        };
        let (target, status) = (target.get(), *status);
        assert_eq!(
            body.pointer_status("attributes_features"),
            Some((target, status))
        );
        assert_eq!(body.pointer("attributes_features"), Some(target));
        assert_eq!(
            body.pointers(["attributes_features", "shell"])
                .map(|pointers| pointers[0]),
            Some(target)
        );
        assert_eq!(body.field_count(), 27);
        assert_eq!(body.values_at(0), Some(vec![FieldValue::Integer(0x71e4)]));
        assert_eq!(body.values_at(27), None);
        assert_eq!(body.is_null("res_size"), Some(false));
        assert_eq!(
            crate::records::NodeFields::integer(&body, "highest_node_id"),
            Some(0x71e4)
        );
        assert_eq!(body.integer("res_size"), None);
        assert_eq!(body.pointer("absent"), None);
        let range = body.field_range("highest_node_id").unwrap();
        assert_eq!(bytes[range], 0x71e4u32.to_be_bytes());
    });
}

fn status_pointer(bytes: &mut Vec<u8>, target: u16) {
    bytes.extend(target.to_be_bytes());
    bytes.push(1);
}

#[test]
fn deltas_nodes_read_arrays_and_hvec_points() {
    let mut bytes = test_pmarks::prefix(1);
    bytes.extend([0, 127, 255]);
    bytes.extend(3u32.to_be_bytes());
    bytes.extend(6u16.to_be_bytes());
    for value in [3u16, 1, 3] {
        bytes.extend(value.to_be_bytes());
    }
    bytes.extend([0, 41, 255]);
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(7u16.to_be_bytes());
    bytes.push(b'L');
    for value in [
        0.001f64, 0.002, 0.003, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0,
    ] {
        bytes.extend(value.to_be_bytes());
    }
    bytes.extend([0, 56, 255]);
    bytes.extend(8u16.to_be_bytes());
    bytes.extend(9u32.to_be_bytes());
    for target in [1, 1, 1, 1, 1] {
        status_pointer(&mut bytes, target);
    }
    bytes.extend(b"+R");
    for target in [20, 21, 30] {
        status_pointer(&mut bytes, target);
    }
    for value in [0.002f64, -0.5, 1.0, 2.0] {
        bytes.extend(value.to_be_bytes());
    }
    for target in [1, 1, 1, 1] {
        status_pointer(&mut bytes, target);
    }
    let bytes = test_pmarks::complete(bytes);
    with_decode_context(|_ctx| {
        let records = DeltasRecords::read(&bytes).unwrap();
        let find = |kind| records.nodes().find(|node| node.kind() == kind).unwrap();
        assert_eq!(find(127).shorts("mult"), Some(vec![3, 1, 3]));
        assert_eq!(
            find(41).hvec_points("hvec"),
            Some(vec![[0.001, 0.002, 0.003]])
        );
        let blend = find(56);
        assert_eq!(blend.pointer_array::<2>("surface"), Some([20, 21]));
        assert_eq!(blend.real_array::<2>("range"), Some([0.002, -0.5]));
        assert_eq!(blend.real_array::<2>("thumb_weight"), Some([1.0, 2.0]));
        assert_eq!(blend.pointer_array::<3>("surface"), None);
        assert_eq!(blend.pointer_list("surface"), None);
    });
}
