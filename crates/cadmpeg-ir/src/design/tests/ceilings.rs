// SPDX-License-Identifier: Apache-2.0
//! Each nesting ceiling admits a value nested to it and refuses one level
//! past it with `ResourceLimit`.

use std::collections::BTreeMap;
use std::fmt::Debug;

use cadmpeg_core::decode::{ResourceDimension, ResourceFailure, ResourceLimits};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::fixtures::{container, feature, picked};
use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, InputReference,
};
use crate::design::dimension::ParameterRef;
use crate::design::expr::{Neutral, NeutralWire};
use crate::design::names::{NameEntry, TopoName};
use crate::design::read::{self, Ceiling};
use crate::design::selection::{PickDomain, Picked};

fn policies() -> [ResourceLimits; 2] {
    [ResourceLimits::desktop(), ResourceLimits::service()]
}

/// Requires `at` to read and `past` to be refused with the ceiling's
/// `ResourceLimit`.
fn admits_and_refuses<T: Serialize + DeserializeOwned + Debug>(
    ceiling: Ceiling,
    limits: &ResourceLimits,
    at: &str,
    past: &str,
) {
    let limit = ceiling.of(limits);
    if let Err(error) = read::from_str::<T>(at, limits) {
        panic!("{ceiling:?} at its ceiling of {limit} is refused: {error}");
    }
    match read::from_str::<T>(past, limits) {
        Err(CodecError::ResourceLimit(refusal)) => {
            assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
            assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
            assert_eq!(refusal.limit, limit);
            assert_eq!(refusal.used, limit);
            assert_eq!(refusal.additional, 1);
        }
        other => panic!("{ceiling:?} one past its ceiling of {limit} gave {other:?}"),
    }
}

fn negations(depth: u64) -> String {
    let wire = NeutralWire::<ParameterRef> {
        text: format!("{}1", "-".repeat(usize::try_from(depth - 1).unwrap())),
        refs: Vec::new(),
        measures: Vec::new(),
    };
    serde_json::to_string(&wire).unwrap()
}

fn modified(depth: u64) -> String {
    let mut name = TopoName::Root {
        container: container(1),
    };
    for _ in 1..depth {
        name = TopoName::Modified {
            feature: feature(1),
            from: Box::new(name),
        };
    }
    serde_json::to_string(&name).unwrap()
}

fn limited(depth: u64) -> String {
    let mut pick = picked(1);
    for _ in 1..depth {
        pick = Picked {
            domain: Some(PickDomain::Limits {
                start: Some(Box::new(pick)),
                end: None,
            }),
            ..picked(2)
        };
    }
    serde_json::to_string(&pick).unwrap()
}

fn built(depth: u64) -> String {
    let empty = || Construction::<kinds::Plane> {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    };
    let mut construction = empty();
    for _ in 1..depth {
        construction = Construction {
            inputs: vec![ConstructionInput {
                role: None,
                reference: Some(InputReference::Built(Box::new(AnyConstruction::Plane(
                    construction,
                )))),
                value: None,
                reversed: None,
            }],
            ..empty()
        };
    }
    serde_json::to_string(&construction).unwrap()
}

#[test]
fn expression_text_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Expression.of(&limits);
        admits_and_refuses::<Neutral<ParameterRef>>(
            Ceiling::Expression,
            &limits,
            &negations(limit),
            &negations(limit + 1),
        );
    }
}

#[test]
fn a_topo_name_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Name.of(&limits);
        admits_and_refuses::<TopoName>(
            Ceiling::Name,
            &limits,
            &modified(limit),
            &modified(limit + 1),
        );
    }
}

#[test]
fn a_picked_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Pick.of(&limits);
        admits_and_refuses::<Picked>(Ceiling::Pick, &limits, &limited(limit), &limited(limit + 1));
    }
}

#[test]
fn a_construction_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Construction.of(&limits);
        admits_and_refuses::<Construction<kinds::Plane>>(
            Ceiling::Construction,
            &limits,
            &built(limit),
            &built(limit + 1),
        );
    }
}

/// A ceiling refusal inside a larger value is still `ResourceLimit`, and a
/// read after it starts from depth zero.
#[test]
fn a_nested_refusal_surfaces_and_does_not_leak_into_the_next_read() {
    let limits = ResourceLimits::service();
    let limit = Ceiling::Name.of(&limits);
    let entry = format!("{{\"name\":{}}}", modified(limit + 1));
    assert!(matches!(
        read::from_str::<NameEntry>(&entry, &limits),
        Err(CodecError::ResourceLimit(_))
    ));
    read::from_str::<TopoName>(&modified(limit), &limits).unwrap();
    let malformed = read::from_str::<TopoName>("{\"kind\":\"root\"}", &limits);
    assert!(
        matches!(malformed, Err(CodecError::Malformed { .. })),
        "{malformed:?}"
    );
}

/// The ceilings sit below the JSON reader's own nesting limit, so each is
/// reached first.
#[test]
fn the_desktop_ceilings_are_reached_below_the_json_limit() {
    let limits = ResourceLimits::desktop();
    let past = |ceiling: Ceiling| ceiling.of(&limits) + 1;
    assert!(matches!(
        read::from_str::<TopoName>(&modified(past(Ceiling::Name)), &limits),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(matches!(
        read::from_str::<Picked>(&limited(past(Ceiling::Pick)), &limits),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(matches!(
        read::from_str::<Construction<kinds::Plane>>(&built(past(Ceiling::Construction)), &limits),
        Err(CodecError::ResourceLimit(_))
    ));
}
