// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino NURBS and plane-surface payload decoding.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use cadmpeg_core::decode::{alloc_filled, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_ir::units::FiniteVector;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader};
use crate::curves::{decode_embedded_curve, error, exact_nurbs, DecodedCurve, GeometryError};
use crate::settings::{
    bbox, interval, plane, point, vector as native_vector, MillimeterScale, Plane,
};
use crate::wire::{vector, Uuid};

const EPS_SURFACE_DEGENERATE: f64 = 1.0e-10;

pub(crate) const NURBS_CURVE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xdd, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const NURBS_SURFACE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xde, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const NURBS_SURFACE_TL: Uuid = Uuid::from_canonical([
    0x47, 0x60, 0xc8, 0x17, 0x0b, 0xe3, 0x11, 0xd4, 0xbf, 0xfe, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const NURBS_SURFACE_LEGACY: Uuid = Uuid::from_canonical([
    0xfa, 0x4f, 0xd4, 0xb5, 0x16, 0x13, 0x11, 0xd4, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const PLANE_SURFACE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xdf, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const CLIPPING_PLANE_SURFACE: Uuid = Uuid::from_canonical([
    0xdb, 0xc5, 0xa5, 0x84, 0xce, 0x3f, 0x41, 0x70, 0x98, 0xa8, 0x49, 0x70, 0x69, 0xca, 0x5c, 0x36,
]);
pub(crate) const REV_SURFACE: Uuid = Uuid::from_canonical([
    0xa1, 0x62, 0x20, 0xd3, 0x16, 0x3b, 0x11, 0xd4, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const REV_SURFACE_LEGACY: Uuid = Uuid::from_canonical([
    0x0a, 0x84, 0x01, 0xb6, 0x4d, 0x34, 0x4b, 0x99, 0x86, 0x15, 0x1b, 0x4e, 0x72, 0x3d, 0xc4, 0xe5,
]);
pub(crate) const SUM_SURFACE: Uuid = Uuid::from_canonical([
    0xc4, 0xcd, 0x53, 0x59, 0x44, 0x6d, 0x46, 0x90, 0x9f, 0xf5, 0x29, 0x05, 0x97, 0x32, 0x47, 0x2b,
]);

/// Returns whether a class is one of the native procedural surfaces.
pub(crate) fn is_procedural_class(uuid: Uuid) -> bool {
    matches!(uuid, REV_SURFACE | REV_SURFACE_LEGACY | SUM_SURFACE)
}

#[derive(Debug, Clone)]
pub(crate) enum TypedSurface {
    Plane {
        plane: cadmpeg_ir::geometry::analytic::PlaneSurface,
        parameterization: PlaneParameterization,
    },
    Nurbs(NurbsSurface),
}

impl TypedSurface {
    pub(crate) fn plane_parameterization(&self) -> Option<PlaneParameterization> {
        match self {
            Self::Plane {
                parameterization, ..
            } => Some(*parameterization),
            Self::Nurbs(_) => None,
        }
    }

    pub(crate) fn into_geometry(self) -> SurfaceGeometry {
        match self {
            Self::Plane { plane, .. } => {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane))
            }
            Self::Nurbs(nurbs) => SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum DecodedSurface {
    /// A typed surface and its conversion state.
    Typed {
        /// Decoded surface geometry.
        geometry: TypedSurface,
        /// Whether native coordinates were scaled or reconstructed.
        derived: bool,
    },
    /// A solved native procedural surface and its ordered child trees.
    Procedural {
        /// Exact solved NURBS carrier.
        geometry: NurbsSurface,
        /// Native construction fields.
        definition: DecodedProceduralSurface,
    },
}

/// Affine map from a plane surface's source parameter domain to its physical
/// plane extents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlaneParameterization {
    u_domain: [f64; 2],
    v_domain: [f64; 2],
    u_extents: [f64; 2],
    v_extents: [f64; 2],
}

impl PlaneParameterization {
    pub(crate) fn map_point(self, point: Point2) -> Point2 {
        Point2::new(
            map_parameter(point.u, self.u_domain, self.u_extents),
            map_parameter(point.v, self.v_domain, self.v_extents),
        )
    }
}

/// Native procedural fields and their fixed-cardinality child curves.
#[derive(Debug, Clone)]
pub(crate) enum DecodedProceduralSurface {
    /// Revolution of the first child.
    Revolution {
        children: Box<[DecodedCurve; 1]>,
        /// Scaled axis origin.
        axis_origin: Point3,
        /// Unit axis direction.
        axis_direction: Vector3,
        /// Native angular interval.
        angular_interval: [f64; 2],
        /// Native revolution parameter interval.
        parameter_interval: [f64; 2],
        /// Source parameter-direction transpose flag.
        transposed: bool,
    },
    /// Sum of the first and second children.
    Sum {
        children: Box<[DecodedCurve; 2]>,
        /// Scaled basepoint vector.
        basepoint: Vector3,
    },
}

impl DecodedProceduralSurface {
    pub(crate) fn into_definition<E>(
        self,
        mut commit_child: impl FnMut(
            usize,
            &'static str,
            DecodedCurve,
        ) -> Result<cadmpeg_ir::ids::CurveId, E>,
        reject_payload: impl FnOnce(cadmpeg_ir::geometry::ProceduralGeometryError) -> E,
    ) -> Result<cadmpeg_ir::geometry::ProceduralSurfaceDefinition, E> {
        use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

        Ok(match self {
            Self::Revolution {
                children,
                axis_origin,
                axis_direction,
                angular_interval,
                parameter_interval,
                transposed,
            } => {
                let [directrix] = *children;
                let directrix = commit_child(0, "directrix", directrix)?;
                ProceduralSurfaceDefinition::Revolution(
                    cadmpeg_ir::geometry::surface_payloads::admit_revolution_axis(
                        axis_origin,
                        axis_direction,
                    )
                    .and_then(|axis| {
                        cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                            directrix,
                            axis,
                            angular_interval,
                            None,
                            Some(parameter_interval),
                            transposed,
                            cadmpeg_ir::geometry::CacheContract::from_form(None),
                        )
                    })
                    .map_err(reject_payload)?,
                )
            }
            Self::Sum {
                children,
                basepoint,
            } => {
                let [first, second] = *children;
                let first = commit_child(0, "first", first)?;
                let second = commit_child(1, "second", second)?;
                ProceduralSurfaceDefinition::Sum(
                    cadmpeg_ir::geometry::surface_payloads::SumSurfaceConstruction::try_new(
                        first,
                        second,
                        basepoint,
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                    .map_err(reject_payload)?,
                )
            }
        })
    }
}

pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedSurface, GeometryError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let result = if matches!(
        class,
        NURBS_SURFACE | NURBS_SURFACE_TL | NURBS_SURFACE_LEGACY
    ) {
        DecodedSurface::Typed {
            geometry: TypedSurface::Nurbs(read_nurbs_surface(ctx, &mut reader, scale)?),
            derived: true,
        }
    } else if class == PLANE_SURFACE {
        let geometry = read_plane_surface_with_parameterization(&mut reader, scale)?;
        DecodedSurface::Typed {
            geometry,
            derived: scale != MillimeterScale::IDENTITY,
        }
    } else if class == CLIPPING_PLANE_SURFACE {
        read_clipping_plane_surface(data, &mut reader, scale, archive)?
    } else if matches!(class, REV_SURFACE | REV_SURFACE_LEGACY) {
        read_revolution(ctx, data, &mut reader, scale, archive, depth)?
    } else if class == SUM_SURFACE {
        read_sum(ctx, data, &mut reader, scale, archive, depth)?
    } else {
        return Err(GeometryError::unsupported(
            range.start,
            "unsupported Rhino surface class",
        ));
    };
    reader.skip_remaining()?;
    Ok(result)
}

fn read_clipping_plane_surface(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<DecodedSurface, GeometryError> {
    const ANONYMOUS: u32 = 0x4000_8000;
    let outer = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if outer.typecode != ANONYMOUS || outer.short() {
        return Err(error(
            reader.position(),
            "invalid clipping-plane outer chunk",
        ));
    }
    let mut payload = BoundedReader::new(data, outer.body().start, outer.body().end)?;
    let version = (payload.i32()?, payload.i32()?);
    if version.0 != 1 || version.1 < 0 {
        return Err(GeometryError::unsupported(
            outer.body().start,
            "unsupported clipping-plane surface version",
        ));
    }
    let plane_chunk = chunk_at(data, payload.position(), payload.end(), archive, false)?;
    if plane_chunk.typecode != ANONYMOUS || plane_chunk.short() {
        return Err(error(
            plane_chunk.header_start,
            "invalid clipping-plane carrier chunk",
        ));
    }
    let mut plane_reader =
        BoundedReader::new(data, plane_chunk.body().start, plane_chunk.body().end)?;
    let geometry = read_plane_surface_with_parameterization(&mut plane_reader, scale)?;
    plane_reader.skip_remaining()?;
    payload.skip(plane_chunk.next_offset() - payload.position())?;
    read_clipping_plane(data, &mut payload, archive)?;
    payload.skip_remaining()?;
    reader.skip(outer.next_offset() - reader.position())?;
    Ok(DecodedSurface::Typed {
        geometry,
        derived: scale != MillimeterScale::IDENTITY,
    })
}

fn read_clipping_plane(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(), GeometryError> {
    const ANONYMOUS: u32 = 0x4000_8000;
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(error(chunk.header_start, "invalid clipping-plane chunk"));
    }
    let mut payload = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    if payload.i32()? != 1 {
        return Err(GeometryError::unsupported(
            chunk.body().start,
            "unsupported clipping-plane major version",
        ));
    }
    let minor = payload.i32()?;
    if minor < 0 {
        return Err(GeometryError::unsupported(
            chunk.body().start,
            "unsupported clipping-plane minor version",
        ));
    }
    let _first_viewport = Uuid::from_wire(payload.array()?);
    let _plane_id = Uuid::from_wire(payload.array()?);
    let native_plane = plane(&mut payload)?;
    validate_plane(native_plane, payload.position())?;
    let _enabled = payload.bool()?;
    if minor != 0 {
        read_uuid_list(data, &mut payload, archive)?;
    }
    if minor >= 2 {
        let depth = payload.f64()?;
        if !depth.is_finite() {
            return Err(error(payload.position() - 8, "invalid clipping depth"));
        }
    }
    if minor >= 4 {
        payload.bool()?;
    }
    if minor >= 5 {
        read_clipping_participation(&mut payload)?;
    }
    payload.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(())
}

fn read_uuid_list(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(), GeometryError> {
    const ANONYMOUS: u32 = 0x4000_8000;
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(error(chunk.header_start, "invalid clipping viewport list"));
    }
    let mut payload = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let version = (payload.i32()?, payload.i32()?);
    if version.0 != 1 || version.1 < 0 {
        return Err(GeometryError::unsupported(
            chunk.body().start,
            "unsupported clipping viewport-list version",
        ));
    }
    let count = crate::wire::element_count(&mut payload, 16)?;
    payload.skip(count * 16)?;
    payload.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(())
}

fn read_clipping_participation(reader: &mut BoundedReader<'_>) -> Result<(), GeometryError> {
    let mut item = reader.u8()?;
    if item == 10 {
        let count = crate::wire::element_count(reader, 16)?;
        reader.skip(count * 16)?;
        item = reader.u8()?;
    }
    if item == 11 {
        let count = crate::wire::element_count(reader, 4)?;
        reader.skip(count * 4)?;
        item = reader.u8()?;
    }
    if item == 12 {
        reader.bool()?;
        item = reader.u8()?;
    }
    if item == 13 {
        reader.bool()?;
        item = reader.u8()?;
    }
    if item >= 14 {
        return Ok(());
    }
    if item != 0 {
        return Err(error(
            reader.position() - 1,
            "clipping participation item is invalid or out of order",
        ));
    }
    Ok(())
}

fn read_revolution(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedSurface, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    let major = version >> 4;
    if !(major == 1 || major == 2) {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported revolution-surface version",
        ));
    }
    let from = crate::wire::scaled_point(point(reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled revolution axis is invalid"))?
        .get();
    let to = crate::wire::scaled_point(point(reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled revolution axis is invalid"))?
        .get();
    let angular_interval =
        increasing_interval(interval(reader)?.0, reader.position(), "revolution angle")?;
    if angular_interval[1] - angular_interval[0] > TAU + EPS_SURFACE_DEGENERATE {
        return Err(error(
            reader.position(),
            "revolution angle span exceeds one turn",
        ));
    }
    let parameter_interval = if major >= 2 {
        increasing_interval(
            interval(reader)?.0,
            reader.position(),
            "revolution parameter interval",
        )?
    } else {
        angular_interval
    };
    bbox(reader)?;
    let transposed = match reader.i32()? {
        0 => false,
        1 => true,
        _ => {
            return Err(error(
                reader.position(),
                "revolution transpose flag is invalid",
            ))
        }
    };
    if reader.u8()? != 1 {
        return Err(error(
            reader.position(),
            "revolution profile presence flag is invalid",
        ));
    }
    let axis_delta = Vector3::new(to.x - from.x, to.y - from.y, to.z - from.z);
    let axis_length = axis_delta.norm();
    if !axis_length.is_finite() || axis_length <= 0.0 {
        return Err(error(reader.position(), "revolution axis is invalid"));
    }
    let axis_direction = Vector3::new(
        axis_delta.x / axis_length,
        axis_delta.y / axis_length,
        axis_delta.z / axis_length,
    );
    let child = decode_embedded_curve(ctx, data, reader, scale, archive, depth + 1)?;
    let profile = exact_nurbs(&child, version_offset)?;
    let geometry = revolution_nurbs(
        ctx,
        &profile,
        from,
        axis_direction,
        RevolutionIntervals {
            angle: angular_interval,
            parameter: parameter_interval,
        },
        transposed,
        version_offset,
    )?;
    reader.skip_remaining()?;
    Ok(DecodedSurface::Procedural {
        geometry,
        definition: DecodedProceduralSurface::Revolution {
            children: Box::new([child]),
            axis_origin: from,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
        },
    })
}

fn read_sum(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedSurface, GeometryError> {
    let version_offset = reader.position();
    if reader.u8()? >> 4 != 1 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported sum-surface version",
        ));
    }
    let native = native_vector(reader)?;
    let basepoint = Vector3::new(
        crate::wire::scaled_coordinate(native.0[0], scale)
            .ok_or_else(|| error(reader.position(), "scaled sum basepoint is invalid"))?
            .get(),
        crate::wire::scaled_coordinate(native.0[1], scale)
            .ok_or_else(|| error(reader.position(), "scaled sum basepoint is invalid"))?
            .get(),
        crate::wire::scaled_coordinate(native.0[2], scale)
            .ok_or_else(|| error(reader.position(), "scaled sum basepoint is invalid"))?
            .get(),
    );
    bbox(reader)?;
    let first = decode_embedded_curve(ctx, data, reader, scale, archive, depth + 1)?;
    let second = decode_embedded_curve(ctx, data, reader, scale, archive, depth + 1)?;
    let first_nurbs = exact_nurbs(&first, version_offset)?;
    let second_nurbs = exact_nurbs(&second, version_offset)?;
    let geometry = sum_nurbs(ctx, &first_nurbs, &second_nurbs, basepoint, version_offset)?;
    reader.skip_remaining()?;
    Ok(DecodedSurface::Procedural {
        geometry,
        definition: DecodedProceduralSurface::Sum {
            children: Box::new([first, second]),
            basepoint,
        },
    })
}

#[derive(Clone, Copy)]
struct RevolutionIntervals {
    angle: [f64; 2],
    parameter: [f64; 2],
}

fn revolution_nurbs(
    ctx: &DecodeContext<'_>,
    profile: &NurbsCurve,
    axis_origin: Point3,
    axis: Vector3,
    intervals: RevolutionIntervals,
    transposed: bool,
    offset: usize,
) -> Result<NurbsSurface, GeometryError> {
    let RevolutionIntervals { angle, parameter } = intervals;
    let span_count = ((angle[1] - angle[0]) / FRAC_PI_2).ceil().max(1.0) as usize;
    let angular_count = span_count
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| {
            GeometryError::not_implemented("revolution control count exceeds address space")
        })?;
    let profile_count = profile.pole_count();
    let output_count = angular_count.checked_mul(profile_count).ok_or_else(|| {
        GeometryError::not_implemented("revolution control count exceeds address space")
    })?;
    let knot_count = angular_count.checked_add(3).ok_or_else(|| {
        GeometryError::not_implemented("revolution knot count exceeds address space")
    })?;
    let temp_items = angular_count
        .checked_add(knot_count)
        .and_then(|count| count.checked_add(profile_count.checked_mul(2)?))
        .and_then(|count| count.checked_add(output_count.checked_mul(2)?))
        .ok_or_else(|| {
            GeometryError::not_implemented("revolution temporary count exceeds address space")
        })?;
    ctx.charge_collection_items(
        u64::try_from(temp_items).map_err(|_| {
            GeometryError::not_implemented("revolution temporary count exceeds address space")
        })?,
        "Rhino revolution temporary lanes",
    )?;
    let temporary_bytes = angular_count
        .checked_mul(std::mem::size_of::<(f64, f64)>())
        .and_then(|bytes| bytes.checked_add(knot_count.checked_mul(std::mem::size_of::<f64>())?))
        .and_then(|bytes| {
            bytes.checked_add(
                profile_count.checked_mul(
                    std::mem::size_of::<FinitePoint3>() + std::mem::size_of::<f64>(),
                )?,
            )
        })
        .and_then(|bytes| {
            bytes.checked_add(
                output_count
                    .checked_mul(std::mem::size_of::<Point3>() + std::mem::size_of::<f64>())?,
            )
        })
        .ok_or_else(|| {
            GeometryError::not_implemented("revolution temporary bytes exceed address space")
        })?;
    let temporary_bytes = u64::try_from(temporary_bytes).map_err(|_| {
        GeometryError::not_implemented("revolution temporary bytes exceed address space")
    })?;
    let _temporary = ctx.reserve_scoped(temporary_bytes, "Rhino revolution temporary lanes")?;
    let angle_step = (angle[1] - angle[0]) / span_count as f64;
    let parameter_step = (parameter[1] - parameter[0]) / span_count as f64;
    let parameter_at = |span: usize| -> Result<f64, GeometryError> {
        if parameter_step.is_finite() {
            return Ok(parameter[0] + parameter_step * span as f64);
        }
        cadmpeg_ir::math::interpolate(parameter[0], parameter[1], span as f64 / span_count as f64)
            .map(FiniteReal::get)
            .ok_or_else(|| error(offset, "revolution parameter interval is invalid"))
    };
    let mut angular = Vec::new();
    angular.try_reserve_exact(angular_count).map_err(|_| {
        temporary_allocation_failed("Rhino revolution angular controls", temporary_bytes)
    })?;
    let mut knots = Vec::new();
    knots.try_reserve_exact(knot_count).map_err(|_| {
        temporary_allocation_failed("Rhino revolution angular knots", temporary_bytes)
    })?;
    for span in 0..span_count {
        let a0 = angle[0] + angle_step * span as f64;
        let a1 = angle[0] + angle_step * (span + 1) as f64;
        let middle = (a0 + a1) * 0.5;
        let middle_weight = ((a1 - a0) * 0.5).cos();
        if span == 0 {
            angular.push((a0, 1.0));
        }
        angular.push((middle, middle_weight));
        angular.push((a1, 1.0));
        let t0 = parameter_at(span)?;
        let t1 = parameter_at(span + 1)?;
        if span == 0 {
            knots.extend([t0, t0, t0]);
        } else {
            knots.extend([t0, t0]);
        }
        if span + 1 == span_count {
            knots.extend([t1, t1, t1]);
        }
    }
    let profile_points = profile.control_points();
    let profile_weights = match profile.pole_rows().weights() {
        Some(weights) => weights,
        None => alloc_filled(profile_count, 1.0, "Rhino revolution profile weights")?,
    };
    let mut control_points = Vec::new();
    control_points
        .try_reserve_exact(output_count)
        .map_err(|_| {
            temporary_allocation_failed("Rhino revolution control points", temporary_bytes)
        })?;
    let mut weights = Vec::new();
    weights
        .try_reserve_exact(output_count)
        .map_err(|_| temporary_allocation_failed("Rhino revolution weights", temporary_bytes))?;
    for (theta, angular_weight) in angular {
        let radial_scale = 1.0 / angular_weight;
        for (profile_point, profile_weight) in
            profile_points.iter().zip(profile_weights.iter().copied())
        {
            let relative = Vector3::new(
                profile_point.x - axis_origin.x,
                profile_point.y - axis_origin.y,
                profile_point.z - axis_origin.z,
            );
            let axial_length = relative.dot(axis);
            let axial = axis.scale(axial_length);
            let radial = relative - axial;
            let point = if radial.dot(radial) <= 1.0e-24 {
                // A pole row is one exact axis point for every angular control point.
                axis_origin.translated(axial, 1.0)
            } else {
                let rotated = rodrigues(radial, axis, theta);
                axis_origin.translated(axial + rotated.scale(radial_scale), 1.0)
            };
            control_points.push(point);
            weights.push(profile_weight * angular_weight);
        }
    }
    let row_len = profile_count;
    let point_rows = copy_rows(ctx, &control_points, row_len, "Rhino revolution pole grid")?;
    let weight_rows = copy_rows(ctx, &weights, row_len, "Rhino revolution weight grid")?;
    ctx.charge_retained(
        u64::try_from(knots.len().checked_mul(8).ok_or_else(|| {
            GeometryError::not_implemented("revolution knot bytes exceed address space")
        })?)
        .map_err(|_| {
            GeometryError::not_implemented("revolution knot bytes exceed address space")
        })?,
        "Rhino revolution angular knots",
    )?;
    let profile_knots = copy_axis_knots(ctx, profile.knots(), "Rhino revolution profile knots")?;
    admit_nurbs_pole_conversion(ctx, output_count, true)?;
    let mut result = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(2, knots, false),
        NurbsSurfaceAxis::new(profile.degree(), profile_knots, profile.periodic()),
        NurbsSurfaceLanes::new(point_rows, Some(weight_rows)),
        false,
    )
    .map_err(|error| GeometryError::malformed(offset, error.to_string()))?;
    if transposed {
        result.transpose_parameter_axes();
    }
    Ok(result)
}

fn sum_nurbs(
    ctx: &DecodeContext<'_>,
    first: &NurbsCurve,
    second: &NurbsCurve,
    basepoint: Vector3,
    offset: usize,
) -> Result<NurbsSurface, GeometryError> {
    let u_count = first.pole_count();
    let v_count = second.pole_count();
    let product_count = admit_sum_product(ctx, u_count, v_count)?;
    let first_rational = matches!(first.pole_rows(), NurbsPoles3::Rational { .. });
    let second_rational = matches!(second.pole_rows(), NurbsPoles3::Rational { .. });
    let rational = first_rational || second_rational;
    let input_count = u_count.checked_add(v_count).ok_or_else(|| {
        GeometryError::not_implemented("sum surface input count exceeds address space")
    })?;
    let temp_items = input_count
        .checked_mul(2)
        .and_then(|count| {
            if rational {
                count.checked_add(product_count)
            } else {
                Some(count)
            }
        })
        .ok_or_else(|| {
            GeometryError::not_implemented("sum surface temporary count exceeds address space")
        })?;
    ctx.charge_collection_items(
        u64::try_from(temp_items).map_err(|_| {
            GeometryError::not_implemented("sum surface temporary count exceeds address space")
        })?,
        "Rhino sum surface temporary lanes",
    )?;
    let input_bytes = input_count
        .checked_mul(std::mem::size_of::<FinitePoint3>() + std::mem::size_of::<f64>())
        .ok_or_else(|| {
            GeometryError::not_implemented("sum surface input bytes exceed address space")
        })?;
    let point_bytes = product_count
        .checked_mul(std::mem::size_of::<Point3>())
        .ok_or_else(|| {
            GeometryError::not_implemented("sum surface point bytes exceed address space")
        })?;
    let weight_bytes = if rational {
        product_count
            .checked_mul(std::mem::size_of::<NonZeroReal>())
            .ok_or_else(|| {
                GeometryError::not_implemented("sum surface weight bytes exceed address space")
            })?
    } else {
        0
    };
    let temporary_bytes = input_bytes
        .checked_add(point_bytes)
        .and_then(|bytes| bytes.checked_add(weight_bytes))
        .ok_or_else(|| {
            GeometryError::not_implemented("sum surface temporary bytes exceed address space")
        })?;
    let temporary_bytes = u64::try_from(temporary_bytes).map_err(|_| {
        GeometryError::not_implemented("sum surface temporary bytes exceed address space")
    })?;
    let _temporary = ctx.reserve_scoped(temporary_bytes, "Rhino sum surface temporary lanes")?;
    let first_points = first.control_points();
    let second_points = second.control_points();
    let first_weights = match first.pole_rows().weights() {
        Some(weights) => weights,
        None => alloc_filled(u_count, 1.0, "Rhino sum-surface first weights")?,
    };
    let second_weights = match second.pole_rows().weights() {
        Some(weights) => weights,
        None => alloc_filled(v_count, 1.0, "Rhino sum-surface second weights")?,
    };
    let mut control_points = Vec::new();
    control_points
        .try_reserve_exact(product_count)
        .map_err(|_| {
            temporary_allocation_failed("Rhino sum surface control points", temporary_bytes)
        })?;
    let mut weights = if rational {
        let mut values = Vec::new();
        values.try_reserve_exact(product_count).map_err(|_| {
            temporary_allocation_failed("Rhino sum surface weights", temporary_bytes)
        })?;
        Some(values)
    } else {
        None
    };
    for (first_point, first_weight) in first_points.iter().zip(first_weights.iter().copied()) {
        for (second_point, second_weight) in
            second_points.iter().zip(second_weights.iter().copied())
        {
            let Some(product) = NonZeroReal::new(first_weight * second_weight) else {
                return Err(error(offset, "sum surface weight is invalid"));
            };
            control_points.push(Point3::new(
                first_point.x + second_point.x + basepoint.x,
                first_point.y + second_point.y + basepoint.y,
                first_point.z + second_point.z + basepoint.z,
            ));
            if let Some(values) = &mut weights {
                values.push(product);
            }
        }
    }
    let row_len = v_count;
    let point_rows = copy_rows(ctx, &control_points, row_len, "Rhino sum surface pole grid")?;
    let weight_rows = weights
        .as_deref()
        .map(|values| copy_rows(ctx, values, row_len, "Rhino sum surface weight grid"))
        .transpose()?;
    let u_knots = copy_axis_knots(ctx, first.knots(), "Rhino sum surface U knots")?;
    let v_knots = copy_axis_knots(ctx, second.knots(), "Rhino sum surface V knots")?;
    admit_nurbs_pole_conversion(ctx, product_count, rational)?;
    NurbsSurface::from_checked_lanes(
        NurbsSurfaceAxis::new(first.degree(), u_knots, first.periodic()),
        NurbsSurfaceAxis::new(second.degree(), v_knots, second.periodic()),
        NurbsSurfaceLanes::new(point_rows, weight_rows),
        false,
    )
    .map_err(|error| GeometryError::malformed(offset, error.to_string()))
}

fn admit_sum_product(
    ctx: &DecodeContext<'_>,
    u_count: usize,
    v_count: usize,
) -> Result<usize, CodecError> {
    let count = u_count.checked_mul(v_count).ok_or_else(|| {
        CodecError::NotImplemented(
            "Rhino sum surface control count exceeds address space".to_string(),
        )
    })?;
    ctx.charge_collection_items(
        u64::try_from(count).map_err(|_| {
            CodecError::NotImplemented(
                "Rhino sum surface control count exceeds address space".to_string(),
            )
        })?,
        "Rhino sum surface control points",
    )?;
    Ok(count)
}

fn temporary_allocation_failed(operation: &'static str, bytes: u64) -> GeometryError {
    GeometryError::Codec(CodecError::ResourceLimit(
        cadmpeg_core::decode::ResourceLimit {
            dimension: cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            reason: cadmpeg_core::decode::ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: bytes,
            operation,
        },
    ))
}

fn copy_rows<T: Clone>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    row_len: usize,
    operation: &'static str,
) -> Result<Vec<Vec<T>>, GeometryError> {
    if row_len == 0 || !values.len().is_multiple_of(row_len) {
        return Err(GeometryError::unpositioned(
            "Rhino surface grid has inconsistent rows",
        ));
    }
    let row_count = values.len() / row_len;
    let item_count = values.len().checked_add(row_count).ok_or_else(|| {
        GeometryError::not_implemented("Rhino surface grid count exceeds address space")
    })?;
    let value_bytes = values
        .len()
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| {
            GeometryError::not_implemented("Rhino surface grid bytes exceed address space")
        })?;
    let header_bytes = row_count
        .checked_mul(std::mem::size_of::<Vec<T>>())
        .ok_or_else(|| {
            GeometryError::not_implemented("Rhino surface grid bytes exceed address space")
        })?;
    let bytes = value_bytes.checked_add(header_bytes).ok_or_else(|| {
        GeometryError::not_implemented("Rhino surface grid bytes exceed address space")
    })?;
    let bytes = u64::try_from(bytes).map_err(|_| {
        GeometryError::not_implemented("Rhino surface grid bytes exceed address space")
    })?;
    ctx.charge_collection_items(
        u64::try_from(item_count).map_err(|_| {
            GeometryError::not_implemented("Rhino surface grid count exceeds address space")
        })?,
        operation,
    )?;
    ctx.charge_retained(bytes, operation)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(row_count)
        .map_err(|_| crate::curves::allocation_failed(operation, bytes))?;
    for source in values.chunks(row_len) {
        let mut row = Vec::new();
        row.try_reserve_exact(source.len())
            .map_err(|_| crate::curves::allocation_failed(operation, bytes))?;
        row.extend_from_slice(source);
        rows.push(row);
    }
    Ok(rows)
}

fn copy_axis_knots(
    ctx: &DecodeContext<'_>,
    source: &[f64],
    operation: &'static str,
) -> Result<Vec<f64>, GeometryError> {
    let count = u64::try_from(source.len()).map_err(|_| {
        GeometryError::not_implemented("Rhino surface knot count exceeds address space")
    })?;
    let bytes = count.checked_mul(8).ok_or_else(|| {
        GeometryError::not_implemented("Rhino surface knot bytes exceed address space")
    })?;
    ctx.charge_collection_items(count, operation)?;
    ctx.charge_retained(bytes, operation)?;
    let mut knots = Vec::new();
    knots
        .try_reserve_exact(source.len())
        .map_err(|_| crate::curves::allocation_failed(operation, bytes))?;
    knots.extend_from_slice(source);
    Ok(knots)
}

fn admit_nurbs_pole_conversion(
    ctx: &DecodeContext<'_>,
    pole_count: usize,
    rational: bool,
) -> Result<(), GeometryError> {
    let count = u64::try_from(pole_count).map_err(|_| {
        GeometryError::not_implemented("Rhino surface pole count exceeds address space")
    })?;
    let item_count = if rational {
        count.checked_mul(2)
    } else {
        Some(count)
    }
    .ok_or_else(|| {
        GeometryError::not_implemented("Rhino surface pole count exceeds address space")
    })?;
    let bytes_per_pole = std::mem::size_of::<FinitePoint3>()
        .checked_add(if rational {
            std::mem::size_of::<NonZeroReal>()
        } else {
            0
        })
        .ok_or_else(|| {
            GeometryError::not_implemented("Rhino surface pole bytes exceed address space")
        })?;
    let bytes = count
        .checked_mul(u64::try_from(bytes_per_pole).map_err(|_| {
            GeometryError::not_implemented("Rhino surface pole bytes exceed address space")
        })?)
        .ok_or_else(|| {
            GeometryError::not_implemented("Rhino surface pole bytes exceed address space")
        })?;
    ctx.charge_collection_items(item_count, "Rhino surface admitted poles")?;
    ctx.charge_retained(bytes, "Rhino surface admitted poles")?;
    Ok(())
}

/// Constructs the exact degree-one tensor interpolation between two profile curves.
pub(crate) fn extrusion_nurbs(
    start: &NurbsCurve,
    end: &NurbsCurve,
    path_domain: [f64; 2],
    transposed: bool,
    offset: usize,
) -> Result<NurbsSurface, GeometryError> {
    if start.degree() != end.degree()
        || start.knots() != end.knots()
        || start.control_points().len() != end.control_points().len()
        || start.weights() != end.weights()
        || start.periodic() != end.periodic()
        || !path_domain.iter().all(|value| value.is_finite())
        || path_domain[0] >= path_domain[1]
    {
        return Err(error(offset, "extrusion tensor inputs are incompatible"));
    }
    let profile_count = start.control_points().len();
    profile_count
        .checked_mul(2)
        .ok_or_else(|| error(offset, "extrusion surface control count overflow"))?;
    let start_points = start.control_points();
    let end_points = end.control_points();
    let start_weights = start.weights();
    let mut control_points = Vec::with_capacity(profile_count * 2);
    let mut weights = start_weights
        .as_ref()
        .map(|_| Vec::with_capacity(profile_count * 2));
    for index in 0..profile_count {
        control_points.push(start_points[index]);
        control_points.push(end_points[index]);
        if let (Some(source), Some(target)) = (&start_weights, &mut weights) {
            target.push(source[index]);
            target.push(source[index]);
        }
    }
    let mut surface = NurbsSurface::from_checked_lanes(
        NurbsSurfaceAxis::new(start.degree(), start.knots().to_vec(), start.periodic()),
        NurbsSurfaceAxis::new(
            1,
            vec![
                path_domain[0],
                path_domain[0],
                path_domain[1],
                path_domain[1],
            ],
            false,
        ),
        NurbsSurfaceLanes::new(
            control_points.chunks(2_usize).map(<[_]>::to_vec).collect(),
            weights.map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    )
    .map_err(|error| GeometryError::malformed(offset, error.to_string()))?;
    if transposed {
        surface.transpose_parameter_axes();
    }
    Ok(surface)
}

fn rodrigues(value: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    let cross = axis.cross(value);
    value.scale(cosine) + cross.scale(sine) + axis.scale(axis.dot(value) * (1.0 - cosine))
}

pub(crate) fn read_nurbs_curve(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<NurbsCurve, GeometryError> {
    read_nurbs_curve_inner(ctx, reader, scale, None)
}

/// Reads a Rhino NURBS curve whose poles are two-dimensional UV values.
pub(crate) fn read_nurbs_curve_2d(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
) -> Result<NurbsCurve, GeometryError> {
    read_nurbs_curve_inner(ctx, reader, MillimeterScale::IDENTITY, Some(2))
}

fn read_nurbs_curve_inner(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    expected_dimension: Option<i32>,
) -> Result<NurbsCurve, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    let major = version >> 4;
    let minor = version & 0x0f;
    if major != 1 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported NURBS curve version",
        ));
    }
    let dimension = reader.i32()?;
    let rational = reader.i32()?;
    let order = checked_positive(reader.i32()?, reader.position(), "curve order")?;
    let cv_count = checked_positive(reader.i32()?, reader.position(), "curve CV count")?;
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    if expected_dimension.is_some_and(|expected| dimension != expected)
        || !(2..=3).contains(&dimension)
        || !(rational == 0 || rational == 1)
        || cv_count < order
    {
        return Err(error(reader.position(), "invalid NURBS curve header"));
    }
    let stored_knot_count = crate::wire::element_count(reader, 8)?;
    let expected_knot_count = order
        .checked_add(cv_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(reader.position(), "NURBS knot count overflow"))?;
    if stored_knot_count != expected_knot_count {
        return Err(error(reader.position(), "NURBS curve knot count mismatch"));
    }
    let knots = read_knots(ctx, reader, stored_knot_count)?;
    validate_stored_domain(&knots, order, cv_count, reader.position())?;
    let stored_cv_count = crate::wire::element_count(reader, (dimension + rational) as usize * 8)?;
    if stored_cv_count != cv_count {
        return Err(error(reader.position(), "NURBS curve CV count mismatch"));
    }
    let (control_points, weights) = read_poles(
        ctx,
        reader,
        stored_cv_count,
        rational != 0,
        dimension,
        scale,
    )?;
    if minor >= 1 {
        reader.bool()?;
    }
    let periodic = periodic_knots(&knots, order, cv_count);
    admit_reconstructed_knots(ctx, stored_knot_count)?;
    let full_knots = reconstruct_knots(&knots, order, cv_count)?;
    reader.skip_remaining()?;
    admit_nurbs_pole_conversion(ctx, stored_cv_count, rational != 0)?;
    NurbsCurve::from_lanes(
        u32::try_from(order - 1).map_err(|_| error(reader.position(), "NURBS order overflow"))?,
        full_knots,
        control_points,
        weights,
        periodic,
    )
    .map_err(|error| GeometryError::malformed(reader.position(), error.to_string()))
}

