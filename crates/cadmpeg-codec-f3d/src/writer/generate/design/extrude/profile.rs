// SPDX-License-Identifier: Apache-2.0
//! Row 5 of the rule: the loops of the region that a sketch's curves bound,
//! in the order the profile records store them (f3d.md §3.1, "Side-face
//! walk", "Walk of a loop of lines" and "Profile with circular holes").

use std::f64::consts::TAU;

use cadmpeg_ir::math::portable::Portable;

use super::super::sketch::{CurveGeometry, PlannedCurve, PlannedSketch};

/// One entry of the profile loop: a curve of the planned sketch, and whether
/// the ring runs it from its end point to its start point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::writer::generate::design) struct LoopEntry {
    pub(in crate::writer::generate::design) curve: usize,
    pub(in crate::writer::generate::design) reversed: bool,
}

impl LoopEntry {
    /// The entry's traversal flag: `1` when the ring runs the curve from
    /// its end point (f3d.md §3.1, "Side-face walk"). The loop entry and
    /// the sketch links of its side face's start-cap edges store it.
    pub(in crate::writer::generate::design) fn traversal_flag(self) -> u32 {
        u32::from(self.reversed)
    }
}

/// One loop of the profile region: its entries in run order, and whether it
/// is the region's exterior loop (f3d.md §3.1, "Profile with circular
/// holes").
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::writer::generate::design) struct ProfileLoop {
    pub(in crate::writer::generate::design) exterior: bool,
    pub(in crate::writer::generate::design) entries: Vec<LoopEntry>,
}

const NO_CURVES: &str = "its profile has no curves";
const NOT_CLOSED: &str = "its profile is not closed";
const NO_AREA: &str = "its profile encloses no area";
const SEVERAL_REGIONS: &str = "its profile has several regions";
const LOOP_IN_HOLE: &str = "its profile has a loop inside a hole";
const ARC_ENDS_MEET: &str = "an arc of its profile has coincident ends";
const HOLES_MEET: &str = "holes of its profile meet";
const LOOPS_MEET: &str = "loops of its profile meet";
const NO_POINT: &str = "its profile names a point its sketch lacks";

type Point = [f64; 2];

/// A circular arc counterclockwise from `angles[0]` to `angles[1]`, at most
/// one turn; a whole circle spans one turn.
#[derive(Clone, Copy, Debug)]
struct Arc {
    center: Point,
    radius: f64,
    angles: [f64; 2],
}

impl Arc {
    fn sweep(self) -> f64 {
        self.angles[1] - self.angles[0]
    }

    /// Whether the ray from the centre towards `at` meets the arc.
    fn spans(self, at: Point) -> bool {
        let sweep = self.sweep();
        sweep >= TAU || (direction(self.center, at) - self.angles[0]).rem_euclid(TAU) <= sweep
    }

    /// The distance of `at` from the arc.
    fn gap_to(self, at: Point) -> f64 {
        if self.spans(at) {
            (distance(self.center, at) - self.radius).abs()
        } else {
            distance(self.point(self.angles[0]), at).min(distance(self.point(self.angles[1]), at))
        }
    }

    fn point(self, angle: f64) -> Point {
        let (sin, cos) = angle.portable_sin_cos();
        [
            self.center[0] + self.radius * cos,
            self.center[1] + self.radius * sin,
        ]
    }
}

/// A piece of a loop boundary in run order.
#[derive(Clone, Copy, Debug)]
enum Segment {
    Line {
        from: Point,
        to: Point,
    },
    /// `arc` run from `from` to `to`: counterclockwise when `forward`.
    Arc {
        arc: Arc,
        from: Point,
        to: Point,
        forward: bool,
    },
}

impl Segment {
    fn ends(self) -> (Point, Point) {
        match self {
            Self::Line { from, to } | Self::Arc { from, to, .. } => (from, to),
        }
    }

