// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::units::Units;

fn with_source(data: &[u8], run: impl FnOnce(&DecodeContext<'_>, cadmpeg_core::decode::View<'_>)) {
    let arena = DecodeArena::new();
    let Ok((ctx, source)) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())
    else {
        return;
    };
    run(&ctx, source);
}

/// Exercise bounded FCStd ZIP scanning.
pub fn container(data: &[u8]) {
    with_source(data, |ctx, source| {
        let _ = crate::container::scan(ctx, source);
    });
}

/// Exercise `Document.xml` persistence-graph recovery.
pub fn persistence(data: &[u8]) {
    let _ = crate::persistence::parse(data);
}

/// Exercise text and binary exact-shape carrier framing.
pub fn brep(data: &[u8]) {
    let _ = crate::brep::parse_text(data);
    let _ = crate::brep::parse_binary_prefix(data);
}

/// Exercise element-map XML recovery and the side-entry token grammar.
pub fn element_map(data: &[u8]) {
    let _ = crate::element_map::parse(data, &[], &[]);
    let _ = crate::element_map::parse_element_map(data, false);
    let _ = crate::element_map::parse_element_map(data, true);
}

/// Exercise `GuiDocument.xml` transfer with empty supporting tables.
pub fn gui(data: &[u8]) {
    let mut ir = CadIr::empty(Units::default());
    let entries = BTreeMap::new();
    let _ = crate::gui::transfer(&mut ir, data, &entries, &[], &[], &[], &[], false);
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::container(&[]);
        super::persistence(&[]);
        super::brep(&[]);
        super::element_map(&[]);
        super::gui(&[]);
    }
}
