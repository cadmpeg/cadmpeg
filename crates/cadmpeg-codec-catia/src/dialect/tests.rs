// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list. The witnesses are the registry's own: each
//! row cites a golden fixture, and the matrix below scans those bytes.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::container;
use crate::test_support::{outer_body_catpart, summary_preview_segment};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    cadmpeg_test_support::assert_registry_closed("catia", &Variant::ALL.map(Variant::id));
}

/// One registry row: the witness fixture it cites and the id it must classify
/// into.
struct Witness {
    /// Fixture basename under `tests/golden/fixtures`.
    fixture: &'static str,
    /// The `id` of the row this fixture witnesses.
    id: &'static str,
    /// Whether the row's declared strategy was the one applied.
    admitted: bool,
}

/// The `witness = "corpus:…"` fixture of every `catia:` row, in registry order.
const WITNESSES: &[Witness] = &[
    Witness {
        fixture: "standard_part.catpart",
        id: "catia:standard-nested",
        admitted: true,
    },
    Witness {
        fixture: "fbb_only_fallthrough.catpart",
        id: "catia:fbb-only",
        admitted: true,
    },
    Witness {
        fixture: "e5_circle.catpart",
        id: "catia:e5-stream",
        admitted: true,
    },
    Witness {
        fixture: "zero_entity_cylinder.catpart",
        id: "catia:zero-entity",
        admitted: true,
    },
    Witness {
        fixture: "freeform_b5_topology.catpart",
        id: "catia:float-packed-inner-no-fbb",
        admitted: true,
    },
    Witness {
        fixture: "inner_no_directory_b2_cylinder.catpart",
        id: "catia:inner-no-directory",
        admitted: true,
    },
    Witness {
        fixture: "outer_directory_only.catpart",
        id: "catia:unknown",
        admitted: false,
    },
];

/// Reads one golden fixture by basename.
fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn every_registry_row_is_witnessed_by_the_fixture_it_cites() {
    let mut seen = BTreeSet::new();
    for witness in WITNESSES {
        let bytes = fixture_bytes(witness.fixture);
        let scan = container::scan_bytes(bytes.as_slice());
        let matched = classify(&scan);

        assert_eq!(matched.format(), FORMAT, "{}", witness.fixture);
        assert_eq!(
            matched.dialect().as_str(),
            witness.id,
            "{} must witness its own row",
            witness.fixture
        );
        seen.insert(witness.id.to_owned());
    }
    assert_eq!(
        seen,
        cadmpeg_test_support::registry_ids("catia"),
        "every catia row needs a witness fixture in this matrix"
    );
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    for witness in WITNESSES {
        let bytes = fixture_bytes(witness.fixture);
        let scan = container::scan_bytes(bytes.as_slice());
        let matched = classify(&scan);
        let charged = dialect_loss(&matched).is_some();

        assert_eq!(
            witness.admitted, !charged,
            "{}: the witness table and the charged loss disagree",
            witness.fixture
        );
        assert_eq!(
            matched.admission() == Admission::Admitted,
            !charged,
            "{}: admission and the dialect-unverified loss must agree",
            witness.fixture
        );
    }
}

#[test]
fn the_totality_row_is_admitted_unverified_and_names_itself() {
    assert_eq!(
        admission(Variant::Unknown),
        Admission::AdmittedUnverified {
            nearest: DialectId::pinned("catia:unknown"),
        },
        "no CATIA family's grammar is substituted for an unrecognized layout, so the only \
         honest referent for `nearest` is the row whose declared disposition is the \
         metadata-IR fallback itself"
    );
    for variant in Variant::ALL {
        if variant != Variant::Unknown {
            assert_eq!(admission(variant), Admission::Admitted, "{variant:?}");
        }
    }
}

/// No golden fixture carries a `LastSaveVersion` tuple, so the declaration is
/// exercised against the in-tree summary-information segment, whose tags are
/// `<Version>5`, `<Release>27`, `<ServicePack>2`, `<HotFix>0`, and
/// `<BuildDate>03-10-2017.22.00`.
///
/// The same container also states the declaration is evidence and not identity:
/// it carries a complete CATIA V5R27 stamp and still classifies as
/// `catia:unknown`, because no storage family's structural invariants hold in
/// it. A declaration cannot promote a file into a family.
#[test]
fn the_last_save_declaration_is_recorded_as_the_source_wrote_it() {
    let bytes = outer_body_catpart(&summary_preview_segment());
    let scan = container::scan_bytes(bytes.as_slice());
    let matched = classify(&scan);

    assert_eq!(matched.declared()[DECLARED_VERSION], "5");
    assert_eq!(matched.declared()[DECLARED_RELEASE], "27");
    assert_eq!(matched.declared()[DECLARED_SERVICE_PACK], "2");
    assert_eq!(matched.declared()[DECLARED_HOT_FIX], "0");
    assert_eq!(matched.declared()[DECLARED_BUILD_DATE], "03-10-2017.22.00");

    assert_eq!(matched.dialect().as_str(), "catia:unknown");
    assert_eq!(
        matched.admission(),
        Admission::AdmittedUnverified {
            nearest: DialectId::pinned("catia:unknown"),
        }
    );
}

/// The declaration is evidence, never identity: a file with no
/// `LastSaveVersion` tuple still classifies into its structural row.
#[test]
fn an_absent_declaration_leaves_the_identity_intact() {
    for witness in WITNESSES {
        let bytes = fixture_bytes(witness.fixture);
        let scan = container::scan_bytes(bytes.as_slice());
        let matched = classify(&scan);
        assert_eq!(
            matched.declared().is_empty(),
            scan.last_save_version.is_none(),
            "{}: declared keys track the tuple's presence",
            witness.fixture
        );
        assert!(
            !matched.dialect().as_str().is_empty(),
            "{}",
            witness.fixture
        );
    }
}
