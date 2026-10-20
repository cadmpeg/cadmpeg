// SPDX-License-Identifier: Apache-2.0
use super::{
    assert_rejected, inspect_view, kernel_loss, kernel_loss_note, nx_13006, registry_rows,
    report_view, sldprt_13006, source_view, validate, Admission,
};
use cadmpeg_ir::report::loss::LossKind;
use cadmpeg_ir::report::Severity;
use serde_json::json;
use std::path::Path;

#[test]
fn view_local_kernel_loss_counts_include_zero_for_admitted_layers() {
    let registry = registry_rows();
    let nx = nx_13006(Admission::Residual);
    let decode = json!({
        "ir": {"source": source_view(&nx)},
        "report": report_view(&nx, &[kernel_loss("nx")])
    });
    let inspect = inspect_view(&nx, &[kernel_loss("nx")]);
    let wrapper = json!({"decode": decode, "inspect": inspect});
    assert_eq!(validate(&wrapper, &registry), Ok(3));

    let sldprt = sldprt_13006(Admission::Admitted);
    assert_rejected(
        &report_view(&sldprt, &[kernel_loss("sldprt")]),
        &registry,
        "has 0 residual Parasolid layers but 1 typed kernel losses",
    );

    let no_loss = report_view(&nx, &[]);
    assert_rejected(
        &no_loss,
        &registry,
        "has 1 residual Parasolid layers but 0 typed kernel losses",
    );

    assert_rejected(
        &report_view(&nx, &[kernel_loss("sldprt")]),
        &registry,
        "kernel loss must use the nx namespaced source_dialect_unverified code",
    );

    let mut missing_floor = kernel_loss_note("nx");
    let code = match &missing_floor.code {
        LossKind::Namespaced(code) => code.clone().with_strict_floor(None),
        LossKind::Shared { .. } => panic!("kernel loss uses a namespaced code"),
    };
    missing_floor.code = LossKind::Namespaced(code);
    assert_rejected(
        &report_view(
            &nx,
            &[serde_json::to_value(missing_floor).expect("typed loss serializes")],
        ),
        &registry,
        "kernel loss must reject strict mode",
    );

    let downgraded = kernel_loss_note("nx").with_severity(Severity::Info);
    assert_rejected(
        &report_view(
            &nx,
            &[serde_json::to_value(downgraded).expect("typed loss serializes")],
        ),
        &registry,
        "kernel loss must reject strict mode",
    );

    let mut flattened = report_view(&nx, &[kernel_loss("nx")]);
    flattened["losses"][0]["code"] = json!("source.kernel-dialect-unverified");
    assert_rejected(&flattened, &registry, "invalid typed loss note");
}

#[test]
fn source_and_decode_report_layer_sets_must_match() {
    let registry = registry_rows();
    let nx = nx_13006(Admission::Residual);
    let mut decode = json!({
        "ir": {"source": source_view(&nx)},
        "report": report_view(&nx, &[kernel_loss("nx")])
    });
    decode["report"]["identity"]["dialects"]["extra"][0]["admission"] = json!("admitted");
    let violation = validate(&decode, &registry).expect_err("the two decode identities differ");
    assert_eq!(violation.pointer, "/report/identity/dialects");
    assert_eq!(
        violation.reason,
        "layer identity differs from /ir/source/identity/dialects"
    );
}

#[test]
fn decode_report_and_inspect_layer_sets_must_match() {
    let registry = registry_rows();
    let nx = nx_13006(Admission::Residual);
    let mut wrapper = json!({
        "decode": {
            "ir": {"source": source_view(&nx)},
            "report": report_view(&nx, &[kernel_loss("nx")])
        },
        "inspect": inspect_view(&nx, &[kernel_loss("nx")])
    });
    wrapper["inspect"]["identity"]["dialects"]["extra"][0]["admission"] = json!("admitted");
    let violation =
        validate(&wrapper, &registry).expect_err("the decode and inspect identities differ");
    assert_eq!(violation.pointer, "/inspect/identity/dialects");
    assert_eq!(
        violation.reason,
        "layer identity differs from /decode/report/identity/dialects"
    );
}

#[test]
fn successful_view_identities_cannot_all_be_removed() {
    let registry = registry_rows();
    let nx = nx_13006(Admission::Residual);
    let mut wrapper = json!({
        "decode": {
            "ir": {"source": source_view(&nx)},
            "report": report_view(&nx, &[kernel_loss("nx")])
        },
        "inspect": inspect_view(&nx, &[kernel_loss("nx")])
    });
    wrapper["decode"]["ir"]["source"]
        .as_object_mut()
        .expect("source metadata is an object")
        .remove("identity");
    wrapper["decode"]["report"]
        .as_object_mut()
        .expect("decode report is an object")
        .remove("identity");
    wrapper["inspect"]
        .as_object_mut()
        .expect("container summary is an object")
        .remove("identity");

    assert_rejected(&wrapper, &registry, "missing its typed identity");
}

#[test]
fn each_successful_view_shape_requires_a_typed_identity() {
    let registry = registry_rows();
    let nx = nx_13006(Admission::Residual);
    let mut report = report_view(&nx, &[kernel_loss("nx")]);
    report
        .as_object_mut()
        .expect("decode report is an object")
        .remove("identity");
    let mut inspect = inspect_view(&nx, &[kernel_loss("nx")]);
    inspect
        .as_object_mut()
        .expect("container summary is an object")
        .remove("identity");
    let source = json!({"ir": {"source": {"attributes": {}}}});

    for (value, pointer, reason) in [
        (
            source,
            "/ir/source/identity",
            "successful source metadata is missing its typed identity",
        ),
        (
            report,
            "/identity",
            "successful decode report is missing its typed identity",
        ),
        (
            inspect,
            "/identity",
            "successful container summary is missing its typed identity",
        ),
    ] {
        let violation = validate(&value, &registry).expect_err("typed identity is required");
        assert_eq!(violation.pointer, pointer);
        assert_eq!(violation.reason, reason);
    }
}

#[test]
fn unclassified_successful_view_identities_remain_valid() {
    let registry = registry_rows();
    let identity = json!({"classification": "unclassified", "format": "nx"});
    let wrapper = json!({
        "decode": {
            "ir": {
                "source": {"identity": identity.clone(), "attributes": {}}
            },
            "report": {
                "identity": identity.clone(),
                "transfer": {"transfer": "full", "geometry_transferred": true},
                "losses": [],
                "notes": []
            }
        },
        "inspect": {
            "identity": identity.clone(),
            "container_kind": "flat",
            "entries": [],
            "losses": [],
            "notes": []
        }
    });
    assert_eq!(validate(&wrapper, &registry), Ok(0));
}

#[test]
fn real_decode_and_inspect_error_fixtures_are_not_success_views() {
    let registry = registry_rows();
    let golden_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../cadmpeg-codec-rhino/tests/golden");
    for relative in [
        "decode/reject_v1.json",
        "decode/reject_v2.json",
        "inspect/reject_v2.json",
    ] {
        let path = golden_root.join(relative);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
        assert_eq!(validate(&value, &registry), Ok(0), "{}", path.display());
    }
}
