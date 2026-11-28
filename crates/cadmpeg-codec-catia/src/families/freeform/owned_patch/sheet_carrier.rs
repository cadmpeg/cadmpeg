// SPDX-License-Identifier: Apache-2.0
//! Carrier charts selected by the owned face and loop records.

use std::borrow::Cow;

use cadmpeg_core::decode::View;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::geometry::nurbs::{KnotVector, NurbsSurface};
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

use crate::families::a5a8::records::A5FreeformCurve;
use crate::families::b2::records::{
    b2_plane_carriers_from_records, b2_plane_geometry, b2_sphere_geometry, b2_torus_geometry,
    cylinder_range_origin, B2Cone, B2Sphere, B2Torus,
};
use crate::layout::{
    sheet_plane_explicit_v_a0_payload as explicit_v_a0,
    sheet_plane_implicit_v_payload as implicit_v_a0,
};
use crate::wire::bytes::{allocation_reference, AllocationReference};
use crate::wire::records::ConsolidatedRecord;

use super::super::rolling_ball_patch::fit_rectangular_patch;
use super::ruled::Ruled;
use super::{near, surface_fit, SheetSurfaceFit};

// resource-cap: MAX_ROLLING_EXTENSION_MM = N
const MAX_ROLLING_EXTENSION_MM: f64 = 0.005;

pub(super) const SPHERE_BRIDGE_RADIUS_TOLERANCE_MM: f64 = 1.0e-6;

