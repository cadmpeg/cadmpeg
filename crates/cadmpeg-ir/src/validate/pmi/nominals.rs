// SPDX-License-Identifier: Apache-2.0
//! A PMI dimension's bound nominal (part 10, §8): the nominal's quantity is
//! the kind of the parameter it binds, a length, an angle, or dimensionless,
//! and it agrees with `dimension` as a literal nominal's quantity does.

use std::collections::{BTreeMap, BTreeSet};

use super::error;
use crate::design::dimension::Dimension;
use crate::design::parameter::DesignParameter;
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::pmi::{PmiDimension, PmiQuantity};
use crate::report::check::Finding;

/// The parameters a bound nominal can name: the design root's, whose kinds
/// it reads, and the model's, which state no kind.
pub(super) struct BoundParameters<'a> {
    design: BTreeMap<&'a str, &'a DesignParameter>,
    model: BTreeSet<&'a str>,
}

impl<'a> BoundParameters<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
        Self {
            design: ir
                .design
                .parameters
                .iter()
                .map(|parameter| (parameter.id.as_str(), parameter))
                .collect(),
            model: ir
                .model
                .parameters
                .iter()
                .map(|parameter| parameter.id.as_str())
                .collect(),
        }
    }

    /// The design parameter a nominal binds, where the design states it.
    pub(super) fn design(&self, id: &str) -> Option<&'a DesignParameter> {
        self.design.get(id).copied()
    }
}

/// The quantity a PMI nominal has that binds a parameter of `kind`: a
/// length, an angle, or a dimensionless ratio. `None` for any other kind.
pub(super) fn bound_quantity(kind: &ValueKind) -> Option<PmiQuantity> {
    match kind {
        ValueKind::Quantity(Quantity::LENGTH) => Some(PmiQuantity::Length),
        ValueKind::Quantity(Quantity::ANGLE) => Some(PmiQuantity::Angle),
        ValueKind::Quantity(Quantity::REAL) => Some(PmiQuantity::Ratio),
        ValueKind::Quantity(_)
        | ValueKind::Integer
        | ValueKind::Boolean
        | ValueKind::Text
        | ValueKind::Native(_) => None,
    }
}

/// The quantity of a dimension's stated nominal: a literal's own, or that of
/// the parameter it binds. `None` where no nominal is stated or its quantity
/// is not known.
pub(super) fn nominal_quantity(
    parameters: &BoundParameters<'_>,
    dimension: &PmiDimension,
) -> Option<PmiQuantity> {
    match dimension.nominal.as_ref()? {
        Dimension::Literal(value) => Some(value.quantity),
        Dimension::Parameter(id) | Dimension::Negated(id) => parameters
            .design(id.as_str())
            .and_then(|parameter| bound_quantity(&parameter.kind)),
    }
}

pub(super) fn check_bound_nominal(
    parameters: &BoundParameters<'_>,
    annotation: &str,
    dimension: &PmiDimension,
    findings: &mut Vec<Finding>,
) {
    let Some(Dimension::Parameter(id) | Dimension::Negated(id)) = &dimension.nominal else {
        return;
    };
    let Some(parameter) = parameters.design.get(id.as_str()) else {
        if !parameters.model.contains(id.as_str()) {
            error(
                findings,
                annotation,
                crate::rule!(
                    "pmi.dimension-nominal-binds-no-parameter-document",
                    "dimension nominal binds {id}, which is no parameter of the document"
                ),
            );
        }
        return;
    };
    let quantity = match bound_quantity(&parameter.kind) {
        Some(quantity) => quantity,
        None if matches!(parameter.kind, ValueKind::Native(_)) => return,
        None => {
            error(
                findings,
                annotation,
                crate::rule!("pmi.dimension-nominal-binds-kind-no-length", 
                    "dimension nominal binds {id}, whose kind is no length, angle, or dimensionless quantity"
                ),
            );
            return;
        }
    };
    if dimension
        .dimension
        .quantity()
        .is_some_and(|expected| expected != quantity)
    {
        error(
            findings,
            annotation,
            crate::rule!(
                "pmi.dimension-nominal-binds-kind-disagrees-dimension",
                "dimension nominal binds {id}, whose kind disagrees with the dimension's kind"
            ),
        );
    }
}
