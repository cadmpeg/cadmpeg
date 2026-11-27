// SPDX-License-Identifier: Apache-2.0
//! Positional solid-loft operand admission.

use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::parameters::ParameterValue;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::design::PmDcParameter;
use crate::feature::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::pmdc::{type_id_string, PmDcReference};

use super::{FACE_LOOP_PARTICIPANT_TYPE, LOFT_MODE_TYPE, SECTION_CONDITION_TYPE, SECTION_TYPE};

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
    ResultStateBounded,
    ResultStateFailed,
    ResultJoin,
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
            Self::ResultTopology
            | Self::ResultStateBounded
            | Self::ResultStateFailed
            | Self::ResultJoin => InventorLossCode::FeatureLoftResultOperandsNative,
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
            Self::ResultStateFailed => "history state of the loft does not decode".to_owned(),
            Self::ResultJoin => "joined result body does not merge with a body of the preceding history state".to_owned(),
            Self::ResultStateBounded => "history state of the loft passed the local work cap of state views".to_owned(),
            Self::ResultIdentity => "result body identity does not resolve".to_owned(),
        };
        code.note(format!("Retained Inventor solid loft natively: {reason}."))
    }
}

/// One admitted loft section: its link, the section's own source index, and
/// its `source_value` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdmittedSection {
    /// The loft-section selection record in the section list.
    pub(crate) record: PmDcReference,
    pub(crate) entity_link: PmDcReference,
    pub(crate) source_index: u32,
    pub(crate) source_value: u32,
}

/// The result and operation slots that distinguish the two loft classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// Part operation value 1 or 3; slot 13 holds one result body.
    Solid,
    /// Part operation value 5; slot 6 holds the result surface-body wrapper.
    Surface,
}

impl Form {
    fn result_slot(self) -> usize {
        match self {
            Self::Solid => 13,
            Self::Surface => 6,
        }
    }
}

pub(crate) fn sections<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
) -> Result<([PmDcReference; 2], BooleanOp), NativeReason> {
    let (admitted, op) = admit(source, Form::Solid, property, parameter)?;
    Ok((admitted.map(|section| section.entity_link), op))
}

/// The two sections of a positional surface loft: the section that selects a
/// sketch wire, with `source_value` 1, and the face-loop section, whose
/// `source_value` equals its own content-header source index. Either can be
/// first; `sketch_first` states the stored order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceSections {
    pub(crate) sketch: AdmittedSection,
    pub(crate) face_loop: AdmittedSection,
    pub(crate) sketch_first: bool,
}

pub(crate) fn surface_sections<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
) -> Result<SurfaceSections, NativeReason> {
    let ([first, last], _) = admit(source, Form::Surface, property, parameter)?;
    // A section whose own source index is 1 stores `source_value` 1 in both
    // forms, so the two forms cannot be told apart: the pair is refused.
    if first.source_index == 1 || last.source_index == 1 {
        return Err(NativeReason::SectionRecord);
    }
    let is_loop = |section: &AdmittedSection| section.source_value == section.source_index;
    let is_sketch = |section: &AdmittedSection| section.source_value == 1 && !is_loop(section);
    if is_sketch(&first) && is_loop(&last) {
        Ok(SurfaceSections {
            sketch: first,
            face_loop: last,
            sketch_first: true,
        })
    } else if is_loop(&first) && is_sketch(&last) {
        Ok(SurfaceSections {
            sketch: last,
            face_loop: first,
            sketch_first: false,
        })
    } else {
        Err(NativeReason::SectionRecord)
    }
}

