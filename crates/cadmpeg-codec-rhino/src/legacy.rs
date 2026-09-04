// SPDX-License-Identifier: Apache-2.0
//! Rhino V1 flat geometry and direct-record decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, PcurveNurbs, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::TransferLedger;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, PcurveUse, Point, Region, Sense,
    Shell, Vertex,
};
use cadmpeg_ir::unknown::UnknownRecord;
use serde::Serialize;

use crate::chunks::{chunk_at, parse_header, ArchiveVersion, BoundedReader, FramingError};
use crate::layout::file_header;
use crate::loss::RhinoLossCode;

const TCODE_COMMENT: u32 = 0x0000_0001;
const TCODE_RH_POINT: u32 = 0x0010_0001;
const TCODE_LEGACY_CRV: u32 = 0x0001_0008;
const TCODE_LEGACY_CRVSTUFF: u32 = 0x0001_0108;
const TCODE_LEGACY_SPL: u32 = 0x0001_0009;
const TCODE_LEGACY_SPLSTUFF: u32 = 0x0001_0109;
const TCODE_LEGACY_SHL: u32 = 0x0001_0003;
const TCODE_LEGACY_FAC: u32 = 0x0001_0004;
const TCODE_LEGACY_BND: u32 = 0x0001_0005;
const TCODE_LEGACY_TRM: u32 = 0x0001_0006;
const TCODE_LEGACY_SRF: u32 = 0x0001_0007;
const TCODE_LEGACY_SHLSTUFF: u32 = 0x0001_0103;
const TCODE_LEGACY_FACSTUFF: u32 = 0x0001_0104;
const TCODE_LEGACY_BNDSTUFF: u32 = 0x0001_0105;
const TCODE_LEGACY_TRMSTUFF: u32 = 0x0001_0106;
const TCODE_LEGACY_SRFSTUFF: u32 = 0x0001_0107;
const TCODE_MESH_OBJECT: u32 = 0x0010_0015;
const TCODE_COMPRESSED_MESH_GEOMETRY: u32 = 0x0010_0017;
const TCODE_UNIT_AND_TOLERANCES: u32 = 0x0200_0010;
const TCODE_NAMED_CPLANE: u32 = 0x0200_0004;
const TCODE_NAMED_VIEW: u32 = 0x0200_0005;
const TCODE_VIEWPORT: u32 = 0x0200_0006;
const TCODE_ENDOFTABLE: u32 = 0xffff_ffff;
const TCODE_ENDOFFILE: u32 = 0x8000_7fff;
const TCODE_TEXT_BLOCK: u32 = 0x0020_0004;
const TCODE_LINEAR_DIMENSION: u32 = 0x0020_0006;
const TCODE_ANGULAR_DIMENSION: u32 = 0x0020_0007;
const TCODE_RADIAL_DIMENSION: u32 = 0x0020_0008;
const TCODE_ANNOTATION_LEADER: u32 = 0x0020_0005;
const TCODE_RHINOIO_OBJECT_NURBS_CURVE: u32 = 0x0002_0008;
const TCODE_RHINOIO_OBJECT_NURBS_SURFACE: u32 = 0x0002_0009;
const TCODE_RHINOIO_OBJECT_BREP: u32 = 0x0002_000b;
const TCODE_RHINOIO_OBJECT_DATA: u32 = 0x0002_fffe;

fn is_v1_presentation_setting(typecode: u32) -> bool {
    matches!(
        typecode,
        TCODE_NAMED_CPLANE | TCODE_NAMED_VIEW | TCODE_VIEWPORT
    )
}

#[derive(Debug, Serialize)]
struct V1String {
    text: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct V1Plane {
    origin: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
}

#[derive(Debug, Serialize)]
struct V1TextBlock {
    version: i32,
    type_flag: i32,
    plane: V1Plane,
    user_text: V1String,
    flags: i32,
    by_object: i32,
    face_name: V1String,
    face_weight: i32,
    height: f64,
    version_one_extra: Option<[f64; 2]>,
}

#[derive(Debug, Serialize)]
struct V1Leader {
    version: i32,
    type_flag: i32,
    plane: V1Plane,
    flags: i32,
    by_object: i32,
    points: Vec<[f64; 3]>,
}

#[derive(Debug, Serialize)]
struct V1LinearDimension {
    version: i32,
    annotation_type: i32,
    plane: V1Plane,
    points: Vec<[f64; 3]>,
    user_text: V1String,
    default_text: V1String,
    user_positioned_text: i32,
    flags: i32,
    by_object: i32,
}

#[derive(Debug, Serialize)]
struct V1AngularDimension {
    version: i32,
    annotation_type: i32,
    plane: V1Plane,
    angle: f64,
    radius: f64,
    extension_distances: [f64; 4],
    points: Vec<[f64; 3]>,
    user_text: V1String,
    default_text: V1String,
    user_positioned_text: i32,
    flags: i32,
    by_object: i32,
}

#[derive(Debug, Serialize)]
struct V1RadialDimension {
    version: i32,
    annotation_type: i32,
    plane: V1Plane,
    points: Vec<[f64; 3]>,
    user_text: V1String,
    default_text: V1String,
    user_positioned_text: i32,
    flags: i32,
    by_object: i32,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "fields")]
enum V1AnnotationPayload {
    TextBlock(V1TextBlock),
    Leader(V1Leader),
    LinearDimension(V1LinearDimension),
    AngularDimension(V1AngularDimension),
    RadialDimension(V1RadialDimension),
}

#[derive(Debug, Serialize)]
struct V1NurbsCurve {
    wire_version: i32,
    version: i32,
    dimension: i32,
    rational: bool,
    order: i32,
    knots: Vec<f64>,
    control_values: Vec<Vec<f64>>,
}

#[derive(Debug, Serialize)]
struct V1NurbsSurface {
    wire_version: i32,
    version: i32,
    dimension: i32,
    rational: bool,
    orders: [i32; 2],
    control_counts: [i32; 2],
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    control_values: Vec<Vec<f64>>,
}

#[derive(Debug, Serialize)]
struct V1NurbsCurveGroup {
    segments: Vec<V1NurbsCurve>,
}

#[derive(Debug, Serialize)]
struct V1BrepVertex {
    index: i32,
    point: [f64; 3],
    edge_indices: Vec<i32>,
    tolerance: f64,
}

#[derive(Debug, Serialize)]
struct V1BrepEdge {
    index: i32,
    curve_3d_index: i32,
    domain: [f64; 2],
    vertex_indices: [i32; 2],
    trim_indices: Vec<i32>,
    tolerance: f64,
}

#[derive(Debug, Serialize)]
struct V1BrepTrim {
    index: i32,
    curve_2d_index: i32,
    domain: [f64; 2],
    edge_index: i32,
    vertex_indices: [i32; 2],
    reversed_3d: bool,
    trim_type: i32,
    iso: i32,
    loop_index: i32,
    tolerances: [f64; 2],
    old_points: [[f64; 3]; 2],
    tolerance_2d: f64,
    tolerance_3d: f64,
}

#[derive(Debug, Serialize)]
struct V1BrepLoop {
    index: i32,
    trim_indices: Vec<i32>,
    loop_type: i32,
    face_index: i32,
}

#[derive(Debug, Serialize)]
struct V1BrepFace {
    index: i32,
    loop_indices: Vec<i32>,
    surface_index: i32,
    reversed: bool,
}

#[derive(Debug, Serialize)]
struct V1NurbsBrep {
    wire_version: i32,
    version: i32,
    curves_2d: Vec<V1NurbsCurveGroup>,
    curves_3d: Vec<V1NurbsCurveGroup>,
    surfaces: Vec<V1NurbsSurface>,
    vertices: Vec<V1BrepVertex>,
    edges: Vec<V1BrepEdge>,
    trims: Vec<V1BrepTrim>,
    loops: Vec<V1BrepLoop>,
    faces: Vec<V1BrepFace>,
    bbox: [[f64; 3]; 2],
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "fields")]
enum V1DirectPayload {
    Annotation(V1AnnotationPayload),
    NurbsCurve(V1NurbsCurve),
    NurbsSurface(V1NurbsSurface),
    NurbsBrep(V1NurbsBrep),
}

#[derive(Debug, Serialize)]
struct V1DirectRecord {
    id: String,
    source_offset: u64,
    typecode: u32,
    document_scale: f64,
    payload: V1DirectPayload,
}

#[allow(clippy::needless_pass_by_value)]
fn malformed(error: FramingError) -> CodecError {
    CodecError::Malformed(error.to_string())
}

fn v1_f64(reader: &mut BoundedReader<'_>, label: &str) -> Result<f64, CodecError> {
    let value = reader.f64().map_err(malformed)?;
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| CodecError::malformed(format_args!("V1 {label} is not finite")))
}

fn v1_count(
    reader: &mut BoundedReader<'_>,
    label: &str,
    maximum: usize,
) -> Result<usize, CodecError> {
    let count = reader.i32().map_err(malformed)?;
    usize::try_from(count)
        .ok()
        .filter(|value| *value <= maximum)
        .ok_or_else(|| CodecError::malformed(format_args!("invalid V1 {label} count {count}")))
}

fn v1_string(reader: &mut BoundedReader<'_>, label: &str) -> Result<V1String, CodecError> {
    let count = v1_count(reader, label, 1 << 20)?;
    let bytes = reader.take(count).map_err(malformed)?.to_vec();
    Ok(V1String {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        bytes,
    })
}

fn v1_plane(reader: &mut BoundedReader<'_>) -> Result<V1Plane, CodecError> {
    let mut values = [0.0; 9];
    for value in &mut values {
        *value = v1_f64(reader, "annotation plane coordinate")?;
    }
    Ok(V1Plane {
        origin: [values[0], values[1], values[2]],
        x_axis: [values[3], values[4], values[5]],
        y_axis: [values[6], values[7], values[8]],
    })
}

fn v1_points(
    reader: &mut BoundedReader<'_>,
    count: usize,
    label: &str,
) -> Result<Vec<[f64; 3]>, CodecError> {
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let mut point = [0.0; 3];
        for value in &mut point {
            *value = v1_f64(reader, label)?;
        }
        points.push(point);
    }
    Ok(points)
}

fn v1_annotation(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
) -> Result<V1AnnotationPayload, CodecError> {
    let mut reader =
        BoundedReader::new(data, chunk.body.start, chunk.body.end).map_err(malformed)?;
    let version = reader.i32().map_err(malformed)?;
    match chunk.typecode {
        TCODE_TEXT_BLOCK => {
            if version != 1 && version != 2 {
                return Err(CodecError::malformed(format_args!(
                    "unsupported V1 text-block version {version}"
                )));
            }
            let type_flag = reader.i32().map_err(malformed)?;
            let plane = v1_plane(&mut reader)?;
            let user_text = v1_string(&mut reader, "text-block user text")?;
            let flags = reader.i32().map_err(malformed)?;
            let by_object = reader.i32().map_err(malformed)?;
            let face_name = v1_string(&mut reader, "text-block face name")?;
            let face_weight = reader.i32().map_err(malformed)?;
            let height = v1_f64(&mut reader, "text-block height")?;
            let version_one_extra = if version == 1 {
                Some([
                    v1_f64(&mut reader, "text-block version-1 extra")?,
                    v1_f64(&mut reader, "text-block version-1 extra")?,
                ])
            } else {
                None
            };
            reader.skip_remaining().map_err(malformed)?;
            Ok(V1AnnotationPayload::TextBlock(V1TextBlock {
                version,
                type_flag,
                plane,
                user_text,
                flags,
                by_object,
                face_name,
                face_weight,
                height,
                version_one_extra,
            }))
        }
        TCODE_ANNOTATION_LEADER => {
            if version != 1 {
                return Err(CodecError::malformed(format_args!(
                    "unsupported V1 leader version {version}"
                )));
            }
            let type_flag = reader.i32().map_err(malformed)?;
            let plane = v1_plane(&mut reader)?;
            let flags = reader.i32().map_err(malformed)?;
            let by_object = reader.i32().map_err(malformed)?;
            let count = v1_count(&mut reader, "leader point", 1 << 16)?;
            let points = v1_points(&mut reader, count, "leader point")?;
            reader.skip_remaining().map_err(malformed)?;
            Ok(V1AnnotationPayload::Leader(V1Leader {
                version,
                type_flag,
                plane,
                flags,
                by_object,
                points,
            }))
        }
        TCODE_LINEAR_DIMENSION => {
            if version != 1 {
                return Err(CodecError::malformed(format_args!(
                    "unsupported V1 linear-dimension version {version}"
                )));
            }
            let annotation_type = reader.i32().map_err(malformed)?;
            let plane = v1_plane(&mut reader)?;
            let points = v1_points(&mut reader, 11, "linear-dimension point")?;
            let user_text = v1_string(&mut reader, "linear-dimension user text")?;
            let default_text = v1_string(&mut reader, "linear-dimension default text")?;
            let user_positioned_text = reader.i32().map_err(malformed)?;
            let flags = reader.i32().map_err(malformed)?;
            let by_object = reader.i32().map_err(malformed)?;
            reader.skip_remaining().map_err(malformed)?;
            Ok(V1AnnotationPayload::LinearDimension(V1LinearDimension {
                version,
                annotation_type,
                plane,
                points,
                user_text,
                default_text,
                user_positioned_text,
                flags,
                by_object,
            }))
        }
        TCODE_ANGULAR_DIMENSION => {
            if version != 1 {
                return Err(CodecError::malformed(format_args!(
                    "unsupported V1 angular-dimension version {version}"
                )));
            }
            let annotation_type = reader.i32().map_err(malformed)?;
            let plane = v1_plane(&mut reader)?;
            let angle = v1_f64(&mut reader, "angular-dimension angle")?;
            let radius = v1_f64(&mut reader, "angular-dimension radius")?;
            let mut extension_distances = [0.0; 4];
            for value in &mut extension_distances {
                *value = v1_f64(&mut reader, "angular-dimension extension")?;
            }
            let points = v1_points(&mut reader, 5, "angular-dimension point")?;
            let user_text = v1_string(&mut reader, "angular-dimension user text")?;
            let default_text = v1_string(&mut reader, "angular-dimension default text")?;
            let user_positioned_text = reader.i32().map_err(malformed)?;
            let flags = reader.i32().map_err(malformed)?;
            let by_object = reader.i32().map_err(malformed)?;
            reader.skip_remaining().map_err(malformed)?;
            Ok(V1AnnotationPayload::AngularDimension(V1AngularDimension {
                version,
                annotation_type,
                plane,
                angle,
                radius,
                extension_distances,
                points,
                user_text,
                default_text,
                user_positioned_text,
                flags,
                by_object,
            }))
        }
        TCODE_RADIAL_DIMENSION => {
            if version != 1 {
                return Err(CodecError::malformed(format_args!(
                    "unsupported V1 radial-dimension version {version}"
                )));
            }
            let annotation_type = reader.i32().map_err(malformed)?;
            let plane = v1_plane(&mut reader)?;
            let points = v1_points(&mut reader, 5, "radial-dimension point")?;
            let user_text = v1_string(&mut reader, "radial-dimension user text")?;
            let default_text = v1_string(&mut reader, "radial-dimension default text")?;
            let user_positioned_text = reader.i32().map_err(malformed)?;
            let flags = reader.i32().map_err(malformed)?;
            let by_object = reader.i32().map_err(malformed)?;
            reader.skip_remaining().map_err(malformed)?;
            Ok(V1AnnotationPayload::RadialDimension(V1RadialDimension {
                version,
                annotation_type,
                plane,
                points,
                user_text,
                default_text,
                user_positioned_text,
                flags,
                by_object,
            }))
        }
        _ => Err(CodecError::malformed(format_args!(
            "typecode {:#010x} is not a V1 annotation",
            chunk.typecode
        ))),
    }
}

