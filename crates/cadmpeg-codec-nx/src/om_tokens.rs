// SPDX-License-Identifier: Apache-2.0
//! Single source of truth for the NX object-model serialization vocabulary.
//!
//! An NX OM section is not a fixed set of record classes: the class registry is
//! a run of length-framed `UGS::` names discovered in the stream, so the class
//! set is input data, not a spec ([`om`](crate::om) reads it dynamically). What
//! *is* fixed is the small literal vocabulary the OM decode keys on: the root
//! entity marker, the `hostglobalvariables` section gate, the class-name prefix,
//! the numeric-expression prefix, and the closed set of unit tokens a numeric
//! expression may declare. This module names each once; [`om`](crate::om)
//! matches against these constants and resolves declared units through
//! [`unit_for`], and the reference doc plus the `nx_om` fuzz dictionary are
//! generated from the same table. A checked test regenerates the derived
//! artifacts and diffs them, and a drift test asserts [`unit_for`] round-trips
//! every [`UNITS`] row.

use crate::om::ExpressionUnit;

/// Root OM entity marker: the first entity of an accepted section begins here.
pub const ROOT_MARKER: &[u8] = b"\x04\x01\x0eNX ";
/// Section gate: numeric expressions are decoded only from a section whose
/// records contain this literal.
pub const HOST_GLOBALS: &[u8] = b"hostglobalvariables";
/// Registered class-definition name prefix in the type-definition run.
pub const CLASS_NAME_PREFIX: &[u8] = b"UGS::";
/// Numeric-expression payload prefix, immediately preceding the unit token.
pub const NUMBER_PREFIX: &[u8] = b"(Number [";

/// One structural anchor literal the OM decode matches verbatim.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    /// Stable short name used in the reference doc.
    pub name: &'static str,
    /// The literal bytes matched in the stream.
    pub literal: &'static [u8],
    /// What the decode does at this anchor.
    pub summary: &'static str,
}

/// Every structural anchor, in decode order.
pub const ANCHORS: &[Anchor] = &[
    Anchor {
        name: "root_marker",
        literal: ROOT_MARKER,
        summary: "root entity marker anchoring an accepted section's first record",
    },
    Anchor {
        name: "host_globals",
        literal: HOST_GLOBALS,
        summary: "section gate: numeric expressions decode only when present",
    },
    Anchor {
        name: "class_name_prefix",
        literal: CLASS_NAME_PREFIX,
        summary: "registered class-definition name prefix in the type run",
    },
    Anchor {
        name: "number_prefix",
        literal: NUMBER_PREFIX,
        summary: "numeric-expression prefix immediately before the unit token",
    },
];

/// One declared numeric-expression unit token.
#[derive(Debug, Clone, Copy)]
pub struct UnitToken {
    /// The serialized token inside the `[...]` of a numeric expression.
    pub literal: &'static str,
    /// The unit it resolves to.
    pub unit: ExpressionUnit,
    /// One-line description.
    pub summary: &'static str,
}

/// The closed set of numeric-expression unit tokens, in declaration order.
pub const UNITS: &[UnitToken] = &[
    UnitToken {
        literal: "mm",
        unit: ExpressionUnit::Millimeter,
        summary: "canonical model length in millimeters",
    },
    UnitToken {
        literal: "degrees",
        unit: ExpressionUnit::Degree,
        summary: "angular value in degrees",
    },
];

/// Resolve a declared unit token, or `None` for a token outside the closed set.
/// [`om`](crate::om) dispatches its numeric-expression unit through this, so the
/// accepted set cannot drift from the reference doc.
#[must_use]
pub fn unit_for(token: &str) -> Option<ExpressionUnit> {
    UNITS
        .iter()
        .find(|entry| entry.literal == token)
        .map(|entry| entry.unit)
}

/// Render the OM-vocabulary reference doc from [`ANCHORS`] and [`UNITS`].
/// `docs/formats/siemens_nx_om_tokens.md` is this output verbatim.
#[must_use]
pub fn render_reference() -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("<!-- Generated from crates/cadmpeg-codec-nx/src/om_tokens.rs. ");
    out.push_str("Do not edit by hand; run `cargo test -p cadmpeg-codec-nx`. -->\n\n");
    out.push_str("# NX object-model serialization vocabulary\n\n");
    out.push_str(
        "The NX OM class registry is a run of length-framed `UGS::` names \
discovered in the stream, so there is no fixed record-class set. The fixed \
vocabulary is the literals the OM decode keys on and the closed set of \
numeric-expression unit tokens.\n\n",
    );

    out.push_str("## Structural anchors\n\n");
    out.push_str("| Name | Literal | Description |\n");
    out.push_str("|---|---|---|\n");
    for anchor in ANCHORS {
        let _ = writeln!(
            out,
            "| {} | `{}` | {} |",
            anchor.name,
            escape_literal(anchor.literal),
            anchor.summary,
        );
    }

    out.push_str("\n## Numeric-expression units\n\n");
    out.push_str("| Token | Unit | Description |\n");
    out.push_str("|---|---|---|\n");
    for entry in UNITS {
        let _ = writeln!(
            out,
            "| `{}` | {:?} | {} |",
            entry.literal, entry.unit, entry.summary,
        );
    }
    out
}

/// Render each byte of a literal, escaping non-printable bytes as `\xNN`.
fn escape_literal(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    for &byte in bytes {
        if (0x20..0x7f).contains(&byte) {
            out.push(byte as char);
        } else {
            let _ = write!(out, "\\x{byte:02x}");
        }
    }
    out
}

/// Render the `nx_om` fuzz dictionary: one quoted token per anchor literal and
/// unit token, for structure-aware fuzzing of the OM section decode. libFuzzer
/// dictionary syntax is `"token"` per line, non-printable bytes as `\xNN`.
#[must_use]
pub fn render_dictionary() -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("# Generated from crates/cadmpeg-codec-nx/src/om_tokens.rs.\n");
    out.push_str("# Do not edit by hand; run `cargo test -p cadmpeg-codec-nx`.\n");
    out.push_str("# NX object-model serialization tokens, for structure-aware fuzzing.\n");
    for anchor in ANCHORS {
        let _ = writeln!(out, "\"{}\"", escape_literal(anchor.literal));
    }
    for entry in UNITS {
        let _ = writeln!(out, "\"{}\"", entry.literal);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    fn check_generated(relative: &str, expected: &str) {
        let path = workspace_root().join(relative);
        if std::env::var_os("CADMPEG_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().expect("artifact has a parent"))
                .expect("create artifact directory");
            std::fs::write(&path, expected).expect("write blessed artifact");
            return;
        }
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}; run with CADMPEG_BLESS=1", path.display()));
        assert_eq!(
            actual,
            expected,
            "{} is stale; run `CADMPEG_BLESS=1 cargo test -p cadmpeg-codec-nx`",
            path.display()
        );
    }

    #[test]
    fn reference_doc_matches_table() {
        check_generated("docs/formats/siemens_nx_om_tokens.md", &render_reference());
    }

    #[test]
    fn fuzz_dictionary_matches_table() {
        check_generated(
            "crates/cadmpeg-fuzz/dictionaries/nx_om.dict",
            &render_dictionary(),
        );
    }

    /// The unit dispatch the OM decode calls must resolve exactly the table's
    /// tokens, so the accepted unit set cannot drift from the reference doc.
    #[test]
    fn unit_dispatch_matches_table() {
        for entry in UNITS {
            assert_eq!(unit_for(entry.literal), Some(entry.unit));
        }
        assert_eq!(unit_for("furlongs"), None);
    }
}
