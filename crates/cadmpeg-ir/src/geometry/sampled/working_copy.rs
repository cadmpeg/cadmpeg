// SPDX-License-Identifier: Apache-2.0
//! Working copies preserve admitted sample and index relationships.

use super::{PolygonalSurface, PolylineCurve, PolylineSamples};
use crate::geometry::working_copy::lane;
use cadmpeg_core::{decode::DecodeContext, CodecError};

impl PolygonalSurface {
    /// Copy admitted vertices and indices without recomputing a field.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            vertices: lane(ctx, &self.vertices)?,
            triangles: lane(ctx, &self.triangles)?,
            chordal_deflection: self.chordal_deflection,
        })
    }
}

impl PolylineCurve {
    /// Copy admitted sample rows without reconstructing their parameters.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let samples = match &self.samples {
            PolylineSamples::Unparameterized { points } => PolylineSamples::Unparameterized {
                points: points.try_clone_working(ctx)?,
            },
            PolylineSamples::Parameterized { vertices } => PolylineSamples::Parameterized {
                vertices: vertices.try_clone_working(ctx)?,
            },
        };
        Ok(Self {
            samples,
            chordal_deflection: self.chordal_deflection,
        })
    }
}
