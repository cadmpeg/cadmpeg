// SPDX-License-Identifier: Apache-2.0
//! Exact sketch geometry comparison with caller-owned scan admission.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
use super::super::scans::all;

pub(super) fn text_equal(ctx: &DecodeContext<'_>, left: &str, right: &str, operation: &'static str) -> Result<bool, CodecError> {
    if left.len() != right.len() { return Ok(false); }
    ctx.charge_work(u64_from_index(left.len()), operation)?;
    Ok(left == right)
}

fn nurbs_equal(ctx: &DecodeContext<'_>, left: &PcurveNurbs, right: &PcurveNurbs) -> Result<bool, CodecError> {
    if left.degree() != right.degree() || left.knots().len() != right.knots().len() { return Ok(false); }
    if !all(ctx, left.knots().iter().zip(right.knots()), |(left, right)| Ok(left == right))? { return Ok(false); }
    let poles_equal = match (left.pole_rows(), right.pole_rows()) {
        (PcurveNurbsPoles::Polynomial { points: left }, PcurveNurbsPoles::Polynomial { points: right }) =>
            left.len() == right.len() && all(ctx, left.iter().zip(right), |(left, right)| Ok(left == right))?,
        (PcurveNurbsPoles::Rational { points: left }, PcurveNurbsPoles::Rational { points: right }) =>
            left.len() == right.len() && all(ctx, left.iter().zip(right), |(left, right)| Ok(left == right))?,
        _ => false,
    };
    Ok(poles_equal && left.periodic() == right.periodic())
}

