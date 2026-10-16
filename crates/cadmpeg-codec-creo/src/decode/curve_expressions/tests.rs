// SPDX-License-Identifier: Apache-2.0

#[test]
fn curve_expression_frame_admits_only_finite_local_origin() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=0-t*360\0z=-2+10*t\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let slots = record
        .local_system
        .as_ref()
        .expect("local system")
        .explicit_slots
        .expect("explicit slots");
    let mut nonfinite = slots.get();
    nonfinite[9] = f64::NAN;
    assert!(cadmpeg_ir::units::FiniteVector::new(nonfinite).is_none());

    let helix =
        crate::decode::with_service_context(|ctx| crate::curve::expression_helix(ctx, &record))
            .expect("decode the expression helix")
            .expect("affine helix");
    assert!(super::curve_expression_helix_definition(&record, &helix).is_some());
}

#[test]
fn curve_expression_parameter_order_orders_a_dependent_assignment() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=a+1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    assert_eq!(record.assignments.len(), 2);
    let unique_assignment_indices = std::collections::BTreeMap::new();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("build the decode context");
    assert!(
        super::curve_expression_parameter_order(&ctx, &record, &unique_assignment_indices)
            .expect("compute the curve expression parameter order")
            .is_some()
    );
}

#[test]
fn curve_expression_parameter_order_resolves_a_unique_dependency() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=a+1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let unique_assignment_indices = std::collections::BTreeMap::from([("a".to_owned(), 0)]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("build the decode context");
    assert!(
        super::curve_expression_parameter_order(&ctx, &record, &unique_assignment_indices)
            .expect("compute the curve expression parameter order")
            .is_some()
    );
}

#[test]
fn curve_expression_parameter_order_looks_up_named_dependencies() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=a+1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let indices = std::collections::BTreeMap::from([("a".to_string(), 0)]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &DecodePolicy::service())
        .expect("build the decode context");
    assert!(
        super::curve_expression_parameter_order(&ctx, &record, &indices)
            .expect("compute the curve expression parameter order")
            .is_some()
    );
}

#[test]
fn curve_expression_dependency_walk_reaches_a_direct_dependency() {
    let dependencies = [vec![1], vec![]];
    assert!(crate::decode::with_service_context(|ctx| {
        super::expression_dependency_reaches(ctx, &dependencies, 0, 1)
    })
    .expect("check expression dependency reaches"));
}

#[test]
fn curve_expression_parameter_order_admits_cyclic_dependencies() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=b\0b=a\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let indices = std::collections::BTreeMap::from([("a".to_owned(), 0), ("b".to_owned(), 1)]);
    assert!(crate::decode::with_service_context(|ctx| {
        super::curve_expression_parameter_order(ctx, &record, &indices)
    })
    .expect("compute the curve expression parameter order")
    .is_some());
}

#[test]
fn curve_expression_parameter_order_assigns_independent_ordinals() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let indices = std::collections::BTreeMap::new();
    assert!(crate::decode::with_service_context(|ctx| {
        super::curve_expression_parameter_order(ctx, &record, &indices)
    })
    .expect("compute the curve expression parameter order")
    .is_some());
}

#[test]
fn curve_expression_unique_name_keeps_its_spelling() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let names = { super::curve_expression_parameter_names(&record.assignments) }
        .expect("compute the curve expression parameter names");
    assert_eq!(names, vec![Some("a".to_owned())]);
}

#[test]
fn curve_expression_repeated_names_get_occurrence_suffixes() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0a=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let names = { super::curve_expression_parameter_names(&record.assignments) }
        .expect("compute the curve expression parameter names");
    assert_eq!(names, vec![Some("a#1".to_owned()), Some("a#2".to_owned())]);
}

#[test]
fn curve_expression_assignment_indices_map_each_name() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let indices = super::curve_expression_assignment_indices(&record);
    let (by_name, unique) = (indices.by_name, indices.unique);
    assert_eq!(by_name.len(), 2);
    assert_eq!(unique.len(), 2);
}

#[test]
fn curve_expression_emitted_ordinals_follow_assignment_order() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02a=1\0b=2\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let emitted = { super::curve_expression_emitted_ordinals(&record, &[0, 1]) }
        .expect("compute the curve expression emitted ordinals");
    assert_eq!(emitted.get(&0), Some(&0));
    assert_eq!(emitted.get(&1), Some(&1));
}

