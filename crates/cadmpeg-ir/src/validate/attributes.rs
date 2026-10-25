// SPDX-License-Identifier: Apache-2.0
//! Reference and placement domains of typed attribute values.

use crate::attributes::{AttributeNumericList, AttributeValue};
use crate::index::{ModelIndex, StandardIndex};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::CadIr;

pub(super) fn check_values(ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    for attribute in &ir.model.attributes {
        for value in &attribute.values {
            match value {
                AttributeValue::SubReference { object, .. } => {
                    if !ids.contains(object, StandardIndex) {
                        super::record_finding(
                            findings,
                            Check::ReferentialIntegrity,
                            Severity::Error,
                            Some(attribute.id.as_str()),
                            format_args!("references missing local attribute object `{object}`"),
                        );
                    }
                }
                AttributeValue::Expression(value) => {
                    for parameter in value.expression.dependencies() {
                        if !ir.model.parameters.iter().any(|held| &held.id == parameter) {
                            super::record_finding(
                                findings,
                                Check::ReferentialIntegrity,
                                Severity::Error,
                                Some(attribute.id.as_str()),
                                format_args!("references missing attribute expression parameter `{parameter}`"),
                            );
                        }
                    }
                }
                AttributeValue::NumericList(AttributeNumericList::Placement(Some(values))) => {
                    for placement in values.iter().flatten() {
                        if placement
                            .quaternion
                            .as_raw()
                            .iter()
                            .all(|value| *value == 0.0)
                        {
                            super::record_finding(
                                findings,
                                Check::GeometricConsistency,
                                Severity::Error,
                                Some(attribute.id.as_str()),
                                format_args!("attribute placement has a zero quaternion"),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