pub(crate) fn read_nurbs_surface(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<NurbsSurface, GeometryError> {
    let surface = read_nurbs_surface_prefix(ctx, reader, scale)?;
    reader.skip_remaining()?;
    Ok(surface)
}

/// Reads one NURBS surface without consuming bytes after its final pole.
pub(crate) fn read_nurbs_surface_prefix(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<NurbsSurface, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported NURBS surface version",
        ));
    }
    let dimension = reader.i32()?;
    let rational = reader.i32()?;
    let u_order = checked_positive(reader.i32()?, reader.position(), "surface U order")?;
    let v_order = checked_positive(reader.i32()?, reader.position(), "surface V order")?;
    let u_count = checked_positive(reader.i32()?, reader.position(), "surface U CV count")?;
    let v_count = checked_positive(reader.i32()?, reader.position(), "surface V CV count")?;
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    if !(2..=3).contains(&dimension)
        || !(rational == 0 || rational == 1)
        || u_count < u_order
        || v_count < v_order
    {
        return Err(error(reader.position(), "invalid NURBS surface header"));
    }
    let u_knot_count = crate::wire::element_count(reader, 8)?;
    let expected_u = u_order
        .checked_add(u_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(reader.position(), "surface U knot count overflow"))?;
    if u_knot_count != expected_u {
        return Err(error(reader.position(), "surface U knot count mismatch"));
    }
    let u_knots = read_knots(ctx, reader, u_knot_count)?;
    validate_stored_domain(&u_knots, u_order, u_count, reader.position())?;
    let v_knot_count = crate::wire::element_count(reader, 8)?;
    let expected_v = v_order
        .checked_add(v_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(reader.position(), "surface V knot count overflow"))?;
    if v_knot_count != expected_v {
        return Err(error(reader.position(), "surface V knot count mismatch"));
    }
    let v_knots = read_knots(ctx, reader, v_knot_count)?;
    validate_stored_domain(&v_knots, v_order, v_count, reader.position())?;
    let u_periodic = periodic_knots(&u_knots, u_order, u_count);
    let v_periodic = periodic_knots(&v_knots, v_order, v_count);
    let stored_cv_count = crate::wire::element_count(reader, (dimension + rational) as usize * 8)?;
    let expected_cv_count = u_count
        .checked_mul(v_count)
        .ok_or_else(|| error(reader.position(), "surface CV count overflow"))?;
    if stored_cv_count != expected_cv_count {
        return Err(error(reader.position(), "NURBS surface CV count mismatch"));
    }
    let (control_points, weights) = read_poles(
        ctx,
        reader,
        stored_cv_count,
        rational != 0,
        dimension,
        scale,
    )?;
    admit_reconstructed_knots(ctx, u_knot_count)?;
    let u_knots = reconstruct_knots(&u_knots, u_order, u_count)?;
    admit_reconstructed_knots(ctx, v_knot_count)?;
    let v_knots = reconstruct_knots(&v_knots, v_order, v_count)?;
    let row_len = v_count;
    let point_rows = copy_rows(
        ctx,
        &control_points,
        row_len,
        "Rhino NURBS surface pole grid",
    )?;
    let weight_rows = weights
        .as_deref()
        .map(|values| copy_rows(ctx, values, row_len, "Rhino NURBS surface weight grid"))
        .transpose()?;
    admit_nurbs_pole_conversion(ctx, stored_cv_count, rational != 0)?;
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(
            u32::try_from(u_order - 1)
                .map_err(|_| error(reader.position(), "surface U order overflow"))?,
            u_knots,
            u_periodic,
        ),
        NurbsSurfaceAxis::new(
            u32::try_from(v_order - 1)
                .map_err(|_| error(reader.position(), "surface V order overflow"))?,
            v_knots,
            v_periodic,
        ),
        NurbsSurfaceLanes::new(point_rows, weight_rows),
        false,
    )
    .map_err(|error| GeometryError::malformed(reader.position(), error.to_string()))
}

