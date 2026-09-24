// SPDX-License-Identifier: Apache-2.0
//! Numeric and vector formatters for native Keywords property values.

use super::super::literals::{format_angle_rad, format_length_mm, format_length_number};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::scalar::{Angle, Length};

const EPS_DEGREE_ROUND: f64 = 1.0e-12;

pub(super) fn format_length_like(value: Length, previous: Option<&str>) -> String {
    let previous = previous.map(str::trim).unwrap_or_default();
    if previous.starts_with(['R', 'r']) {
        format!("R{}", value.get())
    } else if previous.starts_with(['\u{2300}', '\u{00d8}']) {
        format!("\u{2300}{}", value.get())
    } else if previous.parse::<f64>().is_ok() {
        format_length_number(value)
    } else {
        format_length_mm(value)
    }
}

/// The angle in the unit of `previous`: degrees when it ends with a degree
/// sign, radians otherwise. An angle whose degree value is not finite has no
/// degree literal and is refused.
pub(super) fn format_angle_like(
    value: Angle,
    previous: Option<&str>,
) -> Result<String, CodecError> {
    if previous
        .map(str::trim)
        .is_some_and(|value| value.ends_with('\u{00b0}'))
    {
        let degrees = value.get().to_degrees();
        if !degrees.is_finite() {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT angle {} rad has no finite degree value",
                value.get()
            )));
        }
        let rounded = degrees.round();
        let degrees = if (degrees - rounded).abs() <= EPS_DEGREE_ROUND {
            rounded
        } else {
            degrees
        };
        Ok(format!("{degrees}\u{00b0}"))
    } else {
        Ok(format_angle_rad(value))
    }
}

pub(super) fn format_point3_mm(value: FinitePoint3) -> String {
    let value = value.get();
    format!("{}mm,{}mm,{}mm", value.x, value.y, value.z)
}

pub(super) fn format_vector3(value: FiniteVector3) -> String {
    let value = value.get();
    format!("{},{},{}", value.x, value.y, value.z)
}

#[cfg(test)]
mod tests {
    use super::format_angle_like;
    use cadmpeg_ir::scalar::Angle;

    /// An angle whose degree value overflows has no degree literal; it was
    /// written as `inf°`.
    #[test]
    fn an_angle_whose_degree_value_overflows_has_no_degree_literal() {
        let angle = Angle::new(1.0e307).expect("a finite angle");
        let written = format_angle_like(angle, Some("30\u{00b0}"));
        let Err(error) = written else {
            panic!("{written:?}");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::NotImplemented(_)),
            "{error}"
        );
        assert_eq!(
            format_angle_like(Angle::new(0.5).expect("a finite angle"), Some("30\u{00b0}"))
                .expect("a finite degree value"),
            format!("{}\u{00b0}", 0.5_f64.to_degrees())
        );
    }
}
