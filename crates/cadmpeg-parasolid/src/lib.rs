// SPDX-License-Identifier: Apache-2.0
//! Shared Parasolid stream identity and header primitives.
//!
//! Parasolid is an embedded modelling-kernel layer in both NX and SLDPRT.
//! This crate owns the `parasolid:` dialect rows and the schema-token grammar
//! so hosts cannot disagree about the identity of the same declaration.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};

/// Dialect namespace owned by the Parasolid stream classifier.
pub const FORMAT: &str = "parasolid";

/// Declared-key name for the source schema token.
pub const DECLARED_SCHEMA: &str = "schema";
/// Declared-key name for the host location carrying the stream.
pub const DECLARED_CARRIER: &str = "carrier";

/// One exact ASCII `SCH_` token and its location in a supplied prologue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaToken<'a> {
    value: &'a str,
    offset: usize,
}

impl<'a> SchemaToken<'a> {
    /// Exact token text, including the `SCH_` prefix.
    #[must_use]
    pub const fn value(self) -> &'a str {
        self.value
    }

    /// Byte offset of the `S` in the supplied prologue.
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    /// Exclusive byte end of the token in the supplied prologue.
    #[must_use]
    pub const fn end(self) -> usize {
        self.offset + self.value.len()
    }
}

/// Find the first complete Parasolid schema token in a bounded prologue.
///
/// The caller owns the carrier-specific bound. The token grammar is shared:
/// `SCH_` followed by one or more ASCII alphanumeric or underscore bytes.
#[must_use]
pub fn find_schema_token(prologue: &[u8]) -> Option<SchemaToken<'_>> {
    prologue
        .windows(4)
        .enumerate()
        .filter(|(_, bytes)| *bytes == b"SCH_")
        .find_map(|(offset, _)| {
            let mut end = offset + 4;
            while end < prologue.len()
                && (prologue[end].is_ascii_alphanumeric() || prologue[end] == b'_')
            {
                end += 1;
            }
            schema_token(prologue, offset, end)
        })
}

/// Find a complete schema token whose byte length immediately precedes it.
///
/// This is the `SLDPRT` embedded-header form. The declared length bounds the
/// token even when the first record begins with an ASCII token character.
#[must_use]
pub fn find_u8_length_prefixed_schema_token(prologue: &[u8]) -> Option<SchemaToken<'_>> {
    prologue
        .windows(4)
        .enumerate()
        .filter(|(_, bytes)| *bytes == b"SCH_")
        .find_map(|(offset, _)| {
            let length = usize::from(*prologue.get(offset.checked_sub(1)?)?);
            let end = offset.checked_add(length)?;
            schema_token(prologue, offset, end)
        })
}

fn schema_token(prologue: &[u8], offset: usize, end: usize) -> Option<SchemaToken<'_>> {
    let bytes = prologue.get(offset..end)?;
    (bytes.len() > 4
        && bytes.starts_with(b"SCH_")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_'))
    .then_some(())?;
    let value = std::str::from_utf8(bytes).ok()?;
    Some(SchemaToken { value, offset })
}

/// Classify one schema-bearing Parasolid stream and record its host carrier.
///
/// `instance_tagged` identifies the carrier when the host contains more than
/// one Parasolid stream. The schema and carrier are always retained verbatim as
/// declarations, independent of whether the schema has a named registry row.
#[must_use]
pub fn classify_layer(schema: &str, carrier: &str, instance_tagged: bool) -> DialectMatch {
    let (id, admitted) = if schema.eq_ignore_ascii_case("SCH_SW_33103_11000") {
        ("parasolid:sch-sw-33103", true)
    } else if schema.eq_ignore_ascii_case("SCH_SW_32001_11000") {
        ("parasolid:sch-sw-32001", true)
    } else if schema
        .rsplit_once('_')
        .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case("13006"))
    {
        ("parasolid:format-13006", true)
    } else {
        ("parasolid:unknown", false)
    };
    let declared = BTreeMap::from([
        (DECLARED_SCHEMA.to_owned(), schema.to_owned()),
        (DECLARED_CARRIER.to_owned(), carrier.to_owned()),
    ]);
    let matched = if admitted {
        DialectMatch::admitted(DialectId::pinned(id))
    } else {
        DialectMatch::residual(DialectId::pinned(id))
    }
    .with_declared(declared);
    if instance_tagged {
        matched.with_instance(carrier)
    } else {
        matched
    }
}