pub(super) enum Carrier<'a> {
    RollingBall(&'a ProceduralSurfaceDefinition, f64),
    OwnedRollingBall {
        definition: Box<Cow<'a, ProceduralSurfaceDefinition>>,
        chart_end: f64,
        native_v_upper: Option<f64>,
        variable_radius: bool,
        max_rail_shift_mm: f64,
    },
    Cylinder(SpatialCylinder),
    Cone(B2Cone, bool),
    Torus(B2Torus, bool),
    Sphere(B2Sphere),
    Plane(SpatialPlane),
    Tensor(&'a NurbsSurface),
    CachedTensor(&'a NurbsSurface),
    Extrusion(Box<NurbsSurface>),
    OffsetTensor(&'a NurbsSurface, [[f64; 2]; 2], f64),
    Ruled(Ruled),
}

impl Carrier<'_> {
    pub(super) fn rolling_ball_at_radius(
        jet: A5FreeformCurve,
        radius: f64,
        chart_end: f64,
    ) -> Option<Self> {
        let native_v_upper = jet.extended_v_end;
        let (definition, max_rail_shift_mm) = jet.definition_at_radius(radius)?;
        Some(Self::OwnedRollingBall {
            definition: Box::new(Cow::Owned(definition)),
            chart_end,
            native_v_upper,
            variable_radius: false,
            max_rail_shift_mm,
        })
    }

    pub(super) fn extended_rolling_ball(jet: &A5FreeformCurve) -> Option<Self> {
        let upper = jet.extended_v_end?;
        let first = &jet.sites.first()?.site;
        let radius = first.limit1.distance(first.center.get());
        let chart_end = jet.chart_v_end;
        Self::rolling_ball_at_radius(jet.clone(), radius, chart_end).or_else(|| {
            Some(Self::OwnedRollingBall {
                definition: Box::new(Cow::Owned(jet.definition()?)),
                chart_end,
                native_v_upper: Some(upper),
                variable_radius: true,
                max_rail_shift_mm: 0.0,
            })
        })
    }

    pub(super) fn untransferred_plane_bounds(&self) -> Option<[[f64; 2]; 2]> {
        match self {
            Self::Plane(plane) => plane.bounds,
            _ => None,
        }
    }

    pub(super) fn bounds(&self) -> Option<[[f64; 2]; 2]> {
        match self {
            Self::RollingBall(..) | Self::OwnedRollingBall { .. } => {
                let (ProceduralSurfaceDefinition::RollingBallJet(jet), radius) =
                    self.rolling_definition()?
                else {
                    return None;
                };
                let upper = match self {
                    Self::OwnedRollingBall {
                        native_v_upper: Some(upper),
                        ..
                    } => *upper,
                    _ => radius,
                };
                Some([
                    [
                        jet.stations().first()?.knot.get(),
                        jet.stations().last()?.knot.get(),
                    ],
                    [0.0, upper],
                ])
            }
            Self::Cylinder(cylinder) => Some(cylinder.bounds),
            Self::Cone(cone, _) => Some([
                cone.angular_domain
                    .endpoints()
                    .map(|u| u * cone.angular_scale.get()),
                [
                    -cone.reference_radius.get() / cone.half_angle.get().cos(),
                    f64::INFINITY,
                ],
            ]),
            Self::Torus(torus, _) => Some([
                torus
                    .major_angular_domain
                    .endpoints()
                    .map(|u| u * torus.major_scale.get()),
                torus
                    .minor_angular_domain
                    .endpoints()
                    .map(|v| v * torus.minor_scale.get()),
            ]),
            Self::Plane(_) => Some([[f64::NEG_INFINITY, f64::INFINITY]; 2]),
            Self::Sphere(sphere) => Some([
                sphere
                    .azimuth_range
                    .endpoints()
                    .map(|u| u * sphere.radius.get()),
                sphere
                    .latitude_range
                    .endpoints()
                    .map(|v| v * sphere.radius.get()),
            ]),
            Self::OffsetTensor(_, bounds, _) => Some(*bounds),
            Self::Ruled(ruled) => Some(ruled.bounds),
            Self::Tensor(surface) | Self::CachedTensor(surface) => tensor_bounds(surface),
            Self::Extrusion(surface) => tensor_bounds(surface),
        }
    }

    fn rolling_definition(&self) -> Option<(&ProceduralSurfaceDefinition, f64)> {
        match self {
            Self::RollingBall(definition, end) => Some((definition, *end)),
            Self::OwnedRollingBall {
                definition,
                chart_end,
                ..
            } => Some((definition, *chart_end)),
            _ => None,
        }
    }

    pub(super) fn scale(&self) -> [f64; 2] {
        match self {
            Self::RollingBall(_, chart_end) | Self::OwnedRollingBall { chart_end, .. } => {
                [1.0, chart_end.recip()]
            }
            Self::Cylinder(cylinder) => [cylinder.angle_scale, 1.0],
            Self::Cone(cone, _) => [
                cone.angular_scale.get().recip(),
                cone.half_angle.get().sin(),
            ],
            Self::Torus(torus, _) => [
                torus.major_scale.get().recip(),
                torus.minor_scale.get().recip(),
            ],
            Self::Sphere(sphere) => [sphere.radius.get().recip(); 2],
            Self::Plane(_)
            | Self::Tensor(_)
            | Self::CachedTensor(..)
            | Self::OffsetTensor(..)
            | Self::Extrusion(_)
            | Self::Ruled(_) => [1.0, 1.0],
        }
    }

    pub(super) fn surface_fit(&self) -> Option<SheetSurfaceFit> {
        match self {
            Self::RollingBall(..) => Some(SheetSurfaceFit::RollingBall {
                max_rail_shift_mm: 0.0,
            }),
            Self::OwnedRollingBall {
                max_rail_shift_mm, ..
            } => Some(SheetSurfaceFit::RollingBall {
                max_rail_shift_mm: *max_rail_shift_mm,
            }),
            Self::OffsetTensor(..) => Some(SheetSurfaceFit::Offset),
            Self::Ruled(..) => Some(SheetSurfaceFit::Ruled),
            Self::Extrusion(..) => Some(SheetSurfaceFit::Extrusion),
            _ => None,
        }
    }

    pub(super) fn roster_sense_inferred(&self) -> bool {
        !matches!(self, Self::RollingBall(..) | Self::OwnedRollingBall { .. })
    }

    pub(super) fn reversed(&self) -> bool {
        matches!(self, Self::Cone(_, true) | Self::Torus(_, true))
    }

    pub(super) fn point(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        let [su, sv] = self.scale();
        let u = if self.reversed() { -u } else { u };
        let origin_v = match self {
            Self::Cone(cone, _) => cone.slant_range.lower(),
            _ => 0.0,
        };
        [u * su, (v - origin_v) * sv]
    }

    pub(super) fn evaluate(&self, [u, v]: [f64; 2]) -> Option<Point3> {
        let admission = EvaluationAdmission::Standard;
        match self {
            Self::RollingBall(..) | Self::OwnedRollingBall { .. } => {
                let (definition, _) = self.rolling_definition()?;
                let end = self.extended_rolling_span().unwrap_or(1.0);
                let point = if matches!(
                    self,
                    Self::OwnedRollingBall {
                        variable_radius: true,
                        ..
                    }
                ) {
                    eval::variable_rolling_ball_jet_point_on_span(definition, u, v, end)
                } else {
                    eval::rolling_ball_jet_point_on_span(definition, u, v, end)
                };
                point.ok().map(FinitePoint3::get)
            }
            Self::Ruled(ruled) => ruled.point(u, v),
            Self::OffsetTensor(surface, _, distance) => {
                surface_fit::offset_point(surface, *distance, u, v)
            }
            Self::Extrusion(surface) => eval::decode::nurbs_surface_point(admission, surface, u, v)
                .ok()
                .map(FinitePoint3::get),
            Self::CachedTensor(surface) | Self::Tensor(surface) => {
                eval::decode::nurbs_surface_point(admission, surface, u, v)
                    .ok()
                    .map(FinitePoint3::get)
            }
            _ => {
                let geometry = self.solved([[u, u], [v, v]], false)?;
                eval::decode::surface_point_solved(admission, &geometry, u, v)
                    .ok()
                    .map(FinitePoint3::get)
            }
        }
    }

    fn extended_rolling_span(&self) -> Option<f64> {
        match self {
            Self::OwnedRollingBall {
                native_v_upper: Some(upper),
                chart_end,
                ..
            } => Some(upper * chart_end.recip()),
            _ => None,
        }
    }

    pub(super) fn rolling_boundary_gap(&self, u: f64, v: f64) -> Option<f64> {
        let (definition, _) = self.rolling_definition()?;
        if let Some(end) = self.extended_rolling_span() {
            return (0.0..=end).contains(&v).then_some(0.0);
        }
        let [u, s] = self.point([u, v]);
        eval::rolling_ball_jet_boundary_gap(definition, u, s).ok()
    }

    pub(super) fn evaluate_rolling_extension(&self, u: f64, v: f64) -> Option<Point3> {
        let (definition, _) = self.rolling_definition()?;
        if self.extended_rolling_span().is_some() {
            return self.evaluate([u, v]);
        }
        eval::rolling_ball_jet_extended_point(definition, u, v, MAX_ROLLING_EXTENSION_MM)
            .ok()
            .map(FinitePoint3::get)
    }

    pub(super) fn solve_rolling_extension(
        &self,
        limits: [[f64; 2]; 2],
    ) -> Option<SolvedSurfaceGeometry> {
        let (ProceduralSurfaceDefinition::RollingBallJet(jet), _) = self.rolling_definition()?
        else {
            return None;
        };
        let surface = surface_fit::bounded(
            jet.stations().iter().map(|s| s.knot.get()),
            limits,
            |u, v| self.evaluate_rolling_extension(u, v),
        )?;
        Some(SolvedSurfaceGeometry::Nurbs(surface))
    }

    pub(super) fn solved(
        &self,
        limits: [[f64; 2]; 2],
        refine: bool,
    ) -> Option<SolvedSurfaceGeometry> {
        match self {
            Self::RollingBall(..) | Self::OwnedRollingBall { .. } => {
                let (definition, _) = self.rolling_definition()?;
                if self.extended_rolling_span().is_some() {
                    return self.solve_rolling_extension(limits);
                }
                let adaptive = || {
                    surface_fit::rolling(
                        definition,
                        [limits[0], limits[1].map(|v| v.clamp(0.0, 1.0))],
                    )
                };
                let rectangular =
                    || fit_rectangular_patch(definition, limits[0]).map(|fit| fit.surface);
                let surface = if refine {
                    adaptive().or_else(rectangular)?
                } else {
                    rectangular().or_else(adaptive)?
                };
                Some(SolvedSurfaceGeometry::Nurbs(surface))
            }
            Self::Cylinder(cylinder) => Some(SolvedSurfaceGeometry::Cylinder(cylinder.surface)),
            Self::Cone(cone, _) => Some(SolvedSurfaceGeometry::Cone(cone.surface)),
            Self::Torus(torus, _) => match b2_torus_geometry(torus) {
                SurfaceGeometry::Solved(geometry) => Some(geometry),
                SurfaceGeometry::Procedural { .. } => None,
            },
            Self::Sphere(sphere) => match b2_sphere_geometry(sphere) {
                SurfaceGeometry::Solved(geometry) => Some(geometry),
                SurfaceGeometry::Procedural { .. } => None,
            },
            Self::Plane(plane) => Some(SolvedSurfaceGeometry::Plane(plane.surface)),
            Self::Tensor(surface) => Some(SolvedSurfaceGeometry::Nurbs((*surface).clone())),
            Self::CachedTensor(surface) => Some(SolvedSurfaceGeometry::Nurbs((*surface).clone())),
            Self::OffsetTensor(surface, _, distance) => Some(SolvedSurfaceGeometry::Nurbs(
                surface_fit::offset(surface, *distance, limits)?,
            )),
            Self::Ruled(ruled) => Some(SolvedSurfaceGeometry::Nurbs(ruled.fit(limits)?)),
            Self::Extrusion(surface) => Some(SolvedSurfaceGeometry::Nurbs((**surface).clone())),
        }
    }
}

fn tensor_bounds(surface: &NurbsSurface) -> Option<[[f64; 2]; 2]> {
    let domain = |knots: &KnotVector, degree: u32, count: usize| {
        Some([
            *knots.get(usize::try_from(degree).ok()?)?,
            *knots.get(count)?,
        ])
    };
    Some([
        domain(surface.u_knots(), surface.u_degree(), surface.u_count())?,
        domain(surface.v_knots(), surface.v_degree(), surface.v_count())?,
    ])
}

pub(super) struct OffsetBridge {
    pub(super) result: AllocationReference,
    pub(super) support: AllocationReference,
    pub(super) distance: f64,
    pub(super) kind: u8,
    pub(super) bounds: [[f64; 2]; 2],
}

pub(super) fn offset_bridge(bytes: &[u8]) -> Option<OffsetBridge> {
    let mut at = 0;
    let result = allocation_reference(bytes, &mut at)?;
    let support = allocation_reference(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let distance = view.f64_le()?;
    let kind = view.u8()?;
    let mut range = || IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?]);
    let bounds = [range()?.endpoints(), range()?.endpoints()];
    (distance.is_finite() && view.is_empty()).then_some(OffsetBridge {
        result,
        support,
        distance,
        kind,
        bounds,
    })
}

pub(super) fn offset_cache(
    bytes: &[u8],
    bounds: [[f64; 2]; 2],
) -> Option<(AllocationReference, f64)> {
    let mut at = 0;
    let base = allocation_reference(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let distance = view.f64_le()?;
    let lower = [view.f64_le()?, view.f64_le()?];
    let upper = [view.f64_le()?, view.f64_le()?];
    for (axis, [lo, hi]) in bounds.into_iter().enumerate() {
        let cache = IncreasingParameterInterval::new([lower[axis], upper[axis]])?;
        if (lo < cache.lower() && !near(lo, cache.lower()))
            || (hi > cache.upper() && !near(hi, cache.upper()))
        {
            return None;
        }
    }
    (distance.is_finite() && view.remaining() == 0).then_some((base, distance))
}

pub(super) struct SpatialPlane {
    surface: PlaneSurface,
    pub(super) bounds: Option<[[f64; 2]; 2]>,
}

impl SpatialPlane {
    pub(super) fn from_record(data: &[u8], record: &ConsolidatedRecord) -> Option<Self> {
        let bytes = super::payload(data, record)?;
        if let Some(plane) = Self::read(bytes) {
            return Some(plane);
        }
        let carrier = b2_plane_carriers_from_records(data, std::slice::from_ref(record)).pop()?;
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(surface)) =
            b2_plane_geometry(&carrier)?
        else {
            return None;
        };
        Some(Self {
            surface,
            bounds: None,
        })
    }

    pub(super) fn read(bytes: &[u8]) -> Option<Self> {
        let mut view = View::over_retained(bytes);
        let layout = view.u8()?;
        let mask = view.u8()?;
        let frame = if layout == 0x08 {
            Some(view.u8()?)
        } else {
            None
        };
        let origin_x = view.f64_le()?;
        let compressed_origin = matches!((layout, mask), (0xb4, 0xd6));
        let origin_y = if compressed_origin {
            0.0
        } else {
            view.f64_le()?
        };
        let origin_z = if compressed_origin || (layout, mask) == (0xc4, 0x3c) {
            0.0
        } else {
            view.f64_le()?
        };
        let origin = FinitePoint3::new(Point3::new(origin_x, origin_y, origin_z))?;
        let mut direction = |mask: u8| {
            let omitted = mask & 0x70;
            let unit_component = mask & 0x08 != 0;
            if unit_component && omitted.count_ones() != 2 {
                return None;
            }
            let mut component = |bit: u8| {
                if omitted & bit != 0 {
                    Some(0.0)
                } else if unit_component {
                    Some(1.0)
                } else {
                    view.f64_le()
                }
            };
            UnitVector3::new(Vector3::new(
                component(0x10)?,
                component(0x20)?,
                component(0x40)?,
            ))
        };
        let (u, v) = match (layout, mask, frame) {
            (0x84, 0x00 | 0x10 | 0x20 | 0x40, None)
            | (0x94, 0x10 | 0x60 | 0x68, None)
            | (0xa4, 0x50 | 0x58, None)
            | (0xb4, 0x58, None)
            | (0xe4, 0x50, None)
            | (0xc4, 0x30 | 0x38 | 0x3c, None)
            | (0xd4, 0x20, None) => (
                direction(mask)?,
                direction((layout & 0x70) | ((mask & 0x80) >> 4))?,
            ),
            (0xd4, 0xa0, None) => {
                let v_mask = match bytes.len() {
                    implicit_v_a0::LEN => 0x58,
                    explicit_v_a0::LEN => 0x50,
                    _ => return None,
                };
                (direction(mask)?, direction(v_mask)?)
            }
            (0xb4, 0xd0, None) => {
                let y = view.f64_le()?;
                if y.abs() != 1.0 {
                    return None;
                }
                (
                    UnitVector3::new(Vector3::new(0.0, y, 0.0))?,
                    UnitVector3::Z_AXIS,
                )
            }
            (0xb4, 0xd6, None) => (direction(mask)?, UnitVector3::Z_AXIS),
            (0x08, 0, Some(0x18)) => (direction(0)?, direction(0)?),
            (0x08, 0, Some(0x19)) => (direction(0)?, direction(0x10)?),
            (0x08, 0x68 | 0xe8, Some(0x1d)) => {
                (direction(mask)?, direction(0x50 | ((mask & 0x80) >> 4))?)
            }
            _ => return None,
        };
        let bounds = if frame.is_none() {
            let mut interval =
                || IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?]);
            Some([interval()?.endpoints(), interval()?.endpoints()])
        } else {
            None
        };
        if view.remaining() != 0 {
            return None;
        }
        Some(Self {
            surface: PlaneSurface::new(origin, OrthonormalFrame3::completing(u, v)?),
            bounds,
        })
    }
}

