// SPDX-License-Identifier: Apache-2.0

use super::SchemaNode;
use crate::test_support::with_decode_context;

fn region() -> Vec<u8> {
    // V13 REGION: node_id, attributes_features, body, next, previous, shell, type.
    let mut bytes = vec![0, 19, 255, 0, 3];
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    for (target, status) in [(1u16, 1), (7, 0), (1, 1), (9, 1), (10, 0)] {
        bytes.extend_from_slice(&target.to_be_bytes());
        bytes.push(status);
    }
    bytes.push(b'S');
    bytes
}

#[test]
fn region_reads_every_declared_field_and_preserves_statuses_and_scalar_bits() {
    let bytes = region();
    with_decode_context(|ctx| {
        let (node, end) = SchemaNode::read(ctx, &bytes, 0).unwrap().unwrap();
        assert_eq!(end, 25);
        assert_eq!(node.kind(), 19);
        let wire = serde_json::to_value(&node).unwrap();
        assert_eq!(wire["schema_byte_len"], 3);
        assert_eq!(wire["xmt"], 3);
        assert!(wire["variable_length"].is_null());
        assert_eq!(
            wire["values"][0],
            serde_json::json!([{"type":"integer","value":u32::MAX}])
        );
        assert_eq!(
            wire["values"][2],
            serde_json::json!([{"type":"pointer","value":{"target":7,"status":false}}])
        );
        assert_eq!(
            wire["values"][6],
            serde_json::json!([{"type":"char","value":83}])
        );
        let mut followed = bytes.clone();
        followed.extend_from_slice(&[0, 19, 0, 3]);
        assert_eq!(
            SchemaNode::read(ctx, &followed, 0).unwrap(),
            Some((node.clone(), end))
        );
        for length in 0..bytes.len() {
            assert!(SchemaNode::read(ctx, &bytes[..length], 0)
                .unwrap()
                .is_none());
        }
        assert_eq!(
            serde_json::from_value::<SchemaNode>(wire.clone()).unwrap(),
            node
        );
        for (field, value) in [
            ("xmt", serde_json::json!(1)),
            ("variable_length", serde_json::json!(0)),
            ("schema_byte_len", serde_json::json!(2)),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<SchemaNode>(invalid).is_err());
        }
        for values in [
            serde_json::json!([]),
            serde_json::json!([{"type":"byte","value":1}]),
            serde_json::json!([{"type":"integer","value":1},{"type":"integer","value":2}]),
        ] {
            let mut invalid = wire.clone();
            invalid["values"][0] = values;
            assert!(serde_json::from_value::<SchemaNode>(invalid).is_err());
        }
    });
}

#[test]
fn variable_limit_retains_two_complete_ext11_blocks_without_swallowing_the_successor() {
    let mut bytes = vec![0, 41, 3, b'C', b'I', 8];
    bytes.extend_from_slice(b"term_use");
    bytes.extend_from_slice(&[0, 0, 0, 1, 1, b'c', b'C', b'Z']);
    let schema_end = bytes.len();
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&[0, 86, b'T', b'?']);
    for value in 0..22 {
        bytes.extend_from_slice(&f64::from(value).to_be_bytes());
    }
    let end = bytes.len();
    bytes.extend_from_slice(&[0, 40, 0, 1]);
    with_decode_context(|ctx| {
        let (node, actual_end) = SchemaNode::read(ctx, &bytes, 0).unwrap().unwrap();
        assert_eq!(actual_end, end);
        assert_eq!(node.schema_byte_len, schema_end);
        let wire = serde_json::to_value(&node).unwrap();
        assert_eq!(wire["variable_length"], 2);
        assert_eq!(wire["values"][0][0]["value"], 84);
        assert_eq!(wire["values"][1][0]["value"], 63);
        assert_eq!(
            wire["values"][2][0]["value"],
            serde_json::json!([0., 1., 2., 3., 4., 5., 6., 7., 8., 9., 10.])
        );
        assert_eq!(
            wire["values"][2][1]["value"],
            serde_json::json!([11., 12., 13., 14., 15., 16., 17., 18., 19., 20., 21.])
        );
        for length in schema_end..end {
            assert!(SchemaNode::read(ctx, &bytes[..length], 0)
                .unwrap()
                .is_none());
        }
        let mut invalid = bytes.clone();
        invalid[schema_end..schema_end + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(SchemaNode::read(ctx, &invalid, 0).unwrap().is_none());
        let mut nonfinite = bytes.clone();
        nonfinite[end - 8..end].copy_from_slice(&f64::NAN.to_be_bytes());
        assert!(SchemaNode::read(ctx, &nonfinite, 0).unwrap().is_none());
        let mut missing = wire.clone();
        missing["values"][2].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<SchemaNode>(missing).is_err());
    });
}

#[test]
fn omitted_variable_storage_has_a_length_but_no_values() {
    let mut bytes = vec![0, 41, 2, b'C', b'D', b'A', 4];
    bytes.extend_from_slice(b"skip");
    bytes.extend_from_slice(&[0, 0, 0, 2, 1, b'd', 0, b'Z']);
    bytes.extend_from_slice(&4u32.to_be_bytes());
    bytes.extend_from_slice(&[0, 9, b'T']);
    with_decode_context(|ctx| {
        let (node, end) = SchemaNode::read(ctx, &bytes, 0).unwrap().unwrap();
        assert_eq!(end, bytes.len());
        assert_eq!(node.variable_length, Some(4));
        assert!(node.values[1].is_empty());
        let mut invalid = serde_json::to_value(node).unwrap();
        invalid["values"][1] = serde_json::json!([{"type":"integer","value":7}]);
        assert!(serde_json::from_value::<SchemaNode>(invalid).is_err());
    });
}