pub(super) fn geometry_equal(ctx: &DecodeContext<'_>, left: &SketchGeometry, right: &SketchGeometry) -> Result<bool, CodecError> {
    match (left.definition(), right.definition()) {
        (SketchGeometryDefinition::Nurbs { curve: left }, SketchGeometryDefinition::Nurbs { curve: right }) => nurbs_equal(ctx, left, right),
        (SketchGeometryDefinition::Text { text: left_text, font_family: left_family, font_weight: left_weight, height: left_height, width_factor: left_width, placement: left_placement, horizontal_alignment: left_horizontal, vertical_alignment: left_vertical },
         SketchGeometryDefinition::Text { text: right_text, font_family: right_family, font_weight: right_weight, height: right_height, width_factor: right_width, placement: right_placement, horizontal_alignment: right_horizontal, vertical_alignment: right_vertical }) => Ok(
            text_equal(ctx, left_text.as_str(), right_text.as_str(), "compare sketch geometry text")?
                && text_equal(ctx, left_family.as_str(), right_family.as_str(), "compare sketch geometry text")?
                && left_weight == right_weight && left_height == right_height && left_width == right_width
                && left_placement == right_placement && left_horizontal == right_horizontal && left_vertical == right_vertical
        ),
        (SketchGeometryDefinition::ExternalReference { document: left_document, object: left_object, subelements: left_elements },
         SketchGeometryDefinition::ExternalReference { document: right_document, object: right_object, subelements: right_elements }) => {
            let documents_equal = match (left_document.as_deref(), right_document.as_deref()) {
                (Some(left), Some(right)) => text_equal(ctx, left, right, "compare sketch geometry text")?,
                (None, None) => true,
                _ => false,
            };
            Ok(documents_equal
                && text_equal(ctx, left_object.as_str(), right_object.as_str(), "compare sketch geometry text")?
                && left_elements.len() == right_elements.len()
                && all(ctx, left_elements.iter().zip(right_elements), |(left, right)| text_equal(ctx, left, right, "compare sketch geometry text"))?
            )
        }
        (SketchGeometryDefinition::Native { native_kind: left }, SketchGeometryDefinition::Native { native_kind: right }) => text_equal(ctx, left.as_str(), right.as_str(), "compare sketch geometry text"),
        (SketchGeometryDefinition::Point { .. } | SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::ReferenceLine { .. } | SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Arc { .. } | SketchGeometryDefinition::Ellipse { .. } | SketchGeometryDefinition::Hyperbola { .. } | SketchGeometryDefinition::Parabola { .. }, _) => Ok(left == right),
        (SketchGeometryDefinition::Nurbs { .. } | SketchGeometryDefinition::Text { .. } | SketchGeometryDefinition::ExternalReference { .. } | SketchGeometryDefinition::Native { .. }, _) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::geometry_equal;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
    use crate::geometry::pcurve::PcurveNurbs;
    use crate::math::Point2;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn nurbs(points: Vec<Point2>, weights: Option<Vec<f64>>) -> SketchGeometry {
        SketchGeometry::nurbs(PcurveNurbs::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0], points, weights, false).unwrap())
    }

    fn reference(document: Option<&str>, object: &str, selectors: &[&str]) -> SketchGeometry {
        SketchGeometry::try_from(SketchGeometryDefinition::ExternalReference { document: document.map(str::to_owned), object: cadmpeg_core::text::NonBlankString::new(object).unwrap(), subelements: selectors.iter().map(|text| (*text).to_owned()).collect() }).unwrap()
    }

    fn text(family: &str, width_factor: Option<f64>, height: f64) -> SketchGeometry {
        SketchGeometry::try_from(SketchGeometryDefinition::Text {
            text: cadmpeg_core::text::NonBlankString::new("label").unwrap(), font_family: cadmpeg_core::text::NonBlankString::new(family).unwrap(), font_weight: crate::sketches::SketchFontWeight::Regular,
            height: crate::scalar::Length::new(height).unwrap(), width_factor,
            placement: Some(crate::sketches::TextPlacement { anchor: Point2::new(2.0, 3.0), rotation: crate::scalar::Angle::new(0.5).unwrap() }),
            horizontal_alignment: None, vertical_alignment: None,
        }).unwrap()
    }

    #[test]
    fn sketch_geometry_comparison_preserves_nurbs_row_and_text_refusals() {
        let curve = nurbs(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], Some(vec![1.0, 2.0]));
        let referenced = reference(Some("document"), "object", &["first", "other"]);
        let text = text("font", Some(1.0), 2.0);
        let native = SketchGeometry::try_from(SketchGeometryDefinition::Native { native_kind: cadmpeg_core::text::NonBlankString::new("native").unwrap() }).unwrap();
        // Four knots precede two pole rows. Eight document bytes and six object bytes precede selector scans.
        for (geometry, cap, operation) in [(&curve, 0, "validation predicate scan"), (&curve, 4, "validation predicate scan"), (&curve, 5, "validation predicate scan"), (&referenced, 0, "compare sketch geometry text"), (&referenced, 8, "compare sketch geometry text"), (&referenced, 14, "validation predicate scan"), (&referenced, 15, "compare sketch geometry text"), (&referenced, 20, "validation predicate scan"), (&text, 0, "compare sketch geometry text"), (&text, 5, "compare sketch geometry text"), (&native, 0, "compare sketch geometry text")] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = geometry_equal(&ctx, geometry, geometry) else { panic!("geometry equality must refuse"); };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, operation);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }

    #[test]
    fn sketch_geometry_comparison_matches_exact_typed_equality_without_storage() {
        let points = vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)];
        let values = [nurbs(points.clone(), None), nurbs(points.clone(), Some(vec![1.0, 1.0])), nurbs(points, Some(vec![1.0, 2.0])), reference(None, "object", &["first", "other"]), reference(Some("document"), "object", &["first", "other"]), reference(None, "object", &["other", "first"]), SketchGeometry::try_from(SketchGeometryDefinition::Native { native_kind: cadmpeg_core::text::NonBlankString::new("native").unwrap() }).unwrap(), SketchGeometry::try_from(SketchGeometryDefinition::Point { position: Point2::new(0.0, 0.0) }).unwrap(), text("font", None, 2.0), text("font", Some(1.0), 2.0), text("other", None, 2.0), text("font", None, 3.0)];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for left in &values {
            for right in &values { assert_eq!(geometry_equal(&ctx, left, right).unwrap(), left == right); }
        }
        ctx.finish_session().unwrap();
    }

    #[test]
    fn sketch_geometry_comparison_short_circuits_before_later_selectors() {
        let left = reference(None, "object", &["first", "later"]);
        let right = reference(None, "object", &["other", "later"]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Six object bytes, one selector visit and five bytes for the first selector.
        policy.limits.max_work_units = 12;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(!geometry_equal(&ctx, &left, &right).unwrap());
        ctx.finish_session().unwrap();
    }
}
