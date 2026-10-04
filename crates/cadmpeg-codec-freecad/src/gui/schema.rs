// SPDX-License-Identifier: Apache-2.0
//! Admission of the independent `GuiDocument.xml` schema layer.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Admission result for the GUI document schema.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Admission {
    /// Schema 1 uses the verified GUI vocabulary.
    Schema1,
    /// Any other declaration is read with the schema-1 vocabulary without a
    /// verified declaration match.
    Unverified { declaration: Option<String> },
}

impl Admission {
    /// Neutral presentation schema identity exists only for a declaration that
    /// verified the schema-1 GUI vocabulary.
    pub(super) const fn neutral_schema_version(&self) -> Option<u32> {
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
pub(super) fn classify(
    ctx: &DecodeContext<'_>,
    schema_version: Option<&str>,
) -> Result<Admission, CodecError> {
    Ok(match schema_version {
        Some("1") => Admission::Schema1,
        Some(value) => Admission::Unverified {
            declaration: Some(ctx.copy_retained_text(value, "FreeCAD GUI schema declaration")?),
        },
        None => Admission::Unverified { declaration: None },
    })
}

#[cfg(test)]
mod tests {
    use super::{classify, Admission};
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::CodecError;

    #[test]
    fn admission_matches_the_verbatim_declaration() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let admitted = classify(&ctx, Some("1")).expect("verified declaration");
        assert_eq!(admitted, Admission::Schema1);
        assert_eq!(admitted.neutral_schema_version(), Some(1));
        for declaration in ["01", "2", "not-an-integer", "missing"] {
            let admission = classify(&ctx, Some(declaration)).expect("unverified declaration");
            assert_eq!(
                admission,
                Admission::Unverified {
                    declaration: Some(declaration.to_owned()),
                }
            );
            assert_eq!(admission.neutral_schema_version(), None);
        }
        let missing = classify(&ctx, None).expect("missing declaration");
        assert_eq!(missing, Admission::Unverified { declaration: None });
        assert_eq!(missing.neutral_schema_version(), None);
    }

    #[test]
    fn gui_schema_declaration_copy_propagates_retained_refusal() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        assert!(matches!(
            classify(&ctx, Some("01")),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD GUI schema declaration"
                    && limit.additional == 2
        ));
    }
}