fn retain_v1_record(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    retained_bytes: &mut usize,
) -> UnknownRecord {
    let range = chunk.range();
    let bytes = &data[range.clone()];
    let retain = bytes.len() <= crate::decode::RETAINED_RECORD_CAP
        && retained_bytes
            .checked_add(bytes.len())
            .is_some_and(|end| end <= crate::decode::RETAINED_DOCUMENT_CAP);
    if retain {
        *retained_bytes = retained_bytes
            .checked_add(bytes.len())
            .expect("V1 retention cap checked");
    }
    let id = UnknownId(format!(
        "rhino:legacy:record#{:08x}-{:016x}",
        chunk.typecode, chunk.header_start
    ));
    let offset = u64::try_from(range.start).expect("V1 record offset fits u64");
    if retain {
        UnknownRecord::retained(id, offset, bytes.to_vec(), Vec::new())
    } else {
        UnknownRecord::unavailable(
            id,
            offset,
            u64::try_from(bytes.len()).expect("V1 record length fits u64"),
            sha256_hex(bytes),
            Vec::new(),
        )
    }
}

fn child_with_type(
    data: &[u8],
    range: std::ops::Range<usize>,
    typecode: u32,
) -> Result<Option<crate::chunks::Chunk>, CodecError> {
    let mut offset = range.start;
    // A V1 legacy wrapper and its final stuff child share the wrapper CRC16.
    let end = range.end.saturating_add(2).min(data.len());
    while offset < end {
        let chunk = chunk_at(data, offset, end, ArchiveVersion::V1, false).map_err(malformed)?;
        if chunk.typecode == typecode {
            return Ok(Some(chunk));
        }
        offset = chunk.next_offset;
    }
    Ok(None)
}

