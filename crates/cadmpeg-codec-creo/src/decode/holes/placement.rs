// SPDX-License-Identifier: Apache-2.0
//! Hole placement, cap outlines, and cylinder construction from envelopes.

use crate::vecmath::normalize;
use cadmpeg_ir::features::LinearTermination;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_SIGNED_LENGTH: f64 = 1.0e-9;
const EPS_SPAN_AGREEMENT: f64 = 1.0e-9;
const EPS_AXIS_COMPONENT: f64 = 1.0e-9;
const EPS_CENTER_AGREEMENT: f64 = 1.0e-9;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrusionSpan {
    pub lower: f64,
    pub upper: f64,
}

pub fn hole_extent_and_direction(
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<([f64; 3], LinearTermination)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(first_origin, first_normal), (second_origin, second_normal)] = planes.as_slice() else {
        return None;
    };
    let first_normal = normalize(*first_normal)?;
    let second_normal = normalize(*second_normal)?;
    let alignment = first_normal
        .iter()
        .zip(second_normal)
        .map(|(first, second)| first * second)
        .sum::<f64>()
        .abs();
    if (alignment - 1.0).abs() > EPS_AXIS_ALIGNMENT {
        return None;
    }
    let signed_length = second_origin
        .iter()
        .zip(first_origin)
        .zip(first_normal)
        .map(|((second, first), axis)| (second - first) * axis)
        .sum::<f64>();
    let scale = second_origin
        .iter()
        .chain(first_origin)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if signed_length.abs() <= EPS_SIGNED_LENGTH * scale {
        return None;
    }
    Some((
        first_normal.map(|value| value * signed_length.signum()),
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(signed_length.abs())?,
        },
    ))
}

pub fn hole_placement(
    planes: impl IntoIterator<Item = (u32, [f64; 3], [f64; 3])>,
) -> Option<(u32, [f64; 3], LinearTermination)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(entry_id, entry_origin, entry_normal), (_, termination_origin, termination_normal)] =
        planes.as_slice()
    else {
        return None;
    };
    let (direction, extent) = hole_extent_and_direction([
        (*entry_origin, *entry_normal),
        (*termination_origin, *termination_normal),
    ])?;
    Some((*entry_id, direction, extent))
}

pub fn plane_envelope_corners(envelope: &crate::surface::PlaneEnvelope) -> Option<[[f64; 3]; 2]> {
    let corners = match envelope {
        crate::surface::PlaneEnvelope::Standard { corners_3d, .. }
        | crate::surface::PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
    };
    Some([
        [corners[0][0]?, corners[0][1]?, corners[0][2]?],
        [corners[1][0]?, corners[1][1]?, corners[1][2]?],
    ])
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapOutline {
    pub surface_id: u32,
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub corners: Option<[[f64; 3]; 2]>,
}

pub fn cap_square_center_radius(
    corners: [[f64; 3]; 2],
    axis_index: usize,
) -> Option<([f64; 3], f64)> {
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let spans = [
        (corners[1][radial[0]] - corners[0][radial[0]]).abs(),
        (corners[1][radial[1]] - corners[0][radial[1]]).abs(),
    ];
    let scale = spans[0]
        .max(spans[1])
        .max(corners[0][axis_index].abs())
        .max(corners[1][axis_index].abs())
        .max(1.0);
    if (corners[1][axis_index] - corners[0][axis_index]).abs() > EPS_SPAN_AGREEMENT * scale
        || spans[0] <= EPS_SPAN_AGREEMENT
        || (spans[0] - spans[1]).abs() > EPS_SPAN_AGREEMENT * scale
    {
        return None;
    }
    Some((
        std::array::from_fn(|index| 0.5 * (corners[0][index] + corners[1][index])),
        0.5 * spans[0],
    ))
}

pub fn cylinder_from_single_cap_outline(cap: CapOutline) -> Option<HoleCylinder> {
    let axis = normalize(cap.normal)?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - EPS_AXIS_ALIGNMENT
            && (0..3).all(|other| other == *index || axis[other].abs() < EPS_AXIS_COMPONENT)
    })?;
    let (center, radius) = cap_square_center_radius(cap.corners?, axis_index)?;
    let radial_axis = (0..3).find(|index| *index != axis_index)?;
    let mut ref_direction = [0.0; 3];
    ref_direction[radial_axis] = 1.0;
    Some(HoleCylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius,
    })
}

