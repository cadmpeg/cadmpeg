//! Stable digests over projected sketches, constraints and lanes.

use crate::history::hash::hash_records;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Stable hash of neutral sketch records.
pub(crate) fn sketch_hash(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<String, CodecError> {
    hash_records(
        ctx,
        &(
            &ir.model.sketches,
            &ir.model.sketch_entities,
            &ir.model.sketch_constraints,
            &ir.model.spatial_sketches,
            &ir.model.spatial_sketch_entities,
        ),
    )
}

/// Stable hash of neutral sketch constraints.
pub(crate) fn constraint_hash(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<String, CodecError> {
    hash_records(ctx, &ir.model.sketch_constraints)
}

/// Stable hash of retained native feature-input lanes.
pub(crate) fn lane_hash(
    ctx: &DecodeContext<'_>,
    lanes: &[crate::records::FeatureInputLane],
) -> Result<String, CodecError> {
    hash_records(ctx, lanes)
}
