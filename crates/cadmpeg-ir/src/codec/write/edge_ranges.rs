// SPDX-License-Identifier: Apache-2.0
//! Parameter ranges a writer states for open edges that state none.
//!
//! A target whose edge record stores a parameter range (F3D, IGES) writes an
//! open edge with no stated range over the range its vertices fix on its
//! curve. [`open_edge_ranges`] finds that range and [`EdgeRangePlan::apply`]
//! states it; the writer charges the plan's report entries in its own loss
//! vocabulary.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};
use cadmpeg_core::CodecError;

use crate::document::{CadIr, Topology};
use crate::eval::admission::EvaluationAdmission;
use crate::eval::EvaluationFailure;
use crate::geometry::SolvedCurveGeometry;
use crate::ids::{CurveId, EdgeId, VertexId};
use crate::math::Point3;
use crate::scalar::PositiveLength;
use crate::topology::{Edge, EdgeCarrier, EdgeEnds, Sense};
use crate::units::binding_allowance;

/// The ranges [`open_edge_ranges`] states, and what a writer reports for them.
#[derive(Debug, Clone, Default)]
pub struct EdgeRangePlan {
    stated: Vec<(usize, EdgeCarrier)>,
    exchanged: HashSet<EdgeId>,
    /// Each vertex whose tolerance is raised to its distance from the end of
    /// an edge's curve, with the raised tolerance.
    pub widened: BTreeMap<VertexId, PositiveLength>,
    /// Each line or conic edge ranged between its vertices' feet on the
    /// curve, with each vertex that lies beyond the edge's allowance from the
    /// curve and that distance. The output's validation finds the vertex off
    /// its curve.
    pub off_curve: Vec<(EdgeId, Vec<(VertexId, f64)>)>,
    /// Each edge whose curve passes a vertex of the edge on two arcs, with
    /// the range of the shortest arc the writer can state that passes neither
    /// vertex between its ends.
    pub assumed_arcs: Vec<(EdgeId, [f64; 2])>,
}

impl EdgeRangePlan {
    /// States each planned range in `ir`, the document the plan was made
    /// from: the edge takes its range, an edge that runs against its line or
    /// aperiodic NURBS curve has its ends exchanged and its coedges' senses
    /// reversed, and each widened vertex takes its raised tolerance.
    pub fn apply(&self, ir: &mut CadIr) {
        let geometry = &mut ir.geometry;
        for (edge_index, carrier) in &self.stated {
            let edge = &mut geometry.edges[*edge_index];
            edge.carrier = carrier.clone();
            if self.exchanged.contains(&edge.id) {
                edge.ends = edge
                    .ends
                    .take()
                    .map(|ends| EdgeEnds::new(ends.end, ends.start));
            }
        }
        for coedge in &mut geometry.coedges {
            if self.exchanged.contains(&coedge.edge) {
                coedge.sense = match coedge.sense {
                    Sense::Forward => Sense::Reversed,
                    Sense::Reversed => Sense::Forward,
                };
            }
        }
        for vertex in &mut geometry.vertices {
            if let Some(tolerance) = self.widened.get(&vertex.id) {
                vertex.tolerance = Some(*tolerance);
            }
        }
    }
}

/// Each vertex's position and stated tolerance, by vertex id.
#[must_use]
pub fn vertex_positions(geometry: &Topology) -> HashMap<&str, (Point3, Option<f64>)> {
    let points = geometry
        .points
        .iter()
        .map(|point| (point.id.as_str(), point.position().get()))
        .collect::<HashMap<_, _>>();
    geometry
        .vertices
        .iter()
        .filter_map(|vertex| {
            Some((
                vertex.id.as_str(),
                (
                    *points.get(vertex.point.as_str())?,
                    vertex.tolerance.map(PositiveLength::get),
                ),
            ))
        })
        .collect()
}

