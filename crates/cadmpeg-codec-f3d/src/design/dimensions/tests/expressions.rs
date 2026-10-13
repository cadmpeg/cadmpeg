// SPDX-License-Identifier: Apache-2.0
use crate::design::dimensions::{
    expression_identifiers, unresolved_parameter_expression_dependency_count,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn expression_tokens_preserve_function_unit_and_unicode_rules() {
    let expression =
        "Width + sin(Width) + 1 cm + cm + _Point + (2) mm + mm + foo + 1 rad + rad + 2Width";
    let tokens = crate::test_support::with_decode_context(|ctx| {
        expression_identifiers(ctx, expression).collect::<Result<Vec<_>, _>>()
    })
    .unwrap();
    assert_eq!(
        tokens,
        ["Width", "Width", "cm", "_Point", "mm", "foo", "rad"]
    );
    let expression = "Width$µ°\"A + function\u{2003}(2) + _B";
    let tokens = crate::test_support::with_decode_context(|ctx| {
        expression_identifiers(ctx, expression).collect::<Result<Vec<_>, _>>()
    })
    .unwrap();
    assert_eq!(tokens, ["Width$µ°\"A", "_B"]);
}

#[test]
fn expression_iterator_ends_after_its_first_resource_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut identifiers = expression_identifiers(ctx, "\u{2003}Width");
        assert!(
            matches!(identifiers.next(), Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "f3d expression character scan")
        );
        assert!(identifiers.next().is_none());
    });
}

fn fixture() -> (
    Vec<crate::records::parameters::DesignParameter>,
    Vec<cadmpeg_ir::features::DesignParameter>,
) {
    let native = [(1, "1 mm", "Width"), (2, "Width + 1 mm", "Half")]
        .map(|(record, expression, name)| {
            let mut parameter = crate::design::decode::parameters::parse_design_parameter_record(
                &crate::design::test_support::parameter_record(
                    None,
                    expression,
                    "User Parameter",
                    Some("mm"),
                    name,
                    1.0,
                ),
            )
            .unwrap();
            parameter.id = format!("f3d:A:parameter#{record}");
            parameter.record_index = record;
            parameter.source_ordinal = record;
            parameter
        })
        .to_vec();
    let (_, mut projected) = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &native,
                ..Default::default()
            },
        )
    })
    .unwrap();
    for parameter in &mut projected {
        parameter.dependencies.clear();
    }
    (native, projected)
}

#[test]
fn expression_audit_uses_only_scoped_lookup_storage() {
    let (native, projected) = fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let count = crate::test_support::with_decode_policy(&policy, |ctx| {
        unresolved_parameter_expression_dependency_count(ctx, &native, &projected)
    })
    .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn expression_audit_refuses_at_character_and_key_operations() {
    let (native, projected) = fixture();
    for operation in [
        "f3d expression character scan",
        "f3d expression following character",
        "f3d expression preceding character",
        "f3d expression stream lookup",
        "f3d expression projected parameter lookup",
        "f3d expression name lookup",
        "f3d expression identifier",
        "f3d expression dependency name lookup",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| unresolved_parameter_expression_dependency_count(ctx, &native, &projected),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}
