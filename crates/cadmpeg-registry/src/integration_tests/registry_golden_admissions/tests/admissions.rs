// SPDX-License-Identifier: Apache-2.0
use super::{
    assert_rejected, kernel_layer, kernel_loss, layer, layers, nx_13006, registry_rows,
    report_view, sldprt_13006, validate, Admission, BTreeMap, Grammar, NonBlankString,
    DECLARED_SCHEMA,
};

#[test]
fn literal_nx_and_sldprt_13006_expectations_keep_host_admissions_distinct() {
    let registry = registry_rows();
    let nx = nx_13006(Admission::Residual);
    let nx_view = report_view(&nx, &[kernel_loss("nx")]);
    assert_eq!(validate(&nx_view, &registry), Ok(1));

    let sldprt = sldprt_13006(Admission::Admitted);
    let sldprt_view = report_view(&sldprt, &[]);
    assert_eq!(validate(&sldprt_view, &registry), Ok(1));

    let sldprt_residual = layers(
        layer("sldprt:unknown", Admission::Residual),
        vec![kernel_layer(
            "parasolid:format-13006",
            Some("SCH_TEST_13006"),
            Admission::Admitted,
        )],
    );
    assert_eq!(
        validate(&report_view(&sldprt_residual, &[]), &registry),
        Ok(1)
    );
}

#[test]
fn host_policy_rejects_wrong_admission_and_schema_identity() {
    let registry = registry_rows();
    assert_rejected(
        &report_view(&nx_13006(Admission::Admitted), &[]),
        &registry,
        "host nx:splmsstr classifies kernel parasolid:format-13006",
    );
    assert_rejected(
        &report_view(&sldprt_13006(Admission::Residual), &[kernel_loss("sldprt")]),
        &registry,
        "host sldprt:sw-version-pre-12000 classifies kernel parasolid:format-13006",
    );
    for invalid_admission in [
        Admission::Unverified {
            using: Grammar::parse("parasolid").expect("grammar is valid"),
        },
        Admission::Refused,
    ] {
        assert_rejected(
            &report_view(&nx_13006(invalid_admission.clone()), &[]),
            &registry,
            "host nx:splmsstr classifies kernel parasolid:format-13006",
        );
        assert_rejected(
            &report_view(&sldprt_13006(invalid_admission), &[]),
            &registry,
            "host sldprt:sw-version-pre-12000 classifies kernel parasolid:format-13006",
        );
    }
    let wrong_id = layers(
        layer("sldprt:sw-version-pre-12000", Admission::Admitted),
        vec![kernel_layer(
            "parasolid:sch-sw-33103",
            Some("SCH_TEST_13006"),
            Admission::Admitted,
        )],
    );
    assert_rejected(
        &report_view(&wrong_id, &[]),
        &registry,
        "classifies kernel parasolid:format-13006",
    );
}

#[test]
fn unknown_schema_stays_residual_and_unknown_host_cannot_claim_kernel_policy() {
    let registry = registry_rows();
    for (host, layers, losses) in [
        (
            "nx",
            layers(
                layer("nx:splmsstr", Admission::Admitted),
                vec![kernel_layer(
                    "parasolid:unknown",
                    Some("SCH_TEST_1_9999"),
                    Admission::Residual,
                )],
            ),
            1,
        ),
        (
            "sldprt",
            layers(
                layer("sldprt:unknown", Admission::Residual),
                vec![kernel_layer(
                    "parasolid:unknown",
                    Some("SCH_TEST_1_9999"),
                    Admission::Residual,
                )],
            ),
            1,
        ),
    ] {
        let losses: Vec<_> = (0..losses).map(|_| kernel_loss(host)).collect();
        assert_eq!(validate(&report_view(&layers, &losses), &registry), Ok(1));
    }

    let foreign_host = layers(
        layer("catia:standard-nested", Admission::Admitted),
        vec![kernel_layer(
            "parasolid:format-13006",
            Some("SCH_TEST_13006"),
            Admission::Residual,
        )],
    );
    assert_rejected(
        &report_view(&foreign_host, &[kernel_loss("catia")]),
        &registry,
        "has no Parasolid verification policy",
    );
}

#[test]
fn parasolid_extras_require_registry_rows_and_both_declarations() {
    let mut registry = registry_rows();
    let missing_row = registry
        .remove("parasolid:format-13006")
        .expect("row exists");
    let value = report_view(&nx_13006(Admission::Residual), &[kernel_loss("nx")]);
    assert_rejected(
        &value,
        &registry,
        "parasolid:format-13006 has no registry support row",
    );
    registry.insert("parasolid:format-13006".to_owned(), missing_row);

    let no_schema = layers(
        layer("nx:splmsstr", Admission::Admitted),
        vec![kernel_layer(
            "parasolid:format-13006",
            None,
            Admission::Residual,
        )],
    );
    assert_rejected(
        &report_view(&no_schema, &[kernel_loss("nx")]),
        &registry,
        "is missing its schema declaration",
    );

    let no_carrier =
        layer("parasolid:format-13006", Admission::Residual).with_declared(BTreeMap::from([(
            NonBlankString::new(DECLARED_SCHEMA.to_owned()).expect("key is nonblank"),
            "SCH_TEST_13006".to_owned(),
        )]));
    let no_carrier = layers(layer("nx:splmsstr", Admission::Admitted), vec![no_carrier]);
    assert_rejected(
        &report_view(&no_carrier, &[kernel_loss("nx")]),
        &registry,
        "is missing its carrier declaration",
    );

    let invalid_schema = layers(
        layer("nx:splmsstr", Admission::Admitted),
        vec![kernel_layer(
            "parasolid:format-13006",
            Some("not-a-schema"),
            Admission::Residual,
        )],
    );
    assert_rejected(
        &report_view(&invalid_schema, &[kernel_loss("nx")]),
        &registry,
        "has an invalid schema declaration",
    );
}
