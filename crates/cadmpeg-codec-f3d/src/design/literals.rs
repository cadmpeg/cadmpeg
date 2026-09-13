// SPDX-License-Identifier: Apache-2.0

/// A checked nonempty string from a static literal.
pub(crate) fn nonempty(value: &'static str) -> cadmpeg_ir::products::NonBlankString {
    cadmpeg_ir::products::NonBlankString::new(value).expect("static literal must be nonempty")
}
