//! Stable digests over projected sketches, constraints and lanes.

use sha2::Digest;
use sha2::Sha256;
use std::fmt::Write as _;

/// Stable hash of neutral sketch records.
pub fn sketch_hash(ir: &cadmpeg_ir::CadIr) -> String {
    hash_debug(&(
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &ir.model.sketch_constraints,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
    ))
}

/// Stable hash of neutral sketch constraints.
pub fn constraint_hash(ir: &cadmpeg_ir::CadIr) -> String {
    hash_debug(&ir.model.sketch_constraints)
}

/// Stable hash of retained native feature-input lanes.
pub fn lane_hash(native: &crate::native::SldprtNative) -> String {
    hash_debug(&native.feature_input_lanes)
}

fn hash_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> String {
    let bytes = format!("{value:?}");
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes.as_bytes()) {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}
