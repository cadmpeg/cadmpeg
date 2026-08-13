// SPDX-License-Identifier: Apache-2.0
//! Numeric and vector formatters for native Keywords property values.

use super::super::{format_angle_rad, format_f64_literal, format_length_mm};
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_DEGREE_ROUND: f64 = 1.0e-12;

pub(super) fn format_length_like(value: f64, previous: Option<&str>) -> String {
    let previous = previous.map(str::trim).unwrap_or_default();
    if previous.starts_with(['R', 'r']) {
        format!("R{value}")
    } else if previous.starts_with(['\u{2300}', '\u{00d8}']) {
        format!("\u{2300}{value}")
    } else if previous.parse::<f64>().is_ok() {
        format_f64_literal(value)
    } else {
        format_length_mm(value)
    }
}

pub(super) fn format_angle_like(value: f64, previous: Option<&str>) -> String {
    if previous
        .map(str::trim)
        .is_some_and(|value| value.ends_with('\u{00b0}'))
    {
        let degrees = value.to_degrees();
        let rounded = degrees.round();
        let degrees = if (degrees - rounded).abs() <= EPS_DEGREE_ROUND {
            rounded
        } else {
            degrees
        };
        format!("{degrees}\u{00b0}")
    } else {
        format_angle_rad(value)
    }
}

pub(super) fn format_point3_mm(value: Point3) -> String {
    format!("{}mm,{}mm,{}mm", value.x, value.y, value.z)
}

pub(super) fn format_vector3(value: Vector3) -> String {
    format!("{},{},{}", value.x, value.y, value.z)
}
