// SPDX-License-Identifier: Apache-2.0
//! The one spelling of the sketch, datum, and shared operation types' unit
//! variants.

use std::collections::BTreeMap;
use std::path::Path;

use crate::design::ops::shared::RunSides;
use crate::design::ops::sketch::{
    AngleSector, DefiningRole, DerivationMode, DistanceAxis, EntityForm, Planar, SlotKind,
    SolveState, Spatial, TextAlign,
};
use crate::design::tests::unit_variants::{assert_units_refuse_value, reads, Reader};
use crate::scalar::Length;

fn source(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/design/ops")
        .join(name)
}

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 8] = [
        ("AngleSector", reads::<AngleSector>),
        ("DefiningRole", reads::<DefiningRole>),
        ("DerivationMode", reads::<DerivationMode>),
        ("DistanceAxis", reads::<DistanceAxis<Planar>>),
        ("EntityForm", reads::<EntityForm<Spatial>>),
        ("SlotKind", reads::<SlotKind>),
        ("SolveState", reads::<SolveState>),
        ("TextAlign", reads::<TextAlign>),
    ];
    let files = [source("sketch.rs"), source("datum.rs")];
    assert_units_refuse_value(&files, &BTreeMap::from(readers));
}

/// The shared run types' unit variants have the same one spelling.
#[test]
fn every_shared_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 1] = [("RunSides", reads::<RunSides<Length>>)];
    assert_units_refuse_value(&[source("shared.rs")], &BTreeMap::from(readers));
}
