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
    let quantity = match &parameter.kind {
        ValueKind::Quantity(Quantity::LENGTH) => PmiQuantity::Length,
        ValueKind::Quantity(Quantity::ANGLE) => PmiQuantity::Angle,
        ValueKind::Quantity(Quantity::REAL) => PmiQuantity::Ratio,
        ValueKind::Native(_) => return,
        ValueKind::Quantity(_) | ValueKind::Integer | ValueKind::Boolean | ValueKind::Text => {
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