fn read_plane_surface_with_parameterization(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<TypedSurface, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported plane-surface version",
        ));
    }
    let native_plane = plane(reader)?;
    validate_plane(native_plane, reader.position())?;
    let domain = increasing_interval(interval(reader)?.0, reader.position(), "plane U domain")?;
    let v_domain = increasing_interval(interval(reader)?.0, reader.position(), "plane V domain")?;
    let (u_extents, v_extents) = if version & 0x0f == 1 {
        (
            increasing_interval(interval(reader)?.0, reader.position(), "plane U extents")?,
            increasing_interval(interval(reader)?.0, reader.position(), "plane V extents")?,
        )
    } else {
        (domain, v_domain)
    };
    let geometry = TypedSurface::Plane {
        plane: cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            crate::wire::scaled_point(native_plane.origin, scale)
                .ok_or_else(|| error(reader.position(), "scaled plane origin is invalid"))?
                .get(),
            vector(native_plane.zaxis),
            vector(native_plane.xaxis),
        )
        .map_err(|message| error(reader.position(), message))?,
        parameterization: PlaneParameterization {
            u_domain: domain,
            v_domain,
            u_extents,
            v_extents,
        },
    };
    reader.skip_remaining()?;
    Ok(geometry)
}

fn map_parameter(value: f64, domain: [f64; 2], extents: [f64; 2]) -> f64 {
    if value == domain[0] {
        return extents[0];
    }
    if value == domain[1] {
        return extents[1];
    }
    let width = domain[1] - domain[0];
    let offset = value - domain[0];
    let fraction = if width.is_finite() && offset.is_finite() {
        offset / width
    } else {
        (0.5 * value - 0.5 * domain[0]) / (0.5 * domain[1] - 0.5 * domain[0])
    };
    let mapped = (1.0 - fraction) * extents[0] + fraction * extents[1];
    if mapped.is_finite() {
        mapped
    } else {
        let fallback = (extents[1] - extents[0]).mul_add(fraction, extents[0]);
        if fallback.is_finite() {
            return fallback;
        }
        if let (Some(source), Some(target), Some(parameter)) = (
            cadmpeg_ir::topology::IncreasingParameterInterval::new(domain),
            cadmpeg_ir::topology::IncreasingParameterInterval::new(extents),
            FiniteReal::new(value),
        ) {
            if let Ok(mapped) = target.map_from(source, parameter, false) {
                return mapped.get();
            }
        }
        fallback
    }
}

