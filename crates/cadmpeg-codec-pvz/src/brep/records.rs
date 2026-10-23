// SPDX-License-Identifier: Apache-2.0
//! TLV records of the exact-geometry stream (§8.2 to §8.16).

use cadmpeg_core::decode::{index_from_u32, DecodeContext, ParseError, View};
use cadmpeg_core::CodecError;

use super::model::{
    BsplineCurve, BsplineSurface, Coedge, Conic, CurveGeometry, EdgeCurve, ExactModel, Face, Loop,
    OpaqueRecord, Pcurve, Precision, SingularUse, SurfaceGeometry, SurfaceRecord, Torus, Vec2,
};
use super::{EXACT_STREAM_F32, EXACT_STREAM_F64};

/// First record of every OL stream: the stream index (§4.5).
const TAG_STREAM_INDEX: u32 = 0x0100_0002;
/// Supporting surface; starts a face.
const TAG_SURFACE: u32 = 0x13;
/// Loop.
const TAG_LOOP: u32 = 0x0100_0010;
/// Coedge.
const TAG_COEDGE: u32 = 0x11;
/// Singular edge use.
const TAG_SINGULAR_USE: u32 = 0x0100_0012;
/// Loop terminator.
const TAG_TERMINATOR: u32 = 0;
/// Edge curve.
const TAG_EDGE_CURVE: u32 = 0x14;
/// Pcurve.
const TAG_PCURVE: u32 = 0x15;

/// Loop marker that starts a new loop group.
const LOOP_GROUP_START: i32 = -2;
/// Index value that names no record.
const NO_INDEX: u32 = u32::MAX;
/// Coedge record payload length.
const COEDGE_LEN: usize = 13;
/// Loop record payload length.
const LOOP_LEN: usize = 4;
/// Stream index record payload length.
const STREAM_INDEX_LEN: usize = 4;

/// Surface `flags` bit that adds the u-range pair.
const SURFACE_U_RANGE: u8 = 0x80;
/// B-spline surface `flags` bit that makes the surface periodic in u.
const SURFACE_U_PERIODIC: u8 = 0x08;
/// B-spline surface `flags` bit that makes the surface periodic in v.
const SURFACE_V_PERIODIC: u8 = 0x10;
/// Surface `marker` bit that adds the v-range pair.
const SURFACE_V_RANGE: u8 = 0x01;
/// Curve and pcurve `flags` bit that adds a parameter-range pair.
const CURVE_RANGE: u8 = 0x08;
/// Byte that ends a record.
const RECORD_END: u8 = 0x00;

/// Surface type codes (§8.3).
const SURFACE_CONE: u8 = 1;
const SURFACE_CYLINDER: u8 = 2;
const SURFACE_BSPLINE: u8 = 4;
const SURFACE_PLANE: u8 = 5;
const SURFACE_SPHERE: u8 = 8;
const SURFACE_TORUS: u8 = 9;

/// Curve type codes (§8.15, §8.16).
const CURVE_CIRCLE: u8 = 1;
const CURVE_COMPOSITE: u8 = 2;
const CURVE_POINT: u8 = 3;
const CURVE_ELLIPSE: u8 = 4;
const CURVE_LINE: u8 = 5;
const CURVE_BSPLINE: u8 = 6;
const CURVE_POLYLINE: u8 = 7;

/// Section of the stream that the next record belongs to (§8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Faces,
    Curves,
}

