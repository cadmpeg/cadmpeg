// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    cadmpeg_test_support::assert_registry_closed(
        "rhino",
        &ArchiveVersion::ALL.map(ArchiveVersion::id),
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
    crate::chunks::ArchiveVersion::from_word(word).classify(None)
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
fn the_totality_row_names_the_declared_strategy_with_the_selected_width() {
    // The residual row is admitted, not refused: words 2 through 90 are one
    // chunked grammar, so a word no row claims still selects a scan. It names
    // the newest declared row with the width the word selected, and the charge
    // comes from the admission itself.
    for (word, nearest) in [(49, ArchiveVersion::V4), (51, ArchiveVersion::V9)] {
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
fn verified_rows_charge_no_dialect_loss() {
    for (word, _) in ENUMERATED {
        assert!(
            dialect_loss(&classify_word(*word)).is_none(),
            "archive word {word}: only the totality row is unverified"
        );
    }
}

#[test]
fn the_writer_stamp_is_declared_when_the_run_read_it_and_omitted_when_it_did_not() {
    let archive = crate::chunks::ArchiveVersion::from_word(80);

    let stamped = archive.classify(Some(2_348_836_140));
    assert_eq!(
        stamped.declared[DECLARED_OPENNURBS_WRITER_VERSION],
        "2348836140"
    );

    let unstamped = archive.classify(None);
    assert!(!unstamped
        .declared
        .contains_key(DECLARED_OPENNURBS_WRITER_VERSION));

    // The stamp is evidence, never an admission discriminant.
    assert_eq!(stamped.admission, unstamped.admission);
    assert_eq!(stamped.dialect, unstamped.dialect);
}
