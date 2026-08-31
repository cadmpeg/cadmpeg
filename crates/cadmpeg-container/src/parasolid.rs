// SPDX-License-Identifier: Apache-2.0
//! Shared identity classification for embedded Parasolid streams.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};

/// Dialect namespace owned by the Parasolid stream classifier.
pub const FORMAT: &str = "parasolid";

/// Declared-key name for the source schema token.
pub const DECLARED_SCHEMA: &str = "schema";
/// Declared-key name for the host location carrying the stream.
pub const DECLARED_CARRIER: &str = "carrier";

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
/// of the declarations produced by [`classify_layer`], so every host can wrap
/// the same kernel fact in its codec-specific loss code.
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
    use super::*;

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
    fn admitted_schemas_do_not_claim_unverified_recovery() {
        let matched = classify_layer("SCH_SW_33103_11000", "stream@12", false);
        assert_eq!(unverified_message(&matched), None);
    }
}
