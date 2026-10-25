// SPDX-License-Identifier: Apache-2.0
//! Loft and circular sweep construction from typed operands and owned result faces.

use std::collections::HashSet;

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    features::{
        BooleanOp, DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureOperation,
        FeatureResultTopology, ProfileRef,
    },
    sketches::Sketch,
};

use super::super::{
    angle_parameter, boolean, boolean_properties, enum16, feature_result, references,
    resolve_property, slot_property, FeatureFamily, PmDcFeature, PmDcFeatureEnumFamily,
    PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
    ProjectionIndex,
};
use super::{
    LOFT_MODE_TYPE, SECTION_CONDITION_TYPE, SWEEP_MODE_TYPE, SWEEP_ORIENTATION_TYPE,
    SWEEP_SCALING_TYPE,
};
use crate::pmdc::type_id_string;
use cadmpeg_ir::features::{
    LoftGuidance, LoftSection, SolidSweepOperation, SweepSection, SweepShape,
};

mod evidence;

pub(in crate::feature) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    family: FeatureFamily,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if references(
        source,
        family.output_slot(),
        PmDcFeatureReferenceFamily::ObjectCollection,
        index,
    )?
    .references()
    .len()
        != 1
    {
        return None;
    }
    let operation = match family {
        FeatureFamily::Sweep => sweep(source, label, index)?,
        FeatureFamily::Loft => loft(source, label, index)?,
        _ => return None,
    };
    let (id, result) = match feature_result(ctx, source, family.output_slot(), index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ctx.charge_entities(1, "project Inventor loft or sweep") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
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

fn sweep(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<FeatureOperation> {
    if !only_slots(source, &[0, 1, 2, 3, 4, 7, 8, 9, 10, 15])
        || enum16(source, 2, PmDcFeatureEnumFamily::PartOperation, index)? != 1
        || enum16(source, 4, PmDcFeatureEnumFamily::Extent, index)? != 3
        || !zero_control(slot_property(source, 8, index)?, SWEEP_MODE_TYPE, 3)
        || !zero_control(slot_property(source, 9, index)?, SWEEP_ORIENTATION_TYPE, 2)
        || !zero_control(slot_property(source, 10, index)?, SWEEP_SCALING_TYPE, 2)
        || angle_parameter(source, 3, index)?.get() != 0.0
    {
        return None;
    }
    let token = source.identity.segment_token.as_str();
    let boundary = references(source, 0, PmDcFeatureReferenceFamily::BoundaryPatch, index)?;
    let [selection] = boundary.references() else {
        return None;
    };
    let section = resolve_property(token, selection.index(), index)?;
    let PmDcFeaturePropertyKind::ProfileSelection {
        entity_link,
        value: 0,
    } = section.kind
    else {
        return None;
    };
    let sketches = participant_sketches(source, label, index);
    let mut circles = sketches.iter().filter_map(|(ordinal, sketch)| {
        index
            .profiles
            .circle(token, *ordinal, sketch, entity_link.record_ordinal()?)
    });
    let (profile, radius, name) = circles.next()?;
    if circles.next().is_some() {
        return None;
    }
    let path_property = slot_property(source, 1, index)?;
    let PmDcFeaturePropertyKind::ProfileSelection {
        entity_link,
        value: 0,
    } = path_property.kind
    else {
        return None;
    };
    let path = index.profiles.selected_path(
        token,
        entity_link.record_ordinal()?,
        path_property.id(),
        &sketches,
    )?;
    evidence::sweep(
        index.result_model?,
        source.header.source_index,
        radius,
        name,
        &path.names,
        path.endpoints,
    )?;
    // The selected wire is the complete primary path. No further tangent
    // expansion is requested. Keep its stored face partitions without cleanup.
    Some(FeatureOperation::Sweep {
        shape: SweepShape::Solid {
            op: SolidSweepOperation::NewBody,
            section: SweepSection::Profile(profile),
            sections: Vec::new(),
        },
        path: Some(path.reference),
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    })
}

fn loft(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<FeatureOperation> {
    if !only_slots(source, &[0, 1, 2, 9, 10, 13])
        || enum16(source, 1, PmDcFeatureEnumFamily::PartOperation, index)? != 1
        || boolean(source, 2, index)?
        || boolean(source, 9, index)?
        || !zero_control(slot_property(source, 10, index)?, LOFT_MODE_TYPE, 2)
    {
        return None;
    }
    let list = references(source, 0, PmDcFeatureReferenceFamily::LoftSections, index)?;
    if list.references().len() != 2 {
        return None;
    }
    let token = source.identity.segment_token.as_str();
    let sketches = participant_sketches(source, label, index);
    let mut sections = Vec::new();
    let mut section_sources = Vec::new();
    let mut distinct = HashSet::new();
    for reference in list.references() {
        if !distinct.insert(reference.index()) {
            return None;
        }
        let section = resolve_property(token, reference.index(), index)?;
        let PmDcFeaturePropertyKind::LoftSection {
            entity_link,
            value: 0,
            condition,
            map_value: 0,
            source_value: 1,
            option,
            ..
        } = section.kind
        else {
            return None;
        };
        if !zero_control(
            resolve_property(token, condition.index(), index)?,
            SECTION_CONDITION_TYPE,
            2,
        ) || (option.index() != 0
            && !matches!(
                resolve_property(token, option.index(), index)?.kind,
                PmDcFeaturePropertyKind::Boolean { value: false, .. }
            ))
        {
            return None;
        }
        let profiles = sketches
            .iter()
            .filter_map(|(ordinal, sketch)| {
                Some((
                    *ordinal,
                    index.profiles.regions(
                        token,
                        *ordinal,
                        sketch,
                        &[entity_link.record_ordinal()?],
                    )?,
                ))
            })
            .collect::<Vec<_>>();
        let [(ordinal, profile)] = profiles.as_slice() else {
            return None;
        };
        section_sources.push(
            index
                .sketches
                .get(&(token, *ordinal))?
                .as_ref()?
                .header
                .source_index,
        );
        let profile = profile.clone();
        sections.push(LoftSection::Profile(ProfileRef::Planar(profile)));
    }
    evidence::loft(
        index.result_model?,
        source.header.source_index,
        section_sources.try_into().ok()?,
    )?;
    Some(FeatureOperation::Loft {
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

fn participant_sketches<'a>(
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

fn only_slots(source: &PmDcFeature, slots: &[usize]) -> bool {
    source
        .properties
        .references()
        .iter()
        .enumerate()
        .all(|(slot, reference)| reference.index() == 0 || slots.contains(&slot))
}

fn zero_control(property: &PmDcFeatureProperty, type_id: [u8; 16], expected_type: i16) -> bool {
    property.identity.type_id.as_str() == type_id_string(type_id)
        && matches!(property.kind, PmDcFeaturePropertyKind::Enumeration { family: PmDcFeatureEnumFamily::Auxiliary, type_value, value: 0 } if type_value == expected_type)
}

#[cfg(test)]
mod tests;
