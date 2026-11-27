// SPDX-License-Identifier: Apache-2.0
//! Design-loss and geometry-report tests for SLDPRT decode.

fn empty_report(geometry_transferred: bool) -> cadmpeg_ir::codec::DecodeBody {
    cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::full(
        geometry_transferred,
    ))
}

/// How many `record.unreadable` charges of `result` give a record up with
/// `code` as their cause.
fn given_up(result: &cadmpeg_ir::codec::DecodeResult, code: crate::loss::SldprtLossCode) -> usize {
    let cause = format!(" given up: {}:", code.kind());
    result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss.message.contains(&cause)
        })
        .count()
}

mod admission;
mod body_membership;
mod configuration;
mod configuration_completeness;
mod configuration_sites;
mod curves_and_loops;
mod design_completeness;
mod feature_degradation;
mod feature_snapshots;
mod geometry_report;
mod lane_indexes;
mod lost_faces;
mod merges;
mod metadata_fallback;
mod nurbs_surfaces;
mod partition_merge;
mod pcurves;
mod round_trip;
mod sketch_losses;
