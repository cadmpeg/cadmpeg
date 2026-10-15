// SPDX-License-Identifier: Apache-2.0
//! Classify source sketch owners from reached point and curve geometry.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::ids::native_stream_charged;
use crate::records::sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity, SketchPoint};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::HashSet;

const EPS_SKETCH_CURVE_SPATIAL: f64 = 1.0e-9;
const EPS_SPATIAL_OWNER_DEPTH: f64 = 1.0e-9;

fn sketch_curve_is_spatial(
    ctx: &DecodeContext<'_>,
    curve: &SketchCurveIdentity,
) -> Result<bool, CodecError> {
    Ok(match curve.geometry.as_ref() {
        Some(SketchCurveGeometry::Line { start, end, .. }) => {
            !(planar_point(start.as_raw()) && planar_point(end.as_raw()))
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            ..
        }) => {
            !(planar_point(center.as_raw())
                && reference_direction.as_raw().z.abs() <= EPS_SKETCH_CURVE_SPATIAL
                && sketch_normal_sign(normal.as_raw()).is_some())
        }
        Some(SketchCurveGeometry::Nurbs { geometry, .. }) => {
            let poles = geometry.poles();
            poles.point_count() != 0
                && ctx.any_by(
                    poles.points(),
                    |point| Ok(!planar_point(point)),
                    "classify F3D sketch NURBS poles",
                )?
        }
        None => false,
    })
}

pub(super) fn spatial_geometry_owners<'a>(
    ctx: &DecodeContext<'_>,
    points: &'a [SketchPoint],
    curves: &'a [SketchCurveIdentity],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let mut owners = HashSet::new();
    if !curves.is_empty() {
        let mut input = curves.iter();
        while let Some(curve) = ctx.next_charged(&mut input, "visit F3D spatial owner curve")? {
            let Some(owner) = curve.owner_reference else {
                continue;
            };
            if !sketch_curve_is_spatial(ctx, curve)? {
                continue;
            }
            let Some(scope) =
                native_stream_charged(ctx, &curve.id, "resolve F3D spatial curve owner stream")?
            else {
                continue;
            };
            ctx.insert_hash_set(&mut owners, (scope, owner), "f3d spatial geometry owner")?;
        }
    }
    if !points.is_empty() {
        let mut input = points.iter();
        while let Some(point) = ctx.next_charged(&mut input, "visit F3D spatial owner point")? {
            let Some(owner) = point.owner_reference else {
                continue;
            };
            if point.depth().abs().partial_cmp(&EPS_SPATIAL_OWNER_DEPTH)
                != Some(std::cmp::Ordering::Greater)
            {
                continue;
            }
            let Some(scope) =
                native_stream_charged(ctx, &point.id, "resolve F3D spatial point owner stream")?
            else {
                continue;
            };
            ctx.insert_hash_set(&mut owners, (scope, owner), "f3d spatial geometry owner")?;
        }
    }
    Ok(owners)
}

#[cfg(test)]
mod tests;
