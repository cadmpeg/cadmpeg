// SPDX-License-Identifier: Apache-2.0
//! Conic-arc classification and bounded neutral projection.

use super::curve_conversion::angularly_equal;
use super::geometry::{
    admit, entity_loss, resolve_transform, source_object, WireProjectionOutcome,
};
use crate::directory::DirectoryEntry;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const EPS_CONIC_DEGENERATE: f64 = 1.0e-10;
const EPS_CONIC_EXACT_GEOMETRY: f64 = 1.0e-12;

const CONIC_STANDARD_POSITION_RELATIVE_EPSILON: f64 = EPS_CONIC_EXACT_GEOMETRY;

/// The bounded span one conic carrier is projected over.
#[derive(Clone, Copy)]
struct BoundedSpan {
    start: FinitePoint3,
    end: FinitePoint3,
    parameter_range: [f64; 2],
    tolerance: Option<cadmpeg_ir::scalar::PositiveReal>,
}

fn add_bounded_curve(
    ir: &mut CadIr,
    entry: &DirectoryEntry,
    geometry: CurveGeometry,
    span: BoundedSpan,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<EdgeId, cadmpeg_core::CodecError> {
    let BoundedSpan {
        start,
        end,
        parameter_range,
        tolerance,
    } = span;
    let stem = crate::ids::Stem::directory(entry.sequence);
    let start_point = crate::ids::point(&stem.tail(crate::ids::Word::Start));
    sequences.record_point(&start_point, &stem);
    let end_point = crate::ids::point(&stem.tail(crate::ids::Word::End));
    sequences.record_point(&end_point, &stem);
    let start_vertex = crate::ids::vertex(&stem.tail(crate::ids::Word::Start));
    let end_vertex = crate::ids::vertex(&stem.tail(crate::ids::Word::End));
    let curve = crate::ids::curve(&stem);
    let edge = crate::ids::edge(&stem);
    ir.model.points.extend([
        Point::new(start_point.clone(), start, None),
        Point::new(end_point.clone(), end, None),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: start_vertex.clone(),
            point: start_point,
            tolerance,
        },
        Vertex {
            id: end_vertex.clone(),
            point: end_point,
            tolerance,
        },
    ]);
    sequences.record_curve(&curve, entry.sequence);
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry,
        source_object: Some(source_object(entry)?),
    });
    ir.model.edges.push(Edge {
        id: edge.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve), Some(parameter_range))
            .map_err(cadmpeg_core::CodecError::malformed)?,
        start: start_vertex,
        end: end_vertex,
        tolerance,
    });
    Ok(edge)
}