pub fn hole_cylinder_from_cap_outlines(caps: [CapOutline; 2]) -> Option<HoleCylinder> {
    let placement = hole_placement(caps.map(|cap| (cap.surface_id, cap.origin, cap.normal)))?;
    let axis = placement.1;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - EPS_AXIS_ALIGNMENT
            && (0..3).all(|other| other == *index || axis[other].abs() < EPS_AXIS_COMPONENT)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let mut centers = Vec::<[f64; 3]>::new();
    let mut radii = Vec::new();
    for cap in caps {
        let (center, radius) = cap_square_center_radius(cap.corners?, axis_index)?;
        centers.push(center);
        radii.push(radius);
    }
    let scale = centers
        .iter()
        .flatten()
        .chain(&radii)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if radial
        .iter()
        .any(|index| (centers[0][*index] - centers[1][*index]).abs() > EPS_CENTER_AGREEMENT * scale)
        || (radii[0] - radii[1]).abs() > EPS_RADIUS_AGREEMENT * scale
    {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(HoleCylinder {
        origin: Point3::new(centers[0][0], centers[0][1], centers[0][2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius: radii[0],
    })
}

pub fn cylinder_from_complementary_outline_bounds(
    plane: &SurfaceGeometry,
    bounds: [[[f64; 2]; 2]; 2],
) -> Option<SurfaceGeometry> {
    let SurfaceGeometry::Plane(plane_surface) = plane else {
        return None;
    };
    let origin = plane_surface.origin();
    let normal = plane_surface.normal();
    let axis = normalize([normal.x, normal.y, normal.z])?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - EPS_AXIS_ALIGNMENT
            && (0..3).all(|other| other == *index || axis[other].abs() < EPS_AXIS_COMPONENT)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let scale = bounds
        .iter()
        .flatten()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_CENTER_AGREEMENT * scale;
    if bounds
        .iter()
        .any(|rectangle| (0..2).any(|index| rectangle[1][index] <= rectangle[0][index]))
    {
        return None;
    }
    let union = if close(bounds[0][0][0], bounds[1][0][0])
        && close(bounds[0][1][0], bounds[1][1][0])
        && (close(bounds[0][1][1], bounds[1][0][1]) || close(bounds[1][1][1], bounds[0][0][1]))
    {
        [
            [bounds[0][0][0], bounds[0][0][1].min(bounds[1][0][1])],
            [bounds[0][1][0], bounds[0][1][1].max(bounds[1][1][1])],
        ]
    } else if close(bounds[0][0][1], bounds[1][0][1])
        && close(bounds[0][1][1], bounds[1][1][1])
        && (close(bounds[0][1][0], bounds[1][0][0]) || close(bounds[1][1][0], bounds[0][0][0]))
    {
        [
            [bounds[0][0][0].min(bounds[1][0][0]), bounds[0][0][1]],
            [bounds[0][1][0].max(bounds[1][1][0]), bounds[0][1][1]],
        ]
    } else {
        return None;
    };
    let spans = [union[1][0] - union[0][0], union[1][1] - union[0][1]];
    if spans.iter().any(|span| !span.is_finite() || *span <= 0.0) || !close(spans[0], spans[1]) {
        return None;
    }
    let mut center = [origin.x, origin.y, origin.z];
    for (coordinate, index) in radial.iter().enumerate() {
        center[*index] = 0.5 * (union[0][coordinate] + union[1][coordinate]);
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(center[0], center[1], center[2]),
            Vector3::new(axis[0], axis[1], axis[2]),
            Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
            0.5 * spans[0],
        )
        .ok()?,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoleCylinder {
    pub origin: Point3,
    pub axis: Vector3,
    pub ref_direction: Vector3,
    pub radius: f64,
}

impl TryFrom<HoleCylinder> for SurfaceGeometry {
    type Error = &'static str;

    fn try_from(cylinder: HoleCylinder) -> Result<Self, Self::Error> {
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            cylinder.origin,
            cylinder.axis,
            cylinder.ref_direction,
            cylinder.radius,
        )
        .map(Self::Cylinder)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimpleHoleGeometry<'a> {
    pub entry_surface_id: Option<u32>,
    pub cylinder_rows: Vec<&'a crate::surface::SurfaceRow>,
    pub direction: [f64; 3],
    pub extent: LinearTermination,
    pub geometry: HoleCylinder,
}
