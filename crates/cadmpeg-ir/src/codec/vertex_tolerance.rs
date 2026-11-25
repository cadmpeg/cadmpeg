// SPDX-License-Identifier: Apache-2.0
//! Vertex tolerances widened to the positions part 10 compares a vertex with
//! (§2.3).
//!
//! A vertex the source stores farther from the end of an edge's curve, or of
//! a coedge's use curve, than the coincidence allowance there states its
//! `tolerance` as its largest such distance. Only records the decoder read
//! are measured: a record whose annotations mark it computed (any exactness
//! but byte-exact, other than its `tolerance`) is not. A vertex is left as
//! stored where a compared position lies more than twice as far from it as
//! the carrier itself does, or where that carrier distance cannot be
//! measured: there a range is off, not the vertex.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimit};

use crate::annotations::{AnnotationBuilder, AnnotationFieldError, Annotations};
use crate::eval::decode::curve_point;
use crate::eval::finite_or_refusal;
use crate::geometry::{Curve, SolvedCurveGeometry};
use crate::math::Point3;
use crate::provenance::Exactness;
use crate::scalar::PositiveLength;
use crate::topology::{Edge, IncreasingParameterInterval, Sense};
use crate::units::{binding_allowance, coincidence_allowance, tolerance_in_force};
use crate::CadIr;

/// A compared position farther from the vertex than this many times the
/// vertex's distance from the carrier is off on the carrier.
const CARRIER_DISTANCE_FACTOR: f64 = 2.0;

/// Parameters sampled across a carrier's domain to find its nearest point.
const CARRIER_SAMPLES: u32 = 64;

/// Golden-section steps that refine the nearest sampled parameter.
const CARRIER_REFINEMENTS: usize = 48;

/// The vertices whose tolerance the pass stated, and the largest increase
/// over the tolerance in force at each before.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Widened {
    pub(super) vertices: Vec<String>,
    pub(super) largest: f64,
}

impl Widened {
    /// The message of the `geometry.vertex-tolerance-widened` loss.
    pub(super) fn message(&self) -> String {
        format!(
            "widened {} vertex tolerance(s) to each vertex's largest distance from the curve ends \
             compared with it; the largest widening is {:.17} mm",
            self.vertices.len(),
            self.largest
        )
    }
}

/// The ends of a carrier compared with the vertices its edge runs between:
/// each vertex with the parameter it is compared at.
struct Comparison<'ir> {
    edge: &'ir Edge,
    curve: &'ir Curve,
    fit: Option<f64>,
    ends: [(&'ir str, f64); 2],
}

/// One compared position beyond the coincidence allowance.
struct Miss<'ir> {
    distance: f64,
    carrier: &'ir Curve,
}

/// Widens the tolerance of each vertex of `ir` that a compared curve end
/// misses, and marks that tolerance derived in `annotations`. The pass
/// measures in a session of its own under `policy`, so a ceiling it meets
/// refuses no part of the decode; it then widens nothing.
///
/// # Errors
///
/// A resource refusal while marking the widened tolerances.
pub(super) fn widen(
    ir: &mut CadIr,
    annotations: &mut Annotations,
    policy: &DecodePolicy,
) -> Result<Option<Widened>, AnnotationFieldError> {
    let arena = DecodeArena::new();
    let Ok((ctx, _)) = DecodeContext::from_root_bytes(&[], &arena, policy) else {
        return Ok(None);
    };
    let Ok(tolerances) = measure(&ctx, ir, annotations) else {
        return Ok(None);
    };
    let mut widened = Widened {
        vertices: Vec::new(),
        largest: 0.0,
    };
    for vertex in &mut ir.geometry.vertices {
        let Some(&tolerance) = tolerances.get(vertex.id.as_str()) else {
            continue;
        };
        let Some(stated) = PositiveLength::new(tolerance) else {
            continue;
        };
        let before = vertex
            .tolerance
            .map_or(ir.tolerances.linear.get(), PositiveLength::get);
        vertex.tolerance = Some(stated);
        widened.largest = widened.largest.max(tolerance - before);
        widened.vertices.push(vertex.id.as_str().to_owned());
    }
    if widened.vertices.is_empty() {
        return Ok(None);
    }
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let marked = widened
        .vertices
        .iter()
        .try_for_each(|id| builder.derived(id, "tolerance").map(|_| ()));
    *annotations = builder.build();
    marked?;
    Ok(Some(widened))
}

/// Whether the annotations mark the record computed rather than read.
fn computed(annotations: &Annotations, id: &str) -> bool {
    annotations.exactness().get(id).is_some_and(|note| {
        note.entity() != Exactness::ByteExact
            || note.fields().iter().any(|(field, exactness)| {
                field.as_str() != "tolerance" && *exactness != Exactness::ByteExact
            })
    })
}