fn legacy_spline(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<NurbsCurve, CodecError> {
    let mut reader = BoundedReader::new(data, range.start, range.end).map_err(malformed)?;
    let dimension = reader.u8().map_err(malformed)?;
    if !matches!(dimension, 2 | 3) {
        return Err(CodecError::Malformed(
            "invalid V1 spline dimension".to_string(),
        ));
    }
    let rational = reader.u8().map_err(malformed)?;
    if rational > 2 {
        return Err(CodecError::Malformed(
            "invalid V1 spline rational flag".to_string(),
        ));
    }
    let order = usize::from(reader.u8().map_err(malformed)?);
    let cv_count = usize::from(reader.u16().map_err(malformed)?);
    if order < 2 || cv_count < order {
        return Err(CodecError::Malformed("invalid V1 spline shape".to_string()));
    }
    let closed = reader.u8().map_err(malformed)?;
    if closed > 2 {
        return Err(CodecError::Malformed(
            "invalid V1 spline closure".to_string(),
        ));
    }
    let _form = reader.u8().map_err(malformed)?;
    reader
        .skip(usize::from(dimension) * 16)
        .map_err(malformed)?;

    let clamped = if order > 2 {
        reader.u8().map_err(malformed)?
    } else {
        0
    };
    if clamped > 3 {
        return Err(CodecError::Malformed(
            "invalid V1 spline clamp flag".to_string(),
        ));
    }
    let knot_count = order + cv_count - 2;
    let mut stored_knots = Vec::with_capacity(knot_count);
    let first = reader.f64().map_err(malformed)?;
    stored_knots.push(first);
    if clamped & 1 != 0 {
        while stored_knots.len() <= order - 2 {
            stored_knots.push(first);
        }
    }
    while stored_knots.len() < cv_count {
        stored_knots.push(reader.f64().map_err(malformed)?);
    }
    let last = *stored_knots.last().expect("first knot exists");
    if clamped & 2 != 0 {
        stored_knots.resize(knot_count, last);
    } else {
        while stored_knots.len() < knot_count {
            stored_knots.push(reader.f64().map_err(malformed)?);
        }
    }
    if order == 2 && cv_count == 2 && stored_knots[0] > stored_knots[1] {
        stored_knots[0] = -stored_knots[0];
        stored_knots[1] = -stored_knots[1];
    }
    let knots = crate::surfaces::reconstruct_knots(&stored_knots, order, cv_count)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    let mut control_points = Vec::with_capacity(cv_count);
    let mut weights = (rational != 0).then(|| Vec::with_capacity(cv_count));
    for _ in 0..cv_count {
        let x = reader.f64().map_err(malformed)?;
        let y = reader.f64().map_err(malformed)?;
        let z = if dimension == 3 {
            reader.f64().map_err(malformed)?
        } else {
            0.0
        };
        if let Some(weights) = &mut weights {
            let weight = reader.f64().map_err(malformed)?;
            if !weight.is_finite() || weight == 0.0 {
                return Err(CodecError::Malformed(
                    "invalid V1 spline weight".to_string(),
                ));
            }
            let divisor = if rational == 2 { weight } else { 1.0 };
            control_points.push(Point3::new(
                x * scale / divisor,
                y * scale / divisor,
                z * scale / divisor,
            ));
            weights.push(weight);
        } else {
            control_points.push(Point3::new(x * scale, y * scale, z * scale));
        }
    }
    NurbsCurve::new(
        u32::try_from(order - 1)
            .map_err(|_| CodecError::Malformed("V1 spline degree overflow".to_string()))?,
        knots,
        control_points,
        weights,
        closed == 2,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn legacy_curve_segments(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<Vec<NurbsCurve>, CodecError> {
    let stuff = child_with_type(data, range, TCODE_LEGACY_CRVSTUFF)?
        .ok_or_else(|| CodecError::Malformed("V1 curve has no curve-stuff chunk".to_string()))?;
    let mut reader =
        BoundedReader::new(data, stuff.body.start, stuff.body.end).map_err(malformed)?;
    let dimension = reader.u8().map_err(malformed)?;
    if !matches!(dimension, 2 | 3) {
        return Err(CodecError::Malformed(
            "invalid V1 curve dimension".to_string(),
        ));
    }
    let closure = reader.u8().map_err(malformed)?;
    if !matches!(closure, 0 | 1 | 2 | 255) {
        return Err(CodecError::Malformed(
            "invalid V1 curve closure".to_string(),
        ));
    }
    let count = usize::from(reader.u16().map_err(malformed)?);
    if count == 0 {
        return Err(CodecError::Malformed("empty V1 curve".to_string()));
    }
    reader
        .skip(usize::from(dimension) * 16)
        .map_err(malformed)?;
    let mut segments = Vec::with_capacity(count);
    for _ in 0..count {
        let spline = chunk_at(
            data,
            reader.position(),
            stuff.body.end,
            ArchiveVersion::V1,
            false,
        )
        .map_err(malformed)?;
        if spline.typecode != TCODE_LEGACY_SPL {
            return Err(CodecError::Malformed(
                "V1 curve segment is not a spline".to_string(),
            ));
        }
        let spline_stuff = child_with_type(data, spline.body.clone(), TCODE_LEGACY_SPLSTUFF)?
            .ok_or_else(|| {
                CodecError::Malformed("V1 spline has no spline-stuff chunk".to_string())
            })?;
        segments.push(legacy_spline(data, spline_stuff.body, scale)?);
        reader
            .skip(spline.next_offset - reader.position())
            .map_err(malformed)?;
    }
    Ok(segments)
}

fn legacy_curve(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<NurbsCurve, CodecError> {
    let offset = range.start;
    let segments = legacy_curve_segments(data, range, scale)?;
    crate::curves::join_nurbs_segments(segments, offset)
        .map(|joined| joined.curve)
        .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn nested_chunk(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    typecode: u32,
) -> Result<crate::chunks::Chunk, CodecError> {
    let chunk = chunk_at(
        data,
        reader.position(),
        reader.end(),
        ArchiveVersion::V1,
        false,
    )
    .map_err(malformed)?;
    if chunk.short || chunk.typecode != typecode {
        return Err(CodecError::malformed(format_args!(
            "expected V1 nested chunk {typecode:#010x} at offset {}",
            reader.position()
        )));
    }
    reader
        .skip(chunk.next_offset - reader.position())
        .map_err(malformed)?;
    Ok(chunk)
}

fn nested_stuff(
    data: &[u8],
    range: std::ops::Range<usize>,
    wrapper_type: u32,
    stuff_type: u32,
) -> Result<crate::chunks::Chunk, CodecError> {
    child_with_type(data, range, stuff_type)?.ok_or_else(|| {
        CodecError::malformed(format_args!(
            "V1 wrapper {wrapper_type:#010x} has no stuff chunk"
        ))
    })
}

fn v1_i32_array(reader: &mut BoundedReader<'_>, label: &str) -> Result<Vec<i32>, CodecError> {
    let count = v1_count(reader, label, 1 << 20)?;
    (0..count)
        .map(|_| reader.i32().map_err(malformed))
        .collect::<Result<Vec<_>, _>>()
}

fn v1_point3(reader: &mut BoundedReader<'_>, label: &str) -> Result<[f64; 3], CodecError> {
    Ok([
        v1_f64(reader, label)?,
        v1_f64(reader, label)?,
        v1_f64(reader, label)?,
    ])
}

fn v1_interval(reader: &mut BoundedReader<'_>, label: &str) -> Result<[f64; 2], CodecError> {
    Ok([v1_f64(reader, label)?, v1_f64(reader, label)?])
}

fn v1_nurbs_curve_data(
    data: &[u8],
    range: std::ops::Range<usize>,
) -> Result<V1NurbsCurve, CodecError> {
    let mut reader = BoundedReader::new(data, range.start, range.end).map_err(malformed)?;
    let wire_version = reader.i32().map_err(malformed)?;
    let version = wire_version & !0x100;
    if version != 100 && version != 101 {
        return Err(CodecError::malformed(format_args!(
            "unsupported RhinoIO V1 NURBS curve version {wire_version}"
        )));
    }
    let dimension = reader.i32().map_err(malformed)?;
    if !(1..=1 << 10).contains(&dimension) {
        return Err(CodecError::Malformed(
            "invalid RhinoIO V1 NURBS curve dimension".to_string(),
        ));
    }
    let rational = match reader.i32().map_err(malformed)? {
        0 => false,
        1 => true,
        value => {
            return Err(CodecError::malformed(format_args!(
                "invalid RhinoIO V1 NURBS curve rational flag {value}"
            )))
        }
    };
    let order = reader.i32().map_err(malformed)?;
    if order < 2 {
        return Err(CodecError::Malformed(
            "invalid RhinoIO V1 NURBS curve order".to_string(),
        ));
    }
    let control_count = reader.i32().map_err(malformed)?;
    if control_count < order {
        return Err(CodecError::Malformed(
            "invalid RhinoIO V1 NURBS curve control count".to_string(),
        ));
    }
    let flag = reader.i32().map_err(malformed)?;
    if flag != 0 {
        return Err(CodecError::malformed(format_args!(
            "invalid RhinoIO V1 NURBS curve flag {flag}"
        )));
    }
    let knot_count = usize::try_from(order + control_count - 2)
        .map_err(|_| CodecError::Malformed("V1 NURBS curve knot count overflow".to_string()))?;
    if knot_count > 1 << 20 {
        return Err(CodecError::Malformed(
            "V1 NURBS curve knot count exceeds limit".to_string(),
        ));
    }
    let knots = (0..knot_count)
        .map(|_| v1_f64(&mut reader, "NURBS curve knot"))
        .collect::<Result<Vec<_>, _>>()?;
    let control_count = usize::try_from(control_count)
        .map_err(|_| CodecError::Malformed("V1 NURBS curve control count overflow".to_string()))?;
    let value_width = usize::try_from(dimension)
        .map_err(|_| CodecError::Malformed("V1 NURBS curve dimension overflow".to_string()))?
        + usize::from(rational);
    let value_count = control_count
        .checked_mul(value_width)
        .filter(|count| *count <= 1 << 20)
        .ok_or_else(|| CodecError::Malformed("V1 NURBS curve values exceed limit".to_string()))?;
    let mut control_values = Vec::with_capacity(control_count);
    for _ in 0..control_count {
        let mut values = Vec::with_capacity(value_width);
        for _ in 0..value_width {
            values.push(v1_f64(&mut reader, "NURBS curve control value")?);
        }
        control_values.push(values);
    }
    debug_assert_eq!(
        value_count,
        control_values.iter().map(Vec::len).sum::<usize>()
    );
    reader.skip_remaining().map_err(malformed)?;
    Ok(V1NurbsCurve {
        wire_version,
        version,
        dimension,
        rational,
        order,
        knots,
        control_values,
    })
}

fn v1_nurbs_surface_data(
    data: &[u8],
    range: std::ops::Range<usize>,
) -> Result<V1NurbsSurface, CodecError> {
    let mut reader = BoundedReader::new(data, range.start, range.end).map_err(malformed)?;
    let wire_version = reader.i32().map_err(malformed)?;
    let version = wire_version & !0x100;
    if version != 100 && version != 101 {
        return Err(CodecError::malformed(format_args!(
            "unsupported RhinoIO V1 NURBS surface version {wire_version}"
        )));
    }
    let dimension = reader.i32().map_err(malformed)?;
    if !(1..=1 << 10).contains(&dimension) {
        return Err(CodecError::Malformed(
            "invalid RhinoIO V1 NURBS surface dimension".to_string(),
        ));
    }
    let rational = match reader.i32().map_err(malformed)? {
        0 => false,
        1 => true,
        value => {
            return Err(CodecError::malformed(format_args!(
                "invalid RhinoIO V1 NURBS surface rational flag {value}"
            )));
        }
    };
    let orders = [
        reader.i32().map_err(malformed)?,
        reader.i32().map_err(malformed)?,
    ];
    if orders.iter().any(|order| *order < 2) {
        return Err(CodecError::Malformed(
            "invalid RhinoIO V1 NURBS surface order".to_string(),
        ));
    }
    let control_counts = [
        reader.i32().map_err(malformed)?,
        reader.i32().map_err(malformed)?,
    ];
    if control_counts
        .iter()
        .zip(orders)
        .any(|(count, order)| *count < order)
    {
        return Err(CodecError::Malformed(
            "invalid RhinoIO V1 NURBS surface control count".to_string(),
        ));
    }
    let flag = reader.i32().map_err(malformed)?;
    if flag != 0 {
        return Err(CodecError::malformed(format_args!(
            "invalid RhinoIO V1 NURBS surface flag {flag}"
        )));
    }
    let knot_counts = [
        usize::try_from(orders[0] + control_counts[0] - 2)
            .map_err(|_| CodecError::Malformed("V1 U knot count overflow".to_string()))?,
        usize::try_from(orders[1] + control_counts[1] - 2)
            .map_err(|_| CodecError::Malformed("V1 V knot count overflow".to_string()))?,
    ];
    if knot_counts.iter().any(|count| *count > 1 << 20) {
        return Err(CodecError::Malformed(
            "V1 NURBS surface knot count exceeds limit".to_string(),
        ));
    }
    let u_knots = (0..knot_counts[0])
        .map(|_| v1_f64(&mut reader, "NURBS surface U knot"))
        .collect::<Result<Vec<_>, _>>()?;
    let v_knots = (0..knot_counts[1])
        .map(|_| v1_f64(&mut reader, "NURBS surface V knot"))
        .collect::<Result<Vec<_>, _>>()?;
    let u_count = usize::try_from(control_counts[0])
        .map_err(|_| CodecError::Malformed("V1 U control count overflow".to_string()))?;
    let v_count = usize::try_from(control_counts[1])
        .map_err(|_| CodecError::Malformed("V1 V control count overflow".to_string()))?;
    let value_width = usize::try_from(dimension)
        .map_err(|_| CodecError::Malformed("V1 NURBS surface dimension overflow".to_string()))?
        + usize::from(rational);
    let pole_count = u_count
        .checked_mul(v_count)
        .filter(|count| {
            count
                .checked_mul(value_width)
                .is_some_and(|size| size <= 1 << 20)
        })
        .ok_or_else(|| CodecError::Malformed("V1 NURBS surface values exceed limit".to_string()))?;
    let mut control_values = Vec::with_capacity(pole_count);
    for _ in 0..pole_count {
        let mut values = Vec::with_capacity(value_width);
        for _ in 0..value_width {
            values.push(v1_f64(&mut reader, "NURBS surface control value")?);
        }
        control_values.push(values);
    }
    reader.skip_remaining().map_err(malformed)?;
    Ok(V1NurbsSurface {
        wire_version,
        version,
        dimension,
        rational,
        orders,
        control_counts,
        u_knots,
        v_knots,
        control_values,
    })
}

fn v1_nurbs_curve_object(
    data: &[u8],
    range: std::ops::Range<usize>,
) -> Result<V1NurbsCurve, CodecError> {
    let mut reader = BoundedReader::new(data, range.start, range.end).map_err(malformed)?;
    let data_chunk = nested_chunk(data, &mut reader, TCODE_RHINOIO_OBJECT_DATA)?;
    let curve = v1_nurbs_curve_data(data, data_chunk.body)?;
    reader.skip_remaining().map_err(malformed)?;
    Ok(curve)
}

fn v1_nurbs_surface_object(
    data: &[u8],
    range: std::ops::Range<usize>,
) -> Result<V1NurbsSurface, CodecError> {
    let mut reader = BoundedReader::new(data, range.start, range.end).map_err(malformed)?;
    let data_chunk = nested_chunk(data, &mut reader, TCODE_RHINOIO_OBJECT_DATA)?;
    let surface = v1_nurbs_surface_data(data, data_chunk.body)?;
    reader.skip_remaining().map_err(malformed)?;
    Ok(surface)
}

fn v1_nurbs_curve_group(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
) -> Result<V1NurbsCurveGroup, CodecError> {
    let segment_count = v1_count(reader, "Brep curve segment", 1 << 16)?;
    if segment_count == 0 {
        return Err(CodecError::Malformed(
            "V1 RhinoIO Brep curve has no segments".to_string(),
        ));
    }
    let mut segments = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let object = nested_chunk(data, reader, TCODE_RHINOIO_OBJECT_NURBS_CURVE)?;
        segments.push(v1_nurbs_curve_object(data, object.body)?);
    }
    Ok(V1NurbsCurveGroup { segments })
}

fn v1_nurbs_brep(data: &[u8], chunk: &crate::chunks::Chunk) -> Result<V1NurbsBrep, CodecError> {
    let mut outer =
        BoundedReader::new(data, chunk.body.start, chunk.body.end).map_err(malformed)?;
    let data_chunk = nested_chunk(data, &mut outer, TCODE_RHINOIO_OBJECT_DATA)?;
    let mut reader =
        BoundedReader::new(data, data_chunk.body.start, data_chunk.body.end).map_err(malformed)?;
    let wire_version = reader.i32().map_err(malformed)?;
    if wire_version != 100 && wire_version != 101 {
        return Err(CodecError::malformed(format_args!(
            "unsupported RhinoIO V1 Brep version {wire_version}"
        )));
    }

    let curve_count = v1_count(&mut reader, "Brep 2D curve", 1 << 16)?;
    if curve_count == 0 {
        return Err(CodecError::Malformed(
            "V1 RhinoIO Brep has no 2D curves".to_string(),
        ));
    }
    let mut curves_2d = Vec::with_capacity(curve_count);
    for _ in 0..curve_count {
        curves_2d.push(v1_nurbs_curve_group(data, &mut reader)?);
    }

    let curve_count = v1_count(&mut reader, "Brep 3D curve", 1 << 16)?;
    if curve_count == 0 {
        return Err(CodecError::Malformed(
            "V1 RhinoIO Brep has no 3D curves".to_string(),
        ));
    }
    let mut curves_3d = Vec::with_capacity(curve_count);
    for _ in 0..curve_count {
        curves_3d.push(v1_nurbs_curve_group(data, &mut reader)?);
    }

    let surface_count = v1_count(&mut reader, "Brep surface", 1 << 16)?;
    if surface_count == 0 {
        return Err(CodecError::Malformed(
            "V1 RhinoIO Brep has no surfaces".to_string(),
        ));
    }
    let mut surfaces = Vec::with_capacity(surface_count);
    for _ in 0..surface_count {
        let object = nested_chunk(data, &mut reader, TCODE_RHINOIO_OBJECT_NURBS_SURFACE)?;
        surfaces.push(v1_nurbs_surface_object(data, object.body)?);
    }

    let vertex_count = v1_count(&mut reader, "Brep vertex", 1 << 20)?;
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push(V1BrepVertex {
            index: reader.i32().map_err(malformed)?,
            point: v1_point3(&mut reader, "Brep vertex point")?,
            edge_indices: v1_i32_array(&mut reader, "Brep vertex edge")?,
            tolerance: v1_f64(&mut reader, "Brep vertex tolerance")?,
        });
    }

    let edge_count = v1_count(&mut reader, "Brep edge", 1 << 20)?;
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        edges.push(V1BrepEdge {
            index: reader.i32().map_err(malformed)?,
            curve_3d_index: reader.i32().map_err(malformed)?,
            domain: v1_interval(&mut reader, "Brep edge domain")?,
            vertex_indices: [
                reader.i32().map_err(malformed)?,
                reader.i32().map_err(malformed)?,
            ],
            trim_indices: v1_i32_array(&mut reader, "Brep edge trim")?,
            tolerance: v1_f64(&mut reader, "Brep edge tolerance")?,
        });
    }

    let trim_count = v1_count(&mut reader, "Brep trim", 1 << 20)?;
    let mut trims = Vec::with_capacity(trim_count);
    for _ in 0..trim_count {
        trims.push(V1BrepTrim {
            index: reader.i32().map_err(malformed)?,
            curve_2d_index: reader.i32().map_err(malformed)?,
            domain: v1_interval(&mut reader, "Brep trim domain")?,
            edge_index: reader.i32().map_err(malformed)?,
            vertex_indices: [
                reader.i32().map_err(malformed)?,
                reader.i32().map_err(malformed)?,
            ],
            reversed_3d: reader.i32().map_err(malformed)? != 0,
            trim_type: reader.i32().map_err(malformed)?,
            iso: reader.i32().map_err(malformed)?,
            loop_index: reader.i32().map_err(malformed)?,
            tolerances: [
                v1_f64(&mut reader, "Brep trim tolerance")?,
                v1_f64(&mut reader, "Brep trim tolerance")?,
            ],
            old_points: [
                v1_point3(&mut reader, "Brep trim old point")?,
                v1_point3(&mut reader, "Brep trim old point")?,
            ],
            tolerance_2d: v1_f64(&mut reader, "Brep trim 2D tolerance")?,
            tolerance_3d: v1_f64(&mut reader, "Brep trim 3D tolerance")?,
        });
    }

    let loop_count = v1_count(&mut reader, "Brep loop", 1 << 20)?;
    let mut loops = Vec::with_capacity(loop_count);
    for _ in 0..loop_count {
        loops.push(V1BrepLoop {
            index: reader.i32().map_err(malformed)?,
            trim_indices: v1_i32_array(&mut reader, "Brep loop trim")?,
            loop_type: reader.i32().map_err(malformed)?,
            face_index: reader.i32().map_err(malformed)?,
        });
    }

    let face_count = v1_count(&mut reader, "Brep face", 1 << 20)?;
    let mut faces = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        faces.push(V1BrepFace {
            index: reader.i32().map_err(malformed)?,
            loop_indices: v1_i32_array(&mut reader, "Brep face loop")?,
            surface_index: reader.i32().map_err(malformed)?,
            reversed: reader.i32().map_err(malformed)? != 0,
        });
    }
    let bbox = [
        v1_point3(&mut reader, "Brep bounding box")?,
        v1_point3(&mut reader, "Brep bounding box")?,
    ];
    reader.skip_remaining().map_err(malformed)?;
    outer.skip_remaining().map_err(malformed)?;
    Ok(V1NurbsBrep {
        wire_version,
        version: wire_version,
        curves_2d,
        curves_3d,
        surfaces,
        vertices,
        edges,
        trims,
        loops,
        faces,
        bbox,
    })
}

fn v1_direct_record(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    document_scale: f64,
) -> Result<V1DirectRecord, CodecError> {
    let payload = match chunk.typecode {
        TCODE_TEXT_BLOCK
        | TCODE_ANNOTATION_LEADER
        | TCODE_LINEAR_DIMENSION
        | TCODE_ANGULAR_DIMENSION
        | TCODE_RADIAL_DIMENSION => V1DirectPayload::Annotation(v1_annotation(data, chunk)?),
        TCODE_RHINOIO_OBJECT_NURBS_CURVE => {
            V1DirectPayload::NurbsCurve(v1_nurbs_curve_object(data, chunk.body.clone())?)
        }
        TCODE_RHINOIO_OBJECT_NURBS_SURFACE => {
            V1DirectPayload::NurbsSurface(v1_nurbs_surface_object(data, chunk.body.clone())?)
        }
        TCODE_RHINOIO_OBJECT_BREP => V1DirectPayload::NurbsBrep(v1_nurbs_brep(data, chunk)?),
        _ => {
            return Err(CodecError::malformed(format_args!(
                "unsupported V1 direct typecode {:#010x}",
                chunk.typecode
            )))
        }
    };
    Ok(V1DirectRecord {
        id: format!(
            "rhino:legacy:v1-record#{:08x}-{:016x}",
            chunk.typecode, chunk.header_start
        ),
        source_offset: u64::try_from(chunk.header_start)
            .map_err(|_| CodecError::Malformed("V1 direct record offset overflow".to_string()))?,
        typecode: chunk.typecode,
        document_scale,
        payload,
    })
}

fn legacy_surface(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<NurbsSurface, CodecError> {
    let stuff = nested_stuff(data, range, TCODE_LEGACY_SRF, TCODE_LEGACY_SRFSTUFF)?;
    let mut reader =
        BoundedReader::new(data, stuff.body.start, stuff.body.end).map_err(malformed)?;
    let dimension = usize::from(reader.u8().map_err(malformed)?);
    if !matches!(dimension, 2 | 3) {
        return Err(CodecError::Malformed(
            "invalid V1 surface dimension".to_string(),
        ));
    }
    let _form = reader.u8().map_err(malformed)?;
    let orders = [
        usize::from(reader.u8().map_err(malformed)?) + 1,
        usize::from(reader.u8().map_err(malformed)?) + 1,
    ];
    if orders.iter().any(|order| *order < 2) {
        return Err(CodecError::Malformed(
            "invalid V1 surface order".to_string(),
        ));
    }
    let counts = [
        orders[0] - 1 + usize::from(reader.u16().map_err(malformed)?),
        orders[1] - 1 + usize::from(reader.u16().map_err(malformed)?),
    ];
    if counts[0] < orders[0] || counts[1] < orders[1] {
        return Err(CodecError::Malformed(
            "invalid V1 surface pole count".to_string(),
        ));
    }
    let rational_modes = [
        reader.u8().map_err(malformed)?,
        reader.u8().map_err(malformed)?,
    ];
    if rational_modes.iter().any(|mode| *mode > 2) {
        return Err(CodecError::Malformed(
            "invalid V1 surface rational flag".to_string(),
        ));
    }
    let rational_mode = rational_modes
        .into_iter()
        .rfind(|mode| *mode != 0)
        .unwrap_or(0);
    let closed = [
        reader.u8().map_err(malformed)?,
        reader.u8().map_err(malformed)?,
    ];
    if closed.iter().any(|value| *value > 2) {
        return Err(CodecError::Malformed(
            "invalid V1 surface closure flag".to_string(),
        ));
    }
    let singular = [
        reader.u8().map_err(malformed)?,
        reader.u8().map_err(malformed)?,
    ];
    if singular.iter().any(|value| *value > 3) {
        return Err(CodecError::Malformed(
            "invalid V1 surface singular flag".to_string(),
        ));
    }
    reader.skip(dimension * 16).map_err(malformed)?;
    let stored_u = (0..orders[0] + counts[0] - 2)
        .map(|_| reader.f64().map_err(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    let stored_v = (0..orders[1] + counts[1] - 2)
        .map(|_| reader.f64().map_err(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    let u_knots = crate::surfaces::reconstruct_knots(&stored_u, orders[0], counts[0])
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    let v_knots = crate::surfaces::reconstruct_knots(&stored_v, orders[1], counts[1])
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    let pole_count = counts[0]
        .checked_mul(counts[1])
        .ok_or_else(|| CodecError::Malformed("V1 surface pole count overflow".to_string()))?;
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = (rational_mode != 0).then(|| Vec::with_capacity(pole_count));
    for _ in 0..pole_count {
        let coordinates = (0..dimension)
            .map(|_| reader.f64().map_err(malformed))
            .collect::<Result<Vec<_>, _>>()?;
        let weight = if rational_mode == 0 {
            1.0
        } else {
            reader.f64().map_err(malformed)?
        };
        if !weight.is_finite() || weight == 0.0 {
            return Err(CodecError::Malformed(
                "invalid V1 surface weight".to_string(),
            ));
        }
        let divisor = if rational_mode == 2 { weight } else { 1.0 };
        let coordinate = |index: usize| coordinates.get(index).copied().unwrap_or(0.0) / divisor;
        let point = Point3::new(
            coordinate(0) * scale,
            coordinate(1) * scale,
            coordinate(2) * scale,
        );
        if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
            return Err(CodecError::Malformed("invalid V1 surface pole".to_string()));
        }
        control_points.push(point);
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    NurbsSurface::new(
        u32::try_from(orders[0] - 1)
            .map_err(|_| CodecError::Malformed("V1 surface degree overflow".to_string()))?,
        u32::try_from(orders[1] - 1)
            .map_err(|_| CodecError::Malformed("V1 surface degree overflow".to_string()))?,
        u_knots,
        v_knots,
        u32::try_from(counts[0])
            .map_err(|_| CodecError::Malformed("V1 surface pole count overflow".to_string()))?,
        u32::try_from(counts[1])
            .map_err(|_| CodecError::Malformed("V1 surface pole count overflow".to_string()))?,
        control_points,
        weights,
        false,
        closed[0] == 2,
        closed[1] == 2,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

#[derive(Clone)]
struct LegacyTrim {
    has_mate: bool,
    seam: bool,
    reversed: bool,
    tolerance_3d: f64,
    tolerance_2d: f64,
    pcurve: NurbsCurve,
    curve: Option<NurbsCurve>,
}

struct LegacyLoop {
    role: LoopBoundaryRole,
    trims: Vec<LegacyTrim>,
}

struct LegacyFace {
    reversed: bool,
    seam_glue: Vec<usize>,
    surface: NurbsSurface,
    loops: Vec<LegacyLoop>,
}

struct LegacyBrep {
    shell_glue: Vec<usize>,
    faces: Vec<LegacyFace>,
}

fn curve_domain(curve: &NurbsCurve) -> Result<[f64; 2], CodecError> {
    let degree = usize::try_from(curve.degree())
        .map_err(|_| CodecError::Malformed("V1 curve degree overflow".to_string()))?;
    let end = curve
        .knots()
        .len()
        .checked_sub(degree + 1)
        .ok_or_else(|| CodecError::Malformed("invalid V1 curve knot vector".to_string()))?;
    Ok([curve.knots()[degree], curve.knots()[end]])
}

fn find_root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

fn union(parents: &mut [usize], left: usize, right: usize) {
    let left = find_root(parents, left);
    let right = find_root(parents, right);
    if left != right {
        parents[right] = left;
    }
}

fn append_legacy_brep(ir: &mut CadIr, brep: LegacyBrep, suffix: &str) -> Result<(), CodecError> {
    let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{suffix}").into();
    let region_id: cadmpeg_ir::ids::RegionId = format!("rhino:object:region#{suffix}").into();
    let shell_id: cadmpeg_ir::ids::ShellId = format!("rhino:object:shell#{suffix}").into();
    let mut trim_paths = Vec::new();
    let mut face_trim_indices = alloc_filled(
        brep.faces.len(),
        Vec::<usize>::new(),
        "Rhino legacy Brep face trim indices",
    )?;
    for (face_index, face) in brep.faces.iter().enumerate() {
        for (loop_index, loop_record) in face.loops.iter().enumerate() {
            for trim_index in 0..loop_record.trims.len() {
                let global = trim_paths.len();
                trim_paths.push((face_index, loop_index, trim_index));
                face_trim_indices[face_index].push(global);
            }
        }
    }
    if trim_paths.is_empty() {
        return Err(CodecError::Malformed("V1 Brep has no trims".to_string()));
    }
    let mut parents = (0..trim_paths.len()).collect::<Vec<_>>();
    let has_edge = |index: usize| {
        let (face, loop_index, trim) = trim_paths[index];
        brep.faces[face].loops[loop_index].trims[trim]
            .curve
            .is_some()
    };
    let glue_edges = |parents: &mut [usize], left: usize, right: usize| {
        if has_edge(left) != has_edge(right) {
            union(parents, left, right);
        }
    };
    for (face_index, face) in brep.faces.iter().enumerate() {
        let seams = face_trim_indices[face_index]
            .iter()
            .copied()
            .filter(|index| {
                let (_, loop_index, trim_index) = trim_paths[*index];
                face.loops[loop_index].trims[trim_index].seam
            })
            .collect::<Vec<_>>();
        if face.seam_glue.len() == seams.len() {
            for (index, mate) in face.seam_glue.iter().copied().enumerate() {
                if mate < seams.len() {
                    glue_edges(&mut parents, seams[index], seams[mate]);
                }
            }
        }
    }
    let mates = trim_paths
        .iter()
        .enumerate()
        .filter_map(|(index, (face, loop_index, trim))| {
            let record = &brep.faces[*face].loops[*loop_index].trims[*trim];
            (record.has_mate && !record.seam).then_some(index)
        })
        .collect::<Vec<_>>();
    if brep.shell_glue.len() == mates.len() {
        for (index, mate) in brep.shell_glue.iter().copied().enumerate() {
            if mate < mates.len() {
                glue_edges(&mut parents, mates[index], mates[mate]);
            }
        }
    }
    let roots = (0..trim_paths.len())
        .map(|index| find_root(&mut parents, index))
        .collect::<Vec<_>>();
    let group_roots = roots.iter().copied().collect::<BTreeSet<_>>();
    let mut group_curve = BTreeMap::<usize, NurbsCurve>::new();
    let mut group_tolerance = BTreeMap::<usize, f64>::new();
    for (index, (face, loop_index, trim)) in trim_paths.iter().copied().enumerate() {
        let record = &brep.faces[face].loops[loop_index].trims[trim];
        let root = roots[index];
        if let Some(curve) = &record.curve {
            group_curve.entry(root).or_insert_with(|| curve.clone());
        }
        group_tolerance
            .entry(root)
            .and_modify(|value| *value = value.max(record.tolerance_3d))
            .or_insert(record.tolerance_3d);
    }
    let mut group_points = BTreeMap::<usize, [Point3; 2]>::new();
    for (root, curve) in &group_curve {
        let domain = curve_domain(curve)?;
        group_points.insert(
            *root,
            [
                evaluate_nurbs(curve, domain[0])?,
                evaluate_nurbs(curve, domain[1])?,
            ],
        );
    }
    for (face_index, face) in brep.faces.iter().enumerate() {
        for (loop_index, loop_record) in face.loops.iter().enumerate() {
            let globals = trim_paths
                .iter()
                .enumerate()
                .find_map(|(global, path)| (*path == (face_index, loop_index, 0)).then_some(global))
                .map(|start| (start..start + loop_record.trims.len()).collect::<Vec<_>>())
                .unwrap_or_default();
            for (position, global) in globals.iter().copied().enumerate() {
                let root = roots[global];
                if group_points.contains_key(&root) {
                    continue;
                }
                let previous = globals[(position + globals.len() - 1) % globals.len()];
                let previous_record =
                    &loop_record.trims[(position + globals.len() - 1) % globals.len()];
                if let Some(points) = group_points.get(&roots[previous]).copied() {
                    let point = if previous_record.reversed {
                        points[0]
                    } else {
                        points[1]
                    };
                    group_points.insert(root, [point, point]);
                }
            }
        }
    }
    for root in &group_roots {
        if !group_points.contains_key(root) {
            return Err(CodecError::Malformed(
                "V1 edge group has no model-space endpoint curve".to_string(),
            ));
        }
    }

    // The V1 reader creates vertices from trim connectivity.  A loop joins
    // each trim end to the next trim start, and a glued edge joins the
    // corresponding ends of every trim that uses that edge.  Coordinates
    // are only used for the resulting vertex position; they never identify
    // two topologically separate vertices.
    let mut endpoint_parents = (0..trim_paths.len() * 2).collect::<Vec<_>>();
    for (face_index, face) in brep.faces.iter().enumerate() {
        for (loop_index, loop_record) in face.loops.iter().enumerate() {
            let start = face_trim_indices[face_index]
                .iter()
                .position(|global| {
                    let (_, candidate_loop, candidate_trim) = trim_paths[*global];
                    candidate_loop == loop_index && candidate_trim == 0
                })
                .map(|position| face_trim_indices[face_index][position])
                .ok_or_else(|| CodecError::Malformed("V1 loop has no indexed trim".to_string()))?;
            let globals = (0..loop_record.trims.len())
                .map(|offset| start + offset)
                .collect::<Vec<_>>();
            for (position, global) in globals.iter().copied().enumerate() {
                let next = globals[(position + 1) % globals.len()];
                let (_, _, trim_index) = trim_paths[global];
                let (_, _, next_trim_index) = trim_paths[next];
                let current = &loop_record.trims[trim_index];
                let following = &loop_record.trims[next_trim_index];
                let current_root = roots[global];
                let next_root = roots[next];
                let current_end = usize::from(!current.reversed);
                let next_start = usize::from(following.reversed);
                union(
                    &mut endpoint_parents,
                    current_root * 2 + current_end,
                    next_root * 2 + next_start,
                );
            }
        }
    }
    for (index, root) in roots.iter().copied().enumerate() {
        let (_, loop_index, trim_index) = trim_paths[index];
        let trim = &brep.faces[trim_paths[index].0].loops[loop_index].trims[trim_index];
        for endpoint in 0..2 {
            let slot = if trim.reversed {
                1 - endpoint
            } else {
                endpoint
            };
            union(&mut endpoint_parents, index * 2 + endpoint, root * 2 + slot);
        }
    }

    let mut class_samples = BTreeMap::<usize, Vec<(Point3, f64)>>::new();
    for root in &group_roots {
        let points = group_points[root];
        let tolerance = group_tolerance.get(root).copied().unwrap_or(0.0);
        for (slot, point) in points.into_iter().enumerate() {
            let class = find_root(&mut endpoint_parents, root * 2 + slot);
            class_samples
                .entry(class)
                .or_default()
                .push((point, tolerance));
        }
    }
    let mut vertex_by_class = BTreeMap::<usize, cadmpeg_ir::ids::VertexId>::new();
    for (class, samples) in class_samples {
        let count = samples.len() as f64;
        let (position, tolerance) = samples.into_iter().fold(
            (Point3::new(0.0, 0.0, 0.0), 0.0_f64),
            |(sum, maximum_tolerance), (point, sample_tolerance)| {
                (
                    Point3::new(sum.x + point.x, sum.y + point.y, sum.z + point.z),
                    maximum_tolerance.max(sample_tolerance),
                )
            },
        );
        let position = Point3::new(position.x / count, position.y / count, position.z / count);
        let index = vertex_by_class.len();
        let point_id: cadmpeg_ir::ids::PointId =
            format!("rhino:object:point#{suffix}.vertex-{index}").into();
        let vertex_id: cadmpeg_ir::ids::VertexId =
            format!("rhino:object:vertex#{suffix}.slot-{index}").into();
        ir.model.points.push(Point {
            id: point_id.clone(),
            position,
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: (tolerance > 0.0).then_some(tolerance),
        });
        vertex_by_class.insert(class, vertex_id);
    }
    let mut group_vertices = BTreeMap::new();
    for root in &group_roots {
        let ids = [0, 1].map(|slot| {
            let class = find_root(&mut endpoint_parents, root * 2 + slot);
            vertex_by_class
                .get(&class)
                .expect("every V1 endpoint class has a vertex")
                .clone()
        });
        group_vertices.insert(*root, ids);
    }
    let mut group_edges = BTreeMap::new();
    for (edge_index, root) in group_roots.iter().copied().enumerate() {
        let curve_id = if let Some(curve) = group_curve.remove(&root) {
            let id: cadmpeg_ir::ids::CurveId =
                format!("rhino:object:curve#{suffix}.edge-{edge_index}").into();
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: CurveGeometry::Nurbs(curve.clone()),
                source_object: None,
            });
            Some((id, curve_domain(&curve)?))
        } else {
            None
        };
        let edge_id: cadmpeg_ir::ids::EdgeId =
            format!("rhino:object:edge#{suffix}.slot-{edge_index}").into();
        let vertices = group_vertices
            .get(&root)
            .ok_or_else(|| CodecError::Malformed("V1 edge has no vertices".to_string()))?;
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: curve_id.as_ref().map(|value| value.0.clone()),
            start: vertices[0].clone(),
            end: vertices[1].clone(),
            param_range: curve_id.map(|value| value.1),
            tolerance: group_tolerance
                .get(&root)
                .copied()
                .filter(|value| *value > 0.0),
        });
        group_edges.insert(root, edge_id);
    }
    let mut shell_faces = Vec::with_capacity(brep.faces.len());
    let mut coedges_by_root = BTreeMap::<usize, Vec<cadmpeg_ir::ids::CoedgeId>>::new();
    let mut global_trim = 0_usize;
    for (face_index, face_record) in brep.faces.into_iter().enumerate() {
        let surface_id: cadmpeg_ir::ids::SurfaceId =
            format!("rhino:object:surface#{suffix}.face-{face_index}").into();
        let face_id: cadmpeg_ir::ids::FaceId =
            format!("rhino:object:face#{suffix}.slot-{face_index}").into();
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(face_record.surface),
            source_object: None,
        });
        let mut face_loops = Vec::with_capacity(face_record.loops.len());
        for (loop_index, loop_record) in face_record.loops.into_iter().enumerate() {
            let loop_id: cadmpeg_ir::ids::LoopId =
                format!("rhino:object:loop#{suffix}.face-{face_index}-{loop_index}").into();
            let mut coedge_ids = Vec::with_capacity(loop_record.trims.len());
            let coedge_start = ir.model.coedges.len();
            for (trim_index, trim) in loop_record.trims.into_iter().enumerate() {
                let root = roots[global_trim];
                let pcurve_id: cadmpeg_ir::ids::PcurveId = format!(
                    "rhino:object:pcurve#{suffix}.face-{face_index}-{loop_index}-{trim_index}"
                )
                .into();
                let pcurve_domain = curve_domain(&trim.pcurve)?;
                ir.model.pcurves.push(Pcurve {
                    id: pcurve_id.clone(),
                    geometry: PcurveGeometry::Nurbs {
                        nurbs: PcurveNurbs::new(
                            trim.pcurve.degree(),
                            trim.pcurve.knots().to_vec(),
                            trim.pcurve
                                .control_points()
                                .into_iter()
                                .map(|point| Point2::new(point.x, point.y))
                                .collect(),
                            trim.pcurve.weights().map(<[f64]>::to_vec),
                            trim.pcurve.periodic(),
                        )
                        .map_err(|error| CodecError::Malformed(error.to_string()))?,
                    },
                    metadata: cadmpeg_ir::geometry::PcurveMetadata::general(
                        None,
                        Some(pcurve_domain),
                        (trim.tolerance_2d > 0.0).then_some(trim.tolerance_2d),
                    ),
                });
                let coedge_id: cadmpeg_ir::ids::CoedgeId = format!(
                    "rhino:object:coedge#{suffix}.face-{face_index}-{loop_index}-{trim_index}"
                )
                .into();
                coedges_by_root
                    .entry(root)
                    .or_default()
                    .push(coedge_id.clone());
                coedge_ids.push(coedge_id.clone());
                ir.model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: group_edges[&root].clone(),
                    next: coedge_id.clone(),
                    previous: coedge_id.clone(),
                    radial_next: coedge_id,
                    sense: if trim.reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves: vec![PcurveUse {
                        pcurve: pcurve_id,
                        isoparametric: None,
                        parameter_range: Some(pcurve_domain),
                    }],
                    use_curve: None,
                });
                global_trim += 1;
            }
            for index in 0..coedge_ids.len() {
                let coedge = &mut ir.model.coedges[coedge_start + index];
                coedge.previous =
                    coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()].clone();
                coedge.next = coedge_ids[(index + 1) % coedge_ids.len()].clone();
            }
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role: loop_record.role,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
                    coedges: coedge_ids,
                    vertex_uses: Vec::new(),
                },
            });
            face_loops.push(loop_id);
        }
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id.clone(),
            surface: surface_id,
            sense: if face_record.reversed {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            loops: face_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        shell_faces.push(face_id);
    }
    let coedge_positions = ir
        .model
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for ring in coedges_by_root.values() {
        for index in 0..ring.len() {
            ir.model.coedges[coedge_positions[&ring[index]]].radial_next =
                ring[(index + 1) % ring.len()].clone();
        }
    }
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: shell_faces,
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::General,
        regions: vec![region_id],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    Ok(())
}

