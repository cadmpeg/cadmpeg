// SPDX-License-Identifier: Apache-2.0
//! Assembly identity keys, repeated records, mate scope and malformed frames.
use crate::test_support::{assembly::*, read_bytes, Report};
use cadmpeg_core::json;
use serde_json::Value;
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
                "onshape/assembly.record-repeated",
                "element id repeats"
            ),
            1,
            "{element}"
        );
    }
}
#[test]
fn a_repeated_record_charges_only_when_its_content_differs() {
    let code = "onshape/assembly.record-repeated";
    let mut definition = nested();
    let mut sub = definition["subAssemblies"][0].clone();
    let mut row = definition["parts"][0].clone();
    // The same content spelled with folded identity fields drops nothing.
    sub["documentMicroversion"] = json!("microversion");
    sub["configuration"] = json!("");
    row["documentMicroversion"] = json!("microversion");
    row["configuration"] = json!("");
    for record in [sub.clone(), definition["subAssemblies"][0].clone()] {
        definition["subAssemblies"]
            .as_array_mut()
            .unwrap()
            .push(record);
    }
    for record in [row.clone(), definition["parts"][0].clone()] {
        definition["parts"].as_array_mut().unwrap().push(record);
    }
    let (_, report) = read(definition);
    assert_eq!(count(&report, code, ""), 0);
    assert_eq!(
        count(&report, "onshape/assembly.reference-unresolved", ""),
        0
    );
    // A different name or different instances is a real conflict.
    let mut definition = nested();
    sub["instances"] = json!([part("first")]);
    row["name"] = json!("other name");
    definition["subAssemblies"]
        .as_array_mut()
        .unwrap()
        .push(sub);
    definition["parts"].as_array_mut().unwrap().push(row);
    let (_, report) = read(definition);
    assert_eq!(count(&report, code, "subassembly definition repeats"), 1);
    assert_eq!(count(&report, code, "part identity repeats"), 1);
}
#[test]
fn a_subassembly_without_instances_charges_its_own_code() {
    let mut definition = nested();
    definition["subAssemblies"][0]
        .as_object_mut()
        .unwrap()
        .remove("instances");
    let (_, report) = read(definition);
    assert_eq!(
        count(
            &report,
            "onshape/assembly.instances-missing",
            "no instances field"
        ),
        1
    );
    assert_eq!(
        count(
            &report,
            "onshape/assembly.record-unsupported",
            "no instances field"
        ),
        0
    );
}
#[test]
fn an_omitted_or_empty_document_and_microversion_bind_like_the_pinned_ones() {
    for (field, value) in [
        ("documentId", None),
        ("documentId", Some("")),
        ("documentMicroversion", Some("")),
    ] {
        let mut definition = definition();
        let instance = definition["rootAssembly"]["instances"][1]
            .as_object_mut()
            .unwrap();
        match value {
            Some(value) => instance.insert(field.into(), json!(value)),
            None => instance.remove(field),
        };
        let (model, report) = read(definition);
        assert_eq!(parts(&model), 1, "{field} {value:?}");
        assert_eq!(
            count(&report, "onshape/assembly.reference-unresolved", ""),
            0,
            "{field} {value:?}"
        );
        let bound = model
            .ir
            .model
            .product_definitions
            .iter()
            .filter(|product| product.id.as_str().contains("part:") && !product.bodies.is_empty())
            .count();
        assert_eq!(bound, 1, "{field} {value:?}");
    }
}
#[test]
fn a_loss_detail_prints_the_stated_reference_fields() {
    let mut definition = nested();
    definition["rootAssembly"]["instances"][0]["documentMicroversion"] = json!("microversion");
    definition["rootAssembly"]["instances"][0]["configuration"] = json!("default");
    definition["rootAssembly"]["instances"][0]["elementId"] = json!("missing");
    let (_, report) = read(definition);
    assert_eq!(
        count(
            &report,
            "onshape/assembly.reference-unresolved",
            "documentMicroversion=\"microversion\", configuration=\"default\""
        ),
        1
    );
    let mut definition = nested();
    definition["rootAssembly"]["instances"][0]["elementId"] = json!("missing");
    definition["rootAssembly"]["instances"][0]
        .as_object_mut()
        .unwrap()
        .remove("configuration");
    let (_, report) = read(definition);
    assert_eq!(
        count(
            &report,
            "onshape/assembly.reference-unresolved",
            "documentMicroversion=omitted, configuration=omitted"
        ),
        1
    );
}
#[test]
fn several_part_studios_keep_each_source_on_its_definition() {
    let (model, _) = decode(&bundle_with_studios(definition(), true));
    let sources = model
        .ir
        .model
        .product_definitions
        .iter()
        .filter_map(|product| match &product.storage {
            cadmpeg_ir::products::Storage::File {
                document,
                source: Some(_),
            } => document.path.as_ref().map(|path| path.as_str().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(sources.len(), 2, "{sources:?}");
    assert!(sources
        .iter()
        .any(|path| path == "partstudios/studio/export.step"));
    assert!(sources
        .iter()
        .any(|path| path == "partstudios/other/export.step"));
}
#[test]
fn an_off_switch_governs_only_the_limit_it_names() {
    // (parameters, the id charged, ids left uncharged)
    for (parameters, expected, absent) in [
        (
            json!([{"parameterId":"minimumLimitEnabled","value":false},{"parameterId":"maximumLimit","value":1.0}]),
            Some("maximumLimit"),
            vec!["minimumLimit"],
        ),
        (
            json!([{"parameterId":"limitsEnabled","value":false},{"parameterId":"maximumLimit","value":1.0},{"parameterId":"offsetEnabled","value":true}]),
            None,
            vec!["maximumLimit", "offsetEnabled"],
        ),
        (
            json!([{"parameterId":"offsetEnabled","value":false},{"parameterId":"limitMaximum","value":1.0}]),
            Some("limitMaximum"),
            vec!["offsetEnabled"],
        ),
        // A switch name with an extra word silences the limit it names.
        (
            json!([{"parameterId":"enableMinimumLimitValue","value":false},{"parameterId":"minimumLimit","value":1.0},{"parameterId":"maximumLimit","value":2.0}]),
            Some("maximumLimit"),
            vec!["minimumLimit", "enableMinimumLimitValue"],
        ),
        // A parameter name with an extra word is silenced by the shorter switch.
        (
            json!([{"parameterId":"minimumLimitEnabled","value":false},{"parameterId":"minimumLimitValue","value":1.0},{"parameterId":"maximumLimitValue","value":2.0}]),
            Some("maximumLimitValue"),
            vec!["minimumLimitValue"],
        ),
        // A switch for one family leaves the other family charged.
        (
            json!([{"parameterId":"limitsEnabled","value":false},{"parameterId":"offsetDistance","value":1.0}]),
            Some("offsetDistance"),
            vec!["limitsEnabled"],
        ),
        // A true switch states a mode and is not charged.
        (
            json!([{"parameterId":"limitsEnabled","value":true},{"parameterId":"minimumLimitEnabled","value":true}]),
            None,
            vec!["limitsEnabled", "minimumLimitEnabled"],
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
        assert_eq!(
            count(&report, code, ""),
            usize::from(expected.is_some()),
            "{parameters}"
        );
        if let Some(id) = expected {
            assert_eq!(count(&report, code, id), 1, "{parameters}");
        }
        for id in absent {
            assert_eq!(count(&report, code, id), 0, "{id} {parameters}");
        }
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