fn read_knots(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    count: usize,
) -> Result<Vec<f64>, GeometryError> {
    let count_u64 = u64::try_from(count)
        .map_err(|_| GeometryError::not_implemented("NURBS knot count exceeds address space"))?;
    let bytes = count_u64
        .checked_mul(8)
        .ok_or_else(|| GeometryError::not_implemented("NURBS knot bytes exceed address space"))?;
    ctx.charge_collection_items(count_u64, "Rhino NURBS knots")?;
    ctx.charge_retained(bytes, "Rhino NURBS knots")?;
    let mut knots = Vec::new();
    knots
        .try_reserve_exact(count)
        .map_err(|_| crate::curves::allocation_failed("Rhino NURBS knots", bytes))?;
    for _ in 0..count {
        let knot_offset = reader.position();
        let value = reader.f64()?;
        if !value.is_finite() || knots.last().is_some_and(|last| value < *last) {
            return Err(error(knot_offset, "NURBS knots are invalid"));
        }
        knots.push(value);
    }
    Ok(knots)
}

fn admit_reconstructed_knots(
    ctx: &DecodeContext<'_>,
    stored_count: usize,
) -> Result<(), GeometryError> {
    let count = stored_count.checked_add(2).ok_or_else(|| {
        GeometryError::not_implemented("NURBS reconstructed knot count exceeds address space")
    })?;
    let count = u64::try_from(count).map_err(|_| {
        GeometryError::not_implemented("NURBS reconstructed knot count exceeds address space")
    })?;
    let bytes = count.checked_mul(8).ok_or_else(|| {
        GeometryError::not_implemented("NURBS reconstructed knot bytes exceed address space")
    })?;
    ctx.charge_collection_items(count, "Rhino NURBS reconstructed knots")?;
    ctx.charge_retained(bytes, "Rhino NURBS reconstructed knots")?;
    Ok(())
}

