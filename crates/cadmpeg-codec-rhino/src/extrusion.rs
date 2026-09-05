// SPDX-License-Identifier: Apache-2.0
//! Bounded `ON_Extrusion` parsing and exact profile-plane construction.

use std::ops::Range;

use cadmpeg_ir::eval::{nurbs_curve_parameter_domain, nurbs_curve_point};
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, ChecksumStatus, Chunk};
use crate::curves::{decode_embedded_curve_2d, error, exact_nurbs, DecodedCurve, GeometryError};
use crate::objects::{parse_class_wrapper_with_userdata, UserdataDescriptor};
use crate::settings::{interval, point, vector};
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
#[derive(Debug, Clone)]
pub(crate) struct DecodedExtrusion {
    /// Ordered outer then inner profile boundaries.
    pub(crate) boundaries: Vec<ExtrusionBoundary>,
    /// One solved lateral tensor surface per boundary.
    pub(crate) laterals: Vec<NurbsSurface>,
    /// Effective model-space path direction from trimmed start to end.
    pub(crate) direction: Vector3,
    /// Effective cap origins.
    pub(crate) cap_origins: [Point3; 2],
    /// Effective cap normals.
    pub(crate) cap_normals: [Vector3; 2],
    /// Effective cap U axes.
    pub(crate) cap_u_axes: [Vector3; 2],
    /// Independent cap flags.
    pub(crate) caps: [bool; 2],
    /// Valid optional display meshes.
    pub(crate) meshes: Vec<crate::mesh::DecodedMesh>,
    /// Recoverable mesh-cache warnings.
    pub(crate) warnings: Vec<String>,
}

/// Returns whether a UUID is `ON_Extrusion`.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    uuid == ON_EXTRUSION
}

