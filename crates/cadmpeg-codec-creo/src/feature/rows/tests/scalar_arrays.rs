// SPDX-License-Identifier: Apache-2.0

use crate::feature::rows::FeatureFieldValue;
use crate::psb;

fn field_value(payload: &[u8]) -> crate::feature::rows::FeatureFieldValue {
    crate::decode::with_test_decode_ctx(|ctx| crate::feature::rows::field_value(ctx, payload))
        .expect("field value is admitted")
}

#[test]
fn decodes_compact_feature_scalar_array_extents() {
    let mut payload = vec![psb::token::SCALAR_BODY, 0x80, 0x88, 0x03];
    payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

    let FeatureFieldValue::ScalarArray {
        dimensions,
        count,
        body,
        decoded_values,
    } = field_value(&payload)
    else {
        panic!("scalar array");
    };
    assert_eq!(dimensions, 136);
    assert_eq!(count, 3);
    assert_eq!(body.len(), 408);
    assert_eq!(decoded_values, Some(vec![0.0; 408]));
}

