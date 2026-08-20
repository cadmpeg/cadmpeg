// SPDX-License-Identifier: Apache-2.0
//! Tolerance-aware semantic comparison of decoded values.
//!
//! ## Tolerance
//!
//! Decoded geometry calls `f64::cos`, `f64::sin`, and other libm functions.
//! glibc, the MSVC runtime, and Apple's libm disagree by one or two units in
//! the last place. One conical face pins `origin.v` scaled by
//! `cos(half_angle)`, which serializes as `1.802581857082682` on Linux and
//! `1.8025818570826815` on Windows and macOS: two ULPs apart, identical to
//! fourteen significant digits.
//!
//! Exact equality therefore treats a platform difference as a model change.
//! That breaks any check of the form "do these two decodes describe the same
//! model", whether a snapshot harness or a cross-machine compare of one file.
//!
//! ## `1e-12` relative, floor of one
//!
//! Platform libm disagreement sits near `1e-16` relative. [`FLOAT_TOLERANCE`]
//! leaves four decimal orders of headroom and still flags a change with
//! physical meaning: at millimeter scale it admits a picometer. The tolerance
//! uses the larger of the two magnitudes, floored at one, so values below unit
//! magnitude compare absolutely. A relative test against zero can never pass,
//! and `0.0` against `1e-30` agrees for every use of this relation.
//!
//! ## Exact integers
//!
//! Counts, indices, degrees, versions, and identifiers serialize as JSON
//! integers. None of them pass through libm. A change of one is a real model
//! change. [`values_agree`] admits a tolerance only when both sides are
//! fractional. An integer pair, and a value that moved between integer and
//! fractional form, compare exactly.
//!
//! String values compare exactly except for embedded numeric tokens. Two
//! fractional tokens use the same tolerance. An integer zero and a fractional
//! token also agree when the fractional value is within that tolerance of
//! zero, because one libm can return exact zero where another returns a tiny
//! residual. All other integer tokens remain exact. Encode goldens pin writer
//! text that still carries platform libm bits.
//!
//! ## Non-transitive relation
//!
//! If `a` agrees with `b` and `b` agrees with `c`, `a` need not agree with
//! `c`: two steps under the tolerance can sum over it. Each verdict covers
//! only the pair you pass in.
//!
//! - Do not use this relation for a hash, equivalence class, `BTreeMap` key,
//!   or deduplication pass. Those need transitivity.
//! - A digest over tolerantly compared values is a bitwise fingerprint and
//!   agrees only under exact equality.
//! - Chaining through an intermediate proves nothing about the endpoints.
//!   Compare the two values the question names.
//!
//! ## Local digests
//!
//! A codec still needs bitwise digests over decoded content: the write path
//! asks whether the document changed since decode, and only a bitwise digest
//! answers that cheaply. Such a digest is valid within one machine's decode
//! and nowhere else, for the libm reason above. Name it with
//! [`LOCAL_DIGEST_SUFFIX`]. [`is_local_digest_attribute`] is the sole check
//! for whether a source attribute holds one.

use std::fmt::Write as _;

use serde_json::Value;

/// Relative tolerance for a fractional number in a semantic comparison.
///
/// Applied against the larger magnitude of the two values, with a floor of one
/// so small values compare absolutely. See the module documentation for why
/// this magnitude.
pub const FLOAT_TOLERANCE: f64 = 1e-12;

/// Suffix for a source attribute holding a machine-local content digest.
///
/// Bitwise over decoded content; machine-local only. Digests over retained
/// source bytes must not use this suffix.
pub const LOCAL_DIGEST_SUFFIX: &str = "_local_sha256";

/// Whether a source attribute key names a machine-local content digest under the
/// [`LOCAL_DIGEST_SUFFIX`] convention.
///
/// Structural diffs report these informationally; the golden harness elides them.
#[must_use]
pub fn is_local_digest_attribute(key: &str) -> bool {
    key.ends_with(LOCAL_DIGEST_SUFFIX)
}

/// Whether two fractional numbers agree within [`FLOAT_TOLERANCE`] relative to
/// the larger magnitude, with a floor of one so small values compare
/// absolutely.
///
/// Exact equality short-circuits (including same-sign infinities). Other
/// non-finite pairs disagree.
#[must_use]
pub fn floats_agree(left: f64, right: f64) -> bool {
    if left == right {
        return true;
    }
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    let magnitude = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= FLOAT_TOLERANCE * magnitude
}

