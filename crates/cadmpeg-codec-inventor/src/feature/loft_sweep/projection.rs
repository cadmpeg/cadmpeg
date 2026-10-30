// SPDX-License-Identifier: Apache-2.0
//! Loft and circular sweep construction from typed operands and owned result faces.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    features::{
        BooleanOp, DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureOperation,
        FeatureResultTopology, ProfileRef,
    },
    sketches::Sketch,
};

use super::super::{
    boolean_properties, feature_result, resolve_property, FeatureFamily, PmDcFeature,
    PmDcFeatureLabel, ProjectionIndex,
};
use cadmpeg_ir::features::{LoftGuidance, LoftSection};

pub(super) mod evidence;

use super::controls::{self, NativeReason};

pub(in crate::feature) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    family: FeatureFamily,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let operation = match family {
        FeatureFamily::Sweep => match super::sweep::project(source, label, index) {
            Ok(operation) => operation,
            Err(reason) => {
                losses.push(reason.note());
                return None;
            }
        },
        FeatureFamily::Loft => match loft(source, label, index) {
            Ok(operation) => operation,
            Err(reason) => {
                losses.push(reason.note());
                return None;
            }
        },
        _ => return None,
    };
    let (id, result) = match feature_result(ctx, source, family.output_slot(source), index) {
        Some(Ok(value)) => value,
        Some(Err(error)) => return Some(Err(error)),
        None => {
            losses.push(if matches!(family, FeatureFamily::Sweep) {
                super::sweep::NativeReason::ResultIdentity.note()
            } else {
                NativeReason::ResultIdentity.note()
            });
            return None;
        }
    };
    if let Err(error) = ctx.charge_entities(1, "project Inventor loft or sweep") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            product_definition: None,
            id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: boolean_properties(source, &[2, 9], index),
            source_tag: Some(
                if matches!(family, FeatureFamily::Sweep) {
                    "sweep"
                } else {
                    "loft"
                }
                .into(),
            ),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(operation),
            ),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

fn loft(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Result<FeatureOperation, NativeReason> {
    let token = source.identity.segment_token.as_str();
    let links = controls::sections(
        source,
        |reference| resolve_property(token, reference.index(), index),
        |reference| {
            let parameter = index
                .parameters
                .get(&(token, reference.record_ordinal()?))?
                .as_ref()?;
            let value = index
                .parameter_values
                .get(parameter.id().as_str())?
                .as_ref()?
                .value
                .as_ref()?;
            Some((*parameter, value))
        },
    )?;
    let sketches = participant_sketches(source, label, index);
    let mut selected = Vec::new();
    let mut section_sources = Vec::new();
    for entity_link in links {
        let profiles = sketches
            .iter()
            .filter_map(|(ordinal, sketch)| {
                Some((
                    *ordinal,
                    *sketch,
                    index.profiles.regions(
                        token,
                        *ordinal,
                        sketch,
                        &[entity_link.record_ordinal()?],
                    )?,
                ))
            })
            .collect::<Vec<_>>();
        let [(ordinal, sketch, profile)] = profiles.as_slice() else {
            return Err(NativeReason::Profile);
        };
        section_sources.push(
            index
                .sketches
                .get(&(token, *ordinal))
                .and_then(Option::as_ref)
                .ok_or(NativeReason::Profile)?
                .header
                .source_index,
        );
        selected.push((*ordinal, *sketch, profile.clone()));
    }
    evidence::loft(
        index.result_model.ok_or(NativeReason::ResultTopology)?,
        source.header.source_index,
        section_sources
            .try_into()
            .map_err(|_| NativeReason::ResultTopology)?,
        |section, name| {
            selected
                .get(section)
                .is_some_and(|(ordinal, sketch, profile)| {
                    index
                        .profiles
                        .contains_curve_name(token, *ordinal, sketch, profile, name)
                })
        },
    )
    .ok_or(NativeReason::ResultTopology)?;
    let sections = selected
        .into_iter()
        .map(|entry| LoftSection::Profile(ProfileRef::Planar(entry.2)))
        .collect();

    Ok(FeatureOperation::Loft {
        sections,
        guidance: LoftGuidance::default(),
        op: BooleanOp::NewBody,
        closed: false,
        solid: true,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    })
}

pub(super) fn participant_sketches<'a>(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &'a ProjectionIndex<'a>,
) -> Vec<(u32, &'a Sketch)> {
    label
        .participants
        .references()
        .iter()
        .filter_map(|reference| {
            let ordinal = reference.record_ordinal()?;
            let raw = index
                .sketches
                .get(&(source.identity.segment_token.as_str(), ordinal))?
                .as_ref()?;
            let id = index.sketch_ids.get(raw.id().as_str())?;
            Some((ordinal, *index.sketches_by_id.get(id.as_str())?))
        })
        .collect()
}

pub(super) fn zero_control(
    property: &crate::feature::PmDcFeatureProperty,
    type_id: [u8; 16],
    expected_type: i16,
) -> bool {
    property.identity.type_id.as_str() == crate::pmdc::type_id_string(type_id)
        && matches!(property.kind, crate::feature::PmDcFeaturePropertyKind::Enumeration { family: crate::feature::PmDcFeatureEnumFamily::Auxiliary, type_value, value: 0 } if type_value == expected_type)
}

#[cfg(test)]
mod tests;
