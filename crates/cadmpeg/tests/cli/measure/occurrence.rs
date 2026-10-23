// SPDX-License-Identifier: Apache-2.0
//! `measure --occurrence`: rows in the instance space of an occurrence of
//! `measure_assembly.p21` (fixture README).
//!
//! The pin body is the cylinder r = 10 about +z, z ∈ [0, 30], with centroid
//! (0, 0, 15) and box (−10, −10, 0)..(10, 10, 30). PIN's shape places it at
//! (0, 0, 5): centroid (0, 0, 20), box (−10, −10, 5)..(10, 10, 35). Each
//! occurrence applies inside its parent, and the body's own placement first:
//!
//! - `#140` = T(100, 0, 0): centroid (100, 0, 20), box
//!   (90, −10, 5)..(110, 10, 35), axis +z through (100, 0, 5).
//! - `#160` inside `#150`: T(0, 0, 100) ∘ (T(0, 50, 0) ∘ R) with
//!   R(x, y, z) = (z, y, −x). The centroid (0, 0, 20) turns to (20, 0, 0)
//!   and moves to (20, 50, 100); the box x, y ∈ [−10, 10], z ∈ [5, 35]
//!   turns to x ∈ [5, 35], y ∈ [−10, 10], z ∈ [−10, 10] and moves to
//!   (5, 40, 90)..(35, 60, 110); the axis turns to +x through
//!   (5, 50, 100).
//! - `#150` (SUB) lists no body; its subtree places the pin through `#160`
//!   by the same composition, so its rows are those of `#160`.
//! - The root occurrence of ROOT reaches the pin twice, through `#140` and
//!   through `#160`.

use std::fs;
use std::path::{Path, PathBuf};

use predicates::prelude::{predicate, PredicateBooleanExt};
use serde_json::Value;
use tempfile::tempdir;

use super::{assert_box, assert_measured, assert_point, cadmpeg, measure_json, only_body};

fn assembly() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/measure/measure_assembly.p21")
}

fn occurrence_report(path: &Path, occurrence: &str, extra: &[&str]) -> Value {
    let mut args = vec![path.to_str().unwrap(), "--occurrence", occurrence];
    args.extend_from_slice(extra);
    measure_json(&args)
}

