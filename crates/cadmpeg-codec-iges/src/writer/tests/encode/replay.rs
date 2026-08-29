// SPDX-License-Identifier: Apache-2.0
//! Reporting of a declined verbatim replay.

use super::*;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_ir::codec::TargetRequest;
use cadmpeg_ir::report::FidelityResolution;

/// Reads the degradation reason from `plan`, or panics with the resolution.
fn degraded_reason(plan: &cadmpeg_ir::codec::ExportPlan<'_>, context: &str) -> String {
    match plan.fidelity_resolution() {
        FidelityResolution::Degraded { reason } => reason.clone(),
        other => panic!("{context}: {other:?}"),
    }
}

#[test]
fn encode_reports_a_version_mismatch_as_dialect_displacement() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(point_file()), &DecodeOptions::default())
        .unwrap();
    let source_dialect = decoded
        .ir()
        .source
        .as_ref()
        .unwrap()
        .dialect
        .clone()
        .unwrap();
    let plan = IgesEncoder
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Explicit(IgesVersion::V5_2.target()),
        )
        .unwrap();

    assert_eq!(plan.write_path(), WritePath::Synthesized);
    assert_eq!(plan.fidelity_resolution(), &FidelityResolution::NotConsumed);
    let displacement = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::SourceDialectDisplaced.kind())
        .expect("version displacement is charged");
    assert!(displacement
        .message
        .contains(source_dialect.dialect().as_str()));
    assert!(displacement.message.contains("iges:5.2-fixed-ascii"));
}

#[test]
fn encode_declines_replay_when_the_source_records_no_dialect() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(point_file()), &DecodeOptions::default())
        .unwrap();
    let mut unclassified = decoded.ir().clone();
    unclassified.source.as_mut().unwrap().dialect = None;
    let plan = IgesEncoder
        .plan(
            EncodeInput::new(&unclassified, Some(decoded.source_fidelity())),
            TargetRequest::Explicit(IgesVersion::V5_3.target()),
        )
        .unwrap();

    assert_eq!(plan.write_path(), WritePath::Synthesized);
    let reason = degraded_reason(&plan, "an unclassified source must degrade");
    assert!(reason.contains("records no dialect"), "{reason}");
    assert!(
        reason.contains("target is iges:5.3-fixed-ascii"),
        "{reason}"
    );
}

#[test]
fn a_replayed_export_states_the_preserved_dialect_as_its_target() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(point_file()), &DecodeOptions::default())
        .unwrap();
    let plan = IgesEncoder
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Explicit(IgesVersion::V5_3.target()),
        )
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::VerbatimReplay);

    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert_eq!(
        report.target(),
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect
            .as_ref()
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
    );
}

#[test]
fn a_synthesized_export_states_the_target_it_wrote() {
    for (version, id) in [
        (IgesVersion::V4_0, "iges:4.0-fixed-ascii"),
        (IgesVersion::V5_0, "iges:5.0-fixed-ascii"),
        (IgesVersion::V5_1, "iges:5.1-fixed-ascii"),
        (IgesVersion::V5_2, "iges:5.2-fixed-ascii"),
        (IgesVersion::V5_3, "iges:5.3-fixed-ascii"),
    ] {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(Point {
            id: PointId(format!("point#{id}")),
            source_object: None,
            position: Point3::new(1.0, 2.0, 3.0),
        });
        let plan = IgesEncoder
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.target()),
            )
            .unwrap();
        let mut written = Vec::new();
        let report = plan.write_to(&mut written).unwrap();
        assert_eq!(report.target().map(DialectId::as_str), Some(id), "{id}");
    }
}

#[test]
fn encode_reports_a_digest_mismatch_as_degraded_fidelity() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(point_file()), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    edited.model.points.push(Point {
        id: PointId("point#edited".into()),
        source_object: None,
        position: Point3::new(7.0, 8.0, 9.0),
    });
    let plan = IgesEncoder
        .plan(
            EncodeInput::new(&edited, Some(decoded.source_fidelity())),
            TargetRequest::Explicit(IgesVersion::V5_3.target()),
        )
        .unwrap();

    assert_eq!(plan.write_path(), WritePath::Synthesized);
    let reason = degraded_reason(&plan, "an edited model must degrade");
    assert!(reason.contains("digest"), "{reason}");
}
