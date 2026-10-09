// SPDX-License-Identifier: Apache-2.0
//! Bounded `ON_Extrusion` parsing and exact profile-plane construction.

use crate::loss::{Diagnostics, ScratchVec};
use std::ops::Range;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::{nurbs_curve_parameter_domain, nurbs_curve_point_at_with_basis};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoles3, NurbsSurface},
    CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::units::{FiniteVector, UnitVector3};

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, ChecksumStatus, Chunk};
use crate::curves::{decode_embedded_curve_2d, error, exact_nurbs, DecodedCurve, GeometryError};
use crate::objects::{parse_class_wrapper_with_scoped_userdata, UserdataDescriptor};
use crate::settings::{interval, point, vector, MillimeterScale};
use crate::wire::Uuid;

const EPS_EXTRUSION_POSITION: f64 = 1.0e-8;
const EPS_EXTRUSION_DEGENERATE: f64 = 1.0e-10;

/// `ON_Extrusion` class UUID.
pub(crate) const ON_EXTRUSION: Uuid = Uuid::from_canonical([
    0x36, 0xf5, 0x31, 0x75, 0x72, 0xb8, 0x4d, 0x47, 0xbf, 0x1f, 0xb4, 0xe6, 0xfc, 0x24, 0xf4, 0xb9,
]);
const ANONYMOUS: u32 = 0x4000_8000;
/// Obsolete V5 extrusion display-mesh cache userdata class and item UUID.
const ON_V5_EXTRUSION_DISPLAY_MESH_CACHE: Uuid = Uuid::from_canonical([
    0xa8, 0x13, 0x0a, 0x3e, 0xe4, 0xf3, 0x4c, 0xb0, 0xbb, 0x8a, 0xf1, 0x0a, 0x47, 0x39, 0x12, 0xd0,
]);
const UNIT_TOLERANCE: f64 = EPS_EXTRUSION_DEGENERATE;
const MITER_Z_MINIMUM: f64 = 1.0 / 64.0;
const CLOSURE_ABSOLUTE_TOLERANCE: f64 = 2.328_306_436_538_696_3e-10;
const CLOSURE_RELATIVE_TOLERANCE: f64 = 2.273_736_754_432_320_6e-13;

/// One exact profile boundary at both effective path ends.
#[derive(Debug, Clone)]
pub(crate) struct ExtrusionBoundary {
    /// Curve tree transformed into the effective start plane.
    pub(crate) start_curve: DecodedCurve,
    /// Exact start-plane NURBS.
    pub(crate) start_nurbs: NurbsCurve,
    /// Exact end-plane NURBS.
    pub(crate) end_nurbs: NurbsCurve,
    /// Exact cap pcurve at the start.
    pub(crate) start_pcurve: CapPcurve,
    /// Exact cap pcurve at the end.
    pub(crate) end_pcurve: CapPcurve,
    /// Solved lateral tensor surface for this boundary.
    pub(crate) lateral: NurbsSurface,
}

/// Exact parameter-space curve for one planar cap.
#[derive(Debug, Clone)]
pub(crate) struct CapPcurve {
    /// Curve degree.
    pub(crate) degree: u32,
    /// Full knot vector.
    pub(crate) knots: Vec<f64>,
    /// Cap-plane control points.
    pub(crate) control_points: Vec<Point2>,
    /// Rational weights.
    pub(crate) weights: Option<Vec<f64>>,
    /// Periodicity.
    pub(crate) periodic: bool,
}

/// Parsed and validated native extrusion.
#[derive(Debug)]
pub(crate) struct DecodedExtrusion<'ctx> {
    /// Ordered outer then inner profile boundaries.
    pub(crate) boundaries: Vec<ExtrusionBoundary>,
    /// Effective model-space path direction from trimmed start to end.
    pub(crate) direction: Vector3,
    /// Effective cap origins.
    pub(crate) cap_origins: [Point3; 2],
    /// Effective cap normals.
    pub(crate) cap_normals: [UnitVector3; 2],
    /// Effective cap U axes.
    pub(crate) cap_u_axes: [UnitVector3; 2],
    /// Independent cap flags.
    pub(crate) caps: [bool; 2],
    /// Valid optional display meshes.
    pub(crate) meshes: ScopedMeshList<'ctx>,
    /// Recoverable mesh-cache warnings.
    pub(crate) warnings: Diagnostics,
}

