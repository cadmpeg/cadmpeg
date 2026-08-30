// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;
use std::collections::BTreeSet;

#[test]
fn exactly_the_alternate_encoding_rows_carry_a_refusal_message() {
    let refused = [
        StepDialect::Ap203Edition1,
        StepDialect::Ap203Edition2,
        StepDialect::Ap214,
        StepDialect::Ap242,
        StepDialect::Ap242Edition1,
        StepDialect::Ap242Edition2,
        StepDialect::Ap242Edition3,
        StepDialect::Part28Xml,
        StepDialect::Ap242BoModelXml,
        StepDialect::Part26Hdf5,
        StepDialect::Unknown,
    ]
    .iter()
    .filter(|dialect| dialect.alternate_encoding_refusal().is_some())
    .map(|dialect| dialect.id().as_str().to_owned())
    .collect::<BTreeSet<_>>();

    assert_eq!(
        refused,
        BTreeSet::from([
            "step:part26-hdf5".to_owned(),
            "step:part28-xml".to_owned(),
            "step:ap242-bo-model-xml".to_owned(),
        ]),
        "the alternate encodings are the rows refused below decode; every other row is Part 21"
    );
}

/// A Part 21 exchange whose `FILE_SCHEMA` list is `identifiers` and whose
/// `FILE_DESCRIPTION` implementation level is `level`.
fn exchange(identifiers: &[&str], level: &str) -> crate::parse::Exchange {
    let list = identifiers
        .iter()
        .map(|identifier| format!("'{identifier}'"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'{level}');\
         FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(({list}));ENDSEC;\
         DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
    );
    crate::parse::parse(source.as_bytes())
        .expect("the fixture exchange parses")
        .0
}

/// One matrix row: a declaration and the row its discriminants match.
struct Case {
    /// The `FILE_SCHEMA` identifiers, verbatim as written on the card.
    identifiers: &'static [&'static str],
    /// Registry id the declaration must classify into.
    id: &'static str,
    /// The `long_form_arcs` key the match must carry, or `None` when the
    /// classified identifier declares no object identifier.
    arcs: Option<&'static str>,
}

/// Declarations spanning every Part 21 row and the totality row.
///
/// Ids come from `docs/dialects.toml`. Four rows share the AP242 schema name
/// and separate on the object identifier, which Part 21 makes optional: absent
/// is `step:ap242`, each declared edition has its own row, and an edition claim
/// naming no declared edition satisfies no row. The three single-row names
/// carry only the name, so a bare name satisfies theirs.
const CASES: &[Case] = &[
    Case {
        identifiers: &["CONFIG_CONTROL_DESIGN"],
        id: "step:ap203-e1",
        arcs: None,
    },
    Case {
        identifiers: &[
            "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF",
        ],
        id: "step:ap203-e2",
        arcs: None,
    },
    Case {
        identifiers: &[
            "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF { 1 0 10303 403 2 1 2 }",
        ],
        id: "step:ap203-e2",
        arcs: Some(" 1 0 10303 403 2 1 2 "),
    },
    Case {
        identifiers: &["AUTOMOTIVE_DESIGN"],
        id: "step:ap214",
        arcs: None,
    },
    Case {
        identifiers: &["AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }"],
        id: "step:ap214",
        arcs: Some(" 1 0 10303 214 1 1 1 1 "),
    },
    Case {
        identifiers: &["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"],
        id: "step:ap242-e1",
        arcs: Some(" 1 0 10303 442 1 1 4 "),
    },
    Case {
        identifiers: &["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }"],
        id: "step:ap242-e2",
        arcs: Some(" 1 0 10303 442 3 1 4 "),
    },
    Case {
        identifiers: &["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }"],
        id: "step:ap242-e3",
        arcs: Some(" 1 0 10303 442 4 1 4 "),
    },
    // The AP242 name with no object identifier: a complete declaration naming
    // no edition. Its own row, not the totality row.
    Case {
        identifiers: &["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"],
        id: "step:ap242",
        arcs: None,
    },
    // The AP242 name with arcs no edition declares: an edition claim matching
    // nothing, which is unrecognized rather than unspecified.
    Case {
        identifiers: &["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 9 1 4 }"],
        id: "step:unknown",
        arcs: Some(" 1 0 10303 442 9 1 4 "),
    },
    // The AP242 name with arcs that do not read as a numeric object
    // identifier. Same call, same answer as arcs naming no edition.
    Case {
        identifiers: &["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 x 442 }"],
        id: "step:unknown",
        arcs: Some(" 1 0 x 442 "),
    },
    // The literal string 'AP242' is not the AP242 schema name.
    Case {
        identifiers: &["AP242"],
        id: "step:unknown",
        arcs: None,
    },
    Case {
        identifiers: &["AP214"],
        id: "step:unknown",
        arcs: None,
    },
    Case {
        identifiers: &["AUTOMOTIVE_DESIGN_CC2"],
        id: "step:unknown",
        arcs: None,
    },
    // The identity is the first identifier of the list.
    Case {
        identifiers: &["AUTOMOTIVE_DESIGN", "CONFIG_CONTROL_DESIGN"],
        id: "step:ap214",
        arcs: None,
    },
];

