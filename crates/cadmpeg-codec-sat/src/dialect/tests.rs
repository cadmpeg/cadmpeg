// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::loss::SatLossCode;
use crate::test_support::{
    acis_text_sphere_stream, binary_sphere_stream, text_sphere_stream, BinaryFixtureKind,
    UNVERIFIED_SAVE_FORMAT,
};
use crate::SatCodec;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    cadmpeg_test_support::assert_registry_closed("sat", &StreamKind::ALL.map(StreamKind::id));
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
    // is admitted on them. Both ACIS branches take the one 217/218 comparison,
    // and outside it they recover rather than refuse.
    for version in [Some(10_000), Some(21_700), Some(21_800), Some(23_200), None] {
        let kernel = header(version);
        // Stated as the raw declared word, not as a second call to the code
        // under test.
        let verified = matches!(version, Some(21_700..=21_899));
        let nearest = if matches!(version, Some(23_200)) {
            "acis:save-format-218"
        } else {
            "acis:save-format-217"
        };

        for asm in [
            StreamEvidence::AsmBinary(Some(&kernel)),
            StreamEvidence::Text(Some(TextEvidence {
                branch: sat::Terminator::Asm,
                header: &kernel,
            })),
        ] {
            assert_eq!(classify(&asm).admission, Admission::Admitted, "{version:?}");
        }

        for acis in [
            StreamEvidence::AcisBinary(Some(&kernel)),
            StreamEvidence::Text(Some(TextEvidence {
                branch: sat::Terminator::Acis,
                header: &kernel,
            })),
        ] {
            let matched = classify(&acis);
            if verified {
                assert_eq!(matched.admission, Admission::Admitted, "{version:?}");
                assert!(dialect_loss(&matched).is_none(), "{version:?}");
            } else {
                assert_eq!(
                    matched.admission,
                    Admission::AdmittedUnverified {
                        nearest: DialectId::pinned(nearest)
                    },
                    "{version:?}"
                );
                let loss = dialect_loss(&matched).expect("the recovery is charged");
                assert_eq!(loss.code, SatLossCode::SourceDialectUnverified.kind());
                assert!(loss.message.contains(nearest), "{}", loss.message);
            }
        }
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
fn the_recovery_loss_is_charged_exactly_on_the_unverified_admission() {
    // The biconditional §7 requires: `AdmittedUnverified` and the
    // `source.dialect-unverified` charge are the same fact, read from one
    // place. `Refused` here is structural — the discriminant matched and the
    // stream did not frame — and carries no recovery mark.
    let verified = header(Some(21_800));
    let unverified = header(Some(UNVERIFIED_SAVE_FORMAT));
    for evidence in [
        StreamEvidence::AsmBinary(Some(&verified)),
        StreamEvidence::AsmBinary(Some(&unverified)),
        StreamEvidence::AsmBinary(None),
        StreamEvidence::AcisBinary(Some(&verified)),
        StreamEvidence::AcisBinary(Some(&unverified)),
        StreamEvidence::AcisBinary(None),
        StreamEvidence::Text(Some(TextEvidence {
            branch: sat::Terminator::Asm,
            header: &unverified,
        })),
        StreamEvidence::Text(Some(TextEvidence {
            branch: sat::Terminator::Acis,
            header: &unverified,
        })),
        StreamEvidence::Text(None),
        StreamEvidence::Unknown,
    ] {
        let matched = classify(&evidence);
        assert_eq!(
            matches!(matched.admission, Admission::AdmittedUnverified { .. }),
            dialect_loss(&matched).is_some(),
            "{:?}",
            matched.admission
        );
    }
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
        branch: sat::Terminator::Acis,
        header: &kernel,
    })))
    .declared;
    assert_eq!(text[DECLARED_ENCODING], "text");
    assert_eq!(text[DECLARED_TERMINATOR], "End-of-ACIS-data");
    assert_eq!(text[DECLARED_SAVE_FORMAT_MAJOR], "218");
    assert_eq!(text[DECLARED_SAVE_FORMAT_MINOR], "4");

    let asm_text = classify(&StreamEvidence::Text(Some(TextEvidence {
        branch: sat::Terminator::Asm,
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
    kernel_id: &'static str,
    admission: Admission,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            label: "asm text sphere",
            bytes: text_sphere_stream(1.0),
            id: "sat:text",
            kernel_id: "acis:text-asm",
            admission: Admission::Admitted,
        },
        Case {
            label: "asm binary sphere",
            bytes: binary_sphere_stream(BinaryFixtureKind::Asm),
            id: "sat:asm-binary",
            kernel_id: "acis:asm-binaryfile-8",
            admission: Admission::Admitted,
        },
        Case {
            label: "acis binary sphere at 218",
            bytes: binary_sphere_stream(BinaryFixtureKind::Acis),
            id: "sat:acis-binary",
            kernel_id: "acis:save-format-218",
            admission: Admission::Admitted,
        },
        Case {
            label: "acis text at 700",
            bytes: acis_text(700),
            id: "sat:text",
            kernel_id: "acis:text-acis",
            admission: Admission::AdmittedUnverified {
                nearest: DialectId::pinned("acis:save-format-217"),
            },
        },
        Case {
            label: "acis text sphere outside the verified band",
            bytes: acis_text_sphere_stream(UNVERIFIED_SAVE_FORMAT),
            id: "sat:text",
            kernel_id: "acis:text-acis",
            admission: Admission::AdmittedUnverified {
                nearest: DialectId::pinned("acis:save-format-218"),
            },
        },
        Case {
            label: "acis binary sphere outside the verified band",
            bytes: binary_sphere_stream(BinaryFixtureKind::AcisUnverifiedBand),
            id: "sat:acis-binary",
            kernel_id: "acis:save-format-binary-other",
            admission: Admission::AdmittedUnverified {
                nearest: DialectId::pinned("acis:save-format-218"),
            },
        },
        Case {
            label: "acis text at 218",
            bytes: acis_text(21_800),
            id: "sat:text",
            kernel_id: "acis:text-acis",
            admission: Admission::Admitted,
        },
    ]
}

#[test]
fn decode_admission_matches_the_stream_and_carries_the_recovery_mark() {
    // End to end on real bytes: the admission the decode reports, and the
    // recovery loss charged exactly with it.
    let recovery = SatLossCode::SourceDialectUnverified.kind();
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
            .any(|loss| loss.code == recovery);
        let matched = result
            .report()
            .dialects
            .as_ref()
            .expect("SAT reports dialect layers")
            .primary();

        assert_eq!(matched.admission, case.admission, "{}", case.label);
        assert_eq!(
            matches!(matched.admission, Admission::AdmittedUnverified { .. }),
            charged,
            "{}: admission and the recovery mark must agree",
            case.label
        );
    }
}