/// Mesh-cache results whose list backing stays scoped through consumption.
#[derive(Debug)]
pub(crate) struct ScopedMeshList<'ctx> {
    values: Vec<crate::mesh::DecodedMesh>,
    storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl<'ctx> ScopedMeshList<'ctx> {
    pub(crate) fn empty() -> Self {
        Self {
            values: Vec::new(),
            storage: None,
        }
    }

    fn new(
        ctx: &'ctx DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            values: Vec::new(),
            storage: Some(ctx.reserve_scoped(0, operation)?),
        })
    }

    fn push_admitted(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        mesh: crate::mesh::DecodedMesh,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if self.storage.is_none() {
            self.storage = Some(ctx.reserve_scoped(0, operation)?);
        }
        ctx.charge_collection_items(1, operation)?;
        self.storage
            .as_mut()
            .expect("mesh-list storage initialized")
            .with_storage(|| ctx.reserve_capacity(&mut self.values, 1, operation))?;
        self.values.push(mesh);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn from_test_values(values: Vec<crate::mesh::DecodedMesh>) -> Self {
        Self {
            values,
            storage: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn into_test_values(self) -> Vec<crate::mesh::DecodedMesh> {
        self.values
    }
}

impl std::ops::Deref for ScopedMeshList<'_> {
    type Target = [crate::mesh::DecodedMesh];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<'ctx> IntoIterator for ScopedMeshList<'ctx> {
    type Item = crate::mesh::DecodedMesh;
    type IntoIter = ScopedMeshIntoIter<'ctx>;

    fn into_iter(self) -> Self::IntoIter {
        ScopedMeshIntoIter {
            values: self.values.into_iter(),
            _storage: self.storage,
        }
    }
}

pub(crate) struct ScopedMeshIntoIter<'ctx> {
    values: std::vec::IntoIter<crate::mesh::DecodedMesh>,
    _storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl Iterator for ScopedMeshIntoIter<'_> {
    type Item = crate::mesh::DecodedMesh;

    fn next(&mut self) -> Option<Self::Item> {
        self.values.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl ExactSizeIterator for ScopedMeshIntoIter<'_> {}

struct ProfileFrame {
    origin: Point3,
    xaxis: Vector3,
    yaxis: Vector3,
    zaxis: Vector3,
    miter: Option<UnitVector3>,
}

/// Returns whether a UUID is `ON_Extrusion`.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    uuid == ON_EXTRUSION
}

/// Archive settings shared by the extrusion and its embedded mesh caches.
#[derive(Clone, Copy)]
pub(crate) struct ExtrusionFormat {
    pub(crate) archive: ArchiveVersion,
    pub(crate) writer_version: Option<i64>,
    pub(crate) scale: MillimeterScale,
}

/// Decodes one complete bounded `ON_Extrusion` class payload.
pub(crate) fn decode<'ctx>(
    expand: crate::mesh::MeshExpand<'ctx>,
    data: &[u8],
    range: Range<usize>,
    format: ExtrusionFormat,
    userdata: &[UserdataDescriptor],
    mesh_budget: &mut crate::mesh::MeshBudget,
) -> Result<DecodedExtrusion<'ctx>, GeometryError> {
    let ExtrusionFormat {
        archive,
        writer_version,
        scale,
    } = format;
    let outer = chunk_at(data, range.start, range.end, archive, false)?;
    if outer.typecode != ANONYMOUS || outer.short() {
        return Err(error(range.start, "invalid extrusion anonymous framing"));
    }
    let mut reader = BoundedReader::new(data, outer.body().start, outer.body().end)?;
    let version_offset = reader.position();
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported extrusion anonymous version",
        ));
    }

    let profile_start = reader.position();
    let mut profile_storage = expand
        .ctx()
        .reserve_scoped(0, "Rhino extrusion source profile")?;
    let profile = profile_storage.with_storage(|| {
        decode_embedded_curve_2d(expand.ctx(), data, &mut reader, scale, archive, 1)
    })?;
    let profile_range = profile_start..reader.position();
    let path_from = crate::wire::scaled_point(point(expand.ctx(), &mut reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled extrusion path is invalid"))?
        .get();
    let path_to = crate::wire::scaled_point(point(expand.ctx(), &mut reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled extrusion path is invalid"))?
        .get();
    let trim = increasing_interval(
        expand.ctx(),
        interval(expand.ctx(), &mut reader)?.0,
        reader.position(),
        "path trim",
    )?;
    if trim[0] < 0.0 || trim[1] > 1.0 {
        return Err(error(
            reader.position(),
            "extrusion path trim is outside the line interval",
        ));
    }
    let up = crate::wire::vector(vector(expand.ctx(), &mut reader)?.0.get());
    let miter_present = [
        reader.bool_with_writer_version(writer_version)?,
        reader.bool_with_writer_version(writer_version)?,
    ];
    let miter_normals = [
        crate::wire::vector(vector(expand.ctx(), &mut reader)?.0.get()),
        crate::wire::vector(vector(expand.ctx(), &mut reader)?.0.get()),
    ];
    let path_domain = increasing_interval(
        expand.ctx(),
        interval(expand.ctx(), &mut reader)?.0,
        reader.position(),
        "path domain",
    )?;
    let transposed = reader.bool_with_writer_version(writer_version)?;
    let profile_count = if minor >= 1 { reader.i32()? } else { 1 };
    if profile_count <= 0 {
        return Err(error(
            reader.position(),
            "extrusion profile count is invalid",
        ));
    }

    let raw_caps = if minor >= 2 {
        [
            reader.bool_with_writer_version(writer_version)?,
            reader.bool_with_writer_version(writer_version)?,
        ]
    } else {
        [false, false]
    };
    let mut warnings = Diagnostics::new();
    let mut payload_children = vec![profile_range];
    let meshes = if minor >= 3 {
        let cache_start = reader.position();
        let cache_range = chunk_at(data, cache_start, reader.end(), archive, false)
            .ok()
            .map(|chunk| chunk.range());
        if let Some(range) = cache_range {
            payload_children.push(range);
        }
        match read_mesh_cache(
            expand,
            data,
            &mut reader,
            ExtrusionFormat {
                archive,
                writer_version,
                scale,
            },
            mesh_budget,
            &mut warnings,
        ) {
            Ok(meshes) => meshes,
            Err(error @ GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(_))) => {
                return Err(error);
            }
            Err(cache_error) => {
                // MeshBudget counters stay monotonic after rejection. The
                // temporary decoded meshes, descriptor lists, and their
                // scoped backing guards drop with `read_mesh_cache`.
                warnings.push_admitted(
                    expand.ctx(),
                    format_args!("extrusion mesh cache dropped: {cache_error}"),
                )?;
                reader.skip(reader.remaining())?;
                ScopedMeshList::empty()
            }
        }
    } else {
        match read_v5_mesh_cache(
            expand,
            data,
            ExtrusionFormat {
                archive,
                writer_version,
                scale,
            },
            userdata,
            mesh_budget,
            &mut warnings,
        ) {
            Ok(meshes) => meshes,
            Err(error @ GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(_))) => {
                return Err(error);
            }
            Err(cache_error) => {
                warnings.push_admitted(
                    expand.ctx(),
                    format_args!("V5 extrusion mesh cache dropped: {cache_error}"),
                )?;
                ScopedMeshList::empty()
            }
        }
    };
    expand.ctx().fold(
        &meshes[..],
        (),
        |(), mesh| warnings.extend_cloned_admitted(expand.ctx(), &mesh.warnings),
        "Rhino extrusion cached mesh warning traversal",
    )?;
    finish_payload(
        expand.ctx(),
        data,
        &outer,
        reader,
        &payload_children,
        &mut warnings,
    )?;

    let path_delta = path_to.vector_from(path_from);
    let path_length = path_delta.norm();
    if !path_length.is_finite() || path_length <= 0.0 {
        return Err(error(
            version_offset,
            "extrusion path is not finite and distinct",
        ));
    }
    let tangent = path_delta.scale(1.0 / path_length);
    require_unit(expand.ctx(), up, version_offset, "extrusion up vector")?;
    if up.dot(tangent).abs() > UNIT_TOLERANCE {
        return Err(error(
            version_offset,
            "extrusion up vector is not perpendicular to path",
        ));
    }
    let active_miters = [
        active_miter(miter_present[0], miter_normals[0]),
        active_miter(miter_present[1], miter_normals[1]),
    ];

    let mut boundary_storage = expand
        .ctx()
        .reserve_scoped(0, "Rhino extrusion source boundaries")?;
    let source_boundaries = boundary_storage.with_storage(|| {
        split_profiles(
            expand.ctx(),
            profile,
            usize::try_from(profile_count)
                .map_err(|_| GeometryError::unpositioned("geometry count exceeds address space"))?,
            version_offset,
        )
    })?;
    let xaxis = normalize(
        expand.ctx(),
        up.cross(tangent),
        version_offset,
        "extrusion profile X axis",
    )?;
    let cap_origins = [
        path_from.translated(path_delta, trim[0]),
        path_from.translated(path_delta, trim[1]),
    ];
    let direction = cap_origins[1].vector_from(cap_origins[0]);
    let mut boundaries = expand
        .ctx()
        .collection_vec(source_boundaries.len(), "Rhino extrusion boundaries")
        .map_err(crate::curves::GeometryError::from)?;
    let mut orientation_storage = expand
        .ctx()
        .reserve_scoped(0, "Rhino extrusion orientations")?;
    let mut orientations = orientation_storage.with_storage(|| {
        expand
            .ctx()
            .collection_vec(source_boundaries.len(), "Rhino extrusion orientations")
    })?;
    let source_profile_count = source_boundaries.len();
    let mut source_profiles = source_boundaries.into_iter();
    for _ in 0..source_profile_count {
        let source = expand
            .ctx()
            .next_charged(&mut source_profiles, "Rhino extrusion profile traversal")?
            .ok_or_else(|| error(version_offset, "extrusion profile source ended early"))?;
        let mut source_nurbs_storage = expand
            .ctx()
            .reserve_scoped(0, "Rhino extrusion source NURBS")?;
        let source_nurbs = source_nurbs_storage
            .with_storage(|| exact_nurbs(expand.ctx(), &source, version_offset))?;
        orientations.push(nurbs_orientation(
            expand.ctx(),
            &source_nurbs,
            version_offset,
        )?);
        let start_nurbs = transform_nurbs(
            expand.ctx(),
            &source_nurbs,
            &ProfileFrame {
                origin: cap_origins[0],
                xaxis: xaxis.into(),
                yaxis: up,
                zaxis: tangent,
                miter: active_miters[0],
            },
            version_offset,
        )?;
        let end_nurbs = transform_nurbs(
            expand.ctx(),
            &source_nurbs,
            &ProfileFrame {
                origin: cap_origins[1],
                xaxis: xaxis.into(),
                yaxis: up,
                zaxis: tangent,
                miter: active_miters[1],
            },
            version_offset,
        )?;
        let mut boundary_warnings = Diagnostics::new();
        let mut source_warnings = source.into_warnings();
        boundary_warnings.append_admitted(expand.ctx(), &mut source_warnings)?;
        let start_curve = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                start_nurbs.try_clone_for_decode(expand.ctx(), "Rhino extrusion start curve")?,
            )),
            boundary_warnings,
        );
        let start_frame = cap_frame(
            expand.ctx(),
            xaxis.into(),
            up,
            tangent,
            active_miters[0],
            version_offset,
        )?;
        let end_frame = cap_frame(
            expand.ctx(),
            xaxis.into(),
            up,
            tangent,
            active_miters[1],
            version_offset,
        )?;
        let start_pcurve = cap_pcurve(
            expand.ctx(),
            &start_nurbs,
            cap_origins[0],
            start_frame,
            version_offset,
        )?;
        let end_pcurve = cap_pcurve(
            expand.ctx(),
            &end_nurbs,
            cap_origins[1],
            end_frame,
            version_offset,
        )?;
        let lateral = crate::surfaces::extrusion_nurbs(
            expand.ctx(),
            &start_nurbs,
            &end_nurbs,
            path_domain,
            transposed,
            version_offset,
        )?;
        boundaries.push(ExtrusionBoundary {
            start_curve,
            start_nurbs,
            end_nurbs,
            start_pcurve,
            end_pcurve,
            lateral,
        });
    }
    drop(source_profiles);
    drop(boundary_storage);
    drop(profile_storage);
    if (orientations.len() > 1
        && (orientations.first() != Some(&1)
            || expand.ctx().any_by(
                &orientations[1..],
                |value| Ok(*value != -1),
                "Rhino decode traversal",
            )?))
        || (orientations.len() == 1 && !matches!(orientations[0], 0 | 1))
    {
        return Err(error(
            version_offset,
            "extrusion profile orientations are invalid",
        ));
    }
    let all_closed = expand.ctx().all_by(
        &orientations[..],
        |orientation| Ok(*orientation != 0),
        "Rhino decode traversal",
    )?;
    let caps = if minor >= 2 {
        raw_caps
    } else if all_closed {
        [true, true]
    } else {
        [false, false]
    };
    if (caps[0] || caps[1]) && !all_closed {
        return Err(error(
            version_offset,
            "capped extrusion requires closed profile boundaries",
        ));
    }
    let cap_frames = [
        cap_frame(
            expand.ctx(),
            xaxis.into(),
            up,
            tangent,
            active_miters[0],
            version_offset,
        )?,
        cap_frame(
            expand.ctx(),
            xaxis.into(),
            up,
            tangent,
            active_miters[1],
            version_offset,
        )?,
    ];
    Ok(DecodedExtrusion {
        boundaries,
        direction,
        cap_origins,
        cap_normals: [cap_frames[0].2, cap_frames[1].2],
        cap_u_axes: [cap_frames[0].0, cap_frames[1].0],
        caps,
        meshes,
        warnings,
    })
}

