// SPDX-License-Identifier: Apache-2.0
//! Admission of the independent `GuiDocument.xml` schema layer.

/// Admission result for the GUI document schema.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Admission {
    /// Schema 1 uses the verified GUI vocabulary.
    Schema1,
    /// Any other declaration, or none, is read with the schema-1 vocabulary
    /// without a verified declaration match.
    Unverified,
}

impl Admission {
    /// Neutral presentation schema identity exists only for a declaration that
    /// verified the schema-1 GUI vocabulary.
    pub(super) const fn neutral_schema_version(&self) -> Option<u32> {
        match self {
            Self::Schema1 => Some(1),
            Self::Unverified => None,
        }
    }
}

/// Select the `GuiDocument.xml` parser admission path from the exact declaration.
///
/// GUI schema is not an `FCStd` host identity row. The declaration is matched
/// verbatim because `"01"` does not declare the verified schema-1 vocabulary.
pub(super) fn classify(schema_version: Option<&str>) -> Admission {
    // The literal "1" bounds the comparison.
    if schema_version == Some("1") {
        Admission::Schema1
    } else {
        Admission::Unverified
    }
}

#[cfg(test)]
mod tests {
    use super::{classify, Admission};

    #[test]
    fn admission_matches_the_verbatim_declaration() {
        let admitted = classify(Some("1"));
        assert_eq!(admitted, Admission::Schema1);
        assert_eq!(admitted.neutral_schema_version(), Some(1));
        for declaration in [Some("01"), Some("2"), Some("not-an-integer"), None] {
            let admission = classify(declaration);
            assert_eq!(admission, Admission::Unverified);
            assert_eq!(admission.neutral_schema_version(), None);
        }
    }
}