/// Reads one exact-geometry stream payload.
pub(super) fn read_stream(
    ctx: &DecodeContext<'_>,
    stream_id: u32,
    payload: View<'_>,
) -> Result<ExactModel, CodecError> {
    let precision = match stream_id {
        EXACT_STREAM_F32 => Precision::Single,
        EXACT_STREAM_F64 => Precision::Double,
        other => {
            return Err(CodecError::malformed(format_args!(
                "stream {other:#010x} is not an exact-geometry stream"
            )))
        }
    };
    let mut cursor = payload;
    let mut builder = StreamBuilder::new(precision);
    let (tag, mut body) = next_record(&mut cursor)?;
    if tag != TAG_STREAM_INDEX || body.remaining() != STREAM_INDEX_LEN {
        return Err(CodecError::malformed(format_args!(
            "exact stream {stream_id:#010x} does not open with its stream index record"
        )));
    }
    let version = body.req_u32_le()?;
    if version != precision.version() {
        return Err(CodecError::malformed(format_args!(
            "exact stream {stream_id:#010x} stores exact version {version}, not {}",
            precision.version()
        )));
    }
    while !cursor.is_empty() {
        let (tag, body) = next_record(&mut cursor)?;
        builder.record(ctx, tag, body)?;
    }
    builder.finish()
}

/// Splits the next TLV record off `cursor`.
fn next_record<'a>(cursor: &mut View<'a>) -> Result<(u32, View<'a>), ParseError> {
    let tag = cursor.req_u32_le()?;
    let length = index_from_u32(cursor.req_u32_le()?);
    let body = cursor.req_take_child(length)?;
    Ok((tag, body))
}

/// Record scope that the next terminator closes (§8.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The loop with this global index.
    Loop(usize),
    /// A singular edge use.
    SingularUse,
}

/// Accumulates records and enforces the record order of §8.2.
struct StreamBuilder {
    model: ExactModel,
    section: Section,
    open: Option<Scope>,
    ended: bool,
}

impl StreamBuilder {
    fn new(precision: Precision) -> Self {
        Self {
            model: ExactModel {
                precision,
                faces: Vec::new(),
                loops: Vec::new(),
                edges: Vec::new(),
                pcurves: Vec::new(),
                opaque: Vec::new(),
            },
            section: Section::Faces,
            open: None,
            ended: false,
        }
    }

    fn precision(&self) -> Precision {
        self.model.precision
    }

    fn record(
        &mut self,
        ctx: &DecodeContext<'_>,
        tag: u32,
        body: View<'_>,
    ) -> Result<(), CodecError> {
        let offset = body.start();
        if self.ended {
            return Err(CodecError::malformed(format_args!(
                "exact-stream record {tag:#x} at offset {offset} follows the stream terminator"
            )));
        }
        match tag {
            TAG_SURFACE => {
                self.require_face_section(tag, offset)?;
                let surface = read_surface(ctx, self.precision(), body)?;
                ctx.charge_entities(1, "pvz exact face")?;
                self.model.faces.push(Face {
                    surface,
                    loops: Vec::new(),
                    singular_uses: Vec::new(),
                    offset,
                });
            }
            TAG_LOOP => self.open_loop(ctx, body)?,
            TAG_COEDGE => self.coedge(ctx, body)?,
            TAG_SINGULAR_USE => self.singular_use(ctx, body)?,
            TAG_TERMINATOR if body.is_empty() => {
                if self.open.take().is_none() {
                    self.ended = true;
                }
            }
            TAG_EDGE_CURVE => {
                self.enter(Section::Curves, tag, offset)?;
                let geometry = read_curve::<3>(ctx, self.precision(), body)?;
                ctx.charge_entities(1, "pvz exact edge curve")?;
                self.model.edges.push(EdgeCurve {
                    flags: geometry.1,
                    geometry: geometry.0,
                    offset,
                });
            }
            TAG_PCURVE => {
                self.enter(Section::Curves, tag, offset)?;
                let geometry = read_curve::<2>(ctx, self.precision(), body)?;
                ctx.charge_entities(1, "pvz exact pcurve")?;
                self.model.pcurves.push(Pcurve {
                    flags: geometry.1,
                    geometry: geometry.0,
                    offset,
                });
            }
            _ => {
                if matches!(self.open, Some(Scope::Loop(_))) {
                    return Err(CodecError::malformed(format_args!(
                        "exact-stream record {tag:#x} at offset {offset} is inside a loop"
                    )));
                }
                ctx.charge_entities(1, "pvz exact opaque record")?;
                self.model.opaque.push(OpaqueRecord {
                    tag,
                    length: body.remaining(),
                    offset,
                });
            }
        }
        Ok(())
    }

