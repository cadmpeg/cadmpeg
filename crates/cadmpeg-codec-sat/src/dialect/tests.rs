// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::loss::SatLossCode;
use crate::test_support::{binary_sphere_stream, text_sphere_stream, BinaryFixtureKind};
use crate::SatCodec;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::PathBuf;

/// Path of the identity registry, from this crate's manifest directory.
fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/dialects.toml")
        .canonicalize()
        .expect("docs/dialects.toml resolves from the crate manifest directory")
}

/// Every `id = "sat:…"` value in `docs/dialects.toml`.
///
/// The `acis:` rows in the same file are the embedded kernel layer, owned by
/// `cadmpeg-asm` and cited here rather than declared, so they are not this
/// enum's business.
fn registry_ids() -> BTreeSet<String> {
    let text = std::fs::read_to_string(registry_path()).expect("read docs/dialects.toml");
    let ids = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("id = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|id| id.starts_with("sat:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no sat rows");
    ids
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    let pinned = SatDialect::ALL
        .iter()
        .map(|dialect| dialect.id().as_str().to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        pinned.len(),
        SatDialect::ALL.len(),
        "two variants pin the same id"
    );
    assert_eq!(
        pinned,
        registry_ids(),
        "docs/dialects.toml and SatDialect disagree; ids are pinned forever, so reconcile the enum"
    );
}

#[test]
fn every_stream_kind_names_its_own_row() {
    let kinds = [
        StreamKind::AsmBinary,
        StreamKind::AcisBinary,
        StreamKind::Text,
        StreamKind::Unknown,
    ];
    let rows = kinds
        .iter()
        .map(|kind| SatDialect::from_stream_kind(*kind))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        rows.len(),
        kinds.len(),
        "two stream kinds share one registry row"
    );
    assert_eq!(
        rows,
        SatDialect::ALL.into_iter().collect::<BTreeSet<_>>(),
        "identity here is the detection discriminant and nothing else"
    );
}

/// A kernel header declaring `save_format_version` and nothing else that
/// classification reads.
fn header(save_format_version: Option<u32>) -> KernelHeader {
    KernelHeader {
        width: 4,
        save_format_version,
        record_count: None,
        entity_count: None,
        flags: None,
        product_family: None,
        product_version: None,
        save_date: None,
        scale: None,
        linear: None,
        angular: None,
    }
}

#[test]
fn only_the_acis_branches_are_banded() {
    // The ASM binary and ASM text paths compare no save format, so every band
    // is admitted on them. Both ACIS branches take the one 217/218 gate.
    for version in [Some(10_000), Some(21_700), Some(21_800), Some(23_200), None] {
        let kernel = header(version);
        // Stated as the raw declared word, not as a second call to the
        // predicate under test.
        let banded = matches!(version, Some(21_700..=21_899));

        assert!(admits_semantic_decode(&StreamEvidence::AsmBinary(Some(
            &kernel
        ))));
        assert!(admits_semantic_decode(&StreamEvidence::Text(Some(
            TextEvidence {
                branch: sat::Dialect::Asm,
                header: &kernel,
            }
        ))));
        assert_eq!(
            admits_semantic_decode(&StreamEvidence::AcisBinary(Some(&kernel))),
            banded,
            "acis binary at {version:?}"
        );
        assert_eq!(
            admits_semantic_decode(&StreamEvidence::Text(Some(TextEvidence {
                branch: sat::Dialect::Acis,
                header: &kernel,
            }))),
            banded,
            "acis text at {version:?}"
        );
    }
}

#[test]
fn a_stream_that_stops_at_its_own_discriminant_is_refused() {
    // Reachable at inspect only: decode returns a malformed error on the same
    // bytes. The row is still named, because the discriminant did match.
    for (evidence, id) in [
        (StreamEvidence::AsmBinary(None), "sat:asm-binary"),
        (StreamEvidence::AcisBinary(None), "sat:acis-binary"),
        (StreamEvidence::Text(None), "sat:text"),
        (StreamEvidence::Unknown, "sat:unknown"),
    ] {
        let matched = classify(&evidence);
        assert_eq!(matched.dialect.as_ref().map(DialectId::as_str), Some(id));
        assert_eq!(matched.admission, Admission::Refused, "{id}");
    }
}