    fn reversed(self) -> Self {
        match self {
            Self::Line { from, to } => Self::Line { from: to, to: from },
            Self::Arc {
                arc,
                from,
                to,
                forward,
            } => Self::Arc {
                arc,
                from: to,
                to: from,
                forward: !forward,
            },
        }
    }

    /// The segment's term of twice the signed area of its loop: the chord's
    /// cross product, and an arc's circular segment on the side it bulges.
    fn area(self) -> f64 {
        let (from, to) = self.ends();
        let chord = from[0] * to[1] - to[0] * from[1];
        match self {
            Self::Line { .. } => chord,
            Self::Arc { arc, forward, .. } => {
                let sweep = arc.sweep();
                let bulge = arc.radius * arc.radius * (sweep - sweep.portable_sin());
                if forward {
                    chord + bulge
                } else {
                    chord - bulge
                }
            }
        }
    }

    /// The signed angle the segment turns through about `at`, which lies
    /// off it.
    fn angle_about(self, at: Point) -> f64 {
        let (from, to) = self.ends();
        let chord = {
            let (a, b) = (
                [from[0] - at[0], from[1] - at[1]],
                [to[0] - at[0], to[1] - at[1]],
            );
            (a[0] * b[1] - a[1] * b[0]).portable_atan2(a[0] * b[0] + a[1] * b[1])
        };
        match self {
            Self::Arc { arc, forward, .. } if distance(arc.center, at) < arc.radius => {
                // From inside its circle an arc turns monotonically, through
                // less than a turn unless it is the whole circle.
                let turned = if arc.sweep() >= TAU {
                    TAU
                } else {
                    let (start, end) = if forward { (from, to) } else { (to, from) };
                    (direction(at, end) - direction(at, start)).rem_euclid(TAU)
                };
                if forward {
                    turned
                } else {
                    -turned
                }
            }
            Self::Line { .. } | Self::Arc { .. } => chord,
        }
    }

    /// The least distance between the two segments; `0` where they cross.
    fn gap(self, other: Self) -> f64 {
        match (self, other) {
            (Self::Line { from: a, to: b }, Self::Line { from: c, to: d }) => {
                if crosses(a, b, c, d) {
                    0.0
                } else {
                    segment_distance(a, c, d)
                        .min(segment_distance(b, c, d))
                        .min(segment_distance(c, a, b))
                        .min(segment_distance(d, a, b))
                }
            }
            (Self::Line { from, to }, Self::Arc { arc, .. })
            | (Self::Arc { arc, .. }, Self::Line { from, to }) => line_arc_gap(from, to, arc),
            (Self::Arc { arc: a, .. }, Self::Arc { arc: b, .. }) => arc_arc_gap(a, b),
        }
    }
}

/// The least distance between the segment `a`-`b` and `arc`.
fn line_arc_gap(a: Point, b: Point, arc: Arc) -> f64 {
    let ends = [arc.point(arc.angles[0]), arc.point(arc.angles[1])];
    let mut gap = arc
        .gap_to(a)
        .min(arc.gap_to(b))
        .min(segment_distance(ends[0], a, b))
        .min(segment_distance(ends[1], a, b));
    let along = [b[0] - a[0], b[1] - a[1]];
    let length = along[0] * along[0] + along[1] * along[1];
    if length > 0.0 {
        let offset = [a[0] - arc.center[0], a[1] - arc.center[1]];
        let half = offset[0] * along[0] + offset[1] * along[1];
        let foot = (-half / length).clamp(0.0, 1.0);
        let nearest = [a[0] + foot * along[0], a[1] + foot * along[1]];
        if arc.spans(nearest) {
            gap = gap.min((distance(arc.center, nearest) - arc.radius).abs());
        }
        let constant = offset[0] * offset[0] + offset[1] * offset[1] - arc.radius * arc.radius;
        let discriminant = half * half - length * constant;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            for t in [(-half - root) / length, (-half + root) / length] {
                if (0.0..=1.0).contains(&t) && arc.spans([a[0] + t * along[0], a[1] + t * along[1]])
                {
                    return 0.0;
                }
            }
        }
    }
    gap
}

