// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Path of the identity registry, from this crate's manifest directory.
fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/dialects.toml")
        .canonicalize()
        .expect("docs/dialects.toml resolves from the crate manifest directory")
}

/// Every `id = "rhino:…"` value in `docs/dialects.toml`.
fn registry_ids() -> BTreeSet<String> {
    let text = std::fs::read_to_string(registry_path()).expect("read docs/dialects.toml");
    let ids = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("id = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|id| id.starts_with("rhino:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no rhino rows");
    ids
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    let pinned = RhinoDialect::ALL
        .iter()
        .map(|dialect| dialect.id().as_str().to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        pinned.len(),
        RhinoDialect::ALL.len(),
        "two variants pin the same id"
    );
    assert_eq!(
        pinned,
        registry_ids(),
        "docs/dialects.toml and RhinoDialect disagree; ids are pinned forever, so reconcile the enum"
    );
}

/// Every archive word with a row of its own, beside the row it must reach.
const ENUMERATED: &[(u64, &str)] = &[
    (1, "rhino:archive-1"),
    (2, "rhino:archive-2"),
    (3, "rhino:archive-3"),
    (4, "rhino:archive-4"),
    (5, "rhino:archive-5"),
    (50, "rhino:archive-50"),
    (60, "rhino:archive-60"),
    (70, "rhino:archive-70"),
    (80, "rhino:archive-80"),
    (90, "rhino:archive-90"),
];

/// Archive words the registry enumerates no row for.
///
/// 51 and 61 are the near-miss cases: the discriminant is exact equality, so a
/// word one away from a declared one is not that row with extras.
const OUTSIDE: &[u64] = &[6, 7, 40, 49, 51, 61, 71, 81, 89, 91, 100, u64::MAX];

fn classify_word(word: u64) -> cadmpeg_core::dialect::DialectMatch {
    RhinoDialect::classify(crate::chunks::ArchiveVersion::classify(word), None)
}

#[test]
fn each_enumerated_word_classifies_into_the_row_its_discriminant_matches() {
    for (word, id) in ENUMERATED {
        let matched = classify_word(*word);
        assert_eq!(matched.format, FORMAT, "archive word {word}");
        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some(*id),
            "archive word {word}"
        );
        assert_eq!(
            matched.declared[DECLARED_ARCHIVE_VERSION],
            word.to_string(),
            "archive word {word}: the declaration is recorded as the header made it"
        );
    }
}

#[test]
fn the_totality_row_absorbs_every_word_the_registry_omits() {
    for word in OUTSIDE {
        let matched = classify_word(*word);
        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some("rhino:unknown"),
            "archive word {word} has no declared row"
        );
        assert_eq!(
            matched.declared[DECLARED_ARCHIVE_VERSION],
            word.to_string(),
            "archive word {word}: an unclassified word still records its declaration"
        );
    }
}

#[test]
fn admission_is_refused_exactly_where_decode_is_refused() {
    // The biconditional the single predicate exists to guarantee: one function
    // decides both the refusal `container::decode` returns and the admission
    // the report carries, so a document can never be decoded while its report
    // says the row was refused, or refused while the report says admitted.
    let words = ENUMERATED
        .iter()
        .map(|(word, _)| *word)
        .chain(OUTSIDE.iter().copied());
    for word in words {
        let archive = crate::chunks::ArchiveVersion::classify(word);
        let matched = RhinoDialect::classify(archive, None);
        assert_eq!(
            matched.admission == Admission::Refused,
            refuses_decode(archive),
            "archive word {word}: admission and the decode refusal must agree"
        );
    }
}

#[test]
fn the_totality_row_names_the_declared_strategy_with_the_selected_width() {
    // The residual row is admitted, not refused: words 2 through 90 are one
    // chunked grammar, so a word no row claims still selects a scan. It names
    // the newest declared row with the width the word selected, and the charge
    // comes from the admission itself.
    for (word, nearest) in [(49, RhinoDialect::Archive4), (51, RhinoDialect::Archive90)] {
        let matched = classify_word(word);
        assert_eq!(
            matched.admission,
            Admission::AdmittedUnverified {
                nearest: nearest.id()
            },
            "archive word {word}"
        );
        let note = dialect_loss(&matched).expect("an unverified admission charges its loss");
        assert_eq!(
            note.code,
            crate::loss::RhinoLossCode::SourceDialectUnverified.kind(),
            "archive word {word}"
        );
        assert!(
            note.message.contains(&word.to_string()),
            "archive word {word}: the observed word is reported"
        );
    }
}

#[test]
fn a_verified_row_and_a_refused_row_charge_no_dialect_loss() {
    for (word, _) in ENUMERATED {
        assert!(
            dialect_loss(&classify_word(*word)).is_none(),
            "archive word {word}: only the totality row is unverified"
        );
    }
}

#[test]
fn only_archive_5_is_refused() {
    for dialect in RhinoDialect::ALL {
        let refused = matches!(dialect, RhinoDialect::Archive5);
        assert_eq!(
            dialect.refuses_decode(),
            refused,
            "{}: the codec's grammar coverage moved without the registry disposition",
            dialect.id()
        );
    }
}

#[test]
fn the_writer_stamp_is_declared_when_the_run_read_it_and_omitted_when_it_did_not() {
    let archive = crate::chunks::ArchiveVersion::classify(80);

    let stamped = RhinoDialect::classify(archive, Some(2_348_836_140));
    assert_eq!(
        stamped.declared[DECLARED_OPENNURBS_WRITER_VERSION],
        "2348836140"
    );

    let unstamped = RhinoDialect::classify(archive, None);
    assert!(!unstamped
        .declared
        .contains_key(DECLARED_OPENNURBS_WRITER_VERSION));

    // The stamp is evidence, never an admission discriminant.
    assert_eq!(stamped.admission, unstamped.admission);
    assert_eq!(stamped.dialect, unstamped.dialect);
}
