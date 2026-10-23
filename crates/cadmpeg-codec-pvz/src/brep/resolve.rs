// SPDX-License-Identifier: Apache-2.0
//! Derived topology of §8.17 to §8.20: coedge sense, loop traversal, face
//! regions, vertices, and shells.

use std::f64::consts::TAU;

use super::eval::{bspline_surface_point, curve_ends, curve_interval, curve_point, distance};
use super::model::{CurveGeometry, ExactModel, Face, SurfaceGeometry, Vec2, Vec3};

/// Endpoint agreement of §8.21, in metres.
pub(crate) const ENDPOINT_TOLERANCE: f64 = 2.0e-5;
/// Interior fraction of a closed edge sampled to decide its sense.
const CLOSED_EDGE_PROBE: f64 = 0.25;
/// Pcurve samples per coedge in a loop polygon.
const POLYGON_SAMPLES: usize = 16;

/// Parameter domain a coedge pcurve lies in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PcurveSlot {
    /// Native pcurve in the native domain of the face surface.
    Native,
    /// Secondary pcurve in the domain of the embedded B-spline.
    Secondary,
}

/// Coedge sense relative to its edge curve (§8.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sense {
    /// The pcurve start maps to the edge start.
    Forward,
    /// The pcurve start maps to the edge end.
    Reversed,
}

/// Geometry of one coedge, resolved against its face.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CoedgeGeometry {
    /// Pcurve used for the coedge and its domain.
    pub(crate) pcurve: Option<(usize, PcurveSlot)>,
    /// Pcurve parameter interval, start to end.
    pub(crate) interval: Option<Vec2>,
    /// Native or embedded uv of the pcurve start and end.
    pub(crate) uv_ends: Option<[Vec2; 2]>,
    /// Model-space start and end of the coedge in pcurve direction.
    pub(crate) ends: Option<[Vec3; 2]>,
    /// Model-space start and end of the edge curve.
    pub(crate) edge_ends: Option<[Vec3; 2]>,
    /// Sense against the edge curve, when both curves evaluate.
    pub(crate) sense: Option<Sense>,
    /// Larger endpoint distance of the chosen sense, in metres.
    pub(crate) residual: Option<f64>,
}

/// Pcurve domain used for every coedge of a face: the native domain when
/// each coedge has a native pcurve, otherwise the embedded B-spline domain
/// when each coedge has a secondary pcurve.
pub(crate) fn face_slot(model: &ExactModel, face: usize) -> Option<PcurveSlot> {
    let record = model.faces.get(face)?;
    let coedges = || {
        record
            .loops
            .iter()
            .filter_map(|index| model.loops.get(*index))
            .flat_map(|record| record.coedges.iter())
    };
    let native = !matches!(record.surface.geometry, SurfaceGeometry::Undefined { .. })
        && coedges().all(|coedge| coedge.pcurve.is_some());
    let secondary =
        record.surface.embedded.is_some() && coedges().all(|coedge| coedge.secondary.is_some());
    if native {
        Some(PcurveSlot::Native)
    } else if secondary {
        Some(PcurveSlot::Secondary)
    } else {
        None
    }
}

/// Point of the domain surface of `slot` on `face`.
pub(crate) fn domain_point(face: &Face, slot: PcurveSlot, uv: Vec2) -> Option<Vec3> {
    match slot {
        PcurveSlot::Native => super::eval::surface_point(&face.surface.geometry, uv),
        PcurveSlot::Secondary => face
            .surface
            .embedded
            .as_ref()
            .and_then(|embedded| bspline_surface_point(embedded, uv)),
    }
}

