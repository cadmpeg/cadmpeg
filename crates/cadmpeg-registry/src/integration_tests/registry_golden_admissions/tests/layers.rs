// SPDX-License-Identifier: Apache-2.0
use super::{
    assert_rejected, kernel_loss, layer, layers, native_admission_compatible, nx_13006,
    registry_rows, report_view, validate, Admission, Grammar, ReadDisposition,
};
use serde_json::json;

#[test]
fn malformed_or_orphaned_layer_sets_are_rejected() {
    let registry = registry_rows();
    let nx = nx_13006(Admission::Residual);
    let mut missing_primary = report_view(&nx, &[kernel_loss("nx")]);
    missing_primary["identity"]["dialects"]
        .as_object_mut()
        .expect("dialect set is an object")
        .remove("primary");
    assert_rejected(&missing_primary, &registry, "invalid dialect layer set");

    let mut missing_layer_set = report_view(&nx, &[kernel_loss("nx")]);
    missing_layer_set["identity"]
        .as_object_mut()
        .expect("identity is an object")
        .remove("dialects");
    assert_rejected(
        &missing_layer_set,
        &registry,
        "classified identity is missing its dialect layer set",
    );

    let mut missing_extra = report_view(&nx, &[kernel_loss("nx")]);
    missing_extra["identity"]["dialects"]
        .as_object_mut()
        .expect("dialect set is an object")
        .remove("extra");
    assert_rejected(&missing_extra, &registry, "invalid dialect layer set");

    let mut invalid_admission = report_view(&nx, &[kernel_loss("nx")]);
    invalid_admission["identity"]["dialects"]["extra"][0]["admission"] = json!("recovered");
    assert_rejected(&invalid_admission, &registry, "invalid dialect layer");

    let mut duplicate = report_view(&nx, &[kernel_loss("nx")]);
    let repeated = duplicate["identity"]["dialects"]["extra"][0].clone();
    duplicate["identity"]["dialects"]["extra"]
        .as_array_mut()
        .expect("extra layers are an array")
        .push(repeated);
    assert_rejected(&duplicate, &registry, "invalid dialect layer set");

    let mut orphan = report_view(&nx, &[kernel_loss("nx")]);
    orphan["orphan"] = orphan["identity"]["dialects"]["extra"][0].clone();
    assert_rejected(&orphan, &registry, "layer is orphaned or missing");
}

#[test]
fn primary_and_non_parasolid_admission_matrix_stays_strict() {
    let level = ReadDisposition::try_from("L4".to_owned()).expect("registry level is valid");
    assert!(native_admission_compatible(level, &Admission::Admitted));
    assert!(native_admission_compatible(
        ReadDisposition::Detected,
        &Admission::Admitted
    ));
    assert!(native_admission_compatible(
        ReadDisposition::UnclassifiedRecovered,
        &Admission::Residual
    ));
    assert!(native_admission_compatible(
        ReadDisposition::UnclassifiedRecovered,
        &Admission::Unverified {
            using: Grammar::parse("ap242").expect("grammar is valid")
        }
    ));
    assert!(native_admission_compatible(
        ReadDisposition::Refused,
        &Admission::Refused
    ));
    assert!(!native_admission_compatible(
        ReadDisposition::UnclassifiedRecovered,
        &Admission::Admitted
    ));
    assert!(!native_admission_compatible(
        ReadDisposition::Detected,
        &Admission::Residual
    ));
    assert!(!native_admission_compatible(
        level,
        &Admission::Unverified {
            using: Grammar::parse("ap242").expect("grammar is valid")
        }
    ));
    assert!(!native_admission_compatible(
        ReadDisposition::Refused,
        &Admission::Admitted
    ));
    assert!(!native_admission_compatible(
        ReadDisposition::Detected,
        &Admission::Refused
    ));

    let registry = registry_rows();
    let acis_read = *registry
        .get("acis:unknown")
        .expect("ACIS unknown row exists");
    assert_eq!(acis_read, ReadDisposition::Refused);
    let refused_acis = layers(
        layer("nx:splmsstr", Admission::Admitted),
        vec![layer("acis:unknown", Admission::Refused)],
    );
    assert_eq!(validate(&report_view(&refused_acis, &[]), &registry), Ok(1));

    let wrong_acis = layers(
        layer("nx:splmsstr", Admission::Admitted),
        vec![layer("acis:unknown", Admission::Admitted)],
    );
    assert_rejected(
        &report_view(&wrong_acis, &[]),
        &registry,
        &format!("secondary acis:unknown admission Admitted contradicts registry read {acis_read}"),
    );
}
