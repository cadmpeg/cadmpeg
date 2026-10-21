// SPDX-License-Identifier: Apache-2.0
//! Design-loss and geometry-report tests for SLDPRT decode.

fn empty_report(geometry_transferred: bool) -> cadmpeg_ir::codec::DecodeBody {
    cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer {
        scope: cadmpeg_ir::ContentScope::full(),
        geometry_transferred,
    })
}

mod active_configuration;
mod admission;
mod body_membership;
mod configuration;
mod configuration_completeness;
mod configuration_sites;
mod curves_and_loops;
mod design_completeness;
mod digest;
mod feature_degradation;
mod feature_snapshots;
mod geometry_report;
mod lane_indexes;
mod merges;
mod metadata_fallback;
mod nurbs_surfaces;
mod partition_merge;
mod pcurves;
mod round_trip;
mod sketch_losses;

mod scope;
