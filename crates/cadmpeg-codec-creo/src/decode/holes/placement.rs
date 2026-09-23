// SPDX-License-Identifier: Apache-2.0
//! Hole placement, cap outlines, and cylinder construction from envelopes.

use crate::decode::axis::Axis;
use crate::vecmath::normalize;
use cadmpeg_ir::features::LinearTermination;
use cadmpeg_ir::geometry::analytic::CylinderSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_SIGNED_LENGTH: f64 = 1.0e-9;
const EPS_SPAN_AGREEMENT: f64 = 1.0e-9;
const EPS_AXIS_COMPONENT: f64 = 1.0e-9;
const EPS_CENTER_AGREEMENT: f64 = 1.0e-9;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;

/// Signed offsets of an extrusion's bottom and top along the section normal.
///
/// The interval contains the section plane (`lower <= 0 <= upper`), is not degenerate
/// (`lower < upper`), and has a finite length `upper - lower`, so both offsets are finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::decode) struct ExtrusionSpan {
    lower: f64,
    upper: f64,
}

impl ExtrusionSpan {
    /// Admits an interval that contains the section plane and has a finite positive length.
    pub(in crate::decode) fn new(lower: f64, upper: f64) -> Option<Self> {
        (lower <= 0.0 && upper >= 0.0 && lower < upper && (upper - lower).is_finite())
            .then_some(Self { lower, upper })
    }

    /// Returns the bottom offset, which is not positive.
    pub(in crate::decode) fn lower(self) -> f64 {
        self.lower
    }

    /// Returns the top offset, which is not negative.
    pub(in crate::decode) fn upper(self) -> f64 {
        self.upper
    }
}

pub(in crate::decode) fn hole_extent_and_direction(
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

pub(in crate::decode) fn hole_placement(
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

pub(in crate::decode) fn plane_envelope_corners(
    envelope: &crate::surface::PlaneEnvelope,
) -> Option<[[f64; 3]; 2]> {
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
pub(in crate::decode) struct CapOutline {
    pub(in crate::decode) surface_id: u32,
    pub(in crate::decode) origin: [f64; 3],
    pub(in crate::decode) normal: [f64; 3],
    pub(in crate::decode) corners: [[f64; 3]; 2],
}

/// The model axis a direction is aligned with, when it is aligned with one.
pub(super) fn axis_aligned_with(direction: [f64; 3], component_tolerance: f64) -> Option<Axis> {
    Axis::ALL.into_iter().find(|axis| {
        direction[axis.index()].abs() > 1.0 - EPS_AXIS_ALIGNMENT
            && axis
                .complement()
                .iter()
                .all(|other| direction[other.index()].abs() < component_tolerance)
    })
}

pub(super) fn cap_square_center_radius(
    corners: [[f64; 3]; 2],
    axis: Axis,
) -> Option<([f64; 3], f64)> {
    let axis_index = axis.index();
    let radial = axis.complement().map(Axis::index);
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

pub(in crate::decode) fn cylinder_from_single_cap_outline(
    cap: CapOutline,
) -> Option<CylinderSurface> {
    let axis = normalize(cap.normal)?;
    let aligned_axis = axis_aligned_with(axis, EPS_AXIS_COMPONENT)?;
    let (center, radius) = cap_square_center_radius(cap.corners, aligned_axis)?;
    let mut ref_direction = [0.0; 3];
    ref_direction[aligned_axis.complement()[0].index()] = 1.0;
    CylinderSurface::try_new(
        Point3::from(center),
        Vector3::from(axis),
        Vector3::from(ref_direction),
        radius,
    )
    .ok()
}

pub(in crate::decode) fn hole_cylinder_from_cap_outlines(
    caps: [CapOutline; 2],
) -> Option<CylinderSurface> {
    let placement = hole_placement(caps.map(|cap| (cap.surface_id, cap.origin, cap.normal)))?;
    let axis = placement.1;
    let aligned_axis = axis_aligned_with(axis, EPS_AXIS_COMPONENT)?;
    let radial = aligned_axis.complement().map(Axis::index);
    let mut centers = Vec::<[f64; 3]>::new();
    let mut radii = Vec::new();
    for cap in caps {
        let (center, radius) = cap_square_center_radius(cap.corners, aligned_axis)?;
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
    CylinderSurface::try_new(
        Point3::from(centers[0]),
        Vector3::from(axis),
        Vector3::from(ref_direction),
        radii[0],
    )
    .ok()
}

pub(in crate::decode) fn cylinder_from_complementary_outline_bounds(
    plane: &SurfaceGeometry,
    bounds: [[[f64; 2]; 2]; 2],
) -> Option<SurfaceGeometry> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = plane else {
        return None;
    };
    let origin = plane_surface.origin().get();
    let normal = plane_surface.normal();
    let axis = normalize([normal.x, normal.y, normal.z])?;
    let aligned_axis = axis_aligned_with(axis, EPS_AXIS_COMPONENT)?;
    let radial = aligned_axis.complement().map(Axis::index);
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
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::from(center),
            Vector3::from(axis),
            Vector3::from(ref_direction),
            0.5 * spans[0],
        )
        .ok()?,
    )))
}

/// A solved simple hole: its entry plane, generated cylinder rows, extent and cylinder
/// carrier. The carrier axis is the drilling direction.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::decode) struct SimpleHoleGeometry<'a> {
    pub(in crate::decode) entry_surface_id: Option<u32>,
    pub(in crate::decode) cylinder_rows: Vec<&'a crate::surface::SurfaceRow>,
    pub(in crate::decode) extent: LinearTermination,
    pub(in crate::decode) geometry: CylinderSurface,
}

#[cfg(test)]
mod tests;