/// The least distance between two arcs.
fn arc_arc_gap(a: Arc, b: Arc) -> f64 {
    let mut gap = [a.angles[0], a.angles[1]]
        .map(|angle| b.gap_to(a.point(angle)))
        .into_iter()
        .chain([b.angles[0], b.angles[1]].map(|angle| a.gap_to(b.point(angle))))
        .fold(f64::INFINITY, f64::min);
    let apart = distance(a.center, b.center);
    if apart == 0.0 {
        return gap;
    }
    let unit = [
        (b.center[0] - a.center[0]) / apart,
        (b.center[1] - a.center[1]) / apart,
    ];
    // Away from the arcs' ends, two points are nearest on the line of the
    // centres; measure along it.
    for (sa, sb) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let toward = |arc: Arc, sense: f64| {
            arc.spans([
                arc.center[0] + sense * unit[0],
                arc.center[1] + sense * unit[1],
            ])
        };
        if toward(a, sa) && toward(b, sb) {
            gap = gap.min((apart + sb * b.radius - sa * a.radius).abs());
        }
    }
    if apart <= a.radius + b.radius && apart >= (a.radius - b.radius).abs() {
        let along = (apart * apart + a.radius * a.radius - b.radius * b.radius) / (2.0 * apart);
        let across = (a.radius * a.radius - along * along).max(0.0).sqrt();
        for side in [1.0, -1.0] {
            let at = [
                a.center[0] + along * unit[0] - side * across * unit[1],
                a.center[1] + along * unit[1] + side * across * unit[0],
            ];
            if a.spans(at) && b.spans(at) {
                return 0.0;
            }
        }
    }
    gap
}

/// The closed boundary of a loop in sketch coordinates, in run order.
struct Boundary(Vec<Segment>);

impl Boundary {
    /// Twice the signed area: positive for a counterclockwise ring.
    fn area(&self) -> f64 {
        self.0.iter().map(|segment| segment.area()).sum()
    }

    fn reverse(&mut self) {
        self.0.reverse();
        for segment in &mut self.0 {
            *segment = segment.reversed();
        }
    }

    /// The least distance between the two boundaries; `0` where they cross.
    fn gap(&self, other: &Self) -> f64 {
        self.0
            .iter()
            .flat_map(|a| other.0.iter().map(move |b| a.gap(*b)))
            .fold(f64::INFINITY, f64::min)
    }

    /// Whether `at`, which lies off the boundary, is inside it.
    fn encloses(&self, at: Point) -> bool {
        let turned = self
            .0
            .iter()
            .map(|segment| segment.angle_about(at))
            .sum::<f64>();
        turned.abs() > std::f64::consts::PI
    }

    /// Whether `other` lies inside this boundary.
    fn holds(&self, other: &Self) -> bool {
        self.gap(other) > 0.0
            && other
                .0
                .first()
                .is_some_and(|first| self.encloses(first.ends().0))
    }
}

/// Whether the segments `a`-`b` and `c`-`d` cross at a point inside both.
fn crosses(a: Point, b: Point, c: Point, d: Point) -> bool {
    let side = |p: Point, q: Point, r: Point| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
}

fn distance(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).portable_hypot(a[1] - b[1])
}

/// The angle of `to` about `from`.
fn direction(from: Point, to: Point) -> f64 {
    (to[1] - from[1]).portable_atan2(to[0] - from[0])
}

