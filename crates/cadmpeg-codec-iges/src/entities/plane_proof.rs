// SPDX-License-Identifier: Apache-2.0
//! Plane identities and the conditions for reusing geometric proofs.

use super::geometry;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

/// Plane equation identity, with a placement fallback for indeterminate equations.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum CoplanarityPlaneKey {
    Equation([u64; 5]),
    Placement([u64; 7]),
}

impl cadmpeg_core::decode::cost::DecodeCost for CoplanarityPlaneKey {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let coefficients = match self {
            Self::Equation(values) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(values, ctx, operation)?
            }
            Self::Placement(values) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(values, ctx, operation)?
            }
        };
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<u64>()) + coefficients)
    }
}

pub(super) fn canonical_plane_normal(
    normal: cadmpeg_ir::math::Vector3,
) -> Option<cadmpeg_ir::math::Vector3> {
    let reverse = [normal.x, normal.y, normal.z]
        .into_iter()
        .find(|component| *component != 0.0)
        .is_some_and(f64::is_sign_negative);
    let normal = normal.unit()?;
    Some(if reverse { normal.scale(-1.0) } else { normal })
}

fn plane_key_bits(value: f64) -> u64 {
    if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}

pub(super) fn coplanarity_plane_key(
    (point, normal): (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    resolution: f64,
) -> CoplanarityPlaneKey {
    let raw_normal = normal;
    let Some(normal) = canonical_plane_normal(normal) else {
        return coplanarity_placement_key((point, normal), resolution);
    };
    let offset = point
        .vector_from(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
        .dot(normal);
    if point.is_finite() && !offset.is_nan() && resolution.is_finite() && resolution > 0.0 {
        let angle = geometry::CURVE_PLANE_NORMAL_EPSILON;
        CoplanarityPlaneKey::Equation([
            plane_key_bits((offset / resolution).floor()),
            plane_key_bits((normal.x / angle).floor()),
            plane_key_bits((normal.y / angle).floor()),
            plane_key_bits((normal.z / angle).floor()),
            plane_key_bits(resolution),
        ])
    } else {
        coplanarity_placement_key((point, raw_normal), resolution)
    }
}

pub(super) fn coplanarity_placement_key(
    (point, normal): (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    resolution: f64,
) -> CoplanarityPlaneKey {
    CoplanarityPlaneKey::Placement([
        point.x.to_bits(),
        point.y.to_bits(),
        point.z.to_bits(),
        normal.x.to_bits(),
        normal.y.to_bits(),
        normal.z.to_bits(),
        resolution.to_bits(),
    ])
}

pub(super) fn coplanarity_neighbors(
    key: CoplanarityPlaneKey,
) -> impl Iterator<Item = CoplanarityPlaneKey> {
    // Each coefficient can differ by one bin while remaining within tolerance.
    (0..81).filter_map(move |mut position| {
        let CoplanarityPlaneKey::Equation(mut values) = key else {
            return None;
        };
        for value in &mut values[..4] {
            let bin = f64::from_bits(*value);
            let step = f64::from(position % 3) - 1.0;
            position /= 3;
            if !bin.is_finite() && step != 0.0 {
                return None;
            }
            *value = plane_key_bits(bin + step);
        }
        let neighbor = CoplanarityPlaneKey::Equation(values);
        (neighbor != key).then_some(neighbor)
    })
}

// This is below the smallest overflow-rounding margin at either finite f64 endpoint.
const MAX_COPLANAR_TANGENTIAL_ORIGIN: f64 = f64::MAX * f64::EPSILON / 16.0;
const COPLANAR_ROUNDING_BOUND: f64 = 64.0 * f64::EPSILON;

#[derive(Clone, Copy, Default)]
pub(super) struct CoplanarityEnvelope {
    pub(super) coordinate: f64,
    pub(super) distance: f64,
    pub(super) angular_residual: f64,
}

impl CoplanarityEnvelope {
    pub(super) fn record_point(&mut self, point: cadmpeg_ir::math::Point3, distance: f64) {
        self.coordinate = self
            .coordinate
            .max(point.x.abs())
            .max(point.y.abs())
            .max(point.z.abs());
        self.distance = self.distance.max(distance);
    }

    pub(super) fn matches(
        self,
        previous: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
        current: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
        resolution: f64,
    ) -> bool {
        let (Some(old), Some(new)) = (
            canonical_plane_normal(previous.1),
            canonical_plane_normal(current.1),
        ) else {
            return false;
        };
        let delta =
            cadmpeg_ir::math::Vector3::new(new.x - old.x, new.y - old.y, new.z - old.z).norm();
        if self.angular_residual + delta + COPLANAR_ROUNDING_BOUND
            > geometry::CURVE_PLANE_NORMAL_EPSILON
        {
            return false;
        }
        let magnitude =
            |point: cadmpeg_ir::math::Point3| point.x.abs().max(point.y.abs()).max(point.z.abs());
        let radius = 3.0_f64.sqrt() * (self.coordinate + magnitude(previous.0));
        let rounding = COPLANAR_ROUNDING_BOUND
            * (self.coordinate + magnitude(previous.0) + magnitude(current.0));
        let shift = current.0.vector_from(previous.0).dot(new).abs();
        let bound = self.distance / previous.1.norm() + radius * delta + shift + rounding;
        bound.is_finite() && bound <= resolution / current.1.norm()
    }
}

/// A proof and the bounds needed to reuse it for an equivalent plane placement.
pub(super) struct CoplanarityProof<'ir, T> {
    pub(super) origin: cadmpeg_ir::math::Point3,
    pub(super) normal: cadmpeg_ir::math::Vector3,
    pub(super) geometry: Option<&'ir SolvedCurveGeometry>,
    pub(super) envelope: Option<CoplanarityEnvelope>,
    pub(super) result: T,
}

impl<T> CoplanarityProof<'_, T> {
    pub(super) fn matches(
        &self,
        key: CoplanarityPlaneKey,
        (origin, normal): (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
        resolution: f64,
    ) -> bool {
        if matches!(key, CoplanarityPlaneKey::Placement(_))
            || (self.origin == origin
                && (self.normal == normal || self.normal == normal.scale(-1.0)))
        {
            return true;
        }
        if self.envelope.is_some_and(|envelope| {
            envelope.matches((self.origin, self.normal), (origin, normal), resolution)
        }) {
            return true;
        }
        if canonical_plane_normal(self.normal) != canonical_plane_normal(normal) {
            return false;
        }
        let Some(geometry) = self.geometry else {
            return true;
        };
        let point = match geometry {
            SolvedCurveGeometry::Line(curve) => Some(curve.origin().get()),
            SolvedCurveGeometry::Circle(curve) => Some(curve.center().get()),
            SolvedCurveGeometry::Ellipse(curve) => Some(curve.center().get()),
            SolvedCurveGeometry::Parabola(curve) => Some(curve.vertex().get()),
            SolvedCurveGeometry::Hyperbola(curve) => Some(curve.center().get()),
            SolvedCurveGeometry::Degenerate(curve) => Some(curve.point().get()),
            SolvedCurveGeometry::Unknown { .. } => return true,
            SolvedCurveGeometry::Composite { .. }
            | SolvedCurveGeometry::Nurbs(_)
            | SolvedCurveGeometry::Polyline(_)
            | SolvedCurveGeometry::Transformed(_) => None,
        };
        if let Some(point) = point {
            let fits = |origin, normal| {
                let distance = point.vector_from(origin).dot(normal).abs();
                distance.is_finite() && distance <= resolution
            };
            return fits(self.origin, self.normal) == fits(origin, normal);
        }
        if self.normal != normal && self.normal != normal.scale(-1.0) {
            return false;
        }
        if origin.vector_from(self.origin).dot(normal) != 0.0 {
            return false;
        }
        // Axis-aligned distances depend on one coordinate. Bound the other
        // origins so subtraction cannot overflow for a finite control point.
        let components = [normal.x, normal.y, normal.z];
        let Some(axis) = components
            .iter()
            .position(|component| component.abs() == 1.0)
        else {
            return false;
        };
        let previous = [self.origin.x, self.origin.y, self.origin.z];
        let current = [origin.x, origin.y, origin.z];
        previous[axis] == current[axis]
            && [0, 1, 2].into_iter().all(|coordinate| {
                coordinate == axis
                    || (components[coordinate] == 0.0
                        && previous[coordinate].abs() <= MAX_COPLANAR_TANGENTIAL_ORIGIN
                        && current[coordinate].abs() <= MAX_COPLANAR_TANGENTIAL_ORIGIN)
            })
    }
}

/// Exact key presence skips B-tree lookups for uncached neighboring planes.
/// Small populations use fixed storage, so they add no heap allocation peak.
pub(super) enum PlaneProofKeys {
    Inline {
        values: [(u32, CoplanarityPlaneKey); 2],
        len: usize,
    },
    Indexed(std::collections::HashSet<(u32, CoplanarityPlaneKey)>),
}

impl Default for PlaneProofKeys {
    fn default() -> Self {
        Self::Inline {
            values: [(0, CoplanarityPlaneKey::Equation([0; 5])); 2],
            len: 0,
        }
    }
}

impl PlaneProofKeys {
    pub(super) fn contains(&self, key: (u32, CoplanarityPlaneKey)) -> bool {
        match self {
            Self::Inline { values, len } => values[..*len].contains(&key),
            Self::Indexed(values) => values.contains(&key),
        }
    }

    /// Adds an absent key after admitting each new table slot and its growth.
    pub(super) fn insert(
        &mut self,
        key: (u32, CoplanarityPlaneKey),
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        match self {
            Self::Inline { values, len } => {
                if *len < values.len() {
                    values[*len] = key;
                    *len += 1;
                } else {
                    let mut indexed = std::collections::HashSet::new();
                    for value in values.iter() {
                        ctx.reserve_set(&mut indexed, 1, operation)?;
                        indexed.extend(std::iter::once(*value));
                    }
                    ctx.reserve_set(&mut indexed, 1, operation)?;
                    indexed.extend(std::iter::once(key));
                    *self = Self::Indexed(indexed);
                }
            }
            Self::Indexed(values) => {
                ctx.reserve_set(values, 1, operation)?;
                values.extend(std::iter::once(key));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_proof_key_presence_distinguishes_planes_and_sequences() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_policy_context(&[], &policy, |ctx| {
            let mut keys = PlaneProofKeys::default();
            let key = |position| {
                if position % 2 == 0 {
                    CoplanarityPlaneKey::Equation([position, 0, 0, 0, 0])
                } else {
                    CoplanarityPlaneKey::Placement([position, 0, 0, 0, 0, 0, 0])
                }
            };
            assert!(!keys.contains((1, key(0))));
            for position in 0..64 {
                let key_bytes = cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<u32>() + 6 * std::mem::size_of::<u64>(),
                );
                let _probe = (position == 2).then(|| {
                    cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                        cadmpeg_core::decode::ResourceDimension::WorkUnits,
                        "test plane proof keys",
                        Some(key_bytes),
                    )
                });
                keys.insert((1, key(position)), ctx, "test plane proof keys")
                    .expect("proof key admission");
                for previous in 0..=position {
                    assert!(keys.contains((1, key(previous))));
                    assert!(!keys.contains((2, key(previous))));
                }
                assert!(!keys.contains((1, key(position + 1))));
            }
        });
    }
}
