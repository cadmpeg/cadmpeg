// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino NURBS and plane-surface payload decoding.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{
        KnotVector, NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
        NurbsSurfaceLanes,
    },
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_ir::units::{FiniteVector, OrthonormalFrame3, UnitVector3};

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
        axis_origin: FinitePoint3,
        /// Unit axis direction.
        axis_direction: Option<UnitVector3>,
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
        reject_payload: impl FnOnce(cadmpeg_ir::geometry::ProceduralGeometryError) -> Result<E, E>,
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
                    axis_direction
                    .ok_or(cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
                        "revolution axis_origin and axis_direction must be finite, with unit axis_direction",
                    ))
                    .and_then(|axis_direction| {
                        cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                            directrix,
                            (axis_origin, axis_direction),
                            angular_interval,
                            None,
                            Some(parameter_interval),
                            transposed,
                            cadmpeg_ir::geometry::CacheContract::from_form(None),
                        )
                    })
                    .or_else(|error| Err(reject_payload(error)?))?,
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
                    .or_else(|error| Err(reject_payload(error)?))?,
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
        let geometry = read_plane_surface_with_parameterization(ctx, &mut reader, scale)?;
        DecodedSurface::Typed {
            geometry,
            derived: scale != MillimeterScale::IDENTITY,
        }
    } else if class == CLIPPING_PLANE_SURFACE {
        read_clipping_plane_surface(ctx, data, &mut reader, scale, archive)?
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let geometry = read_plane_surface_with_parameterization(ctx, &mut plane_reader, scale)?;
    plane_reader.skip_remaining()?;
    payload.skip(plane_chunk.next_offset() - payload.position())?;
    read_clipping_plane(ctx, data, &mut payload, archive)?;
    payload.skip_remaining()?;
    reader.skip(outer.next_offset() - reader.position())?;
    Ok(DecodedSurface::Typed {
        geometry,
        derived: scale != MillimeterScale::IDENTITY,
    })
}