/// Resolves the pcurve, ends, and sense of every coedge of `loop_index` in
/// the pcurve domain `slot`.
pub(crate) fn coedge_geometry(
    model: &ExactModel,
    loop_index: usize,
    slot: Option<PcurveSlot>,
) -> Vec<CoedgeGeometry> {
    let Some(record) = model.loops.get(loop_index) else {
        return Vec::new();
    };
    let Some(face) = model.faces.get(record.face) else {
        return Vec::new();
    };
    record
        .coedges
        .iter()
        .map(|coedge| {
            let choice = slot.and_then(|slot| {
                let index = match slot {
                    PcurveSlot::Native => coedge.pcurve,
                    PcurveSlot::Secondary => coedge.secondary,
                }?;
                Some((index, slot))
            });
            let surface_at = |uv: Vec2| domain_point(face, choice?.1, uv);
            let pcurve = choice.and_then(|(index, _)| model.pcurves.get(index));
            let interval = pcurve.and_then(|pcurve| curve_interval(&pcurve.geometry));
            let uv_ends = pcurve.and_then(|pcurve| curve_ends(&pcurve.geometry));
            let ends =
                uv_ends.and_then(|[start, end]| Some([surface_at(start)?, surface_at(end)?]));
            let edge = coedge.edge.and_then(|edge| model.edges.get(edge));
            let edge_ends = edge.and_then(|edge| curve_ends(&edge.geometry));
            let (sense, residual) = match (ends, edge_ends, edge) {
                (Some(ends), Some(edge_ends), Some(edge)) => {
                    let probe = pcurve.zip(interval).and_then(|(pcurve, [start, end])| {
                        let t = start + (end - start) * CLOSED_EDGE_PROBE;
                        surface_at(curve_point(&pcurve.geometry, t)?)
                    });
                    let (sense, residual) = sense_from_ends(ends, edge_ends, probe, &edge.geometry);
                    (Some(sense), Some(residual))
                }
                _ => (None, None),
            };
            CoedgeGeometry {
                pcurve: choice,
                interval,
                uv_ends,
                ends,
                edge_ends,
                sense,
                residual,
            }
        })
        .collect()
}

/// Sense from the endpoint test of §8.17. When both senses agree within
/// tolerance the edge is closed, and the interior probe point decides.
fn sense_from_ends(
    ends: [Vec3; 2],
    edge_ends: [Vec3; 2],
    probe: Option<Vec3>,
    edge: &CurveGeometry<3>,
) -> (Sense, f64) {
    let [start, end] = ends;
    let [edge_start, edge_end] = edge_ends;
    let forward = distance(start, edge_start).max(distance(end, edge_end));
    let reversed = distance(start, edge_end).max(distance(end, edge_start));
    if forward <= ENDPOINT_TOLERANCE && reversed <= ENDPOINT_TOLERANCE {
        let interior = probe
            .zip(curve_interval(edge))
            .and_then(|(probe, [low, high])| {
                let early = curve_point(edge, low + (high - low) * CLOSED_EDGE_PROBE)?;
                let late = curve_point(edge, high - (high - low) * CLOSED_EDGE_PROBE)?;
                Some(distance(probe, early) <= distance(probe, late))
            });
        return match interior {
            Some(false) => (Sense::Reversed, reversed),
            _ => (Sense::Forward, forward),
        };
    }
    if forward <= reversed {
        (Sense::Forward, forward)
    } else {
        (Sense::Reversed, reversed)
    }
}

/// Direction of each coedge pcurve along the stored coedge order of a loop
/// (§8.18): `true` when the pcurve runs from the end it shares with the
/// previous coedge to the end it shares with the next. A coedge whose ends do
/// not decide the direction takes it from its `orientation` byte and its
/// sense: the byte is 1 when the edge curve runs along the stored order.
pub(crate) fn pcurve_directions(orientations: &[u8], coedges: &[CoedgeGeometry]) -> Vec<bool> {
    let count = coedges.len();
    let near = |point: Vec3, other: Option<[Vec3; 2]>| {
        other.is_some_and(|[start, end]| {
            distance(point, start).min(distance(point, end)) <= ENDPOINT_TOLERANCE
        })
    };
    (0..count)
        .map(|position| {
            let coedge = &coedges[position];
            let by_byte = orientations
                .get(position)
                .is_none_or(|byte| (*byte == 1) == (coedge.sense != Some(Sense::Reversed)));
            let Some([start, end]) = coedge.ends else {
                return by_byte;
            };
            if count < 3 {
                return by_byte;
            }
            let next = coedges[(position + 1) % count].ends;
            let previous = coedges[(position + count - 1) % count].ends;
            match (
                near(end, next) && near(start, previous),
                near(start, next) && near(end, previous),
            ) {
                (true, false) => true,
                (false, true) => false,
                _ => by_byte,
            }
        })
        .collect()
}

