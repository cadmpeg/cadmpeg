// SPDX-License-Identifier: Apache-2.0
//! Pattern-form projection.

use crate::classification::NativeClassKind;
use crate::records::Feature;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    patterns::{PatternKind, PatternSeed, PatternTransform},
    FeatureDefinition, FeatureId, FeatureOperation, PathRef,
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
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &HashMap<String, FeatureId>,
    native_by_source: &HashMap<String, &str>,
) -> Result<FeatureDefinition, CodecError> {
    const OPERATION: &str = "project SLDPRT pattern seeds";
    let form = pattern_form(feature);
    let mut seeds = Vec::new();
    if let Some(source_seeds) = feature.properties.get("Seeds") {
        for source in source_seeds.split(',').map(str::trim) {
            ctx.charge_work(1, OPERATION)?;
            let Some(id) = by_source.get(source) else {
                seeds.clear();
                break;
            };
            let copied = crate::text_admission::format_retained(ctx, format_args!("{}", id.as_str()), OPERATION)?;
            let id = FeatureId::mint(copied).map_err(CodecError::malformed)?;
            ctx.reserve_collection_vec(&mut seeds, 1, OPERATION)?;
            seeds.push(PatternSeed::Feature(id));
        }
    }
    let mut curve_path = if form == Some(NativePatternClass::CurveDriven) {
        feature.properties.get("Path").map(|source| {
            let text = native_by_source.get(source.as_str()).copied().unwrap_or(source);
            crate::text_admission::format_retained(ctx, format_args!("{text}"), "retain SLDPRT pattern path")
                .map(PathRef::Native)
        }).transpose()?
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
                        })
                    }
                    _ => None,
                },
            })
            .ok()?,
            NativePatternClass::Circular => PatternKind::new(PatternTransform::Circular {
                axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
                axis_dir: parse_valid_direction(feature.properties.get("AxisDirection")?)?,
                angle: feature
                    .parameters
                    .get("Angle")
                    .and_then(|value| parse_positive_angle_rad(value))?,
                count: parse_count(feature.parameters.get("Count")?)?,
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
    Ok(FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }))
}

pub(crate) fn parse_count(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|count| *count > 0)
}

#[cfg(test)]
mod tests {
    use super::project_pattern;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FeatureId;
    use std::collections::HashMap;

    #[test]
    fn pattern_seed_projection_refuses_collection_limit() {
        let mut feature = crate::history::tests::feature("pattern", None, 0);
        feature.properties.insert(cadmpeg_core::nonblank_literal!("Seeds"), "source".to_owned());
        let by_source = HashMap::from([(
            "source".to_owned(),
            FeatureId::mint("synthetic:test:id#seed").unwrap(),
        )]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"pattern", &arena, &policy).unwrap();
        let error = project_pattern(&ctx, &feature, &by_source, &HashMap::new()).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }

    #[test]
    fn pattern_path_projection_refuses_retained_limit() {
        let mut feature = crate::history::tests::feature("pattern", None, 0);
        feature.kind = "CurvePattern".to_owned();
        feature.properties.insert(cadmpeg_core::nonblank_literal!("Path"), "native-path".to_owned());
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"pattern", &arena, &policy).unwrap();
        let error = project_pattern(&ctx, &feature, &HashMap::new(), &HashMap::new()).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }
}