pub(super) fn carrier_bridge(bytes: &[u8]) -> Option<(AllocationReference, u8)> {
    if bytes.first() != Some(&0x81) {
        return None;
    }
    let mut at = 1;
    let carrier = allocation_reference(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let control = view.take(3)?;
    if matches!(control, [0x05, 0x01 | 0x05, 0x09]) && view.is_empty() {
        return Some((carrier, 0x29));
    }
    if !matches!(control, [0x05, 0x01 | 0x05, 0x05]) || !near(view.f64_le()?, 1.0) {
        return None;
    }
    IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    let u = view.f64_le()?;
    let v = view.f64_le()?;
    if !(u.is_finite() && near(v, 1.0) || near(u.hypot(v), 1.0)) {
        return None;
    }
    IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    view.is_empty().then_some((carrier, 0x27))
}

pub(super) struct SpatialCylinder {
    surface: CylinderSurface,
    bounds: [[f64; 2]; 2],
    angle_scale: f64,
}

impl SpatialCylinder {
    pub(super) fn read(bytes: &[u8]) -> Option<Self> {
        let mut view = View::over_retained(bytes);
        let origin =
            FinitePoint3::new(Point3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?))?;
        let mask = view.u8()?;
        if mask >> 6 != 0 {
            return None;
        }
        let mut direction = |offset: u32| {
            let mut component = |i: u32| {
                if mask & (1u8 << i) == 0u8 {
                    view.f64_le()
                } else {
                    Some(0.0)
                }
            };
            UnitVector3::new(Vector3::new(
                component(offset)?,
                component(offset + 1)?,
                component(offset + 2)?,
            ))
        };
        let frame = OrthonormalFrame3::completing(direction(0)?, direction(3)?)?;
        let radius = PositiveLength::new(view.f64_le()?)?;
        let mut interval = || IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?]);
        let u = interval()?.endpoints();
        let v = interval()?.endpoints();
        let angle_scale = match (view.u8()?, view.f64_le()?) {
            (3, shift) if near(shift, cylinder_range_origin(radius.get(), u)) => {
                radius.get().recip()
            }
            (6, factor) if factor.is_finite() && factor > 0.0 => factor / radius.get(),
            _ => return None,
        };
        if !view.is_empty()
            || (u[1] - u[0]) * angle_scale > std::f64::consts::TAU + super::CHART_TOLERANCE
        {
            return None;
        }
        Some(Self {
            surface: CylinderSurface::new(origin, frame, radius),
            bounds: [u, v],
            angle_scale,
        })
    }

    pub(super) fn matches_radius(&self, radius: f64) -> bool {
        near(self.surface.radius().get(), radius)
    }
}