fn endpoint_agrees_with_coefficient_carrier(
    endpoint: Point3,
    evaluated: Point3,
    resolution: f64,
) -> bool {
    let distance = endpoint.distance(evaluated);
    distance == 0.0 || distance < resolution
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<WireProjectionOutcome, CodecError> {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 104 && (0..=3).contains(&entry.form))
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(values) = (1..=11)
            .map(|index| record.number(index).and_then(FiniteReal::new))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(
                entry,
                "conic coefficients or endpoints are invalid",
            ));
            continue;
        };
        let [coeff_a, coeff_b, coeff_c, coeff_d, coeff_e, coeff_f, plane_z, start_x, start_y, end_x, end_y] =
            values.as_slice()
        else {
            losses.push(entity_loss(entry, "conic parameter count is invalid"));
            continue;
        };
        let [coeff_a, coeff_b, coeff_c, coeff_d, coeff_e, coeff_f, plane_z, start_x, start_y, end_x, end_y] =
            [
                coeff_a, coeff_b, coeff_c, coeff_d, coeff_e, coeff_f, plane_z, start_x, start_y,
                end_x, end_y,
            ]
            .map(|value| value.get());
        let coefficient_scale = coeff_a
            .abs()
            .max(coeff_b.abs())
            .max(coeff_c.abs())
            .max(coeff_d.abs())
            .max(coeff_e.abs())
            .max(coeff_f.abs());
        let zero = |value: f64| {
            value.abs() <= coefficient_scale * CONIC_STANDARD_POSITION_RELATIVE_EPSILON
        };
        if !zero(coeff_b) || (!zero(coeff_d) && !zero(coeff_e)) {
            losses.push(entity_loss(
                entry,
                "conic axes or center are not in the required standard position",
            ));
            continue;
        }
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            Ok(transform) => transform,
            Err(error) => {
                let message = error.non_resource()?;
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let Some((basis_x, scale_x)) = transform
            .apply_vector(Vector3::new(1.0, 0.0, 0.0))
            .and_then(|v| {
                let raw = v.get();
                let n = raw.norm();
                n.is_finite().then_some(())?;
                Some((UnitVector3::normalized_nonzero(v)?, n))
            })
        else {
            losses.push(entity_loss(entry, "conic placement collapses the x axis"));
            continue;
        };
        let Some((basis_y, scale_y)) = transform
            .apply_vector(Vector3::new(0.0, 1.0, 0.0))
            .and_then(|v| {
                let raw = v.get();
                let n = raw.norm();
                n.is_finite().then_some(())?;
                Some((UnitVector3::normalized_nonzero(v)?, n))
            })
        else {
            losses.push(entity_loss(entry, "conic placement collapses the y axis"));
            continue;
        };
        if basis_x.as_raw().dot(*basis_y.as_raw()).abs() > EPS_CONIC_DEGENERATE {
            losses.push(entity_loss(
                entry,
                "conic placement produces non-orthogonal principal axes",
            ));
            continue;
        }
        let Some((mut axis, mut axis_raw)) = ({
            let v = basis_x.as_raw().cross(*basis_y.as_raw());
            let n = v.norm();
            (n.is_finite() && n > 0.0)
                .then(|| (UnitVector3::normalized_by_reciprocal(v), v.scale(1.0 / n)))
        }) else {
            losses.push(entity_loss(entry, "conic placement collapses its plane"));
            continue;
        };
        let Some(plane_origin) = transform.apply_point(Point3::new(0.0, 0.0, plane_z * factor))
        else {
            losses.push(entity_loss(entry, "placement produces a non-finite point"));
            continue;
        };
        let Some(start_position) = transform.apply_point(Point3::new(
            start_x * factor,
            start_y * factor,
            plane_z * factor,
        )) else {
            losses.push(entity_loss(entry, "placement produces a non-finite point"));
            continue;
        };
        let Some(end_position) = transform.apply_point(Point3::new(
            end_x * factor,
            end_y * factor,
            plane_z * factor,
        )) else {
            losses.push(entity_loss(entry, "placement produces a non-finite point"));
            continue;
        };
        let start = start_position.get();
        let end = end_position.get();

        let geometry_and_range = if zero(coeff_e)
            && coeff_a != 0.0
            && coeff_c != 0.0
            && coeff_a.is_sign_positive() == coeff_c.is_sign_positive()
        {
            let radius_x = if coeff_f.is_sign_positive() == coeff_a.is_sign_positive() {
                0.0
            } else {
                coeff_f.abs().sqrt() / coeff_a.abs().sqrt()
            };
            let radius_y = if coeff_f.is_sign_positive() == coeff_c.is_sign_positive() {
                0.0
            } else {
                coeff_f.abs().sqrt() / coeff_c.abs().sqrt()
            };
            if radius_x <= 0.0 || radius_y <= 0.0 {
                None
            } else {
                let radius_x = radius_x * factor * scale_x;
                let radius_y = radius_y * factor * scale_y;
                let (major_direction, minor_direction, major_radius, minor_radius) =
                    if radius_x < radius_y {
                        (basis_y, basis_x.reversed(), radius_y, radius_x)
                    } else {
                        (basis_x, basis_y, radius_x, radius_y)
                    };
                let parameter = |point: Point3| {
                    let delta = point.vector_from(plane_origin.get());
                    (delta.dot(*minor_direction.as_raw()) / minor_radius)
                        .atan2(delta.dot(*major_direction.as_raw()) / major_radius)
                        .rem_euclid(std::f64::consts::TAU)
                };
                let raw_start_parameter = parameter(start);
                let raw_end_parameter = parameter(end);
                let mut sweep =
                    (raw_end_parameter - raw_start_parameter).rem_euclid(std::f64::consts::TAU);
                if angularly_equal(sweep, 0.0) {
                    sweep = std::f64::consts::TAU;
                }
                let start_parameter = if angularly_equal(raw_start_parameter, std::f64::consts::TAU)
                {
                    0.0
                } else {
                    raw_start_parameter
                };
                let Some(payload) = admit(
                    axis.and_then(|axis| OrthonormalFrame3::from_units(axis, major_direction))
                        .ok_or("EllipseCurve.axis/major_direction must form an orthonormal frame")
                        .and_then(|frame| {
                            let major_radius = PositiveLength::new(major_radius)
                                .ok_or("EllipseCurve.major_radius must be positive and finite")?;
                            let minor_radius = PositiveLength::new(minor_radius)
                                .ok_or("EllipseCurve.minor_radius must be positive and finite")?;
                            cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                                plane_origin,
                                frame,
                                major_radius,
                                minor_radius,
                            )
                        }),
                    entry,
                    &mut losses,
                ) else {
                    continue;
                };
                (sweep > 0.0).then_some((
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(payload)),
                    [start_parameter, start_parameter + sweep],
                ))
            }
        } else if zero(coeff_e)
            && coeff_a != 0.0
            && coeff_c != 0.0
            && coeff_a.is_sign_positive() != coeff_c.is_sign_positive()
        {
            let (major, minor, major_radius, minor_radius) =
                if coeff_f.is_sign_positive() == coeff_a.is_sign_positive() {
                    (
                        basis_y,
                        basis_x.reversed(),
                        coeff_f.abs().sqrt() / coeff_c.abs().sqrt(),
                        coeff_f.abs().sqrt() / coeff_a.abs().sqrt(),
                    )
                } else {
                    (
                        basis_x,
                        basis_y,
                        coeff_f.abs().sqrt() / coeff_a.abs().sqrt(),
                        coeff_f.abs().sqrt() / coeff_c.abs().sqrt(),
                    )
                };
            if major_radius <= 0.0 || minor_radius <= 0.0 {
                None
            } else {
                let major_scale = if major.as_raw().dot(*basis_x.as_raw()).abs() > 0.5 {
                    scale_x
                } else {
                    scale_y
                };
                let minor_scale = if minor.as_raw().dot(*basis_x.as_raw()).abs() > 0.5 {
                    scale_x
                } else {
                    scale_y
                };
                let major_radius = major_radius * factor * major_scale;
                let minor_radius = minor_radius * factor * minor_scale;
                let branch = if start.vector_from(plane_origin.get()).dot(*major.as_raw()) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let major_direction = if branch < 0.0 {
                    major.reversed()
                } else {
                    major
                };
                let parameter = |point: Point3, axis: Vector3| {
                    let minor_direction = axis.cross(*major_direction.as_raw());
                    (point.vector_from(plane_origin.get()).dot(minor_direction) / minor_radius)
                        .asinh()
                };
                let mut start_parameter = parameter(start, axis_raw);
                let mut end_parameter = parameter(end, axis_raw);
                if end_parameter < start_parameter {
                    axis = axis.map(UnitVector3::reversed);
                    axis_raw = axis_raw.scale(-1.0);
                    start_parameter = parameter(start, axis_raw);
                    end_parameter = parameter(end, axis_raw);
                }
                let Some(payload) = admit(
                    axis.and_then(|axis| OrthonormalFrame3::from_units(axis, major_direction))
                        .ok_or("HyperbolaCurve.axis/major_direction must form an orthonormal frame")
                        .and_then(|frame| {
                            let major_radius = PositiveLength::new(major_radius)
                                .ok_or("HyperbolaCurve.major_radius must be positive and finite")?;
                            let minor_radius = PositiveLength::new(minor_radius)
                                .ok_or("HyperbolaCurve.minor_radius must be positive and finite")?;
                            Ok(cadmpeg_ir::geometry::analytic::HyperbolaCurve::new(
                                plane_origin,
                                frame,
                                major_radius,
                                minor_radius,
                            ))
                        }),
                    entry,
                    &mut losses,
                ) else {
                    continue;
                };
                (end_parameter > start_parameter).then_some((
                    CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(payload)),
                    [start_parameter, end_parameter],
                ))
            }
        } else if zero(coeff_c) && zero(coeff_f) && !zero(coeff_a) && !zero(coeff_e) {
            let opening = if -coeff_a / coeff_e >= 0.0 { 1.0 } else { -1.0 };
            let major_direction = if opening < 0.0 {
                basis_y.reversed()
            } else {
                basis_y
            };
            let Some(focal) = cadmpeg_ir::math::product_quotient(
                [coeff_e, factor, scale_x, scale_x],
                [4.0, coeff_a, scale_y],
            )
            .map(cadmpeg_ir::scalar::FiniteReal::abs) else {
                losses.push(entity_loss(
                    entry,
                    "parabola focal distance is not representable",
                ));
                continue;
            };
            let focal_distance = focal.get();
            let parameter = |point: Point3, axis: Vector3| {
                cadmpeg_ir::math::multiply_divide(
                    cadmpeg_ir::scalar::FiniteReal::new(
                        point
                            .vector_from(plane_origin.get())
                            .dot(axis.cross(*major_direction.as_raw())),
                    )?,
                    cadmpeg_ir::scalar::FiniteReal::HALF,
                    focal,
                )
                .map(cadmpeg_ir::scalar::FiniteReal::get)
            };
            let (Some(mut start_parameter), Some(mut end_parameter)) =
                (parameter(start, axis_raw), parameter(end, axis_raw))
            else {
                losses.push(entity_loss(
                    entry,
                    "parabola endpoint parameter is not representable",
                ));
                continue;
            };
            if end_parameter < start_parameter {
                axis = axis.map(UnitVector3::reversed);
                start_parameter = -start_parameter;
                end_parameter = -end_parameter;
            }
            let Some(payload) = admit(
                axis.and_then(|axis| OrthonormalFrame3::from_units(axis, major_direction))
                    .ok_or("ParabolaCurve.axis/major_direction must form an orthonormal frame")
                    .and_then(|frame| {
                        let focal_distance = PositiveLength::new(focal_distance)
                            .ok_or("ParabolaCurve.focal_distance must be positive and finite")?;
                        Ok(cadmpeg_ir::geometry::analytic::ParabolaCurve::new(
                            plane_origin,
                            frame,
                            focal_distance,
                        ))
                    }),
                entry,
                &mut losses,
            ) else {
                continue;
            };
            (focal_distance > 0.0 && end_parameter > start_parameter).then_some((
                CurveGeometry::Solved(SolvedCurveGeometry::Parabola(payload)),
                [start_parameter, end_parameter],
            ))
        } else if zero(coeff_a) && zero(coeff_f) && !zero(coeff_c) && !zero(coeff_d) {
            let opening = if -coeff_c / coeff_d >= 0.0 { 1.0 } else { -1.0 };
            let major_direction = if opening < 0.0 {
                basis_x.reversed()
            } else {
                basis_x
            };
            let Some(focal) = cadmpeg_ir::math::product_quotient(
                [coeff_d, factor, scale_y, scale_y],
                [4.0, coeff_c, scale_x],
            )
            .map(cadmpeg_ir::scalar::FiniteReal::abs) else {
                losses.push(entity_loss(
                    entry,
                    "parabola focal distance is not representable",
                ));
                continue;
            };
            let focal_distance = focal.get();
            let parameter = |point: Point3, axis: Vector3| {
                cadmpeg_ir::math::multiply_divide(
                    cadmpeg_ir::scalar::FiniteReal::new(
                        point
                            .vector_from(plane_origin.get())
                            .dot(axis.cross(*major_direction.as_raw())),
                    )?,
                    cadmpeg_ir::scalar::FiniteReal::HALF,
                    focal,
                )
                .map(cadmpeg_ir::scalar::FiniteReal::get)
            };
            let (Some(mut start_parameter), Some(mut end_parameter)) =
                (parameter(start, axis_raw), parameter(end, axis_raw))
            else {
                losses.push(entity_loss(
                    entry,
                    "parabola endpoint parameter is not representable",
                ));
                continue;
            };
            if end_parameter < start_parameter {
                axis = axis.map(UnitVector3::reversed);
                start_parameter = -start_parameter;
                end_parameter = -end_parameter;
            }
            let Some(payload) = admit(
                axis.and_then(|axis| OrthonormalFrame3::from_units(axis, major_direction))
                    .ok_or("ParabolaCurve.axis/major_direction must form an orthonormal frame")
                    .and_then(|frame| {
                        let focal_distance = PositiveLength::new(focal_distance)
                            .ok_or("ParabolaCurve.focal_distance must be positive and finite")?;
                        Ok(cadmpeg_ir::geometry::analytic::ParabolaCurve::new(
                            plane_origin,
                            frame,
                            focal_distance,
                        ))
                    }),
                entry,
                &mut losses,
            ) else {
                continue;
            };
            (focal_distance > 0.0 && end_parameter > start_parameter).then_some((
                CurveGeometry::Solved(SolvedCurveGeometry::Parabola(payload)),
                [start_parameter, end_parameter],
            ))
        } else {
            None
        };

        let Some((geometry, parameter_range)) = geometry_and_range else {
            losses.push(entity_loss(
                entry,
                "standard-position coefficients do not define a nondegenerate conic arc",
            ));
            continue;
        };
        let Some(evaluated_start) =
            finite_or_refusal(cadmpeg_ir::eval::curve_point(&geometry, parameter_range[0]))?
        else {
            losses.push(entity_loss(entry, "conic start point cannot be evaluated"));
            continue;
        };
        let Some(evaluated_end) =
            finite_or_refusal(cadmpeg_ir::eval::curve_point(&geometry, parameter_range[1]))?
        else {
            losses.push(entity_loss(
                entry,
                "conic terminate point cannot be evaluated",
            ));
            continue;
        };
        // CADIR decision: IGES defines the carrier and ordered endpoints but
        // does not prescribe an endpoint-consistency test or receiver action.
        let resolution = global.minimum_resolution_mm();
        if !endpoint_agrees_with_coefficient_carrier(start, evaluated_start.get(), resolution) {
            losses.push(entity_loss(
                entry,
                "conic start point disagrees with the evaluated carrier beyond the minimum resolution",
            ));
            continue;
        }
        if !endpoint_agrees_with_coefficient_carrier(end, evaluated_end.get(), resolution) {
            losses.push(entity_loss(
                entry,
                "conic terminate point disagrees with the evaluated carrier beyond the minimum resolution",
            ));
            continue;
        }
        let tolerance = if resolution > 0.0 {
            let Some(value) = cadmpeg_ir::scalar::PositiveReal::new(resolution) else {
                losses.push(entity_loss(entry, "conic tolerance must be finite"));
                continue;
            };
            Some(value)
        } else {
            None
        };
        let edge = match add_bounded_curve(
            ir,
            entry,
            geometry,
            BoundedSpan {
                start: start_position,
                end: end_position,
                parameter_range,
                tolerance,
            },
            sequences,
        ) {
            Ok(edge) => edge,
            Err(error) => {
                losses.push(entity_loss(entry, error.to_string()));
                continue;
            }
        };
        wire_edges.push(edge);
        decoded.insert(entry.sequence);
    }

    Ok(WireProjectionOutcome {
        decoded,
        losses,
        wire_edges,
    })
}

#[cfg(test)]
mod tests;