/// Compares two JSON values structurally, tolerating only last-place
/// disagreement between fractional numbers.
///
/// Object key sets, array lengths, booleans, nulls, and integers must match
/// exactly; a fractional pair may differ by up to [`FLOAT_TOLERANCE`] relative
/// to the larger magnitude. Strings must match outside fractional tokens; see
/// [`texts_agree`]. The relation is not transitive; see the module
/// documentation.
///
/// # Errors
///
/// Returns a description of the first disagreement, located by JSON path
/// relative to the values passed in.
pub fn values_agree(left: &Value, right: &Value) -> Result<(), String> {
    let mut path = String::new();
    walk(left, right, &mut path)
}

/// Whether two texts agree, tolerating last-place drift in fractional tokens.
///
/// Non-numeric spans and integer tokens must match byte-exactly, except that an
/// integer zero agrees with a fractional token within [`FLOAT_TOLERANCE`] of
/// zero. A pair of fractional tokens may differ by the same tolerance. IGES
/// writer output uses `D` exponents; both `E` and `D` parse.
#[must_use]
pub fn texts_agree(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let mut left_rest = left;
    let mut right_rest = right;
    loop {
        let left_next = next_numeric_token(left_rest);
        let right_next = next_numeric_token(right_rest);
        match (left_next, right_next) {
            (None, None) => return left_rest == right_rest,
            (
                Some((left_prefix, left_token, left_after)),
                Some((right_prefix, right_token, right_after)),
            ) => {
                if left_prefix != right_prefix || !numeric_tokens_agree(left_token, right_token) {
                    return false;
                }
                left_rest = left_after;
                right_rest = right_after;
            }
            _ => return false,
        }
    }
}

#[derive(Clone, Copy)]
struct NumericToken<'a> {
    text: &'a str,
    value: f64,
    fractional: bool,
}

fn numeric_tokens_agree(left: NumericToken<'_>, right: NumericToken<'_>) -> bool {
    match (left.fractional, right.fractional) {
        (true, true) => floats_agree(left.value, right.value),
        (false, false) => left.text == right.text,
        _ => {
            let (integer, fractional) = if left.fractional {
                (right, left)
            } else {
                (left, right)
            };
            integer.text == "0" && floats_agree(0.0, fractional.value)
        }
    }
}

/// First numeric token in `text`: prefix before it, token metadata, and the
/// remainder after it.
fn next_numeric_token(text: &str) -> Option<(&str, NumericToken<'_>, &str)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if let Some((end, value, fractional)) = match_numeric_at(bytes, index) {
            let prefix = &text[..index];
            let after = &text[end..];
            return Some((
                prefix,
                NumericToken {
                    text: &text[index..end],
                    value,
                    fractional,
                },
                after,
            ));
        }
        index += 1;
    }
    None
}

/// Numeric token starting at `start`, or `None` when that byte is not the start
/// of one. Directory-section `D` markers stay outside tokens because a `D`
/// exponent is accepted only after a decimal point.
fn match_numeric_at(bytes: &[u8], start: usize) -> Option<(usize, f64, bool)> {
    if start > 0 {
        let previous = bytes[start - 1];
        if previous.is_ascii_alphanumeric() || previous == b'.' {
            return None;
        }
    }

    let mut index = start;
    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        let next = *bytes.get(index + 1)?;
        if next != b'.' && !next.is_ascii_digit() {
            return None;
        }
        index += 1;
    }

    let mut saw_digit = false;
    let mut saw_dot = false;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        saw_digit = true;
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        saw_dot = true;
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            saw_digit = true;
            index += 1;
        }
    }
    if !saw_digit {
        return None;
    }

    if saw_dot && index < bytes.len() && matches!(bytes[index], b'e' | b'E' | b'd' | b'D') {
        let exponent_mark = index;
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        let exponent_digits = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_digits {
            index = exponent_mark;
        }
    }

    let token = std::str::from_utf8(&bytes[start..index]).ok()?;
    let normalized = token.replace(['d', 'D'], "e");
    let value = normalized.parse().ok()?;
    Some((index, value, saw_dot))
}