pub(super) fn analytic_bridge(bytes: &[u8]) -> Option<(AllocationReference, u8, f64)> {
    if bytes.first()? != &0x85 {
        return None;
    }
    let mut at = 1;
    let carrier = allocation_reference(bytes, &mut at)?;
    for _ in 0..4 {
        allocation_reference(bytes, &mut at)?;
    }
    let mut view = View::over_retained(bytes.get(at..)?);
    let class = match view.u8()? {
        0x05 => 0x28,
        0x09 => 0x2b,
        0x0d => 0x2a,
        _ => return None,
    };
    if view.u8()? != 0x05 {
        return None;
    }
    let radius = PositiveLength::new(view.f64_le()?)?;
    if !matches!(
        (class, view.take(2)?),
        (0x28, [0x01, 0x03 | 0x05]) | (_, [0x03 | 0x05, 0x03 | 0x05])
    ) || view.take(8)? != [0; 8]
        || !matches!(
            (class, view.take(2)?),
            (0x2a, [0x05, 0x81 | 0x09] | [0x01, 0x11 | 0x81]) | (0x28 | 0x2b, [0x01 | 0x05, 0x05])
        )
        || view.remaining() != 0
    {
        return None;
    }
    Some((carrier, class, radius.get()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_plane_retains_the_mirrored_chart_direction() {
        let mut bytes = vec![0xb4, 0xd0];
        for value in [3.0_f64, 4.0, 5.0, -1.0, -2.0, 3.0, 0.0, 4.0] {
            bytes.extend(value.to_le_bytes());
        }
        let plane = SpatialPlane::read(&bytes).expect("bounded mirrored plane");
        assert_eq!(plane.bounds, Some([[-2.0, 3.0], [0.0, 4.0]]));
        let carrier = Carrier::Plane(plane);
        assert_eq!(
            carrier.evaluate([2.0, 3.0]),
            Some(Point3::new(3.0, 2.0, 8.0))
        );
        bytes.push(0);
        assert!(SpatialPlane::read(&bytes).is_none());
    }

    #[test]
    fn offset_cache_admits_a_trimmed_subdomain() {
        let mut bytes = vec![5];
        for value in [3.0_f64, -2.0, 0.0, 8.0, 12.0] {
            bytes.extend(value.to_le_bytes());
        }
        assert_eq!(
            offset_cache(&bytes, [[0.0, 4.0], [2.0, 9.0]])
                .expect("valid fixture carrier")
                .1,
            3.0
        );
        assert!(offset_cache(&bytes, [[0.0, 10.0], [2.0, 9.0]]).is_none());
    }

    #[test]
    fn spatial_cylinder_elides_zero_components_of_both_radial_directions() {
        let mut bytes = Vec::new();
        for f in [3.0_f64, 4.0, 5.0] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.push(0x33);
        for f in [1.0_f64, 1.0, 0.5, 0.0, 1.0, -2.0, 2.0] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.push(3);
        bytes.extend((0.5 - std::f64::consts::FRAC_PI_2).to_le_bytes());
        let cylinder = SpatialCylinder::read(&bytes).expect("valid fixture carrier");
        assert_eq!(
            cylinder.surface.frame().axis().as_raw(),
            &Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            cylinder.surface.frame().reference().as_raw(),
            &Vector3::new(0.0, 0.0, 1.0)
        );
        let chart = Carrier::Cylinder(cylinder);
        assert_eq!(chart.scale(), [2.0, 1.0]);
        assert_eq!(chart.bounds(), Some([[0.0, 1.0], [-2.0, 2.0]]));
        bytes[24] ^= 0x01;
        assert!(SpatialCylinder::read(&bytes).is_none());
    }
    #[test]
    fn bounded_plane_refuses_unestablished_selectors() {
        for (prefix, frame) in [
            ([0xb4, 0xde], &[3.0_f64][..]),
            ([0xb4, 0x50], &[3.0, 4.0, 5.0, -1.0, 1.0][..]),
            ([0xe4, 0x58], &[3.0, 4.0, 5.0, 1.0][..]),
            ([0xd4, 0x68], &[3.0, 4.0, 5.0, -1.0][..]),
            ([0xd4, 0xe8], &[3.0, 4.0, 5.0][..]),
            ([0xa4, 0x00], &[3.0, 4.0, 5.0, 0.0, 1.0, 0.0, 0.6, 0.8][..]),
        ] {
            let mut bytes = prefix.to_vec();
            for value in frame.iter().copied().chain([-4.0, 4.0, -6.0, 6.0]) {
                bytes.extend(value.to_le_bytes());
            }
            assert!(SpatialPlane::read(&bytes).is_none(), "prefix {prefix:02x?}");
        }
    }

    #[test]
    fn unrestricted_plane_retains_the_signed_v_axis() {
        for (mask, sign) in [(0xe8, 1.0), (0x68, -1.0), (0x68, 1.0)] {
            let mut bytes = vec![0x08, mask, 0x1d];
            for value in [3.0_f64, 4.0, 5.0] {
                bytes.extend(value.to_le_bytes());
            }
            if mask == 0x68 {
                bytes.extend(f64::to_le_bytes(sign));
            }
            let plane = SpatialPlane::read(&bytes).expect("implicit-axis plane");
            assert_eq!(plane.bounds, None);
            assert_eq!(
                Carrier::Plane(plane).evaluate([2.0, 3.0]),
                Some(Point3::new(5.0, 4.0 + 3.0 * sign, 5.0))
            );
            bytes.push(0);
            assert!(SpatialPlane::read(&bytes).is_none());
        }
        for sign in [0.0_f64, 2.0, f64::NAN] {
            let mut bytes = vec![0x08, 0x68, 0x1d];
            for value in [3.0, 4.0, 5.0, sign] {
                bytes.extend(value.to_le_bytes());
            }
            assert!(SpatialPlane::read(&bytes).is_none());
        }
    }

    #[test]
    fn bounded_plane_retains_implicit_and_explicit_v_components() {
        for (mask, sign) in [(0xa0, 1.0), (0x20, -1.0), (0x20, 1.0)] {
            let mut bytes = vec![0xd4, mask];
            for value in [3.0_f64, 4.0, 5.0, -1.0, 0.0] {
                bytes.extend(value.to_le_bytes());
            }
            if mask == 0x20 {
                bytes.extend(f64::to_le_bytes(sign));
            }
            for value in [0.0_f64, 20.0, -10.0, 10.0] {
                bytes.extend(value.to_le_bytes());
            }
            let plane = SpatialPlane::read(&bytes).expect("bounded signed-axis plane");
            assert_eq!(plane.bounds, Some([[0.0, 20.0], [-10.0, 10.0]]));
            assert_eq!(
                Carrier::Plane(plane).evaluate([2.0, 3.0]),
                Some(Point3::new(1.0, 4.0 + 3.0 * sign, 5.0))
            );
            bytes.push(0);
            assert!(SpatialPlane::read(&bytes).is_none());
        }
    }

    #[test]
    fn packed_plane_omits_the_zero_x_component_of_v() {
        let mut bytes = vec![0x08, 0x00, 0x19];
        for value in [3.0_f64, 4.0, 5.0, 1.0, 0.0, 0.0, 0.6, 0.8] {
            bytes.extend(value.to_le_bytes());
        }
        let plane = Carrier::Plane(SpatialPlane::read(&bytes).expect("packed plane"));
        let point = plane.evaluate([2.0, 5.0]).expect("chart point");
        assert!(point.distance(Point3::new(5.0, 7.0, 9.0)) < 1e-12);
    }
    #[test]
    fn bounded_plane_preserves_the_82_byte_a0_frame_and_its_signed_v_axis() {
        for sign in [-1.0_f64, 1.0] {
            let mut bytes = vec![0xd4, 0xa0];
            for value in [3.0_f64, 4.0, 5.0, -1.0, 0.0, sign, 0.0, 20.0, -10.0, 10.0] {
                bytes.extend(value.to_le_bytes());
            }
            assert_eq!(bytes.len(), explicit_v_a0::LEN);
            let plane = SpatialPlane::read(&bytes).expect("legacy explicit V frame");
            assert_eq!(plane.bounds, Some([[0.0, 20.0], [-10.0, 10.0]]));
            assert_eq!(
                Carrier::Plane(plane).evaluate([2.0, 3.0]),
                Some(Point3::new(1.0, 4.0 + 3.0 * sign, 5.0))
            );
            bytes.push(0);
            assert!(SpatialPlane::read(&bytes).is_none());
            bytes.truncate(explicit_v_a0::LEN - 1);
            assert!(SpatialPlane::read(&bytes).is_none());
        }
    }
    #[test]
    fn packed_plane_refuses_unestablished_frame_forms() {
        for (prefix, axes) in [
            ([0x08, 0x00, 0x1a], vec![1.0_f64, 0.0, 0.0, 0.0, 1.0]),
            ([0x08, 0x40, 0x1b], vec![0.0, 1.0, 1.0]),
            ([0x08, 0x50, 0x1b], vec![1.0, 1.0]),
            ([0x08, 0x58, 0x1b], vec![1.0]),
            ([0x08, 0xd0, 0x1b], vec![1.0]),
            ([0x08, 0xd8, 0x1b], vec![]),
        ] {
            let mut bytes = prefix.to_vec();
            for value in [3.0_f64, 4.0, 5.0].into_iter().chain(axes) {
                bytes.extend(value.to_le_bytes());
            }
            assert!(SpatialPlane::read(&bytes).is_none(), "prefix {prefix:02x?}");
        }
    }
}
