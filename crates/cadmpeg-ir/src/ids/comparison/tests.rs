// SPDX-License-Identifier: Apache-2.0

use std::cmp::Ordering;

#[test]
fn identity_text_comparison_uses_length_and_byte_order() {
    let ctx = crate::index::StandardIndex;
    for (first, second, expected) in [
        ("alpha", "longer", false),
        ("alpha", "blope", false),
        ("alpha", "alpha", true),
        ("é", "ê", false),
        ("", "", true),
    ] {
        assert_eq!(
            super::equal(&ctx, first, second, "actual text equality").unwrap(),
            expected
        );
    }
    for (first, second, expected) in [
        ("alpha", "beta", Ordering::Less),
        ("alpha", "alpha", Ordering::Equal),
        ("a", "alpha", Ordering::Less),
        ("é", "ê", Ordering::Less),
        ("", "a", Ordering::Less),
    ] {
        assert_eq!(
            super::compare(&ctx, first, second, "actual cache comparison").unwrap(),
            expected
        );
    }
}