#[test]
fn an_unverified_band_recovers_the_same_solid_as_the_verified_one() {
    // The recovery is real, not a relabelled refusal: the same records under a
    // band no row verifies decode to the same solid, in both encodings.
    for (label, bytes) in [
        ("text", acis_text_sphere_stream(UNVERIFIED_SAVE_FORMAT)),
        (
            "binary",
            binary_sphere_stream(BinaryFixtureKind::AcisUnverifiedBand),
        ),
    ] {
        let result = SatCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert!(result.report().geometry_transferred, "{label}");
        assert_eq!(result.ir().model.bodies.len(), 1, "{label}");
        assert_eq!(result.ir().model.faces.len(), 1, "{label}");
        assert_eq!(result.ir().model.surfaces.len(), 1, "{label}");
        assert_eq!(result.report().coverage["unknown_records"], 0, "{label}");
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
        let dialects = result
            .report()
            .dialects
            .as_ref()
            .expect("SAT reports dialect layers");

        assert_eq!(dialects.iter().count(), 2, "{}", case.label);
        let matched = dialects.primary();
        assert_eq!(matched.format, FORMAT, "{}", case.label);
        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some(case.id),
            "{}",
            case.label
        );
        let kernel = dialects.iter().nth(1).expect("SAT reports its ACIS layer");
        assert_eq!(kernel.format, "acis", "{}", case.label);
        assert_eq!(
            kernel.dialect.as_ref().map(DialectId::as_str),
            Some(case.kernel_id),
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

        assert_eq!(
            summary
                .dialects
                .as_ref()
                .expect("SAT inspection reports dialect layers")
                .iter()
                .count(),
            2,
            "{}",
            case.label
        );
        assert_eq!(
            summary.dialects,
            decoded.report().dialects,
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