/// The tolerance each vertex states after the pass, for the vertices it
/// widens.
fn measure(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    annotations: &Annotations,
) -> Result<BTreeMap<String, f64>, ResourceLimit> {
    let linear = ir.tolerances.linear;
    let curves: HashMap<&str, &Curve> = ir
        .geometry
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect();
    let constructions: HashMap<&str, _> = ir
        .geometry
        .procedural_curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect();
    let edges: HashMap<&str, &Edge> = ir
        .geometry
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect();
    let points: HashMap<&str, _> = ir
        .geometry
        .points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect();
    // Each vertex read from the source, with its position and stated tolerance.
    let vertices: HashMap<&str, (Point3, Option<f64>)> = ir
        .geometry
        .vertices
        .iter()
        .filter(|vertex| {
            !computed(annotations, vertex.id.as_str())
                && !computed(annotations, vertex.point.as_str())
        })
        .filter_map(|vertex| {
            let point = points.get(vertex.point.as_str())?;
            Some((
                vertex.id.as_str(),
                (
                    point.position().get(),
                    vertex.tolerance.map(PositiveLength::get),
                ),
            ))
        })
        .collect();
    // A carrier the decoder read, with the fit of the cache it is evaluated
    // through.
    let carrier = |id: &str| {
        let curve = *curves.get(id)?;
        if computed(annotations, id) {
            return None;
        }
        let construction = match curve.geometry.procedural_construction() {
            Some(construction) => {
                if computed(annotations, construction.as_str()) {
                    return None;
                }
                constructions.get(construction.as_str()).copied()
            }
            None => None,
        };
        let fit = construction
            .and_then(crate::geometry::ProceduralCurve::cache_fit_tolerance)
            .map(crate::geometry::legacy_cache::FitTolerance::get);
        Some((curve, fit))
    };
    // Each comparison of a carrier's ends with the vertices it runs between.
    let mut comparisons = Vec::new();
    for edge in &ir.geometry.edges {
        let (Some(curve), Some(range), Some(ends)) = (edge.curve(), edge.param_range(), &edge.ends)
        else {
            continue;
        };
        if computed(annotations, edge.id.as_str()) {
            continue;
        }
        let Some((curve, fit)) = carrier(curve.as_str()) else {
            continue;
        };
        let [start, end] = range.get();
        comparisons.push(Comparison {
            edge,
            curve,
            fit,
            ends: [(ends.start.as_str(), start), (ends.end.as_str(), end)],
        });
    }
    for coedge in &ir.geometry.coedges {
        let Some(use_curve) = &coedge.use_curve else {
            continue;
        };
        if computed(annotations, coedge.id.as_str()) {
            continue;
        }
        let Some(edge) = edges.get(coedge.edge.as_str()).copied() else {
            continue;
        };
        let Some(ends) = &edge.ends else {
            continue;
        };
        let Some((curve, fit)) = carrier(use_curve.curve.as_str()) else {
            continue;
        };
        let (first, last) = match coedge.sense {
            Sense::Forward => (&ends.start, &ends.end),
            Sense::Reversed => (&ends.end, &ends.start),
        };
        let [start, end] = use_curve.parameter_range.endpoints();
        comparisons.push(Comparison {
            edge,
            curve,
            fit,
            ends: [(first.as_str(), start), (last.as_str(), end)],
        });
    }
    let mut misses: BTreeMap<&str, Vec<Miss<'_>>> = BTreeMap::new();
    for Comparison {
        edge,
        curve,
        fit,
        ends,
    } in comparisons
    {
        let (Some(&(start, start_tolerance)), Some(&(end, end_tolerance))) =
            (vertices.get(ends[0].0), vertices.get(ends[1].0))
        else {
            continue;
        };
        let (Some(at_start), Some(at_end)) = (
            finite_or_refusal(curve_point(ctx, &curve.geometry, ends[0].1))?,
            finite_or_refusal(curve_point(ctx, &curve.geometry, ends[1].1))?,
        ) else {
            continue;
        };
        let (at_start, at_end) = (at_start.get(), at_end.get());
        let tolerances = [
            edge.tolerance.map(PositiveLength::get),
            start_tolerance,
            end_tolerance,
        ];
        let allowance = coincidence_allowance(
            tolerance_in_force(linear, &tolerances),
            &[fit],
            magnitude(&[at_start, at_end, start, end]),
        )
        .max(binding_allowance(
            linear,
            &[tolerances[0], tolerances[1], tolerances[2], fit],
        ));
        for ((vertex, _), (position, at)) in
            ends.into_iter().zip([(start, at_start), (end, at_end)])
        {
            let distance = Point3::distance(position, at);
            if distance.is_finite() && distance > allowance {
                misses.entry(vertex).or_default().push(Miss {
                    distance,
                    carrier: curve,
                });
            }
        }
    }
    let mut tolerances = BTreeMap::new();
    'vertices: for (vertex, misses) in misses {
        let Some(&(position, _)) = vertices.get(vertex) else {
            continue;
        };
        let mut largest = 0.0_f64;
        for miss in misses {
            let Some(carrier) = carrier_distance(ctx, miss.carrier, position)? else {
                continue 'vertices;
            };
            if miss.distance > CARRIER_DISTANCE_FACTOR * carrier {
                continue 'vertices;
            }
            largest = largest.max(miss.distance);
        }
        tolerances.insert(vertex.to_owned(), largest);
    }
    Ok(tolerances)
}