fn split_profiles(
    ctx: &DecodeContext<'_>,
    profile: DecodedCurve,
    profile_count: usize,
    offset: usize,
) -> Result<Vec<DecodedCurve>, GeometryError> {
    if profile_count == 1 {
        let mut profiles = ctx
            .collection_vec(1, "Rhino extrusion profile split")
            .map_err(crate::curves::GeometryError::from)?;
        profiles.push(profile);
        return Ok(profiles);
    }
    let DecodedCurve::Compound { children, .. } = profile else {
        return Err(error(
            offset,
            "multiple extrusion profiles require an exact polycurve",
        ));
    };
    if children.len() != profile_count {
        return Err(error(offset, "extrusion profile count mismatch"));
    }
    let mut profiles = ctx
        .collection_vec(children.len(), "Rhino extrusion profile split")
        .map_err(crate::curves::GeometryError::from)?;
    profiles.extend(
        ctx.admit_iter(children, "Rhino extrusion profile split traversal")
            .map_err(CodecError::from)?
            .map(|(_, child)| child),
    );
    Ok(profiles)
}

fn nurbs_orientation(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    offset: usize,
) -> Result<i8, GeometryError> {
    if curve.pole_count() < 2 || curve.degree() == 0 {
        return Err(error(offset, "extrusion profile closure is degenerate"));
    }
    if profile_off_plane(ctx, curve)? {
        return Err(error(offset, "extrusion profile is not in the XY plane"));
    }
    let domain = nurbs_curve_parameter_domain(curve)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
        .ok_or_else(|| error(offset, "extrusion profile parameter domain is invalid"))?;
    let degree = usize::try_from(curve.degree())
        .map_err(|_| error(offset, "extrusion profile degree is too large"))?;
    let basis_count = degree
        .checked_add(1)
        .ok_or_else(|| error(offset, "extrusion profile degree is too large"))?;
    let mut basis_storage = ctx.reserve_scoped(0, "Rhino extrusion profile basis")?;
    let mut basis = basis_storage
        .with_storage(|| ctx.alloc_filled(basis_count, 0.0, "Rhino extrusion profile basis"))?;
    let start = evaluate_profile_point(ctx, curve, domain[0], offset, &mut basis)?;
    let end = evaluate_profile_point(ctx, curve, domain[1], offset, &mut basis)?;
    let sample_parameter = |start: f64, end: f64, fraction: f64, ordinary: f64| {
        if ordinary.is_finite() {
            Ok(ordinary)
        } else {
            cadmpeg_ir::math::interpolate(start, end, fraction)
                .map(cadmpeg_ir::scalar::FiniteReal::get)
                .ok_or_else(|| error(offset, "extrusion profile parameter domain is invalid"))
        }
    };
    if !source_periodic(ctx, curve)? {
        if !points_coincident(start, end) {
            return Ok(0);
        }
        let span = domain[1] - domain[0];
        let one_third = evaluate_profile_point(
            ctx,
            curve,
            sample_parameter(domain[0], domain[1], 1.0 / 3.0, domain[0] + span / 3.0)?,
            offset,
            &mut basis,
        )?;
        let two_thirds = evaluate_profile_point(
            ctx,
            curve,
            sample_parameter(
                domain[0],
                domain[1],
                2.0 / 3.0,
                domain[0] + 2.0 * span / 3.0,
            )?,
            offset,
            &mut basis,
        )?;
        if points_coincident(start, one_third)
            || points_coincident(start, two_thirds)
            || points_coincident(end, one_third)
            || points_coincident(end, two_thirds)
        {
            return Ok(0);
        }
    }

    let span_count = ctx
        .admit_iter(
            curve.knots().as_slice(),
            "Rhino extrusion orientation span count",
        )
        .map_err(CodecError::from)?
        .windows(
            std::num::NonZeroUsize::new(2)
                .ok_or_else(|| CodecError::malformed("invalid window size"))?,
        )
        .filter(|pair| pair[0] < pair[1] && pair[1] > domain[0] && pair[0] < domain[1])
        .count();
    if span_count == 0 {
        return Err(error(offset, "extrusion profile has no nonempty spans"));
    }
    let mut samples_per_span = degree;
    if samples_per_span <= 1 {
        samples_per_span = 1;
    } else if samples_per_span < 4 {
        samples_per_span = 4;
        while span_count.checked_mul(samples_per_span).ok_or_else(|| {
            error(
                offset,
                "extrusion profile orientation sample count overflow",
            )
        })? < 17
        {
            samples_per_span = samples_per_span.checked_mul(2).ok_or_else(|| {
                error(
                    offset,
                    "extrusion profile orientation sample count overflow",
                )
            })?;
        }
    }

    let mut previous = end;
    let mut twice_area = 0.0;
    let knot_window_count = curve.knots().len().saturating_sub(1);
    let mut knot_windows = curve.knots()[..].windows(2);
    for _ in 0..knot_window_count {
        let pair = ctx
            .next_charged(&mut knot_windows, "Rhino exact orientation window traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino orientation knot source ended early")
            })?;
        let span_start = pair[0].max(domain[0]);
        let span_end = pair[1].min(domain[1]);
        if span_start >= span_end {
            continue;
        }
        for sample in 0..samples_per_span {
            ctx.charge_work(1, "Rhino extrusion exact_orientation records")?;
            let fraction = cadmpeg_core::convert::f64_from_index(sample)
                .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?
                / cadmpeg_core::convert::f64_from_index(samples_per_span)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
            let parameter = sample_parameter(
                span_start,
                span_end,
                fraction,
                span_start + fraction * (span_end - span_start),
            )?;
            let current = evaluate_profile_point(ctx, curve, parameter, offset, &mut basis)?;
            twice_area += (previous.x - current.x) * (previous.y + current.y);
            previous = current;
        }
    }
    let final_point = evaluate_profile_point(ctx, curve, domain[1], offset, &mut basis)?;
    twice_area += (previous.x - final_point.x) * (previous.y + final_point.y);
    if !twice_area.is_finite() {
        return Err(error(offset, "extrusion profile orientation is invalid"));
    }
    Ok(if twice_area > 0.0 {
        1
    } else if twice_area < 0.0 {
        -1
    } else {
        0
    })
}

