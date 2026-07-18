// SPDX-License-Identifier: Apache-2.0
//! Verifies per-codec stage-2 oracle selection against parser manifests.

use cadmpeg_harness::stage2::{statuses, Stage2Oracle};

const BASE: [Stage2Oracle; 3] = [
    Stage2Oracle::NoBypass,
    Stage2Oracle::ResourceClassification,
    Stage2Oracle::BudgetEnforcement,
];

const BUILDER_ADOPTED: [&str; 5] = ["catia", "creo", "f3d", "rhino", "sldprt"];

const TICKET_ADOPTED: [&str; 6] = ["catia", "creo", "f3d", "nx", "rhino", "sldprt"];

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
fn typed_lossy_builder_adoption_matches_expected_codecs() {
    let root = cadmpeg_harness::stage2::workspace_root();
    for status in statuses(&root).expect("read manifests") {
        let expected = BUILDER_ADOPTED.contains(&status.codec_id.as_str());
        assert_eq!(
            status.typed_lossy_builder, expected,
            "codec {} typed-lossy-builder adoption {} disagrees with \
             {BUILDER_ADOPTED:?}",
            status.codec_id, status.typed_lossy_builder
        );
    }
}
