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
    DefinitionRef, OccurrencePath, PathStep, PickContext, PickTarget, Picked, StepRef,
};

/// The path through the occurrences `keys` name.
fn path(keys: &[&str]) -> OccurrencePath {
    let steps = keys
        .iter()
        .map(|key| PathStep {
            occurrence: StepRef::Occurrence(id("occurrence", key)),
            configuration: None,
        })
        .collect();
    OccurrencePath(steps)
}

/// `picked` inside the occurrences `keys` step through.
fn under(mut picked: Picked, keys: &[&str]) -> Picked {
    picked.context = Some(PickContext::Occurrence(path(keys)));
    picked
}

/// An axis through what the pick inside `path` picks.
fn pin(path: &[&str]) -> Construction<kinds::Axis> {
    through(under(entity(NameKind::Edge, 1), path))
}

/// An axis through what `picked` picks.
fn through(picked: Picked) -> Construction<kinds::Axis> {
    Construction {
        inputs: vec![ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(named(vec![picked]))),
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

#[test]
fn an_in_context_pin_is_judged_by_its_target_path() {
    let off = "picks geometry of no seed";
    let in_context = |keys: &[&str]| {
        let mut picked = entity(NameKind::Edge, 1);
        picked.context = Some(PickContext::InContext {
            assembly: DefinitionRef {
                definition: id("definition", "assembly"),
                configuration: None,
            },
            own: None,
            target: Some(path(keys)),
        });
        through(picked)
    };
    let links = |second| {
        ChainLinks::Connected(vec![ChainLink {
            pins: [pin(&["a"]), second],
        }])
    };
    assert_no_finding_containing(&with(chain(links(in_context(&["a", "c"])))), off);
    assert_finding(&with(chain(links(in_context(&["b"])))), off);
}
