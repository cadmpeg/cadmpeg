// SPDX-License-Identifier: Apache-2.0
//! The planar area of a swept CSG solid: the boundary loops of a
//! `CURVE_BOUNDED_SURFACE` whose basis surface is a `PLANE`
//! (ISO 10303-42 §6.4.37, `swept_area_solid` WR1).

use std::f64::consts::TAU;

use cadmpeg_ir::math::{Point3, Vector3};

use super::super::geometry::GeometryData;
use super::super::{RecordExt, ValueExt};
use super::angles::AngleUnits;
use super::frame::{rotate_about, unit, Frame};
use crate::parse::{Exchange, RawRecord, Value};

/// One curve segment of a profile loop, in model millimetres, traversed from
/// its start to its end.
#[derive(Debug, Clone, Copy)]
pub(super) enum Segment {
    /// A straight segment.
    Line { start: Point3, end: Point3 },
    /// A circular arc that turns counterclockwise about `normal` through
    /// `sweep` radians, in `(0, 2π]`, from `start`.
    Arc {
        center: Point3,
        normal: Vector3,
        radius: f64,
        start: Point3,
        sweep: f64,
    },
}

impl Segment {
    pub(super) fn start(&self) -> Point3 {
        match *self {
            Self::Line { start, .. } | Self::Arc { start, .. } => start,
        }
    }

    pub(super) fn end(&self) -> Point3 {
        match *self {
            Self::Line { end, .. } => end,
            Self::Arc {
                center,
                normal,
                start,
                sweep,
                ..
            } => rotate_about(start, center, normal, sweep),
        }
    }

    /// The point at `fraction` of the traversal.
    pub(super) fn point_at(&self, fraction: f64) -> Point3 {
        match *self {
            Self::Line { start, end } => start.translated(end.vector_from(start), fraction),
            Self::Arc {
                center,
                normal,
                start,
                sweep,
                ..
            } => rotate_about(start, center, normal, sweep * fraction),
        }
    }

    /// The unit direction of travel at `fraction` of the traversal.
    pub(super) fn tangent_at(&self, fraction: f64) -> Option<Vector3> {
        match *self {
            Self::Line { start, end } => unit(end.vector_from(start)),
            Self::Arc { center, normal, .. } => {
                unit(normal.cross(self.point_at(fraction).vector_from(center)))
            }
        }
    }

    /// The same point set traversed from its end to its start.
    pub(super) fn reversed(&self) -> Self {
        match *self {
            Self::Line { start, end } => Self::Line {
                start: end,
                end: start,
            },
            Self::Arc {
                center,
                normal,
                radius,
                sweep,
                ..
            } => Self::Arc {
                center,
                normal: normal.scale(-1.0),
                radius,
                start: self.end(),
                sweep,
            },
        }
    }

    /// The segment moved by `map`, which is a rigid motion given as a point
    /// map and its direction map.
    pub(super) fn moved(
        &self,
        place: &impl Fn(Point3) -> Point3,
        turn: &impl Fn(Vector3) -> Vector3,
    ) -> Self {
        match *self {
            Self::Line { start, end } => Self::Line {
                start: place(start),
                end: place(end),
            },
            Self::Arc {
                center,
                normal,
                radius,
                start,
                sweep,
            } => Self::Arc {
                center: place(center),
                normal: turn(normal),
                radius,
                start: place(start),
                sweep,
            },
        }
    }
}

/// A closed loop of segments: segment `i` ends where segment `i + 1` starts.
pub(super) type ProfileLoop = Vec<Segment>;

/// A planar area: its plane and its boundary loops, the outer loop first.
#[derive(Debug, Clone)]
pub(super) struct Profile {
    pub(super) origin: Point3,
    pub(super) normal: Vector3,
    pub(super) loops: Vec<ProfileLoop>,
}

/// The number of chords per arc that the signed-area estimate takes.
const AREA_CHORDS_PER_ARC: u32 = 16;

