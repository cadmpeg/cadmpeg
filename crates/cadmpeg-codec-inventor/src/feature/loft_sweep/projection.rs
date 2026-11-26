// SPDX-License-Identifier: Apache-2.0
//! Loft and circular sweep construction from typed operands and owned result faces.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::features::evaluation::FeatureResultTopology;
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{Feature, FeatureContent, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::sketches::Sketch;

use super::super::{
    body_result, boolean_properties, feature_result, resolve_property, FeatureFamily, PmDcFeature,
    PmDcFeatureLabel, ProjectionIndex,
};
use cadmpeg_ir::features::profiles::LoftSection;
use cadmpeg_ir::features::LoftGuidance;

pub(super) mod evidence;

use super::controls::{self, NativeReason};

pub(in crate::feature) fn project<'a>(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    family: FeatureFamily,
    surface: controls::SurfaceContext<impl Fn(u32) -> Option<&'a str>>,
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
        FeatureFamily::Loft => match loft(ctx, source, label, index) {
            Ok(operation) => operation,
            Err(Refusal::Native(reason)) => {
                losses.push(reason.note());
                return None;
            }
            Err(Refusal::Codec(error)) => return Some(Err(error)),
        },
        FeatureFamily::SurfaceLoft if surface.wrapper_used_earlier => return None,
        FeatureFamily::SurfaceLoft => match surface_loft(source, label, index, surface.type_at) {
            Ok(operation) => operation,
            Err(_) => return None,
        },
        _ => return None,
    };
    let result = if matches!(family, FeatureFamily::SurfaceLoft) {
        body_result(ctx, source, family.output_slot(source), index)
    } else {
        feature_result(ctx, source, family.output_slot(source), index)
    };
    let (id, result) = match result {
        Some(Ok(value)) => value,
        Some(Err(error)) => return Some(Err(error)),
        None if matches!(family, FeatureFamily::SurfaceLoft) => return None,
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
    if matches!(family, FeatureFamily::SurfaceLoft) {
        losses.push(crate::loss::InventorLossCode::FeatureSurfaceLoftOperationAssumed.note(
            "Surface loft reads part-operation value 5 as a new-body creation; the creation-or-combination rule is open.",
        ));
        losses.push(crate::loss::InventorLossCode::FeatureSurfaceLoftFaceLoopNative.note(
            "Surface loft retains its face-loop section selection; the selected loop has no resolved neutral profile.",
        ));
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
                    crate::feature::history_states::LOFT_TAG
                }
                .into(),
            ),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(operation),
            ),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

/// Why a loft is not typed.
enum Refusal {
    /// The loft stays native.
    Native(NativeReason),
    /// The decode fails.
    Codec(CodecError),
}

impl From<NativeReason> for Refusal {
    fn from(reason: NativeReason) -> Self {
        Self::Native(reason)
    }
}

/// The numeric parameter record and solved value that `reference` selects.
fn section_parameter<'a>(
    token: &str,
    reference: crate::pmdc::PmDcReference,
    index: &'a ProjectionIndex<'a>,
) -> Option<(
    &'a crate::design::PmDcParameter,
    &'a cadmpeg_ir::features::parameters::ParameterValue,
)> {
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
}

/// A positional surface loft: one sketch-wire section and one face-loop
/// section. The sketch section selects the whole single closed profile of its
/// consumed sketch, so that a validator recomputes it from the sketch alone.
/// The face-loop section stays a native reference to its section record. The
/// second label participant has the face-loop participant type, and the two
/// sections select different wires.
fn surface_loft<'a>(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    type_at: impl Fn(u32) -> Option<&'a str>,
) -> Result<FeatureOperation, NativeReason> {
    let token = source.identity.segment_token.as_str();
    let admitted = controls::surface_sections(
        source,
        |reference| resolve_property(token, reference.index(), index),
        |reference| section_parameter(token, reference, index),
    )?;
    let sketches = participant_sketches(source, label, index);
    let [(ordinal, sketch)] = sketches.as_slice() else {
        return Err(NativeReason::Profile);
    };
    if !controls::face_loop_participant(label, *ordinal, type_at) {
        return Err(NativeReason::Profile);
    }
    let sketch_link = admitted
        .sketch
        .entity_link
        .record_ordinal()
        .ok_or(NativeReason::Profile)?;
    let loop_link = admitted
        .face_loop
        .entity_link
        .record_ordinal()
        .ok_or(NativeReason::Profile)?;
    if sketch_link == loop_link {
        return Err(NativeReason::Profile);
    }
    let profile = index
        .profiles
        .regions(token, *ordinal, sketch, &[sketch_link])
        .ok_or(NativeReason::Profile)?;
    if index.profiles.whole_sketch(token, *ordinal, sketch) != Some(profile.clone()) {
        return Err(NativeReason::Profile);
    }
    index
        .profiles
        .numbered_edge_count(token, loop_link)
        .ok_or(NativeReason::Profile)?;
    let section = resolve_property(token, admitted.face_loop.record.index(), index)
        .ok_or(NativeReason::SectionRecord)?;
    let sketch_section = LoftSection::Profile(ProfileRef::Planar(profile));
    let loop_section =
        LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::native(section.id())));
    let sections = if admitted.sketch_first {
        vec![sketch_section, loop_section]
    } else {
        vec![loop_section, sketch_section]
    };
    Ok(FeatureOperation::Loft {
        sections,
        guidance: LoftGuidance::default(),
        op: BooleanOp::NewBody,
        closed: false,
        solid: false,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    })
}

fn loft(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Result<FeatureOperation, Refusal> {
    let token = source.identity.segment_token.as_str();
    let links = controls::sections(
        source,
        |reference| resolve_property(token, reference.index(), index),
        |reference| section_parameter(token, reference, index),
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
            return Err(NativeReason::Profile.into());
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
    let section_sources: [u32; 2] = section_sources
        .try_into()
        .map_err(|_| NativeReason::ResultTopology)?;
    evidence::loft_proof(
        ctx,
        index.result_model,
        index.result_states,
        source.header.source_index,
        section_sources,
        |section| {
            selected
                .get(section)
                .map_or_else(Vec::new, |(ordinal, sketch, profile)| {
                    index.profiles.curve_names(token, *ordinal, sketch, profile)
                })
        },
    )
    .map_err(|failure| match failure {
        evidence::Unproved::Result => Refusal::Native(NativeReason::ResultTopology),
        evidence::Unproved::StateFailed => Refusal::Native(NativeReason::ResultStateFailed),
        evidence::Unproved::Bounded => Refusal::Native(NativeReason::ResultStateBounded),
        evidence::Unproved::Codec(error) => Refusal::Codec(error),
    })?;
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