    fn require_face_section(&self, tag: u32, offset: usize) -> Result<(), CodecError> {
        if self.section != Section::Faces || self.open.is_some() {
            return Err(CodecError::malformed(format_args!(
                "exact-stream record {tag:#x} at offset {offset} is outside the face section \
                 or inside an open loop"
            )));
        }
        Ok(())
    }

    fn enter(&mut self, section: Section, tag: u32, offset: usize) -> Result<(), CodecError> {
        if section < self.section || self.open.is_some() {
            return Err(CodecError::malformed(format_args!(
                "exact-stream record {tag:#x} at offset {offset} breaks the section order"
            )));
        }
        self.section = section;
        Ok(())
    }

    fn current_face(&self, tag: u32, offset: usize) -> Result<usize, CodecError> {
        self.model.faces.len().checked_sub(1).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "exact-stream record {tag:#x} at offset {offset} precedes every surface record"
            ))
        })
    }

    fn open_loop(&mut self, ctx: &DecodeContext<'_>, mut body: View<'_>) -> Result<(), CodecError> {
        let offset = body.start();
        self.require_face_section(TAG_LOOP, offset)?;
        let face = self.current_face(TAG_LOOP, offset)?;
        if body.remaining() != LOOP_LEN {
            return Err(record_length(TAG_LOOP, offset));
        }
        let marker = body.req_i32_le()?;
        let index = self.model.loops.len();
        let group = if marker == LOOP_GROUP_START {
            index
        } else {
            let first = usize::try_from(marker)
                .ok()
                .filter(|first| {
                    self.model
                        .loops
                        .get(*first)
                        .is_some_and(|start| start.face == face && start.group == *first)
                })
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "loop marker {marker} at offset {offset} names no loop group of its face"
                    ))
                })?;
            first
        };
        ctx.charge_entities(1, "pvz exact loop")?;
        self.model.loops.push(Loop {
            face,
            group,
            coedges: Vec::new(),
            offset,
        });
        if let Some(record) = self.model.faces.get_mut(face) {
            record.loops.push(index);
        }
        self.open = Some(Scope::Loop(index));
        Ok(())
    }

    fn coedge(&mut self, ctx: &DecodeContext<'_>, mut body: View<'_>) -> Result<(), CodecError> {
        let offset = body.start();
        let Some(Scope::Loop(open)) = self.open else {
            return Err(CodecError::malformed(format_args!(
                "coedge record at offset {offset} is outside a loop"
            )));
        };
        if body.remaining() != COEDGE_LEN {
            return Err(record_length(TAG_COEDGE, offset));
        }
        let edge = optional_index(body.req_u32_le()?);
        let orientation = body.req_u8()?;
        let pcurve = optional_index(body.req_u32_le()?);
        let secondary = optional_index(body.req_u32_le()?);
        ctx.charge_entities(1, "pvz exact coedge")?;
        if let Some(record) = self.model.loops.get_mut(open) {
            record.coedges.push(Coedge {
                edge,
                orientation,
                pcurve,
                secondary,
                offset,
            });
        }
        Ok(())
    }

    fn singular_use(&mut self, ctx: &DecodeContext<'_>, body: View<'_>) -> Result<(), CodecError> {
        let offset = body.start();
        self.require_face_section(TAG_SINGULAR_USE, offset)?;
        let face = self.current_face(TAG_SINGULAR_USE, offset)?;
        let mut fields = Fields::new(body, self.precision());
        let edge = index_from_u32(fields.view.req_u32_le()?);
        let range = fields.pair()?;
        fields.finish_bare(TAG_SINGULAR_USE, offset)?;
        ctx.charge_entities(1, "pvz exact singular use")?;
        if let Some(record) = self.model.faces.get_mut(face) {
            record.singular_uses.push(SingularUse {
                edge,
                range,
                offset,
            });
        }
        self.open = Some(Scope::SingularUse);
        Ok(())
    }

    fn finish(self) -> Result<ExactModel, CodecError> {
        if !self.ended {
            return Err(CodecError::malformed(
                "exact stream does not end with its stream terminator",
            ));
        }
        check_indexes(&self.model)?;
        Ok(self.model)
    }
}

