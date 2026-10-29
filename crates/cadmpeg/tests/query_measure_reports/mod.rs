// SPDX-License-Identifier: Apache-2.0
//! `query` views of measure reports.

use super::*;

/// The unit cube with one face flipped: its sense disagrees with its loop
/// winding, so the volume and centroid are unavailable with
/// `volume.orientation-conflict` (category `input`).
#[test]
fn summary_names_a_measure_report_and_counts_its_rows_and_unavailable_values() {
    let dir = tempdir().unwrap();
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.faces[0].sense = match ir.model.faces[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let model = dir.path().join("cube.cadir.json");
    fs::write(&model, ir.to_canonical_json().unwrap()).unwrap();
    let report = dir.path().join("cube.measure.json");
    cadmpeg()
        .args(["measure", model.to_str().unwrap(), "-o"])
        .arg(&report)
        .assert()
        .success();
    let measured: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    let unavailable = measured["measure"]["document"]["unavailable"]
        .as_object()
        .unwrap();
    let per_category: Vec<(String, u64)> = unavailable
        .iter()
        .filter(|(category, _)| category.as_str() != "undefined")
        .map(|(category, codes)| {
            let count = codes
                .as_object()
                .unwrap()
                .values()
                .map(|count| count.as_u64().unwrap())
                .sum();
            (category.clone(), count)
        })
        .collect();
    assert!(
        unavailable["input"]["volume.orientation-conflict"]
            .as_u64()
            .unwrap()
            >= 2
    );

    let output = cadmpeg()
        .args(["query", "summary", "--json"])
        .arg(&report)
        .output()
        .unwrap();
    assert!(output.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let payload = &summary["payload"];
    assert_eq!(payload["kind"], "command report");
    assert_eq!(payload["command"], "measure");
    for (level, rows) in [
        ("bodies", "1"),
        ("faces", "0"),
        ("edges", "0"),
        ("surfaces", "0"),
    ] {
        assert_eq!(payload[format!("measure_{level}")], rows, "{level}");
    }
    let total: u64 = per_category.iter().map(|(_, count)| count).sum();
    assert_eq!(payload["measure_unavailable"], total.to_string());
    for (category, count) in &per_category {
        assert_eq!(
            payload[format!("measure_unavailable.{category}")],
            count.to_string()
        );
    }
    assert_eq!(payload["measure_work_exhausted"], "false");
    assert!(payload.get("measure_unavailable.undefined").is_none());

    // The unflipped cube is measured: its three equal principal moments
    // leave three undetermined axes, which count as undefined only.
    let cube = dir.path().join("cube_ok.cadir.json");
    let measured_cube = dir.path().join("cube_ok.measure.json");
    let ir = unit_cube().expect("unit cube fixture is admitted");
    fs::write(&cube, ir.to_canonical_json().unwrap()).unwrap();
    cadmpeg()
        .args(["measure", cube.to_str().unwrap(), "-o"])
        .arg(&measured_cube)
        .assert()
        .success();
    cadmpeg()
        .args(["query", "summary"])
        .arg(&measured_cube)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("measure_unavailable\t0\n")
                .and(predicate::str::contains("measure_undefined\t3\n")),
        );

    cadmpeg()
        .args(["query", "findings"])
        .arg(&report)
        .assert()
        .success()
        .stdout("severity\tcheck\tentity\tmessage\n")
        .stderr(predicate::str::contains("this command did not check"));
    cadmpeg()
        .args(["query", "losses"])
        .arg(&report)
        .assert()
        .success()
        .stdout("severity\tcode\tmessage\n")
        .stderr(predicate::str::contains(
            "measure report has no decode stage",
        ));
}

/// A measure report that leaves out its document's `work` reads it as
/// absent; one that states `work: null` is refused with the key named.
#[test]
fn a_measure_report_states_its_work_or_leaves_it_out() {
    let dir = tempdir().unwrap();
    let report = |work: &str| {
        format!(
            r#"{{"ir_version": "10", "command": "measure", "status": "ok", "refusal": null,
               "decode_report": null, "measure": {{"bodies": [{{}}], "faces": [],
               "edges": [], "surfaces": [], "document": {{"unavailable": {{}}{work}}}}}}}"#
        )
    };
    let left_out = write(dir.path(), "left_out.json", &report(""));
    cadmpeg()
        .args(["query", "summary"])
        .arg(&left_out)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("measure_bodies\t1")
                .and(predicate::str::contains("measure_unavailable\t0"))
                .and(predicate::str::contains("measure_work").not()),
        );
    let null = write(dir.path(), "null.json", &report(r#", "work": null"#));
    cadmpeg()
        .args(["query", "summary"])
        .arg(&null)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "work: this key states a value or is left out",
        ));
}
