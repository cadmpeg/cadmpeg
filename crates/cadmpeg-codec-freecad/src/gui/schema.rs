// SPDX-License-Identifier: Apache-2.0
//! Admission of the independent `GuiDocument.xml` schema layer.

/// Admission result for the GUI document schema.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    /// Schema 1 uses the verified GUI vocabulary.
    Schema1,
    /// Any other declaration is read with the schema-1 vocabulary without a
    /// verified declaration match.
    Unverified { declaration: String },
}

/// Select the `GuiDocument.xml` parser admission path from the exact declaration.
///
/// GUI schema is not an `FCStd` host identity row. The declaration is matched
/// verbatim because `"01"` does not declare the verified schema-1 vocabulary.
pub(crate) fn classify(schema_version: Option<&str>) -> Admission {
    match schema_version {
        Some("1") => Admission::Schema1,
        Some(value) => Admission::Unverified {
            declaration: value.to_owned(),
        },
        None => Admission::Unverified {
            declaration: "missing".into(),
        },
    }
}
