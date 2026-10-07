// SPDX-License-Identifier: Apache-2.0
//! Pattern-form projection.

use crate::classification::NativeClassKind;
use crate::records::Feature;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    patterns::{PatternKind, PatternSeed, PatternTransform},
    FeatureDefinition, FeatureOperation, PathRef,
};

use super::{
    copy_projected_feature_id, either_parameter, parameter_literal, property_literal,
    property_value,
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

/// The pattern form a native record names, for the writer.
pub(in crate::history) fn pattern_form(feature: &Feature) -> Option<NativePatternClass> {
    match pattern_form_with(feature, || {
        Ok::<_, std::convert::Infallible>(feature.properties.get("PatternType").map(String::as_str))
    }) {
        Ok(form) => form,
        Err(never) => match never {},
    }
}

/// The pattern form a native record names, with a charged property lookup.
pub(in crate::history) fn native_pattern_form(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<NativePatternClass>, CodecError> {
    pattern_form_with(feature, || property_value(ctx, feature, "PatternType"))
}

/// The pattern form from the native class, the kind token, the element tag,
/// and, for a generic pattern element, its `PatternType` property.
fn pattern_form_with<'f, E>(
    feature: &'f Feature,
    pattern_type: impl FnOnce() -> Result<Option<&'f str>, E>,
) -> Result<Option<NativePatternClass>, E> {
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
        return Ok(Some(NativePatternClass::Linear));
    }
    if feature_input_class(feature, NativeClassKind::CircularPattern) {
        return Ok(Some(NativePatternClass::Circular));
    }
    if feature_input_class(feature, NativeClassKind::CurvePattern) {
        return Ok(Some(NativePatternClass::CurveDriven));
    }
    if let Some(form) = parse(&feature.kind) {
        return Ok(Some(form));
    }
    if feature.xml_tag.eq_ignore_ascii_case("Mirror") {
        return Ok(Some(NativePatternClass::Mirror));
    }
    if !feature.xml_tag.eq_ignore_ascii_case("Pattern") {
        return Ok(None);
    }
    Ok(pattern_type()?.and_then(parse))
}

/// The pattern transform a native record states completely.
fn resolve_pattern(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    form: NativePatternClass,
    curve_path: Option<PathRef>,
) -> Result<Option<PatternKind>, CodecError> {
    let direction = |name| -> Result<_, CodecError> {
        Ok(property_literal(ctx, feature, name)?.and_then(parse_valid_direction))
    };
    let spacing = || -> Result<_, CodecError> {
        Ok(either_parameter(ctx, feature, "Spacing", "D3")?
            .and_then(parse_positive_dimension_length_mm))
    };
    let parse_count = |value: &str| -> Result<_, CodecError> {
        let value = ctx.trim_text(value, "trim SLDPRT pattern count")?;
        Ok(ctx
            .parse_text::<u32>(value, "parse SLDPRT pattern count")?
            .ok()
            .filter(|count| *count > 0))
    };
    let count = |name, positional| -> Result<_, CodecError> {
        let value = match ctx.get_btree_map(&feature.parameters, name, super::FEATURE_LITERAL)? {
            Some(value) => Some(value),
            None => ctx.get_btree_map(&feature.parameters, positional, super::FEATURE_LITERAL)?,
        };
        value
            .map(|value| parse_count(value))
            .transpose()
            .map(Option::flatten)
    };
    let transform = match form {
        NativePatternClass::Linear => {
            let direction = match property_literal(ctx, feature, "Direction")? {
                Some(value) => Some(require!(parse_valid_direction(value))),
                None => None,
            };
            let spacing = require!(spacing()?);
            let count = require!(count("Count", "D1")?);
            let second = match (
                property_literal(ctx, feature, "Direction2")?,
                parameter_literal(ctx, feature, "D4")?,
                ctx.get_btree_map(&feature.parameters, "D2", super::FEATURE_LITERAL)?,
            ) {
                (Some(direction), Some(spacing), Some(count)) => {
                    Some(cadmpeg_ir::features::patterns::LinearPatternDirection {
                        direction: require!(parse_valid_direction(direction)),
                        spacing: require!(parse_positive_dimension_length_mm(spacing)),
                        count: require!(parse_count(count)?),
                    })
                }
                _ => None,
            };
            PatternTransform::Linear {
                direction,
                spacing,
                count,
                second,
            }
        }
        NativePatternClass::Circular => PatternTransform::Circular {
            axis_origin: require!(
                property_literal(ctx, feature, "AxisOrigin")?.and_then(parse_point3_mm)
            ),
            axis_dir: require!(direction("AxisDirection")?),
            angle: require!(
                parameter_literal(ctx, feature, "Angle")?.and_then(parse_positive_angle_rad)
            ),
            count: require!(ctx
                .get_btree_map(&feature.parameters, "Count", super::FEATURE_LITERAL)?
                .map(|value| parse_count(value))
                .transpose()?
                .flatten()),
        },
        NativePatternClass::CurveDriven => PatternTransform::CurveDriven {
            path: curve_path,
            spacing: require!(spacing()?),
            count: require!(count("Count", "D1")?),
        },
        NativePatternClass::Mirror => PatternTransform::Mirror {
            plane_origin: require!(
                property_literal(ctx, feature, "PlaneOrigin")?.and_then(parse_point3_mm)
            ),
            plane_normal: require!(direction("PlaneNormal")?),
        },
    };
    Ok(PatternKind::new(transform).ok())
}