impl Profile {
    /// Orient the outer loop counterclockwise about `normal` and every other
    /// loop clockwise about it.
    pub(super) fn oriented(mut self, normal: Vector3) -> Self {
        for (index, profile_loop) in self.loops.iter_mut().enumerate() {
            let counterclockwise = signed_area(profile_loop, normal) > 0.0;
            if counterclockwise != (index == 0) {
                *profile_loop = reversed_loop(profile_loop);
            }
        }
        self
    }

    /// The profile with every segment end and arc centre moved by `place`,
    /// in the plane through `origin` normal to the unit `normal`. Each arc
    /// keeps its sense about the plane normal and runs from its moved start
    /// to its moved end. Its centre then moves to the point equidistant from
    /// both ends, along the direction `slide` gives for the source centre or
    /// else along the chord.
    pub(super) fn mapped(
        &self,
        origin: Point3,
        normal: Vector3,
        place: impl Fn(Point3) -> Point3,
        slide: impl Fn(Point3) -> Option<Vector3>,
    ) -> Self {
        let mut loops = Vec::with_capacity(self.loops.len());
        for profile_loop in &self.loops {
            let count = profile_loop.len();
            let mut mapped = Vec::with_capacity(count);
            for (index, segment) in profile_loop.iter().enumerate() {
                let start = place(segment.start());
                let end = place(profile_loop[(index + 1) % count].start());
                mapped.push(match *segment {
                    Segment::Line { .. } => Segment::Line { start, end },
                    Segment::Arc {
                        center,
                        normal: arc_normal,
                        sweep,
                        ..
                    } => {
                        let center = equidistant(place(center), start, end, slide(center));
                        let arc_normal = normal.scale(arc_normal.dot(normal).signum());
                        let sweep = if count == 1 {
                            sweep
                        } else {
                            turn_between(center, arc_normal, start, end).unwrap_or(sweep)
                        };
                        Segment::Arc {
                            center,
                            normal: arc_normal,
                            radius: start.distance(center),
                            start,
                            sweep,
                        }
                    }
                });
            }
            loops.push(mapped);
        }
        Self {
            origin,
            normal,
            loops,
        }
    }

    /// The profile moved by a rigid motion, given as its point map and its
    /// direction map.
    pub(super) fn moved(
        &self,
        place: &impl Fn(Point3) -> Point3,
        turn: &impl Fn(Vector3) -> Vector3,
    ) -> Self {
        Self {
            origin: place(self.origin),
            normal: turn(self.normal),
            loops: self
                .loops
                .iter()
                .map(|profile_loop| {
                    profile_loop
                        .iter()
                        .map(|segment| segment.moved(place, turn))
                        .collect()
                })
                .collect(),
        }
    }

    /// Every point the loops reach at their segment ends and arc midpoints.
    pub(super) fn sample_points(&self) -> Vec<Point3> {
        let mut points = Vec::new();
        for segment in self.loops.iter().flatten() {
            points.push(segment.start());
            points.push(segment.point_at(0.5));
        }
        points
    }
}

/// `center` moved along `direction`, or along the chord where `direction`
/// is `None`, to the point as far from `start` as from `end`.
fn equidistant(center: Point3, start: Point3, end: Point3, direction: Option<Vector3>) -> Point3 {
    let Some(direction) = direction.or_else(|| unit(end.vector_from(start))) else {
        return center;
    };
    let span = direction.dot(start.vector_from(end));
    if span.abs() <= f64::EPSILON * start.distance(end).max(1.0) {
        return center;
    }
    let excess = start.distance_squared(center) - end.distance_squared(center);
    center.translated(direction, excess / (2.0 * span))
}

/// The loop traversed backward.
fn reversed_loop(profile_loop: &[Segment]) -> ProfileLoop {
    profile_loop.iter().rev().map(Segment::reversed).collect()
}