fn read_clipping_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let native_plane = plane(ctx, &mut payload)?;
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
    let from = crate::wire::scaled_point(point(ctx, reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled revolution axis is invalid"))?;
    let to = crate::wire::scaled_point(point(ctx, reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled revolution axis is invalid"))?
        .get();
    let angular_interval = increasing_interval(
        ctx,
        interval(ctx, reader)?.0,
        reader.position(),
        "revolution angle",
    )?;
    if angular_interval[1] - angular_interval[0] > TAU + EPS_SURFACE_DEGENERATE {
        return Err(error(
            reader.position(),
            "revolution angle span exceeds one turn",
        ));
    }
    let parameter_interval = if major >= 2 {
        increasing_interval(
            ctx,
            interval(ctx, reader)?.0,
            reader.position(),
            "revolution parameter interval",
        )?
    } else {
        angular_interval
    };
    bbox(ctx, reader)?;
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
    let axis_direction =
        UnitVector3::normalized_with_length(axis_delta).map(|(direction, _)| direction);
    let raw_axis_direction = Vector3::new(
        axis_delta.x / axis_length,
        axis_delta.y / axis_length,
        axis_delta.z / axis_length,
    );
    let child = decode_embedded_curve(ctx, data, reader, scale, archive, depth + 1)?;
    let mut profile_storage = ctx.reserve_scoped(0, "Rhino revolution source NURBS")?;
    let profile = profile_storage.with_storage(|| exact_nurbs(ctx, &child, version_offset))?;
    let geometry = revolution_nurbs(
        ctx,
        &profile,
        from.get(),
        raw_axis_direction,
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
    let native = native_vector(ctx, reader)?;
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
    bbox(ctx, reader)?;
    let first = decode_embedded_curve(ctx, data, reader, scale, archive, depth + 1)?;
    let second = decode_embedded_curve(ctx, data, reader, scale, archive, depth + 1)?;
    let mut profile_storage = ctx.reserve_scoped(0, "Rhino sum source NURBS")?;
    let first_nurbs = profile_storage.with_storage(|| exact_nurbs(ctx, &first, version_offset))?;
    let second_nurbs =
        profile_storage.with_storage(|| exact_nurbs(ctx, &second, version_offset))?;
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
    let span_count = cadmpeg_core::convert::truncate_f64_to_usize(
        ((angle[1] - angle[0]) / FRAC_PI_2).ceil().max(1.0),
    )
    .ok_or_else(|| error(offset, "revolution span count exceeds address space"))?;
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
    let mut temporary = ctx.reserve_scoped(0, "Rhino revolution temporary lanes")?;
    let angle_step = (angle[1] - angle[0])
        / cadmpeg_core::convert::f64_from_index(span_count)
            .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
    let parameter_step = (parameter[1] - parameter[0])
        / cadmpeg_core::convert::f64_from_index(span_count)
            .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
    let parameter_at = |span: usize| -> Result<f64, GeometryError> {
        if parameter_step.is_finite() {
            return Ok(parameter[0]
                + parameter_step
                    * cadmpeg_core::convert::f64_from_index(span).ok_or_else(|| {
                        error(offset, "geometry index exceeds exact float range")
                    })?);
        }
        cadmpeg_ir::math::interpolate(
            parameter[0],
            parameter[1],
            cadmpeg_core::convert::f64_from_index(span)
                .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?
                / cadmpeg_core::convert::f64_from_index(span_count)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?,
        )
        .map(FiniteReal::get)
        .ok_or_else(|| error(offset, "revolution parameter interval is invalid"))
    };
    let mut angular = temporary
        .with_storage(|| ctx.collection_vec(angular_count, "Rhino revolution temporary lanes"))?;
    let mut knots = Vec::new();
    ctx.reserve_vec(&mut knots, knot_count, "Rhino revolution angular knots")?;
    for span in 0..span_count {
        let a0 = angle[0]
            + angle_step
                * cadmpeg_core::convert::f64_from_index(span)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
        let a1 = angle[0]
            + angle_step
                * cadmpeg_core::convert::f64_from_index(span + 1)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
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
    let mut control_points = temporary
        .with_storage(|| ctx.collection_vec(output_count, "Rhino revolution temporary lanes"))?;
    let mut weights = temporary
        .with_storage(|| ctx.collection_vec(output_count, "Rhino revolution temporary lanes"))?;
    for (theta, angular_weight) in ctx
        .admit_iter(angular, "Rhino revolution angular traversal")
        .map_err(CodecError::from)?
    {
        let radial_scale = 1.0 / angular_weight;
        for index in ctx
            .admit_iter(0..profile_count, "Rhino revolution nurbs traversal")
            .map_err(CodecError::from)?
        {
            let profile_point = profile
                .pole_rows()
                .point_at(index)
                .ok_or_else(|| error(offset, "revolution profile pole is absent"))?;
            let profile_weight = profile.pole_rows().weight_at(index).unwrap_or(1.0);
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
    let point_rows = temporary
        .with_storage(|| copy_rows(ctx, &control_points, row_len, "Rhino revolution pole grid"))?;
    let weight_rows = temporary
        .with_storage(|| copy_rows(ctx, &weights, row_len, "Rhino revolution weight grid"))?;
    let profile_knots = ctx
        .copy_slice(profile.knots(), "Rhino revolution profile knots")
        .map_err(crate::curves::GeometryError::from)?;
    let mut result = cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
        ctx,
        NurbsSurfaceAxis::new(2, knots, false),
        NurbsSurfaceAxis::new(profile.degree(), profile_knots, profile.periodic()),
        NurbsSurfaceLanes::new(point_rows, Some(weight_rows)),
        false,
    )
    .map_err(GeometryError::from)?
    .or_else(|error| {
        Err(GeometryError::malformed(
            offset,
            ctx.format_retained(format_args!("{error}"), "Rhino revolution_nurbs text")?,
        ))
    })?;
    if transposed {
        result.transpose_parameter_axes(ctx)?;
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
    let product_count = u_count.checked_mul(v_count).ok_or_else(|| {
        GeometryError::not_implemented("Rhino sum surface control count exceeds address space")
    })?;
    let first_rational = matches!(first.pole_rows(), NurbsPoles3::Rational { .. });
    let second_rational = matches!(second.pole_rows(), NurbsPoles3::Rational { .. });
    let rational = first_rational || second_rational;
    let mut temporary = ctx.reserve_scoped(0, "Rhino sum surface temporary lanes")?;
    let mut control_points = temporary
        .with_storage(|| ctx.collection_vec(product_count, "Rhino sum surface temporary lanes"))?;
    let mut weights = if rational {
        let values = temporary.with_storage(|| {
            ctx.collection_vec(product_count, "Rhino sum surface temporary lanes")
        })?;
        Some(values)
    } else {
        None
    };
    for first_index in 0..u_count {
        ctx.charge_work(1, "Rhino sum nurbs traversal")?;
        let first_point = first
            .pole_rows()
            .point_at(first_index)
            .ok_or_else(|| error(offset, "first sum profile pole is absent"))?;
        let first_weight = first.pole_rows().weight_at(first_index).unwrap_or(1.0);
        for second_index in 0..v_count {
            ctx.charge_work(1, "Rhino sum nurbs traversal")?;
            let second_point = second
                .pole_rows()
                .point_at(second_index)
                .ok_or_else(|| error(offset, "second sum profile pole is absent"))?;
            let second_weight = second.pole_rows().weight_at(second_index).unwrap_or(1.0);
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
    let point_rows = temporary
        .with_storage(|| copy_rows(ctx, &control_points, row_len, "Rhino sum surface pole grid"))?;
    let weight_rows = weights
        .as_deref()
        .map(|values| {
            temporary
                .with_storage(|| copy_rows(ctx, values, row_len, "Rhino sum surface weight grid"))
        })
        .transpose()?;
    let u_knots = first
        .knots()
        .try_clone_for_decode(ctx, "Rhino sum surface U knots")
        .map_err(crate::curves::GeometryError::from)?;
    let v_knots = second
        .knots()
        .try_clone_for_decode(ctx, "Rhino sum surface V knots")
        .map_err(crate::curves::GeometryError::from)?;
    NurbsSurface::from_checked_lanes(
        ctx,
        NurbsSurfaceAxis::new(first.degree(), u_knots, first.periodic()),
        NurbsSurfaceAxis::new(second.degree(), v_knots, second.periodic()),
        NurbsSurfaceLanes::new(point_rows, weight_rows),
        false,
    )?
    .or_else(|error| {
        Err(GeometryError::malformed(
            offset,
            ctx.format_retained(format_args!("{error}"), "Rhino sum_nurbs text")?,
        ))
    })
}

fn copy_rows<T: Copy>(
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
    let rows = ctx.copy_rows(values, row_len, operation, operation)?;
    Ok(rows)
}

/// Constructs the exact degree-one tensor interpolation between two profile curves.
pub(crate) fn extrusion_nurbs(
    ctx: &DecodeContext<'_>,
    start: &NurbsCurve,
    end: &NurbsCurve,
    path_domain: FiniteVector<2>,
    transposed: bool,
    offset: usize,
) -> Result<NurbsSurface, GeometryError> {
    if start.degree() != end.degree()
        || !ctx.equal(
            start.knots().as_slice(),
            end.knots().as_slice(),
            "Rhino extrusion knot equality",
        )?
        || start.pole_count() != end.pole_count()
        || !matching_pole_weights(ctx, start.pole_rows(), end.pole_rows())?
        || start.periodic() != end.periodic()
        || path_domain[0] >= path_domain[1]
    {
        return Err(error(offset, "extrusion tensor inputs are incompatible"));
    }
    let profile_count = start.pole_count();
    profile_count
        .checked_mul(2)
        .ok_or_else(|| error(offset, "extrusion surface control count overflow"))?;
    let poles = match (start.pole_rows(), end.pole_rows()) {
        (
            NurbsPoles3::Polynomial {
                points: start_points,
            },
            NurbsPoles3::Polynomial { points: end_points },
        ) => NurbsPoleGrid::Polynomial {
            rows: extrusion_rows(ctx, start_points, end_points)?,
        },
        (
            NurbsPoles3::Rational {
                points: start_points,
            },
            NurbsPoles3::Rational { points: end_points },
        ) => NurbsPoleGrid::Rational {
            rows: extrusion_rows(ctx, start_points, end_points)?,
        },
        _ => return Err(error(offset, "extrusion tensor inputs are incompatible")),
    };
    let u_knots = start
        .knots()
        .try_clone_for_decode(ctx, "Rhino extrusion surface knots")?;
    let [path_start, path_end] = path_domain.finite_components();
    let path_knots =
        KnotVector::from_finite_lanes(ctx, vec![path_start, path_start, path_end, path_end])?
            .or_else(|error| {
                Err(GeometryError::malformed(
                    offset,
                    ctx.format_retained(format_args!("{error}"), "Rhino extrusion_nurbs text")?,
                ))
            })?;
    let mut surface = NurbsSurface::new(
        ctx,
        NurbsSurfaceAxis::new(start.degree(), u_knots, start.periodic()),
        NurbsSurfaceAxis::new(1, path_knots, false),
        poles,
        false,
    )?
    .or_else(|error| {
        Err(GeometryError::malformed(
            offset,
            ctx.format_retained(format_args!("{error}"), "Rhino extrusion_nurbs text")?,
        ))
    })?;
    if transposed {
        surface.transpose_parameter_axes(ctx)?;
    }
    Ok(surface)
}

fn matching_pole_weights(
    ctx: &DecodeContext<'_>,
    start: &NurbsPoles3<FinitePoint3>,
    end: &NurbsPoles3<FinitePoint3>,
) -> Result<bool, CodecError> {
    Ok(match (start, end) {
        (NurbsPoles3::Polynomial { .. }, NurbsPoles3::Polynomial { .. }) => true,
        (NurbsPoles3::Rational { points: start }, NurbsPoles3::Rational { points: end }) => ctx
            .all_by(
                start.iter().zip(end.iter()),
                |(first, second)| Ok(first.weight == second.weight),
                "Rhino extrusion start pole weights",
            )?,
        _ => false,
    })
}

fn extrusion_rows<T: Copy>(
    ctx: &DecodeContext<'_>,
    start: &[T],
    end: &[T],
) -> Result<Vec<Vec<T>>, GeometryError> {
    let operation = "Rhino extrusion surface rows";
    let row_count = start.len();
    row_count.checked_mul(3).ok_or_else(|| {
        GeometryError::not_implemented("Rhino extrusion surface row count exceeds address space")
    })?;
    ctx.charge_collection_items(cadmpeg_core::decode::u64_from_index(row_count), operation)?;
    let mut rows = Vec::new();
    ctx.reserve_capacity(&mut rows, row_count, operation)?;
    let mut points = start.iter().copied().zip(end.iter().copied());
    for _ in 0..row_count {
        let (first, second) = ctx
            .next_charged(&mut points, "Rhino extrusion rows traversal")?
            .ok_or_else(|| GeometryError::unpositioned("extrusion row source ended early"))?;
        ctx.charge_collection_items(2, operation)?;
        let mut row = Vec::new();
        ctx.reserve_capacity(&mut row, 2, operation)?;
        row.push(first);
        row.push(second);
        rows.push(row);
    }
    Ok(rows)
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
    let order = checked_positive(ctx, reader.i32()?, reader.position(), "curve order")?;
    let cv_count = checked_positive(ctx, reader.i32()?, reader.position(), "curve CV count")?;
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
    let mut knots_storage = ctx.reserve_scoped(0, "Rhino NURBS stored knot scratch")?;
    let knots = knots_storage.with_storage(|| read_knots(ctx, reader, stored_knot_count))?;
    validate_stored_domain(&knots, order, cv_count, reader.position())?;
    let stored_cv_count = crate::wire::element_count(
        reader,
        usize::try_from(dimension + rational)
            .map_err(|_| GeometryError::unpositioned("geometry count exceeds address space"))?
            * 8,
    )?;
    if stored_cv_count != cv_count {
        return Err(error(reader.position(), "NURBS curve CV count mismatch"));
    }
    let mut pole_storage = ctx.reserve_scoped(0, "Rhino NURBS curve lane scratch")?;
    let mut read = || {
        read_poles(
            ctx,
            reader,
            stored_cv_count,
            rational != 0,
            dimension,
            scale,
        )
    };
    let (control_points, weights) = if rational != 0 {
        pole_storage.with_storage(read)?
    } else {
        read()?
    };
    if minor >= 1 {
        reader.bool()?;
    }
    let periodic = periodic_knots_checked(ctx, &knots, order, cv_count)?;
    let full_knots = reconstruct_checked_knots(ctx, &knots, order, cv_count)?;
    reader.skip_remaining()?;
    let poles =
        NurbsPoles3::from_checked_lanes(ctx, control_points, weights)?.or_else(|error| {
            Err(GeometryError::malformed(
                reader.position(),
                ctx.format_retained(format_args!("{error}"), "Rhino read_nurbs_curve_inner text")?,
            ))
        })?;
    NurbsCurve::new(
        ctx,
        u32::try_from(order - 1).map_err(|_| error(reader.position(), "NURBS order overflow"))?,
        full_knots,
        poles,
        periodic,
    )?
    .or_else(|error| {
        Err(GeometryError::malformed(
            reader.position(),
            ctx.format_retained(format_args!("{error}"), "Rhino read_nurbs_curve_inner text")?,
        ))
    })
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
    let u_order = checked_positive(ctx, reader.i32()?, reader.position(), "surface U order")?;
    let v_order = checked_positive(ctx, reader.i32()?, reader.position(), "surface V order")?;
    let u_count = checked_positive(ctx, reader.i32()?, reader.position(), "surface U CV count")?;
    let v_count = checked_positive(ctx, reader.i32()?, reader.position(), "surface V CV count")?;
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
    let mut u_knots_storage = ctx.reserve_scoped(0, "Rhino NURBS stored knot scratch")?;
    let u_knots = u_knots_storage.with_storage(|| read_knots(ctx, reader, u_knot_count))?;
    validate_stored_domain(&u_knots, u_order, u_count, reader.position())?;
    let v_knot_count = crate::wire::element_count(reader, 8)?;
    let expected_v = v_order
        .checked_add(v_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(reader.position(), "surface V knot count overflow"))?;
    if v_knot_count != expected_v {
        return Err(error(reader.position(), "surface V knot count mismatch"));
    }
    let mut v_knots_storage = ctx.reserve_scoped(0, "Rhino NURBS stored knot scratch")?;
    let v_knots = v_knots_storage.with_storage(|| read_knots(ctx, reader, v_knot_count))?;
    validate_stored_domain(&v_knots, v_order, v_count, reader.position())?;
    let u_periodic = periodic_knots_checked(ctx, &u_knots, u_order, u_count)?;
    let v_periodic = periodic_knots_checked(ctx, &v_knots, v_order, v_count)?;
    let stored_cv_count = crate::wire::element_count(
        reader,
        usize::try_from(dimension + rational)
            .map_err(|_| GeometryError::unpositioned("geometry count exceeds address space"))?
            * 8,
    )?;
    let expected_cv_count = u_count
        .checked_mul(v_count)
        .ok_or_else(|| error(reader.position(), "surface CV count overflow"))?;
    if stored_cv_count != expected_cv_count {
        return Err(error(reader.position(), "NURBS surface CV count mismatch"));
    }
    let mut pole_storage = ctx.reserve_scoped(0, "Rhino NURBS surface lane scratch")?;
    let (control_points, weights) = pole_storage.with_storage(|| {
        read_poles(
            ctx,
            reader,
            stored_cv_count,
            rational != 0,
            dimension,
            scale,
        )
    })?;
    let u_knots = reconstruct_checked_knots(ctx, &u_knots, u_order, u_count)?;
    let v_knots = reconstruct_checked_knots(ctx, &v_knots, v_order, v_count)?;
    let row_len = v_count;
    let copy_points = || {
        copy_rows(
            ctx,
            &control_points,
            row_len,
            "Rhino NURBS surface pole grid",
        )
    };
    let point_rows = if weights.is_some() {
        pole_storage.with_storage(copy_points)?
    } else {
        copy_points()?
    };
    let weight_rows = weights
        .as_deref()
        .map(|values| {
            pole_storage
                .with_storage(|| copy_rows(ctx, values, row_len, "Rhino NURBS surface weight grid"))
        })
        .transpose()?;
    let poles =
        NurbsPoleGrid::from_checked_lanes(ctx, point_rows, weight_rows)?.or_else(|error| {
            Err(GeometryError::malformed(
                reader.position(),
                ctx.format_retained(
                    format_args!("{error}"),
                    "Rhino read_nurbs_surface_prefix text",
                )?,
            ))
        })?;
    NurbsSurface::new(
        ctx,
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
        poles,
        false,
    )?
    .or_else(|error| {
        Err(GeometryError::malformed(
            reader.position(),
            ctx.format_retained(
                format_args!("{error}"),
                "Rhino read_nurbs_surface_prefix text",
            )?,
        ))
    })
}

fn read_plane_surface_with_parameterization(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let native_plane = plane(ctx, reader)?;
    let frame = validate_plane(native_plane, reader.position())?;
    let domain = increasing_interval(
        ctx,
        interval(ctx, reader)?.0,
        reader.position(),
        "plane U domain",
    )?;
    let v_domain = increasing_interval(
        ctx,
        interval(ctx, reader)?.0,
        reader.position(),
        "plane V domain",
    )?;
    let (u_extents, v_extents) = if version & 0x0f == 1 {
        (
            increasing_interval(
                ctx,
                interval(ctx, reader)?.0,
                reader.position(),
                "plane U extents",
            )?,
            increasing_interval(
                ctx,
                interval(ctx, reader)?.0,
                reader.position(),
                "plane V extents",
            )?,
        )
    } else {
        (domain, v_domain)
    };
    let geometry = TypedSurface::Plane {
        plane: cadmpeg_ir::geometry::analytic::PlaneSurface::new(
            crate::wire::scaled_point(native_plane.origin.get(), scale)
                .ok_or_else(|| error(reader.position(), "scaled plane origin is invalid"))?,
            frame,
        ),
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
) -> Result<Vec<FiniteReal>, GeometryError> {
    let mut knots = ctx.collection_vec(count, "Rhino NURBS knots")?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino surfaces read_knots records")?;
        let knot_offset = reader.position();
        let value = reader.f64()?;
        let Some(value) = FiniteReal::new(value) else {
            return Err(error(knot_offset, "NURBS knots are invalid"));
        };
        if knots.last().is_some_and(|last| value < *last) {
            return Err(error(knot_offset, "NURBS knots are invalid"));
        }
        knots.push(value);
    }
    Ok(knots)
}

fn read_poles(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    count: usize,
    rational: bool,
    dimension: i32,
    scale: MillimeterScale,
) -> Result<(Vec<FinitePoint3>, Option<Vec<NonZeroReal>>), GeometryError> {
    let mut points = ctx.collection_vec(count, "Rhino NURBS poles")?;
    let mut weights = if rational {
        Some(ctx.collection_vec(count, "Rhino NURBS weights")?)
    } else {
        None
    };
    for _ in 0..count {
        ctx.charge_work(1, "Rhino surfaces read_poles records")?;
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
            target.push(weight);
            FiniteReal::from(weight)
        } else {
            FiniteReal::ONE
        };
        let coordinate = |value| {
            cadmpeg_ir::math::multiply_divide(value, scale.real(), weight)
                .ok_or_else(|| error(pole_offset, "scaled NURBS pole is invalid"))
        };
        points.push(FinitePoint3::from_coordinates(
            coordinate(x)?,
            coordinate(y)?,
            coordinate(z)?,
        ));
    }
    Ok((points, weights))
}

/// Reconstruct the omitted endpoints. The indexes below are zero-based.
pub(crate) fn reconstruct_knots(
    ctx: &DecodeContext<'_>,
    knots: &[f64],
    order: usize,
    cv_count: usize,
) -> Result<Vec<f64>, GeometryError> {
    let ([start, end], capacity) =
        reconstructed_endpoints(knots.len(), order, cv_count, |index| knots[index])?;
    let mut result = ctx.collection_vec(capacity, "Rhino NURBS reconstructed knots")?;
    fill_reconstructed_knots(ctx, &mut result, knots, [start.get(), end.get()])?;
    Ok(result)
}

fn reconstruct_checked_knots(
    ctx: &DecodeContext<'_>,
    knots: &[FiniteReal],
    order: usize,
    cv_count: usize,
) -> Result<KnotVector, GeometryError> {
    let ([start, end], capacity) =
        reconstructed_endpoints(knots.len(), order, cv_count, |index| knots[index].get())?;
    let mut scratch = Vec::new();
    let _storage = ctx
        .reserve_temporary_vec(&mut scratch, capacity, "Rhino NURBS reconstructed knots")
        .map_err(CodecError::from)?;
    // Move the allocated scratch after its reservation so it drops first.
    let mut result = scratch;
    fill_reconstructed_knots(ctx, &mut result, knots, [start, end])?;
    KnotVector::from_finite_lanes(ctx, result)?
        .map_err(|_| GeometryError::unpositioned("NURBS reconstructed knots are invalid"))
}

fn fill_reconstructed_knots<T: Copy>(
    ctx: &DecodeContext<'_>,
    output: &mut Vec<T>,
    knots: &[T],
    [start, end]: [T; 2],
) -> Result<(), CodecError> {
    let source = ctx.admit_iter(knots, "Rhino NURBS reconstructed knots")?;
    output.push(start);
    output.extend(source.copied());
    output.push(end);
    Ok(())
}

fn reconstructed_endpoints(
    knot_count: usize,
    order: usize,
    cv_count: usize,
    knot: impl Fn(usize) -> f64,
) -> Result<([FiniteReal; 2], usize), GeometryError> {
    let m = order
        .checked_add(cv_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| GeometryError::unpositioned("NURBS knot arithmetic overflow"))?;
    if knot_count != m || order < 2 || cv_count < order {
        return Err(GeometryError::unpositioned(
            "NURBS knot reconstruction input is invalid",
        ));
    }
    let mut start = knot(0);
    if order > 2 && cv_count >= 2 * order - 2 && cv_count >= 6 && knot(0) < knot(order - 2) {
        start = knot(0) - (knot(cv_count - order + 1) - knot(cv_count - order));
    }
    let mut end = knot(m - 1);
    if order > 2 && cv_count >= 2 * order - 2 && cv_count >= 6 && knot(cv_count - 1) < knot(m - 1) {
        end = knot(m - 1) + (knot(order + 1) - knot(order));
    }
    let (Some(start), Some(end)) = (FiniteReal::new(start), FiniteReal::new(end)) else {
        return Err(GeometryError::unpositioned(
            "NURBS reconstructed knots are invalid",
        ));
    };
    if start.get() > knot(0) || end.get() < knot(m - 1) {
        return Err(GeometryError::unpositioned(
            "NURBS reconstructed knots are invalid",
        ));
    }
    let capacity = order.checked_add(cv_count).ok_or_else(|| {
        GeometryError::not_implemented("NURBS reconstructed knot count exceeds address space")
    })?;
    Ok(([start, end], capacity))
}

pub(crate) fn periodic_knots(
    ctx: &DecodeContext<'_>,
    knots: &[f64],
    order: usize,
    cv_count: usize,
) -> Result<bool, CodecError> {
    periodic_knots_by(ctx, knots, order, cv_count, |value| *value, true)
}

fn periodic_knots_checked(
    ctx: &DecodeContext<'_>,
    knots: &[FiniteReal],
    order: usize,
    cv_count: usize,
) -> Result<bool, CodecError> {
    periodic_knots_by(ctx, knots, order, cv_count, |value| value.get(), false)
}

fn periodic_knots_by<T>(
    ctx: &DecodeContext<'_>,
    knots: &[T],
    order: usize,
    cv_count: usize,
    value: impl Fn(&T) -> f64,
    require_source_finite: bool,
) -> Result<bool, CodecError> {
    // This is ON_IsKnotVectorPeriodic over the stored, zero-based knot array.
    if order < 3 || cv_count < order || (order <= 4 && cv_count < order + 2) {
        return Ok(false);
    }
    if order > 4 && cv_count < 2 * order - 2 {
        return Ok(false);
    }
    let mut scale = 0.0_f64;
    if require_source_finite {
        if !ctx.all_by(
            knots,
            |knot| {
                let value = value(knot);
                if !value.is_finite() {
                    return Ok(false);
                }
                scale = scale.max(value.abs());
                Ok(true)
            },
            "Rhino periodic knot scale",
        )? {
            return Ok(false);
        }
    } else {
        for knot in ctx.admit_iter(knots, "Rhino periodic knot scale")? {
            scale = scale.max(value(knot).abs());
        }
    }
    if scale == 0.0 || !scale.is_finite() {
        return Ok(false);
    }
    let knot = |index: usize| value(&knots[index]) / scale;
    let mut tolerance = (knot(order - 1) - knot(order - 3)).abs() * f64::EPSILON.sqrt();
    tolerance = tolerance.max((knot(cv_count - 1) - knot(order - 2)).abs() * f64::EPSILON.sqrt());
    let mut paired = 2 * (order - 2);
    let mut index = 0;
    let mut other = cv_count - order + 1;
    while paired > 0 {
        ctx.charge_work(1, "Rhino periodic knot comparison")?;
        if ((knot(index + 1) - knot(index)) + (knot(other) - knot(other + 1))).abs() > tolerance {
            return Ok(false);
        }
        index += 1;
        other += 1;
        paired -= 1;
    }
    Ok(true)
}

fn validate_stored_domain(
    knots: &[FiniteReal],
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

fn checked_positive(
    ctx: &DecodeContext<'_>,
    value: i32,
    offset: usize,
    label: &str,
) -> Result<usize, GeometryError> {
    if value < 2 && label.ends_with("order") || value <= 0 {
        return Err(error(
            offset,
            ctx.copy_retained_text(label, "Rhino surface invariant message")?,
        ));
    }
    usize::try_from(value).or_else(|_| {
        Err(error(
            offset,
            ctx.copy_retained_text(label, "Rhino surface invariant message")?,
        ))
    })
}

fn increasing_interval(
    ctx: &DecodeContext<'_>,
    value: FiniteVector<2>,
    offset: usize,
    label: &str,
) -> Result<[f64; 2], GeometryError> {
    let value = value.get();
    if value[0] < value[1] {
        Ok(value)
    } else {
        Err(error(
            offset,
            ctx.copy_retained_text(label, "Rhino surface invariant message")?,
        ))
    }
}

fn validate_plane(value: Plane, offset: usize) -> Result<OrthonormalFrame3, GeometryError> {
    let x = vector(value.xaxis.get());
    let y = vector(value.yaxis.get());
    let z = vector(value.zaxis.get());
    if (x.norm() - 1.0).abs() > EPS_SURFACE_DEGENERATE
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
    OrthonormalFrame3::new(z, x)
        .ok_or_else(|| error(offset, "plane frame is not orthonormal and right-handed"))
}

#[cfg(test)]
pub(crate) mod tests;
