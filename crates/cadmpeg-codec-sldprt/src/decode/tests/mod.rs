// SPDX-License-Identifier: Apache-2.0
//! Design-loss and geometry-report tests for SLDPRT decode.

fn empty_report(geometry_transferred: bool) -> cadmpeg_ir::report::DecodeReport {
    cadmpeg_ir::report::DecodeReport::unclassified(
        "sldprt",
        cadmpeg_ir::DecodeTransfer::full(geometry_transferred),
        std::collections::BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        cadmpeg_ir::report::TransferLedger::default(),
    )
}

mod admission;
mod body_membership;
mod configuration;
mod configuration_completeness;
mod configuration_sites;
mod curves_and_loops;
mod design_completeness;
mod feature_degradation;
mod geometry_report;
mod metadata_fallback;
mod nurbs_surfaces;
mod partition_merge;
mod pcurves;
mod round_trip;
mod site_keys;
mod sketch_losses;