/// The distance of `at` from the segment `a`-`b`.
fn segment_distance(at: Point, a: Point, b: Point) -> f64 {
    let along = [b[0] - a[0], b[1] - a[1]];
    let length = along[0] * along[0] + along[1] * along[1];
    let t = if length > 0.0 {
        (((at[0] - a[0]) * along[0] + (at[1] - a[1]) * along[1]) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    distance(at, [a[0] + t * along[0], a[1] + t * along[1]])
}

/// A sketch line or arc of the profile: its curve and its boundary piece,
/// run from its start point to its end point.
struct Piece {
    curve: usize,
    segment: Segment,
}

/// A closed loop of the profile: its entries in run order and its boundary.
struct Ring {
    entries: Vec<LoopEntry>,
    boundary: Boundary,
}

impl Ring {
    /// Turns the ring to run the other way.
    fn reverse(&mut self) {
        self.entries.reverse();
        for entry in &mut self.entries {
            entry.reversed = !entry.reversed;
        }
        self.boundary.reverse();
    }

    fn first_curve(&self) -> Option<usize> {
        self.entries.iter().map(|entry| entry.curve).min()
    }

    /// The entries of the counterclockwise exterior loop or the clockwise
    /// hole loop, from the curve of the lowest side token of the loop's
    /// range ("Walk of a loop of lines"). `C` is the loop curve of the
    /// largest record index, the curve written last; a loop that holds an
    /// arc takes the same rule, which f3d-open-items.md DR-883 leaves open.
    fn walk(mut self, hole: bool) -> Vec<LoopEntry> {
        let count = self.entries.len();
        let Some((position, last)) = self
            .entries
            .iter()
            .enumerate()
            .max_by_key(|(_, entry)| entry.curve)
            .map(|(position, entry)| (position, *entry))
        else {
            return self.entries;
        };
        let first = match (hole, last.reversed) {
            (false, false) | (true, true) => position,
            (false, true) => (position + 1) % count,
            (true, false) => (position + count - 1) % count,
        };
        self.entries.rotate_left(first);
        self.entries
    }
}

/// The loops of the one region that `sketch`'s non-construction curves
/// bound, with endpoints joined within `tolerance` cm. No two loops cross
/// or touch within `tolerance`. The exterior loop comes first, counter-
/// clockwise from the curve of side token `3`; one circle is one entry.
/// Each hole runs clockwise from the curve of its lowest token, a hole
/// circle against its direction, and the holes follow in the order of the
/// first curve of each.
pub(in crate::writer::generate::design) fn profile(
    sketch: &PlannedSketch<'_>,
    tolerance: f64,
) -> Result<Vec<ProfileLoop>, &'static str> {
    profile_of(sketch, tolerance, |_| true)
}

/// [`profile`] of the non-construction curves of `sketch` that `walked`
/// admits.
pub(in crate::writer::generate::design) fn profile_of(
    sketch: &PlannedSketch<'_>,
    tolerance: f64,
    walked: impl Fn(&PlannedCurve) -> bool,
) -> Result<Vec<ProfileLoop>, &'static str> {
    let point = |index: usize| sketch.points.get(index).copied().ok_or(NO_POINT);
    let mut rings = Vec::new();
    let mut pieces = Vec::new();
    for (curve, planned) in sketch
        .curves
        .iter()
        .enumerate()
        .filter(|(_, curve)| !curve.construction && walked(curve))
    {
        match planned.geometry {
            CurveGeometry::Line { start, end } => pieces.push(Piece {
                curve,
                segment: Segment::Line {
                    from: point(start)?,
                    to: point(end)?,
                },
            }),
            CurveGeometry::Arc {
                center,
                start,
                end,
                radius,
                angles,
            } => {
                let (from, to) = (point(start)?, point(end)?);
                if distance(from, to) <= tolerance {
                    return Err(ARC_ENDS_MEET);
                }
                pieces.push(Piece {
                    curve,
                    segment: Segment::Arc {
                        arc: Arc {
                            center: point(center)?,
                            radius,
                            angles,
                        },
                        from,
                        to,
                        forward: true,
                    },
                });
            }
            CurveGeometry::Circle { center, radius } => {
                let arc = Arc {
                    center: point(center)?,
                    radius,
                    angles: [0.0, TAU],
                };
                let at = arc.point(0.0);
                rings.push(Ring {
                    entries: vec![LoopEntry {
                        curve,
                        reversed: false,
                    }],
                    boundary: Boundary(vec![Segment::Arc {
                        arc,
                        from: at,
                        to: at,
                        forward: true,
                    }]),
                });
            }
        }
    }
    if rings.is_empty() && pieces.is_empty() {
        return Err(NO_CURVES);
    }
    rings.extend(chains(&pieces, tolerance)?);
    let depths = rings
        .iter()
        .enumerate()
        .map(|(at, ring)| {
            rings
                .iter()
                .enumerate()
                .filter(|(other, holder)| *other != at && holder.boundary.holds(&ring.boundary))
                .count()
        })
        .collect::<Vec<_>>();
    for (at, ring) in rings.iter().enumerate() {
        for (other, beside) in rings.iter().enumerate().skip(at + 1) {
            if ring.boundary.gap(&beside.boundary) <= tolerance {
                return Err(if depths[at] == 1 && depths[other] == 1 {
                    HOLES_MEET
                } else {
                    LOOPS_MEET
                });
            }
        }
    }
    let mut exterior = None;
    let mut holes = Vec::new();
    for (ring, depth) in rings.into_iter().zip(depths) {
        match depth {
            0 if exterior.is_none() => exterior = Some(ring),
            0 => return Err(SEVERAL_REGIONS),
            1 => holes.push(ring),
            _ => return Err(LOOP_IN_HOLE),
        }
    }
    let exterior = exterior.ok_or(SEVERAL_REGIONS)?;
    holes.sort_by_key(Ring::first_curve);
    Ok(std::iter::once(ProfileLoop {
        exterior: true,
        entries: exterior.walk(false),
    })
    .chain(holes.into_iter().map(|mut hole| {
        hole.reverse();
        ProfileLoop {
            exterior: false,
            entries: hole.walk(true),
        }
    }))
    .collect())
}

