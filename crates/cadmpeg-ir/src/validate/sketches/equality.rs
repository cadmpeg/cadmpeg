// SPDX-License-Identifier: Apache-2.0
//! Exact sketch geometry comparison with caller-owned scan admission.

use super::super::scans::all;
use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn text_equal(
    ctx: &DecodeContext<'_>,
    left: &str,
    right: &str,
    operation: &'static str,
) -> Result<bool, CodecError> {
    ctx.equal_bytes(left.as_bytes(), right.as_bytes(), operation)
}

fn nurbs_equal(
    ctx: &DecodeContext<'_>,
    left: &PcurveNurbs,
    right: &PcurveNurbs,
) -> Result<bool, CodecError> {
    if left.degree() != right.degree() || left.knots().len() != right.knots().len() {
        return Ok(false);
    }
    if !all(
        ctx,
        left.knots().iter().zip(right.knots()),
        |(left, right)| Ok(left == right),
    )? {
        return Ok(false);
    }
    let poles_equal = match (left.pole_rows(), right.pole_rows()) {
        (
            PcurveNurbsPoles::Polynomial { points: left },
            PcurveNurbsPoles::Polynomial { points: right },
        ) => {
            left.len() == right.len()
                && all(ctx, left.iter().zip(right), |(left, right)| {
                    Ok(left == right)
                })?
        }
        (
            PcurveNurbsPoles::Rational { points: left },
            PcurveNurbsPoles::Rational { points: right },
        ) => {
            left.len() == right.len()
                && all(ctx, left.iter().zip(right), |(left, right)| {
                    Ok(left == right)
                })?
        }
        _ => false,
    };
    Ok(poles_equal && left.periodic() == right.periodic())
}