/// The largest magnitude of a finite coordinate of `points`.
fn magnitude(points: &[Point3]) -> f64 {
    points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .filter(|coordinate| coordinate.is_finite())
        .fold(0.0, |largest, coordinate| largest.max(coordinate.abs()))
}

/// The distance of `point` from `curve` over the curve's own domain, or
/// `None` where the curve has no domain this pass searches or no finite
/// point in it.
fn carrier_distance(
    ctx: &DecodeContext<'_>,
    curve: &Curve,
    point: Point3,
) -> Result<Option<f64>, ResourceLimit> {
    let range = curve
        .parameter_range
        .map(IncreasingParameterInterval::endpoints);
    let solved = curve.geometry.solved();
    if let Some(SolvedCurveGeometry::Line(_)) = solved {
        return line_distance(ctx, curve, range, point);
    }
    let domain = range.or_else(|| match solved {
        Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
            Some([0.0, std::f64::consts::TAU])
        }
        Some(SolvedCurveGeometry::Nurbs(nurbs)) => crate::eval::nurbs_curve_parameter_domain(nurbs)
            .map(IncreasingParameterInterval::endpoints),
        _ => None,
    });
    let Some([low, high]) = domain else {
        return Ok(None);
    };
    let distance_at = |parameter: f64| -> Result<Option<f64>, ResourceLimit> {
        Ok(
            finite_or_refusal(curve_point(ctx, &curve.geometry, parameter))?
                .map(|at| Point3::distance(at.get(), point))
                .filter(|distance| distance.is_finite()),
        )
    };
    let step = (high - low) / f64::from(CARRIER_SAMPLES);
    let mut nearest: Option<(f64, f64)> = None;
    for sample in 0..=CARRIER_SAMPLES {
        let parameter = low + step * f64::from(sample);
        if let Some(distance) = distance_at(parameter)? {
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((parameter, distance));
            }
        }
    }
    let Some((parameter, mut best)) = nearest else {
        return Ok(None);
    };
    let (mut a, mut b) = ((parameter - step).max(low), (parameter + step).min(high));
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..CARRIER_REFINEMENTS {
        let (left, right) = (b - ratio * (b - a), a + ratio * (b - a));
        let (Some(at_left), Some(at_right)) = (distance_at(left)?, distance_at(right)?) else {
            break;
        };
        best = best.min(at_left).min(at_right);
        if at_left < at_right {
            b = right;
        } else {
            a = left;
        }
    }
    Ok(Some(best))
}

/// The distance of `point` from a line, over its `range` where it has one.
fn line_distance(
    ctx: &DecodeContext<'_>,
    curve: &Curve,
    range: Option<[f64; 2]>,
    point: Point3,
) -> Result<Option<f64>, ResourceLimit> {
    let (Some(origin), Some(unit)) = (
        finite_or_refusal(curve_point(ctx, &curve.geometry, 0.0))?,
        finite_or_refusal(curve_point(ctx, &curve.geometry, 1.0))?,
    ) else {
        return Ok(None);
    };
    let (origin, unit) = (origin.get(), unit.get());
    let direction = [unit.x - origin.x, unit.y - origin.y, unit.z - origin.z];
    let offset = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    let length = direction
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if !(length.is_finite() && length > 0.0) {
        return Ok(None);
    }
    let along = direction
        .iter()
        .zip(offset)
        .map(|(direction, offset)| direction * offset)
        .sum::<f64>()
        / length;
    let parameter = match range {
        Some([low, high]) => along.clamp(low, high),
        None => along,
    };
    Ok(
        finite_or_refusal(curve_point(ctx, &curve.geometry, parameter))?
            .map(|at| Point3::distance(at.get(), point))
            .filter(|distance| distance.is_finite()),
    )
}

#[cfg(test)]
mod tests;
