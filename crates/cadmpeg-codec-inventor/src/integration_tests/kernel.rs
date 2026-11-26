// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::InventorLossCode;
use crate::test_support::test_fixtures::{
    empty_asm_history_kernel_stream, primary_envelope_fixture_with_kernel, EnvelopeDeclarations,
};
use crate::InventorCodec;

#[test]
fn empty_solved_partition_retains_the_carrier_and_reports_the_blocking_loss() {
    let kernel = empty_asm_history_kernel_stream();
    let bytes = primary_envelope_fixture_with_kernel(EnvelopeDeclarations::default(), &kernel);
    let (ir, report, fidelity) = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("empty solved partition decodes")
        .into_parts();
    assert!(ir.geometry.bodies.is_empty());
    assert!(ir.geometry.faces.is_empty());
    assert!(ir.model.features.is_empty());
    assert!(!report.geometry_transferred());
    let losses: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code == InventorLossCode::GeometryKernelCarrierNotTransferred.kind())
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].message,
        "the part has no bodies: its active kernel carrier holds no model records"
    );
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Blocking);
    assert!(!fidelity.retained_records().is_empty());
    assert!(fidelity.unrepresented().is_empty());
    let native = ir.native.namespace("inventor").expect("native carrier");
    let carriers = native
        .arena_as::<crate::native::ActiveCarrierRecord>("active_carrier")
        .expect("carrier arena");
    assert!(matches!(
        carriers.as_slice(),
        [crate::native::ActiveCarrierRecord::Selected { .. }]
    ));
    let validation =
        cadmpeg_ir::validate::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses)
            .expect("validate the empty model and its retained source");
    assert!(validation
        .findings
        .iter()
        .all(|finding| { finding.severity < cadmpeg_ir::report::Severity::Error }));
}

/// A pinned decode refuses a part whose kernel carrier it gave up, through
/// `record.unreadable` naming that carrier; a default decode admits the part
/// and reports the codec loss beside it.
#[test]
fn strict_decode_refuses_a_given_up_kernel_carrier() {
    let mut undeclared_width = b"ASM BinaryFile9".to_vec();
    undeclared_width.extend_from_slice(&[0_u8; 16]);
    let mut unsigned = b"NOT A KERNEL STREAM".to_vec();
    unsigned.extend_from_slice(&[0_u8; 16]);
    for (kernel, record) in [
        (undeclared_width, "inventor:kernel:carrier#seg-0"),
        (unsigned, "inventor:rse:record#seg-0"),
    ] {
        let bytes = primary_envelope_fixture_with_kernel(EnvelopeDeclarations::default(), &kernel);
        let decoded = InventorCodec
            .decode(
                &mut std::io::Cursor::new(bytes.clone()),
                &DecodeOptions::default(),
            )
            .expect("a default decode admits the given-up carrier");
        let losses = &decoded.report().losses;
        assert!(
            losses
                .iter()
                .any(|loss| loss.code
                    == InventorLossCode::GeometryKernelCarrierNotTransferred.kind()),
            "{losses:#?}"
        );
        let unreadable = losses
            .iter()
            .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
            .collect::<Vec<_>>();
        assert_eq!(unreadable.len(), 1, "{losses:#?}");
        assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(
            unreadable[0]
                .message
                .split_whitespace()
                .any(|token| token == record),
            "{}",
            unreadable[0].message
        );
        if record.starts_with("inventor:kernel:carrier#") {
            assert!(
                unreadable[0]
                    .message
                    .contains(" stream=\"RSeStorage/Bseg:expanded\" offset="),
                "{}",
                unreadable[0].message
            );
        }

        let mut pinned = DecodeOptions::default();
        pinned.strictness.pinned = true;
        let Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) =
            InventorCodec.decode(&mut std::io::Cursor::new(bytes), &pinned)
        else {
            panic!("a pinned decode refuses the given-up carrier {record}");
        };
        assert_eq!(
            rejection.loss().code,
            cadmpeg_ir::unknown::part::record_unreadable_kind()
        );
        assert!(
            rejection
                .loss()
                .message
                .split_whitespace()
                .any(|token| token == record),
            "{}",
            rejection.loss().message
        );
    }
}
