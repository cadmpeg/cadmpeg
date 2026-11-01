// SPDX-License-Identifier: Apache-2.0
//! Constant-radius fillet operands and refusal causes.

use super::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::pmdc::PmDcReference;
use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
use cadmpeg_ir::scalar::{Length, PositiveLength};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Slots,
    Law,
    Slot(usize),
    Sets,
    Set,
    Selection,
    Continuity,
    Option,
    Edges,
    EmptyItems,
    Radius,
    Result,
}

impl NativeReason {
    pub(crate) fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let reason = match self {
            Self::Slots => "property vector lacks the constant-radius operands".into(),
            Self::Law => "slot 11 has an unresolved fillet law".into(),
            Self::Slot(slot) => format!("slot {slot} has an auxiliary construction operand"),
            Self::Sets => "slot 0 edge-set collection is unresolved or empty".into(),
            Self::Set => "edge-set constant-radius record is unresolved".into(),
            Self::Selection => "edge-set selection descriptor is unresolved".into(),
            Self::Continuity => "edge-set continuity control is unresolved".into(),
            Self::Option => "edge-set fifth reference has an unresolved control".into(),
            Self::Edges => "edge-set collection or edge-item reference is unresolved".into(),
            Self::EmptyItems => "edge-item index-reference list is empty".into(),
            Self::Radius => "edge-set radius length is unresolved or nonpositive".into(),
            Self::Result => "result-body collection is unresolved".into(),
        };
        crate::loss::InventorLossCode::FeatureFilletOperandsNative
            .note(format!("Retained Inventor fillet natively: {reason}."))
    }
}

pub(crate) fn native_loss(
    reason: NativeReason,
    corner: bool,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (corner || reason != NativeReason::EmptyItems).then(|| reason.note())
}

pub(crate) struct Operand<'a> {
    pub(crate) set: &'a PmDcFeatureProperty,
    pub(crate) collection: &'a PmDcFeatureProperty,
    pub(crate) direct: bool,
    pub(crate) radius: PositiveLength,
}

pub(crate) fn operands<'a>(
    source: &PmDcFeature,
    corner: bool,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(PmDcReference) -> Option<Length>,
) -> Result<Vec<Operand<'a>>, NativeReason> {
    let slots = source.properties.references();
    if slots.len() < 16 {
        return Err(NativeReason::Slots);
    }
    if !matches!(
        property(slots[11]).map(|p| &p.kind),
        Some(PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Fillet,
            type_value: 2,
            value: 0,
        })
    ) {
        return Err(NativeReason::Law);
    }
    for slot in [1, 10] {
        if slots[slot].index() != 0 {
            return Err(NativeReason::Slot(slot));
        }
    }
    let Some(PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::FilletEdgeSets,
        items,
    }) = property(slots[0]).map(|p| &p.kind)
    else {
        return Err(NativeReason::Sets);
    };
    if items.references().is_empty() {
        return Err(NativeReason::Sets);
    }
    let mut operands = Vec::new();
    for reference in items.references() {
        let set = property(*reference).ok_or(NativeReason::Set)?;
        let PmDcFeaturePropertyKind::FilletEdgeSet {
            edges,
            radius,
            selection,
            continuity,
            option,
        } = &set.kind
        else {
            return Err(NativeReason::Set);
        };
        let direct = match property(*selection).map(|p| &p.kind) {
            Some(PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 4,
                value: 0,
            }) => true,
            Some(PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 0x0002_0004,
                value: 0,
            }) => false,
            _ => return Err(NativeReason::Selection),
        };
        let continuity_admitted = (corner && continuity.index() == 0)
            || matches!(
                property(*continuity).map(|p| &p.kind),
                Some(PmDcFeaturePropertyKind::Boolean { value: false, .. })
            );
        if !continuity_admitted {
            return Err(NativeReason::Continuity);
        }
        if let Some(option) = option.filter(|reference| reference.index() != 0) {
            if !matches!(
                property(option).map(|p| &p.kind),
                Some(PmDcFeaturePropertyKind::Boolean { value: false, .. })
            ) {
                return Err(NativeReason::Option);
            }
        }
        let collection = property(*edges).ok_or(NativeReason::Edges)?;
        super::edge_selection::check_items(collection, &property).map_err(
            |reason| match reason {
                super::edge_selection::ItemReason::Empty => NativeReason::EmptyItems,
                super::edge_selection::ItemReason::Unresolved => NativeReason::Edges,
            },
        )?;
        let radius = length(*radius)
            .and_then(|value| PositiveLength::new(value.get()))
            .ok_or(NativeReason::Radius)?;
        operands.push(Operand {
            set,
            collection,
            direct,
            radius,
        });
    }
    Ok(operands)
}

pub(crate) fn matches(expected: &[Operand<'_>], groups: &[FilletGroup]) -> bool {
    expected.len() == groups.len()
        && expected.iter().zip(groups).all(|(expected, actual)| {
            actual.radius
                == RadiusSpec::Constant {
                    radius: cadmpeg_ir::Dimension::Literal(expected.radius),
                }
                && actual.tangency_weight.is_none()
                && super::edge_selection::matches_selection(
                    &actual.edges,
                    &if expected.direct {
                        expected.collection.id()
                    } else {
                        expected.set.id()
                    },
                    expected.direct,
                )
        })
}

#[cfg(test)]
pub(crate) mod tests;
