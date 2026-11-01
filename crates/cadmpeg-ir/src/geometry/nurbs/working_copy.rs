// SPDX-License-Identifier: Apache-2.0
//! Working copies preserve admitted knot and pole invariants by construction.

use super::{KnotVector, NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface};
use crate::geometry::working_copy::lane;
use cadmpeg_core::{decode::DecodeContext, CodecError};

fn grid<T: Copy>(ctx: &DecodeContext<'_>, rows: &[Vec<T>]) -> Result<Vec<Vec<T>>, CodecError> {
    let mut target = Vec::new();
    ctx.reserve_working(&mut target, rows.len(), "copy admitted NURBS grid")?;
    for row in rows {
        target.push(lane(ctx, row)?);
    }
    Ok(target)
}

impl KnotVector {
    /// Copy admitted knots without revalidating or changing their order.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self(lane(ctx, &self.0)?))
    }
}

impl NurbsCurve {
    /// Copy admitted geometry through fallible caller working storage.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let poles = match &self.poles {
            NurbsPoles3::Polynomial { points } => NurbsPoles3::Polynomial {
                points: lane(ctx, points)?,
            },
            NurbsPoles3::Rational { points } => NurbsPoles3::Rational {
                points: lane(ctx, points)?,
            },
        };
        Ok(Self {
            degree: self.degree,
            knots: self.knots.try_clone_working(ctx)?,
            poles,
            periodic: self.periodic,
        })
    }
}

impl NurbsSurface {
    /// Copy admitted geometry through fallible caller working storage.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let poles = match &self.poles {
            NurbsPoleGrid::Polynomial { rows } => NurbsPoleGrid::Polynomial {
                rows: grid(ctx, rows)?,
            },
            NurbsPoleGrid::Rational { rows } => NurbsPoleGrid::Rational {
                rows: grid(ctx, rows)?,
            },
        };
        Ok(Self {
            u_degree: self.u_degree,
            v_degree: self.v_degree,
            u_knots: self.u_knots.try_clone_working(ctx)?,
            v_knots: self.v_knots.try_clone_working(ctx)?,
            poles,
            normal_reversed: self.normal_reversed,
            u_periodic: self.u_periodic,
            v_periodic: self.v_periodic,
            cache_fit_tolerance: self.cache_fit_tolerance,
        })
    }
}
