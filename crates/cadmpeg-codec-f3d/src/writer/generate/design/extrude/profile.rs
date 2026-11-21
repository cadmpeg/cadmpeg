// SPDX-License-Identifier: Apache-2.0
//! Row 5 of the rule: the loops of the region that a sketch's curves bound,
//! in the order the profile records store them (f3d.md §3.1, "Side-face
//! walk", "Walk of a loop of lines" and "Profile with circular holes").

use cadmpeg_ir::math::portable::Portable;

use super::super::sketch::{CurveGeometry, PlannedSketch};

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
const ARC_IN_LOOP: &str = "a loop of its profile has an arc";
const HOLE_NOT_CIRCLE: &str = "a hole of its profile is not one circle";

/// A closed loop of the profile in sketch coordinates.
enum Outline {
    Circle { center: [f64; 2], radius: f64 },
    Polygon(Vec<[f64; 2]>),
}

impl Outline {
    /// Whether `other` lies inside this outline.
    fn holds(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Circle { center, radius },
                Self::Circle {
                    center: inner,
                    radius: r,
                },
            ) => distance(*center, *inner) + r < *radius,
            (Self::Circle { center, radius }, Self::Polygon(corners)) => corners
                .iter()
                .all(|corner| distance(*center, *corner) < *radius),
            (Self::Polygon(corners), Self::Circle { center, radius }) => {
                encloses(corners, *center)
                    && corners
                        .iter()
                        .zip(corners.iter().cycle().skip(1))
                        .all(|(a, b)| segment_distance(*center, *a, *b) > *radius)
            }
            (Self::Polygon(corners), Self::Polygon(inner)) => {
                inner.iter().all(|corner| encloses(corners, *corner))
            }
        }
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).portable_hypot(a[1] - b[1])
}

