// SPDX-License-Identifier: Apache-2.0
//! Solid-loft source operands and generated-result validation.

use cadmpeg_core::decode::unique_index;
use cadmpeg_ir::{
    document::Model,
    features::{
        FeatureDefinition, FeatureOperation, LoftSection, ParameterValue, PlanarProfileRef,
        ProfileRef,
    },
};
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
    model: &'a Model,
    raw: HashMap<String, Option<&'a PmDcSketch>>,
    sketches: HashMap<&'a str, Option<&'a cadmpeg_ir::sketches::Sketch>>,
    profiles: ProfileIndex<'a>,
}

impl<'a> ValidationIndex<'a> {
    pub(crate) fn new(
        model: &'a Model,
        raw_sketches: &'a [PmDcSketch],
        kernel_links: &'a [PmDcKernelEntityLink],
        entity_links: &'a [PmDcEntityStyleLink],
        raw_entities: &'a [PmDcSketchEntity],
    ) -> Self {
        Self {
            model,
            raw: unique_index(raw_sketches.iter().map(|sketch| (sketch.id(), sketch))),
            sketches: unique_index(
                model
                    .sketches
                    .iter()
                    .map(|sketch| (sketch.id.as_str(), sketch)),
            ),
            profiles: ProfileIndex::new(
                &[],
                kernel_links,
                entity_links,
                raw_entities,
                &model.sketch_entities,
            ),
        }
    }

    pub(crate) fn matches(
        &self,
        source: &PmDcFeature,
        label: &PmDcFeatureLabel,
        definition: &FeatureDefinition,
        property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
        parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
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
        evidence::loft(
            self.model,
            source.header.source_index,
            [first.0.header.source_index, last.0.header.source_index],
            |section, name| {
                selected.get(section).is_some_and(|(raw, sketch, profile)| {
                    self.profiles.contains_curve_name(
                        token,
                        raw.identity.record_ordinal,
                        sketch,
                        profile,
                        name,
                    )
                })
            },
        )
        .is_some()
    }
}
