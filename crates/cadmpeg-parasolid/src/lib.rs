// SPDX-License-Identifier: Apache-2.0
//! Shared Parasolid stream identity and header primitives.
//!
//! Parasolid is an embedded modelling-kernel layer in both NX and SLDPRT.
//! This crate owns the `parasolid:` dialect rows and the schema-token grammar
//! so hosts cannot disagree about the identity of the same declaration.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch, LayerInstance};

include!("registry_ids.rs");

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

/// Registry row named by one schema token, or the residual row.
///
/// Identity comes from this shared schema-token map so hosts cannot disagree
/// about the identity of the same declaration.
fn schema_row(schema: &str) -> DialectId {
    if schema.eq_ignore_ascii_case("SCH_SW_33103_11000") {
        PARASOLID_SCH_SW_33103
    } else if schema.eq_ignore_ascii_case("SCH_SW_32001_11000") {
        PARASOLID_SCH_SW_32001
    } else if schema
        .rsplit_once('_')
        .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case("13006"))
    {
        PARASOLID_FORMAT_13006
    } else {
        PARASOLID_UNKNOWN
    }
}

/// Classify one schema-bearing Parasolid stream and record its host carrier.
///
/// `instance` identifies the carrier when the host contains more than one
/// Parasolid stream. The schema and carrier are always retained verbatim as
/// declarations, independent of whether the schema has a named registry row.
/// `verified` lists the rows whose grammar the host applied and verified; every
/// other row, and the residual row, is admitted without verification.
#[must_use]
pub fn classify_layer(
    schema: &str,
    carrier: &str,
    instance: LayerInstance,
    verified: &[DialectId],
) -> DialectMatch {
    let id = schema_row(schema);
    let declared = BTreeMap::from([
        (DECLARED_SCHEMA.to_owned(), schema.to_owned()),
        (DECLARED_CARRIER.to_owned(), carrier.to_owned()),
    ]);
    let matched = if verified.contains(&id) {
        DialectMatch::admitted(id)
    } else {
        DialectMatch::residual(id)
    }
    .with_declared(declared);
    match instance {
        LayerInstance::Sole => matched,
        LayerInstance::Tagged => matched.with_instance(carrier),
    }
}

/// Classify every Parasolid carrier in one host document.
///
/// A lone layer needs no instance. Several layers use their carrier paths as
/// stable instance keys, so hosts cannot disagree about when identity needs a
/// disambiguator.
#[must_use]
pub fn extra_layers(streams: Vec<(String, String)>, verified: &[DialectId]) -> Vec<DialectMatch> {
    let instance = if streams.len() > 1 {
        LayerInstance::Tagged
    } else {
        LayerInstance::Sole
    };
    streams
        .into_iter()
        .map(|(schema, carrier)| classify_layer(&schema, &carrier, instance, verified))
        .collect()
}

/// Adds classified Parasolid layers and reports every uniqueness collision.
///
/// Hosts own their loss-code vocabulary. This helper owns the shared layer-set
/// operation and its explanation so NX and SLDPRT cannot describe the same
/// Parasolid collision differently.
pub fn push_extras(
    layers: &mut DialectLayers,
    extras: impl IntoIterator<Item = DialectMatch>,
) -> Vec<String> {
    let mut collisions = Vec::new();
    for layer in extras {
        let format = layer.format().to_owned();
        let carrier = layer.instance().unwrap_or("unidentified").to_owned();
        if layers.insert(layer).is_err() {
            collisions.push(format!(
                "the container produced a duplicate {format} dialect layer at carrier {carrier}; \
                 the later classification was omitted"
            ));
        }
    }
    collisions
}

