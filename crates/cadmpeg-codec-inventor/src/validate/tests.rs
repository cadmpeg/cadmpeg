// SPDX-License-Identifier: Apache-2.0
//! Kept native states equal the pairs, except where a given-up stub links the pair.

use std::collections::HashSet;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::{Identity, UnknownId};
use cadmpeg_ir::report::check::Finding;
use cadmpeg_ir::{CadIr, NativeUnknownRecord};

use crate::test_support::test_fixtures::{
    primary_envelope_fixture, primary_envelope_fixture_with_design,
};
use crate::InventorCodec;

const PAIR_TOKEN: &str = "seg";
const MISSING: &str = "Inventor segment metadata states do not cover paired segments exactly";
const UNPAIRED: &str = "Inventor segment metadata state has no paired segment";

#[test]
fn a_pair_without_a_state_or_a_linking_stub_needs_its_state() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(&mut findings, &pairs, &HashSet::new(), [], [], "metadata");
    assert_eq!(findings.len(), 1);
    assert!(findings[0]
        .message
        .contains("states do not cover paired segments exactly"));
}

#[test]
fn every_kept_metadata_state_still_needs_its_actual_pair() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(
        &mut findings,
        &pairs,
        &HashSet::from(["seg"]),
        ["orphan"],
        [],
        "metadata",
    );
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("has no paired segment"));
}

#[test]
fn duplicate_kept_metadata_states_are_still_rejected() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(
        &mut findings,
        &pairs,
        &HashSet::new(),
        ["seg", "seg"],
        [],
        "metadata",
    );
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("repeats"));
}

fn decoded() -> CadIr {
    decode(primary_envelope_fixture())
}

fn decode(bytes: Vec<u8>) -> CadIr {
    InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("primary envelope decodes")
        .ir()
        .clone()
}

fn segment_state_findings(ir: &CadIr) -> Vec<Finding> {
    let ctx = crate::native::test_ctx();
    InventorCodec
        .validate_native(&ctx, ir)
        .expect("validation")
        .into_iter()
        .filter(|finding| {
            finding
                .message
                .ends_with("states do not cover paired segments exactly")
                || finding.message.ends_with("state has no paired segment")
        })
        .collect()
}

fn edit_metadata_states(ir: &mut CadIr, edit: impl FnOnce(&mut Vec<serde_json::Value>)) {
    let ctx = crate::native::test_ctx();
    let namespace = ir.native.namespace_mut("inventor");
    let mut states = namespace
        .arena_as::<serde_json::Value>("segment_meta")
        .expect("metadata states");
    edit(&mut states);
    namespace
        .set_arena(&ctx, "segment_meta", &states)
        .expect("edited metadata states");
}

fn remove_metadata_state(ir: &mut CadIr) {
    edit_metadata_states(ir, |states| {
        let before = states.len();
        states.retain(|state| state["token"] != PAIR_TOKEN);
        assert_eq!(before - states.len(), 1);
    });
}

fn add_stub(ir: &mut CadIr, links: Vec<Identity>) {
    let ctx = crate::native::test_ctx();
    let namespace = ir.native.namespace_mut("inventor");
    let mut stubs = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("stubs");
    stubs.push(NativeUnknownRecord {
        id: UnknownId::mint("inventor:part:test#given-up").expect("stub identity"),
        links,
    });
    namespace
        .set_arena(&ctx, "unknowns", &stubs)
        .expect("stub arena");
}

#[test]
fn the_decoded_states_equal_the_pairs() {
    assert!(segment_state_findings(&decoded()).is_empty());
}

#[test]
fn a_missing_state_whose_segment_record_a_given_up_stub_links_is_accepted() {
    let mut ir = decoded();
    remove_metadata_state(&mut ir);
    add_stub(
        &mut ir,
        vec![Identity::new(crate::rse::segment_record_id(PAIR_TOKEN)).expect("pair id")],
    );
    assert!(segment_state_findings(&ir).is_empty());
}

#[test]
fn a_missing_state_without_a_linking_stub_is_refused() {
    let mut ir = decoded();
    remove_metadata_state(&mut ir);
    add_stub(
        &mut ir,
        vec![Identity::new("inventor:rse:segment-meta#seg".to_owned()).expect("other id")],
    );
    let findings = segment_state_findings(&ir);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].message, MISSING);
}

#[test]
fn an_extra_state_that_matches_no_pair_is_refused() {
    let mut ir = decoded();
    edit_metadata_states(&mut ir, |states| {
        let mut extra = states[0].clone();
        extra["id"] = "inventor:rse:segment-meta#orphan".into();
        extra["token"] = "orphan".into();
        states.push(extra);
    });
    let findings = segment_state_findings(&ir);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].message, UNPAIRED);
}

#[test]
fn a_given_up_type_table_links_its_segment_record_and_its_missing_states_validate() {
    let design = vec![([0x11; 16], Vec::new()); 300];
    let ir = decode(primary_envelope_fixture_with_design(&[], &design));
    let namespace = ir.native.namespace("inventor").expect("namespace");
    let metadata = namespace
        .arena_as::<serde_json::Value>("segment_meta")
        .expect("metadata states");
    assert!(metadata.iter().all(|state| state["token"] != "dc"));
    let stubs = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("stubs");
    assert!(stubs.iter().any(|stub| stub
        .links
        .iter()
        .any(|link| link.as_str() == crate::rse::segment_record_id("dc"))));
    assert!(segment_state_findings(&ir).is_empty());
}