fn optional_index(value: u32) -> Option<usize> {
    (value != NO_INDEX).then(|| index_from_u32(value))
}

fn record_length(tag: u32, offset: usize) -> CodecError {
    CodecError::malformed(format_args!(
        "exact-stream record {tag:#x} at offset {offset} has the wrong payload length"
    ))
}

/// Checks every cross-section index against the target section count.
fn check_indexes(model: &ExactModel) -> Result<(), CodecError> {
    let edges = model.edges.len();
    let pcurves = model.pcurves.len();
    let out_of_range = |what: &str, index: usize, offset: usize| {
        CodecError::malformed(format_args!(
            "{what} index {index} at offset {offset} is outside its section"
        ))
    };
    for record in &model.loops {
        for coedge in &record.coedges {
            if let Some(edge) = coedge.edge.filter(|edge| *edge >= edges) {
                return Err(out_of_range("coedge edge", edge, coedge.offset));
            }
            for pcurve in [coedge.pcurve, coedge.secondary].into_iter().flatten() {
                if pcurve >= pcurves {
                    return Err(out_of_range("coedge pcurve", pcurve, coedge.offset));
                }
            }
        }
    }
    for face in &model.faces {
        for singular in &face.singular_uses {
            if singular.edge >= edges {
                return Err(out_of_range(
                    "singular-use edge",
                    singular.edge,
                    singular.offset,
                ));
            }
        }
    }
    for curve in &model.edges {
        if let CurveGeometry::Composite(members) = &curve.geometry {
            if let Some(member) = members.iter().find(|member| **member >= edges) {
                return Err(out_of_range("composite edge member", *member, curve.offset));
            }
        }
    }
    for curve in &model.pcurves {
        if let CurveGeometry::Composite(members) = &curve.geometry {
            if let Some(member) = members.iter().find(|member| **member >= pcurves) {
                return Err(out_of_range(
                    "composite pcurve member",
                    *member,
                    curve.offset,
                ));
            }
        }
    }
    Ok(())
}

/// Scalar field reader over one record payload.
struct Fields<'a> {
    view: View<'a>,
    precision: Precision,
}

impl<'a> Fields<'a> {
    fn new(view: View<'a>, precision: Precision) -> Self {
        Self { view, precision }
    }

    fn scalar(&mut self) -> Result<f64, ParseError> {
        match self.precision {
            Precision::Single => self.view.req_f32_le().map(f64::from),
            Precision::Double => self.view.req_f64_le(),
        }
    }

    fn vector<const N: usize>(&mut self) -> Result<[f64; N], ParseError> {
        let mut value = [0.0; N];
        for slot in &mut value {
            *slot = self.scalar()?;
        }
        Ok(value)
    }

    fn pair(&mut self) -> Result<Vec2, ParseError> {
        self.vector::<2>()
    }

    fn optional_pair(&mut self, present: bool) -> Result<Option<Vec2>, ParseError> {
        if present {
            self.pair().map(Some)
        } else {
            Ok(None)
        }
    }

