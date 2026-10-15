// SPDX-License-Identifier: Apache-2.0

use crate::brep::attributes::attribute_value;
use crate::sab::Token;
use cadmpeg_ir::attributes::AttributeValue;

#[test]
fn attribute_integer_42_is_free() {
    let token = Token::Long(42);
    crate::test_support::with_zero_context(|ctx| {
        let result = attribute_value(ctx, &token, crate::asm_format!("f3d"));

        assert_eq!(
            result.expect("fixed scalar is free"),
            Some(AttributeValue::Integer(42))
        );
    });
}

#[test]
fn attribute_nan_scalar_is_rejected_without_allocation() {
    let token = Token::Double(f64::NAN);
    crate::test_support::with_zero_context(|ctx| {
        let result = attribute_value(ctx, &token, crate::asm_format!("f3d"));

        assert_eq!(result.expect("nonfinite scalar recovery is free"), None);
    });
}

#[test]
fn attribute_nonfinite_vector_is_rejected_without_allocation() {
    let token = Token::Vector3([1.0, f64::NAN, 3.0]);
    crate::test_support::with_zero_context(|ctx| {
        let result = attribute_value(ctx, &token, crate::asm_format!("f3d"));

        assert_eq!(result.expect("invalid vector needs no lane"), None);
    });
}
