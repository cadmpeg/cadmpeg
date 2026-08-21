// SPDX-License-Identifier: Apache-2.0
//! Pattern-form projection.

use crate::classification::NativeClassKind;
use crate::records::Feature;
use cadmpeg_ir::features::{
    Angle, FeatureDefinition, FeatureId, Length, PathRef, PatternForm, PatternKind, PatternSeed,
};
use std::collections::HashMap;

use crate::history::classify::feature_input_class;
use crate::history::literals::{
    parse_point3_mm, parse_positive_angle_rad, parse_positive_dimension_length_mm,
    parse_valid_direction,
};

pub(crate) fn pattern_form(feature: &Feature) -> Option<PatternForm> {
    let parse = |form: &str| match form.to_ascii_lowercase().as_str() {
        "linear" | "linearpattern" | "lpattern" => Some(PatternForm::Linear),
        "circular" | "circularpattern" | "cirpattern" => Some(PatternForm::Circular),
        "crvpattern" | "curvepattern" | "curvedrivenpattern" => Some(PatternForm::CurveDriven),
        "mirror" => Some(PatternForm::Mirror),
        _ => None,
    };
    if feature_input_class(feature, NativeClassKind::LinearPattern) {
        return Some(PatternForm::Linear);
    }
    if feature_input_class(feature, NativeClassKind::CircularPattern) {
        return Some(PatternForm::Circular);
    }
    if feature_input_class(feature, NativeClassKind::CurvePattern) {
        return Some(PatternForm::CurveDriven);
    }
    if let Some(form) = parse(&feature.kind) {
        return Some(form);
    }
    if feature.xml_tag.eq_ignore_ascii_case("Mirror") {
        return Some(PatternForm::Mirror);
    }
    feature
        .xml_tag
        .eq_ignore_ascii_case("Pattern")
        .then(|| feature.properties.get("PatternType"))
        .flatten()
        .and_then(|form| parse(form))
}

pub(crate) fn project_pattern(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
    native_by_source: &HashMap<&str, &str>,
) -> FeatureDefinition {
    let form = pattern_form(feature);
    let seeds = match feature.properties.get("Seeds") {
        Some(seeds) => seeds
            .split(',')
            .map(str::trim)
            .map(|source| by_source.get(source).cloned().map(PatternSeed::Feature))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let resolved = form.and_then(|form| {
        Some(match form {
            PatternForm::Linear => PatternKind::Linear {
                direction: match feature.properties.get("Direction") {
                    Some(value) => Some(parse_valid_direction(value)?),
                    None => None,
                },
                spacing: Length(parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?),
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
                        Some(cadmpeg_ir::features::LinearPatternDirection {
                            direction: parse_valid_direction(direction)?,
                            spacing: Length(parse_positive_dimension_length_mm(spacing)?),
                            count: parse_count(count)?,
                        })
                    }
                    _ => None,
                },
            },
            PatternForm::Circular => PatternKind::Circular {
                axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
                axis_dir: parse_valid_direction(feature.properties.get("AxisDirection")?)?,
                angle: Angle(
                    feature
                        .parameters
                        .get("Angle")
                        .and_then(|value| parse_positive_angle_rad(value))?,
                ),
                count: parse_count(feature.parameters.get("Count")?)?,
            },
            PatternForm::CurveDriven => PatternKind::CurveDriven {
                path: feature.properties.get("Path").map(|source| {
                    PathRef::Native(
                        native_by_source
                            .get(source.as_str())
                            .map_or_else(|| source.clone(), |id| (*id).to_string()),
                    )
                }),
                spacing: Length(parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?),
                count: parse_count(
                    feature
                        .parameters
                        .get("Count")
                        .or_else(|| feature.parameters.get("D1"))?,
                )?,
            },
            PatternForm::Mirror => PatternKind::Mirror {
                plane_origin: parse_point3_mm(feature.properties.get("PlaneOrigin")?)?,
                plane_normal: parse_valid_direction(feature.properties.get("PlaneNormal")?)?,
            },
            PatternForm::Scale | PatternForm::Composite => return None,
        })
    });
    let seeds_required = !matches!(form, Some(PatternForm::Linear | PatternForm::CurveDriven));
    let pattern = resolved
        .filter(|_| !seeds_required || !seeds.is_empty())
        .unwrap_or(PatternKind::Unresolved { form });
    FeatureDefinition::Pattern { seeds, pattern }
}

pub(crate) fn parse_count(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|count| *count > 0)
}