fn source_periodic(ctx: &DecodeContext<'_>, curve: &NurbsCurve) -> Result<bool, CodecError> {
    if !curve.periodic() || curve.degree() <= 1 {
        return Ok(false);
    }
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(false);
    };
    let count = curve.pole_count();
    if count < degree {
        return Ok(false);
    }
    ctx.all_by(
        0..degree,
        |index| {
            let first = curve.pole_rows().point_at(degree - 1 - index);
            let last = curve.pole_rows().point_at(count - 1 - index);
            Ok(match (first, last) {
                (Some(first), Some(last)) => points_coincident(first.get(), last.get()),
                _ => false,
            })
        },
        "Rhino extrusion periodic closure search",
    )
}

fn evaluate_profile_point(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    parameter: f64,
    offset: usize,
    basis: &mut [f64],
) -> Result<Point3, GeometryError> {
    nurbs_curve_point_at_with_basis(ctx, curve, parameter, basis)
        .map(cadmpeg_ir::features::FinitePoint3::get)
        .map_err(|failure| match failure {
            cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit) => {
                GeometryError::Codec(limit.into())
            }
            cadmpeg_ir::eval::EvaluationFailure::NoValue
            | cadmpeg_ir::eval::EvaluationFailure::NonFinite(_) => {
                error(offset, "extrusion profile cannot be evaluated")
            }
        })
}

