// SPDX-License-Identifier: Apache-2.0
//! Length, angle, vector, and parameter-literal parse/format helpers.

use cadmpeg_ir::features::{
    Angle, BooleanOp, ChamferSpec, DimensionDisplay, FaceMotion, FeatureDefinition, Length,
    ParameterValue, PatternKind,
};
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_LITERALS_VALID_PLANE_FRAME_E9: f64 = 1.0e-9;
const EPS_LITERALS_VALID_COORDINATE_FRAME_E9: f64 = 1.0e-9;
const EPS_LITERALS_PARSE_LENGTH_MM_E6: f64 = 1.0e-6;
const EPS_LITERALS_PARSE_LENGTH_MM_E7: f64 = 1.0e-7;
const EPS_LITERALS_FORMAT_F64_LITERAL_E6: f64 = 1.0e-6;

pub(crate) fn valid_plane_frame(normal: Vector3, u_axis: Vector3) -> bool {
    let normal_length = normal.norm();
    let u_length = u_axis.norm();
    normal_length.is_finite()
        && u_length.is_finite()
        && normal_length > f64::EPSILON
        && u_length > f64::EPSILON
        && normal.dot(u_axis).abs() <= EPS_LITERALS_VALID_PLANE_FRAME_E9 * normal_length * u_length
}