/// Decodes one complete bounded `ON_Extrusion` class payload.
#[allow(clippy::too_many_arguments)] // Keeps the borrowed archive and mesh-owner contexts explicit.
pub(crate) fn decode(
    expand: crate::mesh::MeshExpand<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: f64,
    userdata: &[UserdataDescriptor],
    mesh_budget: &mut crate::mesh::MeshBudget,
) -> Result<DecodedExtrusion, GeometryError> {
    let outer = chunk_at(data, range.start, range.end, archive, false)?;
    if outer.typecode != ANONYMOUS || outer.short {
        return Err(error(range.start, "invalid extrusion anonymous framing"));
    }
    let mut reader = BoundedReader::new(data, outer.body.start, outer.body.end)?;
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
    let profile = decode_embedded_curve_2d(data, &mut reader, scale, archive, 1)?;
    let profile_range = profile_start..reader.position();
    let path_from = scaled_point(point(&mut reader)?, scale)
        .ok_or_else(|| error(reader.position(), "scaled extrusion path is invalid"))?;
    let path_to = scaled_point(point(&mut reader)?, scale)
        .ok_or_else(|| error(reader.position(), "scaled extrusion path is invalid"))?;
    let trim = increasing_interval(interval(&mut reader)?.0, reader.position(), "path trim")?;
    if trim[0] < 0.0 || trim[1] > 1.0 {
        return Err(error(
            reader.position(),
            "extrusion path trim is outside the line interval",
        ));
    }
    let up = ir_vector(vector(&mut reader)?);
    let miter_present = [
        reader.bool_with_writer_version(writer_version)?,
        reader.bool_with_writer_version(writer_version)?,
    ];
    let miter_normals = [
        ir_vector(vector(&mut reader)?),
        ir_vector(vector(&mut reader)?),
    ];
    let path_domain =
        increasing_interval(interval(&mut reader)?.0, reader.position(), "path domain")?;
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
    let mut warnings = Vec::new();
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
            archive,
            writer_version,
            scale,
            mesh_budget,
            &mut warnings,
        ) {
            Ok(meshes) => meshes,
            Err(cache_error) => {
                // The document budget is not rolled back: any buffer the cache
                // inflated before failing is retained in the arena, so its
                // charge must stand (see `mesh::MeshBudget`).
                warnings.push(format!("extrusion mesh cache dropped: {cache_error}"));
                reader.skip(reader.remaining())?;
                Vec::new()
            }
        }
    } else {
        match read_v5_mesh_cache(
            expand,
            data,
            archive,
            writer_version,
            scale,
            userdata,
            mesh_budget,
            &mut warnings,
        ) {
            Ok(meshes) => meshes,
            Err(cache_error) => {
                warnings.push(format!("V5 extrusion mesh cache dropped: {cache_error}"));
                Vec::new()
            }
        }
    };
    for mesh in &meshes {
        warnings.extend(mesh.warnings.iter().cloned());
    }
    finish_payload(data, &outer, reader, &payload_children, &mut warnings)?;

    let path_delta = path_to.vector_from(path_from);
    let path_length = path_delta.norm();
    if !path_length.is_finite() || path_length <= 0.0 {
        return Err(error(
            version_offset,
            "extrusion path is not finite and distinct",
        ));
    }
    let tangent = path_delta.scale(1.0 / path_length);
    require_unit(up, version_offset, "extrusion up vector")?;
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

    let source_boundaries = split_profiles(profile, profile_count as usize, version_offset)?;
    let xaxis = normalize(
        up.cross(tangent),
        version_offset,
        "extrusion profile X axis",
    )?;
    let cap_origins = [
        path_from.translated(path_delta, trim[0]),
        path_from.translated(path_delta, trim[1]),
    ];
    let direction = cap_origins[1].vector_from(cap_origins[0]);
    let mut boundaries = Vec::with_capacity(source_boundaries.len());
    let mut laterals = Vec::with_capacity(source_boundaries.len());
    let mut orientations = Vec::with_capacity(source_boundaries.len());
    for source in source_boundaries {
        orientations.push(exact_orientation(&source, version_offset)?);
        let source_nurbs = exact_nurbs(&source, version_offset)?;
        require_profile_plane(&source_nurbs, version_offset)?;
        let start_nurbs = transform_nurbs(
            &source_nurbs,
            cap_origins[0],
            xaxis,
            up,
            tangent,
            active_miters[0],
            version_offset,
        )?;
        let end_nurbs = transform_nurbs(
            &source_nurbs,
            cap_origins[1],
            xaxis,
            up,
            tangent,
            active_miters[1],
            version_offset,
        )?;
        let start_curve = DecodedCurve::leaf(
            CurveGeometry::Nurbs(start_nurbs.clone()),
            source.warnings().to_vec(),
        );
        let start_frame = cap_frame(xaxis, up, tangent, active_miters[0], version_offset)?;
        let end_frame = cap_frame(xaxis, up, tangent, active_miters[1], version_offset)?;
        let start_pcurve = cap_pcurve(&start_nurbs, cap_origins[0], start_frame, version_offset)?;
        let end_pcurve = cap_pcurve(&end_nurbs, cap_origins[1], end_frame, version_offset)?;
        laterals.push(crate::surfaces::extrusion_nurbs(
            &start_nurbs,
            &end_nurbs,
            path_domain,
            transposed,
            version_offset,
        )?);
        boundaries.push(ExtrusionBoundary {
            start_curve,
            start_nurbs,
            end_nurbs,
            start_pcurve,
            end_pcurve,
        });
    }
    if (orientations.len() > 1
        && (orientations.first() != Some(&1)
            || orientations.iter().skip(1).any(|value| *value != -1)))
        || (orientations.len() == 1 && !matches!(orientations[0], 0 | 1))
    {
        return Err(error(
            version_offset,
            "extrusion profile orientations are invalid",
        ));
    }
    let all_closed = orientations.iter().all(|orientation| *orientation != 0);
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
        cap_frame(xaxis, up, tangent, active_miters[0], version_offset)?,
        cap_frame(xaxis, up, tangent, active_miters[1], version_offset)?,
    ];
    Ok(DecodedExtrusion {
        boundaries,
        laterals,
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
    profile: DecodedCurve,
    profile_count: usize,
    offset: usize,
) -> Result<Vec<DecodedCurve>, GeometryError> {
    if profile_count == 1 {
        return Ok(vec![profile]);
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
    Ok(children.into_iter().map(|(_, child)| child).collect())
}

fn exact_orientation(curve: &DecodedCurve, offset: usize) -> Result<i8, GeometryError> {
    let curve = exact_nurbs(curve, offset)?;
    if curve.control_points().len() < 2 || curve.degree() == 0 {
        return Err(error(offset, "extrusion profile closure is degenerate"));
    }
    if curve.control_points().iter().any(|point| point.z != 0.0) {
        return Err(error(offset, "extrusion profile is not in the XY plane"));
    }
    let domain = nurbs_curve_parameter_domain(&curve)
        .ok_or_else(|| error(offset, "extrusion profile parameter domain is invalid"))?;
    let start = evaluate_profile_point(&curve, domain[0], offset)?;
    let end = evaluate_profile_point(&curve, domain[1], offset)?;
    if !source_periodic(&curve) {
        if !points_coincident(start, end) {
            return Ok(0);
        }
        let one_third =
            evaluate_profile_point(&curve, domain[0] + (domain[1] - domain[0]) / 3.0, offset)?;
        let two_thirds = evaluate_profile_point(
            &curve,
            domain[0] + 2.0 * (domain[1] - domain[0]) / 3.0,
            offset,
        )?;
        if points_coincident(start, one_third)
            || points_coincident(start, two_thirds)
            || points_coincident(end, one_third)
            || points_coincident(end, two_thirds)
        {
            return Ok(0);
        }
    }

    let degree = usize::try_from(curve.degree())
        .map_err(|_| error(offset, "extrusion profile degree is too large"))?;
    let span_count = curve
        .knots()
        .windows(2)
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
    for pair in curve.knots().windows(2) {
        let span_start = pair[0].max(domain[0]);
        let span_end = pair[1].min(domain[1]);
        if span_start >= span_end {
            continue;
        }
        for sample in 0..samples_per_span {
            let fraction = sample as f64 / samples_per_span as f64;
            let parameter = span_start + fraction * (span_end - span_start);
            let current = evaluate_profile_point(&curve, parameter, offset)?;
            twice_area += (previous.x - current.x) * (previous.y + current.y);
            previous = current;
        }
    }
    let final_point = evaluate_profile_point(&curve, domain[1], offset)?;
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

fn source_periodic(curve: &NurbsCurve) -> bool {
    if !curve.periodic() || curve.degree() <= 1 {
        return false;
    }
    let degree = curve.degree() as usize;
    curve.control_points().len() >= degree
        && (0..degree).all(|offset| {
            points_coincident(
                curve.control_points()[degree - 1 - offset],
                curve.control_points()[curve.control_points().len() - 1 - offset],
            )
        })
}

fn evaluate_profile_point(
    curve: &NurbsCurve,
    parameter: f64,
    offset: usize,
) -> Result<Point3, GeometryError> {
    nurbs_curve_point(
        curve.degree(),
        curve.knots(),
        curve.control_points(),
        curve.weights(),
        parameter,
    )
    .filter(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
    .ok_or_else(|| error(offset, "extrusion profile cannot be evaluated"))
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

fn require_profile_plane(curve: &NurbsCurve, offset: usize) -> Result<(), GeometryError> {
    if curve.control_points().iter().any(|point| point.z != 0.0) {
        return Err(error(offset, "extrusion profile is not in the XY plane"));
    }
    Ok(())
}

fn transform_nurbs(
    curve: &NurbsCurve,
    origin: Point3,
    xaxis: Vector3,
    yaxis: Vector3,
    zaxis: Vector3,
    miter: Option<Vector3>,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let mut result = curve.clone();
    for point in result.control_points_mut() {
        *point = transform_local(*point, origin, xaxis, yaxis, zaxis, miter, offset)?;
    }
    Ok(result)
}

fn transform_local(
    point: Point3,
    origin: Point3,
    xaxis: Vector3,
    yaxis: Vector3,
    zaxis: Vector3,
    miter: Option<Vector3>,
    offset: usize,
) -> Result<Point3, GeometryError> {
    if point.z != 0.0 {
        return Err(error(
            offset,
            "extrusion profile pole is outside the XY plane",
        ));
    }
    let local = mitered_local(Vector3::new(point.x, point.y, 0.0), miter, offset)?;
    Ok(origin.translated(
        xaxis.scale(local.x) + yaxis.scale(local.y) + zaxis.scale(local.z),
        1.0,
    ))
}

fn cap_frame(
    xaxis: Vector3,
    yaxis: Vector3,
    zaxis: Vector3,
    miter: Option<Vector3>,
    offset: usize,
) -> Result<(Vector3, Vector3, Vector3), GeometryError> {
    let local_x = mitered_local(Vector3::new(1.0, 0.0, 0.0), miter, offset)?;
    let local_y = mitered_local(Vector3::new(0.0, 1.0, 0.0), miter, offset)?;
    let world_x = local_to_world_vector(local_x, xaxis, yaxis, zaxis);
    let world_y = local_to_world_vector(local_y, xaxis, yaxis, zaxis);
    let u = normalize(world_x, offset, "extrusion cap U axis")?;
    let normal = normalize(world_x.cross(world_y), offset, "extrusion cap normal")?;
    let v = normalize(normal.cross(u), offset, "extrusion cap V axis")?;
    Ok((u, v, normal))
}

fn cap_pcurve(
    curve: &NurbsCurve,
    origin: Point3,
    frame: (Vector3, Vector3, Vector3),
    offset: usize,
) -> Result<CapPcurve, GeometryError> {
    let mut points = Vec::with_capacity(curve.control_points().len());
    for point in curve.control_points() {
        let delta = point.vector_from(origin);
        let distance = delta.dot(frame.2);
        if distance.abs() > EPS_EXTRUSION_POSITION {
            return Err(error(offset, "extrusion cap boundary is not planar"));
        }
        points.push(Point2::new(delta.dot(frame.0), delta.dot(frame.1)));
    }
    Ok(CapPcurve {
        degree: curve.degree(),
        knots: curve.knots().to_vec(),
        control_points: points,
        weights: curve.weights().map(<[f64]>::to_vec),
        periodic: curve.periodic(),
    })
}

fn mitered_local(
    point: Vector3,
    normal: Option<Vector3>,
    offset: usize,
) -> Result<Vector3, GeometryError> {
    let Some(normal) = normal else {
        return Ok(point);
    };
    if normal.x == 0.0 && normal.y == 0.0 {
        return Ok(point);
    }
    let axis = normalize(
        Vector3::new(-normal.y, normal.x, 0.0),
        offset,
        "extrusion miter rotation axis",
    )?;
    let c = 1.0 - 1.0 / normal.z;
    let scaled = Vector3::new(
        (1.0 - c * axis.y * axis.y) * point.x + c * axis.x * axis.y * point.y,
        c * axis.x * axis.y * point.x + (1.0 - c * axis.x * axis.x) * point.y,
        point.z,
    );
    Ok(rodrigues(scaled, axis, normal.z.acos()))
}

#[allow(clippy::too_many_arguments)]
fn read_mesh_cache(
    expand: crate::mesh::MeshExpand<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: f64,
    mesh_budget: &mut crate::mesh::MeshBudget,
    warnings: &mut Vec<String>,
) -> Result<Vec<crate::mesh::DecodedMesh>, GeometryError> {
    let cache = anonymous_chunk(data, reader, archive, "extrusion mesh cache")?;
    let mut cache_reader = BoundedReader::new(data, cache.body.start, cache.body.end)?;
    require_anonymous_version(&mut cache_reader, 1, 0, "extrusion mesh cache")?;
    let mut meshes = Vec::new();
    let mut cache_children = Vec::new();
    let mut index = 0_usize;
    loop {
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
        let item = anonymous_chunk(data, &mut cache_reader, archive, "mesh-cache item")?;
        cache_children.push(item.range());
        let mut item_reader = BoundedReader::new(data, item.body.start, item.body.end)?;
        require_anonymous_version(&mut item_reader, 1, 0, "mesh-cache item")?;
        item_reader.skip(16)?;
        let wrapper_start = item_reader.position();
        let wrapper = chunk_at(data, wrapper_start, item_reader.end(), archive, false)?;
        let (class, userdata) =
            parse_class_wrapper_with_userdata(data, wrapper.range(), archive, warnings)?;
        item_reader.skip(wrapper.next_offset - wrapper_start)?;
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
                id: format!("rhino:extrusion:mesh-cache#{index}"),
                scale,
                userdata: &userdata,
            },
            mesh_budget,
        )?;
        meshes.push(mesh);
        finish_anonymous(
            data,
            &mut cache_reader,
            &item,
            item_reader,
            std::slice::from_ref(&wrapper.range()),
            "mesh-cache item",
            warnings,
        )?;
        index = index
            .checked_add(1)
            .ok_or_else(|| error(wrapper_start, "mesh-cache item count overflow"))?;
    }
    finish_anonymous(
        data,
        reader,
        &cache,
        cache_reader,
        &cache_children,
        "extrusion mesh cache",
        warnings,
    )?;
    Ok(meshes)
}

#[allow(clippy::too_many_arguments)]
fn read_v5_mesh_cache(
    expand: crate::mesh::MeshExpand<'_>,
    data: &[u8],
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: f64,
    userdata: &[UserdataDescriptor],
    mesh_budget: &mut crate::mesh::MeshBudget,
    warnings: &mut Vec<String>,
) -> Result<Vec<crate::mesh::DecodedMesh>, GeometryError> {
    let Some(cache) = userdata.iter().find(|value| {
        value.class_uuid() == ON_V5_EXTRUSION_DISPLAY_MESH_CACHE
            && value.item_uuid() == ON_V5_EXTRUSION_DISPLAY_MESH_CACHE
    }) else {
        return Ok(Vec::new());
    };

    let mut offset = cache.payload_range().start;
    let mut meshes = Vec::new();
    for index in 0..3_usize {
        let wrapper = chunk_at(data, offset, cache.payload_range().end, archive, false)?;
        let (class, nested_userdata) =
            parse_class_wrapper_with_userdata(data, wrapper.range(), archive, warnings)?;
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
                        id: format!("rhino:extrusion:v5-mesh-cache#{index}"),
                        scale,
                        userdata: &nested_userdata,
                    },
                    mesh_budget,
                )?;
                meshes.push(mesh);
            } else if class.class_uuid != Uuid::nil() {
                return Err(error(
                    wrapper.header_start,
                    "V5 extrusion mesh-cache item is not ON_Mesh or null",
                ));
            }
        }
        offset = wrapper.next_offset;
    }
    // `ON_V5ExtrusionDisplayMeshCache::Read` returns after its three
    // `ReadObject` calls. The enclosing anonymous-chunk end operation then
    // skips any later bounded suffix, so preserve that forward-compatible
    // boundary instead of rejecting the optional cache.
    Ok(meshes)
}