/// Walks two values in step, recording the path to the first disagreement.
fn walk(left: &Value, right: &Value, path: &mut String) -> Result<(), String> {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            if let Some(key) = left_map.keys().find(|key| !right_map.contains_key(*key)) {
                return Err(disagreement(
                    path,
                    &format!("left has key `{key}`, right does not"),
                ));
            }
            if let Some(key) = right_map.keys().find(|key| !left_map.contains_key(*key)) {
                return Err(disagreement(
                    path,
                    &format!("right has key `{key}`, left does not"),
                ));
            }
            for (key, left_child) in left_map {
                let restore = path.len();
                path.push('.');
                path.push_str(key);
                walk(left_child, &right_map[key], path)?;
                path.truncate(restore);
            }
            Ok(())
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            if left_items.len() != right_items.len() {
                return Err(disagreement(
                    path,
                    &format!(
                        "left has {} item(s), right has {}",
                        left_items.len(),
                        right_items.len()
                    ),
                ));
            }
            for (index, (left_child, right_child)) in left_items.iter().zip(right_items).enumerate()
            {
                let restore = path.len();
                write!(path, "[{index}]").expect("writing to a String cannot fail");
                walk(left_child, right_child, path)?;
                path.truncate(restore);
            }
            Ok(())
        }
        (Value::Number(left_number), Value::Number(right_number)) => {
            // Only a pair of fractional numbers can carry platform libm
            // disagreement. Counts, indices, and versions serialize as integers
            // and must match exactly, as must a value that changed between an
            // integer and a fractional form.
            let agree = match (left_number.as_f64(), right_number.as_f64()) {
                (Some(left_float), Some(right_float))
                    if left_number.is_f64() && right_number.is_f64() =>
                {
                    floats_agree(left_float, right_float)
                }
                _ => left_number == right_number,
            };
            if agree {
                Ok(())
            } else {
                Err(disagreement(
                    path,
                    &format!("left {left_number}, right {right_number}"),
                ))
            }
        }
        (Value::String(left_text), Value::String(right_text)) => {
            if texts_agree(left_text, right_text) {
                Ok(())
            } else {
                Err(disagreement(
                    path,
                    &format!("left {}, right {}", truncate(left), truncate(right)),
                ))
            }
        }
        _ if left == right => Ok(()),
        _ => Err(disagreement(
            path,
            &format!("left {}, right {}", truncate(left), truncate(right)),
        )),
    }
}

/// Renders one disagreement with the JSON path that located it.
fn disagreement(path: &str, detail: &str) -> String {
    let location = if path.is_empty() { "<root>" } else { path };
    format!("at `{location}`: {detail}")
}