fn read_poles(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    count: usize,
    rational: bool,
    dimension: i32,
    scale: MillimeterScale,
) -> Result<(Vec<Point3>, Option<Vec<f64>>), GeometryError> {
    let count_u64 = u64::try_from(count)
        .map_err(|_| GeometryError::not_implemented("NURBS pole count exceeds address space"))?;
    let point_bytes = count_u64
        .checked_mul(
            u64::try_from(std::mem::size_of::<Point3>()).map_err(|_| {
                GeometryError::not_implemented("NURBS pole bytes exceed address space")
            })?,
        )
        .ok_or_else(|| GeometryError::not_implemented("NURBS pole bytes exceed address space"))?;
    ctx.charge_collection_items(count_u64, "Rhino NURBS poles")?;
    ctx.charge_retained(point_bytes, "Rhino NURBS poles")?;
    let mut points = Vec::new();
    points
        .try_reserve_exact(count)
        .map_err(|_| crate::curves::allocation_failed("Rhino NURBS poles", point_bytes))?;
    let mut weights = if rational {
        let weight_bytes = count_u64.checked_mul(8).ok_or_else(|| {
            GeometryError::not_implemented("NURBS weight bytes exceed address space")
        })?;
        ctx.charge_collection_items(count_u64, "Rhino NURBS weights")?;
        ctx.charge_retained(weight_bytes, "Rhino NURBS weights")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| crate::curves::allocation_failed("Rhino NURBS weights", weight_bytes))?;
        Some(values)
    } else {
        None
    };
    for _ in 0..count {
        let pole_offset = reader.position();
        let x = reader.f64()?;
        let y = reader.f64()?;
        let z = if dimension == 3 { reader.f64()? } else { 0.0 };
        let weight = weights
            .as_mut()
            .map(|target| reader.f64().map(|weight| (target, weight)))
            .transpose()?;
        let [Some(x), Some(y), Some(z)] = [x, y, z].map(FiniteReal::new) else {
            return Err(error(pole_offset, "NURBS pole is not finite"));
        };
        let weight = if let Some((target, weight)) = weight {
            let Some(weight) = NonZeroReal::new(weight) else {
                return Err(error(reader.position(), "NURBS weight is invalid"));
            };
            target.push(weight.get());
            FiniteReal::from(weight)
        } else {
            FiniteReal::ONE
        };
        let coordinate = |value| {
            cadmpeg_ir::math::multiply_divide(value, scale.real(), weight)
                .ok_or_else(|| error(pole_offset, "scaled NURBS pole is invalid"))
        };
        points.push(Point3::new(
            coordinate(x)?.get(),
            coordinate(y)?.get(),
            coordinate(z)?.get(),
        ));
    }
    Ok((points, weights))
}

