// SPDX-License-Identifier: Apache-2.0
//! The members of a fillet's transitions.

use std::collections::{BTreeMap, BTreeSet};

use super::super::super::fixture::{design_findings, id, parameter};
use super::{
    assert_finding, assert_no_finding_containing, assert_warning, edge_set, fillet, positive, with,
};
use crate::design::dimension::Dimension;
use crate::design::ops::dressup::{FilletSize, Transition, TransitionKind};
use crate::design::parameter::ParameterValue;
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::report::Severity;
use crate::scalar::{FiniteReal, PositiveLength};

const LARGEST: &str = "no single largest piece";

/// The fillet whose transition is a `SweptAround` between sets of `radii`.
fn swept(radii: Vec<Dimension<PositiveLength>>) -> CadIr {
    let between = (0..u32::try_from(radii.len()).unwrap()).collect();
    let sets = radii
        .into_iter()
        .map(|radius| edge_set(Some(FilletSize::Radius(radius))))
        .collect();
    let transition = Transition {
        between,
        at: None,
        kind: Some(TransitionKind::SweptAround {
            radius: None,
            against: None,
        }),
        native_options: BTreeMap::new(),
    };
    with(fillet(sets, vec![transition]))
}

fn literal(radius: f64) -> Dimension<PositiveLength> {
    Dimension::Literal(positive(radius))
}

#[test]
fn a_swept_around_of_three_members_has_one_largest_piece() {
    let swept = |radii: [f64; 3]| {
        let sets = radii
            .iter()
            .map(|radius| {
                edge_set(Some(FilletSize::Radius(Dimension::Literal(positive(
                    *radius,
                )))))
            })
            .collect();
        let transition = Transition {
            between: BTreeSet::from([0, 1, 2]),
            at: None,
            kind: Some(TransitionKind::SweptAround {
                radius: None,
                against: None,
            }),
            native_options: BTreeMap::new(),
        };
        with(fillet(sets, vec![transition]))
    };
    let message = "no single largest piece";
    assert_no_finding_containing(&swept([1.0, 3.0, 2.0]), message);
    assert_finding(&swept([3.0, 1.0, 3.0]), message);
}

#[test]
fn a_swept_around_through_parameters_with_no_largest_piece_is_a_warning() {
    let radius = || Dimension::Parameter(id("parameter", "radius"));
    let mut ir = swept(vec![radius(), literal(1.0), radius()]);
    let mut cached = parameter("radius", ValueKind::Quantity(Quantity::LENGTH));
    cached.value = Some(ParameterValue::Number(FiniteReal::new(3.0).unwrap()));
    ir.design.parameters.push(cached);
    assert_warning(&ir, LARGEST);
    assert!(design_findings(&ir)
        .iter()
        .all(|finding| finding.severity != Severity::Error || !finding.message.contains(LARGEST)));
}

#[test]
fn a_swept_around_of_two_members_has_no_largest_piece_to_judge() {
    assert_no_finding_containing(&swept(vec![literal(3.0), literal(3.0)]), LARGEST);
}
