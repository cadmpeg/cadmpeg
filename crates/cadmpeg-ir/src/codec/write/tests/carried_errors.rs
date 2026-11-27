// SPDX-License-Identifier: Apache-2.0
//! A write carries the validation errors of the records it writes.

use super::*;

#[test]
fn carried_errors_exclude_warnings_and_include_unlocated_errors() {
    let ir = CadIr::empty();
    let plan = CoverageEncoder(ArenaCoverage::Declared(uniform_dispositions(
        ArenaDisposition::Dropped,
    )))
    .plan(EncodeInput::new(&ir, None))
    .unwrap();
    let mut report = validation_report(vec![finding(None, "warning", Severity::Warning)]);
    assert!(plan.carried_errors(&ir, &report).is_empty());
    report
        .findings
        .push(finding(None, "error", Severity::Error));
    report.findings.push(finding(
        Some("test:face#missing"),
        "missing",
        Severity::Blocking,
    ));
    assert_eq!(
        plan.carried_errors(&ir, &report),
        vec![&report.findings[1], &report.findings[2]]
    );
}

#[test]
fn carried_errors_follow_model_coverage_and_related_locations() {
    let ir = unit_cube().expect("valid cube");
    let face = ir.geometry.faces[0].identity();
    let edge = ir.geometry.edges[0].identity();
    let report = validation_report(vec![
        finding(Some(face), "invalid face", Severity::Error),
        finding(
            Some(face),
            &format!("invalid reference `{edge}`."),
            Severity::Error,
        ),
    ]);
    let mut report = report;
    report.findings[1].related.push(edge.to_owned());
    for (disposition, carries_face) in [
        (ArenaDisposition::Written, true),
        (ArenaDisposition::Reported, true),
        (ArenaDisposition::Dropped, false),
        (ArenaDisposition::Omitted, false),
        (ArenaDisposition::Ignored, false),
    ] {
        let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
        dispositions.faces = disposition;
        let plan = CoverageEncoder(ArenaCoverage::Declared(dispositions))
            .plan(EncodeInput::new(&ir, None))
            .unwrap();
        let expected = if carries_face {
            vec![&report.findings[0], &report.findings[1]]
        } else {
            vec![&report.findings[1]]
        };
        assert_eq!(plan.carried_errors(&ir, &report), expected);
    }
}

#[test]
fn carried_errors_follow_native_format_and_complete_coverage() {
    let mut ir = CadIr::empty();
    let f3d_id = "f3d:test:asm-history#0";
    let nx_id = "nx:test:record#0";
    let record = |id: &str| {
        vec![crate::NativeRecord::new(
            crate::ids::Identity::new(id).expect("valid native identity"),
            serde_json::Map::default(),
        )
        .expect("valid native record")]
    };
    ir.native
        .namespace_mut(crate::codec_id!("f3d"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("asm_histories"),
            record(f3d_id),
        );
    ir.native
        .namespace_mut(crate::codec_id!("nx"))
        .arenas_mut()
        .insert(cadmpeg_core::nonblank_literal!("records"), record(nx_id));
    ir.finalize();
    let report = validation_report(vec![
        finding(Some(f3d_id), "invalid native record", Severity::Error),
        finding(Some(nx_id), "invalid native record", Severity::Error),
    ]);
    let declared = ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Written));
    let f3d = F3dCoverageEncoder(declared)
        .plan(EncodeInput::new(&ir, None))
        .unwrap();
    assert_eq!(f3d.carried_errors(&ir, &report), vec![&report.findings[0]]);
    let cadir = CoverageEncoder(declared)
        .plan(EncodeInput::new(&ir, None))
        .unwrap();
    let complete = CoverageEncoder(ArenaCoverage::Complete)
        .plan(EncodeInput::new(&ir, None))
        .unwrap();
    for plan in [cadir, complete] {
        assert_eq!(
            plan.carried_errors(&ir, &report),
            report.findings.iter().collect::<Vec<_>>()
        );
    }
}