/// Renders a value for a failure message, bounded so a whole arena cannot land
/// in a panic or a report.
fn truncate(value: &Value) -> String {
    let text = value.to_string();
    if text.len() > 120 {
        format!("{}…", &text[..120])
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::{floats_agree, texts_agree, values_agree, FLOAT_TOLERANCE};

    /// The two values one conical face produces on Linux against Windows and
    /// macOS. Their difference is platform libm disagreement, not a change in
    /// the model, so the comparison must accept it.
    const LINUX_CONE_V: f64 = 1.802_581_857_082_682;
    const WINDOWS_CONE_V: f64 = 1.802_581_857_082_681_5;

    /// Parses two JSON texts and compares them.
    fn agree(left: &str, right: &str) -> Result<(), String> {
        let left = serde_json::from_str(left).expect("left test literal is JSON");
        let right = serde_json::from_str(right).expect("right test literal is JSON");
        values_agree(&left, &right)
    }

    #[test]
    fn last_place_platform_disagreement_agrees() {
        assert_ne!(
            LINUX_CONE_V.to_bits(),
            WINDOWS_CONE_V.to_bits(),
            "the fixture values must differ, or this test proves nothing"
        );
        assert!(floats_agree(LINUX_CONE_V, WINDOWS_CONE_V));
        let left = format!("{{\"v\": {LINUX_CONE_V:?}}}");
        let right = format!("{{\"v\": {WINDOWS_CONE_V:?}}}");
        assert_ne!(left, right);
        assert!(agree(&left, &right).is_ok());
    }

    #[test]
    fn drift_beyond_the_tolerance_disagrees() {
        let moved = LINUX_CONE_V * (1.0 + 1000.0 * FLOAT_TOLERANCE);
        assert!(!floats_agree(LINUX_CONE_V, moved));
        let error = agree(
            &format!("{{\"v\": {LINUX_CONE_V:?}}}"),
            &format!("{{\"v\": {moved:?}}}"),
        )
        .expect_err("a change above the tolerance must be reported");
        assert!(error.contains(".v"), "{error}");
    }

    #[test]
    fn structure_and_strings_stay_exact() {
        for (left, right, expected_path) in [
            (r#"{"a": "x"}"#, r#"{"a": "y"}"#, ".a"),
            (r#"{"a": true}"#, r#"{"a": false}"#, ".a"),
            (r#"{"a": [1, 2]}"#, r#"{"a": [1, 2, 3]}"#, ".a"),
            (r#"{"a": {"b": 1}}"#, r#"{"a": {"c": 1}}"#, ".a"),
            (r#"{"a": 1}"#, r#"{"a": 2}"#, ".a"),
            (r#"{"a": 1.5}"#, r#"{"a": null}"#, ".a"),
        ] {
            let error = agree(left, right).expect_err("an exact-match field must be reported");
            assert!(error.contains(expected_path), "{left} vs {right}: {error}");
        }
    }

    #[test]
    fn an_integer_never_gets_a_tolerance() {
        // An integer one apart is well inside the relative tolerance at this
        // magnitude, and must still be reported.
        let count = 1_000_000_000_000_000_i64;
        assert!(
            (1.0_f64) <= FLOAT_TOLERANCE * (count as f64),
            "the magnitudes must make this a genuine test of integer exactness"
        );
        let error = agree(
            &format!("{{\"n\": {count}}}"),
            &format!("{{\"n\": {}}}", count + 1),
        )
        .expect_err("an integer field must never be tolerated");
        assert!(error.contains(".n"), "{error}");
    }

    #[test]
    fn an_integer_against_a_fractional_form_disagrees() {
        let error = agree(r#"{"a": 1}"#, r#"{"a": 1.0}"#)
            .expect_err("a change of numeric form must be reported");
        assert!(error.contains(".a"), "{error}");
    }

    #[test]
    fn a_nested_path_is_reported() {
        let error = agree(
            r#"{"ir": {"model": {"pcurves": [{"v": 1.0}, {"v": 2.0}]}}}"#,
            r#"{"ir": {"model": {"pcurves": [{"v": 1.0}, {"v": 9.0}]}}}"#,
        )
        .expect_err("a moved value must be reported");
        assert!(error.contains(".ir.model.pcurves[1].v"), "{error}");
    }

    #[test]
    fn small_values_compare_absolutely() {
        assert!(floats_agree(0.0, FLOAT_TOLERANCE / 2.0));
        assert!(!floats_agree(0.0, FLOAT_TOLERANCE * 10.0));
    }

    #[test]
    fn non_finite_values_agree_only_when_identical() {
        assert!(floats_agree(f64::INFINITY, f64::INFINITY));
        assert!(!floats_agree(f64::INFINITY, f64::NEG_INFINITY));
        assert!(!floats_agree(f64::NAN, f64::NAN));
        assert!(!floats_agree(f64::INFINITY, 0.0));
    }

    #[test]
    fn iges_d_notation_last_place_drift_agrees_in_text() {
        // Writer emits `{value:.16e}` with `e` replaced by `D`. Surface-of-
        // revolution encode goldens carry near-zeros from platform libm; the
        // string field must tolerate the same last-place noise JSON numbers do.
        let left = "6.1232339957367660D-17,9.9999999999999978D-1";
        let right = "6.1232339957367650D-17,9.9999999999999989D-1";
        assert_ne!(left, right);
        assert!(texts_agree(left, right));
        assert!(agree(
            &format!("{{\"output\":{left:?}}}"),
            &format!("{{\"output\":{right:?}}}"),
        )
        .is_ok());
    }

    #[test]
    fn exact_zero_and_a_tiny_fractional_residual_agree_in_text() {
        let residual = FLOAT_TOLERANCE / 2.0;
        let fractional = format!("{residual:.16e}").replace('e', "D");

        assert!(texts_agree("120,0,1;", &format!("120,{fractional},1;")));
        assert!(texts_agree(&format!("120,{fractional},1;"), "120,0,1;"));
    }

    #[test]
    fn integer_and_fractional_tokens_stay_exact_outside_the_zero_residual_case() {
        assert!(!texts_agree("entity 5", "entity 5.0"));
        assert!(!texts_agree(
            "120,0,1;",
            &format!("120,{:.16e},1;", FLOAT_TOLERANCE * 10.0)
        ));
        assert!(!texts_agree("120,-0,1;", "120,0.0,1;"));
    }

    #[test]
    fn iges_directory_section_marker_is_not_an_exponent() {
        // Fixed-width directory cards end with `D` then a sequence column.
        // That `D` is a section letter, not a real exponent.
        let left = "000000000D      1\n110,1.0000000000000000D0;";
        let right = "000000000D      2\n110,1.0000000000000000D0;";
        assert!(!texts_agree(left, right));
    }

    #[test]
    fn integer_runs_in_text_stay_exact() {
        assert!(!texts_agree("entity 110", "entity 111"));
        assert!(texts_agree("entity 110", "entity 110"));
    }

    #[test]
    fn drift_beyond_tolerance_in_text_disagrees() {
        let left = "1.0000000000000000D0";
        let moved = 1.0 * (1.0 + 1000.0 * FLOAT_TOLERANCE);
        let right = format!("{moved:.16e}").replace('e', "D");
        assert!(!texts_agree(left, &right));
    }
}
