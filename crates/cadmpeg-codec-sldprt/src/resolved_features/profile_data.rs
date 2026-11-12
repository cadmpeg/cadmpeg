// SPDX-License-Identifier: Apache-2.0
//! Curve data behind planar feature references.

use std::collections::HashSet;

use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::sketches::SketchGeometryDefinition;

pub(crate) struct FeatureProfileData<'a> {
    features_with_curves: HashSet<&'a FeatureId>,
}

impl<'a> FeatureProfileData<'a> {
    pub(crate) fn new(model: &'a Model) -> Self {
        let sketches_with_curves = model
            .sketch_entities
            .iter()
            .filter(|entity| {
                !entity.construction
                    && matches!(
                        entity.geometry.definition(),
                        SketchGeometryDefinition::Line { .. }
                            | SketchGeometryDefinition::Circle { .. }
                            | SketchGeometryDefinition::Arc { .. }
                            | SketchGeometryDefinition::Ellipse { .. }
                            | SketchGeometryDefinition::Hyperbola { .. }
                            | SketchGeometryDefinition::Parabola { .. }
                            | SketchGeometryDefinition::Nurbs { .. }
                    )
            })
            .map(|entity| &entity.sketch)
            .collect::<HashSet<_>>();
        let sketches = model
            .sketches
            .iter()
            .map(|sketch| &sketch.id)
            .collect::<HashSet<_>>();
        let features_with_curves = model
            .features
            .iter()
            .filter_map(|feature| {
                let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) =
                    feature.evaluation.definition()
                else {
                    return None;
                };
                let sketch = sketch.id()?;
                (sketches.contains(sketch) && sketches_with_curves.contains(sketch))
                    .then_some(&feature.id)
            })
            .collect();
        Self {
            features_with_curves,
        }
    }

    pub(crate) fn has_curves(&self, feature: &FeatureId) -> bool {
        self.features_with_curves.contains(feature)
    }

    pub(crate) fn missing_extrudes<'b>(
        &'b self,
        model: &'b Model,
    ) -> impl Iterator<Item = &'b Feature> + 'b {
        model.features.iter().filter(|feature| {
            matches!(feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    profile: ProfileRef::Planar(PlanarProfileRef::Feature(owner)), ..
                }) if !self.has_curves(owner))
        })
    }
}