/// Reconstruct the omitted endpoints. The indexes below are zero-based.
pub(crate) fn reconstruct_knots(
    knots: &[f64],
    order: usize,
    cv_count: usize,
) -> Result<Vec<f64>, GeometryError> {
    let m = order
        .checked_add(cv_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| GeometryError::unpositioned("NURBS knot arithmetic overflow"))?;
    if knots.len() != m || order < 2 || cv_count < order {
        return Err(GeometryError::unpositioned(
            "NURBS knot reconstruction input is invalid",
        ));
    }
    let mut start = knots[0];
    if order > 2 && cv_count >= 2 * order - 2 && cv_count >= 6 && knots[0] < knots[order - 2] {
        start = knots[0] - (knots[cv_count - order + 1] - knots[cv_count - order]);
    }
    let mut end = knots[m - 1];
    if order > 2 && cv_count >= 2 * order - 2 && cv_count >= 6 && knots[cv_count - 1] < knots[m - 1]
    {
        end = knots[m - 1] + (knots[order + 1] - knots[order]);
    }
    if !start.is_finite() || !end.is_finite() || start > knots[0] || end < knots[m - 1] {
        return Err(GeometryError::unpositioned(
            "NURBS reconstructed knots are invalid",
        ));
    }
    let capacity = order.checked_add(cv_count).ok_or_else(|| {
        GeometryError::not_implemented("NURBS reconstructed knot count exceeds address space")
    })?;
    let allocation_bytes = capacity
        .checked_mul(8)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| {
            GeometryError::not_implemented("NURBS reconstructed knot bytes exceed address space")
        })?;
    let mut result = Vec::new();
    result.try_reserve_exact(capacity).map_err(|_| {
        crate::curves::allocation_failed("Rhino NURBS reconstructed knots", allocation_bytes)
    })?;
    result.push(start);
    result.extend_from_slice(knots);
    result.push(end);
    Ok(result)
}

