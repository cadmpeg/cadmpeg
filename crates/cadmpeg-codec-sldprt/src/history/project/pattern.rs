// SPDX-License-Identifier: Apache-2.0
//! Pattern-form projection.

use crate::classification::NativeClassKind;
use crate::records::Feature;
use cadmpeg_ir::{
    features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureId, FeatureOperation, PathRef,
    },
    scalar::{PositiveAngle, PositiveLength},
};
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
    let parse = |form: &str| match form.to_ascii_lowercase().as_str() {
        "linear" | "linearpattern" | "lpattern" => Some(NativePatternClass::Linear),
        "circular" | "circularpattern" | "cirpattern" => Some(NativePatternClass::Circular),
        "crvpattern" | "curvepattern" | "curvedrivenpattern" => {
            Some(NativePatternClass::CurveDriven)
        }
        "mirror" => Some(NativePatternClass::Mirror),
        _ => None,
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
            NativePatternClass::Linear => PatternKind::new(PatternTransform::Linear {
                direction: match feature.properties.get("Direction") {
                    Some(value) => Some(cadmpeg_ir::features::FeatureDirection3::new(
                        parse_valid_direction(value)?,
                    )?),
                    None => None,
                },
                spacing: PositiveLength::new(parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?)?,
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
                            direction: cadmpeg_ir::features::FeatureDirection3::new(
                                parse_valid_direction(direction)?,
                            )?,
                            spacing: PositiveLength::new(parse_positive_dimension_length_mm(
                                spacing,
                            )?)?,
                            count: parse_count(count)?,
                        })
                    }
                    _ => None,
                },
            })
            .ok()?,
            NativePatternClass::Circular => PatternKind::new(PatternTransform::Circular {
                axis_origin: cadmpeg_ir::features::FinitePoint3::new(parse_point3_mm(
                    feature.properties.get("AxisOrigin")?,
                )?)?,
                axis_dir: cadmpeg_ir::features::FeatureDirection3::new(parse_valid_direction(
                    feature.properties.get("AxisDirection")?,
                )?)?,
                angle: PositiveAngle::new(
                    feature
                        .parameters
                        .get("Angle")
                        .and_then(|value| parse_positive_angle_rad(value))?,
                )?,
                count: parse_count(feature.parameters.get("Count")?)?,
            })
            .ok()?,
            NativePatternClass::CurveDriven => PatternKind::new(PatternTransform::CurveDriven {
                path: feature.properties.get("Path").map(|source| {
                    PathRef::Native(
                        native_by_source
                            .get(source.as_str())
                            .map_or_else(|| source.clone(), |id| (*id).to_string()),
                    )
                }),
                spacing: PositiveLength::new(parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?)?,
                count: parse_count(
                    feature
                        .parameters
                        .get("Count")
                        .or_else(|| feature.parameters.get("D1"))?,
                )?,
            })
            .ok()?,
            NativePatternClass::Mirror => PatternKind::new(PatternTransform::Mirror {
                plane_origin: cadmpeg_ir::features::FinitePoint3::new(parse_point3_mm(
                    feature.properties.get("PlaneOrigin")?,
                )?)?,
                plane_normal: cadmpeg_ir::features::FeatureDirection3::new(parse_valid_direction(
                    feature.properties.get("PlaneNormal")?,
                )?)?,
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

pub(crate) fn parse_count(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|count| *count > 0)
}