/// Explain why a residual Parasolid layer is admitted without verification.
///
/// Host codecs own their loss vocabulary. This helper owns the interpretation
/// of the declarations produced by [`classify_layer`], so every host wraps the
/// same kernel fact in its codec-specific loss code.
#[must_use]
pub fn unverified_message(matched: &DialectMatch) -> Option<String> {
    if matched.format() != FORMAT
        || !matches!(matched.admission(), Admission::AdmittedUnverified { .. })
    {
        return None;
    }

    let schema = matched
        .declared()
        .get(DECLARED_SCHEMA)
        .map_or("<unrecorded>", String::as_str);
    let carrier = matched
        .declared()
        .get(DECLARED_CARRIER)
        .map_or("<unrecorded>", String::as_str);
    Some(format!(
        "The Parasolid stream at {carrier} declares schema {schema:?}, which has no declared \
         grammar. It was admitted as the `{}` residual layer without substituting another \
         schema grammar; bounded structural recovery retains the source stream.",
        matched.dialect()
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn schema_token_uses_one_exact_ascii_grammar() {
        let token =
            find_schema_token(b"prologue\0SCH_3501171_35102_13006\0body").expect("complete token");
        assert_eq!(token.value(), "SCH_3501171_35102_13006");
        assert_eq!(token.offset(), 9);
        assert_eq!(token.end(), 32);

        assert!(find_schema_token(b"SCH_").is_none());
        assert_eq!(
            find_schema_token(b"SCH_-SCH_REAL")
                .expect("the first complete token is selected")
                .value(),
            "SCH_REAL"
        );
        assert_eq!(
            find_schema_token(b"SCH_TEST-ignored")
                .expect("token before delimiter")
                .value(),
            "SCH_TEST"
        );

        let mut prefixed = b"padding".to_vec();
        prefixed.push(8);
        prefixed.extend_from_slice(b"SCH_TEST");
        prefixed.extend_from_slice(b"1234");
        let token = find_u8_length_prefixed_schema_token(&prefixed)
            .expect("the declared length bounds the token");
        assert_eq!(token.value(), "SCH_TEST");
        assert_eq!(token.end(), 16);
    }

    #[test]
    fn named_schemas_and_the_format_suffix_map_to_their_rows_case_insensitively() {
        for (schema, expected) in [
            ("sch_sw_33103_11000", "parasolid:sch-sw-33103"),
            ("Sch_Sw_32001_11000", "parasolid:sch-sw-32001"),
            ("SCH_3201255_32001_13006", "parasolid:format-13006"),
        ] {
            let matched = classify_layer(schema, "stream@12", false);
            assert_eq!(matched.dialect().as_str(), expected);
            assert_eq!(matched.admission(), Admission::Admitted);
            assert_eq!(matched.declared()[DECLARED_SCHEMA], schema);
            assert_eq!(matched.declared()[DECLARED_CARRIER], "stream@12");
            assert_eq!(matched.instance(), None);
        }
    }

    #[test]
    fn residual_schemas_use_unverified_admission_without_a_substitution() {
        let matched = classify_layer("SCH_TEST_1_9999", "block@7:body+3", true);

        assert_eq!(matched.dialect().as_str(), "parasolid:unknown");
        assert_eq!(
            matched.admission(),
            Admission::AdmittedUnverified { using: None }
        );
        assert_eq!(matched.declared()[DECLARED_SCHEMA], "SCH_TEST_1_9999");
        assert_eq!(matched.declared()[DECLARED_CARRIER], "block@7:body+3");
        assert_eq!(matched.instance(), Some("block@7:body+3"));
        let message = unverified_message(&matched).expect("residual layer explains its recovery");
        assert!(message.contains("SCH_TEST_1_9999"));
        assert!(message.contains("block@7:body+3"));
    }

    #[test]
    fn every_parasolid_registry_row_is_produced() {
        let ids: BTreeSet<_> = [
            "SCH_SW_33103_11000",
            "SCH_SW_32001_11000",
            "SCH_3501171_35102_13006",
            "SCH_TEST_1_9999",
        ]
        .map(|schema| {
            classify_layer(schema, "carrier", false)
                .dialect()
                .to_string()
        })
        .into_iter()
        .collect();
        assert_eq!(ids, cadmpeg_test_support::registry_ids(FORMAT));
    }
}