fn admit<'a>(
    source: &PmDcFeature,
    form: Form,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
) -> Result<([AdmittedSection; 2], BooleanOp), NativeReason> {
    let slots = source.properties.references();
    let result_slot = form.result_slot();
    if slots.len() != 14
        || source.properties.marker() != 2
        || slots.iter().enumerate().any(|(slot, reference)| {
            reference.index() != 0 && ![0, 1, 2, 9, 10, result_slot].contains(&slot)
        })
    {
        return Err(NativeReason::Controls);
    }
    let op = match property(slots[1]).ok_or(NativeReason::Slot(1))?.kind {
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PartOperation,
            type_value: 5,
            value,
        } => match (form, value) {
            (Form::Solid, 1) | (Form::Surface, 5) => BooleanOp::NewBody,
            (Form::Solid, 3) => BooleanOp::Join,
            _ => return Err(NativeReason::Slot(1)),
        },
        _ => return Err(NativeReason::Slot(1)),
    };
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
    match form {
        Form::Solid => {
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
        }
        Form::Surface => {
            if !matches!(
                property(slots[6])
                    .ok_or(NativeReason::ResultCollection)?
                    .kind,
                PmDcFeaturePropertyKind::SurfaceBody { .. }
            ) {
                return Err(NativeReason::ResultCollection);
            }
        }
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
    let mut admitted = [AdmittedSection {
        record: PmDcReference::from_packed(0),
        entity_link: PmDcReference::from_packed(0),
        source_index: 0,
        source_value: 0,
    }; 2];
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
            source_value,
            option,
        } = section.kind
        else {
            return Err(NativeReason::SectionRecord);
        };
        // The solid loft stores `source_value` 1 in both sections; this check
        // stays ahead of every later section check.
        if form == Form::Solid && source_value != 1 {
            return Err(NativeReason::SectionRecord);
        }
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
        admitted[ordinal] = AdmittedSection {
            record: [*first, *last][ordinal],
            entity_link,
            source_index: section.header.source_index,
            source_value,
        };
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
    Ok((admitted, op))
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

/// Whether the second participant of a surface-loft label is the face-loop
/// participant record: a record of type `7ca882b3f445b9fffe4a7486104813a4`
/// other than the sketch participant. `type_at` gives the record type string
/// of a record ordinal in the feature's segment.
pub(crate) fn face_loop_participant<'a>(
    label: &crate::feature::PmDcFeatureLabel,
    sketch_ordinal: u32,
    type_at: impl Fn(u32) -> Option<&'a str>,
) -> bool {
    let [first, last] = label.participants.references() else {
        return false;
    };
    let others = [first, last]
        .into_iter()
        .filter_map(|reference| reference.record_ordinal())
        .filter(|ordinal| *ordinal != sketch_ordinal)
        .collect::<Vec<_>>();
    let [other] = others.as_slice() else {
        return false;
    };
    type_at(*other) == Some(type_id_string(FACE_LOOP_PARTICIPANT_TYPE).as_str())
}

/// Whether the surface-body wrapper in slot 6 of `source` is already named by
/// an earlier feature, so that this loft did not create it. `position` gives
/// the history position of a feature; a feature without one cannot be ordered
/// and counts as an earlier user.
pub(crate) fn wrapper_used_earlier(
    source: &PmDcFeature,
    features: &[PmDcFeature],
    position: impl Fn(&PmDcFeature) -> Option<u64>,
) -> bool {
    let Some(wrapper) = source
        .properties
        .references()
        .get(Form::Surface.result_slot())
        .and_then(|reference| reference.record_ordinal())
    else {
        return true;
    };
    let Some(own) = position(source) else {
        return true;
    };
    features
        .iter()
        .filter(|other| {
            other.identity.segment_token == source.identity.segment_token
                && other.identity.record_ordinal != source.identity.record_ordinal
        })
        .filter(|other| {
            other
                .properties
                .references()
                .iter()
                .any(|reference| reference.record_ordinal() == Some(wrapper))
        })
        .any(|other| position(other).is_none_or(|at| at < own))
}

/// Whether `definition` is a surface loft that keeps a native section
/// selection.
pub(crate) fn has_native_section(definition: &FeatureDefinition) -> bool {
    use cadmpeg_ir::features::profiles::{LoftSection, PlanarProfileRef, ProfileRef};
    matches!(definition,
        FeatureDefinition::Operation(FeatureOperation::Loft { sections, solid: false, .. })
            if sections.iter().any(|section| matches!(section,
                LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(_))))))
}

/// Whether `definition` is the neutral form of a positional surface loft.
pub(crate) fn matches_surface_definition(definition: &FeatureDefinition) -> bool {
    matches!(definition,
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            op: cadmpeg_ir::features::operation_settings::BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }) if sections.len() == 2 && guides.is_empty()
    )
}

pub(crate) fn matches_definition(definition: &FeatureDefinition, expected: BooleanOp) -> bool {
    matches!(definition,
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            op,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }) if sections.len() == 2 && guides.is_empty() && *op == expected
    )
}

pub(crate) fn validates<'a>(
    source: &PmDcFeature,
    definition: &FeatureDefinition,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
) -> bool {
    sections(source, property, parameter).is_ok_and(|(_, op)| matches_definition(definition, op))
}

#[cfg(test)]
mod tests;
