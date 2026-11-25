// SPDX-License-Identifier: Apache-2.0
//! The links of a component pattern's chain layout: distinct pins on the
//! geometry of the pattern's seeds.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;

use super::{
    assert_finding, assert_no_finding_containing, empty_construction, entity, id, named, pick, with,
};
use crate::design::construction::{kinds, Construction, ConstructionInput, InputReference};
use crate::design::feature::FeatureOperation;
use crate::design::names::NameKind;
use crate::design::ops::assembly::ComponentPattern;
use crate::design::ops::shared::{ChainLayout, ChainLink, ChainLinks, PatternLayout};
use crate::design::selection::{
    OccurrencePath, PathStep, PickContext, PickTarget, Picked, StepRef,
};

/// `picked` inside the occurrences `path` steps through.
fn under(mut picked: Picked, path: &[&str]) -> Picked {
    let steps = path
        .iter()
        .map(|key| PathStep {
            occurrence: StepRef::Occurrence(id("occurrence", key)),
            configuration: None,
        })
        .collect();
    picked.context = Some(PickContext::Occurrence(OccurrencePath(steps)));
    picked
}

/// An axis through what the pick inside `path` picks.
fn pin(path: &[&str]) -> Construction<kinds::Axis> {
    let target = entity(NameKind::Edge, 1);
    Construction {
        inputs: vec![ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(named(vec![under(target, path)]))),
            value: None,
            reversed: None,
        }],
        ..empty_construction()
    }
}

fn chain(links: ChainLinks) -> FeatureOperation {
    FeatureOperation::ComponentPattern(Box::new(ComponentPattern {
        seeds: named(vec![under(pick(PickTarget::Component), &["a"])]),
        layout: PatternLayout::Chain(ChainLayout {
            path: named(Vec::new()),
            links,
        }),
        instances: Vec::new(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn chain_pins_are_two_axes_of_a_seed() {
    let one = "are one axis";
    let off = "picks geometry of no seed";
    let links = |pins| ChainLinks::Connected(vec![ChainLink { pins }]);
    let ir = with(chain(links([pin(&["a"]), pin(&["a", "c"])])));
    assert_no_finding_containing(&ir, one);
    assert_no_finding_containing(&ir, off);
    assert_finding(&with(chain(links([pin(&["a"]), pin(&["a"])]))), one);
    assert_finding(&with(chain(links([pin(&["a"]), pin(&["b"])]))), off);
    let native = |path: &[&str]| ChainLinks::Native {
        kind: NonBlankString::new("test:chain").unwrap(),
        picks: named(vec![under(pick(PickTarget::Component), path)]),
    };
    let off = "ChainLinks::Native picks geometry of no seed";
    assert_no_finding_containing(&with(chain(native(&["a", "c"]))), off);
    assert_finding(&with(chain(native(&["b"]))), off);
}