/// Largest distance between consecutive coedge ends along the stored order,
/// with each pcurve taken in its direction from [`pcurve_directions`].
pub(crate) fn closure_gap(coedges: &[CoedgeGeometry], directions: &[bool]) -> Option<f64> {
    let count = coedges.len();
    let oriented = |position: usize| -> Option<[Vec3; 2]> {
        let [start, end] = coedges.get(position)?.ends?;
        Some(if *directions.get(position)? {
            [start, end]
        } else {
            [end, start]
        })
    };
    let mut worst: f64 = 0.0;
    for position in 0..count {
        let [_, end] = oriented(position)?;
        let [start, _] = oriented((position + 1) % count)?;
        worst = worst.max(distance(end, start));
    }
    Some(worst)
}

/// A loop sampled in its pcurve domain, in stored order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopPolygon {
    /// Samples; each pcurve after the first is shifted by whole periods so
    /// that it starts at the previous end.
    pub(crate) points: Vec<Vec2>,
    /// Offset from the first sample to the end of the last pcurve: zero for
    /// a loop that closes in the domain, a whole period for a loop that wraps
    /// around a periodic direction.
    pub(crate) wrap: Vec2,
}

impl LoopPolygon {
    /// Whether the loop closes in the domain without wrapping.
    pub(crate) fn closes(&self, periods: [Option<f64>; 2]) -> bool {
        (0..2).all(|axis| periods[axis].is_none_or(|period| self.wrap[axis].abs() < period / 2.0))
    }
}

/// Samples a loop in stored order from its pcurves.
pub(crate) fn loop_polygon(
    model: &ExactModel,
    coedges: &[CoedgeGeometry],
    directions: &[bool],
    periods: [Option<f64>; 2],
) -> Option<LoopPolygon> {
    let mut points: Vec<Vec2> = Vec::new();
    let mut last = None;
    for (coedge, forward) in coedges.iter().zip(directions) {
        let (index, _) = coedge.pcurve?;
        let pcurve = &model.pcurves.get(index)?.geometry;
        let [low, high] = coedge.interval?;
        let [start, end] = if *forward { [low, high] } else { [high, low] };
        let first = curve_point(pcurve, start)?;
        let shift = last.map_or([0.0, 0.0], |previous| {
            period_shift(previous, first, periods)
        });
        for step in 0..POLYGON_SAMPLES {
            let fraction = cadmpeg_core::convert::f64_from_index(step)?
                / cadmpeg_core::convert::f64_from_index(POLYGON_SAMPLES)?;
            let point = curve_point(pcurve, start + (end - start) * fraction)?;
            points.push([point[0] + shift[0], point[1] + shift[1]]);
        }
        let end = curve_point(pcurve, end)?;
        last = Some([end[0] + shift[0], end[1] + shift[1]]);
    }
    let first = *points.first()?;
    let end = last?;
    Some(LoopPolygon {
        points,
        wrap: [end[0] - first[0], end[1] - first[1]],
    })
}

/// Whole-period shift that moves `point` nearest to `target`.
fn period_shift(target: Vec2, point: Vec2, periods: [Option<f64>; 2]) -> Vec2 {
    std::array::from_fn(|axis| {
        periods[axis].map_or(0.0, |period| {
            ((target[axis] - point[axis]) / period).round() * period
        })
    })
}