fn legacy_trim(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<LegacyTrim, CodecError> {
    let stuff = nested_stuff(data, range, TCODE_LEGACY_TRM, TCODE_LEGACY_TRMSTUFF)?;
    let mut reader =
        BoundedReader::new(data, stuff.body.start, stuff.body.end).map_err(malformed)?;
    let flags = reader.u8().map_err(malformed)?;
    let has_edge = flags % 2 != 0;
    let has_mate = flags & 6 != 0;
    let seam = flags & 2 != 0;
    let reversed = reader.i32().map_err(malformed)? != 0;
    let _continuity = reader.i32().map_err(malformed)?;
    let _monotonicity = reader.i32().map_err(malformed)?;
    let tolerance_3d = reader.f64().map_err(malformed)? * scale;
    let tolerance_2d = reader.f64().map_err(malformed)?;
    let pcurve_wrapper = nested_chunk(data, &mut reader, TCODE_LEGACY_CRV)?;
    let pcurve = legacy_curve(data, pcurve_wrapper.body, 1.0)?;
    let curve = if has_edge {
        let curve_wrapper = nested_chunk(data, &mut reader, TCODE_LEGACY_CRV)?;
        Some(legacy_curve(data, curve_wrapper.body, scale)?)
    } else {
        None
    };
    Ok(LegacyTrim {
        has_mate,
        seam,
        reversed,
        tolerance_3d,
        tolerance_2d,
        pcurve,
        curve,
    })
}

fn legacy_loop(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<LegacyLoop, CodecError> {
    let stuff = nested_stuff(data, range, TCODE_LEGACY_BND, TCODE_LEGACY_BNDSTUFF)?;
    let mut reader =
        BoundedReader::new(data, stuff.body.start, stuff.body.end).map_err(malformed)?;
    let count = usize::try_from(reader.i32().map_err(malformed)?)
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| CodecError::Malformed("invalid V1 boundary trim count".to_string()))?;
    let boundary_type = reader.i32().map_err(malformed)?;
    let role = match boundary_type {
        0 => LoopBoundaryRole::Outer,
        1 => LoopBoundaryRole::Inner,
        -1 => LoopBoundaryRole::Unspecified,
        _ => {
            return Err(CodecError::Malformed(
                "invalid V1 boundary type".to_string(),
            ))
        }
    };
    reader.skip(32).map_err(malformed)?;
    let mut trims = Vec::with_capacity(count);
    for _ in 0..count {
        let trim = nested_chunk(data, &mut reader, TCODE_LEGACY_TRM)?;
        trims.push(legacy_trim(data, trim.body, scale)?);
    }
    Ok(LegacyLoop { role, trims })
}

fn legacy_face(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<LegacyFace, CodecError> {
    let stuff = nested_stuff(data, range, TCODE_LEGACY_FAC, TCODE_LEGACY_FACSTUFF)?;
    let mut reader =
        BoundedReader::new(data, stuff.body.start, stuff.body.end).map_err(malformed)?;
    let reversed = match reader.i32().map_err(malformed)? {
        0 => false,
        1 => true,
        _ => {
            return Err(CodecError::Malformed(
                "invalid V1 face reversal".to_string(),
            ))
        }
    };
    let _face_type = reader.i32().map_err(malformed)?;
    let boundary_flags = reader.i32().map_err(malformed)?;
    if boundary_flags < 0 {
        return Err(CodecError::Malformed(
            "invalid V1 face boundary count".to_string(),
        ));
    }
    let boundary_count = usize::try_from(boundary_flags / 2)
        .map_err(|_| CodecError::Malformed("V1 face boundary count overflow".to_string()))?;
    reader.skip(48).map_err(malformed)?;
    let glue_count = usize::try_from(reader.i32().map_err(malformed)?)
        .map_err(|_| CodecError::Malformed("invalid V1 face seam count".to_string()))?;
    let seam_glue = (0..glue_count)
        .map(|_| reader.u16().map(usize::from).map_err(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    let surface_chunk = nested_chunk(data, &mut reader, TCODE_LEGACY_SRF)?;
    let surface = legacy_surface(data, surface_chunk.body, scale)?;
    let mut loops = Vec::with_capacity(boundary_count);
    for _ in 0..boundary_count {
        let loop_chunk = nested_chunk(data, &mut reader, TCODE_LEGACY_BND)?;
        loops.push(legacy_loop(data, loop_chunk.body, scale)?);
    }
    Ok(LegacyFace {
        reversed,
        seam_glue,
        surface,
        loops,
    })
}

fn legacy_brep(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    scale: f64,
) -> Result<LegacyBrep, CodecError> {
    if chunk.typecode == TCODE_LEGACY_FAC {
        return Ok(LegacyBrep {
            shell_glue: Vec::new(),
            faces: vec![legacy_face(data, chunk.body.clone(), scale)?],
        });
    }
    let stuff = child_with_type(data, chunk.body.clone(), TCODE_LEGACY_SHLSTUFF)?
        .ok_or_else(|| CodecError::Malformed("V1 shell has no shell-stuff chunk".to_string()))?;
    let mut reader =
        BoundedReader::new(data, stuff.body.start, stuff.body.end).map_err(malformed)?;
    let _outer = reader.i32().map_err(malformed)?;
    let face_count = usize::try_from(reader.i32().map_err(malformed)?)
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| CodecError::Malformed("invalid V1 shell face count".to_string()))?;
    reader.skip(48).map_err(malformed)?;
    let glue_count = usize::try_from(reader.i32().map_err(malformed)?)
        .map_err(|_| CodecError::Malformed("invalid V1 shell glue count".to_string()))?;
    let shell_glue = (0..glue_count)
        .map(|_| reader.u16().map(usize::from).map_err(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    let mut faces = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        let face = nested_chunk(data, &mut reader, TCODE_LEGACY_FAC)?;
        faces.push(legacy_face(data, face.body, scale)?);
    }
    Ok(LegacyBrep { shell_glue, faces })
}

fn legacy_mesh(
    data: &[u8],
    range: std::ops::Range<usize>,
    id: String,
    scale: f64,
) -> Result<Tessellation, CodecError> {
    let geometry = child_with_type(data, range, TCODE_COMPRESSED_MESH_GEOMETRY)?
        .ok_or_else(|| CodecError::Malformed("V1 mesh has no compressed geometry".to_string()))?;
    let mut reader =
        BoundedReader::new(data, geometry.body.start, geometry.body.end).map_err(malformed)?;
    let point_count = usize::try_from(reader.i32().map_err(malformed)?)
        .ok()
        .filter(|count| *count > 0 && *count <= reader.remaining() / 6)
        .ok_or_else(|| CodecError::Malformed("invalid V1 mesh point count".to_string()))?;
    let face_count = usize::try_from(reader.i32().map_err(malformed)?)
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| CodecError::Malformed("invalid V1 mesh face count".to_string()))?;
    let has_normals = reader.i32().map_err(malformed)? != 0;
    let has_uv = reader.i32().map_err(malformed)? != 0;
    let minimum = Point3::new(
        reader.f64().map_err(malformed)? * scale,
        reader.f64().map_err(malformed)? * scale,
        reader.f64().map_err(malformed)? * scale,
    );
    let maximum = Point3::new(
        reader.f64().map_err(malformed)? * scale,
        reader.f64().map_err(malformed)? * scale,
        reader.f64().map_err(malformed)? * scale,
    );
    let step = [
        (maximum.x - minimum.x) / 65535.0,
        (maximum.y - minimum.y) / 65535.0,
        (maximum.z - minimum.z) / 65535.0,
    ];
    let mut vertices = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let q = [
            reader.u16().map_err(malformed)?,
            reader.u16().map_err(malformed)?,
            reader.u16().map_err(malformed)?,
        ];
        vertices.push(Point3::new(
            minimum.x + step[0] * f64::from(q[0]),
            minimum.y + step[1] * f64::from(q[1]),
            minimum.z + step[2] * f64::from(q[2]),
        ));
    }
    let mut faces = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        let face = if point_count < 65535 {
            [
                u32::from(reader.u16().map_err(malformed)?),
                u32::from(reader.u16().map_err(malformed)?),
                u32::from(reader.u16().map_err(malformed)?),
                u32::from(reader.u16().map_err(malformed)?),
            ]
        } else {
            [
                reader.u32().map_err(malformed)?,
                reader.u32().map_err(malformed)?,
                reader.u32().map_err(malformed)?,
                reader.u32().map_err(malformed)?,
            ]
        };
        if face
            .iter()
            .any(|index| usize::try_from(*index).map_or(true, |index| index >= point_count))
        {
            return Err(CodecError::Malformed(
                "V1 mesh face index is out of range".to_string(),
            ));
        }
        faces.push(face);
    }
    let mut normals = Vec::new();
    if has_normals {
        normals.reserve(point_count);
        for _ in 0..point_count {
            normals.push(Vector3::new(
                f64::from(reader.u8().map_err(malformed)? as i8) / 127.0,
                f64::from(reader.u8().map_err(malformed)? as i8) / 127.0,
                f64::from(reader.u8().map_err(malformed)? as i8) / 127.0,
            ));
        }
    }
    if has_uv {
        reader
            .skip(
                point_count
                    .checked_mul(4)
                    .ok_or_else(|| CodecError::Malformed("V1 mesh UV size overflow".to_string()))?,
            )
            .map_err(malformed)?;
    }
    let triangles = crate::mesh::triangulate_faces(&faces, &vertices);
    Ok(Tessellation {
        id,
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices,
        triangles,
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals,
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    })
}

fn evaluate_nurbs(curve: &NurbsCurve, parameter: f64) -> Result<Point3, CodecError> {
    let degree = usize::try_from(curve.degree())
        .map_err(|_| CodecError::Malformed("V1 curve degree overflow".to_string()))?;
    let last = curve
        .control_points()
        .len()
        .checked_sub(1)
        .ok_or_else(|| CodecError::Malformed("V1 curve has no control points".to_string()))?;
    let span = if parameter >= curve.knots()[last + 1] {
        last
    } else {
        (degree..=last)
            .find(|index| {
                parameter >= curve.knots()[*index] && parameter < curve.knots()[*index + 1]
            })
            .ok_or_else(|| {
                CodecError::Malformed("V1 curve parameter is outside knot domain".to_string())
            })?
    };
    let mut values = (0..=degree)
        .map(|j| {
            let index = span - degree + j;
            let point = curve.control_points()[index];
            let weight = curve.weights().map_or(1.0, |weights| weights[index]);
            [point.x * weight, point.y * weight, point.z * weight, weight]
        })
        .collect::<Vec<_>>();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let index = span - degree + j;
            let denominator = curve.knots()[index + degree + 1 - level] - curve.knots()[index];
            let alpha = if denominator == 0.0 {
                0.0
            } else {
                (parameter - curve.knots()[index]) / denominator
            };
            let previous = values[j - 1];
            for (coordinate, value) in values[j].iter_mut().enumerate() {
                *value = (1.0 - alpha) * previous[coordinate] + alpha * *value;
            }
        }
    }
    let value = values[degree];
    if value[3] == 0.0 || !value[3].is_finite() {
        return Err(CodecError::Malformed(
            "V1 curve evaluates with invalid weight".to_string(),
        ));
    }
    Ok(Point3::new(
        value[0] / value[3],
        value[1] / value[3],
        value[2] / value[3],
    ))
}

