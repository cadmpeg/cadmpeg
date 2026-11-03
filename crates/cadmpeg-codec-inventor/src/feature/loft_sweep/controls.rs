// SPDX-License-Identifier: Apache-2.0
//! Positional solid-loft operand admission.

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, ParameterValue};

use crate::design::PmDcParameter;
use crate::feature::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::pmdc::{type_id_string, PmDcReference};

use super::{LOFT_MODE_TYPE, SECTION_CONDITION_TYPE, SECTION_TYPE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Controls,
    Slot(usize),
    ResultCollection,
    SectionList,
    SectionRecord,
    SectionCondition,
    SectionOption,
    SectionParameters,
    Profile,
    ResultTopology,
    ResultIdentity,
}

impl NativeReason {
    pub(crate) fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        use crate::loss::InventorLossCode;
        let code = match self {
            Self::SectionParameters => InventorLossCode::FeatureLoftSectionScalarsNative,
            Self::Controls
            | Self::Slot(_)
            | Self::SectionList
            | Self::SectionRecord
            | Self::SectionCondition
            | Self::SectionOption => InventorLossCode::FeatureLoftOperandsNative,
            Self::ResultTopology => InventorLossCode::FeatureLoftResultOperandsNative,
            Self::ResultCollection | Self::Profile | Self::ResultIdentity => {
                InventorLossCode::FeatureLoftControlsUnresolved
            }
        };
        let reason = match self {
            Self::Controls => "construction controls differ from the positional two-section form".to_owned(),
            Self::Slot(slot) => format!("property slot {slot} differs from the positional operand pattern"),
            Self::ResultCollection => "result collection does not contain one body".to_owned(),
            Self::SectionList => "ordered section list does not contain two distinct sections".to_owned(),
            Self::SectionRecord => "section selection has unresolved scalar or link controls".to_owned(),
            Self::SectionCondition => "section condition is not the positional form".to_owned(),
            Self::SectionOption => "section option is not null or false".to_owned(),
            Self::SectionParameters => "section scalar operands differ from zero dimensionless and right-angle nominal and model values".to_owned(),
            Self::Profile => "section selection does not resolve to one consumed planar profile".to_owned(),
            Self::ResultTopology => "complete generator-owned result does not prove ordered positional loft construction".to_owned(),
            Self::ResultIdentity => "result body identity does not resolve".to_owned(),
        };
        code.note(format!("Retained Inventor solid loft natively: {reason}."))
    }
}

pub(crate) fn sections<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
) -> Result<[PmDcReference; 2], NativeReason> {
    let slots = source.properties.references();
    if slots.len() != 14
        || source.properties.marker() != 2
        || slots.iter().enumerate().any(|(slot, reference)| {
            reference.index() != 0 && ![0, 1, 2, 9, 10, 13].contains(&slot)
        })
    {
        return Err(NativeReason::Controls);
    }
    if !matches!(
        property(slots[1]).ok_or(NativeReason::Slot(1))?.kind,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PartOperation,
            type_value: 5,
            value: 1,
        }
    ) {
        return Err(NativeReason::Slot(1));
    }
    for slot in [2, 9] {
        if !matches!(
            property(slots[slot]).ok_or(NativeReason::Slot(slot))?.kind,
            PmDcFeaturePropertyKind::Boolean { value: false, .. }
        ) {
            return Err(NativeReason::Slot(slot));
        }
    }
    if !zero_control(
        property(slots[10]).ok_or(NativeReason::Slot(10))?,
        LOFT_MODE_TYPE,
    ) {
        return Err(NativeReason::Slot(10));
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items: result,
    } = &property(slots[13])
        .ok_or(NativeReason::ResultCollection)?
        .kind
    else {
        return Err(NativeReason::ResultCollection);
    };
    if result.references().len() != 1 {
        return Err(NativeReason::ResultCollection);
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::LoftSections,
        items,
    } = &property(slots[0]).ok_or(NativeReason::SectionList)?.kind
    else {
        return Err(NativeReason::SectionList);
    };
    let [first, last] = items.references() else {
        return Err(NativeReason::SectionList);
    };
    if first.index() == last.index() {
        return Err(NativeReason::SectionList);
    }
    let sections = [
        property(*first).ok_or(NativeReason::SectionRecord)?,
        property(*last).ok_or(NativeReason::SectionRecord)?,
    ];
    let mut links = [PmDcReference::from_packed(0); 2];
    for (ordinal, section) in sections.into_iter().enumerate() {
        if section.identity.type_id.as_str() != type_id_string(SECTION_TYPE) {
            return Err(NativeReason::SectionRecord);
        }
        let PmDcFeaturePropertyKind::LoftSection {
            entity_link,
            value: 0,
            condition,
            parameters,
            map_value: 0,
            source_value: 1,
            option,
        } = section.kind
        else {
            return Err(NativeReason::SectionRecord);
        };
        if entity_link.index() == 0 {
            return Err(NativeReason::SectionRecord);
        }
        if !zero_control(
            property(condition).ok_or(NativeReason::SectionCondition)?,
            SECTION_CONDITION_TYPE,
        ) {
            return Err(NativeReason::SectionCondition);
        }
        if option.index() != 0
            && !matches!(
                property(option).ok_or(NativeReason::SectionOption)?.kind,
                PmDcFeaturePropertyKind::Boolean { value: false, .. }
            )
        {
            return Err(NativeReason::SectionOption);
        }
        links[ordinal] = entity_link;
        let (first, first_value) =
            parameter(parameters[0]).ok_or(NativeReason::SectionParameters)?;
        let (second, second_value) =
            parameter(parameters[1]).ok_or(NativeReason::SectionParameters)?;
        if first.nominal_value.get() != 0.0
            || first.model_value.get() != 0.0
            || !matches!(first_value, ParameterValue::Real(value) if value.get() == 0.0)
            || second.nominal_value.get() != std::f64::consts::FRAC_PI_2
            || second.model_value.get() != std::f64::consts::FRAC_PI_2
            || !matches!(second_value, ParameterValue::Angle(value) if value.get() == std::f64::consts::FRAC_PI_2)
        {
            return Err(NativeReason::SectionParameters);
        }
    }
    Ok(links)
}

fn zero_control(property: &PmDcFeatureProperty, type_id: [u8; 16]) -> bool {
    property.identity.type_id.as_str() == type_id_string(type_id)
        && matches!(
            property.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: 2,
                value: 0,
            }
        )
}

pub(crate) fn matches_definition(definition: &FeatureDefinition) -> bool {
    matches!(definition,
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }) if sections.len() == 2 && guides.is_empty()
    )
}

pub(crate) fn validates<'a>(
    source: &PmDcFeature,
    definition: &FeatureDefinition,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
) -> bool {
    matches_definition(definition) && sections(source, property, parameter).is_ok()
}

#[cfg(test)]
mod tests;
