// SPDX-License-Identifier: Apache-2.0
//! Stage-2 gating-adoption gate (doc §7 capability matrix / §10 Phase 4C).
//!
//! Resolves every codec's stage-2 status from its committed
//! `parser-manifest.toml` and asserts the exact set of gating oracle rows. This
//! is the ratchet for Phase 4C: the last two matrix rows turn on per codec as
//! the capability lands in the manifest, so a codec advancing (a new ledger,
//! ticket, or `semantic_builder` module) or regressing (a capability withdrawn)
//! moves this expectation and fails the gate until it is reconciled.

use cadmpeg_harness::stage2::{statuses, Stage2Oracle};

/// The base four rows in force for every codec since 0B/Phase 1/Phase 2.
const BASE: [Stage2Oracle; 4] = [
    Stage2Oracle::NoBypass,
    Stage2Oracle::ResourceClassification,
    Stage2Oracle::StrictTruncation,
    Stage2Oracle::BudgetEnforcement,
];

/// The codecs whose manifest flags a `semantic_builder` module (Phase 4B), so
/// no-silent-fallback gates for them. `f3d` builds lossy IR through the shared
/// `cadmpeg_ir::transfer::{omit, reduce}` helpers called from `decode.rs`;
/// `creo` and `sldprt` through a `builder.rs`; `catia` through `b5_transfer.rs`;
/// `rhino` through `decode.rs`. `nx` constructs no lossy IR through the platform
/// typed builder —
/// its report path is plain note emission and its one value boundary (the
/// intersection secondary support) is an honest `Option` — so it does not gate.
const BUILDER_ADOPTED: [&str; 5] = ["catia", "creo", "f3d", "rhino", "sldprt"];

/// The codecs whose manifest flags a `record_tickets` module (Phase 3D), so
/// disposition validation gates for them. Every decoder codec except `step`
/// commits and resolves record tickets from its decode path (`catia` through
/// `tickets.rs`, the rest through `decode.rs`); `step` has no ticket boundary.
const TICKET_ADOPTED: [&str; 6] = ["catia", "creo", "f3d", "nx", "rhino", "sldprt"];

/// The gating rows expected for `codec_id`, given the current manifests.
///
/// Every codec carries an L1/L2 ledger, so byte-accounting gates for all six.
/// The [`TICKET_ADOPTED`] codecs flag a `record_tickets` module, so disposition
/// validation gates for them. The [`BUILDER_ADOPTED`] codecs flag a
/// `semantic_builder` module, so no-silent-fallback gates for them.
fn expected(codec_id: &str) -> Vec<Stage2Oracle> {
    let mut rows = BASE.to_vec();
    rows.push(Stage2Oracle::ByteAccounting);
    if TICKET_ADOPTED.contains(&codec_id) {
        rows.push(Stage2Oracle::DispositionValidation);
    }
    if BUILDER_ADOPTED.contains(&codec_id) {
        rows.push(Stage2Oracle::NoSilentFallback);
    }
    rows.sort();
    rows
}

#[test]
fn per_codec_gating_rows_match_manifests() {
    let root = cadmpeg_harness::stage2::workspace_root();
    let statuses = statuses(&root).expect("read every codec manifest");
    assert_eq!(statuses.len(), 6, "one status per decoder codec");

    for status in &statuses {
        let mut gating = status.gating_oracles();
        gating.sort();
        assert_eq!(
            gating,
            expected(&status.codec_id),
            "codec {} gates {:?}; manifest-derived status {:?}",
            status.codec_id,
            gating,
            status
        );
    }
}

#[test]
fn typed_lossy_builder_adoption_matches_the_graduated_codecs() {
    let root = cadmpeg_harness::stage2::workspace_root();
    for status in statuses(&root).expect("read manifests") {
        let expected = BUILDER_ADOPTED.contains(&status.codec_id.as_str());
        assert_eq!(
            status.typed_lossy_builder, expected,
            "codec {} typed-lossy-builder adoption {} disagrees with the \
             graduated set {BUILDER_ADOPTED:?}; reconcile `BUILDER_ADOPTED` \
             and `expected` when a codec's `semantic_builder` flag lands or \
             is withdrawn",
            status.codec_id, status.typed_lossy_builder
        );
    }
}