fn points_coincident(first: Point3, second: Point3) -> bool {
    [
        (first.x, second.x),
        (first.y, second.y),
        (first.z, second.z),
    ]
    .into_iter()
    .all(|(a, b)| {
        let difference = (a - b).abs();
        difference <= CLOSURE_ABSOLUTE_TOLERANCE
            || difference <= (a.abs() + b.abs()) * CLOSURE_RELATIVE_TOLERANCE
    })
}

fn profile_off_plane(ctx: &DecodeContext<'_>, curve: &NurbsCurve) -> Result<bool, CodecError> {
    Ok(match curve.pole_rows() {
        NurbsPoles3::Polynomial { points } => ctx.any_by(
            points.as_slice(),
            |point| Ok(point.get().z != 0.0),
            "Rhino extrusion profile plane traversal",
        )?,
        NurbsPoles3::Rational { points } => ctx.any_by(
            points.as_slice(),
            |pole| Ok(pole.point.get().z != 0.0),
            "Rhino extrusion profile plane traversal",
        )?,
    })
}

fn transform_nurbs(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    frame: &ProfileFrame,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let mut curve = curve.try_clone_for_decode(ctx, "Rhino extrusion transformed NURBS")?;
    curve.try_map_control_points(
        |_, point| {
            let transformed = transform_local(ctx, point.get(), frame, offset)?;
            FinitePoint3::new(transformed).ok_or_else(|| {
                GeometryError::malformed(offset, "control_points contains a non-finite point")
            })
        },
        ctx,
    )??;
    Ok(curve)
}

