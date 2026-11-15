// SPDX-License-Identifier: Apache-2.0
//! Wire projections for feature metadata, memberships, and sweep sections.

use super::{
    CoilPlacement, FeatureEquationCurve, FeatureResultMembers, PlanarProfileRef,
    SelectionReference, SketchFeatureBinding, SweepCircularRegion, SweepSection,
};
use crate::math::{Point3, Vector3};
use crate::scalar::PositiveLength;
use crate::sketches::SketchId;
use cadmpeg_core::text::NonBlankString;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepCircularRegionWire {
    outer_radius: PositiveLength,
    #[serde(default, deserialize_with = "deserialize_wall_thickness")]
    wall_thickness: Option<PositiveLength>,
}

cadmpeg_core::named_optional_field!(deserialize_wall_thickness, PositiveLength, "wall_thickness");

impl<'de> Deserialize<'de> for SweepCircularRegion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SweepCircularRegionWire::deserialize(deserializer)?;
        Self::new(wire.outer_radius, wire.wall_thickness).map_err(serde::de::Error::custom)
    }
}

impl<G: Serialize> Serialize for SweepSection<G> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", content = "value", rename_all = "snake_case")]
        enum SectionWire<'a, G> {
            #[serde(rename = "unresolved")]
            Unnamed,
            Unresolved(&'a String),
            Profile(&'a PlanarProfileRef),
            Generated(&'a G),
        }
        match self {
            Self::Unresolved(None) => SectionWire::Unnamed,
            Self::Unresolved(Some(native)) => SectionWire::Unresolved(native),
            Self::Profile(profile) => SectionWire::Profile(profile),
            Self::Generated(generated) => SectionWire::Generated(generated),
        }
        .serialize(serializer)
    }
}