#[test]
fn each_declaration_classifies_into_the_row_its_discriminants_match() {
    for case in CASES {
        let matched = StepDialect::classify(&exchange(case.identifiers, "2;1"));
        let context = format!("FILE_SCHEMA {:?}", case.identifiers);

        assert_eq!(matched.dialect().as_str(), case.id, "{context}");
        assert_eq!(
            matched.declared()[DECLARED_FILE_SCHEMA_IDENTIFIER],
            case.identifiers[0],
            "{context}: the declaration is recorded as the source made it"
        );
        assert_eq!(
            matched
                .declared()
                .get(DECLARED_LONG_FORM_ARCS)
                .map(String::as_str),
            case.arcs,
            "{context}: the arcs are recorded verbatim when the identifier carries them"
        );
        assert_eq!(
            matched.declared()[DECLARED_IMPLEMENTATION_LEVEL],
            "2;1",
            "{context}: the implementation level is evidence, recorded and not classified on"
        );
        assert_eq!(
            matched
                .declared()
                .contains_key(DECLARED_FILE_SCHEMA_IDENTIFIERS),
            case.identifiers.len() > 1,
            "{context}: the whole list is recorded only when it declares more than one identifier"
        );
        assert_eq!(matched.format(), FORMAT, "{context}");
    }
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    let expected = StepLossCode::SourceDialectUnverified
        .note(String::new())
        .code;
    for case in CASES {
        let matched = StepDialect::classify(&exchange(case.identifiers, "2;1"));
        let charged = dialect_loss(&matched).is_some_and(|note| note.code == expected);
        let admitted = matched.admission() == Admission::Admitted;

        assert_eq!(
            admitted, !charged,
            "FILE_SCHEMA {:?}: admission and the dialect-unverified loss must agree",
            case.identifiers
        );
        assert_eq!(
            admitted,
            case.id != StepDialect::Unknown.id().as_str(),
            "FILE_SCHEMA {:?}: only the totality row is unverified",
            case.identifiers
        );
        if !admitted {
            assert_eq!(
                matched.admission(),
                Admission::AdmittedUnverified {
                    using: NEAREST_STRATEGY.id(),
                },
                "FILE_SCHEMA {:?}: `using` names the strategy actually applied",
                case.identifiers
            );
        }
    }
}

#[test]
fn the_edition_unspecified_row_is_admitted_and_charges_nothing() {
    // The AP242 name with no object identifier declares the schema and leaves
    // the edition unspecified. The reader's single Part 21 grammar is that
    // row's declared strategy and the edition axis is undeclared rather than
    // substituted, so this is a verified read: `DecodeMode::Strict` accepts it.
    let matched = StepDialect::classify(&exchange(
        &["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"],
        "2;1",
    ));

    assert_eq!(matched.dialect().as_str(), "step:ap242");
    assert_eq!(matched.admission(), Admission::Admitted);
    assert!(dialect_loss(&matched).is_none());
    assert!(!matched.declared().contains_key(DECLARED_LONG_FORM_ARCS));
}

#[test]
fn a_future_ap242_edition_word_uses_the_unverified_edition_three_strategy() {
    let dialect = StepDialect::from_ap242_edition(None);
    assert_eq!(dialect, StepDialect::Unknown);
    assert_eq!(
        dialect.admission(),
        Admission::AdmittedUnverified {
            using: StepDialect::Ap242Edition3.id(),
        }
    );
}

#[test]
fn the_implementation_level_is_recorded_and_never_classified_on() {
    // The five levels the parser admits select different section grammars.
    // None of them moves the identity, and each is recorded verbatim.
    for level in ["1", "2", "2;1", "2;2", "3;1", "3;2", "4;1", "4;2", "4;3"] {
        let matched = StepDialect::classify(&exchange(&["AUTOMOTIVE_DESIGN"], level));

        assert_eq!(
            matched.dialect().as_str(),
            "step:ap214",
            "implementation level {level:?} must not move the identity"
        );
        assert_eq!(matched.declared()[DECLARED_IMPLEMENTATION_LEVEL], level);
    }
}