#[test]
fn bodies_are_placed_by_their_own_transform_then_the_occurrence_chain() {
    let pi = std::f64::consts::PI;
    let path = assembly();
    let model = measure_json(&[path.to_str().unwrap()]);
    let body = only_body(&model);
    assert_eq!(body["placement"], serde_json::json!({"space": "model"}));
    assert_point(&body["centroid"], [0.0, 0.0, 20.0], 30.0);
    assert!(model["measure"].get("occurrence").is_none());

    for (occurrence, chain, centroid, low, high) in [
        (
            "step:product:occurrence#140",
            &[
                "step:product:occurrence#definition-105",
                "step:product:occurrence#140",
            ][..],
            [100.0, 0.0, 20.0],
            [90.0, -10.0, 5.0],
            [110.0, 10.0, 35.0],
        ),
        (
            "step:product:occurrence#160",
            &[
                "step:product:occurrence#definition-105",
                "step:product:occurrence#150",
                "step:product:occurrence#160",
            ][..],
            [20.0, 50.0, 100.0],
            [5.0, 40.0, 90.0],
            [35.0, 60.0, 110.0],
        ),
    ] {
        let suffix = occurrence.trim_start_matches("step:product:");
        let report = occurrence_report(&path, suffix, &[]);
        let body = only_body(&report);
        assert_eq!(
            body["placement"],
            serde_json::json!({"space": "occurrence", "occurrence": occurrence})
        );
        assert_measured(&body["volume"], 3000.0 * pi, 1.0);
        assert_point(&body["centroid"], centroid, 110.0);
        assert_box(&body["bbox"], low, high, 110.0);
        let block = &report["measure"]["occurrence"];
        assert_eq!(block["occurrence"], occurrence);
        assert_eq!(block["prototype"], "step:product:product#111");
        assert_eq!(block["chain"], serde_json::json!(chain));
        assert_eq!(block["bodies"], serde_json::json!(["step:data:body#71"]));
        assert!(block.get("unavailable").is_none());
    }

    for (occurrence, point, direction) in [
        ("occurrence#140", [100.0, 0.0, 5.0], [0.0, 0.0, 1.0]),
        ("occurrence#160", [5.0, 50.0, 100.0], [1.0, 0.0, 0.0]),
    ] {
        let report = occurrence_report(&path, occurrence, &["--faces", "--kind", "cylinder"]);
        let faces = report["measure"]["faces"].as_array().unwrap();
        let [face] = faces.as_slice() else {
            panic!("one cylinder face");
        };
        assert_eq!(face["placement"]["space"], "occurrence");
        let parameters = &face["carrier"]["parameters"];
        assert_measured(&parameters["radius"], 10.0, 10.0);
        assert_point(&parameters["axis_direction"], direction, 1.0);
        let axis_point = &parameters["axis_point"]["value"];
        let offset = [
            axis_point["x"].as_f64().unwrap() - point[0],
            axis_point["y"].as_f64().unwrap() - point[1],
            axis_point["z"].as_f64().unwrap() - point[2],
        ];
        let along = offset[0] * direction[0] + offset[1] * direction[1] + offset[2] * direction[2];
        let across: f64 = offset
            .iter()
            .zip(direction)
            .map(|(component, axis)| (component - along * axis).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(across <= super::TEST_ABSOLUTE_FLOOR * 100.0, "{axis_point}");
    }
}

/// The pin through `#160`: area `2πrh + 2πr² = 800π`, centroid
/// (20, 50, 100).
#[test]
fn a_subassembly_occurrence_measures_the_bodies_of_its_subtree() {
    let pi = std::f64::consts::PI;
    let report = occurrence_report(&assembly(), "occurrence#150", &[]);
    let body = only_body(&report);
    assert_eq!(
        body["placement"],
        serde_json::json!({"space": "occurrence", "occurrence": "step:product:occurrence#150"})
    );
    assert_measured(&body["area"], 800.0 * pi, 1000.0);
    assert_point(&body["centroid"], [20.0, 50.0, 100.0], 110.0);
    assert_box(&body["bbox"], [5.0, 40.0, 90.0], [35.0, 60.0, 110.0], 110.0);
    assert_measured(&report["measure"]["document"]["area"], 800.0 * pi, 1000.0);
    let block = &report["measure"]["occurrence"];
    assert_eq!(block["prototype"], "step:product:product#107");
    assert_eq!(block["bodies"], serde_json::json!(["step:data:body#71"]));
    assert!(block.get("unavailable").is_none());
    let descendants = block["descendants"].as_array().unwrap();
    let [descendant] = descendants.as_slice() else {
        panic!("one descendant");
    };
    assert_eq!(descendant["occurrence"], "step:product:occurrence#160");
    assert_eq!(descendant["parent"], "step:product:occurrence#150");
    assert_eq!(
        descendant["bodies"],
        serde_json::json!(["step:data:body#71"])
    );

    let root = occurrence_report(&assembly(), "occurrence#definition-105", &[]);
    assert!(root["measure"]["bodies"].as_array().unwrap().is_empty());
    let unavailable = &root["measure"]["occurrence"]["unavailable"];
    assert_eq!(unavailable["reason"], "placement.occurrence-body-repeated");
    assert_eq!(unavailable["entity"], "step:data:body#71");
    assert_eq!(
        root["measure"]["document"]["area"]["reason"],
        "placement.occurrence-body-repeated"
    );
}

/// The assembly as CADIR with `edit` applied to its occurrences.
fn edited(dir: &Path, edit: impl Fn(&mut Value)) -> PathBuf {
    let cadir = dir.join("assembly.cadir.json");
    cadmpeg()
        .arg("dump")
        .arg(assembly())
        .arg("-o")
        .arg(&cadir)
        .arg("--force")
        .assert()
        .success();
    let mut document: Value = serde_json::from_slice(&fs::read(&cadir).unwrap()).unwrap();
    edit(&mut document);
    let out = dir.join("edited.cadir.json");
    fs::write(&out, serde_json::to_vec(&document).unwrap()).unwrap();
    out
}

fn occurrence_mut<'a>(document: &'a mut Value, id: &str) -> &'a mut Value {
    document["model"]["occurrences"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|occurrence| occurrence["id"] == id)
        .unwrap()
}