fn anonymous_chunk(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    name: &str,
) -> Result<Chunk, GeometryError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(error(
            chunk.header_start,
            &format!("expected anonymous {name} chunk"),
        ));
    }
    Ok(chunk)
}

fn finish_anonymous(
    data: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    mut child: BoundedReader<'_>,
    children: &[std::ops::Range<usize>],
    name: &str,
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    child.skip_remaining()?;
    let direct = crate::chunks::direct_checksum_ranges(&chunk.body, children)?;
    if matches!(
        crate::chunks::verify_checksum_ranges(data, chunk, &direct)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "{name} CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    parent.skip(chunk.next_offset - parent.position())?;
    Ok(())
}

fn finish_payload(
    data: &[u8],
    chunk: &Chunk,
    mut reader: BoundedReader<'_>,
    children: &[std::ops::Range<usize>],
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    reader.skip_remaining()?;
    let direct = crate::chunks::direct_checksum_ranges(&chunk.body, children)?;
    if matches!(
        crate::chunks::verify_checksum_ranges(data, chunk, &direct)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "extrusion payload CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    Ok(())
}

fn require_anonymous_version(
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
            format!("unsupported {name} version"),
        ));
    }
    Ok(())
}

fn increasing_interval(
    value: [f64; 2],
    offset: usize,
    name: &str,
) -> Result<[f64; 2], GeometryError> {
    if value.iter().all(|entry| entry.is_finite()) && value[0] < value[1] {
        Ok(value)
    } else {
        Err(error(offset, &format!("extrusion {name} is invalid")))
    }
}

