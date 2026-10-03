// SPDX-License-Identifier: Apache-2.0
//! Borrowed wire projections for feature metadata and memberships.

use super::{
    CoilPlacement, FeatureEquationCurve, FeatureResultMembers, SelectionReference,
    SketchFeatureBinding,
};
use crate::math::{Point3, Vector3};
use crate::sketches::SketchId;
use cadmpeg_core::text::NonBlankString;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

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
        #[serde(tag = "space", rename_all = "snake_case")]
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