pub(crate) fn valid_coordinate_frame(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> bool {
    let finite_origin = [origin.x, origin.y, origin.z]
        .into_iter()
        .all(f64::is_finite);
    let unit = |axis: Vector3| (axis.norm() - 1.0).abs() <= EPS_LITERALS_VALID_COORDINATE_FRAME_E9;
    let cross = x_axis.cross(y_axis);
    finite_origin
        && unit(x_axis)
        && unit(y_axis)
        && unit(z_axis)
        && x_axis.dot(y_axis).abs() <= EPS_LITERALS_VALID_COORDINATE_FRAME_E9
        && x_axis.dot(z_axis).abs() <= EPS_LITERALS_VALID_COORDINATE_FRAME_E9
        && y_axis.dot(z_axis).abs() <= EPS_LITERALS_VALID_COORDINATE_FRAME_E9
        && cross.dot(z_axis) >= 1.0 - EPS_LITERALS_VALID_COORDINATE_FRAME_E9
}

pub(crate) fn valid_direction(direction: Vector3) -> bool {
    direction.norm().is_finite() && direction.norm() > f64::EPSILON
}

pub(crate) fn parse_length_mm(value: &str) -> Option<f64> {
    let value = value.trim();
    let (value, display_length) = value
        .strip_prefix(['R', 'r', '\u{2300}', '\u{00d8}'])
        .map_or((value, false), |value| (value.trim(), true));
    for (suffix, scale) in [
        ("uin", 25.4e-6),
        ("mil", 0.0254),
        ("mm", 1.0),
        ("cm", 10.0),
        ("in", 25.4),
        ("ft", 304.8),
        ("nm", EPS_LITERALS_PARSE_LENGTH_MM_E6),
        ("um", 1.0e-3),
        ("µm", 1.0e-3),
        ("μm", 1.0e-3),
        ("Å", EPS_LITERALS_PARSE_LENGTH_MM_E7),
        ("A", EPS_LITERALS_PARSE_LENGTH_MM_E7),
        ("m", 1000.0),
    ] {
        if let Some(number) = value.strip_suffix(suffix) {
            return number
                .trim()
                .parse::<f64>()
                .ok()
                .map(|value| value * scale)
                .filter(|value| value.is_finite());
        }
    }
    display_length
        .then(|| value.parse::<f64>().ok())
        .flatten()
        .filter(|value| value.is_finite())
}

pub(crate) fn parse_positive_length_mm(value: &str) -> Option<f64> {
    parse_length_mm(value).filter(|value| *value > 0.0)
}

pub(crate) fn parse_positive_dimension_length_mm(value: &str) -> Option<f64> {
    parse_positive_length_mm(value).or_else(|| {
        value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
    })
}

pub(crate) fn parse_dimension_length_mm(value: &str) -> Option<f64> {
    parse_length_mm(value).or_else(|| {
        value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    })
}

pub(crate) fn format_length_mm(value: f64) -> String {
    format!("{}mm", format_f64_literal(value))
}

pub(crate) fn parse_angle_rad(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Some(number) = value
        .strip_suffix("deg")
        .or_else(|| value.strip_suffix('\u{00b0}'))
    {
        return number
            .trim()
            .parse::<f64>()
            .ok()
            .map(f64::to_radians)
            .filter(|value| value.is_finite());
    }
    value
        .strip_suffix("rad")
        .and_then(|number| number.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

pub(crate) fn parse_positive_angle_rad(value: &str) -> Option<f64> {
    parse_angle_rad(value).filter(|value| *value > 0.0)
}

pub(crate) fn parse_bounded_angle_rad(value: &str) -> Option<f64> {
    parse_positive_angle_rad(value).filter(|value| *value < std::f64::consts::PI)
}

pub(crate) fn format_angle_rad(value: f64) -> String {
    format!("{}rad", format_f64_literal(value))
}

pub(crate) fn format_f64_literal(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude != 0.0 && !(EPS_LITERALS_FORMAT_F64_LITERAL_E6..1.0e15).contains(&magnitude) {
        format!("{value:e}")
    } else {
        value.to_string()
    }
}

pub(crate) fn parse_point3_mm(value: &str) -> Option<Point3> {
    let values = value
        .split(',')
        .map(|component| parse_length_mm(component.trim()))
        .collect::<Option<Vec<_>>>()?;
    (values.len() == 3).then(|| Point3::new(values[0], values[1], values[2]))
}

pub(crate) fn parse_vector3(value: &str) -> Option<Vector3> {
    let values = value
        .split(',')
        .map(|component| component.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (values.len() == 3).then(|| Vector3::new(values[0], values[1], values[2]))
}

pub(crate) fn parse_valid_direction(value: &str) -> Option<Vector3> {
    parse_vector3(value).filter(|value| valid_direction(*value))
}

pub(crate) fn parse_boolean_op(value: &str) -> Option<BooleanOp> {
    match value.to_ascii_lowercase().as_str() {
        "join" => Some(BooleanOp::Join),
        "cut" => Some(BooleanOp::Cut),
        "intersect" => Some(BooleanOp::Intersect),
        "newbody" | "new_body" => Some(BooleanOp::NewBody),
        _ => None,
    }
}

pub(crate) fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "True" => Some(true),
        "0" | "false" | "False" => Some(false),
        _ => None,
    }
}

pub(crate) fn parse_parameter_literal(expression: &str) -> Option<ParameterValue> {
    if dimension_display(expression).is_some() {
        return parse_dimension_display_length(expression)
            .map(|value| ParameterValue::Length(Length(value)));
    }
    let expression = expression.trim();
    if expression.eq_ignore_ascii_case("true") {
        return Some(ParameterValue::Boolean(true));
    }
    if expression.eq_ignore_ascii_case("false") {
        return Some(ParameterValue::Boolean(false));
    }
    if let Some(value) = parse_length_mm(expression) {
        return Some(ParameterValue::Length(Length(value)));
    }
    if let Some(value) = parse_angle_rad(expression) {
        return Some(ParameterValue::Angle(Angle(value)));
    }
    if let Ok(value) = expression.trim().parse::<i64>() {
        return Some(ParameterValue::Integer(value));
    }
    expression
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(ParameterValue::Real)
}

pub(crate) fn dimension_display(expression: &str) -> Option<DimensionDisplay> {
    let expression = strip_dimension_count(expression.trim());
    if strip_diameter_modifier(expression).is_some()
        || (expression.starts_with(['⌀', 'Ø']) && parse_length_mm(expression).is_some())
    {
        Some(DimensionDisplay::Diameter)
    } else if strip_radius_modifier(expression).is_some()
        || (expression.starts_with(['R', 'r']) && parse_length_mm(expression).is_some())
    {
        Some(DimensionDisplay::Radius)
    } else {
        None
    }
}

pub(crate) fn parse_dimension_display_length(expression: &str) -> Option<f64> {
    let expression = strip_dimension_count(expression.trim());
    let value = strip_diameter_modifier(expression)
        .or_else(|| strip_radius_modifier(expression))
        .unwrap_or(expression)
        .trim();
    parse_dimension_length_mm(value)
        .or_else(|| strip_dimension_fit(value).and_then(parse_dimension_length_mm))
        .or_else(|| parse_length_mm(expression))
}

pub(crate) fn strip_dimension_count(expression: &str) -> &str {
    let digit_count = expression.bytes().take_while(u8::is_ascii_digit).count();
    let (count, rest) = expression.split_at(digit_count);
    if !count.is_empty()
        && count.parse::<u64>().is_ok_and(|count| count > 0)
        && rest.starts_with(['X', 'x'])
    {
        rest[1..].trim_start()
    } else {
        expression
    }
}

pub(crate) fn strip_dimension_fit(value: &str) -> Option<&str> {
    let fit_start = value
        .char_indices()
        .find_map(|(offset, character)| character.is_ascii_alphabetic().then_some(offset))?;
    let (nominal, fit) = value.split_at(fit_start);
    let grade_start = fit
        .char_indices()
        .find_map(|(offset, character)| character.is_ascii_digit().then_some(offset))?;
    let (position, grade) = fit.split_at(grade_start);
    (!nominal.is_empty()
        && !position.is_empty()
        && position.bytes().all(|byte| byte.is_ascii_alphabetic())
        && grade.bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(nominal)
}

pub(crate) fn strip_diameter_modifier(expression: &str) -> Option<&str> {
    let expression = expression.trim();
    expression
        .strip_prefix("<MOD-DIAM>")
        .or_else(|| expression.strip_prefix("&lt;MOD-DIAM&gt;"))
}

pub(crate) fn strip_radius_modifier(expression: &str) -> Option<&str> {
    let expression = expression.trim();
    expression
        .strip_prefix("<MOD-RHO>")
        .or_else(|| expression.strip_prefix("&lt;MOD-RHO&gt;"))
}

pub(crate) fn parse_neutral_parameter_literal(
    feature: &cadmpeg_ir::features::Feature,
    name: &str,
    expression: &str,
) -> Option<ParameterValue> {
    let positional_length = match name {
        "D1" => matches!(
            &feature.definition,
            FeatureDefinition::Extrude { .. }
                | FeatureDefinition::Fillet { .. }
                | FeatureDefinition::Chamfer { .. }
                | FeatureDefinition::Shell { .. }
                | FeatureDefinition::Thicken { .. }
                | FeatureDefinition::DatumOffsetPlane { .. }
                | FeatureDefinition::MoveFace {
                    motion: FaceMotion::Offset { .. } | FaceMotion::Translate { .. },
                    ..
                }
        ),
        "D2" => matches!(
            &feature.definition,
            FeatureDefinition::Chamfer { groups, .. }
                if groups.iter().any(|group| matches!(group.spec, ChamferSpec::TwoDistances { .. }))
        ),
        "D3" => matches!(
            feature.definition,
            FeatureDefinition::Pattern {
                pattern: PatternKind::Linear { .. } | PatternKind::CurveDriven { .. },
                ..
            }
        ),
        _ => false,
    };
    if positional_length {
        return parse_positive_dimension_length_mm(expression)
            .map(|value| ParameterValue::Length(Length(value)));
    }
    parse_parameter_literal(expression)
}

pub(crate) fn format_parameter_value(value: &ParameterValue) -> String {
    match value {
        ParameterValue::Length(Length(value)) => format_length_mm(*value),
        ParameterValue::Angle(Angle(value)) => format_angle_rad(*value),
        ParameterValue::Real(value) => format_f64_literal(*value),
        ParameterValue::Integer(value) => value.to_string(),
        ParameterValue::Boolean(value) => value.to_string(),
        ParameterValue::String(value) => value.clone(),
    }
}
