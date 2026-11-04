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