pub(super) fn geometry_equal(
    ctx: &DecodeContext<'_>,
    left: &SketchGeometry,
    right: &SketchGeometry,
) -> Result<bool, CodecError> {
    match (left.definition(), right.definition()) {
        (
            SketchGeometryDefinition::Nurbs { curve: left },
            SketchGeometryDefinition::Nurbs { curve: right },
        ) => nurbs_equal(ctx, left, right),
        (
            SketchGeometryDefinition::Text {
                text: left_text,
                font_family: left_family,
                font_weight: left_weight,
                height: left_height,
                width_factor: left_width,
                placement: left_placement,
                horizontal_alignment: left_horizontal,
                vertical_alignment: left_vertical,
            },
            SketchGeometryDefinition::Text {
                text: right_text,
                font_family: right_family,
                font_weight: right_weight,
                height: right_height,
                width_factor: right_width,
                placement: right_placement,
                horizontal_alignment: right_horizontal,
                vertical_alignment: right_vertical,
            },
        ) => Ok(text_equal(
            ctx,
            left_text.as_str(),
            right_text.as_str(),
            "compare sketch geometry text",
        )? && text_equal(
            ctx,
            left_family.as_str(),
            right_family.as_str(),
            "compare sketch geometry text",
        )? && left_weight == right_weight
            && left_height == right_height
            && left_width == right_width
            && left_placement == right_placement
            && left_horizontal == right_horizontal
            && left_vertical == right_vertical),
        (
            SketchGeometryDefinition::ExternalReference {
                document: left_document,
                object: left_object,
                subelements: left_elements,
            },
            SketchGeometryDefinition::ExternalReference {
                document: right_document,
                object: right_object,
                subelements: right_elements,
            },
        ) => {
            let documents_equal = match (left_document.as_deref(), right_document.as_deref()) {
                (Some(left), Some(right)) => {
                    text_equal(ctx, left, right, "compare sketch geometry text")?
                }
                (None, None) => true,
                _ => false,
            };
            Ok(documents_equal
                && text_equal(
                    ctx,
                    left_object.as_str(),
                    right_object.as_str(),
                    "compare sketch geometry text",
                )?
                && left_elements.len() == right_elements.len()
                && all(
                    ctx,
                    left_elements.iter().zip(right_elements),
                    |(left, right)| text_equal(ctx, left, right, "compare sketch geometry text"),
                )?)
        }
        (
            SketchGeometryDefinition::Native { native_kind: left },
            SketchGeometryDefinition::Native { native_kind: right },
        ) => text_equal(
            ctx,
            left.as_str(),
            right.as_str(),
            "compare sketch geometry text",
        ),
        (
            SketchGeometryDefinition::Point {
                position: left_position,
            },
            SketchGeometryDefinition::Point {
                position: right_position,
            },
        ) => Ok(left_position.get() == right_position.get()),
        (
            SketchGeometryDefinition::Line {
                start: left_start,
                end: left_end,
            },
            SketchGeometryDefinition::Line {
                start: right_start,
                end: right_end,
            },
        ) => Ok((left_start.get(), left_end.get()) == (right_start.get(), right_end.get())),
        (
            SketchGeometryDefinition::ReferenceLine {
                origin: left_origin,
                direction: left_direction,
            },
            SketchGeometryDefinition::ReferenceLine {
                origin: right_origin,
                direction: right_direction,
            },
        ) => Ok((left_origin.get(), left_direction.get())
            == (right_origin.get(), right_direction.get())),
        (
            SketchGeometryDefinition::Circle {
                center: left_center,
                radius: left_radius,
            },
            SketchGeometryDefinition::Circle {
                center: right_center,
                radius: right_radius,
            },
        ) => Ok((left_center.get(), left_radius.get()) == (right_center.get(), right_radius.get())),
        (
            SketchGeometryDefinition::Arc {
                center: left_center,
                radius: left_radius,
                start_angle: left_start_angle,
                end_angle: left_end_angle,
            },
            SketchGeometryDefinition::Arc {
                center: right_center,
                radius: right_radius,
                start_angle: right_start_angle,
                end_angle: right_end_angle,
            },
        ) => Ok((
            left_center.get(),
            left_radius.get(),
            left_start_angle.get(),
            left_end_angle.get(),
        ) == (
            right_center.get(),
            right_radius.get(),
            right_start_angle.get(),
            right_end_angle.get(),
        )),
        (
            SketchGeometryDefinition::Ellipse {
                center: left_center,
                major_angle: left_major_angle,
                radii: left_radii,
                bounds: left_bounds,
            },
            SketchGeometryDefinition::Ellipse {
                center: right_center,
                major_angle: right_major_angle,
                radii: right_radii,
                bounds: right_bounds,
            },
        ) => Ok((
            left_center.get(),
            left_major_angle.get(),
            left_radii.major().get(),
            left_radii.minor().get(),
            (*left_bounds).map(|[start, end]| [start.get(), end.get()]),
        ) == (
            right_center.get(),
            right_major_angle.get(),
            right_radii.major().get(),
            right_radii.minor().get(),
            (*right_bounds).map(|[start, end]| [start.get(), end.get()]),
        )),
        (
            SketchGeometryDefinition::Hyperbola {
                center: left_center,
                major_angle: left_major_angle,
                major_radius: left_major_radius,
                minor_radius: left_minor_radius,
                bounds: left_bounds,
            },
            SketchGeometryDefinition::Hyperbola {
                center: right_center,
                major_angle: right_major_angle,
                major_radius: right_major_radius,
                minor_radius: right_minor_radius,
                bounds: right_bounds,
            },
        ) => Ok((
            left_center.get(),
            left_major_angle.get(),
            left_major_radius.get(),
            left_minor_radius.get(),
            (*left_bounds).map(|[start, end]| [start.get(), end.get()]),
        ) == (
            right_center.get(),
            right_major_angle.get(),
            right_major_radius.get(),
            right_minor_radius.get(),
            (*right_bounds).map(|[start, end]| [start.get(), end.get()]),
        )),
        (
            SketchGeometryDefinition::Parabola {
                vertex: left_vertex,
                axis_angle: left_axis_angle,
                focal_length: left_focal_length,
                bounds: left_bounds,
            },
            SketchGeometryDefinition::Parabola {
                vertex: right_vertex,
                axis_angle: right_axis_angle,
                focal_length: right_focal_length,
                bounds: right_bounds,
            },
        ) => Ok((
            left_vertex.get(),
            left_axis_angle.get(),
            left_focal_length.get(),
            (*left_bounds).map(|[start, end]| [start.get(), end.get()]),
        ) == (
            right_vertex.get(),
            right_axis_angle.get(),
            right_focal_length.get(),
            (*right_bounds).map(|[start, end]| [start.get(), end.get()]),
        )),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::geometry_equal;
    use crate::geometry::pcurve::PcurveNurbs;
    use crate::math::Point2;
    use crate::scalar::{Angle, Length};
    use crate::sketches::{EllipseRadii, SketchGeometry, SketchGeometryDefinition};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn nurbs(points: Vec<Point2>, weights: Option<Vec<f64>>) -> SketchGeometry {
        SketchGeometry::nurbs(
            PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                points,
                weights,
                false,
            )
            .expect("fixture pcurve construction admission")
            .unwrap(),
        )
    }

    fn reference(document: Option<&str>, object: &str, selectors: &[&str]) -> SketchGeometry {
        SketchGeometry::try_from(SketchGeometryDefinition::ExternalReference {
            document: document.map(str::to_owned),
            object: cadmpeg_core::text::NonBlankString::try_from(object)
                .expect("nonblank external object"),
            subelements: selectors.iter().map(|text| (*text).to_owned()).collect(),
        })
        .unwrap()
    }

    fn text(family: &str, width_factor: Option<f64>, height: f64) -> SketchGeometry {
        SketchGeometry::try_from(SketchGeometryDefinition::Text {
            text: cadmpeg_core::text::NonBlankString::try_from("label")
                .expect("nonblank sketch text"),
            font_family: cadmpeg_core::text::NonBlankString::try_from(family)
                .expect("nonblank font family"),
            font_weight: crate::sketches::SketchFontWeight::Regular,
            height: crate::scalar::Length::new(height).unwrap(),
            width_factor,
            placement: Some(crate::sketches::TextPlacement {
                anchor: Point2::new(2.0, 3.0),
                rotation: crate::scalar::Angle::new(0.5).unwrap(),
            }),
            horizontal_alignment: None,
            vertical_alignment: None,
        })
        .unwrap()
    }

    fn analytic_geometry(definition: SketchGeometryDefinition) -> SketchGeometry {
        SketchGeometry::try_from(definition).expect("valid analytic geometry fixture")
    }

    #[test]
    fn analytic_points_and_lines_compare_each_field_exactly() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let point = |position| analytic_geometry(SketchGeometryDefinition::Point { position });
        let origin = point(Point2::new(0.0, -0.0));
        let signed_zero_origin = point(Point2::new(-0.0, 0.0));
        let displaced_point = point(Point2::new(0.0, 1.0));
        assert!(geometry_equal(&ctx, &origin, &signed_zero_origin).unwrap());
        assert!(!geometry_equal(&ctx, &origin, &displaced_point).unwrap());

        let line = |start, end| analytic_geometry(SketchGeometryDefinition::Line { start, end });
        let start = Point2::new(-1.0, 0.0);
        let end = Point2::new(2.0, 3.0);
        let line_value = line(start, end);
        let same_line = line(start, end);
        assert!(geometry_equal(&ctx, &line_value, &same_line).unwrap());
        assert!(!geometry_equal(&ctx, &line_value, &line(Point2::new(-2.0, 0.0), end),).unwrap());
        assert!(!geometry_equal(&ctx, &line_value, &line(start, Point2::new(2.0, 4.0)),).unwrap());

        let reference_line = |origin, direction| {
            analytic_geometry(SketchGeometryDefinition::ReferenceLine { origin, direction })
        };
        let reference_value = reference_line(Point2::new(1.0, 2.0), Point2::new(1.0, 0.5));
        let same_reference = reference_line(Point2::new(1.0, 2.0), Point2::new(1.0, 0.5));
        assert!(geometry_equal(&ctx, &reference_value, &same_reference).unwrap());
        assert!(!geometry_equal(
            &ctx,
            &reference_value,
            &reference_line(Point2::new(2.0, 2.0), Point2::new(1.0, 0.5)),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &reference_value,
            &reference_line(Point2::new(1.0, 2.0), Point2::new(1.0, 0.75)),
        )
        .unwrap());

        assert!(!geometry_equal(&ctx, &origin, &line_value).unwrap());
        assert!(!geometry_equal(&ctx, &line_value, &reference_value).unwrap());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn analytic_curves_compare_radii_angles_and_bounds_exactly() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let center = Point2::new(1.0, 2.0);

        let circle = |center, radius| {
            analytic_geometry(SketchGeometryDefinition::Circle {
                center,
                radius: Length::new(radius).unwrap(),
            })
        };
        let circle_value = circle(center, 2.0);
        assert!(geometry_equal(&ctx, &circle_value, &circle(center, 2.0)).unwrap());
        assert!(
            !geometry_equal(&ctx, &circle_value, &circle(Point2::new(2.0, 2.0), 2.0),).unwrap()
        );
        assert!(!geometry_equal(&ctx, &circle_value, &circle(center, 3.0)).unwrap());

        let arc = |center, radius, start_angle, end_angle| {
            analytic_geometry(SketchGeometryDefinition::Arc {
                center,
                radius: Length::new(radius).unwrap(),
                start_angle: Angle::new(start_angle).unwrap(),
                end_angle: Angle::new(end_angle).unwrap(),
            })
        };
        let arc_value = arc(center, 2.0, 0.25, 1.5);
        assert!(geometry_equal(&ctx, &arc_value, &arc(center, 2.0, 0.25, 1.5)).unwrap());
        assert!(geometry_equal(
            &ctx,
            &arc(center, 2.0, 0.0, 1.5),
            &arc(center, 2.0, -0.0, 1.5),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &arc_value,
            &arc(Point2::new(2.0, 2.0), 2.0, 0.25, 1.5),
        )
        .unwrap());
        assert!(!geometry_equal(&ctx, &arc_value, &arc(center, 3.0, 0.25, 1.5)).unwrap());
        assert!(!geometry_equal(&ctx, &arc_value, &arc(center, 2.0, 0.5, 1.5)).unwrap());
        assert!(!geometry_equal(&ctx, &arc_value, &arc(center, 2.0, 0.25, 1.75)).unwrap());

        let ellipse = |center, major_angle, major_radius, minor_radius, bounds| {
            analytic_geometry(SketchGeometryDefinition::Ellipse {
                center,
                major_angle: Angle::new(major_angle).unwrap(),
                radii: EllipseRadii {
                    major_radius: Length::new(major_radius).unwrap(),
                    minor_radius: Length::new(minor_radius).unwrap(),
                },
                bounds,
            })
        };
        let ellipse_bounds = Some([Angle::new(0.0).unwrap(), Angle::new(1.5).unwrap()]);
        let ellipse_value = ellipse(center, 0.25, 4.0, 2.0, ellipse_bounds);
        assert!(geometry_equal(
            &ctx,
            &ellipse_value,
            &ellipse(center, 0.25, 4.0, 2.0, ellipse_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &ellipse_value,
            &ellipse(Point2::new(2.0, 2.0), 0.25, 4.0, 2.0, ellipse_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &ellipse_value,
            &ellipse(center, 0.5, 4.0, 2.0, ellipse_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &ellipse_value,
            &ellipse(center, 0.25, 5.0, 2.0, ellipse_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &ellipse_value,
            &ellipse(center, 0.25, 4.0, 1.5, ellipse_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &ellipse_value,
            &ellipse(
                center,
                0.25,
                4.0,
                2.0,
                Some([Angle::new(0.0).unwrap(), Angle::new(1.75).unwrap()]),
            ),
        )
        .unwrap());
        assert!(
            !geometry_equal(&ctx, &ellipse_value, &ellipse(center, 0.25, 4.0, 2.0, None),).unwrap()
        );

        let hyperbola = |center, major_angle, major_radius, minor_radius, bounds| {
            analytic_geometry(SketchGeometryDefinition::Hyperbola {
                center,
                major_angle: Angle::new(major_angle).unwrap(),
                major_radius: Length::new(major_radius).unwrap(),
                minor_radius: Length::new(minor_radius).unwrap(),
                bounds,
            })
        };
        let hyperbola_bounds = Some([-2.0, 2.0]);
        let hyperbola_value = hyperbola(center, 0.25, 4.0, 2.0, hyperbola_bounds);
        assert!(geometry_equal(
            &ctx,
            &hyperbola_value,
            &hyperbola(center, 0.25, 4.0, 2.0, hyperbola_bounds),
        )
        .unwrap());
        assert!(geometry_equal(
            &ctx,
            &hyperbola(center, 0.25, 4.0, 2.0, Some([-0.0, 2.0]),),
            &hyperbola(center, 0.25, 4.0, 2.0, Some([0.0, 2.0]),),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &hyperbola_value,
            &hyperbola(Point2::new(2.0, 2.0), 0.25, 4.0, 2.0, hyperbola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &hyperbola_value,
            &hyperbola(center, 0.5, 4.0, 2.0, hyperbola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &hyperbola_value,
            &hyperbola(center, 0.25, 5.0, 2.0, hyperbola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &hyperbola_value,
            &hyperbola(center, 0.25, 4.0, 1.5, hyperbola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &hyperbola_value,
            &hyperbola(center, 0.25, 4.0, 2.0, Some([-2.0, 1.5])),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &hyperbola_value,
            &hyperbola(center, 0.25, 4.0, 2.0, None),
        )
        .unwrap());

        let parabola = |vertex, axis_angle, focal_length, bounds| {
            analytic_geometry(SketchGeometryDefinition::Parabola {
                vertex,
                axis_angle: Angle::new(axis_angle).unwrap(),
                focal_length: Length::new(focal_length).unwrap(),
                bounds,
            })
        };
        let parabola_bounds = Some([-1.0, 1.0]);
        let parabola_value = parabola(center, 0.25, 2.0, parabola_bounds);
        assert!(geometry_equal(
            &ctx,
            &parabola_value,
            &parabola(center, 0.25, 2.0, parabola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &parabola_value,
            &parabola(Point2::new(2.0, 2.0), 0.25, 2.0, parabola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &parabola_value,
            &parabola(center, 0.5, 2.0, parabola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &parabola_value,
            &parabola(center, 0.25, 3.0, parabola_bounds),
        )
        .unwrap());
        assert!(!geometry_equal(
            &ctx,
            &parabola_value,
            &parabola(center, 0.25, 2.0, Some([-1.0, 1.5])),
        )
        .unwrap());
        assert!(
            !geometry_equal(&ctx, &parabola_value, &parabola(center, 0.25, 2.0, None),).unwrap()
        );

        assert!(!geometry_equal(&ctx, &circle_value, &arc_value).unwrap());
        assert!(!geometry_equal(&ctx, &ellipse_value, &hyperbola_value).unwrap());
        assert!(!geometry_equal(&ctx, &hyperbola_value, &parabola_value).unwrap());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn sketch_geometry_comparison_preserves_nurbs_row_and_text_refusals() {
        let curve = nurbs(
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            Some(vec![1.0, 2.0]),
        );
        let referenced = reference(Some("document"), "object", &["first", "other"]);
        let text = text("font", Some(1.0), 2.0);
        let native = SketchGeometry::try_from(SketchGeometryDefinition::Native {
            native_kind: cadmpeg_core::text::NonBlankString::try_from("native")
                .expect("nonblank native kind"),
        })
        .unwrap();
        // Four knots precede two pole rows. Eight document bytes and six object bytes precede selector scans.
        for (geometry, cap, operation) in [
            (&curve, 0, "validation predicate scan"),
            (&curve, 4, "validation predicate scan"),
            (&curve, 5, "validation predicate scan"),
            (&referenced, 0, "compare sketch geometry text"),
            (&referenced, 8, "compare sketch geometry text"),
            (&referenced, 14, "validation predicate scan"),
            (&referenced, 15, "compare sketch geometry text"),
            (&referenced, 20, "validation predicate scan"),
            (&text, 0, "compare sketch geometry text"),
            (&text, 5, "compare sketch geometry text"),
            (&native, 0, "compare sketch geometry text"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = geometry_equal(&ctx, geometry, geometry)
            else {
                panic!("geometry equality must refuse");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, operation);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }

    #[test]
    fn sketch_geometry_comparison_matches_exact_typed_equality_without_storage() {
        let points = vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)];
        let values = [
            nurbs(points.clone(), None),
            nurbs(points.clone(), Some(vec![1.0, 1.0])),
            nurbs(points, Some(vec![1.0, 2.0])),
            reference(None, "object", &["first", "other"]),
            reference(Some("document"), "object", &["first", "other"]),
            reference(None, "object", &["other", "first"]),
            SketchGeometry::try_from(SketchGeometryDefinition::Native {
                native_kind: cadmpeg_core::text::NonBlankString::try_from("native")
                    .expect("nonblank native kind"),
            })
            .unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .unwrap(),
            text("font", None, 2.0),
            text("font", Some(1.0), 2.0),
            text("other", None, 2.0),
            text("font", None, 3.0),
        ];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for left in &values {
            for right in &values {
                assert_eq!(geometry_equal(&ctx, left, right).unwrap(), left == right);
            }
        }
        ctx.finish_session().unwrap();
    }

    #[test]
    fn sketch_geometry_comparison_short_circuits_before_later_selectors() {
        let left = reference(None, "object", &["first", "later"]);
        let right = reference(None, "object", &["other", "later"]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Six object bytes, one selector visit and the first differing selector byte.
        policy.limits.max_work_units = 12;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(!geometry_equal(&ctx, &left, &right).unwrap());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn sketch_text_equality_admits_only_the_visited_prefix() {
        for (left, right, cap, expected) in [
            ("same", "longer", 0, false),
            ("alpha", "other", 1, false),
            ("a", "a", 1, true),
            ("", "", 0, true),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert_eq!(
                super::text_equal(&ctx, left, right, "sketch prefix").unwrap(),
                expected
            );
            ctx.finish_session().unwrap();
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) =
            super::text_equal(&ctx, "aa", "ab", "sketch prefix")
        else {
            panic!("second compared byte must refuse");
        };
        assert_eq!((limit.used, limit.additional), (1, 1));
        assert_eq!(limit.operation, "sketch prefix");
        assert!(
            matches!(super::text_equal(&ctx, "", "", "sketch reentry"), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }

    #[test]
    fn sketch_analytic_equality_needs_no_variable_work() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let point = |position| analytic_geometry(SketchGeometryDefinition::Point { position });
        let left = point(Point2::new(0.0, -0.0));
        let same = point(Point2::new(-0.0, 0.0));
        let other = point(Point2::new(0.0, 1.0));
        assert!(geometry_equal(&ctx, &left, &same).unwrap());
        assert!(!geometry_equal(&ctx, &left, &other).unwrap());
        ctx.finish_session().unwrap();
    }
}