fn require_unit(value: Vector3, offset: usize, name: &str) -> Result<(), GeometryError> {
    let length = value.norm();
    if value.x.is_finite()
        && value.y.is_finite()
        && value.z.is_finite()
        && (length - 1.0).abs() <= UNIT_TOLERANCE
    {
        Ok(())
    } else {
        Err(error(offset, &format!("{name} is not unit")))
    }
}

fn active_miter(present: bool, value: Vector3) -> Option<Vector3> {
    if !present {
        return None;
    }
    let length = value.norm();
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let unit = value.scale(1.0 / length);
    (unit.z > MITER_Z_MINIMUM).then_some(unit)
}

fn normalize(value: Vector3, offset: usize, name: &str) -> Result<Vector3, GeometryError> {
    let length = value.norm();
    if !length.is_finite() || length <= 0.0 {
        return Err(error(offset, &format!("{name} is invalid")));
    }
    Ok(value.scale(1.0 / length))
}

fn scaled_point(value: crate::settings::Point3, scale: f64) -> Option<Point3> {
    Some(Point3::new(
        crate::wire::scaled_coordinate(value.0[0], scale)?,
        crate::wire::scaled_coordinate(value.0[1], scale)?,
        crate::wire::scaled_coordinate(value.0[2], scale)?,
    ))
}