fn transform_local(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    point: Point3,
    frame: &ProfileFrame,
    offset: usize,
) -> Result<Point3, GeometryError> {
    if point.z != 0.0 {
        return Err(error(
            offset,
            "extrusion profile pole is outside the XY plane",
        ));
    }
    let local = mitered_local(
        ctx,
        Vector3::new(point.x, point.y, 0.0),
        frame.miter,
        offset,
    )?;
    Ok(frame.origin.translated(
        frame.xaxis.scale(local.x) + frame.yaxis.scale(local.y) + frame.zaxis.scale(local.z),
        1.0,
    ))
}

fn cap_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    xaxis: Vector3,
    yaxis: Vector3,
    zaxis: Vector3,
    miter: Option<UnitVector3>,
    offset: usize,
) -> Result<(UnitVector3, UnitVector3, UnitVector3), GeometryError> {
    let local_x = mitered_local(ctx, Vector3::new(1.0, 0.0, 0.0), miter, offset)?;
    let local_y = mitered_local(ctx, Vector3::new(0.0, 1.0, 0.0), miter, offset)?;
    let world_x = local_to_world_vector(local_x, xaxis, yaxis, zaxis);
    let world_y = local_to_world_vector(local_y, xaxis, yaxis, zaxis);
    let u = normalize(ctx, world_x, offset, "extrusion cap U axis")?;
    let normal = normalize(ctx, world_x.cross(world_y), offset, "extrusion cap normal")?;
    let v = normalize(
        ctx,
        Vector3::from(normal).cross(u.into()),
        offset,
        "extrusion cap V axis",
    )?;
    Ok((u, v, normal))
}

fn cap_pcurve(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    origin: Point3,
    frame: (UnitVector3, UnitVector3, UnitVector3),
    offset: usize,
) -> Result<CapPcurve, GeometryError> {
    let frame = (
        Vector3::from(frame.0),
        Vector3::from(frame.1),
        Vector3::from(frame.2),
    );
    let mut points = ctx
        .collection_vec(curve.pole_count(), "Rhino extrusion cap points")
        .map_err(crate::curves::GeometryError::from)?;
    let mut indices = 0..curve.pole_count();
    for _ in 0..curve.pole_count() {
        let index = ctx
            .next_charged(&mut indices, "Rhino extrusion cap pole traversal")?
            .ok_or_else(|| error(offset, "extrusion cap pole traversal ended early"))?;
        let point = curve
            .pole_rows()
            .point_at(index)
            .ok_or_else(|| GeometryError::malformed(offset, "extrusion cap boundary has no pole"))?
            .get();
        let delta = point.vector_from(origin);
        let distance = delta.dot(frame.2);
        if distance.abs() > EPS_EXTRUSION_POSITION {
            return Err(error(offset, "extrusion cap boundary is not planar"));
        }
        points.push(Point2::new(delta.dot(frame.0), delta.dot(frame.1)));
    }
    let knots = ctx.copy_slice(curve.knots().as_slice(), "Rhino extrusion cap knots")?;
    let weights = match curve.pole_rows() {
        NurbsPoles3::Polynomial { .. } => None,
        NurbsPoles3::Rational { points } => {
            let mut weights = ctx
                .collection_vec(points.len(), "Rhino extrusion cap weights")
                .map_err(crate::curves::GeometryError::from)?;
            weights.extend(
                ctx.admit_iter(&points[..], "Rhino extrusion cap weight traversal")
                    .map_err(CodecError::from)?
                    .map(|pole| pole.weight.get()),
            );
            Some(weights)
        }
    };
    Ok(CapPcurve {
        degree: curve.degree(),
        knots,
        control_points: points,
        weights,
        periodic: curve.periodic(),
    })
}

fn mitered_local(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    point: Vector3,
    normal: Option<UnitVector3>,
    offset: usize,
) -> Result<Vector3, GeometryError> {
    let Some(normal) = normal else {
        return Ok(point);
    };
    let normal: Vector3 = normal.into();
    if normal.x == 0.0 && normal.y == 0.0 {
        return Ok(point);
    }
    let axis: Vector3 = normalize(
        ctx,
        Vector3::new(-normal.y, normal.x, 0.0),
        offset,
        "extrusion miter rotation axis",
    )?
    .into();
    let c = 1.0 - 1.0 / normal.z;
    let scaled = Vector3::new(
        (1.0 - c * axis.y * axis.y) * point.x + c * axis.x * axis.y * point.y,
        c * axis.x * axis.y * point.x + (1.0 - c * axis.x * axis.x) * point.y,
        point.z,
    );
    Ok(rodrigues(
        scaled,
        axis,
        normal.x.hypot(normal.y).atan2(normal.z),
    ))
}

