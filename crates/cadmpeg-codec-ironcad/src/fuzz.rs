// SPDX-License-Identifier: Apache-2.0
//! Wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper runs one internal parser over arbitrary bytes. The fuzz target
//! checks that no input panics.

use cadmpeg_core::decode::View;

/// Exercise the `Brep Data` record-tree walk.
pub fn record_tree(data: &[u8]) {
    drop(crate::tree::read(View::over_retained(data)));
}

/// Exercise the `Public Access` part-record reader.
pub fn public_access(data: &[u8]) {
    drop(crate::placement::records(data));
}

/// Exercise the `Document Data` unit-entry reader.
pub fn document_units(data: &[u8]) {
    std::hint::black_box(crate::units::scene_units(data));
}