#[test]
fn the_totality_row_never_carries_an_admitted_admission() {
    // `sat:unknown` states that no discriminant matched. Detection reports no
    // confidence for it and both entry points return a malformed error before
    // classification, so the pair (unknown, Admitted) must be unreachable.
    let matched = classify(&StreamEvidence::Unknown);
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("sat:unknown")
    );
    assert_ne!(matched.admission, Admission::Admitted);
    assert!(matched.declared.is_empty());
}

#[test]
fn no_classification_is_admitted_unverified() {
    // The host discriminants are magic bytes, read exactly. Nothing here is
    // parsed with a substituted grammar, so this codec has no unverified state
    // and charges no dialect-unverified loss. A future variant-recovery path
    // must add the loss code in the same change that reaches this state.
    let kernel = header(Some(21_800));
    for evidence in [
        StreamEvidence::AsmBinary(Some(&kernel)),
        StreamEvidence::AsmBinary(None),
        StreamEvidence::AcisBinary(Some(&kernel)),
        StreamEvidence::AcisBinary(None),
        StreamEvidence::Text(Some(TextEvidence {
            branch: sat::Dialect::Asm,
            header: &kernel,
        })),
        StreamEvidence::Text(Some(TextEvidence {
            branch: sat::Dialect::Acis,
            header: &kernel,
        })),
        StreamEvidence::Text(None),
        StreamEvidence::Unknown,
    ] {
        assert!(
            !matches!(
                classify(&evidence).admission,
                Admission::AdmittedUnverified { .. }
            ),
            "sat has no dialect-unverified state"
        );
    }
    assert!(
        !SatLossCode::ALL
            .iter()
            .any(|code| code.code().contains("unverified")),
        "a dialect-unverified loss code appeared without a state that charges it"
    );
}

#[test]
fn the_declared_keys_are_pinned() {
    let kernel = header(Some(21_804));

    let binary = classify(&StreamEvidence::AcisBinary(Some(&kernel))).declared;
    assert_eq!(binary[DECLARED_ENCODING], "binary");
    assert_eq!(binary[DECLARED_SAVE_FORMAT_MAJOR], "218");
    assert_eq!(binary[DECLARED_SAVE_FORMAT_MINOR], "4");
    assert!(!binary.contains_key(DECLARED_TERMINATOR));

    let text = classify(&StreamEvidence::Text(Some(TextEvidence {
        branch: sat::Dialect::Acis,
        header: &kernel,
    })))
    .declared;
    assert_eq!(text[DECLARED_ENCODING], "text");
    assert_eq!(text[DECLARED_TERMINATOR], "End-of-ACIS-data");
    assert_eq!(text[DECLARED_SAVE_FORMAT_MAJOR], "218");
    assert_eq!(text[DECLARED_SAVE_FORMAT_MINOR], "4");

    let asm_text = classify(&StreamEvidence::Text(Some(TextEvidence {
        branch: sat::Dialect::Asm,
        header: &kernel,
    })))
    .declared;
    assert_eq!(asm_text[DECLARED_TERMINATOR], "End-of-ASM-data");

    // An absent save-format word declares no band, which is a different
    // statement from a declaration of zero.
    let silent = classify(&StreamEvidence::AsmBinary(Some(&header(None)))).declared;
    assert!(!silent.contains_key(DECLARED_SAVE_FORMAT_MAJOR));
    assert!(!silent.contains_key(DECLARED_SAVE_FORMAT_MINOR));
}

/// An ACIS binary stream declaring save format `100`, outside the covered band.
fn acis_binary_out_of_band() -> Vec<u8> {
    let mut bytes = b"ACIS BinaryFile".to_vec();
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 28]);
    bytes
}

/// An ACIS-terminated text stream at `save_format_version`.
///
/// The product strings name ASM while the terminator names ACIS, which is the
/// asymmetry the gate must ignore: the branch comes from the terminator line.
fn acis_text(save_format_version: u32) -> Vec<u8> {
    format!(
        "{save_format_version} 0 1 0 \n\
         16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n\
         1 1e-06 1.0e-10 \n\
         body $-1 -1 $-1 $-1 $-1 $-1 #\n\
         End-of-ACIS-data \n"
    )
    .into_bytes()
}