/// Twice the signed area enclosed by the loop about `normal`, from a chord
/// polygon through the segment ends and arc interiors.
fn signed_area(profile_loop: &[Segment], normal: Vector3) -> f64 {
    let mut polygon = Vec::new();
    for segment in profile_loop {
        match segment {
            Segment::Line { start, .. } => polygon.push(*start),
            Segment::Arc { .. } => {
                for step in 0..AREA_CHORDS_PER_ARC {
                    polygon
                        .push(segment.point_at(f64::from(step) / f64::from(AREA_CHORDS_PER_ARC)));
                }
            }
        }
    }
    let Some(&anchor) = polygon.first() else {
        return 0.0;
    };
    let mut sum = Vector3::new(0.0, 0.0, 0.0);
    for pair in polygon.windows(2) {
        sum = sum
            + pair[0]
                .vector_from(anchor)
                .cross(pair[1].vector_from(anchor));
    }
    sum.dot(normal)
}

/// The relative resolution of profile coordinates: two profile coordinates
/// closer than this fraction of the profile's extent are one coordinate.
pub(super) const PROFILE_RELATIVE_RESOLUTION: f64 = 1.0e-7;

/// A planar area and the largest junction gap its boundaries closed.
pub(super) struct ReadProfile {
    pub(super) profile: Profile,
    pub(super) gap: f64,
}

/// Read the planar area of `CURVE_BOUNDED_SURFACE` `surface`.
///
/// Each boundary is a `boundary_curve`, closed by its WHERE rule, whose
/// segments meet with positional continuity (ISO 10303-42 §4.5.82 and
/// §4.5.45). A junction gap closes when it is within the larger of
/// `uncertainty` and [`PROFILE_RELATIVE_RESOLUTION`] of the profile's
/// extent; a larger gap refuses the profile. A junction between a straight
/// segment and an arc takes the straight segment's end; the arc runs through
/// the junction points on its circle.
pub(super) fn read(
    surface: u64,
    exchange: &Exchange,
    geometry: &GeometryData,
    angles: &AngleUnits<'_>,
    uncertainty: f64,
) -> Result<ReadProfile, String> {
    let record = record_of(exchange, surface)?;
    let parameters = simple_parameters(record, "CURVE_BOUNDED_SURFACE", surface)?;
    let plane = parameters
        .get(1)
        .and_then(Value::reference)
        .ok_or_else(|| format!("CURVE_BOUNDED_SURFACE #{surface} has no basis surface"))?;
    let plane_record = record_of(exchange, plane)?;
    let position = simple_parameters(plane_record, "PLANE", plane)?
        .get(1)
        .and_then(Value::reference)
        .ok_or_else(|| format!("PLANE #{plane} has no position"))?;
    let Some(&(origin, axis, _)) = geometry.placements.get(&position) else {
        return Err(format!(
            "PLANE #{plane} has an unresolved position #{position}"
        ));
    };
    let boundaries = parameters
        .get(2)
        .and_then(Value::list)
        .ok_or_else(|| format!("CURVE_BOUNDED_SURFACE #{surface} has no boundary list"))?;
    let mut read_loops = Vec::new();
    for boundary in boundaries {
        let boundary = boundary.reference().ok_or_else(|| {
            format!("CURVE_BOUNDED_SURFACE #{surface} has a non-reference boundary")
        })?;
        read_loops.push((
            boundary,
            boundary_segments(boundary, exchange, geometry, angles)?,
        ));
    }
    if read_loops.is_empty() {
        return Err(format!("CURVE_BOUNDED_SURFACE #{surface} has no boundary"));
    }
    let size = extent(read_loops.iter().flat_map(|(_, read)| read));
    let tolerance = uncertainty.max(PROFILE_RELATIVE_RESOLUTION * size);
    let mut gap = 0.0_f64;
    let mut loops = Vec::new();
    for (boundary, read) in &read_loops {
        let (closed, closed_gap) = close_junctions(read, tolerance).map_err(|message| {
            format!("boundary #{boundary} of CURVE_BOUNDED_SURFACE #{surface} {message}")
        })?;
        gap = gap.max(closed_gap);
        loops.push(closed);
    }
    let normal = *axis.as_raw();
    loops.sort_by(|left, right| {
        signed_area(right, normal)
            .abs()
            .total_cmp(&signed_area(left, normal).abs())
    });
    Ok(ReadProfile {
        profile: Profile {
            origin: origin.get(),
            normal,
            loops,
        },
        gap,
    })
}

