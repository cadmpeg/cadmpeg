// SPDX-License-Identifier: Apache-2.0
//! Plane identities and the conditions for reusing geometric proofs.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

/// Plane equation identity, with a placement fallback for indeterminate equations.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

pub(super) fn coplanarity_plane_key(
    (point, normal): (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    resolution: f64,
) -> CoplanarityPlaneKey {
    let bits = |value: f64| if value == 0.0 { 0 } else { value.to_bits() };
    let canonical_normal = if [normal.x, normal.y, normal.z]
        .into_iter()
        .find(|component| *component != 0.0)
        .is_some_and(f64::is_sign_negative)
    {
        normal.scale(-1.0)
    } else {
        normal
    };
    let offset = point
        .vector_from(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
        .dot(canonical_normal);
    if point.is_finite() && canonical_normal.is_finite() && !offset.is_nan() {
        CoplanarityPlaneKey::Equation([
            bits(offset),
            bits(canonical_normal.x),
            bits(canonical_normal.y),
            bits(canonical_normal.z),
            bits(resolution),
        ])
    } else {
        coplanarity_placement_key((point, normal), resolution)
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

// This is below the smallest overflow-rounding margin at either finite f64 endpoint.
const MAX_COPLANAR_TANGENTIAL_ORIGIN: f64 = f64::MAX * f64::EPSILON / 16.0;

/// A proof with the geometry needed to check origin-sensitive floating-point distances.
pub(super) struct CoplanarityProof<'ir, T> {
    pub(super) origin: cadmpeg_ir::math::Point3,
    pub(super) geometry: Option<&'ir SolvedCurveGeometry>,
    pub(super) result: T,
}

impl<T> CoplanarityProof<'_, T> {
    pub(super) fn matches(
        &self,
        key: CoplanarityPlaneKey,
        (origin, normal): (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
        resolution: f64,
    ) -> bool {
        if matches!(key, CoplanarityPlaneKey::Placement(_)) || self.origin == origin {
            return true;
        }
        if origin.vector_from(self.origin).dot(normal) != 0.0 {
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
            let fits = |origin| {
                let distance = point.vector_from(origin).dot(normal).abs();
                distance.is_finite() && distance <= resolution
            };
            return fits(self.origin) == fits(origin);
        }
        // For an exact coordinate-axis normal, only its coordinate affects distance.
        // Bound the other origins so subtraction cannot overflow at any finite point.
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

