// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::report::Severity;

use crate::loss::F3dLossCode;

/// A report carrying the BREP-less geometry losses that `build_container_report`
/// states before the design segment is classified.
fn brep_less_geometry_report() -> cadmpeg_ir::codec::DecodeBody {
    cadmpeg_ir::codec::DecodeBody {
        geometry_transferred: false,
        coverage: cadmpeg_ir::Coverage::default(),
        losses: vec![
            F3dLossCode::GeometryNotTransferred.note("stated before classification"),
            F3dLossCode::TopologyNotTransferred.note("stated before classification"),
            F3dLossCode::MissingGeometryStream.note("stated before classification"),
        ],
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
    }
}

/// A design whose content is sketch curves declares no body, so it has no
/// B-rep to lose: the sketch entities are its complete geometry.
#[test]
fn sketch_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 13, 0);
    assert!(report.geometry_transferred);
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "sketch-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("13 sketch entity(s)")));
}

/// A reference-image timeline object is presentation content. A bodyless
/// document that contains one does not require a BREP geometry carrier.
#[test]
fn presentation_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 1);
    assert!(report.geometry_transferred);
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "presentation-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("1 reference-image timeline object(s)")));
}

/// A declared body whose BREP stream is absent is a real missing carrier. Its
/// sketches do not stand in for the solid the document says it has.
#[test]
fn a_declared_body_without_a_brep_stream_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 1, 13, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}

/// A document with no sketch entities transferred nothing, so nothing settles
/// the loss. An imported drawing whose only entity the importer did not author
/// produces exactly this: a document with no body and no sketch.
#[test]
fn a_document_without_sketch_entities_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}

/// A present BREP stream that produced no geometry is a decode failure, not a
/// sketch-only design, however many sketches the document also carries.
#[test]
fn a_present_brep_stream_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 1, 0, 0, 13, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}

/// A text-encoded B-rep carrier is not sketch-only, regardless of sketch count.
#[test]
fn a_text_brep_carrier_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 2, 0, 13, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}