/// The diagonal of the box that holds every segment end and arc circle.
fn extent<'a>(read: impl Iterator<Item = &'a ReadSegment>) -> f64 {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut include = |point: Point3, reach: f64| {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value - reach);
            high[axis] = high[axis].max(value + reach);
        }
    };
    for piece in read {
        match piece.segment {
            Segment::Line { start, end } => {
                include(start, 0.0);
                include(end, 0.0);
            }
            Segment::Arc { center, radius, .. } => include(center, radius),
        }
    }
    (0..3)
        .map(|axis| (high[axis] - low[axis]).max(0.0).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn record_of(exchange: &Exchange, id: u64) -> Result<&RawRecord, String> {
    exchange
        .records()
        .get(&id)
        .ok_or_else(|| format!("#{id} is not an entity instance of the exchange"))
}

/// The parameters of a simple entity instance of type `name`.
fn simple_parameters<'a>(
    record: &'a RawRecord,
    name: &str,
    id: u64,
) -> Result<&'a [Value], String> {
    match record.simple_name() {
        Some(simple) if simple == name => Ok(record.parameters()),
        _ => Err(format!("#{id} is not a simple {name} instance")),
    }
}

/// One curve segment as read, before the junctions are closed.
struct ReadSegment {
    segment: Segment,
    /// Whether the segment's ends are stated points (a line, or a trim by
    /// Cartesian points) rather than parameters on a circle.
    exact_ends: bool,
}

fn boundary_segments(
    boundary: u64,
    exchange: &Exchange,
    geometry: &GeometryData,
    angles: &AngleUnits<'_>,
) -> Result<Vec<ReadSegment>, String> {
    let record = record_of(exchange, boundary)?;
    let name = record
        .simple_name()
        .filter(|name| {
            matches!(
                *name,
                "BOUNDARY_CURVE" | "OUTER_BOUNDARY_CURVE" | "COMPOSITE_CURVE"
            )
        })
        .ok_or_else(|| format!("boundary #{boundary} is not a composite curve"))?;
    let segments = record
        .parameters()
        .get(1)
        .and_then(Value::list)
        .ok_or_else(|| format!("{name} #{boundary} has no segment list"))?;
    let mut read = Vec::new();
    for segment in segments {
        let segment = segment
            .reference()
            .ok_or_else(|| format!("{name} #{boundary} has a non-reference segment"))?;
        let segment_record = record_of(exchange, segment)?;
        let parameters = match segment_record.simple_name() {
            Some("COMPOSITE_CURVE_SEGMENT" | "REPARAMETRISED_COMPOSITE_CURVE_SEGMENT") => {
                segment_record.parameters()
            }
            _ => return Err(format!("#{segment} is not a composite curve segment")),
        };
        let same_sense = parameters
            .get(1)
            .and_then(Value::logical)
            .ok_or_else(|| format!("composite curve segment #{segment} has no same_sense"))?;
        let parent = parameters
            .get(2)
            .and_then(Value::reference)
            .ok_or_else(|| format!("composite curve segment #{segment} has no parent curve"))?;
        let mut pieces = parent_curve(parent, exchange, geometry, angles)?;
        if !same_sense {
            pieces.reverse();
            for piece in &mut pieces {
                piece.segment = piece.segment.reversed();
            }
        }
        read.extend(pieces);
    }
    Ok(read)
}