    /// A `u32` element count whose elements of `scalars` scalars each must
    /// fit in the unread payload.
    fn count(&mut self, scalars: usize, what: &str) -> Result<usize, CodecError> {
        let offset = self.view.position();
        let count = self.view.req_u32_le()?;
        let width = scalars
            .checked_mul(self.precision.width())
            .ok_or_else(|| CodecError::malformed(format_args!("{what} element width overflows")))?;
        self.view
            .counted(u64::from(count), width.max(1))
            .map(cadmpeg_core::decode::BoundedCount::get)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{what} count {count} at offset {offset} exceeds its record"
                ))
            })
    }

    fn scalars(
        &mut self,
        ctx: &DecodeContext<'_>,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<f64>, CodecError> {
        let mut values = ctx.alloc_filled(count, 0.0, operation)?;
        for value in &mut values {
            *value = self.scalar()?;
        }
        Ok(values)
    }

    /// Reads the record end byte and requires the payload to end there.
    fn finish(mut self, tag: u32, offset: usize) -> Result<(), CodecError> {
        if self.view.req_u8()? != RECORD_END || !self.view.is_empty() {
            return Err(record_length(tag, offset));
        }
        Ok(())
    }

    /// Requires the payload to end here, with no end byte.
    fn finish_bare(self, tag: u32, offset: usize) -> Result<(), CodecError> {
        if self.view.is_empty() {
            Ok(())
        } else {
            Err(record_length(tag, offset))
        }
    }
}

fn read_surface(
    ctx: &DecodeContext<'_>,
    precision: Precision,
    body: View<'_>,
) -> Result<SurfaceRecord, CodecError> {
    let offset = body.start();
    let mut fields = Fields::new(body, precision);
    let surface_type = fields.view.req_u8()?;
    let flags = fields.view.req_u8()?;
    let marker = fields.view.req_u8()?;
    let has_u = flags & SURFACE_U_RANGE != 0;
    let has_v = marker & SURFACE_V_RANGE != 0;
    let geometry = match surface_type {
        SURFACE_PLANE => SurfaceGeometry::Plane {
            origin: fields.vector()?,
            u_axis: fields.vector()?,
            v_axis: fields.vector()?,
            normal: fields.vector()?,
        },
        SURFACE_CYLINDER => SurfaceGeometry::Cylinder {
            origin: fields.vector()?,
            x: fields.vector()?,
            y: fields.vector()?,
            axis: fields.vector()?,
        },
        SURFACE_CONE => SurfaceGeometry::Cone {
            origin: fields.vector()?,
            x: fields.vector()?,
            y: fields.vector()?,
            axis: fields.vector()?,
            a: fields.scalar()?,
            b: fields.scalar()?,
        },
        SURFACE_SPHERE => SurfaceGeometry::Sphere {
            center: fields.vector()?,
            x: fields.vector()?,
            y: fields.vector()?,
            z: fields.vector()?,
        },
        SURFACE_TORUS => SurfaceGeometry::Torus(Torus {
            origin: fields.vector()?,
            major_cos: fields.vector()?,
            major_sin: fields.vector()?,
            axis: fields.vector()?,
            minor_cos: fields.vector()?,
            minor_axial: fields.vector()?,
            minor_sin: fields.vector()?,
        }),
        SURFACE_BSPLINE => {
            let rational = read_rational(&mut fields, offset)?;
            let mut surface = read_bspline_body(ctx, &mut fields, rational, offset)?;
            surface.periodic = [
                flags & SURFACE_U_PERIODIC != 0,
                flags & SURFACE_V_PERIODIC != 0,
            ];
            let u_range = fields.optional_pair(has_u)?;
            let v_range = fields.optional_pair(has_v)?;
            fields.finish(TAG_SURFACE, offset)?;
            return Ok(SurfaceRecord {
                flags,
                marker,
                u_range,
                v_range,
                geometry: SurfaceGeometry::Bspline(surface),
                embedded: None,
            });
        }
        surface_type => {
            return Ok(SurfaceRecord {
                flags,
                marker,
                u_range: None,
                v_range: None,
                geometry: SurfaceGeometry::Undefined { surface_type },
                embedded: None,
            });
        }
    };
    let u_range = fields.optional_pair(has_u)?;
    let v_range = fields.optional_pair(has_v)?;
    let embedded = read_analytic_tail(ctx, fields, offset)?;
    Ok(SurfaceRecord {
        flags,
        marker,
        u_range,
        v_range,
        geometry,
        embedded,
    })
}