fn ir_vector(value: crate::settings::Vector3) -> Vector3 {
    Vector3::new(value.0[0], value.0[1], value.0[2])
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
pub(crate) mod tests {
    use super::*;
    use crate::chunks::ArchiveVersion;
    use crate::curves::DecodedCurve;
    use crate::layout::anonymous_version_prefix as anon_ver;
    use crate::layout::long_chunk_header_wide as long_wide;
    use crate::layout::uuid_wire_form as uuid_wire;
    use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn decode(
        data: &[u8],
        range: std::ops::Range<usize>,
        archive: ArchiveVersion,
        writer_version: Option<i64>,
        scale: f64,
        mesh_budget: &mut crate::mesh::MeshBudget,
    ) -> Result<super::DecodedExtrusion, GeometryError> {
        crate::decode::with_expand_bytes(data, |expand| {
            super::decode(
                expand,
                data,
                range,
                archive,
                writer_version,
                scale,
                &[],
                mesh_budget,
            )
        })
    }

    fn push_i32(bytes: &mut Vec<u8>, value: i32) {
        bytes.extend(value.to_le_bytes());
    }

    fn push_f64(bytes: &mut Vec<u8>, value: f64) {
        bytes.extend(value.to_le_bytes());
    }

    fn long(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut result = typecode.to_le_bytes().to_vec();
        result.extend((body.len() as i64).to_le_bytes());
        result.extend(body);
        result
    }

    fn crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut payload = body.to_vec();
        payload.extend(crc32fast::hash(body).to_le_bytes());
        long(typecode, &payload)
    }

    fn crc_chunk_excluding(
        typecode: u32,
        body: &[u8],
        children: &[std::ops::Range<usize>],
    ) -> Vec<u8> {
        let direct = crate::chunks::direct_checksum_ranges(&(0..body.len()), children)
            .expect("valid test child ranges");
        let mut hasher = crc32fast::Hasher::new();
        for range in direct {
            hasher.update(&body[range]);
        }
        let mut payload = body.to_vec();
        payload.extend(hasher.finalize().to_le_bytes());
        long(typecode, &payload)
    }

    fn polyline_wrapper(clockwise: bool, closed: bool) -> Vec<u8> {
        let mut points = vec![
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        if closed {
            points.push(points[0]);
        }
        if clockwise {
            points.reverse();
        }
        let point_count = points.len();
        let mut payload = vec![0x10];
        push_i32(&mut payload, point_count as i32);
        for point in points {
            for value in point {
                push_f64(&mut payload, value);
            }
        }
        push_i32(&mut payload, point_count as i32);
        for value in 0..point_count {
            push_f64(
                &mut payload,
                f64::from(u32::try_from(value).expect("required invariant")),
            );
        }
        push_i32(&mut payload, 2);
        let wire_uuid = [
            0xe6, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ];
        let mut class_body = crc_chunk(0x0002_fffb, &wire_uuid);
        class_body.extend(crc_chunk(0x0002_fffc, &payload));
        class_body.extend(0x8002_7fff_u32.to_le_bytes());
        class_body.extend(0_i64.to_le_bytes());
        long(0x0002_7ffa, &class_body)
    }

    fn polycurve_wrapper() -> Vec<u8> {
        let children = [polyline_wrapper(false, true), polyline_wrapper(true, true)];
        let mut payload = vec![0x10];
        push_i32(&mut payload, 2);
        push_i32(&mut payload, 0);
        push_i32(&mut payload, 0);
        payload.extend([0_u8; 48]);
        push_i32(&mut payload, 3);
        for value in [0.0, 1.0, 2.0] {
            push_f64(&mut payload, value);
        }
        payload.extend(children.concat());
        let wire_uuid = [
            0xe0, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ];
        let mut class_body = crc_chunk(0x0002_fffb, &wire_uuid);
        class_body.extend(crc_chunk(0x0002_fffc, &payload));
        class_body.extend(0x8002_7fff_u32.to_le_bytes());
        class_body.extend(0_i64.to_le_bytes());
        long(0x0002_7ffa, &class_body)
    }

    fn payload(minor: i32, caps: [bool; 2], cache: Option<Vec<u8>>) -> Vec<u8> {
        payload_with_profile(minor, caps, cache, polyline_wrapper(false, true))
    }

    fn payload_with_profile(
        minor: i32,
        caps: [bool; 2],
        cache: Option<Vec<u8>>,
        profile: Vec<u8>,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        push_i32(&mut body, 1);
        push_i32(&mut body, minor);
        let profile_range = body.len()..body.len() + profile.len();
        body.extend(profile);
        for value in [10.0, 20.0, 30.0, 10.0, 20.0, 40.0] {
            push_f64(&mut body, value);
        }
        for value in [0.25, 0.75] {
            push_f64(&mut body, value);
        }
        for value in [0.0, 1.0, 0.0] {
            push_f64(&mut body, value);
        }
        body.extend([0, 0]);
        for value in [0.0; 6] {
            push_f64(&mut body, value);
        }
        for value in [4.0, 9.0] {
            push_f64(&mut body, value);
        }
        body.push(0);
        if minor >= 1 {
            push_i32(&mut body, 1);
        }
        if minor >= 2 {
            body.push(u8::from(caps[0]));
            body.push(u8::from(caps[1]));
        }
        let mut children = vec![profile_range];
        if minor >= 3 {
            let cache = cache.unwrap_or_else(empty_mesh_cache);
            children.push(body.len()..body.len() + cache.len());
            body.extend(cache);
        }
        crc_chunk_excluding(ANONYMOUS, &body, &children)
    }

    pub(crate) fn archive_payload(
        minor: i32,
        caps: [bool; 2],
        with_hole: bool,
        mesh_cache: bool,
    ) -> Vec<u8> {
        let profile = if with_hole {
            polycurve_wrapper()
        } else {
            polyline_wrapper(false, true)
        };
        let cache = (minor >= 3).then(|| {
            if mesh_cache {
                one_mesh_cache()
            } else {
                empty_mesh_cache()
            }
        });
        let profile_len = profile.len();
        let mut payload = payload_with_profile(minor, caps, cache, profile);
        if with_hole {
            let cap_bytes = usize::from(minor >= 2) * 2;
            let cache_bytes = if minor >= 3 {
                if mesh_cache {
                    one_mesh_cache().len()
                } else {
                    empty_mesh_cache().len()
                }
            } else {
                0
            };
            let count = payload.len() - 4 - cap_bytes - cache_bytes - 4;
            payload[count..count + 4].copy_from_slice(&2_i32.to_le_bytes());
            let body = &payload[12..payload.len() - 4];
            #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
            let mut children = vec![8..8 + profile_len];
            if cache_bytes != 0 {
                children.push(body.len() - cache_bytes..body.len());
            }
            let direct = crate::chunks::direct_checksum_ranges(&(0..body.len()), &children)
                .expect("valid extrusion children");
            let mut hasher = crc32fast::Hasher::new();
            for range in direct {
                hasher.update(&body[range]);
            }
            let crc = hasher.finalize();
            let end = payload.len();
            payload[end - 4..].copy_from_slice(&crc.to_le_bytes());
        }
        payload
    }

    fn empty_mesh_cache() -> Vec<u8> {
        let mut body = Vec::new();
        push_i32(&mut body, 1);
        push_i32(&mut body, 0);
        body.push(0);
        crc_chunk(ANONYMOUS, &body)
    }

    fn one_mesh_wrapper() -> Vec<u8> {
        let mut mesh = vec![0x30];
        push_i32(&mut mesh, 1);
        push_i32(&mut mesh, 0);
        for _ in 0..8 {
            push_f64(&mut mesh, 0.0);
        }
        for _ in 0..2 {
            push_f64(&mut mesh, 1.0);
        }
        for _ in 0..16 {
            mesh.extend(0.0_f32.to_le_bytes());
        }
        push_i32(&mut mesh, 0);
        mesh.extend([0, 0, 0, 0, 0]);
        push_i32(&mut mesh, 1);
        let vertex = [0_u8; 12];
        mesh.extend((vertex.len() as u32).to_le_bytes());
        mesh.extend(crc32fast::hash(&vertex).to_le_bytes());
        mesh.push(0);
        mesh.extend(vertex);
        for _ in 0..4 {
            mesh.extend(0_u32.to_le_bytes());
        }
        let mesh_uuid = [
            0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ];
        let mut class_body = crc_chunk(0x0002_fffb, &mesh_uuid);
        class_body.extend(crc_chunk(0x0002_fffc, &mesh));
        class_body.extend(0x8002_7fff_u32.to_le_bytes());
        class_body.extend(0_i64.to_le_bytes());
        long(0x0002_7ffa, &class_body)
    }

    fn one_mesh_cache() -> Vec<u8> {
        let wrapper = one_mesh_wrapper();
        let wrapper_len = wrapper.len();
        let mut item = Vec::new();
        push_i32(&mut item, 1);
        push_i32(&mut item, 0);
        item.extend([7; uuid_wire::LEN]);
        item.extend(wrapper);
        let wrapper_range =
            anon_ver::LEN + uuid_wire::LEN..anon_ver::LEN + uuid_wire::LEN + wrapper_len;
        let item = crc_chunk_excluding(ANONYMOUS, &item, &[wrapper_range]);
        let item_len = item.len();
        let mut cache = Vec::new();
        push_i32(&mut cache, 1);
        push_i32(&mut cache, 0);
        cache.push(1);
        cache.extend(item);
        cache.push(0);
        #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
        let wrapped = crc_chunk_excluding(ANONYMOUS, &cache, &[9..9 + item_len]);
        wrapped
    }

    fn null_object_wrapper() -> Vec<u8> {
        let uuid = crc_chunk(0x0002_fffb, &[0; 16]);
        long(0x0002_7ffa, &uuid)
    }

    fn decoded_polygon(clockwise: bool, closed: bool) -> DecodedCurve {
        let mut points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        if closed {
            points.push(points[0]);
        }
        if clockwise {
            points.reverse();
        }
        let count = points.len();
        DecodedCurve::leaf(
            CurveGeometry::Nurbs(
                NurbsCurve::new(
                    1,
                    (0..count + 2).map(|value| value as f64).collect(),
                    points,
                    None,
                    false,
                )
                .expect("valid polygon curve"),
            ),
            Vec::new(),
        )
    }

    fn decoded_quadratic_circle(clockwise: bool) -> DecodedCurve {
        let radius = 2.0;
        let mut points = vec![
            Point3::new(radius, 0.0, 0.0),
            Point3::new(radius, radius, 0.0),
            Point3::new(0.0, radius, 0.0),
            Point3::new(-radius, radius, 0.0),
            Point3::new(-radius, 0.0, 0.0),
            Point3::new(-radius, -radius, 0.0),
            Point3::new(0.0, -radius, 0.0),
            Point3::new(radius, -radius, 0.0),
            Point3::new(radius, CLOSURE_ABSOLUTE_TOLERANCE / 2.0, 0.0),
        ];
        let mut weights = vec![
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
        ];
        if clockwise {
            points.reverse();
            weights.reverse();
        }
        DecodedCurve::leaf(
            CurveGeometry::Nurbs(
                NurbsCurve::new(
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
                    points,
                    Some(weights),
                    false,
                )
                .expect("valid circle curve"),
            ),
            Vec::new(),
        )
    }

    #[test]
    fn anonymous_versions_1_0_through_1_3_apply_exact_gates_and_defaults() {
        for minor in 0..=3 {
            let bytes = payload(minor, [true, false], None);
            let decoded = decode(
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                None,
                1.0,
                &mut crate::mesh::MeshBudget::new(),
            )
            .expect("required invariant");
            assert_eq!(decoded.boundaries.len(), 1);
            assert_eq!(decoded.laterals[0].v_knots(), vec![4.0, 4.0, 9.0, 9.0]);
            assert_eq!(
                decoded.caps,
                if minor < 2 {
                    [true, true]
                } else {
                    [true, false]
                }
            );
            assert!(decoded.meshes.is_empty());
            assert!(decoded.warnings.is_empty());
        }
    }

    #[test]
    fn nil_profile_is_rejected_before_extrusion_transfer() {
        let bytes = payload_with_profile(2, [false, false], None, null_object_wrapper());
        assert!(decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .is_err());
    }

    #[test]
    fn profile_frame_uses_trim_start_up_cross_path_and_scales_once() {
        let bytes = payload(2, [false, false], None);
        let decoded = decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            25.4,
            &mut crate::mesh::MeshBudget::new(),
        )
        .expect("required invariant");
        assert_eq!(decoded.cap_origins[0], Point3::new(254.0, 508.0, 825.5));
        assert_eq!(decoded.cap_origins[1], Point3::new(254.0, 508.0, 952.5));
        let first = decoded.boundaries[0].start_nurbs.control_points()[1];
        assert_eq!(first, Point3::new(304.8, 508.0, 825.5));
        assert_eq!(decoded.direction, Vector3::new(0.0, 0.0, 127.0));
    }

    #[test]
    fn single_open_profile_is_exact_when_uncapped_and_rejected_when_capped() {
        let open = polyline_wrapper(false, false);
        let legacy = payload_with_profile(0, [false, false], None, open.clone());
        let decoded = decode(
            &legacy,
            0..legacy.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .expect("required invariant");
        assert_eq!(decoded.caps, [false, false]);
        assert_eq!(decoded.laterals.len(), 1);
        let capped = payload_with_profile(2, [true, false], None, open);
        assert!(decode(
            &capped,
            0..capped.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .is_err());
    }

    #[test]
    fn orientation_supports_polygon_rational_and_open_profiles() {
        assert_eq!(
            exact_orientation(&decoded_polygon(false, true), 0).expect("required invariant"),
            1
        );
        assert_eq!(
            exact_orientation(&decoded_polygon(true, true), 0).expect("required invariant"),
            -1
        );
        assert_eq!(
            exact_orientation(&decoded_polygon(false, false), 0).expect("required invariant"),
            0
        );
        assert_eq!(
            exact_orientation(&decoded_quadratic_circle(false), 0).expect("required invariant"),
            1
        );
        assert_eq!(
            exact_orientation(&decoded_quadratic_circle(true), 0).expect("required invariant"),
            -1
        );
        let mut off_plane = decoded_polygon(false, true);
        let crate::curves::DecodedCurve::Leaf {
            geometry: CurveGeometry::Nurbs(curve),
            ..
        } = &mut off_plane
        else {
            unreachable!()
        };
        curve.control_points_mut()[1].z = 1.0;
        assert!(exact_orientation(&off_plane, 0).is_err());
    }

    #[test]
    fn multiple_profiles_require_exact_polycurve_count_and_outer_hole_orientation() {
        let outer = decoded_polygon(false, true);
        let inner = decoded_polygon(true, true);
        let profile =
            DecodedCurve::from_polycurve_parts(vec![outer, inner], vec![0.0, 1.0, 2.0], Vec::new());
        assert_eq!(
            split_profiles(profile.clone(), 2, 0)
                .expect("required invariant")
                .len(),
            2
        );
        assert!(split_profiles(profile, 3, 0).is_err());
    }

    #[test]
    fn strict_flags_trim_domains_and_later_minor_versions_are_accepted() {
        let valid = payload(2, [false, false], None);
        let body_start = long_wide::LEN;
        let profile_len = polyline_wrapper(false, true).len();
        let common = body_start + anon_ver::LEN + profile_len;
        let trim_start = common + 48;
        let up_start = trim_start + 16;
        let miter_flags = up_start + 24;
        let path_domain = miter_flags + 2 + 48;
        let mut cases = Vec::new();
        let mut noncanonical_bool = valid.clone();
        noncanonical_bool[miter_flags] = 2;
        assert!(decode(
            &noncanonical_bool,
            0..noncanonical_bool.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .is_ok());
        let mut bad_trim = valid.clone();
        bad_trim[trim_start..trim_start + 8].copy_from_slice(&(-0.1_f64).to_le_bytes());
        cases.push(bad_trim);
        let mut bad_domain = valid.clone();
        bad_domain[path_domain + 8..path_domain + 16].copy_from_slice(&4.0_f64.to_le_bytes());
        cases.push(bad_domain);
        for bytes in cases {
            assert!(decode(
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                None,
                1.0,
                &mut crate::mesh::MeshBudget::new(),
            )
            .is_err());
        }
        let mut future = payload(3, [false, false], Some(one_mesh_cache()));
        future[body_start + anon_ver::MINOR..body_start + anon_ver::LEN]
            .copy_from_slice(&4_i32.to_le_bytes());
        assert!(decode(
            &future,
            0..future.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .is_ok());
    }

    #[test]
    fn optional_mesh_cache_over_document_budget_is_dropped() {
        let bytes = payload(3, [false, false], Some(one_mesh_cache()));
        let decoded = decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::with_limit(0),
        )
        .expect("extrusion remains usable");
        assert!(decoded.meshes.is_empty());
        assert_eq!(decoded.warnings.len(), 1);
        assert!(decoded.warnings[0].contains("document mesh buffer budget exceeded"));
    }

    #[test]
    fn active_miter_unitizes_and_applies_only_above_the_z_threshold() {
        assert_eq!(
            active_miter(true, Vector3::new(0.0, 1.2, 1.6)),
            Some(Vector3::new(0.0, 0.6, 0.8))
        );
        assert_eq!(active_miter(true, Vector3::new(1.0, 0.0, 0.01)), None);
        assert_eq!(active_miter(true, Vector3::new(0.0, 0.0, 0.0)), None);
        assert!(mitered_local(
            Vector3::new(1.0, 0.0, 0.0),
            Some(Vector3::new(0.0, 0.6, 0.8)),
            0
        )
        .is_ok());
        let plain = cap_frame(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            None,
            0,
        )
        .expect("required invariant");
        let mitered = cap_frame(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Some(Vector3::new(0.0, 0.6, 0.8)),
            0,
        )
        .expect("required invariant");
        assert_eq!(plain.2, Vector3::new(0.0, 0.0, 1.0));
        assert!((mitered.2.y - 0.6).abs() < 1.0e-12);
        assert!((mitered.2.z - 0.8).abs() < 1.0e-12);
    }

    #[test]
    fn malformed_mesh_cache_is_dropped_without_losing_analytic_geometry() {
        let malformed = crc_chunk(ANONYMOUS, &[1, 0, 0, 0, 0, 0, 0, 0, 2]);
        let bytes = payload(3, [false, false], Some(malformed));
        let decoded = decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .expect("required invariant");
        assert_eq!(decoded.laterals.len(), 1);
        assert!(decoded.meshes.is_empty());
        assert_eq!(decoded.warnings.len(), 1);
    }

    #[test]
    fn valid_mesh_cache_item_reuses_bounded_mesh_decoder() {
        let bytes = payload(3, [false, false], Some(one_mesh_cache()));
        let decoded = decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .expect("required invariant");
        assert_eq!(decoded.laterals.len(), 1);
        assert_eq!(decoded.meshes.len(), 1);
        assert!(decoded.warnings.is_empty());
    }

    #[test]
    fn v5_mesh_cache_consumes_two_mesh_slots_and_a_null_slot() {
        let mut bytes = one_mesh_wrapper();
        bytes.extend(null_object_wrapper());
        bytes.extend(null_object_wrapper());
        let descriptor = crate::objects::UserdataDescriptor::Known {
            range: 0..bytes.len(),
            version: (2, 2),
            class_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            item_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: 0..bytes.len(),
        };
        let result = crate::decode::with_expand_bytes(&bytes, |expand| {
            read_v5_mesh_cache(
                expand,
                &bytes,
                ArchiveVersion::V5,
                None,
                1.0,
                std::slice::from_ref(&descriptor),
                &mut crate::mesh::MeshBudget::new(),
                &mut Vec::new(),
            )
        })
        .expect("V5 mesh cache");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn v5_mesh_cache_skips_a_bounded_suffix_after_three_slots() {
        let mut bytes = one_mesh_wrapper();
        bytes.extend(null_object_wrapper());
        bytes.extend(null_object_wrapper());
        bytes.extend([0xa5, 0x5a]);
        let descriptor = crate::objects::UserdataDescriptor::Known {
            range: 0..bytes.len(),
            version: (2, 2),
            class_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            item_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: 0..bytes.len(),
        };
        let result = crate::decode::with_expand_bytes(&bytes, |expand| {
            read_v5_mesh_cache(
                expand,
                &bytes,
                ArchiveVersion::V5,
                None,
                1.0,
                std::slice::from_ref(&descriptor),
                &mut crate::mesh::MeshBudget::new(),
                &mut Vec::new(),
            )
        })
        .expect("V5 mesh cache suffix");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn payload_suffix_is_skipped() {
        let mut bytes = payload(2, [false, false], None);
        let crc_offset = bytes.len() - 4;
        bytes.insert(crc_offset, 0xff);
        let body_len = i64::from_le_bytes(bytes[4..12].try_into().expect("required invariant")) + 1;
        bytes[4..12].copy_from_slice(&body_len.to_le_bytes());
        let body = &bytes[12..bytes.len() - 4];
        let crc = crc32fast::hash(body);
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&crc.to_le_bytes());
        assert!(decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            1.0,
            &mut crate::mesh::MeshBudget::new(),
        )
        .is_ok());
    }
}