pub(super) fn project_pattern(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &super::NeutralByKey<'_, '_>,
    native_by_source: &HashMap<&str, &str>,
) -> Result<FeatureDefinition, CodecError> {
    const OPERATION: &str = "project SLDPRT pattern seeds";
    let form = native_pattern_form(ctx, feature)?;
    let mut seeds = Vec::new();
    if let Some(source_seeds) = property_value(ctx, feature, "Seeds")? {
        let mut characters = source_seeds.char_indices();
        let mut start = 0;
        let mut finished = false;
        while !finished {
            let delimiter = ctx.find_map(
                &mut characters,
                |(offset, character)| Ok((character == ',').then_some(offset)),
                "scan SLDPRT pattern seed delimiters",
            )?;
            let end = delimiter.unwrap_or(source_seeds.len());
            finished = delimiter.is_none();
            let source = ctx.trim_text(&source_seeds[start..end], "trim SLDPRT pattern seed")?;
            start = end + usize::from(!finished);
            let Some(id) = ctx.get_hash_map(by_source, source, "look up SLDPRT hash key")? else {
                ctx.clear_vec(&mut seeds, OPERATION)?;
                break;
            };
            let id = copy_projected_feature_id(ctx, id)?;
            ctx.push_vec(&mut seeds, PatternSeed::Feature(id), OPERATION)?;
        }
    }
    let curve_path = match (form, property_value(ctx, feature, "Path")?) {
        (Some(NativePatternClass::CurveDriven), Some(source)) => {
            let text = ctx
                .get_hash_map(native_by_source, source, "look up SLDPRT hash key")?
                .copied()
                .unwrap_or(source);
            Some(PathRef::Native(
                ctx.copy_retained_text(text, "retain SLDPRT pattern path")?,
            ))
        }
        _ => None,
    };
    let resolved = match form {
        Some(form) => resolve_pattern(ctx, feature, form, curve_path)?,
        None => None,
    };
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
    Ok(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds,
        pattern,
    }))
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
        feature.properties.insert(
            cadmpeg_core::nonblank_literal!("Seeds"),
            "source".to_owned(),
        );
        let seed = FeatureId::mint("synthetic:test:id#seed").unwrap();
        let by_source = HashMap::from([("source", &seed)]);
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
        feature.properties.insert(
            cadmpeg_core::nonblank_literal!("Path"),
            "native-path".to_owned(),
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"pattern", &arena, &policy).unwrap();
        let error = project_pattern(&ctx, &feature, &HashMap::new(), &HashMap::new()).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }
}