/// Reads the tail of an analytic surface record: one end byte, or an
/// embedded B-spline that ends at the end of the payload (§8.10).
fn read_analytic_tail(
    ctx: &DecodeContext<'_>,
    mut fields: Fields<'_>,
    offset: usize,
) -> Result<Option<BsplineSurface>, CodecError> {
    if fields.view.remaining() == 1 {
        fields.finish(TAG_SURFACE, offset)?;
        return Ok(None);
    }
    let head = [
        fields.view.req_u8()?,
        fields.view.req_u8()?,
        fields.view.req_u8()?,
    ];
    if head != [SURFACE_BSPLINE, 0, 0] {
        return Err(CodecError::malformed(format_args!(
            "analytic surface record at offset {offset} has an embedded head {head:02x?}"
        )));
    }
    let rational = read_rational(&mut fields, offset)?;
    let surface = read_bspline_body(ctx, &mut fields, rational, offset)?;
    fields.finish_bare(TAG_SURFACE, offset)?;
    Ok(Some(surface))
}

fn read_rational(fields: &mut Fields<'_>, offset: usize) -> Result<bool, CodecError> {
    match fields.view.req_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(CodecError::malformed(format_args!(
            "B-spline record at offset {offset} has rational byte {other}"
        ))),
    }
}

/// Order and knot vector of one parameter direction.
fn read_knot_axis(
    ctx: &DecodeContext<'_>,
    fields: &mut Fields<'_>,
    offset: usize,
) -> Result<(usize, Vec<f64>), CodecError> {
    let order = index_from_u32(fields.view.req_u32_le()?);
    let count = fields.count(1, "knot")?;
    if order == 0 || count <= order {
        return Err(CodecError::malformed(format_args!(
            "B-spline record at offset {offset} has order {order} and {count} knots"
        )));
    }
    let knots = fields.scalars(ctx, count, "pvz exact knots")?;
    Ok((order, knots))
}

fn read_bspline_body(
    ctx: &DecodeContext<'_>,
    fields: &mut Fields<'_>,
    rational: bool,
    offset: usize,
) -> Result<BsplineSurface, CodecError> {
    let (u_order, u_knots) = read_knot_axis(ctx, fields, offset)?;
    let (v_order, v_knots) = read_knot_axis(ctx, fields, offset)?;
    let u_count = index_from_u32(fields.view.req_u32_le()?);
    let v_count = index_from_u32(fields.view.req_u32_le()?);
    if u_knots.len().checked_sub(u_order) != Some(u_count)
        || v_knots.len().checked_sub(v_order) != Some(v_count)
    {
        return Err(CodecError::malformed(format_args!(
            "B-spline surface at offset {offset} has a pole grid that does not follow its knots"
        )));
    }
    let total = u_count
        .checked_mul(v_count)
        .and_then(|total| u64::try_from(total).ok())
        .ok_or_else(|| CodecError::malformed("B-spline surface pole count overflows"))?;
    let width = fields
        .precision
        .width()
        .checked_mul(4)
        .ok_or_else(|| CodecError::malformed("B-spline surface pole width overflows"))?;
    let total = fields
        .view
        .counted(total, width)
        .map(cadmpeg_core::decode::BoundedCount::get)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "B-spline surface at offset {offset} has more poles than bytes"
            ))
        })?;
    let mut poles = ctx.alloc_filled(total, [0.0; 4], "pvz exact surface poles")?;
    for pole in &mut poles {
        *pole = fields.vector()?;
    }
    Ok(BsplineSurface {
        rational,
        u_order,
        v_order,
        u_knots,
        v_knots,
        u_count,
        v_count,
        poles,
        periodic: [false, false],
    })
}