#[test]
fn a_scaled_chain_or_an_external_prototype_gives_no_rows() {
    let dir = tempdir().unwrap();
    let scaled = edited(dir.path(), |document| {
        occurrence_mut(document, "step:product:occurrence#150")["scale"] =
            serde_json::json!([2.0, 1.0, 1.0]);
    });
    let report = occurrence_report(&scaled, "occurrence#160", &[]);
    assert!(report["measure"]["bodies"].as_array().unwrap().is_empty());
    let unavailable = &report["measure"]["occurrence"]["unavailable"];
    assert_eq!(unavailable["reason"], "placement.occurrence-scale");
    assert_eq!(unavailable["category"], "placement");
    assert_eq!(unavailable["entity"], "step:product:occurrence#150");
    assert!(report["measure"]["occurrence"].get("transform").is_none());
    let document = &report["measure"]["document"];
    for quantity in ["area", "bbox", "closed_volume"] {
        assert_eq!(document[quantity]["status"], "unavailable", "{quantity}");
        assert_eq!(
            document[quantity]["reason"], "placement.occurrence-scale",
            "{quantity}"
        );
    }
    assert_eq!(
        document["unavailable"]["placement"]["placement.occurrence-scale"],
        1
    );
    let saved = dir.path().join("scaled.measure.json");
    cadmpeg()
        .arg("measure")
        .arg(&scaled)
        .args(["--occurrence", "occurrence#160", "-o"])
        .arg(&saved)
        .assert()
        .success();
    cadmpeg()
        .args(["query", "summary"])
        .arg(&saved)
        .assert()
        .success()
        .stdout(predicate::str::contains("measure_unavailable\t1\n").and(
            predicate::str::contains("measure_unavailable.placement\t1\n"),
        ));

    let external = edited(dir.path(), |document| {
        occurrence_mut(document, "step:product:occurrence#140")["prototype"] = serde_json::json!({
            "scope": "external",
            "document": {"resolution": "path", "path": "pin.step"}
        });
    });
    let report = occurrence_report(&external, "occurrence#140", &[]);
    assert!(report["measure"]["bodies"].as_array().unwrap().is_empty());
    assert_eq!(
        report["measure"]["occurrence"]["unavailable"]["reason"],
        "placement.prototype-unresolved"
    );
}

#[test]
fn occurrence_selectors_resolve_and_scope_the_rows() {
    let path = assembly();
    cadmpeg()
        .arg("measure")
        .arg(&path)
        .args(["--occurrence", "occurrence#999"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("model.occurrences"));
    let dir = tempdir().unwrap();
    let detached = edited(dir.path(), |document| {
        occurrence_mut(document, "step:product:occurrence#160")["prototype"] =
            serde_json::json!({"scope": "unresolved"});
    });
    cadmpeg()
        .arg("measure")
        .arg(&detached)
        .args(["--occurrence", "occurrence#150", "--body", "body#71"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "is not in the prototype of occurrence step:product:occurrence#150",
        ));
    let report = occurrence_report(&detached, "occurrence#150", &[]);
    let unavailable = &report["measure"]["occurrence"]["unavailable"];
    assert_eq!(unavailable["reason"], "placement.prototype-unresolved");
    assert_eq!(unavailable["entity"], "step:product:occurrence#160");
}