pub(crate) fn periodic_knots(knots: &[f64], order: usize, cv_count: usize) -> bool {
    // This is ON_IsKnotVectorPeriodic over the stored, zero-based knot array.
    if order < 3 || cv_count < order || (order <= 4 && cv_count < order + 2) {
        return false;
    }
    if order > 4 && cv_count < 2 * order - 2 {
        return false;
    }
    let scale = knots
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if scale == 0.0 || !scale.is_finite() || knots.iter().any(|knot| !knot.is_finite()) {
        return false;
    }
    let knot = |index: usize| knots[index] / scale;
    let mut tolerance = (knot(order - 1) - knot(order - 3)).abs() * f64::EPSILON.sqrt();
    tolerance = tolerance.max((knot(cv_count - 1) - knot(order - 2)).abs() * f64::EPSILON.sqrt());
    let mut paired = 2 * (order - 2);
    let mut index = 0;
    let mut other = cv_count - order + 1;
    while paired > 0 {
        if ((knot(index + 1) - knot(index)) + (knot(other) - knot(other + 1))).abs() > tolerance {
            return false;
        }
        index += 1;
        other += 1;
        paired -= 1;
    }
    true
}

fn validate_stored_domain(
    knots: &[f64],
    order: usize,
    cv_count: usize,
    offset: usize,
) -> Result<(), GeometryError> {
    if knots[order - 2] < knots[cv_count - 1] {
        Ok(())
    } else {
        Err(error(offset, "NURBS native domain is not increasing"))
    }
}

