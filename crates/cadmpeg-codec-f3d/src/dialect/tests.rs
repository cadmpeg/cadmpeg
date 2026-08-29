// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    cadmpeg_test_support::assert_registry_closed("f3d", &F3dDialect::ALL.map(F3dDialect::id));
}

#[test]
fn a_document_match_names_its_row_and_records_the_version_the_parse_read() {
    let matched = F3dDialect::classify_document("3-2-0-0");

    assert_eq!(matched.format(), FORMAT);
    assert_eq!(matched.dialect().as_str(), "f3d:manifest-3-2-0-0");
    assert_eq!(
        matched.declared()[DECLARED_TOP_LEVEL_MANIFEST_VERSION],
        "3-2-0-0"
    );
    assert_eq!(matched.admission(), Admission::Admitted);
}

#[test]
fn a_version_only_drift_lands_on_the_recovery_row_and_charges_the_loss() {
    // The parse ran the `3-2-0-0` layout and it fitted. The declaration names
    // no row this codec knows, so the reading is recorded verbatim, the row is
    // the recovery row, and the admission names the strategy applied.
    let matched = F3dDialect::classify_document("3-3-0-0");

    assert_eq!(
        matched.declared()[DECLARED_TOP_LEVEL_MANIFEST_VERSION],
        "3-3-0-0"
    );
    assert_eq!(matched.dialect().as_str(), "f3d:unknown");
    assert_eq!(
        matched.admission(),
        Admission::AdmittedUnverified {
            using: DialectId::pinned("f3d:manifest-3-2-0-0"),
        }
    );

    let loss = dialect_loss(&matched).expect("the recovery is charged");
    assert_eq!(loss.code, F3dLossCode::SourceDialectUnverified.kind());
    assert!(loss.message.contains("3-3-0-0"));
    assert!(loss.message.contains("f3d:manifest-3-2-0-0"));
}

#[test]
fn an_f3z_match_names_its_row_and_records_the_root_members() {
    let matched = F3dDialect::classify_f3z(&["Assembly.f3d", "Part.f3d"]);

    assert_eq!(matched.format(), FORMAT);
    assert_eq!(matched.dialect().as_str(), "f3d:f3z-multi-document");
    assert_eq!(
        matched.declared()[DECLARED_ROOT_DOCUMENT_MEMBERS],
        "Assembly.f3d,Part.f3d"
    );
    assert!(
        !matched
            .declared()
            .contains_key(DECLARED_TOP_LEVEL_MANIFEST_VERSION),
        "the F3Z branch reads no version field, so it must declare none"
    );
    assert_eq!(matched.admission(), Admission::Admitted);
}

#[test]
fn the_identity_rows_are_admitted_and_charge_nothing() {
    // A row parsed with the strategy it declares carries no recovery. The loss
    // and the admission are read from one value, so this pins both halves.
    for matched in [
        F3dDialect::classify_document("3-2-0-0"),
        F3dDialect::classify_f3z(&["Part.f3d"]),
    ] {
        assert_eq!(matched.admission(), Admission::Admitted);
        assert!(dialect_loss(&matched).is_none());
    }
}

#[test]
fn the_totality_row_is_the_only_row_a_foreign_version_reaches() {
    assert_eq!(F3dDialect::Unknown.id().as_str(), "f3d:unknown");
    for matched in [
        F3dDialect::classify_document("3-2-0-0"),
        F3dDialect::classify_f3z(&["Part.f3d"]),
    ] {
        assert_ne!(
            matched.dialect().as_str(),
            F3dDialect::Unknown.id().as_str()
        );
    }
    assert_eq!(
        F3dDialect::classify_document("4-0-0-0").dialect().as_str(),
        F3dDialect::Unknown.id().as_str()
    );
}