/// The distance of `at` from the segment `a`-`b`.
fn segment_distance(at: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let along = [b[0] - a[0], b[1] - a[1]];
    let length = along[0] * along[0] + along[1] * along[1];
    let t = if length > 0.0 {
        (((at[0] - a[0]) * along[0] + (at[1] - a[1]) * along[1]) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    distance(at, [a[0] + t * along[0], a[1] + t * along[1]])
}

/// Whether `at` is inside the polygon `corners` (even-odd rule).
fn encloses(corners: &[[f64; 2]], at: [f64; 2]) -> bool {
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .filter(|(a, b)| {
            (a[1] > at[1]) != (b[1] > at[1])
                && at[0] < a[0] + (at[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        })
        .count()
        % 2
        == 1
}

/// The loops of the one region that `sketch`'s non-construction curves
/// bound, with endpoints joined within `tolerance` cm. The exterior loop
/// comes first, in ring order from the curve of side token `3`; one circle
/// is one entry. Each hole is one circle, run against its direction, and
/// the holes follow in the order of their curves.
pub(in crate::writer::generate::design) fn profile(
    sketch: &PlannedSketch<'_>,
    tolerance: f64,
) -> Result<Vec<ProfileLoop>, &'static str> {
    let point = |index: usize| sketch.points.get(index).copied().ok_or(NOT_CLOSED);
    let mut loops = Vec::new();
    let mut lines = Vec::new();
    for (curve, planned) in sketch
        .curves
        .iter()
        .enumerate()
        .filter(|(_, curve)| !curve.construction)
    {
        match planned.geometry {
            CurveGeometry::Line { start, end } => lines.push((curve, point(start)?, point(end)?)),
            CurveGeometry::Circle { center, radius } => loops.push((
                vec![LoopEntry {
                    curve,
                    reversed: false,
                }],
                Outline::Circle {
                    center: point(center)?,
                    radius,
                },
            )),
            CurveGeometry::Arc { .. } => return Err(ARC_IN_LOOP),
        }
    }
    if loops.is_empty() && lines.is_empty() {
        return Err(NO_CURVES);
    }
    loops.extend(line_loops(&lines, tolerance)?);
    let depths = loops
        .iter()
        .enumerate()
        .map(|(at, (_, outline))| {
            loops
                .iter()
                .enumerate()
                .filter(|(other, (_, holder))| *other != at && holder.holds(outline))
                .count()
        })
        .collect::<Vec<_>>();
    let mut exterior = None;
    let mut holes = Vec::new();
    for ((entries, outline), depth) in loops.into_iter().zip(depths) {
        match depth {
            0 if exterior.is_none() => exterior = Some(entries),
            0 => return Err(SEVERAL_REGIONS),
            1 => holes.push((entries, outline)),
            _ => return Err(LOOP_IN_HOLE),
        }
    }
    let exterior = exterior.ok_or(SEVERAL_REGIONS)?;
    let mut holes = holes
        .into_iter()
        .map(|(entries, outline)| match (entries.as_slice(), outline) {
            ([circle], Outline::Circle { .. }) => Ok(LoopEntry {
                curve: circle.curve,
                reversed: true,
            }),
            _ => Err(HOLE_NOT_CIRCLE),
        })
        .collect::<Result<Vec<_>, _>>()?;
    holes.sort_unstable_by_key(|hole| hole.curve);
    Ok(std::iter::once(ProfileLoop {
        exterior: true,
        entries: exterior,
    })
    .chain(holes.into_iter().map(|hole| ProfileLoop {
        exterior: false,
        entries: vec![hole],
    }))
    .collect())
}

/// The closed loops of `lines` (curve, start, end), each in ring order
/// from the curve of side token `3`.
fn line_loops(
    lines: &[(usize, [f64; 2], [f64; 2])],
    tolerance: f64,
) -> Result<Vec<(Vec<LoopEntry>, Outline)>, &'static str> {
    let near = |a: [f64; 2], b: [f64; 2]| distance(a, b) <= tolerance;
    // Each joint is the end of exactly two lines.
    for (position, (_, start, end)) in lines.iter().enumerate() {
        for at in [*start, *end] {
            let meeting = lines
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != position)
                .map(|(_, (_, s, e))| usize::from(near(*s, at)) + usize::from(near(*e, at)))
                .sum::<usize>();
            if meeting != 1 {
                return Err(NOT_CLOSED);
            }
        }
    }
    let mut used = vec![false; lines.len()];
    let mut loops = Vec::new();
    while let Some(first) = used.iter().position(|used| !used) {
        used[first] = true;
        let mut entries = vec![LoopEntry {
            curve: lines[first].0,
            reversed: false,
        }];
        let mut corners = vec![lines[first].1];
        let mut at = lines[first].2;
        while !near(at, corners[0]) {
            let (index, reversed, start, end) = lines
                .iter()
                .enumerate()
                .filter(|(index, _)| !used[*index])
                .find_map(|(index, (_, s, e))| {
                    if near(*s, at) {
                        Some((index, false, *s, *e))
                    } else if near(*e, at) {
                        Some((index, true, *e, *s))
                    } else {
                        None
                    }
                })
                .ok_or(NOT_CLOSED)?;
            used[index] = true;
            entries.push(LoopEntry {
                curve: lines[index].0,
                reversed,
            });
            corners.push(start);
            at = end;
        }
        loops.push((line_ring(entries, &corners)?, Outline::Polygon(corners)));
    }
    Ok(loops)
}

/// The closed chain `entries` with its `corners`, turned counterclockwise
/// and rotated to start at the curve of side token `3`.
fn line_ring(
    mut entries: Vec<LoopEntry>,
    corners: &[[f64; 2]],
) -> Result<Vec<LoopEntry>, &'static str> {
    // Twice the signed area: positive for a counterclockwise ring.
    let area = corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>();
    if area == 0.0 {
        return Err(NO_AREA);
    }
    if area < 0.0 {
        entries.reverse();
        for entry in &mut entries {
            entry.reversed = !entry.reversed;
        }
    }
    // `C`, the line of the largest record index, is the curve written last.
    let last = entries
        .iter()
        .enumerate()
        .max_by_key(|(_, entry)| entry.curve)
        .map(|(position, entry)| (position, entry.reversed))
        .ok_or(NOT_CLOSED)?;
    let first = match last {
        (position, false) => position,
        (position, true) => (position + 1) % entries.len(),
    };
    entries.rotate_left(first);
    Ok(entries)
}