/// Join each segment's end to the next segment's start, and re-run every arc
/// through its junction points. A gap above `tolerance` refuses the loop.
/// One arc closes on itself as its full circle.
fn close_junctions(read: &[ReadSegment], tolerance: f64) -> Result<(ProfileLoop, f64), String> {
    let wide = |gap: f64| {
        format!("leaves a gap of {gap:e} mm, above the closing tolerance {tolerance:e} mm")
    };
    if let [only] = read {
        let gap = only.segment.end().distance(only.segment.start());
        if gap > tolerance {
            return Err(wide(gap));
        }
        return match only.segment {
            Segment::Arc {
                center,
                normal,
                radius,
                start,
                ..
            } => Ok((
                vec![Segment::Arc {
                    center,
                    normal,
                    radius,
                    start,
                    sweep: TAU,
                }],
                gap,
            )),
            Segment::Line { .. } => Err("is one straight segment".to_owned()),
        };
    }
    let count = read.len();
    if count == 0 {
        return Err("has no segment".to_owned());
    }
    let mut gap = 0.0_f64;
    let mut junctions = Vec::with_capacity(count);
    for index in 0..count {
        let before = &read[index];
        let after = &read[(index + 1) % count];
        let end = before.segment.end();
        let start = after.segment.start();
        gap = gap.max(end.distance(start));
        let point = match (before.exact_ends, after.exact_ends) {
            (true, _) => end,
            (false, true) => start,
            (false, false) => start.translated(end.vector_from(start), 0.5),
        };
        junctions.push(point);
    }
    if gap > tolerance {
        return Err(wide(gap));
    }
    let mut closed = Vec::with_capacity(count);
    for (index, piece) in read.iter().enumerate() {
        let start = junctions[(index + count - 1) % count];
        let end = junctions[index];
        closed.push(match piece.segment {
            Segment::Line { .. } => Segment::Line { start, end },
            Segment::Arc {
                center,
                normal,
                radius,
                ..
            } => Segment::Arc {
                center,
                normal,
                radius,
                start,
                sweep: turn_between(center, normal, start, end)
                    .ok_or("has an arc of zero radius")?,
            },
        });
    }
    Ok((closed, gap))
}

/// The counterclockwise turn about `normal` from `start` to `end`, in
/// `(0, 2π]`.
fn turn_between(center: Point3, normal: Vector3, start: Point3, end: Point3) -> Option<f64> {
    let from = unit(start.vector_from(center))?;
    let to = unit(end.vector_from(center))?;
    let angle = normal.dot(from.cross(to)).atan2(from.dot(to));
    let angle = if angle <= 0.0 { angle + TAU } else { angle };
    Some(angle)
}

/// The traversed pieces of a composite segment's parent curve.
fn parent_curve(
    curve: u64,
    exchange: &Exchange,
    geometry: &GeometryData,
    angles: &AngleUnits<'_>,
) -> Result<Vec<ReadSegment>, String> {
    let record = record_of(exchange, curve)?;
    match record.simple_name() {
        Some("TRIMMED_CURVE") => {
            trimmed_curve(curve, record.parameters(), exchange, geometry, angles)
                .map(|piece| vec![piece])
        }
        Some("CIRCLE") => {
            let circle = circle(curve, record.parameters(), geometry)?;
            Ok(vec![ReadSegment {
                segment: circle.arc(0.0, TAU),
                exact_ends: false,
            }])
        }
        Some("POLYLINE") => {
            let points = record
                .parameters()
                .get(1)
                .and_then(Value::list)
                .ok_or_else(|| format!("POLYLINE #{curve} has no point list"))?;
            let mut positions = Vec::new();
            for point in points {
                positions.push(point_of(point.reference(), geometry, curve)?);
            }
            Ok(positions
                .windows(2)
                .map(|pair| ReadSegment {
                    segment: Segment::Line {
                        start: pair[0],
                        end: pair[1],
                    },
                    exact_ends: true,
                })
                .collect())
        }
        Some(other) => Err(format!("profile curve #{curve} is a {other}")),
        None => Err(format!("profile curve #{curve} is a complex instance")),
    }
}

