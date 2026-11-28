// SPDX-License-Identifier: Apache-2.0

const NOMINAL_AGREEMENT_TOLERANCE: f64 = 1e-9;

pub(super) fn decimal_millimetres(designation: &str, profile: &str, value: f64) -> Option<f64> {
    let (nominal, inch) = designation_size(designation)?;
    let profile = profile.to_ascii_lowercase();
    let metric_table = profile.contains("metric");
    let inch_table = profile.contains("unified") || profile.contains("inch");
    if (inch && metric_table)
        || (!inch && inch_table)
        || (nominal - value).abs() > NOMINAL_AGREEMENT_TOLERANCE
    {
        return None;
    }
    Some(value * if inch { 25.4 } else { 1.0 })
}

fn positive(text: &str) -> Option<f64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let value: f64 = text.parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

fn designation_size(text: &str) -> Option<(f64, bool)> {
    if let Some(metric) = text.strip_prefix('M') {
        let diameter = if let Some((diameter, pitch)) = metric.split_once(['x', '×']) {
            positive(pitch)?;
            diameter
        } else {
            metric
        };
        return Some((positive(diameter)?, false));
    }
    let (size, rest) = text.split_once('-')?;
    let mut fields = rest.split_whitespace();
    positive(fields.next()?)?;
    let series = fields.next()?;
    if !matches!(
        series,
        "UNC" | "UNF" | "UNEF" | "UN" | "UNS" | "UNJ" | "UNJC" | "UNJF" | "UNJEF" | "UNJS"
    ) || fields.next().is_some()
    {
        return None;
    }
    let diameter = if let Some(number) = size.strip_prefix('#') {
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        0.060 + 0.013 * f64::from(number.parse::<u32>().ok()?)
    } else if let Some((numerator, denominator)) = size.split_once('/') {
        positive(numerator)? / positive(denominator)?
    } else {
        positive(size)?
    };
    Some((diameter, true))
}
