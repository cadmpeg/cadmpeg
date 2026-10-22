// SPDX-License-Identifier: Apache-2.0
//! Compare analytic pattern copies with their canonical distribution controls.

use super::{angle_close, close, distance2, sketch_locus_point, Geometries};
use crate::math::Point2;
use crate::sketches::{
    SketchCircularPattern, SketchEntityId, SketchGeometryDefinition as Geometry, SketchLocus,
    SketchRectangularPattern,
};
use crate::CadIr;

fn geometry_agrees(
    ir: &CadIr,
    seed: &SketchEntityId,
    copy: &SketchEntityId,
    geometry: &Geometries<'_>,
    point: impl Fn(Point2) -> Point2,
    rotation: f64,
) -> Option<bool> {
    let agrees = |seed, copy| close(ir, distance2(point(seed), copy), 0.0);
    Some(
        match (
            geometry.get(seed.as_str())?.definition(),
            geometry.get(copy.as_str())?.definition(),
        ) {
            (Geometry::Point { position: first }, Geometry::Point { position: second }) => {
                agrees(first.get(), second.get())
            }
            (
                Geometry::Line {
                    start: first_start,
                    end: first_end,
                },
                Geometry::Line {
                    start: second_start,
                    end: second_end,
                },
            ) => {
                agrees(first_start.get(), second_start.get())
                    && agrees(first_end.get(), second_end.get())
            }
            (
                Geometry::Circle {
                    center: first_center,
                    radius: first_radius,
                },
                Geometry::Circle {
                    center: second_center,
                    radius: second_radius,
                },
            ) => {
                agrees(first_center.get(), second_center.get())
                    && close(ir, first_radius.get(), second_radius.get())
            }
            (
                Geometry::Arc {
                    center: first_center,
                    radius: first_radius,
                    start_angle: first_start,
                    end_angle: first_end,
                },
                Geometry::Arc {
                    center: second_center,
                    radius: second_radius,
                    start_angle: second_start,
                    end_angle: second_end,
                },
            ) => {
                agrees(first_center.get(), second_center.get())
                    && close(ir, first_radius.get(), second_radius.get())
                    && angle_close(ir, first_start.get() + rotation, second_start.get())
                    && angle_close(ir, first_end.get() + rotation, second_end.get())
                    && (first_end.get() - first_start.get() - second_end.get() + second_start.get())
                        .abs()
                        <= ir.tolerances.angular.get()
            }
            // A different spline parameterization or unsupported native carrier is
            // not evidence of disagreement. Its references remain independently
            // checked; no sampled geometry is invented for this comparison.
            _ => return None,
        },
    )
}

pub(super) fn rectangular(
    ir: &CadIr,
    pattern: &SketchRectangularPattern,
    geometry: &Geometries<'_>,
) -> Option<&'static str> {
    let counts: Option<Vec<_>> = pattern
        .directions()
        .iter()
        .map(|direction| ir.dimension_value(&direction.count))
        .collect();
    if let Some(counts) = counts {
        if usize::try_from(counts[1]).ok() != Some(pattern.rows().len())
            || pattern
                .rows()
                .iter()
                .any(|row| usize::try_from(counts[0]).ok() != Some(row.len()))
        {
            return Some(
                "solved rectangular-pattern population disagrees with its canonical counts",
            );
        }
    }
    let first = &pattern.directions()[0];
    let second = &pattern.directions()[1];
    let [first_axis, second_axis] = [first.direction().get(), second.direction().get()];
    let [first_spacing, second_spacing] = [first.spacing(ir)?, second.spacing(ir)?];
    let seed = &pattern.rows().first()?.first()?.entities;
    for (row_index, row) in pattern.rows().iter().enumerate() {
        for (column_index, instance) in row.iter().enumerate() {
            let row = f64::from(u32::try_from(row_index).ok()?);
            let column = f64::from(u32::try_from(column_index).ok()?);
            let offset = (
                first_axis[0] * first_spacing.get() * column
                    + second_axis[0] * second_spacing.get() * row,
                first_axis[1] * first_spacing.get() * column
                    + second_axis[1] * second_spacing.get() * row,
            );
            for (seed, copy) in seed.iter().zip(&instance.entities) {
                if geometry_agrees(
                    ir,
                    seed,
                    copy,
                    geometry,
                    |point| Point2::new(point.u + offset.0, point.v + offset.1),
                    0.0,
                ) == Some(false)
                {
                    return Some("solved rectangular-pattern geometry disagrees with its canonical distribution");
                }
            }
        }
    }
    None
}

pub(super) fn circular(
    ir: &CadIr,
    pattern: &SketchCircularPattern,
    geometry: &Geometries<'_>,
) -> Option<&'static str> {
    if let Some(count) = ir.dimension_value(pattern.count()) {
        if usize::try_from(count).ok() != pattern.instances().len().checked_add(1) {
            return Some("solved circular-pattern population disagrees with its canonical count");
        }
    }
    let spacing = pattern.angle().resolved_spacing(ir, pattern.count())?.get();
    let center = sketch_locus_point(&SketchLocus::Entity(pattern.center().clone()), geometry);
    for (index, instance) in pattern.instances().iter().enumerate() {
        let index = u32::try_from(index).ok()?.checked_add(1)?;
        let angle = spacing * f64::from(index);
        if (angle - instance.angle.get()).abs() > ir.tolerances.angular.get() {
            return Some(
                "solved circular-pattern rotation disagrees with its canonical distribution",
            );
        }
        let Some(center) = center else {
            continue;
        };
        let sine = angle.sin();
        let cosine = angle.cos();
        for (seed, copy) in pattern.seed().iter().zip(&instance.entities) {
            if geometry_agrees(
                ir,
                seed,
                copy,
                geometry,
                |point| {
                    let u = point.u - center.u;
                    let v = point.v - center.v;
                    Point2::new(
                        center.u + cosine * u - sine * v,
                        center.v + sine * u + cosine * v,
                    )
                },
                angle,
            ) == Some(false)
            {
                return Some(
                    "solved circular-pattern geometry disagrees with its canonical distribution",
                );
            }
        }
    }
    None
}