/// Reads one edge-curve (`N` = 3) or pcurve (`N` = 2) record and returns
/// its geometry and `flags` byte.
fn read_curve<const N: usize>(
    ctx: &DecodeContext<'_>,
    precision: Precision,
    body: View<'_>,
) -> Result<(CurveGeometry<N>, u8), CodecError> {
    let offset = body.start();
    let tag = if N == 3 { TAG_EDGE_CURVE } else { TAG_PCURVE };
    let edge_curve = N == 3;
    let mut fields = Fields::new(body, precision);
    let curve_type = fields.view.req_u8()?;
    let flags = fields.view.req_u8()?;
    let ranged = flags & CURVE_RANGE != 0;
    let geometry = match curve_type {
        CURVE_CIRCLE if edge_curve => CurveGeometry::Circle(read_conic(&mut fields, ranged)?),
        CURVE_ELLIPSE => CurveGeometry::Ellipse(read_conic(&mut fields, ranged)?),
        CURVE_POINT if edge_curve => CurveGeometry::Point(fields.vector()?),
        CURVE_LINE => CurveGeometry::Line {
            origin: fields.vector()?,
            direction: fields.vector()?,
        },
        CURVE_COMPOSITE => {
            let count = fields.view.req_u32_le()?;
            let count = fields
                .view
                .counted(u64::from(count), 4)
                .map(cadmpeg_core::decode::BoundedCount::get)
                .ok_or_else(|| record_length(tag, offset))?;
            let mut members = ctx.alloc_filled(count, 0, "pvz exact composite members")?;
            for member in &mut members {
                *member = index_from_u32(fields.view.req_u32_le()?);
            }
            CurveGeometry::Composite(members)
        }
        CURVE_BSPLINE => {
            let rational = read_rational(&mut fields, offset)?;
            let order = index_from_u32(fields.view.req_u32_le()?);
            let knot_count = fields.count(1, "knot")?;
            if order == 0 || knot_count <= order {
                return Err(CodecError::malformed(format_args!(
                    "B-spline curve at offset {offset} has order {order} and {knot_count} knots"
                )));
            }
            let knots = fields.scalars(ctx, knot_count, "pvz exact knots")?;
            let pole_count = fields.count(N + 1, "pole")?;
            if knot_count.checked_sub(order) != Some(pole_count) {
                return Err(CodecError::malformed(format_args!(
                    "B-spline curve at offset {offset} has {pole_count} poles for {knot_count} \
                     knots of order {order}"
                )));
            }
            let mut weighted = ctx.alloc_filled(pole_count, [0.0; N], "pvz exact curve poles")?;
            let mut weights = ctx.alloc_filled(pole_count, 0.0, "pvz exact curve weights")?;
            for (point, weight) in weighted.iter_mut().zip(weights.iter_mut()) {
                *point = fields.vector()?;
                *weight = fields.scalar()?;
            }
            let range = fields.optional_pair(ranged && !edge_curve)?;
            CurveGeometry::Bspline(BsplineCurve {
                rational,
                order,
                knots,
                weighted,
                weights,
                range,
            })
        }
        CURVE_POLYLINE => {
            let count = fields.count(N, "polyline point")?;
            let mut points = ctx.alloc_filled(count, [0.0; N], "pvz exact polyline")?;
            for point in &mut points {
                *point = fields.vector()?;
            }
            CurveGeometry::Polyline(points)
        }
        curve_type => return Ok((CurveGeometry::Undefined { curve_type }, flags)),
    };
    fields.finish(tag, offset)?;
    Ok((geometry, flags))
}

fn read_conic<const N: usize>(
    fields: &mut Fields<'_>,
    ranged: bool,
) -> Result<Conic<N>, ParseError> {
    Ok(Conic {
        center: fields.vector()?,
        x: fields.vector()?,
        y: fields.vector()?,
        range: fields.optional_pair(ranged)?,
    })
}

#[cfg(test)]
mod tests;
