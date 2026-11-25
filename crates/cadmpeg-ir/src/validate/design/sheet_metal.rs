// SPDX-License-Identifier: Apache-2.0
//! Attached sheet-metal operands (§6.6).

use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::names::NameKind;
use crate::design::ops::sheet_metal::SheetMetalKind;
use crate::design::parameter::ParameterValue;
use crate::design::selection::{Anchor, OrderedSelection, PickTarget, Picks};
use crate::design::units::{Quantity, ValueKind};
use crate::report::check::Finding;
use crate::scalar::{NonNegativeLength, PositiveLength};

use super::index::{DesignIndex, StateAt};
use super::records::DesignRecords;

pub(super) fn check(index: &DesignIndex<'_>, findings: &mut Vec<Finding>) {
    for (holder, definition) in index.records.feature_definitions() {
        let FeatureOperation::SheetMetal(sheet) = &definition.operation else {
            continue;
        };
        match &sheet.kind {
            SheetMetalKind::AttachedPlate {
                profile,
                thickness,
                side: _,
                attachment,
            } => {
                selection(holder, profile, findings);
                attachment_selection(holder, attachment, findings);
                length(&index.records, holder, thickness, findings);
            }
            SheetMetalKind::AttachedBend {
                plate,
                attachment,
                inside_radius,
                relief,
                transition_form: _,
                transition_radius,
            } => {
                reference(index, holder, plate.as_str(), true, findings);
                attachment_selection(holder, attachment, findings);
                length(&index.records, holder, inside_radius, findings);
                length(&index.records, holder, &relief.width, findings);
                length(&index.records, holder, &relief.depth, findings);
                length(&index.records, holder, &relief.minimum_remnant, findings);
                length(&index.records, holder, &relief.seam_gap, findings);
                length(&index.records, holder, transition_radius, findings);
            }
            SheetMetalKind::UnchangedCorner { bend } => {
                reference(index, holder, bend.as_str(), false, findings);
            }
            _ => {}
        }
    }
}

fn reference<'a>(
    index: &DesignIndex<'a>,
    holder: &'a str,
    target: &str,
    plate: bool,
    findings: &mut Vec<Finding>,
) {
    let kind = index.records.features.get(target).and_then(|feature| {
        match &feature.definition.operation {
            FeatureOperation::SheetMetal(sheet) => Some(&sheet.kind),
            _ => None,
        }
    });
    let owner = index
        .owner_definition(holder)
        .or_else(|| {
            index
                .records
                .configurations
                .get(holder)
                .map(|configuration| &configuration.scope)
        })
        .or_else(|| {
            index
                .records
                .arrangements
                .get(holder)
                .map(|arrangement| &arrangement.scope)
        });
    let same_scope = owner == index.owner_definition(target);
    let precedes = !index.records.features.contains_key(holder)
        || index
            .state_features(StateAt::Entering(holder))
            .is_some_and(|features| features.contains(target));
    if !same_scope
        || !precedes
        || !matches!(
            (plate, kind),
            (true, Some(SheetMetalKind::AttachedPlate { .. }))
                | (false, Some(SheetMetalKind::AttachedBend { .. }))
        )
        || holder == target
    {
        super::error(
            findings,
            holder,
            crate::rule!(
                "design.attached-sheet-metal-reference-preceding",
                "attached sheet-metal reference {target} requires a preceding {}",
                if plate {
                    "attached plate"
                } else {
                    "attached bend"
                }
            ),
        );
    }
}

fn selection(holder: &str, value: &OrderedSelection, findings: &mut Vec<Finding>) {
    if !matches!(&value.picks, Picks::Named(items) if !items.as_slice().is_empty()) {
        super::error(
            findings,
            holder,
            crate::rule!(
                "design.attached-sheet-metal-selection-resolved",
                "attached sheet-metal selection requires resolved members"
            ),
        );
    }
}

fn attachment_selection(holder: &str, value: &OrderedSelection, findings: &mut Vec<Finding>) {
    selection(holder, value, findings);
    if !matches!(value.at, Anchor::Input) {
        super::error(
            findings,
            holder,
            crate::rule!(
                "design.attached-sheet-metal-attachment-input-state",
                "attached sheet-metal attachment requires the input state"
            ),
        );
    }
    if let Picks::Named(items) = &value.picks {
        for pick in items.as_slice() {
            let edge = match &pick.target {
                PickTarget::Entity(name) => {
                    super::names::derived_kinds(name).contains(&NameKind::Edge)
                }
                PickTarget::Native { .. } => pick.native.is_some(),
                _ => false,
            };
            if !edge {
                super::error(
                    findings,
                    holder,
                    crate::rule!(
                        "design.attached-sheet-metal-attachment-edge-picks",
                        "attached sheet-metal attachment requires edge picks"
                    ),
                );
            }
        }
    }
}

trait LengthBound {
    const POSITIVE: bool;
}
impl LengthBound for PositiveLength {
    const POSITIVE: bool = true;
}
impl LengthBound for NonNegativeLength {
    const POSITIVE: bool = false;
}

fn length<T: LengthBound>(
    index: &DesignRecords<'_>,
    holder: &str,
    value: &Dimension<T>,
    findings: &mut Vec<Finding>,
) {
    let parameter = match value {
        Dimension::Literal(_) => return,
        Dimension::Parameter(id) => id,
        Dimension::Negated(_) => {
            super::error(
                findings,
                holder,
                crate::rule!(
                    "design.attached-sheet-metal-length-unsigned",
                    "attached sheet-metal length requires an unsigned dimension"
                ),
            );
            return;
        }
    };
    let Some(record) = index.parameters.get(parameter.as_str()) else {
        return;
    };
    if record.kind != ValueKind::Quantity(Quantity::LENGTH) {
        super::error(
            findings,
            holder,
            crate::rule!(
                "design.attached-sheet-metal-length-parameter-kind",
                "attached sheet-metal length binds a parameter of another kind"
            ),
        );
    }
    if let Some(value) = &record.value {
        if !matches!(value, ParameterValue::Number(value) if if T::POSITIVE { value.get() > 0.0 } else { value.get() >= 0.0 })
        {
            super::error(
                findings,
                holder,
                crate::rule!(
                    "design.attached-sheet-metal-length-range",
                    "attached sheet-metal length is outside its permitted range"
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
