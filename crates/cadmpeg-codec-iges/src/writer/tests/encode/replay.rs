// SPDX-License-Identifier: Apache-2.0
//! Reporting of a declined verbatim replay.

use super::*;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_ir::report::FidelityResolution;

/// Reads the degradation reason from `plan`, or panics with the resolution.
fn degraded_reason(plan: &cadmpeg_ir::codec::ExportPlan<'_>, context: &str) -> String {
    match plan.fidelity_resolution() {
        FidelityResolution::Degraded { reason } => reason.clone(),
        other => panic!("{context}: {other:?}"),
    }
}

#[test]
fn encode_reports_a_version_mismatch_as_degraded_fidelity() {
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
    let plan = IgesEncoder::new(IgesWriteOptions {
        version: IgesVersion::V5_2,
    })
    .plan(EncodeInput {
        ir: decoded.ir(),
        fidelity: Some(decoded.source_fidelity()),
    })
    .unwrap();

    assert_eq!(plan.write_path(), WritePath::Synthesized);
    let reason = degraded_reason(&plan, "a version mismatch must degrade");
    assert!(
        reason.contains(&format!("source is {source_dialect}")),
        "{reason}"
    );
    assert!(
        reason.contains("target is iges:5.2-fixed-ascii"),
        "{reason}"
    );
}

#[test]
fn encode_declines_replay_when_the_source_records_no_dialect() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(point_file()), &DecodeOptions::default())
        .unwrap();
    let mut unclassified = decoded.ir().clone();
    unclassified.source.as_mut().unwrap().dialect = None;
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: &unclassified,
            fidelity: Some(decoded.source_fidelity()),
        })
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
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: Some(decoded.source_fidelity()),
        })
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::VerbatimReplay);

    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert_eq!(report.target, decoded.ir().source.as_ref().unwrap().dialect);
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
        let plan = IgesEncoder::new(IgesWriteOptions { version })
            .plan(EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .unwrap();
        let mut written = Vec::new();
        let report = plan.write_to(&mut written).unwrap();
        assert_eq!(
            report.target.as_ref().map(DialectId::as_str),
            Some(id),
            "{id}"
        );
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
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: &edited,
            fidelity: Some(decoded.source_fidelity()),
        })
        .unwrap();

    assert_eq!(plan.write_path(), WritePath::Synthesized);
    let reason = degraded_reason(&plan, "an edited model must degrade");
    assert!(reason.contains("digest"), "{reason}");
}
