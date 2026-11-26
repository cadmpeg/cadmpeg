// SPDX-License-Identifier: Apache-2.0
//! Borrowed wire projections for features and their metadata and memberships.

use std::collections::BTreeMap;

use super::{Feature, FeatureContent, FeatureDefinition, FeatureId, SketchFeatureBinding};
use crate::features::evaluation::FeatureResultMembers;
use crate::features::geometry::FeatureEquationCurve;
use crate::features::selections::{DistinctMembers, SelectionReference};
use crate::features::sweeps::CoilPlacement;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::math::{Point3, Vector3};
use crate::sketches::SketchId;
use cadmpeg_core::text::NonBlankString;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

#[derive(Serialize)]
pub(crate) struct FeatureWriteWire<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) product_definition: &'a Option<ProductDefinitionId>,
    pub(crate) id: &'a FeatureId,
    pub(crate) ordinal: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) suppressed: &'a Option<bool>,
    /// Regeneration predecessor of a feature that has no tree parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) regeneration_parent: Option<&'a FeatureId>,
    pub(crate) dependencies: &'a DistinctMembers<FeatureId>,
    pub(crate) source_properties: &'a BTreeMap<NonBlankString, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_tag: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_text: &'a Option<String>,
    pub(crate) source_content: &'a FeatureContent,
    pub(crate) outputs: &'a Vec<BodyId>,
    pub(crate) definition: &'a FeatureDefinition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) native_ref: &'a Option<String>,
}

impl<'a> FeatureWriteWire<'a> {
    pub(crate) fn new(feature: &'a Feature, regeneration_parent: Option<&'a FeatureId>) -> Self {
        Self {
            product_definition: &feature.product_definition,
            id: &feature.id,
            ordinal: feature.ordinal,
            name: &feature.name,
            suppressed: &feature.suppressed,
            regeneration_parent,
            dependencies: &feature.dependencies,
            source_properties: &feature.source_properties,
            source_tag: &feature.source_tag,
            source_text: &feature.source_text,
            source_content: &feature.source_content,
            outputs: feature.evaluation.outputs(),
            definition: feature.evaluation.definition(),
            native_ref: &feature.native_ref,
        }
    }

    fn standalone(feature: &'a Feature) -> Self {
        Self::new(feature, None)
    }
}

impl Feature {
    fn serialize_erased(
        &self,
        serializer: cadmpeg_core::serde_erase::SerAdapter<'_>,
    ) -> Result<(), cadmpeg_core::serde_erase::SerError> {
        FeatureWriteWire::standalone(self).serialize(serializer)
    }
}

impl Serialize for Feature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        cadmpeg_core::serde_erase::serialize_with(serializer, |adapter| {
            self.serialize_erased(adapter)
        })
    }
}

impl Serialize for FeatureResultMembers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Member<'a> {
            kind: &'static str,
            id: &'a NonBlankString,
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for (kind, ids) in [
            ("body", &self.bodies),
            ("face", &self.faces),
            ("edge", &self.edges),
            ("vertex", &self.vertices),
        ] {
            for id in ids {
                sequence.serialize_element(&Member { kind, id })?;
            }
        }
        sequence.end()
    }
}

impl Serialize for FeatureEquationCurve {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            parameter: &'a str,
            x_expression: &'a str,
            y_expression: &'a str,
            z_expression: &'a str,
            start: f64,
            end: f64,
        }
        let [start, end] = self.domain.endpoints();
        Wire {
            parameter: &self.parameter,
            x_expression: &self.x_expression,
            y_expression: &self.y_expression,
            z_expression: &self.z_expression,
            start,
            end,
        }
        .serialize(serializer)
    }
}

impl Serialize for SketchFeatureBinding {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Wire<'a> {
            Unresolved {},
            Planar {
                #[serde(skip_serializing_if = "Option::is_none")]
                sketch: Option<&'a SketchId>,
            },
        }
        match self {
            Self::Unresolved => Wire::Unresolved {},
            Self::Planar(sketch) => Wire::Planar {
                sketch: sketch.as_ref(),
            },
        }
        .serialize(serializer)
    }
}

impl Serialize for CoilPlacement {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Wire<'a> {
            Explicit {
                origin: Point3,
                axis: Vector3,
                radial: Vector3,
            },
            Native {
                native_ref: &'a SelectionReference,
            },
        }
        match self {
            Self::Explicit { frame } => Wire::Explicit {
                origin: frame.origin().get(),
                axis: frame.u_axis().into(),
                radial: frame.v_axis().into(),
            },
            Self::Native { native_ref } => Wire::Native { native_ref },
        }
        .serialize(serializer)
    }
}
