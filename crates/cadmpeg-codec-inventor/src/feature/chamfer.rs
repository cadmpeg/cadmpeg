// SPDX-License-Identifier: Apache-2.0
//! Chamfer dimensions, support selections, and refusal causes.

use super::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::pmdc::PmDcReference;
use cadmpeg_ir::features::edge_treatments::{ChamferForm, ChamferSpec};
use cadmpeg_ir::scalar::{Angle, InteriorAngle, Length, PositiveLength};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Slots,
    Form,
    Edges,
    EmptyItems,
    Distance,
    Support,
    Angle,
    SecondDistance,
    Direction,
    AngleReference,
    SideOwnership,
    Result,
}

impl NativeReason {
    pub(crate) fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let reason = match self {
            Self::Slots => "property vector lacks the chamfer operands",
            Self::Form => "slot 4 has an unresolved chamfer form",
            Self::Edges => "slot 0 collection or edge-item reference is unresolved",
            Self::EmptyItems => "edge-item index-reference list is empty",
            Self::Distance => "slot 2 length is unresolved or nonpositive",
            Self::Support => "slot 1 support-face selection is unresolved or empty",
            Self::Angle => {
                "slot 10 angle is unresolved or outside the open interval from zero to pi"
            }
            Self::SecondDistance => "slot 3 length is unresolved or nonpositive",
            Self::Direction => "slot 5 direction control differs from the true Boolean form",
            Self::AngleReference => "distance-angle reference is unresolved",
            Self::SideOwnership => "two-distance support-side ownership is unresolved",
            Self::Result => "result-body collection is unresolved",
        };
        crate::loss::InventorLossCode::FeatureChamferOperandsNative.note(format!(
            "Retained Inventor chamfer operands natively: {reason}."
        ))
    }
}

pub(crate) fn orientation_loss(spec: ChamferSpec) -> Option<cadmpeg_ir::report::loss::LossNote> {
    match spec {
        ChamferSpec::Unresolved {
            form: Some(ChamferForm::DistanceAngle),
        } => Some(NativeReason::AngleReference.note()),
        ChamferSpec::Unresolved {
            form: Some(ChamferForm::TwoDistances),
        } => Some(NativeReason::SideOwnership.note()),
        _ => None,
    }
}

pub(crate) struct Operands<'a> {
    pub(crate) edges: &'a PmDcFeatureProperty,
    pub(crate) spec: ChamferSpec,
    pub(crate) flip_direction: bool,
    pub(crate) direct: bool,
}

pub(crate) fn operands<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(usize) -> Option<Length>,
    angle: impl Fn(usize) -> Option<Angle>,
) -> Result<Operands<'a>, NativeReason> {
    let slots = source.properties.references();
    if slots.len() < 12 {
        return Err(NativeReason::Slots);
    }
    let form = match property(slots[4]).map(|p| &p.kind) {
        Some(PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Chamfer,
            type_value: 2,
            value: form @ 0..=2,
        }) => *form,
        _ => return Err(NativeReason::Form),
    };
    let edges = property(slots[0]).ok_or(NativeReason::Edges)?;
    super::edge_selection::check_items(edges, &property).map_err(|reason| match reason {
        super::edge_selection::ItemReason::Empty => NativeReason::EmptyItems,
        super::edge_selection::ItemReason::Unresolved => NativeReason::Edges,
    })?;
    let distance = length(2)
        .and_then(|value| PositiveLength::new(value.get()))
        .ok_or(NativeReason::Distance)?;
    let spec = if form == 0 {
        ChamferSpec::Distance { distance }
    } else {
        let Some(PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            items,
        }) = property(slots[1]).map(|p| &p.kind)
        else {
            return Err(NativeReason::Support);
        };
        if items.references().is_empty()
            || !items.references().iter().all(|reference| {
                matches!(property(*reference).map(|p| &p.kind),
                    Some(PmDcFeaturePropertyKind::FaceItem { index_references })
                        if !index_references.values().is_empty())
            })
        {
            return Err(NativeReason::Support);
        }
        if form == 1 {
            angle(10)
                .and_then(|value| InteriorAngle::try_from(value).ok())
                .ok_or(NativeReason::Angle)?;
            ChamferSpec::Unresolved {
                form: Some(ChamferForm::DistanceAngle),
            }
        } else {
            let second = length(3)
                .and_then(|value| PositiveLength::new(value.get()))
                .ok_or(NativeReason::SecondDistance)?;
            if distance == second {
                ChamferSpec::TwoDistances {
                    first: distance,
                    second,
                }
            } else {
                ChamferSpec::Unresolved {
                    form: Some(ChamferForm::TwoDistances),
                }
            }
        }
    };
    let Some(PmDcFeaturePropertyKind::Boolean {
        value: flip_direction,
        ..
    }) = property(slots[5]).map(|p| &p.kind)
    else {
        return Err(NativeReason::Direction);
    };
    if !*flip_direction {
        return Err(NativeReason::Direction);
    }
    Ok(Operands {
        edges,
        spec,
        flip_direction: *flip_direction,
        direct: form == 0,
    })
}

pub(crate) fn matches(
    expected: &Operands<'_>,
    source: &PmDcFeature,
    groups: &[cadmpeg_ir::features::edge_treatments::ChamferGroup],
    flip_direction: bool,
) -> bool {
    let [actual] = groups else {
        return false;
    };
    actual.spec == expected.spec
        && flip_direction == expected.flip_direction
        && super::edge_selection::matches_selection(
            &actual.edges,
            &if expected.direct {
                expected.edges.id()
            } else {
                source.id()
            },
            expected.direct,
        )
}

pub(crate) fn explicit_selection_controls<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    let value = |slot: usize| {
        let reference = *source.properties.references().get(slot)?;
        if !reference.qualified() {
            return None;
        }
        let PmDcFeaturePropertyKind::Boolean { value, .. } = &property(reference)?.kind else {
            return None;
        };
        Some(*value)
    };
    let [Some(first), Some(last)] = [value(6), value(9)] else {
        return false;
    };
    [[true, false], [true, true]].contains(&[first, last])
}

#[cfg(test)]
pub(crate) mod tests;
