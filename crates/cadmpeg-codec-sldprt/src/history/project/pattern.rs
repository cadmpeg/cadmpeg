// SPDX-License-Identifier: Apache-2.0
//! Pattern-form projection.

use crate::classification::NativeClassKind;
use crate::records::{
    Feature, CIRCULAR_PATTERN_ANGLE_DIMENSION, CIRCULAR_PATTERN_COUNT_DIMENSION,
    CIRCULAR_PATTERN_SEED_SOURCES,
};
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::profiles::PathRef;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use std::collections::HashMap;

use crate::history::classify::feature_input_class;
use crate::history::literals::{
    parse_point3_mm, parse_positive_angle_rad, parse_positive_dimension_length_mm,
    parse_valid_direction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::history) enum NativePatternClass {
    Linear,
    Circular,
    CurveDriven,
    Mirror,
}

pub(in crate::history) fn pattern_form(feature: &Feature) -> Option<NativePatternClass> {
    let parse = |form: &str| {
        if ["linear", "linearpattern", "lpattern"]
            .iter()
            .any(|name| form.eq_ignore_ascii_case(name))
        {
            Some(NativePatternClass::Linear)
        } else if ["circular", "circularpattern", "cirpattern"]
            .iter()
            .any(|name| form.eq_ignore_ascii_case(name))
        {
            Some(NativePatternClass::Circular)
        } else if ["crvpattern", "curvepattern", "curvedrivenpattern"]
            .iter()
            .any(|name| form.eq_ignore_ascii_case(name))
        {
            Some(NativePatternClass::CurveDriven)
        } else if form.eq_ignore_ascii_case("mirror") {
            Some(NativePatternClass::Mirror)
        } else {
            None
        }
    };
    if feature_input_class(feature, NativeClassKind::LinearPattern) {
        return Some(NativePatternClass::Linear);
    }
    if feature_input_class(feature, NativeClassKind::CircularPattern) {
        return Some(NativePatternClass::Circular);
    }
    if feature_input_class(feature, NativeClassKind::CurvePattern) {
        return Some(NativePatternClass::CurveDriven);
    }
    if let Some(form) = parse(&feature.kind) {
        return Some(form);
    }
    if feature.xml_tag.eq_ignore_ascii_case("Mirror") {
        return Some(NativePatternClass::Mirror);
    }
    feature
        .xml_tag
        .eq_ignore_ascii_case("Pattern")
        .then(|| feature.properties.get("PatternType"))
        .flatten()
        .and_then(|form| parse(form))
}

pub(super) fn project_pattern(
    feature: &Feature,
    by_source: &HashMap<String, FeatureId>,
    native_by_source: &HashMap<String, &str>,
) -> FeatureDefinition {
    let form = pattern_form(feature);
    let mut seeds = Vec::new();
    if let Some(source_seeds) = feature
        .properties
        .get("Seeds")
        .or_else(|| feature.properties.get(CIRCULAR_PATTERN_SEED_SOURCES))
    {
        for source in source_seeds.split(',').map(str::trim) {
            let Some(id) = by_source.get(source) else {
                seeds.clear();
                break;
            };
            seeds.push(PatternSeed::Feature(id.clone()));
        }
    }
    let mut curve_path = if form == Some(NativePatternClass::CurveDriven) {
        feature.properties.get("Path").map(|source| {
            PathRef::Native(
                native_by_source
                    .get(source.as_str())
                    .copied()
                    .unwrap_or(source)
                    .to_owned(),
            )
        })
    } else {
        None
    };
    let resolved = form.and_then(|form| {
        Some(match form {
            NativePatternClass::Linear => PatternKind::new(PatternTransform::Linear {
                direction: match feature.properties.get("Direction") {
                    Some(value) => Some(parse_valid_direction(value)?),
                    None => None,
                },
                spacing: parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?,
                count: parse_count(
                    feature
                        .parameters
                        .get("Count")
                        .or_else(|| feature.parameters.get("D1"))?,
                )?,
                second: match (
                    feature.properties.get("Direction2"),
                    feature.parameters.get("D4"),
                    feature.parameters.get("D2"),
                ) {
                    (Some(direction), Some(spacing), Some(count)) => {
                        Some(cadmpeg_ir::features::patterns::LinearPatternDirection {
                            direction: parse_valid_direction(direction)?,
                            spacing: parse_positive_dimension_length_mm(spacing)?,
                            count: parse_count(count)?,
                            symmetric: false,
                        })
                    }
                    _ => None,
                },
                omitted_instances: Vec::new(),
                symmetric: false,
            })
            .ok()?,
            NativePatternClass::Circular => PatternKind::new(PatternTransform::Circular {
                axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
                axis_dir: parse_valid_direction(feature.properties.get("AxisDirection")?)?,
                angle: dimension(feature, "Angle", CIRCULAR_PATTERN_ANGLE_DIMENSION)
                    .and_then(parse_positive_angle_rad)?,
                count: parse_count(dimension(
                    feature,
                    "Count",
                    CIRCULAR_PATTERN_COUNT_DIMENSION,
                )?)?,
            })
            .ok()?,
            NativePatternClass::CurveDriven => PatternKind::new(PatternTransform::CurveDriven {
                path: curve_path.take(),
                spacing: parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?,
                count: parse_count(
                    feature
                        .parameters
                        .get("Count")
                        .or_else(|| feature.parameters.get("D1"))?,
                )?,
            })
            .ok()?,
            NativePatternClass::Mirror => PatternKind::new(PatternTransform::Mirror {
                plane_origin: parse_point3_mm(feature.properties.get("PlaneOrigin")?)?,
                plane_normal: parse_valid_direction(feature.properties.get("PlaneNormal")?)?,
            })
            .ok()?,
        })
    });
    let seeds_required = !matches!(
        form,
        Some(NativePatternClass::Linear | NativePatternClass::CurveDriven)
    );
    let pattern = resolved
        .filter(|_| !seeds_required || !seeds.is_empty())
        .unwrap_or(match form {
            None => PatternKind::UNRESOLVED,
            Some(NativePatternClass::Linear) => PatternKind::UNRESOLVED_LINEAR,
            Some(NativePatternClass::Circular) => PatternKind::UNRESOLVED_CIRCULAR,
            Some(NativePatternClass::CurveDriven) => PatternKind::UNRESOLVED_CURVE_DRIVEN,
            Some(NativePatternClass::Mirror) => PatternKind::UNRESOLVED_MIRROR,
        });
    FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern })
}

/// The value of `feature`'s parameter `name`, or of the parameter its
/// property `named_by` names.
fn dimension<'a>(feature: &'a Feature, name: &str, named_by: &str) -> Option<&'a str> {
    feature
        .parameters
        .get(name)
        .or_else(|| {
            feature
                .properties
                .get(named_by)
                .and_then(|dimension| feature.parameters.get(dimension.as_str()))
        })
        .map(String::as_str)
}

pub(crate) fn parse_count(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|count| *count > 0)
}
