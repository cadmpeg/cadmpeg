// SPDX-License-Identifier: Apache-2.0
//! Tolerance-aware semantic comparison of decoded values.
//!
//! ## Why a tolerance exists
//!
//! Decoded geometry passes through `f64::cos`, `f64::sin`, and friends, which
//! resolve to the platform's libm and are not bit-reproducible: glibc, the MSVC
//! runtime, and Apple's libm disagree in the last one or two units in the last
//! place. One conical face pins `origin.v` scaled by `cos(half_angle)`, which
//! serializes as `1.802581857082682` on Linux and `1.8025818570826815` on
//! Windows and macOS — two units in the last place apart, and identical to
//! fourteen significant digits.
//!
//! Exact equality therefore reports a platform as a change. That makes it the
//! wrong relation for any question of the form "do these two decodes describe
//! the same model", whether the asker is a snapshot harness or a user running a
//! semantic comparison over the same file decoded on two machines.
//!
//! ## Why `1e-12` relative with a floor of one
//!
//! Platform libm disagreement is a few units in the last place, near `1e-16`
//! relative, so [`FLOAT_TOLERANCE`] leaves four decimal orders of headroom
//! while still catching any change with physical meaning: at millimeter scale
//! it admits a picometer. The tolerance applies to the larger of the two
//! magnitudes, floored at one, so values below unit magnitude compare
//! absolutely rather than demanding ever-finer agreement as they approach zero
//! — a relative test against zero can never pass, and `0.0` against `1e-30` is
//! agreement for every purpose this relation serves.
//!
//! ## Why integers stay exact
//!
//! Counts, indices, degrees, versions, and identifiers serialize as JSON
//! integers. Not one of them passes through libm, and a change of one in any of
//! them is a real change in the model. [`values_agree`] therefore admits a
//! tolerance only when *both* sides are fractional; an integer pair, and a
//! value that moved between integer and fractional form, compare exactly.
//!
//! ## Tolerant equality is not transitive
//!
//! `a` agreeing with `b` and `b` agreeing with `c` do not imply that `a` agrees
//! with `c`: two steps of just under the tolerance sum to just over it. Every
//! verdict this module produces is therefore strictly about the one pair it was
//! given. Consequences:
//!
//! - The relation cannot back a hash, an equivalence class, a `BTreeMap` key,
//!   or a deduplication pass, all of which need transitivity to be coherent.
//! - A digest over tolerantly compared values is a bitwise fingerprint and
//!   agrees only under exact equality, so no tolerance can rescue one.
//! - Chaining comparisons through an intermediate proves nothing about the
//!   endpoints. Compare the two values the question is actually about.
//!
//! ## Naming a digest that this relation cannot reconcile
//!
//! A codec still needs bitwise digests over decoded content: the write path asks
//! "was this document edited since it was decoded?" and only a bitwise digest
//! answers that cheaply. Such a digest is valid within one machine's decode and
//! nowhere else, for exactly the reason stated above. It is therefore named with
//! the [`LOCAL_DIGEST_SUFFIX`] suffix, and [`is_local_digest_attribute`] is the
//! one place that decides whether a source attribute holds one.

use std::fmt::Write as _;

use serde_json::Value;

/// Relative tolerance for a fractional number in a semantic comparison.
///
/// Applied against the larger magnitude of the two values, with a floor of one
/// so small values compare absolutely. See the module documentation for why
/// this magnitude.
pub const FLOAT_TOLERANCE: f64 = 1e-12;

/// Suffix reserved for a source attribute holding a machine-local content
/// digest.
///
/// A key ending in this suffix holds a bitwise digest over decoded neutral
/// content — the very values this module compares tolerantly. It is reproducible
/// only under the same binary on the same platform, it is never a portable
/// identity, and no tolerance can reconcile two of them. A codec that records a
/// new digest of decoded content must name it with this suffix; a digest over
/// retained source bytes is bit-exact everywhere and must not.
pub const LOCAL_DIGEST_SUFFIX: &str = "_local_sha256";

/// Whether a source attribute key names a machine-local content digest under the
/// [`LOCAL_DIGEST_SUFFIX`] convention.
///
/// Two consumers depend on this: a structural diff reports such an attribute
/// informationally rather than as a real difference, and the golden harness
/// elides it from a snapshot. Both would otherwise report a platform as a
/// change.
#[must_use]
pub fn is_local_digest_attribute(key: &str) -> bool {
    key.ends_with(LOCAL_DIGEST_SUFFIX)
}

/// Whether two fractional numbers agree within [`FLOAT_TOLERANCE`] relative to
/// the larger magnitude, with a floor of one so small values compare
/// absolutely.
///
/// Exact equality short-circuits, so two infinities of the same sign agree. Any
/// other non-finite pair disagrees: `NaN` is not equal to itself, and an
/// infinity against a finite value has no meaningful difference to bound.
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
/// disagreement between two fractional numbers.
///
/// Object key sets, array lengths, strings, booleans, nulls, and integers must
/// match exactly; a fractional pair may differ by up to [`FLOAT_TOLERANCE`]
/// relative to the larger magnitude. The relation is not transitive; see the
/// module documentation.
///
/// # Errors
///
/// Returns a description of the first disagreement, located by JSON path
/// relative to the values passed in.
pub fn values_agree(left: &Value, right: &Value) -> Result<(), String> {
    let mut path = String::new();
    walk(left, right, &mut path)
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
    use super::{floats_agree, values_agree, FLOAT_TOLERANCE};

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
}
