// SPDX-License-Identifier: Apache-2.0
//! Solid-loft source operands and generated-result validation.

use cadmpeg_core::decode::unique_index;
use cadmpeg_ir::features::parameters::ParameterValue;
use cadmpeg_ir::features::profiles::{LoftSection, PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::CadIr;
use std::collections::HashMap;

use super::{controls, projection::evidence};
use crate::{
    design::PmDcParameter,
    feature::{
        profile::ProfileIndex, PmDcEntityStyleLink, PmDcFeature, PmDcFeatureLabel,
        PmDcFeatureProperty, PmDcKernelEntityLink,
    },
    pmdc::PmDcReference,
    sketch::{PmDcSketch, PmDcSketchEntity},
};

pub(crate) struct ValidationIndex<'a> {
    ir: &'a CadIr,
    raw: HashMap<String, Option<&'a PmDcSketch>>,
    sketches: HashMap<&'a str, Option<&'a cadmpeg_ir::sketches::Sketch>>,
    profiles: ProfileIndex<'a>,
}

impl<'a> ValidationIndex<'a> {
    pub(crate) fn new(
        ir: &'a CadIr,
        raw_sketches: &'a [PmDcSketch],
        kernel_links: &'a [PmDcKernelEntityLink],
        entity_links: &'a [PmDcEntityStyleLink],
        raw_entities: &'a [PmDcSketchEntity],
    ) -> Self {
        Self {
            ir,
            raw: unique_index(raw_sketches.iter().map(|sketch| (sketch.id(), sketch))),
            sketches: unique_index(
                ir.model
                    .sketches
                    .iter()
                    .map(|sketch| (sketch.id.as_str(), sketch)),
            ),
            profiles: ProfileIndex::new(
                &[],
                kernel_links,
                entity_links,
                raw_entities,
                &ir.model.sketch_entities,
            ),
        }
    }

    /// Whether a neutral surface loft is the positional form of its source
    /// operands: the sketch section is the whole closed profile of the sketch
    /// that the label consumes, the face-loop section is the native reference
    /// to its section record, the second label participant has the face-loop
    /// participant type, and the two sections select different wires. The
    /// sections stand in their stored order.
    pub(crate) fn surface_matches<'t>(
        &self,
        source: &PmDcFeature,
        label: &PmDcFeatureLabel,
        definition: &FeatureDefinition,
        property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
        parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
        type_at: impl Fn(u32) -> Option<&'t str>,
    ) -> bool {
        if !controls::matches_surface_definition(definition) {
            return false;
        }
        let Ok(admitted) = controls::surface_sections(source, &property, parameter) else {
            return false;
        };
        let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) = definition
        else {
            return false;
        };
        let (sketch, face_loop) = if admitted.sketch_first {
            (&sections[0], &sections[1])
        } else {
            (&sections[1], &sections[0])
        };
        let Some(section) = property(admitted.face_loop.record) else {
            return false;
        };
        if !matches!(
            face_loop,
            LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(id)))
                if *id == section.id()
        ) || admitted.sketch.entity_link.record_ordinal()
            == admitted.face_loop.entity_link.record_ordinal()
        {
            return false;
        }
        let LoftSection::Profile(ProfileRef::Planar(
            profile @ PlanarProfileRef::SketchRegions { sketch, .. },
        )) = sketch
        else {
            return false;
        };
        let Some(sketch) = self.sketches.get(sketch.as_str()).copied().flatten() else {
            return false;
        };
        let Some(raw) = sketch
            .native_ref
            .as_deref()
            .and_then(|id| self.raw.get(id))
            .and_then(Option::as_ref)
        else {
            return false;
        };
        let token = source.identity.segment_token.as_str();
        raw.identity.segment_token.as_str() == token
            && label
                .participants
                .references()
                .iter()
                .any(|r| r.record_ordinal() == Some(raw.identity.record_ordinal))
            && controls::face_loop_participant(label, raw.identity.record_ordinal, type_at)
            && self
                .profiles
                .whole_sketch(token, raw.identity.record_ordinal, sketch)
                .as_ref()
                == Some(profile)
    }

    pub(crate) fn matches(
        &self,
        source: &PmDcFeature,
        label: &PmDcFeatureLabel,
        definition: &FeatureDefinition,
        property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
        parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
        history_bound: bool,
    ) -> bool {
        if !controls::validates(source, definition, property, parameter) {
            return false;
        }
        let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) = definition
        else {
            return false;
        };
        let token = source.identity.segment_token.as_str();
        let mut selected = Vec::new();
        for section in sections {
            let LoftSection::Profile(ProfileRef::Planar(
                profile @ PlanarProfileRef::SketchRegions { sketch, .. },
            )) = section
            else {
                return false;
            };
            let Some(sketch) = self.sketches.get(sketch.as_str()).copied().flatten() else {
                return false;
            };
            let Some(raw) = sketch
                .native_ref
                .as_deref()
                .and_then(|id| self.raw.get(id))
                .and_then(Option::as_ref)
            else {
                return false;
            };
            if raw.identity.segment_token.as_str() != token
                || !label
                    .participants
                    .references()
                    .iter()
                    .any(|r| r.record_ordinal() == Some(raw.identity.record_ordinal))
            {
                return false;
            }
            selected.push((*raw, sketch, profile));
        }
        let [first, last] = selected.as_slice() else {
            return false;
        };
        let sections = [first.0.header.source_index, last.0.header.source_index];
        let curves = |section: usize| {
            selected
                .get(section)
                .map_or_else(Vec::new, |(raw, sketch, profile)| {
                    self.profiles
                        .curve_names(token, raw.identity.record_ordinal, sketch, profile)
                })
        };
        evidence::loft_final_proof(
            self.ir,
            source.header.source_index,
            sections,
            curves,
            history_bound,
        )
        .is_some()
    }
}

#[cfg(test)]
mod tests;