/// The closed loops of `pieces`, each counterclockwise.
fn chains(pieces: &[Piece], tolerance: f64) -> Result<Vec<Ring>, &'static str> {
    let near = |a: Point, b: Point| distance(a, b) <= tolerance;
    // Each joint is the end of exactly two pieces.
    for (position, piece) in pieces.iter().enumerate() {
        let (start, end) = piece.segment.ends();
        for at in [start, end] {
            let meeting = pieces
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != position)
                .map(|(_, other)| {
                    let (s, e) = other.segment.ends();
                    usize::from(near(s, at)) + usize::from(near(e, at))
                })
                .sum::<usize>();
            if meeting != 1 {
                return Err(NOT_CLOSED);
            }
        }
    }
    let mut used = vec![false; pieces.len()];
    let mut rings = Vec::new();
    while let Some(first) = used.iter().position(|used| !used) {
        used[first] = true;
        let mut entries = vec![LoopEntry {
            curve: pieces[first].curve,
            reversed: false,
        }];
        let mut segments = vec![pieces[first].segment];
        let (start, mut at) = pieces[first].segment.ends();
        while !near(at, start) {
            let (index, reversed) = pieces
                .iter()
                .enumerate()
                .filter(|(index, _)| !used[*index])
                .find_map(|(index, piece)| {
                    let (s, e) = piece.segment.ends();
                    if near(s, at) {
                        Some((index, false))
                    } else if near(e, at) {
                        Some((index, true))
                    } else {
                        None
                    }
                })
                .ok_or(NOT_CLOSED)?;
            used[index] = true;
            let segment = if reversed {
                pieces[index].segment.reversed()
            } else {
                pieces[index].segment
            };
            entries.push(LoopEntry {
                curve: pieces[index].curve,
                reversed,
            });
            segments.push(segment);
            at = segment.ends().1;
        }
        let mut ring = Ring {
            entries,
            boundary: Boundary(segments),
        };
        let area = ring.boundary.area();
        if area == 0.0 {
            return Err(NO_AREA);
        }
        if area < 0.0 {
            ring.reverse();
        }
        rings.push(ring);
    }
    Ok(rings)
}
