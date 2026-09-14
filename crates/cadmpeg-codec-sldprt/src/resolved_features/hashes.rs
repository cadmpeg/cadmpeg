//! Stable digests over projected sketches, constraints and lanes.

use crate::history::hash_records;
use cadmpeg_core::CodecError;

/// Stable hash of neutral sketch records.
pub(crate) fn sketch_hash(ir: &cadmpeg_ir::CadIr) -> Result<String, CodecError> {
    hash_records(&(
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &ir.model.sketch_constraints,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
    ))
}

/// Stable hash of neutral sketch constraints.
pub(crate) fn constraint_hash(ir: &cadmpeg_ir::CadIr) -> Result<String, CodecError> {
    hash_records(&ir.model.sketch_constraints)
}

/// Stable hash of retained native feature-input lanes.
pub(crate) fn lane_hash(native: &crate::native::SldprtNative) -> Result<String, CodecError> {
    hash_records(&native.feature_input_lanes)
}