/// One end-to-end case: real bytes, the row they must classify into, and
/// whether semantic decode is admitted.
struct Case {
    label: &'static str,
    bytes: Vec<u8>,
    id: &'static str,
    admitted: bool,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            label: "asm text sphere",
            bytes: text_sphere_stream(1.0),
            id: "sat:text",
            admitted: true,
        },
        Case {
            label: "asm binary sphere",
            bytes: binary_sphere_stream(BinaryFixtureKind::Asm),
            id: "sat:asm-binary",
            admitted: true,
        },
        Case {
            label: "acis binary sphere at 218",
            bytes: binary_sphere_stream(BinaryFixtureKind::Acis),
            id: "sat:acis-binary",
            admitted: true,
        },
        Case {
            label: "acis binary at 100",
            bytes: acis_binary_out_of_band(),
            id: "sat:acis-binary",
            admitted: false,
        },
        Case {
            label: "acis text at 700",
            bytes: acis_text(700),
            id: "sat:text",
            admitted: false,
        },
        Case {
            label: "acis text at 218",
            bytes: acis_text(21_800),
            id: "sat:text",
            admitted: true,
        },
    ]
}

#[test]
fn admission_is_refused_exactly_when_the_save_format_refusal_is_charged() {
    // The biconditional the decode policy needs, and it is structural: the
    // decode paths branch on the admission `classify` produced, so a report
    // charging the refusal without a refused admission cannot be built.
    let refusal = SatLossCode::ContainerAcisSaveFormatUnsupported.kind();
    for case in cases() {
        let result = SatCodec
            .decode(
                &mut Cursor::new(case.bytes.clone()),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", case.label));
        let charged = result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == refusal);
        let matched = &result.report().dialects[0];

        assert_eq!(
            matched.admission == Admission::Refused,
            charged,
            "{}: admission and the save-format refusal must agree",
            case.label
        );
        assert_eq!(
            matched.admission != Admission::Refused,
            case.admitted,
            "{}",
            case.label
        );
    }
}

#[test]
fn decode_reports_exactly_one_primary_layer_match_and_mirrors_it_into_the_source() {
    for case in cases() {
        let result = SatCodec
            .decode(
                &mut Cursor::new(case.bytes.clone()),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", case.label));
        let dialects = &result.report().dialects;

        assert_eq!(dialects.len(), 1, "{}", case.label);
        let matched = &dialects[0];
        assert_eq!(matched.format, FORMAT, "{}", case.label);
        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some(case.id),
            "{}",
            case.label
        );

        // Identity survives refusal: the empty-IR result carries the same row.
        let source = result.ir().source.as_ref().expect("source metadata");
        assert_eq!(source.dialect, matched.dialect, "{}", case.label);
        assert_eq!(source.declared, matched.declared, "{}", case.label);
        assert!(
            !source.declared.is_empty(),
            "{}: every stream declares at least its encoding",
            case.label
        );
    }
}

#[test]
fn inspect_and_decode_agree_on_the_row_and_the_admission() {
    for case in cases() {
        let summary = SatCodec
            .inspect(
                &mut Cursor::new(case.bytes.clone()),
                &InspectOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", case.label));
        let decoded = SatCodec
            .decode(
                &mut Cursor::new(case.bytes.clone()),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", case.label));

        assert_eq!(summary.dialects.len(), 1, "{}", case.label);
        assert_eq!(
            summary.dialects[0],
            decoded.report().dialects[0],
            "{}: inspect and decode read the same evidence",
            case.label
        );
    }
}

#[test]
fn a_stream_matching_no_discriminant_never_reaches_a_report() {
    // `sat:unknown` is unreachable through the normal catalog: detection
    // reports no confidence, and both entry points refuse the bytes outright.
    let bytes = b"not a stream at all".to_vec();
    assert!(SatCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .is_err());
    assert!(SatCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .is_err());
}