fn point_of(id: Option<u64>, geometry: &GeometryData, owner: u64) -> Result<Point3, String> {
    id.and_then(|id| geometry.points.get(&id))
        .map(|point| point.get())
        .ok_or_else(|| format!("#{owner} names an unresolved point"))
}

/// A `CIRCLE` in model millimetres.
struct Circle {
    frame: Frame,
    radius: f64,
}

impl Circle {
    fn point(&self, angle: f64) -> Point3 {
        self.frame
            .point(self.radius * angle.cos(), self.radius * angle.sin(), 0.0)
    }

    /// The arc from parameter `from` turning by `turn`, negative clockwise.
    fn arc(&self, from: f64, turn: f64) -> Segment {
        let normal = if turn < 0.0 {
            self.frame.z.scale(-1.0)
        } else {
            self.frame.z
        };
        Segment::Arc {
            center: self.frame.origin,
            normal,
            radius: self.radius,
            start: self.point(from),
            sweep: turn.abs().min(TAU),
        }
    }

    /// The parameter of the point of the circle nearest `point`.
    fn parameter_of(&self, point: Point3) -> f64 {
        let offset = point.vector_from(self.frame.origin);
        offset.dot(self.frame.y).atan2(offset.dot(self.frame.x))
    }
}

fn circle(id: u64, parameters: &[Value], geometry: &GeometryData) -> Result<Circle, String> {
    let position = parameters
        .get(1)
        .and_then(Value::reference)
        .ok_or_else(|| format!("CIRCLE #{id} has no position"))?;
    let frame = geometry
        .placements
        .get(&position)
        .map(|&placement| Frame::from_placement(placement))
        .ok_or_else(|| format!("CIRCLE #{id} has an unresolved position #{position}"))?;
    let radius = parameters
        .get(2)
        .and_then(Value::number)
        .map(|radius| radius * geometry.units.length([id]).get())
        .filter(|radius| radius.is_finite() && *radius > 0.0)
        .ok_or_else(|| format!("CIRCLE #{id} has no positive radius"))?;
    Ok(Circle { frame, radius })
}

/// A trim selector: a Cartesian point, a curve parameter, or both.
struct Trim {
    point: Option<Point3>,
    parameter: Option<f64>,
}

fn trim(value: Option<&Value>, geometry: &GeometryData, owner: u64) -> Result<Trim, String> {
    let values = value
        .and_then(Value::list)
        .ok_or_else(|| format!("TRIMMED_CURVE #{owner} has no trim list"))?;
    let mut trim = Trim {
        point: None,
        parameter: None,
    };
    for value in values {
        match value {
            Value::Reference(_) => trim.point = Some(point_of(value.reference(), geometry, owner)?),
            Value::Typed(name, inner) if name == "PARAMETER_VALUE" => {
                trim.parameter = inner.number();
            }
            _ => {}
        }
    }
    Ok(trim)
}

