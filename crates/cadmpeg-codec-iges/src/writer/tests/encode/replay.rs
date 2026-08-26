// SPDX-License-Identifier: Apache-2.0
//! Reporting of a declined verbatim replay.

use super::*;
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
    let source_version = decoded.ir().source.as_ref().unwrap().attributes["iges_version"].clone();
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
        reason.contains(&format!("source is {source_version}")),
        "{reason}"
    );
    assert!(reason.contains("target is 5.2"), "{reason}");
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
