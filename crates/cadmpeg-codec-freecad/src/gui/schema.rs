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

impl Admission {
    /// Neutral presentation schema identity exists only for a declaration that
    /// verified the schema-1 GUI vocabulary.
    pub(crate) const fn neutral_schema_version(&self) -> Option<u32> {
        match self {
            Self::Schema1 => Some(1),
            Self::Unverified { .. } => None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{classify, Admission};

    #[test]
    fn admission_matches_the_verbatim_declaration() {
        let admitted = classify(Some("1"));
        assert_eq!(admitted, Admission::Schema1);
        assert_eq!(admitted.neutral_schema_version(), Some(1));
        for declaration in ["01", "2", "not-an-integer"] {
            let admission = classify(Some(declaration));
            assert_eq!(
                admission,
                Admission::Unverified {
                    declaration: declaration.to_owned(),
                }
            );
            assert_eq!(admission.neutral_schema_version(), None);
        }
        let missing = classify(None);
        assert_eq!(
            missing,
            Admission::Unverified {
                declaration: "missing".to_string(),
            }
        );
        assert_eq!(missing.neutral_schema_version(), None);
    }
}