fn checked_positive(value: i32, offset: usize, label: &str) -> Result<usize, GeometryError> {
    if value < 2 && label.ends_with("order") || value <= 0 {
        return Err(error(offset, label));
    }
    usize::try_from(value).map_err(|_| error(offset, label))
}

fn increasing_interval(
    value: FiniteVector<2>,
    offset: usize,
    label: &str,
) -> Result<[f64; 2], GeometryError> {
    let value = value.get();
    if value[0] < value[1] {
        Ok(value)
    } else {
        Err(error(offset, label))
    }
}

fn validate_plane(value: Plane, offset: usize) -> Result<(), GeometryError> {
    let x = vector(value.xaxis);
    let y = vector(value.yaxis);
    let z = vector(value.zaxis);
    if ![value.origin[0], value.origin[1], value.origin[2]]
        .into_iter()
        .chain(value.equation)
        .all(f64::is_finite)
        || (x.norm() - 1.0).abs() > EPS_SURFACE_DEGENERATE
        || (y.norm() - 1.0).abs() > EPS_SURFACE_DEGENERATE
        || (z.norm() - 1.0).abs() > EPS_SURFACE_DEGENERATE
        || x.dot(y).abs() > EPS_SURFACE_DEGENERATE
        || x.dot(z).abs() > EPS_SURFACE_DEGENERATE
        || y.dot(z).abs() > EPS_SURFACE_DEGENERATE
        || !crate::wire::close_vector(x.cross(y), z, EPS_SURFACE_DEGENERATE)
    {
        return Err(error(
            offset,
            "plane frame is not orthonormal and right-handed",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