fn read_mesh_cache<'ctx>(
    expand: crate::mesh::MeshExpand<'ctx>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    format: ExtrusionFormat,
    mesh_budget: &mut crate::mesh::MeshBudget,
    warnings: &mut Diagnostics,
) -> Result<ScopedMeshList<'ctx>, GeometryError> {
    let ExtrusionFormat {
        archive,
        writer_version,
        scale,
    } = format;
    let cache = anonymous_chunk(expand.ctx(), data, reader, archive, "extrusion mesh cache")?;
    let mut cache_reader = BoundedReader::new(data, cache.body().start, cache.body().end)?;
    require_anonymous_version(
        expand.ctx(),
        &mut cache_reader,
        1,
        0,
        "extrusion mesh cache",
    )?;
    let mut meshes = ScopedMeshList::new(expand.ctx(), "Rhino extrusion cache meshes")?;
    let mut cache_children =
        ScratchVec::new(expand.ctx(), "Rhino extrusion cache child ranges")?;
    let mut index = 0_usize;
    loop {
        expand
            .ctx()
            .charge_work(1, "Rhino extrusion mesh-cache scan")?;
        match cache_reader.u8()? {
            0 => break,
            1 => {}
            _ => {
                return Err(error(
                    cache_reader.position() - 1,
                    "invalid mesh-cache item marker",
                ))
            }
        }
        let item = anonymous_chunk(
            expand.ctx(),
            data,
            &mut cache_reader,
            archive,
            "mesh-cache item",
        )?;
        cache_children.push_admitted(
            expand.ctx(),
            item.range(),
            "Rhino extrusion mesh-cache children",
        )?;
        let mut item_reader = BoundedReader::new(data, item.body().start, item.body().end)?;
        require_anonymous_version(expand.ctx(), &mut item_reader, 1, 0, "mesh-cache item")?;
        item_reader.skip(16)?;
        let wrapper_start = item_reader.position();
        let wrapper = chunk_at(data, wrapper_start, item_reader.end(), archive, false)?;
        let (class, userdata) = parse_class_wrapper_with_scoped_userdata(
            expand.ctx(),
            data,
            wrapper.range(),
            archive,
            warnings,
        )?;
        item_reader.skip(wrapper.next_offset() - wrapper_start)?;
        if class.class_uuid != crate::mesh::ON_MESH {
            return Err(error(wrapper_start, "mesh-cache item is not ON_Mesh"));
        }
        let mesh = crate::mesh::decode(
            expand,
            data,
            class.class_data_range,
            archive,
            crate::mesh::MeshDecodeOptions {
                writer_version,
                association: None,
                id: crate::mesh::MeshId::ExtrusionCache(index),
                scale,
                userdata: &userdata[..],
            },
            mesh_budget,
        )?;
        meshes.push_admitted(
            expand.ctx(),
            mesh,
            "Rhino extrusion mesh-cache meshes",
        )?;
        finish_anonymous(
            expand.ctx(),
            data,
            &mut cache_reader,
            &item,
            item_reader,
            AnonymousChecksum {
                children: std::slice::from_ref(&wrapper.range()),
                name: "mesh-cache item",
            },
            warnings,
        )?;
        index = index
            .checked_add(1)
            .ok_or_else(|| error(wrapper_start, "mesh-cache item count overflow"))?;
    }
    finish_anonymous(
        expand.ctx(),
        data,
        reader,
        &cache,
        cache_reader,
        AnonymousChecksum {
            children: &cache_children,
            name: "extrusion mesh cache",
        },
        warnings,
    )?;
    Ok(meshes)
}

fn read_v5_mesh_cache<'ctx>(
    expand: crate::mesh::MeshExpand<'ctx>,
    data: &[u8],
    format: ExtrusionFormat,
    userdata: &[UserdataDescriptor],
    mesh_budget: &mut crate::mesh::MeshBudget,
    warnings: &mut Diagnostics,
) -> Result<ScopedMeshList<'ctx>, GeometryError> {
    let ExtrusionFormat {
        archive,
        writer_version,
        scale,
    } = format;
    let Some(cache) = expand.ctx().find_map(
        userdata,
        |raw| {
            let Some(value) = UserdataDescriptor::known(raw) else {
                return Ok(None);
            };
            Ok((value.class_uuid == ON_V5_EXTRUSION_DISPLAY_MESH_CACHE
                && value.item_uuid == ON_V5_EXTRUSION_DISPLAY_MESH_CACHE)
                .then_some(value))
        },
        "Rhino read v5 mesh cache traversal",
    )?
    else {
        return Ok(ScopedMeshList::new(
            expand.ctx(),
            "Rhino V5 extrusion cache meshes",
        )?);
    };

    let mut offset = cache.payload_range.start;
    let mut meshes =
        ScopedMeshList::new(expand.ctx(), "Rhino V5 extrusion cache meshes")?;
    for index in 0..3_usize {
        let wrapper = chunk_at(data, offset, cache.payload_range.end, archive, false)?;
        let (class, nested_userdata) = parse_class_wrapper_with_scoped_userdata(
            expand.ctx(),
            data,
            wrapper.range(),
            archive,
            warnings,
        )?;
        if index < 2 {
            if class.class_uuid == crate::mesh::ON_MESH {
                let mesh = crate::mesh::decode(
                    expand,
                    data,
                    class.class_data_range,
                    archive,
                    crate::mesh::MeshDecodeOptions {
                        writer_version,
                        association: None,
                        id: crate::mesh::MeshId::V5ExtrusionCache(index),
                        scale,
                        userdata: &nested_userdata[..],
                    },
                    mesh_budget,
                )?;
                meshes.push_admitted(
                    expand.ctx(),
                    mesh,
                    "Rhino V5 extrusion mesh-cache meshes",
                )?;
            } else if class.class_uuid != Uuid::nil() {
                return Err(error(
                    wrapper.header_start,
                    "V5 extrusion mesh-cache item is not ON_Mesh or null",
                ));
            }
        }
        offset = wrapper.next_offset();
    }
    // `ON_V5ExtrusionDisplayMeshCache::Read` returns after its three
    // `ReadObject` calls. The enclosing anonymous-chunk end operation then
    // skips any later bounded suffix, so preserve that forward-compatible
    // boundary instead of rejecting the optional cache.
    Ok(meshes)
}

