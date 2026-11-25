// SPDX-License-Identifier: Apache-2.0
//! Assembly identity keys, repeated records, mate scope and malformed frames.
use crate::test_support::{assembly::*, read_bytes, Report};
use serde_json::{json, Value};
use std::path::Path;

fn decode(bytes: &[u8]) -> (crate::reader::Model, Report) {
    let mut report = Report::default();
    let model = read_bytes(Path::new("synthetic.onshape-bundle"), bytes, &mut report).unwrap();
    let validation = cadmpeg_ir::validate::validate_neutral(&model.ir, Vec::new()).unwrap();
    assert!(
        !validation
            .findings
            .iter()
            .any(|finding| finding.severity == cadmpeg_ir::report::Severity::Error),
        "{:?}",
        validation.findings
    );
    (model, report)
}
fn read(definition: Value) -> (crate::reader::Model, Report) {
    decode(&bundle(definition))
}
fn count(report: &Report, code: &str, detail: &str) -> usize {
    report
        .losses()
        .iter()
        .filter(|loss| loss.code.as_deref() == Some(code) && loss.detail.contains(detail))
        .count()
}
fn parts(model: &crate::reader::Model) -> usize {
    model
        .ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| &occurrence.prototype)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

#[test]
fn default_configuration_and_pinned_microversion_share_one_product() {
    for (field, value) in [
        ("configuration", None),
        ("configuration", Some("")),
        ("documentMicroversion", Some("microversion")),
    ] {
        let mut definition = definition();
        let instance = definition["rootAssembly"]["instances"][1]
            .as_object_mut()
            .unwrap();
        match value {
            Some(value) => instance.insert(field.into(), json!(value)),
            None => instance.remove(field),
        };
        let (model, _) = read(definition);
        assert_eq!(parts(&model), 1, "{field} {value:?}");
    }
    let mut definition = definition();
    definition["rootAssembly"]["instances"][0]["documentMicroversion"] = json!("microversion");
    let (model, _) = read(definition);
    assert_eq!(parts(&model), 1);
}
#[test]
fn assembly_instance_without_microversion_matches_a_stated_definition() {
    let mut definition = nested();
    definition["subAssemblies"][0]["documentMicroversion"] = json!("microversion");
    let (model, report) = read(definition);
    assert_eq!(
        count(&report, "onshape/assembly.reference-unresolved", ""),
        0
    );
    assert_eq!(model.ir.model.occurrences.len(), 3);
}
#[test]
fn assembly_decode_keeps_the_nested_source_layers() {
    let (model, _) = read(definition());
    assert!(model
        .ir
        .source
        .as_ref()
        .and_then(|source| source.dialects())
        .is_some_and(|layers| layers.iter().any(|m| m.format() == "step")));
}
#[test]
fn repeated_and_conflicting_element_ids_decode_once_with_a_loss() {
    for element in [
        json!({"id":"studio","type":"PARTSTUDIO"}),
        json!({"id":"studio","type":"ASSEMBLY"}),
        json!({"id":"assembly","type":"PARTSTUDIO"}),
    ] {
        let bytes = edited(definition(), |members| {
            members["manifest.json"]["elements"]
                .as_array_mut()
                .unwrap()
                .push(element.clone());
        });
        let (model, report) = decode(&bytes);
        assert_eq!(model.ir.geometry.bodies.len(), 1, "{element}");
        assert_eq!(model.ir.model.occurrences.len(), 2, "{element}");
        assert_eq!(
            count(
                &report,
                "onshape/assembly.record-unsupported",
                "element id repeats"
            ),
            1,
            "{element}"
        );
    }
}
#[test]
fn repeated_definition_keys_and_missing_instances_charge_losses() {
    let mut definition = nested();
    let sub = definition["subAssemblies"][0].clone();
    definition["subAssemblies"]
        .as_array_mut()
        .unwrap()
        .push(sub);
    let part = definition["parts"][0].clone();
    definition["parts"].as_array_mut().unwrap().push(part);
    let (_, report) = read(definition);
    let code = "onshape/assembly.record-unsupported";
    assert_eq!(count(&report, code, "subassembly definition repeats"), 1);
    assert_eq!(count(&report, code, "part identity repeats"), 1);
    let mut definition = nested();
    definition["subAssemblies"][0]
        .as_object_mut()
        .unwrap()
        .remove("instances");
    let (_, report) = read(definition);
    assert_eq!(count(&report, code, "no instances field"), 1);
}
#[test]
fn an_off_switch_governs_only_the_limit_it_names() {
    for (parameters, expected, absent) in [
        (
            json!([{"parameterId":"minimumLimitEnabled","value":false},{"parameterId":"maximumLimit","value":1.0}]),
            Some("maximumLimit"),
            "minimumLimit",
        ),
        (
            json!([{"parameterId":"limitsEnabled","value":false},{"parameterId":"maximumLimit","value":1.0},{"parameterId":"offsetEnabled","value":true}]),
            Some("offsetEnabled"),
            "maximumLimit",
        ),
        (
            json!([{"parameterId":"offsetEnabled","value":false},{"parameterId":"limitMaximum","value":1.0}]),
            Some("limitMaximum"),
            "offsetEnabled",
        ),
    ] {
        let mut definition = definition();
        definition["rootAssembly"]["features"] = json!([mate("nativeMate", "REVOLUTE")]);
        let bytes = edited(definition, |members| {
            members["assemblies/assembly/features.json"]["features"] =
                json!([{"featureId":"nativeMate","parameters":parameters}]);
        });
        let (_, report) = decode(&bytes);
        let code = "onshape/assembly.mate-limits-unresolved";
        assert_eq!(count(&report, code, expected.unwrap()), 1, "{parameters}");
        assert_eq!(count(&report, code, absent), 0, "{parameters}");
    }
}
#[test]
fn frame_assumption_is_charged_below_a_direct_child_only() {
    let code = "onshape/assembly.mate-frame-assumed";
    let (_, report) = read(nested());
    assert_eq!(count(&report, code, ""), 0);
    let mut definition = nested();
    let mut deep = mate("deepMate", "FASTENED");
    for entity in deep["featureData"]["matedEntities"].as_array_mut().unwrap() {
        entity["matedOccurrence"] = json!(["sub", "first"]);
    }
    definition["rootAssembly"]["features"] = json!([deep]);
    let (_, report) = read(definition);
    assert_eq!(count(&report, code, ""), 1);
}
#[test]
fn transform_bottom_row_must_be_the_affine_row() {
    for row in [[0., 0., 0., 2.], [0., 0., 1., 1.], [1., 0., 0., 1.]] {
        let mut definition = definition();
        let mut transform = placement(0.03, 0.04);
        for (slot, value) in row.iter().enumerate() {
            transform[12 + slot] = json!(value);
        }
        definition["rootAssembly"]["occurrences"][1]["transform"] = transform;
        let (model, report) = read(definition);
        let second = model
            .ir
            .model
            .occurrences
            .iter()
            .find(|o| o.name.as_deref() == Some("second"))
            .unwrap();
        assert!(second.transform.is_none(), "{row:?}");
        assert_eq!(
            count(
                &report,
                "onshape/assembly.record-unsupported",
                "transform bottom row is invalid"
            ),
            1,
            "{row:?}"
        );
    }
}
#[test]
fn mated_frames_must_be_proper_rigid() {
    for (axis, value) in [
        ("xAxis", json!([2., 0., 0.])),
        ("zAxis", json!([0., 0., -1.])),
        ("yAxis", json!([1., 0., 0.])),
    ] {
        let mut definition = definition();
        let mut feature = mate("badMate", "REVOLUTE");
        feature["featureData"]["matedEntities"][0]["matedCS"][axis] = value;
        definition["rootAssembly"]["features"] = json!([feature]);
        let (model, report) = read(definition);
        assert_eq!(model.ir.model.assembly_joints.len(), 1, "{axis}");
        assert_eq!(
            count(
                &report,
                "onshape/assembly.mate-unsupported",
                "proper rigid frame"
            ),
            1,
            "{axis}"
        );
    }
}
