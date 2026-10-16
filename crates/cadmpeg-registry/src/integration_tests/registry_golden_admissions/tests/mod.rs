// SPDX-License-Identifier: Apache-2.0
use super::{
    check_parasolid_capabilities, native_admission_compatible, registry_rows, validate_snapshot,
    AdmissionViolation, ReadDisposition, RegistryRows, DECLARED_CARRIER, DECLARED_SCHEMA,
};
use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch, Grammar};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

mod admissions;
mod capabilities;
mod layers;
mod views;

fn id(value: &str) -> DialectId {
    DialectId::parse(value).expect("test dialect id is valid")
}

fn layer(value: &str, admission: Admission) -> DialectMatch {
    match admission {
        Admission::Admitted => DialectMatch::admitted(id(value)),
        Admission::Unverified { using } => DialectMatch::unverified(id(value), using),
        Admission::Residual => DialectMatch::residual(id(value)),
        Admission::Refused => DialectMatch::refused(id(value)),
    }
}

fn kernel_layer(value: &str, schema: Option<&str>, admission: Admission) -> DialectMatch {
    let mut declared = BTreeMap::new();
    if let Some(schema) = schema {
        declared.insert(
            NonBlankString::new(DECLARED_SCHEMA.to_owned()).expect("key is nonblank"),
            schema.to_owned(),
        );
    }
    declared.insert(
        NonBlankString::new(DECLARED_CARRIER.to_owned()).expect("key is nonblank"),
        "stream@76".to_owned(),
    );
    layer(value, admission).with_declared(declared)
}

fn layers(primary: DialectMatch, extras: Vec<DialectMatch>) -> DialectLayers {
    extras
        .into_iter()
        .fold(DialectLayers::of(primary), |set, extra| {
            set.with(extra).expect("test layer keys are unique")
        })
}

fn kernel_loss_note(host: &str) -> LossNote {
    LossNote::new(
        LossKind::namespaced(
            LossNamespace::new(host).expect("host namespace is not reserved"),
            "source.kernel-dialect-unverified",
            LossTaxonomy::SourceDialectUnverified,
        ),
        "test kernel dialect loss",
    )
}

fn kernel_loss(host: &str) -> Value {
    serde_json::to_value(kernel_loss_note(host)).expect("typed kernel loss serializes")
}

fn report_view(layers: &DialectLayers, losses: &[Value]) -> Value {
    json!({
        "identity": {"classification": "classified", "dialects": layers},
        "transfer": {"transfer": "full", "geometry_transferred": true},
        "losses": losses,
        "notes": []
    })
}

fn inspect_view(layers: &DialectLayers, losses: &[Value]) -> Value {
    json!({
        "identity": {"classification": "classified", "dialects": layers},
        "container_kind": "flat",
        "entries": [],
        "losses": losses,
        "notes": []
    })
}

fn source_view(layers: &DialectLayers) -> Value {
    json!({
        "identity": {"classification": "classified", "dialects": layers},
        "attributes": {}
    })
}

fn validate(value: &Value, registry: &RegistryRows) -> Result<usize, AdmissionViolation> {
    let ctx = cadmpeg_test_support::service_decode_context();
    validate_snapshot(value, Path::new("test.json"), registry, &ctx)
}

fn assert_rejected(value: &Value, registry: &RegistryRows, expected: &str) {
    let error = validate(value, registry).expect_err("mutated snapshot is invalid");
    assert!(error.to_string().contains(expected), "{error}");
}

fn nx_13006(admission: Admission) -> DialectLayers {
    layers(
        layer("nx:splmsstr", Admission::Admitted),
        vec![kernel_layer(
            "parasolid:format-13006",
            Some("SCH_TEST_13006"),
            admission,
        )],
    )
}

fn sldprt_13006(admission: Admission) -> DialectLayers {
    layers(
        layer("sldprt:sw-version-pre-12000", Admission::Admitted),
        vec![kernel_layer(
            "parasolid:format-13006",
            Some("SCH_TEST_13006"),
            admission,
        )],
    )
}