/// Explain why a Parasolid layer was admitted without verification.
///
/// Host codecs own their loss vocabulary. This helper owns the interpretation
/// of the declarations produced by [`classify_layer`], so every host wraps the
/// same kernel fact in its codec-specific loss code.
#[must_use]
pub fn unverified_message(matched: &DialectMatch) -> Option<String> {
    if matched.format() != FORMAT
        || !matches!(
            matched.admission(),
            Admission::Unverified { .. } | Admission::Residual
        )
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
    if matched.dialect() == &PARASOLID_UNKNOWN {
        return Some(format!(
            "The Parasolid stream at {carrier} declares schema {schema:?}, which has no declared \
             grammar. It was admitted as the `{}` residual layer without substituting another \
             schema grammar; bounded structural recovery retains the source stream.",
            matched.dialect()
        ));
    }
    Some(format!(
        "The Parasolid stream at {carrier} declares schema {schema:?}, which maps to the named \
         `{}` row, but the host did not verify that row's schema grammar. It was admitted \
         without substituting another schema grammar; bounded structural recovery retains the \
         source stream.",
        matched.dialect()
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const ALL_ROWS: [DialectId; 3] = [
        PARASOLID_SCH_SW_33103,
        PARASOLID_SCH_SW_32001,
        PARASOLID_FORMAT_13006,
    ];

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
            let matched = classify_layer(schema, "stream@12", LayerInstance::Sole, &ALL_ROWS);
            assert_eq!(matched.dialect().as_str(), expected);
            assert_eq!(matched.admission(), &Admission::Admitted);
            assert_eq!(matched.declared()[DECLARED_SCHEMA], schema);
            assert_eq!(matched.declared()[DECLARED_CARRIER], "stream@12");
            assert_eq!(matched.instance(), None);
        }
    }

    #[test]
    fn residual_schemas_use_residual_admission_without_a_substitution() {
        let matched = classify_layer(
            "SCH_TEST_1_9999",
            "block@7:body+3",
            LayerInstance::Tagged,
            &ALL_ROWS,
        );

        assert_eq!(matched.dialect().as_str(), "parasolid:unknown");
        assert_eq!(matched.admission(), &Admission::Residual);
        assert_eq!(matched.declared()[DECLARED_SCHEMA], "SCH_TEST_1_9999");
        assert_eq!(matched.declared()[DECLARED_CARRIER], "block@7:body+3");
        assert_eq!(matched.instance(), Some("block@7:body+3"));
        let message = unverified_message(&matched).expect("residual layer explains its recovery");
        assert!(message.contains("SCH_TEST_1_9999"));
        assert!(message.contains("block@7:body+3"));
    }

    #[test]
    fn several_layers_receive_carrier_instances() {
        let layers = extra_layers(
            vec![
                ("SCH_SW_33103_11000".to_owned(), "stream@12".to_owned()),
                ("SCH_TEST_1_9999".to_owned(), "stream@48".to_owned()),
            ],
            &ALL_ROWS,
        );
        assert_eq!(layers[0].instance(), Some("stream@12"));
        assert_eq!(layers[1].instance(), Some("stream@48"));

        let one = extra_layers(
            vec![("SCH_SW_33103_11000".to_owned(), "stream@12".to_owned())],
            &ALL_ROWS,
        );
        assert_eq!(one[0].instance(), None);
    }

    #[test]
    fn push_extras_preserves_the_first_layer_and_reports_later_collisions() {
        let mut layers = DialectLayers::of(DialectMatch::admitted(
            DialectId::parse("nx:splmsstr").expect("valid host dialect id"),
        ));
        let first = classify_layer(
            "SCH_SW_33103_11000",
            "stream@12",
            LayerInstance::Tagged,
            &[],
        );
        let later = classify_layer(
            "SCH_SW_32001_11000",
            "stream@12",
            LayerInstance::Tagged,
            &[],
        );

        let collisions = push_extras(&mut layers, [first.clone(), later]);

        assert_eq!(layers.iter().skip(1).collect::<Vec<_>>(), [&first]);
        assert_eq!(
            collisions,
            [
                "the container produced a duplicate parasolid dialect layer at carrier stream@12; \
              the later classification was omitted"
            ]
        );
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
            classify_layer(schema, "carrier", LayerInstance::Sole, &ALL_ROWS)
                .dialect()
                .to_string()
        })
        .into_iter()
        .collect();
        assert_eq!(ids, cadmpeg_test_support::registry_ids(FORMAT));
    }

    #[test]
    fn a_known_row_can_be_identified_without_claiming_host_verification() {
        let matched = classify_layer(
            "SCH_3501171_35102_13006",
            "stream@12",
            LayerInstance::Sole,
            &[PARASOLID_SCH_SW_33103],
        );

        assert_eq!(matched.dialect().as_str(), "parasolid:format-13006");
        assert_eq!(matched.admission(), &Admission::Residual);
        let message = unverified_message(&matched).expect("unverified row explains its admission");
        assert!(message.contains("host did not verify"));
        assert!(message.contains("parasolid:format-13006"));
    }
}