#[test]
fn curve_expression_transfer_emits_one_feature() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let run = |policy: DecodePolicy| {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input fits the limit");
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves.expressions.push(record.clone());
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::new(None);
        super::transfer_curve_expression_features(
            &ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &std::collections::BTreeMap::new(),
            &mut carriers,
        )
    };
    assert_eq!(
        run(DecodePolicy::service()).expect("transfer curve expression features"),
        1
    );
}

#[test]
fn curve_expression_external_dependency_text_joins_included_names() {
    let names = vec!["outside".to_string()];
    assert_eq!(
        super::joined_dependency_names(&names, |_| true),
        Some("outside".to_string())
    );
}

#[test]
fn curve_expression_intrinsic_dependency_text_joins_matching_names() {
    let names = vec!["t".to_string(), "T".to_string()];
    assert_eq!(
        super::joined_dependency_names(&names, |name| name.eq_ignore_ascii_case("t")),
        Some("t,T".to_string())
    );
}

#[test]
fn curve_expression_cyclic_dependency_text_joins_with_commas() {
    let names = ["alpha", "beta"];
    assert_eq!(super::join_cyclic_dependency_names(&names), "alpha,beta");
}

fn quantity_property_result() -> Result<
    std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    cadmpeg_core::CodecError,
> {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let mut record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    record.assignments[0].value = Some(crate::curve::CurveExpressionValue::Quantity(
        crate::curve::CurveExpressionQuantity::new(3.5, [1, 2, 0, 0, 0])
            .expect("valid residual dimension fixture"),
    ));
    let parameter_id = cadmpeg_ir::features::ParameterId::mint("test:test:parameter#a")
        .expect("valid parameter ID");
    super::curve_expression_properties(
        &record.assignments[0],
        0,
        ("a", &parameter_id),
        (
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        ),
        &std::collections::BTreeMap::new(),
        &std::collections::HashSet::new(),
    )
}

#[test]
fn curve_expression_quantity_properties_keep_value_and_dimension() {
    let properties = quantity_property_result().expect("compute the quantity property result");
    assert_eq!(properties.len(), 4);
    assert_eq!(
        properties[&cadmpeg_core::nonblank_literal!("evaluated_canonical_value")],
        "3.5"
    );
    assert_eq!(
        properties[&cadmpeg_core::nonblank_literal!("evaluated_dimension")],
        "length:1,mass:2,time:0,angle:0,temperature:0"
    );
}

#[test]
fn curve_expression_native_fallback_keeps_kind_and_values() {
    let definition = super::native_curve_expression_definition(7, 1)
        .expect("compute the native curve expression definition");
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Native { kind, parameters },
    ) = definition
    else {
        panic!("native fallback definition");
    };
    assert_eq!(kind.as_str(), "CurveFromEquation");
    assert_eq!(
        parameters[&cadmpeg_core::nonblank_literal!("entity_id")],
        "7"
    );
    assert_eq!(
        parameters[&cadmpeg_core::nonblank_literal!("assignment_count")],
        "1"
    );
}

#[test]
fn curve_expression_feature_labels_keep_source_spelling() {
    assert_eq!(
        super::curve_expression_feature_labels(7),
        ("Curve Equation 7".to_string(), "crv_fr_eqn".to_string())
    );
}

#[test]
fn curve_expression_source_text_joins_lines() {
    let lines = [
        crate::curve::CurveExpressionLine {
            text: "alpha".to_string(),
            offset: 0,
        },
        crate::curve::CurveExpressionLine {
            text: "beta".to_string(),
            offset: 6,
        },
    ];
    assert_eq!(super::curve_expression_source_text(&lines), "alpha\nbeta");
}

#[test]
fn curve_expression_named_properties_build_both_maps() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01a=1\0";
    let record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    let assignment = &record.assignments[0];
    let parameter_id = cadmpeg_ir::features::ParameterId::mint("test:test:parameter#a")
        .expect("valid parameter id");
    let assignment_indices = std::collections::BTreeMap::new();
    let unique_indices = std::collections::BTreeMap::new();
    let dimensions = std::collections::BTreeMap::new();
    let edges = std::collections::HashSet::new();
    assert_eq!(
        (super::curve_expression_properties(
            assignment,
            0,
            ("a", &parameter_id),
            (&assignment_indices, &unique_indices),
            &dimensions,
            &edges,
        ))
        .expect("compute the curve expression properties")
        .len(),
        2
    );
}
