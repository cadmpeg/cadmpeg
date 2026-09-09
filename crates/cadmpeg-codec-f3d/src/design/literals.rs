// SPDX-License-Identifier: Apache-2.0

/// A checked nonempty string from a static literal.
pub(crate) fn nonempty(value: &'static str) -> cadmpeg_ir::products::NonEmptyString {
    cadmpeg_ir::products::NonEmptyString::new(value).expect("static literal must be nonempty")
}