/// The ranges of the open edges of `ir` that state none, or `None` where every
/// open edge with a curve states one.
///
/// The vertices invert within the edge's coincidence allowance, the bound
/// validation checks the edge's ends against: the edge's, the vertex's own,
/// and `fit`, the cache-fit tolerance of the curve's construction. A vertex
/// farther from its carrier than its own allowance is raised to that
/// distance. On a curve other than a line, conic or point, each end also
/// inverts from the other end's seed. Where a self-crossing curve passes a
/// vertex on two arcs, the edge takes the shortest range the writer can state
/// between a witness of each end that passes no other witness of its
/// vertices. The two ends of a closed curve's domain are one place. An edge
/// that runs against its line, or against an aperiodic NURBS curve, is
/// written with its ends exchanged, where `names_edge_uses` states that no
/// use curve or native record fixes the edge's direction. A line or conic
/// carrier that a vertex lies beyond the edge's allowance from is ranged
/// between the vertices' feet on it. Any other edge keeps no range.
///
/// # Errors
///
/// Returns a resource-limit error where curve evaluation exhausts a ceiling
/// of `limits`.
pub fn open_edge_ranges(
    ir: &CadIr,
    limits: ResourceLimits,
    fit: impl Fn(&CurveId) -> Option<f64>,
    names_edge_uses: impl Fn(&Edge) -> bool,
) -> Result<Option<EdgeRangePlan>, CodecError> {
    let linear = ir.tolerances.linear;
    let geometry = &ir.geometry;
    let open = geometry
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| {
            edge.param_range().is_none()
                && edge.curve().is_some()
                && edge
                    .ends
                    .as_ref()
                    .is_some_and(|ends| ends.start != ends.end)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if open.is_empty() {
        return Ok(None);
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy { limits })?;
    let index = crate::index::ModelIndex::new_model_only(ir);
    let mut vertices = vertex_positions(geometry);
    let curves = geometry
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<HashMap<_, _>>();
    let mut tolerant = HashMap::<&VertexId, f64>::new();
    let (stated, exchanged, off_curve, assumed_arcs) = loop {
        let mut stated = Vec::new();
        let mut exchanged = HashSet::new();
        let mut off_curve = Vec::new();
        let mut assumed_arcs = Vec::new();
        let mut raised_any = false;
        for &edge_index in &open {
            let edge = &geometry.edges[edge_index];
            let (Some(curve), Some(ends)) = (edge.curve(), edge.ends.as_ref()) else {
                continue;
            };
            let (Some(&(start, start_tolerance)), Some(&(end, end_tolerance)), Some(carrier)) = (
                vertices.get(ends.start.as_str()),
                vertices.get(ends.end.as_str()),
                curves.get(curve.as_str()),
            ) else {
                continue;
            };
            let fit = fit(curve);
            let edge_tolerance = edge.tolerance.map(PositiveLength::get);
            let allowance = binding_allowance(
                linear,
                &[edge_tolerance, start_tolerance, end_tolerance, fit],
            );
            // On a bounded curve the start inverts from the domain's start
            // and the end from its end, the ends validation checks an
            // unranged edge at: on a curve shorter than the allowance, or
            // closed, the end would otherwise invert to the start's parameter.
            let domain = crate::eval::bounded_curve_domain(carrier);
            let invert = |point: Point3, seed: f64, tolerance: f64| {
                crate::eval::model_curve_parameter_near_point_in_index_with_tolerance(
                    &ctx, &index, curve, point, seed, tolerance,
                )
                .map(|parameter| parameter.map(crate::scalar::FiniteReal::get))
            };
            let parameters = |tolerance| -> Result<Option<[f64; 2]>, CodecError> {
                let Some(first) = invert(start, domain.map_or(0.0, |[low, _]| low), tolerance)?
                else {
                    return Ok(None);
                };
                Ok(
                    invert(end, domain.map_or(first, |[_, high]| high), tolerance)?
                        .map(|last| [first, last]),
                )
            };
            let point_at = |parameter| -> Result<Option<Point3>, CodecError> {
                match crate::eval::model_curve_point_by_id(
                    EvaluationAdmission::Decode(&ctx),
                    &index,
                    curve,
                    parameter,
                ) {
                    Ok(foot) => Ok(Some(foot.get())),
                    Err(EvaluationFailure::ResourceLimit(limit)) => {
                        Err(CodecError::ResourceLimit(limit))
                    }
                    Err(
                        EvaluationFailure::NoValue
                        | EvaluationFailure::NonFinite(_)
                        | EvaluationFailure::WorkExhausted,
                    ) => Ok(None),
                }
            };
            let gap = |point: Point3, parameter| -> Result<Option<f64>, CodecError> {
                Ok(point_at(parameter)?.map(|foot| foot.distance(point)))
            };
            let solved = carrier.geometry.solved();
            let line = matches!(solved, Some(SolvedCurveGeometry::Line(_)));
            let aperiodic_nurbs =
                matches!(solved, Some(SolvedCurveGeometry::Nurbs(nurbs)) if !nurbs.periodic());
            let conic = matches!(
                solved,
                Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_))
            );
            let freeform = !matches!(
                solved,
                Some(
                    SolvedCurveGeometry::Line(_)
                        | SolvedCurveGeometry::Circle(_)
                        | SolvedCurveGeometry::Ellipse(_)
                        | SolvedCurveGeometry::Parabola(_)
                        | SolvedCurveGeometry::Hyperbola(_)
                        | SolvedCurveGeometry::Degenerate(_)
                )
            );
            let (within, [mut first, mut last]) = match parameters(allowance)? {
                Some(range) => (true, range),
                None if line || conic => match parameters(f64::MAX)? {
                    Some(range) => (false, range),
                    None => continue,
                },
                None => continue,
            };
            let mut assumed = None;
            if freeform {
                // A freeform curve can pass a point on two arcs, and the
                // seed picks the witness: each end inverts from the other
                // end's seed as well. On a closed curve, periodic or not,
                // the witnesses split the loop into two arcs, and the
                // closure's two witnesses bound a short one across it.
                let bounds = match solved {
                    Some(SolvedCurveGeometry::Nurbs(nurbs)) if nurbs.periodic() => {
                        crate::eval::nurbs_curve_parameter_domain(nurbs)
                            .map(crate::topology::IncreasingParameterInterval::endpoints)
                    }
                    _ => domain,
                };
                let other_place = |point: Point3,
                                   seed: f64,
                                   witness: f64|
                 -> Result<Option<f64>, CodecError> {
                    let near = |parameter| -> Result<bool, CodecError> {
                        Ok(gap(point, parameter)?.is_some_and(|distance| distance <= allowance))
                    };
                    let Some(other) = invert(point, seed, allowance)? else {
                        return Ok(None);
                    };
                    let middle = 0.5 * witness + 0.5 * other;
                    if near(middle)? {
                        return Ok(None);
                    }
                    let Some([low, high]) = bounds else {
                        return Ok(Some(other));
                    };
                    let Some(closure) = point_at(low)? else {
                        return Ok(Some(other));
                    };
                    if !gap(closure, high)?.is_some_and(|distance| distance <= allowance) {
                        return Ok(Some(other));
                    }
                    let half = 0.5 * (high - low);
                    let opposite = if middle + half <= high {
                        middle + half
                    } else {
                        middle - half
                    };
                    Ok((!near(opposite)?).then_some(other))
                };
                let starts = [
                    Some(first),
                    other_place(start, domain.map_or(last, |[_, high]| high), first)?,
                ];
                let finishes = [
                    Some(last),
                    other_place(end, domain.map_or(0.0, |[low, _]| low), last)?,
                ];
                if starts[1].is_some() || finishes[1].is_some() {
                    let reversible = aperiodic_nurbs && !names_edge_uses(edge);
                    let witnesses = starts.iter().chain(&finishes).flatten();
                    let Some(arc) = starts
                        .iter()
                        .flatten()
                        .flat_map(|&from| finishes.iter().flatten().map(move |&to| [from, to]))
                        .filter(|&[from, to]| {
                            let (low, high) = (from.min(to), from.max(to));
                            low < high
                                && (from < to || reversible)
                                && !witnesses
                                    .clone()
                                    .any(|&witness| low < witness && witness < high)
                        })
                        .min_by(|a, b| (a[1] - a[0]).abs().total_cmp(&(b[1] - b[0]).abs()))
                    else {
                        continue;
                    };
                    [first, last] = arc;
                    assumed = Some((edge.id.clone(), [first, last]));
                }
            }
            let reach = [
                (&ends.start, start, start_tolerance, first),
                (&ends.end, end, end_tolerance, last),
            ];
            let mut range = [first, last];
            if last <= first && conic {
                range[1] += std::f64::consts::TAU;
            }
            let exchange = (line || aperiodic_nurbs) && last < first;
            if exchange {
                if names_edge_uses(edge) {
                    continue;
                }
                range = [last, first];
            }
            if range[1] <= range[0] {
                continue;
            }
            let Ok(carrier) = EdgeCarrier::new(Some(curve.clone()), Some(range)) else {
                continue;
            };
            let mut off = Vec::new();
            for (vertex, point, own, parameter) in reach {
                let Some(distance) = gap(point, parameter)? else {
                    continue;
                };
                if !within && distance > allowance {
                    off.push((vertex.clone(), distance));
                } else if within
                    && distance > binding_allowance(linear, &[edge_tolerance, own, fit])
                {
                    let raised = tolerant.entry(vertex).or_insert(distance);
                    if distance > *raised {
                        *raised = distance;
                    }
                    if own.is_none_or(|current| distance > current) {
                        if let Some((_, entry)) = vertices.get_mut(vertex.as_str()) {
                            *entry = Some(distance);
                            raised_any = true;
                        }
                    }
                }
            }
            if !off.is_empty() {
                off_curve.push((edge.id.clone(), off));
            }
            if exchange {
                exchanged.insert(edge.id.clone());
            }
            assumed_arcs.extend(assumed);
            stated.push((edge_index, carrier));
        }
        if !raised_any {
            break (stated, exchanged, off_curve, assumed_arcs);
        }
    };
    let widened = tolerant
        .into_iter()
        .filter_map(|(vertex, distance)| Some((vertex.clone(), PositiveLength::new(distance)?)))
        .collect();
    Ok(Some(EdgeRangePlan {
        stated,
        exchanged,
        widened,
        off_curve,
        assumed_arcs,
    }))
}