/// Signed area of a closed polygon; positive for counter-clockwise order.
pub(crate) fn signed_area(polygon: &[Vec2]) -> f64 {
    let count = polygon.len();
    (0..count)
        .map(|index| {
            let a = polygon[index];
            let b = polygon[(index + 1) % count];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

/// Periods of the pcurve domain `slot` of a surface (§8.19). The embedded
/// B-spline domain has no period.
pub(crate) fn domain_periods(surface: &SurfaceGeometry, slot: PcurveSlot) -> [Option<f64>; 2] {
    if slot == PcurveSlot::Secondary {
        return [None, None];
    }
    match surface {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => [Some(TAU), None],
        SurfaceGeometry::Torus(_) => [Some(TAU), Some(TAU)],
        SurfaceGeometry::Plane { .. }
        | SurfaceGeometry::Bspline(_)
        | SurfaceGeometry::Undefined { .. } => [None, None],
    }
}

/// Whether the edge curve of each coedge runs along the stored order: the
/// pcurve direction composed with the coedge sense, or the `orientation`
/// byte when the sense is not derived.
pub(crate) fn edges_along(
    orientations: &[u8],
    coedges: &[CoedgeGeometry],
    directions: &[bool],
) -> Vec<bool> {
    coedges
        .iter()
        .zip(directions)
        .zip(orientations)
        .map(|((coedge, forward), byte)| match coedge.sense {
            Some(sense) => *forward == (sense == Sense::Forward),
            None => *byte == 1,
        })
        .collect()
}

/// Loops of one face region (§8.19), as positions in the face's loop list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegionLoops {
    /// Member loops; the first is the outer loop when `classified`.
    pub(crate) loops: Vec<usize>,
    /// Whether the outer and hole loops were classified by nesting.
    pub(crate) classified: bool,
}

/// Splits the loops of one face into regions by loop group and nesting
/// parity. `groups[i]` is the loop group of loop `i`, `polygons[i]` its
/// domain polygon.
pub(crate) fn face_regions(
    groups: &[usize],
    polygons: &[Option<LoopPolygon>],
    periods: [Option<f64>; 2],
) -> Vec<RegionLoops> {
    let mut seen: Vec<usize> = Vec::new();
    for group in groups {
        if !seen.contains(group) {
            seen.push(*group);
        }
    }
    let mut regions = Vec::new();
    for group in seen {
        let members: Vec<usize> = (0..groups.len())
            .filter(|position| groups[*position] == group)
            .collect();
        let closed: Option<Vec<&LoopPolygon>> = members
            .iter()
            .map(|position| {
                polygons
                    .get(*position)?
                    .as_ref()
                    .filter(|polygon| polygon.closes(periods))
            })
            .collect();
        let Some(closed) = closed else {
            regions.push(RegionLoops {
                loops: members,
                classified: false,
            });
            continue;
        };
        let contains = |outer: usize, inner: usize| {
            outer != inner
                && closed[inner]
                    .points
                    .first()
                    .is_some_and(|point| encloses(&closed[outer].points, *point, periods))
        };
        let depth: Vec<usize> = (0..members.len())
            .map(|inner| {
                (0..members.len())
                    .filter(|outer| contains(*outer, inner))
                    .count()
            })
            .collect();
        let mut owned: Vec<RegionLoops> = Vec::new();
        let mut region_of = vec![None; members.len()];
        for (local, position) in members.iter().enumerate() {
            if depth[local].is_multiple_of(2) {
                region_of[local] = Some(owned.len());
                owned.push(RegionLoops {
                    loops: vec![*position],
                    classified: true,
                });
            }
        }
        for (local, position) in members.iter().enumerate() {
            if depth[local].is_multiple_of(2) {
                continue;
            }
            let parent = (0..members.len())
                .filter(|outer| contains(*outer, local) && depth[*outer] + 1 == depth[local])
                .find_map(|outer| region_of[outer]);
            match parent.and_then(|region| owned.get_mut(region)) {
                Some(region) => region.loops.push(*position),
                None => owned.push(RegionLoops {
                    loops: vec![*position],
                    classified: false,
                }),
            }
        }
        regions.extend(owned);
    }
    regions
}

/// Even-odd point-in-polygon test, with `point` moved by whole periods next
/// to the polygon.
pub(crate) fn encloses(polygon: &[Vec2], point: Vec2, periods: [Option<f64>; 2]) -> bool {
    let Some(first) = polygon.first() else {
        return false;
    };
    let [low, high] = polygon
        .iter()
        .fold([*first, *first], |[low, high], sample| {
            [
                [low[0].min(sample[0]), low[1].min(sample[1])],
                [high[0].max(sample[0]), high[1].max(sample[1])],
            ]
        });
    let centre = [low[0].midpoint(high[0]), low[1].midpoint(high[1])];
    let shift = period_shift(centre, point, periods);
    let [u, v] = [point[0] + shift[0], point[1] + shift[1]];
    let count = polygon.len();
    let mut inside = false;
    for index in 0..count {
        let a = polygon[index];
        let b = polygon[(index + 1) % count];
        if (a[1] > v) != (b[1] > v) {
            let crossing = a[0] + (v - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if u < crossing {
                inside = !inside;
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests;