fn anonymous_chunk(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    name: &str,
) -> Result<Chunk, GeometryError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(error(
            chunk.header_start,
            ctx.format_retained(
                format_args!("expected anonymous {name} chunk"),
                "Rhino anonymous_chunk text",
            )?,
        ));
    }
    Ok(chunk)
}

#[derive(Clone, Copy)]
struct AnonymousChecksum<'a> {
    children: &'a [std::ops::Range<usize>],
    name: &'a str,
}

fn finish_anonymous(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    mut child: BoundedReader<'_>,
    checksum: AnonymousChecksum<'_>,
    warnings: &mut Diagnostics,
) -> Result<(), GeometryError> {
    child.skip_remaining()?;
    let mut ranges = ctx.reserve_scoped(0, "Rhino extrusion checksum ranges")?;
    let direct = ranges.with_storage(|| {
        crate::chunks::direct_checksum_ranges(ctx, &chunk.body(), checksum.children)
    })?;
    if matches!(
        crate::chunks::verify_checksum_ranges(ctx, data, chunk, &direct)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!(
                "{} CRC mismatch at offset {}",
                checksum.name, chunk.header_start
            ),
        )?;
    }
    parent.skip(chunk.next_offset() - parent.position())?;
    Ok(())
}

fn finish_payload(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    chunk: &Chunk,
    mut reader: BoundedReader<'_>,
    children: &[std::ops::Range<usize>],
    warnings: &mut Diagnostics,
) -> Result<(), GeometryError> {
    reader.skip_remaining()?;
    let mut ranges = ctx.reserve_scoped(0, "Rhino extrusion checksum ranges")?;
    let direct = ranges
        .with_storage(|| crate::chunks::direct_checksum_ranges(ctx, &chunk.body(), children))?;
    if matches!(
        crate::chunks::verify_checksum_ranges(ctx, data, chunk, &direct)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!(
                "extrusion payload CRC mismatch at offset {}",
                chunk.header_start
            ),
        )?;
    }
    Ok(())
}

fn require_anonymous_version(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    major: i32,
    minor: i32,
    name: &str,
) -> Result<(), GeometryError> {
    let offset = reader.position();
    let actual_major = reader.i32()?;
    let actual_minor = reader.i32()?;
    if actual_major != major || actual_minor < minor {
        return Err(GeometryError::unsupported(
            offset,
            ctx.format_retained(
                format_args!("unsupported {name} version"),
                "Rhino require_anonymous_version text",
            )?,
        ));
    }
    Ok(())
}

fn increasing_interval(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: FiniteVector<2>,
    offset: usize,
    name: &str,
) -> Result<FiniteVector<2>, GeometryError> {
    if value[0] < value[1] {
        Ok(value)
    } else {
        Err(error(
            offset,
            ctx.format_retained(
                format_args!("extrusion {name} is invalid"),
                "Rhino increasing_interval text",
            )?,
        ))
    }
}

fn require_unit(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: Vector3,
    offset: usize,
    name: &str,
) -> Result<(), GeometryError> {
    let length = value.norm();
    if (length - 1.0).abs() <= UNIT_TOLERANCE {
        Ok(())
    } else {
        Err(error(
            offset,
            ctx.format_retained(
                format_args!("{name} is not unit"),
                "Rhino require_unit text",
            )?,
        ))
    }
}

fn active_miter(present: bool, value: Vector3) -> Option<UnitVector3> {
    if !present {
        return None;
    }
    let unit = UnitVector3::normalized_nonzero(FiniteVector3::new(value)?)?;
    (Vector3::from(unit).z > MITER_Z_MINIMUM).then_some(unit)
}

fn normalize(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: Vector3,
    offset: usize,
    name: &str,
) -> Result<UnitVector3, GeometryError> {
    FiniteVector3::new(value)
        .and_then(UnitVector3::normalized_nonzero)
        .map_or_else(
            || {
                Err(error(
                    offset,
                    ctx.format_retained(format_args!("{name} is invalid"), "Rhino normalize text")?,
                ))
            },
            Ok,
        )
}

fn local_to_world_vector(
    local: Vector3,
    xaxis: Vector3,
    yaxis: Vector3,
    zaxis: Vector3,
) -> Vector3 {
    xaxis.scale(local.x) + yaxis.scale(local.y) + zaxis.scale(local.z)
}

fn rodrigues(value: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    value.scale(cosine)
        + axis.cross(value).scale(sine)
        + axis.scale(axis.dot(value) * (1.0 - cosine))
}

#[cfg(test)]
pub(crate) mod tests;