/// Decodes the V1 flat geometry stream.
pub(crate) fn decode_v1(data: &[u8]) -> Result<Decoded, CodecError> {
    let header = parse_header(data).map_err(malformed)?;
    if header.archive_version != ArchiveVersion::V1 {
        return Err(CodecError::Malformed(
            "legacy decoder requires V1".to_string(),
        ));
    }
    let mut offset = header.start_offset + file_header::LEN;
    let comment =
        chunk_at(data, offset, data.len(), ArchiveVersion::V1, false).map_err(malformed)?;
    if comment.typecode != TCODE_COMMENT || comment.short {
        return Err(CodecError::Malformed(
            "V1 first post-header chunk is not the comment".to_string(),
        ));
    }
    offset = comment.next_offset;

    // The flat legacy grammar is the strategy `rhino:archive-1` declares, so
    // this path admits the document on its own row. It reads no properties
    // table, so no openNURBS writer-version stamp is declared.
    let primary = ArchiveVersion::V1.classify(None);

    let mut ir = CadIr::empty();
    ir.source = Some(crate::container::source_meta(
        primary,
        crate::container::SourceMetaDetail::FlatLegacyArchive,
    ));
    let mut decoded = 0_usize;
    let mut decoded_curves = 0_usize;
    let mut decoded_meshes = 0_usize;
    let mut decoded_breps = 0_usize;
    let mut decoded_annotations = 0_usize;
    let mut decoded_nurbs_curves = 0_usize;
    let mut decoded_nurbs_surfaces = 0_usize;
    let mut decoded_nurbs_breps = 0_usize;
    let mut omitted = BTreeMap::<u32, usize>::new();
    let mut opaque_records = Vec::new();
    let mut direct_records = Vec::new();
    let mut retained_bytes = 0_usize;
    let mut diagnostics = Vec::new();
    let mut scale = 1.0_f64;
    while offset < data.len() {
        let chunk =
            chunk_at(data, offset, data.len(), ArchiveVersion::V1, false).map_err(malformed)?;
        if chunk.typecode == TCODE_ENDOFFILE {
            break;
        }
        if chunk.typecode == TCODE_ENDOFTABLE {
            offset = chunk.next_offset;
            continue;
        }
        if chunk.typecode == TCODE_UNIT_AND_TOLERANCES && !chunk.short {
            let mut reader =
                BoundedReader::new(data, chunk.body.start, chunk.body.end).map_err(malformed)?;
            let version = reader.i32().map_err(malformed)?;
            if version != 1 {
                return Err(CodecError::malformed(format_args!(
                    "unsupported V1 unit structure {version}"
                )));
            }
            let unit = reader.i32().map_err(malformed)?;
            scale = if unit == 0 {
                1.0
            } else {
                crate::settings::standard_scale(unit).ok_or_else(|| {
                    CodecError::malformed(format_args!("unsupported V1 unit system {unit}"))
                })?
            };
            ir.tolerances.linear = reader.f64().map_err(malformed)? * scale;
            let _relative_tolerance = reader.f64().map_err(malformed)?;
            ir.tolerances.angular = reader.f64().map_err(malformed)?;
        } else if is_v1_presentation_setting(chunk.typecode) && !chunk.short {
            *omitted.entry(chunk.typecode).or_default() += 1;
            opaque_records.push(retain_v1_record(data, &chunk, &mut retained_bytes));
        } else if chunk.typecode == TCODE_RH_POINT && !chunk.short {
            let mut reader =
                BoundedReader::new(data, chunk.body.start, chunk.body.end).map_err(malformed)?;
            let position = Point3::new(
                reader.f64().map_err(malformed)? * scale,
                reader.f64().map_err(malformed)? * scale,
                reader.f64().map_err(malformed)? * scale,
            );
            if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                return Err(CodecError::malformed(format_args!(
                    "V1 point at offset {offset} is not finite"
                )));
            }
            let suffix = format!("legacy-{decoded:06}");
            let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{suffix}").into();
            let region_id: cadmpeg_ir::ids::RegionId =
                format!("rhino:object:region#{suffix}").into();
            let shell_id: cadmpeg_ir::ids::ShellId = format!("rhino:object:shell#{suffix}").into();
            let vertex_id: cadmpeg_ir::ids::VertexId =
                format!("rhino:object:vertex#{suffix}").into();
            let point_id: cadmpeg_ir::ids::PointId = format!("rhino:object:point#{suffix}").into();
            ir.model.points.push(Point {
                id: point_id.clone(),
                position,
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            ir.model.shells.push(Shell {
                id: shell_id.clone(),
                region: region_id.clone(),
                faces: Vec::new(),
                wire_edges: Vec::new(),
                free_vertices: vec![vertex_id],
            });
            ir.model.regions.push(Region {
                id: region_id.clone(),
                body: body_id.clone(),
                shells: vec![shell_id],
            });
            ir.model.bodies.push(Body {
                id: body_id,
                kind: BodyKind::General,
                regions: vec![region_id],
                transform: None,
                name: None,
                color: None,
                visible: None,
            });
            decoded += 1;
        } else if matches!(
            chunk.typecode,
            TCODE_TEXT_BLOCK
                | TCODE_ANNOTATION_LEADER
                | TCODE_LINEAR_DIMENSION
                | TCODE_ANGULAR_DIMENSION
                | TCODE_RADIAL_DIMENSION
                | TCODE_RHINOIO_OBJECT_NURBS_CURVE
                | TCODE_RHINOIO_OBJECT_NURBS_SURFACE
                | TCODE_RHINOIO_OBJECT_BREP
        ) && !chunk.short
        {
            match v1_direct_record(data, &chunk, scale) {
                Ok(record) => {
                    if matches!(
                        chunk.typecode,
                        TCODE_TEXT_BLOCK
                            | TCODE_ANNOTATION_LEADER
                            | TCODE_LINEAR_DIMENSION
                            | TCODE_ANGULAR_DIMENSION
                            | TCODE_RADIAL_DIMENSION
                    ) {
                        decoded_annotations += 1;
                    } else if chunk.typecode == TCODE_RHINOIO_OBJECT_NURBS_CURVE {
                        decoded_nurbs_curves += 1;
                    } else if chunk.typecode == TCODE_RHINOIO_OBJECT_NURBS_SURFACE {
                        decoded_nurbs_surfaces += 1;
                    } else {
                        decoded_nurbs_breps += 1;
                    }
                    direct_records.push(record);
                }
                Err(error) => {
                    diagnostics.push(format!("V1 direct record at offset {offset}: {error}"));
                    *omitted.entry(chunk.typecode).or_default() += 1;
                    opaque_records.push(retain_v1_record(data, &chunk, &mut retained_bytes));
                }
            }
        } else if chunk.typecode == TCODE_LEGACY_CRV && !chunk.short {
            match legacy_curve_segments(data, chunk.body.clone(), scale) {
                Ok(segments) => {
                    for segment in segments {
                        let suffix = format!("legacy-{decoded_curves:06}");
                        let curve_id: cadmpeg_ir::ids::CurveId =
                            format!("rhino:object:curve#{suffix}").into();
                        let body_id: cadmpeg_ir::ids::BodyId =
                            format!("rhino:object:body#curve-{suffix}").into();
                        let region_id: cadmpeg_ir::ids::RegionId =
                            format!("rhino:object:region#curve-{suffix}").into();
                        let shell_id: cadmpeg_ir::ids::ShellId =
                            format!("rhino:object:shell#curve-{suffix}").into();
                        let edge_id: cadmpeg_ir::ids::EdgeId =
                            format!("rhino:object:edge#{suffix}").into();
                        let start_vertex: cadmpeg_ir::ids::VertexId =
                            format!("rhino:object:vertex#{suffix}.start").into();
                        let end_vertex: cadmpeg_ir::ids::VertexId =
                            format!("rhino:object:vertex#{suffix}.end").into();
                        let start_point: cadmpeg_ir::ids::PointId =
                            format!("rhino:object:point#{suffix}.start").into();
                        let end_point: cadmpeg_ir::ids::PointId =
                            format!("rhino:object:point#{suffix}.end").into();
                        let degree = usize::try_from(segment.degree()).map_err(|_| {
                            CodecError::Malformed("V1 curve degree is negative".to_string())
                        })?;
                        let parameter_range = [
                            segment.knots()[degree],
                            segment.knots()[segment.knots().len() - degree - 1],
                        ];
                        let start = evaluate_nurbs(&segment, parameter_range[0])?;
                        let end = evaluate_nurbs(&segment, parameter_range[1])?;
                        ir.model.curves.push(Curve {
                            id: curve_id.clone(),
                            geometry: CurveGeometry::Nurbs(segment),
                            source_object: None,
                        });
                        ir.model.points.extend([
                            Point {
                                id: start_point.clone(),
                                position: start,
                                source_object: None,
                            },
                            Point {
                                id: end_point.clone(),
                                position: end,
                                source_object: None,
                            },
                        ]);
                        ir.model.vertices.extend([
                            Vertex {
                                id: start_vertex.clone(),
                                point: start_point,
                                tolerance: None,
                            },
                            Vertex {
                                id: end_vertex.clone(),
                                point: end_point,
                                tolerance: None,
                            },
                        ]);
                        ir.model.edges.push(Edge {
                            id: edge_id.clone(),
                            curve: Some(curve_id),
                            start: start_vertex,
                            end: end_vertex,
                            param_range: Some(parameter_range),
                            tolerance: None,
                        });
                        ir.model.shells.push(Shell {
                            id: shell_id.clone(),
                            region: region_id.clone(),
                            faces: Vec::new(),
                            wire_edges: vec![edge_id],
                            free_vertices: Vec::new(),
                        });
                        ir.model.regions.push(Region {
                            id: region_id.clone(),
                            body: body_id.clone(),
                            shells: vec![shell_id],
                        });
                        ir.model.bodies.push(Body {
                            id: body_id,
                            kind: BodyKind::General,
                            regions: vec![region_id],
                            transform: None,
                            name: None,
                            color: None,
                            visible: None,
                        });
                        decoded_curves += 1;
                    }
                }
                Err(error) => {
                    diagnostics.push(format!("V1 curve at offset {offset}: {error}"));
                    *omitted.entry(chunk.typecode).or_default() += 1;
                    opaque_records.push(retain_v1_record(data, &chunk, &mut retained_bytes));
                }
            }
        } else if matches!(chunk.typecode, TCODE_LEGACY_FAC | TCODE_LEGACY_SHL) && !chunk.short {
            match legacy_brep(data, &chunk, scale).and_then(|brep| {
                append_legacy_brep(&mut ir, brep, &format!("legacy-brep-{decoded_breps:06}"))
            }) {
                Ok(()) => decoded_breps += 1,
                Err(error) => {
                    diagnostics.push(format!("V1 Brep at offset {offset}: {error}"));
                    *omitted.entry(chunk.typecode).or_default() += 1;
                    opaque_records.push(retain_v1_record(data, &chunk, &mut retained_bytes));
                }
            }
        } else if chunk.typecode == TCODE_MESH_OBJECT && !chunk.short {
            match legacy_mesh(
                data,
                chunk.body.clone(),
                format!("rhino:object:tessellation#legacy-{decoded_meshes:06}"),
                scale,
            ) {
                Ok(mesh) => {
                    ir.model.tessellations.push(mesh);
                    decoded_meshes += 1;
                }
                Err(error) => {
                    diagnostics.push(format!("V1 mesh at offset {offset}: {error}"));
                    *omitted.entry(chunk.typecode).or_default() += 1;
                    opaque_records.push(retain_v1_record(data, &chunk, &mut retained_bytes));
                }
            }
        } else {
            *omitted.entry(chunk.typecode).or_default() += 1;
            opaque_records.push(retain_v1_record(data, &chunk, &mut retained_bytes));
        }
        offset = chunk.next_offset;
    }
    if !direct_records.is_empty() {
        let namespace = ir.native.namespace_mut("rhino");
        namespace.ensure_version_at_least(
            std::num::NonZeroU32::new(2).expect("Rhino native version is nonzero"),
        );
        namespace
            .set_arena("legacy_v1_records", &direct_records)
            .expect("Rhino V1 direct records serialize");
    }
    ir.model.finalize();
    let opaque_count = opaque_records.len();
    let opaque_bytes = opaque_records
        .iter()
        .filter(|record| record.data().is_some())
        .count();
    let losses = omitted
        .into_iter()
        .map(|(typecode, count)| {
            if is_v1_presentation_setting(typecode) {
                RhinoLossCode::PresentationRecordDropped.note(format!(
                    "V1 presentation typecode {typecode:#010x}: {count} record(s) retained as opaque"
                ))
            } else {
                RhinoLossCode::ObjectFamilyNotTransferred.note(format!(
                    "V1 typecode {typecode:#010x}: {count} flat geometry records not transferred"
                ))
            }
        })
        .collect();
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
    source_fidelity.retain_unknown_records("rhino", opaque_records);
    Ok(Decoded {
        ir,
        body: DecodeBody {
            geometry_transferred:
                decoded > 0 || decoded_curves > 0 || decoded_meshes > 0 || decoded_breps > 0,
            coverage: [
                (crate::coverage::LEGACY_V1_POINTS, decoded),
                (crate::coverage::LEGACY_V1_CURVE_SEGMENTS, decoded_curves),
                (crate::coverage::LEGACY_V1_MESHES, decoded_meshes),
                (crate::coverage::LEGACY_V1_BREPS, decoded_breps),
                (crate::coverage::LEGACY_V1_ANNOTATIONS, decoded_annotations),
                (crate::coverage::LEGACY_V1_NURBS_CURVES, decoded_nurbs_curves),
                (
                    crate::coverage::LEGACY_V1_NURBS_SURFACES,
                    decoded_nurbs_surfaces,
                ),
                (crate::coverage::LEGACY_V1_NURBS_BREPS, decoded_nurbs_breps),
            ]
            .into_iter()
            .collect(),
            losses,
            notes: std::iter::once(format!(
            "decoded {decoded} V1 point records, {decoded_curves} curve segments, {decoded_meshes} meshes, and {decoded_breps} Breps"
        ))
        .chain((!direct_records.is_empty()).then(|| {
            format!(
                "typed {decoded_annotations} V1 annotations, {decoded_nurbs_curves} pre-class NURBS curves, {decoded_nurbs_surfaces} pre-class NURBS surfaces, and {decoded_nurbs_breps} pre-class NURBS Breps"
            )
        }))
        .chain((opaque_count > 0).then(|| {
            format!(
                "retained metadata/digests for {opaque_count} unsupported V1 records; complete bytes for {opaque_bytes}"
            )
        }))
        .chain(diagnostics)
        .collect(),
            transfer_ledger: TransferLedger::default(),
        },
        source_fidelity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = typecode.to_le_bytes().to_vec();
        bytes.extend((body.len() as i32).to_le_bytes());
        bytes.extend(body);
        bytes
    }

    fn short(typecode: u32, value: i32) -> Vec<u8> {
        let mut bytes = typecode.to_le_bytes().to_vec();
        bytes.extend(value.to_le_bytes());
        bytes
    }

    fn legacy_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut protected = body.to_vec();
        protected.extend(crate::chunks::crc16(0, body).to_le_bytes());
        chunk(typecode, &protected)
    }

    fn archive(points: &[[f64; 3]]) -> Vec<u8> {
        let mut data = crate::chunks::MAGIC.to_vec();
        data.extend(*b"       1");
        data.extend(chunk(TCODE_COMMENT, b"legacy"));
        for point in points {
            let body = point
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>();
            data.extend(chunk(TCODE_RH_POINT, &body));
        }
        data
    }

    fn v1_settings_archive() -> Vec<u8> {
        let mut units = Vec::new();
        units.extend(1_i32.to_le_bytes());
        units.extend(4_i32.to_le_bytes());
        units.extend(0.01_f64.to_le_bytes());
        units.extend(0.02_f64.to_le_bytes());
        units.extend(0.03_f64.to_le_bytes());
        let mut data = archive(&[]);
        data.extend(chunk(TCODE_UNIT_AND_TOLERANCES, &units));
        for typecode in [TCODE_NAMED_CPLANE, TCODE_NAMED_VIEW, TCODE_VIEWPORT] {
            data.extend(chunk(typecode, &short(TCODE_ENDOFTABLE, 0)));
        }
        data.extend(short(TCODE_ENDOFTABLE, 0));
        data.extend(short(TCODE_ENDOFFILE, 0));
        data
    }

    fn v1_plane() -> Vec<u8> {
        [
            0.0_f64, 0.0, 0.0, // origin
            1.0, 0.0, 0.0, // x axis
            0.0, 1.0, 0.0, // y axis
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect()
    }

    fn v1_string(value: &str) -> Vec<u8> {
        let bytes = value.as_bytes();
        let mut result = (bytes.len() as i32).to_le_bytes().to_vec();
        result.extend(bytes);
        result
    }

    fn v1_annotation_records() -> Vec<Vec<u8>> {
        let plane = v1_plane();
        let mut text = 2_i32.to_le_bytes().to_vec();
        text.extend(0_i32.to_le_bytes());
        text.extend(&plane);
        text.extend(v1_string("text"));
        text.extend(0_i32.to_le_bytes());
        text.extend(1_i32.to_le_bytes());
        text.extend(v1_string("Arial"));
        text.extend(700_i32.to_le_bytes());
        text.extend(2.0_f64.to_le_bytes());

        let mut leader = 1_i32.to_le_bytes().to_vec();
        leader.extend(0_i32.to_le_bytes());
        leader.extend(&plane);
        leader.extend(0_i32.to_le_bytes());
        leader.extend(1_i32.to_le_bytes());
        leader.extend(2_i32.to_le_bytes());
        for point in [[0.0_f64, 0.0, 0.0], [1.0, 2.0, 0.0]] {
            for value in point {
                leader.extend(value.to_le_bytes());
            }
        }

        let mut linear = 1_i32.to_le_bytes().to_vec();
        linear.extend(10_i32.to_le_bytes());
        linear.extend(&plane);
        for index in 0..11 {
            for value in [index as f64, 0.0, 0.0] {
                linear.extend(value.to_le_bytes());
            }
        }
        linear.extend(v1_string("linear"));
        linear.extend(v1_string("default"));
        linear.extend(1_i32.to_le_bytes());
        linear.extend(0_i32.to_le_bytes());
        linear.extend(1_i32.to_le_bytes());

        let mut angular = 1_i32.to_le_bytes().to_vec();
        angular.extend(6_i32.to_le_bytes());
        angular.extend(&plane);
        angular.extend(0.5_f64.to_le_bytes());
        angular.extend(4.0_f64.to_le_bytes());
        for value in [1.0_f64, 2.0, 3.0, 4.0] {
            angular.extend(value.to_le_bytes());
        }
        for index in 0..5 {
            for value in [index as f64, 1.0, 0.0] {
                angular.extend(value.to_le_bytes());
            }
        }
        angular.extend(v1_string("angular"));
        angular.extend(v1_string("default"));
        angular.extend(0_i32.to_le_bytes());
        angular.extend(0_i32.to_le_bytes());
        angular.extend(1_i32.to_le_bytes());

        let mut radial = 1_i32.to_le_bytes().to_vec();
        radial.extend(8_i32.to_le_bytes());
        radial.extend(&plane);
        for index in 0..5 {
            for value in [index as f64, 2.0, 0.0] {
                radial.extend(value.to_le_bytes());
            }
        }
        radial.extend(v1_string("radial"));
        radial.extend(v1_string("default"));
        radial.extend(0_i32.to_le_bytes());
        radial.extend(0_i32.to_le_bytes());
        radial.extend(1_i32.to_le_bytes());

        [
            (TCODE_TEXT_BLOCK, text),
            (TCODE_ANNOTATION_LEADER, leader),
            (TCODE_LINEAR_DIMENSION, linear),
            (TCODE_ANGULAR_DIMENSION, angular),
            (TCODE_RADIAL_DIMENSION, radial),
        ]
        .into_iter()
        .map(|(typecode, body)| chunk(typecode, &body))
        .collect()
    }

    fn rhinoio_curve_object() -> Vec<u8> {
        let mut body = 100_i32.to_le_bytes().to_vec();
        body.extend(3_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(2_i32.to_le_bytes());
        body.extend(2_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        for value in [0.0_f64, 1.0] {
            body.extend(value.to_le_bytes());
        }
        for point in [[0.0_f64, 0.0, 0.0], [1.0, 2.0, 3.0]] {
            for value in point {
                body.extend(value.to_le_bytes());
            }
        }
        let data = chunk(TCODE_RHINOIO_OBJECT_DATA, &body);
        chunk(TCODE_RHINOIO_OBJECT_NURBS_CURVE, &data)
    }

    fn rhinoio_surface_object() -> Vec<u8> {
        let mut body = 101_i32.to_le_bytes().to_vec();
        body.extend(3_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(2_i32.to_le_bytes());
        body.extend(2_i32.to_le_bytes());
        body.extend(2_i32.to_le_bytes());
        body.extend(2_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
        body.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
        for point in [
            [0.0_f64, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
        ] {
            for value in point {
                body.extend(value.to_le_bytes());
            }
        }
        let data = chunk(TCODE_RHINOIO_OBJECT_DATA, &body);
        chunk(TCODE_RHINOIO_OBJECT_NURBS_SURFACE, &data)
    }

    fn rhinoio_brep_object() -> Vec<u8> {
        let curve = rhinoio_curve_object();
        let surface = rhinoio_surface_object();
        let mut body = 100_i32.to_le_bytes().to_vec();
        body.extend(1_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(&curve);
        body.extend(1_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(&curve);
        body.extend(1_i32.to_le_bytes());
        body.extend(&surface);
        // One vertex: index, point, edge-index array, tolerance.
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0] {
            body.extend(value.to_le_bytes());
        }
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0.0_f64.to_le_bytes());

        // One edge: index, C3 index, domain, two vertex indices, trim array,
        // tolerance.
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0.0_f64.to_le_bytes());
        body.extend(1.0_f64.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0.0_f64.to_le_bytes());

        // One trim: index, C2 index, domain, edge, vertices, flags, iso,
        // loop, tolerances, old points, and redundant tolerances.
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0.0_f64.to_le_bytes());
        body.extend(1.0_f64.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        for value in [0.001_f64, 0.001] {
            body.extend(value.to_le_bytes());
        }
        for point in [[0.0_f64, 0.0, 0.0], [1.0, 1.0, 0.0]] {
            for value in point {
                body.extend(value.to_le_bytes());
            }
        }
        for value in [0.001_f64, 0.001] {
            body.extend(value.to_le_bytes());
        }

        // One outer loop: index, trim array, loop type, face index.
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());

        // One face: index, loop array, surface index, reversal flag.
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
            body.extend(value.to_le_bytes());
        }
        let data = chunk(TCODE_RHINOIO_OBJECT_DATA, &body);
        chunk(TCODE_RHINOIO_OBJECT_BREP, &data)
    }

    fn legacy_line(from: [f64; 3], to: [f64; 3], dimension: u8) -> Vec<u8> {
        let mut spline = vec![dimension, 0, 2];
        spline.extend(2_u16.to_le_bytes());
        spline.extend([0, 0]);
        for value in from[..usize::from(dimension)]
            .iter()
            .chain(&to[..usize::from(dimension)])
        {
            spline.extend(value.to_le_bytes());
        }
        spline.extend(0.0_f64.to_le_bytes());
        spline.extend(1.0_f64.to_le_bytes());
        for point in [from, to] {
            for value in &point[..usize::from(dimension)] {
                spline.extend(value.to_le_bytes());
            }
        }
        let spline = legacy_chunk(
            TCODE_LEGACY_SPL,
            &legacy_chunk(TCODE_LEGACY_SPLSTUFF, &spline),
        );
        let mut curve = vec![dimension, 0];
        curve.extend(1_u16.to_le_bytes());
        for value in from[..usize::from(dimension)]
            .iter()
            .chain(&to[..usize::from(dimension)])
        {
            curve.extend(value.to_le_bytes());
        }
        curve.extend(spline);
        legacy_chunk(
            TCODE_LEGACY_CRV,
            &legacy_chunk(TCODE_LEGACY_CRVSTUFF, &curve),
        )
    }

    fn legacy_trim(from: [f64; 3], to: [f64; 3], flags: u8) -> Vec<u8> {
        let mut stuff = vec![flags];
        stuff.extend(0_i32.to_le_bytes());
        stuff.extend(0_i32.to_le_bytes());
        stuff.extend(0_i32.to_le_bytes());
        stuff.extend(0.001_f64.to_le_bytes());
        stuff.extend(0.001_f64.to_le_bytes());
        stuff.extend(legacy_line(from, to, 2));
        if flags & 1 != 0 {
            stuff.extend(legacy_line(from, to, 3));
        }
        legacy_chunk(
            TCODE_LEGACY_TRM,
            &legacy_chunk(TCODE_LEGACY_TRMSTUFF, &stuff),
        )
    }

    fn legacy_surface() -> Vec<u8> {
        let mut stuff = vec![3, 0, 1, 1];
        stuff.extend(1_u16.to_le_bytes());
        stuff.extend(1_u16.to_le_bytes());
        stuff.extend([0, 0, 0, 0, 0, 0]);
        for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            stuff.extend(value.to_le_bytes());
        }
        for _ in 0..2 {
            stuff.extend(0.0_f64.to_le_bytes());
            stuff.extend(1.0_f64.to_le_bytes());
        }
        for point in [
            [0.0_f64, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
        ] {
            for value in point {
                stuff.extend(value.to_le_bytes());
            }
        }
        legacy_chunk(
            TCODE_LEGACY_SRF,
            &legacy_chunk(TCODE_LEGACY_SRFSTUFF, &stuff),
        )
    }

    fn legacy_face_archive_with(corners: &[[f64; 3]], trim_flags: &[u8], glue: &[u16]) -> Vec<u8> {
        assert_eq!(corners.len(), trim_flags.len());
        let mut boundary = (corners.len() as i32).to_le_bytes().to_vec();
        boundary.extend(0_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 1.0, 1.0] {
            boundary.extend(value.to_le_bytes());
        }
        for index in 0..corners.len() {
            boundary.extend(legacy_trim(
                corners[index],
                corners[(index + 1) % corners.len()],
                trim_flags[index],
            ));
        }
        let boundary = legacy_chunk(
            TCODE_LEGACY_BND,
            &legacy_chunk(TCODE_LEGACY_BNDSTUFF, &boundary),
        );
        let mut face = 0_i32.to_le_bytes().to_vec();
        face.extend(0_i32.to_le_bytes());
        face.extend(3_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            face.extend(value.to_le_bytes());
        }
        face.extend((glue.len() as i32).to_le_bytes());
        for value in glue {
            face.extend(value.to_le_bytes());
        }
        face.extend(legacy_surface());
        face.extend(boundary);
        let face = legacy_chunk(
            TCODE_LEGACY_FAC,
            &legacy_chunk(TCODE_LEGACY_FACSTUFF, &face),
        );
        let mut archive = crate::chunks::MAGIC.to_vec();
        archive.extend(*b"       1");
        archive.extend(chunk(TCODE_COMMENT, b"legacy face"));
        archive.extend(face);
        archive
    }

    fn legacy_face_archive() -> Vec<u8> {
        let corners = [
            [0.0_f64, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        legacy_face_archive_with(&corners, &[1, 1, 1, 1], &[])
    }

    fn legacy_shell_archive() -> Vec<u8> {
        let face_archive = legacy_face_archive();
        let comment = chunk_at(
            &face_archive,
            32,
            face_archive.len(),
            ArchiveVersion::V1,
            false,
        )
        .expect("comment chunk");
        let face = chunk_at(
            &face_archive,
            comment.next_offset,
            face_archive.len(),
            ArchiveVersion::V1,
            false,
        )
        .expect("face chunk");
        let mut shell = 1_i32.to_le_bytes().to_vec();
        shell.extend(1_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            shell.extend(value.to_le_bytes());
        }
        shell.extend(0_i32.to_le_bytes());
        shell.extend_from_slice(&face_archive[face.range()]);
        let shell = legacy_chunk(
            TCODE_LEGACY_SHL,
            &legacy_chunk(TCODE_LEGACY_SHLSTUFF, &shell),
        );
        let mut archive = crate::chunks::MAGIC.to_vec();
        archive.extend(*b"       1");
        archive.extend(chunk(TCODE_COMMENT, b"legacy shell"));
        archive.extend(shell);
        archive
    }

    #[test]
    fn v1_flat_points_decode_to_neutral_points() {
        let result = crate::decode::seal_for_test(
            decode_v1(&archive(&[[1.0, 2.0, 3.0], [-4.0, 5.0, 6.0]]))
                .expect("valid V1 point archive"),
            false,
        );
        assert_eq!(result.ir().model.points.len(), 2);
        assert_eq!(
            result.ir().model.points[0].position,
            Point3::new(1.0, 2.0, 3.0)
        );
        assert!(result.report().geometry_transferred());
    }

    #[test]
    fn v1_settings_presentation_records_are_opaque_and_table_end_is_structural() {
        let result = crate::decode::seal_for_test(
            decode_v1(&v1_settings_archive()).expect("valid V1 settings stream"),
            false,
        );
        assert_eq!(result.ir().tolerances.linear, 10.0);
        assert_eq!(result.source_fidelity().retained_records.len(), 3);
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| { loss.code == RhinoLossCode::PresentationRecordDropped.kind() })
                .count(),
            3
        );
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("ffffffff")));
    }

    #[test]
    fn v1_malformed_direct_record_is_retained_atomically() {
        let mut bytes = archive(&[[1.0, 2.0, 3.0]]);
        let record_offset = bytes.len();
        let record = chunk(0x0020_0004, b"legacy annotation payload");
        bytes.extend(&record);

        let result = crate::decode::seal_for_test(
            decode_v1(&bytes).expect("framed malformed direct V1 record"),
            false,
        );
        assert_eq!(result.ir().model.points.len(), 1);
        let retained = &result.source_fidelity().retained_records;
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].offset(), record_offset as u64);
        assert_eq!(retained[0].byte_len(), record.len() as u64);
        assert_eq!(retained[0].data(), Some(record.as_slice()));
        assert_eq!(retained[0].stream(), "rhino");
        assert!(retained[0]
            .id()
            .starts_with("rhino:legacy:record#00200004-"));
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == RhinoLossCode::ObjectFamilyNotTransferred.kind()));
    }

    #[test]
    fn v1_direct_annotations_and_preclass_nurbs_are_typed() {
        let mut bytes = archive(&[]);
        for record in v1_annotation_records() {
            bytes.extend(record);
        }
        bytes.extend(rhinoio_curve_object());
        bytes.extend(rhinoio_surface_object());
        bytes.extend(rhinoio_brep_object());

        let result = crate::decode::seal_for_test(
            decode_v1(&bytes).expect("valid V1 direct records"),
            false,
        );
        let namespace = result
            .ir()
            .native
            .namespace("rhino")
            .expect("Rhino native namespace");
        let records = namespace
            .arenas
            .get("legacy_v1_records")
            .expect("typed V1 direct arena");
        assert_eq!(records.len(), 8);
        let kinds = records
            .iter()
            .filter_map(|record| record.field("payload"))
            .filter_map(|payload| {
                payload
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| kind.as_str() == "Annotation")
                .count(),
            5
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| kind.as_str() == "NurbsCurve")
                .count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| kind.as_str() == "NurbsSurface")
                .count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| kind.as_str() == "NurbsBrep")
                .count(),
            1
        );
        assert_eq!(result.report().coverage()["legacy_v1_annotations"], 5);
        assert_eq!(result.report().coverage()["legacy_v1_nurbs_curves"], 1);
        assert_eq!(result.report().coverage()["legacy_v1_nurbs_surfaces"], 1);
        assert_eq!(result.report().coverage()["legacy_v1_nurbs_breps"], 1);
        assert!(result.source_fidelity().retained_records.is_empty());
    }

    #[test]
    fn v1_legacy_face_decodes_complete_brep_topology() {
        let result = crate::decode::seal_for_test(
            decode_v1(&legacy_face_archive()).expect("valid V1 face archive"),
            false,
        );
        let model = &result.ir().model;
        assert_eq!(model.bodies.len(), 1, "{:?}", result.report());
        assert_eq!(model.faces.len(), 1);
        assert_eq!(model.loops.len(), 1);
        assert_eq!(model.coedges.len(), 4);
        assert_eq!(model.edges.len(), 4);
        assert_eq!(model.pcurves.len(), 4);
        assert_eq!(model.surfaces.len(), 1);
        assert_eq!(result.report().coverage()["legacy_v1_breps"], 1);
        let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
        assert!(report.is_ok(), "{report:?}");
    }

    #[test]
    fn v1_legacy_shell_decodes_nested_faces() {
        let result = crate::decode::seal_for_test(
            decode_v1(&legacy_shell_archive()).expect("valid V1 shell archive"),
            false,
        );
        assert_eq!(result.ir().model.bodies.len(), 1, "{:?}", result.report());
        assert_eq!(result.ir().model.faces.len(), 1);
        assert_eq!(result.report().coverage()["legacy_v1_breps"], 1);
        let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
        assert!(report.is_ok(), "{report:?}");
    }

    #[test]
    fn v1_vertices_follow_trim_connectivity_not_nearby_coordinates() {
        let corners = [
            [0.0_f64, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0005, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let result = crate::decode::seal_for_test(
            decode_v1(&legacy_face_archive_with(&corners, &[1, 1, 1, 1], &[]))
                .expect("nearby but topologically distinct V1 vertices are valid"),
            false,
        );
        assert_eq!(result.ir().model.vertices.len(), 4);
        assert_eq!(result.ir().model.edges.len(), 4);
        let edge_endpoints = result
            .ir()
            .model
            .edges
            .iter()
            .map(|edge| (edge.start.clone(), edge.end.clone()))
            .collect::<Vec<_>>();
        assert_eq!(edge_endpoints[0].0, edge_endpoints[3].1);
        assert_ne!(edge_endpoints[0].0, edge_endpoints[2].0);
    }

    #[test]
    fn v1_seam_glue_keeps_two_explicit_edge_curves_separate() {
        let corners = [
            [0.0_f64, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let result = crate::decode::seal_for_test(
            decode_v1(&legacy_face_archive_with(
                &corners,
                &[3, 3, 3, 3],
                &[1, 0, 3, 2],
            ))
            .expect("explicit seam edge curves are valid"),
            false,
        );
        assert_eq!(result.ir().model.edges.len(), 4);
        assert_eq!(result.ir().model.curves.len(), 4);
        assert!(result
            .ir()
            .model
            .edges
            .iter()
            .all(|edge| edge.curve.is_some()));
    }

    #[test]
    fn v1_seam_glue_attaches_curve_less_trim_to_explicit_edge() {
        let corners = [
            [0.0_f64, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let result = crate::decode::seal_for_test(
            decode_v1(&legacy_face_archive_with(
                &corners,
                &[3, 2, 3, 2],
                &[1, 0, 3, 2],
            ))
            .expect("curve-less seam partners are valid"),
            false,
        );
        assert_eq!(result.ir().model.edges.len(), 2);
        assert_eq!(result.ir().model.curves.len(), 2);
        let coedges = &result.ir().model.coedges;
        assert_eq!(coedges[0].edge, coedges[1].edge);
        assert_eq!(coedges[2].edge, coedges[3].edge);
    }
}