fn trimmed_curve(
    id: u64,
    parameters: &[Value],
    exchange: &Exchange,
    geometry: &GeometryData,
    angles: &AngleUnits<'_>,
) -> Result<ReadSegment, String> {
    let basis = parameters
        .get(1)
        .and_then(Value::reference)
        .ok_or_else(|| format!("TRIMMED_CURVE #{id} has no basis curve"))?;
    let first = trim(parameters.get(2), geometry, id)?;
    let second = trim(parameters.get(3), geometry, id)?;
    let sense = parameters
        .get(4)
        .and_then(Value::logical)
        .ok_or_else(|| format!("TRIMMED_CURVE #{id} has no sense_agreement"))?;
    let cartesian_master = matches!(
        parameters.get(5).and_then(Value::enumeration),
        Some("CARTESIAN" | "UNSPECIFIED")
    );
    let basis_record = record_of(exchange, basis)?;
    match basis_record.simple_name() {
        Some("LINE") => {
            let line = basis_record.parameters();
            let origin = point_of(line.get(1).and_then(Value::reference), geometry, basis)?;
            let vector = line
                .get(2)
                .and_then(Value::reference)
                .and_then(|vector| geometry.vectors.get(&vector))
                .copied()
                .ok_or_else(|| format!("LINE #{basis} has an unresolved vector"))?;
            let end = |trim: &Trim| {
                let parametric = trim.parameter.map(|t| origin.translated(vector, t));
                if cartesian_master {
                    trim.point.or(parametric)
                } else {
                    parametric.or(trim.point)
                }
            };
            let (Some(start), Some(finish)) = (end(&first), end(&second)) else {
                return Err(format!("TRIMMED_CURVE #{id} has unresolved trims"));
            };
            Ok(ReadSegment {
                segment: Segment::Line { start, end: finish },
                exact_ends: true,
            })
        }
        Some("CIRCLE") => {
            let circle = circle(basis, basis_record.parameters(), geometry)?;
            let parameter = |trim: &Trim| {
                let stated = trim.parameter.map(|t| angles.radians(id, t));
                let located = trim.point.map(|point| circle.parameter_of(point));
                if cartesian_master {
                    located.or(stated)
                } else {
                    stated.or(located)
                }
            };
            let (Some(from), Some(to)) = (parameter(&first), parameter(&second)) else {
                return Err(format!("TRIMMED_CURVE #{id} has unresolved trims"));
            };
            let mut turn = to - from;
            if sense && turn <= 0.0 {
                turn += TAU;
            } else if !sense && turn >= 0.0 {
                turn -= TAU;
            }
            let exact_ends = cartesian_master && first.point.is_some() && second.point.is_some();
            Ok(ReadSegment {
                segment: circle.arc(from, turn),
                exact_ends,
            })
        }
        Some(other) => Err(format!("TRIMMED_CURVE #{id} trims a {other}")),
        None => Err(format!("TRIMMED_CURVE #{id} trims a complex instance")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start: [f64; 3], end: [f64; 3]) -> Segment {
        Segment::Line {
            start: start.into(),
            end: end.into(),
        }
    }

    #[test]
    fn a_square_reverses_to_counterclockwise_about_its_normal() {
        let clockwise = vec![
            line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            line([0.0, 1.0, 0.0], [1.0, 1.0, 0.0]),
            line([1.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
            line([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        ];
        let normal = Vector3::new(0.0, 0.0, 1.0);
        assert!(signed_area(&clockwise, normal) < 0.0);
        let profile = Profile {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal,
            loops: vec![clockwise],
        }
        .oriented(normal);
        assert!((signed_area(&profile.loops[0], normal) - 2.0).abs() < 1.0e-12);
    }

    #[test]
    fn a_single_arc_boundary_closes_into_the_full_circle_within_tolerance() {
        let arc = |sweep: f64| ReadSegment {
            segment: Segment::Arc {
                center: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                radius: 2.0,
                start: Point3::new(2.0, 0.0, 0.0),
                sweep,
            },
            exact_ends: false,
        };
        let almost = TAU - CLOSING_TEST_TURN;
        let (closed, gap) = close_junctions(&[arc(almost)], CLOSING_TEST_TOLERANCE)
            .expect("a nearly closed circle closes");
        let Segment::Arc { sweep, .. } = closed[0] else {
            panic!("an arc");
        };
        assert_eq!(sweep, TAU);
        assert!(gap <= CLOSING_TEST_TOLERANCE);
        assert!(close_junctions(&[arc(6.0)], CLOSING_TEST_TOLERANCE).is_err());
    }

    /// A turn whose chord on a circle of radius 2 lies below
    /// [`CLOSING_TEST_TOLERANCE`].
    const CLOSING_TEST_TURN: f64 = 1.0e-9;

    /// The closing tolerance of the unit tests.
    const CLOSING_TEST_TOLERANCE: f64 = 1.0e-6;
}
