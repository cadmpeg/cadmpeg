// SPDX-License-Identifier: Apache-2.0
//! Projection work boundaries and short-circuit reads.

use super::{
    incomplete_history_reference_features, project_feature_dependencies, zero_offset_roots,
};
use crate::history::tests::feature;
use crate::records::{Feature, FeatureContent, FeatureHistory};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use std::collections::{BTreeMap, HashMap};

fn limited<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 10_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    run(&ctx)
}

fn without_retained_storage<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    run(&ctx)
}

fn retained_refusal_at<T>(
    operation: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            run(&ctx)
        },
    );
}

fn property(feature: &mut Feature, key: &'static str, value: String) {
    feature.properties.insert(
        cadmpeg_core::text::NonBlankString::try_from(key).unwrap(),
        value,
    );
}

#[test]
fn rejected_pattern_seed_releases_preceding_identity_copies() {
    let mut source = feature("pattern", None, 0);
    property(&mut source, "Seeds", "1,missing".into());
    let seed = FeatureId::mint("synthetic:test:feature#seed").unwrap();
    let by_source = HashMap::from([("1", &seed)]);
    let definition = without_retained_storage(|ctx| {
        super::pattern::project_pattern(ctx, &source, &by_source, &HashMap::new())
    })
    .unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) if seeds.is_empty())
    );
}

#[test]
fn unresolved_curve_pattern_releases_unused_path() {
    let mut source = feature("pattern", None, 0);
    source.kind = "CurveDrivenPattern".into();
    property(&mut source, "Path", "guide".into());
    let definition = without_retained_storage(|ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    })
    .unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if pattern == cadmpeg_ir::features::patterns::PatternKind::UNRESOLVED_CURVE_DRIVEN)
    );
}

#[test]
fn rejected_variable_fillet_releases_control_storage() {
    let mut source = feature("fillet", None, 0);
    source.kind = "VarFillet".into();
    for (key, value) in [
        ("Position0", "1"),
        ("Position1", "0"),
        ("Radius0", "2mm"),
        ("Radius1", "3mm"),
    ] {
        source
            .parameters
            .insert(key.try_into().unwrap(), value.into());
    }
    let definition =
        without_retained_storage(|ctx| super::modify::project_fillet(ctx, &source)).unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
        panic!("expected fillet");
    };
    assert!(matches!(
        groups.as_slice()[0].radius,
        cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
            form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable)
        }
    ));
}

#[test]
fn rejected_equation_domain_releases_expression_storage() {
    let mut source = feature("equation", None, 0);
    for (key, value) in [
        ("Parameter", "t"),
        ("XEquation", "t"),
        ("YEquation", "t"),
        ("ZEquation", "t"),
        ("Start", "NaN"),
        ("End", "1"),
    ] {
        property(&mut source, key, value.into());
    }
    assert!(
        without_retained_storage(|ctx| super::datum::project_equation_curve(ctx, &source))
            .unwrap()
            .is_none()
    );
}

#[test]
fn invalid_composite_closed_flag_does_not_retain_segments() {
    let mut source = feature("composite", None, 0);
    property(&mut source, "Segments", "first;second".into());
    property(&mut source, "Closed", "invalid".into());
    assert!(
        without_retained_storage(|ctx| super::datum::project_composite_curve(
            ctx,
            &source,
            &HashMap::new()
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn invalid_extrusion_direction_does_not_retain_face_selection() {
    let mut source = feature("extrusion", None, 0);
    property(&mut source, "EndCondition", "ToFace".into());
    property(&mut source, "Face", "face-a".into());
    property(&mut source, "Direction", "0,0,0".into());
    assert!(without_retained_storage(|ctx| {
        let mut sources = super::solid::SourceFeatures::new(ctx, &[])?;
        super::solid::project_extrude(ctx, &source, &HashMap::new(), &mut sources)
    })
    .unwrap()
    .is_none());
}

#[test]
fn invalid_sweep_scale_does_not_retain_profile_or_path() {
    let mut source = feature("sweep", None, 0);
    property(&mut source, "Profile", "profile".into());
    property(&mut source, "Path", "guide".into());
    source
        .parameters
        .insert("Scale".try_into().unwrap(), "invalid".into());
    assert!(without_retained_storage(|ctx| super::spin::project_sweep(
        ctx,
        &source,
        &HashMap::new()
    ))
    .unwrap()
    .is_none());
}

#[test]
fn missing_pattern_seed_does_not_read_the_remaining_text() {
    let mut source = feature("pattern", None, 0);
    property(
        &mut source,
        "Seeds",
        format!("missing,{}", "x".repeat(100_000)),
    );
    let definition = limited(|ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    })
    .unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) if seeds.is_empty())
    );
    crate::test_support::work_refusal_at("scan SLDPRT pattern seed delimiters", |ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    });
}

#[test]
fn ambiguous_extrusion_children_need_only_the_next_nonblank_character() {
    let mut source = feature("extrusion", None, 0);
    property(&mut source, "EndCondition", "ThroughAll".into());
    property(
        &mut source,
        "DissectableChildren",
        format!(" , first ,\u{2003},x{}", "x".repeat(100_000)),
    );
    let definition = limited(|ctx| {
        let mut index = super::solid::SourceFeatures::new(ctx, &[])?;
        super::solid::project_extrude(ctx, &source, &HashMap::new(), &mut index)
    })
    .unwrap();
    assert!(matches!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            ..
        }))
    ));
}

#[test]
fn authoritative_cut_class_does_not_read_the_kind_token() {
    let mut source = feature("cut", None, 0);
    source.input_class = Some("moCut_c".into());
    source.kind = "x".repeat(100_000);
    property(&mut source, "EndCondition", "ThroughAll".into());
    let definition = limited(|ctx| {
        let mut index = super::solid::SourceFeatures::new(ctx, &[])?;
        super::solid::project_extrude(ctx, &source, &HashMap::new(), &mut index)
    })
    .unwrap();
    assert!(matches!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }))
    ));
}

#[test]
fn incomplete_reference_count_stops_at_the_first_missing_source() {
    let mut source = feature("sldprt:history:feature#0:1", Some("1"), 0);
    property(
        &mut source,
        "Dependencies",
        format!("999,{}", "x".repeat(100_000)),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![source],
    };
    assert_eq!(
        limited(|ctx| incomplete_history_reference_features(ctx, std::slice::from_ref(&history)))
            .unwrap(),
        1
    );
}

#[test]
fn dependency_delimiters_preserve_source_order_and_uniqueness() {
    let mut source = feature("owner", None, 0);
    property(
        &mut source,
        "Dependencies",
        " ,1;\u{2003}2,,1;owner\t".into(),
    );
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    let first = FeatureId::mint("synthetic:test:feature#first").unwrap();
    let second = FeatureId::mint("synthetic:test:feature#second").unwrap();
    let index = HashMap::from([("1", &first), ("2", &second), ("owner", &owner)]);
    let result = project_feature_dependencies(
        &cadmpeg_test_support::service_decode_context(),
        &source,
        &owner,
        &index,
    )
    .unwrap();
    assert_eq!(result.as_slice(), [first, second]);
}

#[test]
fn ordered_hole_dimensions_do_not_traverse_unused_parameters() {
    let mut source = feature("profile", None, 0);
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("diameter"),
        "<MOD-DIAM>4".into(),
    );
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("depth"), "6".into());
    source.content = vec![
        FeatureContent::Dimension("diameter".into()),
        FeatureContent::Dimension("depth".into()),
    ];
    for index in 0..12_000 {
        source.parameters.insert(
            cadmpeg_core::text::NonBlankString::try_from(format!("unused-{index}")).unwrap(),
            "ignored".into(),
        );
    }
    let construction = limited(|ctx| super::solid::hole_sketch_construction(ctx, &source))
        .unwrap()
        .unwrap();
    assert_eq!(construction.diameter.get(), 4.0);
    assert_eq!(construction.depth.unwrap().get(), 6.0);
}

#[test]
fn invalid_fillet_position_does_not_read_its_radius() {
    let mut source = feature("fillet", None, 0);
    source.kind = "VarFillet".into();
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("Position0"),
        "invalid".into(),
    );
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("Radius0"),
        "x".repeat(100_000),
    );
    let definition = limited(|ctx| super::modify::project_fillet(ctx, &source)).unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if matches!(groups[0].radius, cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) }))
    );
}

#[test]
fn explicit_retention_mode_does_not_trim_the_kind() {
    let mut source = feature("delete", None, 0);
    source.kind = " ".repeat(100_000);
    property(&mut source, "Mode", "delete".into());
    let definition = limited(|ctx| super::modify::project_delete_body(ctx, &source)).unwrap();
    assert!(matches!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
            ..
        }))
    ));
}

#[test]
fn pattern_count_refuses_at_its_numeric_parse() {
    let mut source = feature("pattern", None, 0);
    source.kind = "LinearPattern".into();
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Spacing"), "4mm".into());
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Count"), "2".into());
    crate::test_support::work_refusal_at("parse SLDPRT pattern count", |ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    });
}

#[test]
fn zero_offset_roots_keep_cycle_entry_and_each_cycle_member() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut storage = ctx.reserve_scoped(0, "test zero offsets").unwrap();
    let roots = zero_offset_roots(
        &ctx,
        &mut storage,
        &[Some(1), Some(2), Some(1), None, Some(3)],
    )
    .unwrap();
    assert_eq!(roots, [1, 1, 2, 3, 3]);
}

#[test]
fn missing_replacement_faces_does_not_retain_target_faces() {
    let mut source = feature("replacement", None, 0);
    property(&mut source, "Faces", "face-a".into());
    assert!(
        without_retained_storage(|ctx| super::modify::project_replace_face(ctx, &source))
            .unwrap()
            .is_none()
    );
}

#[test]
fn accepted_pattern_seeds_refuse_retention_limit() {
    let mut source = feature("pattern", None, 0);
    property(&mut source, "Seeds", "1".into());
    let seed = FeatureId::mint("synthetic:test:feature#seed").unwrap();
    let by_source = HashMap::from([("1", &seed)]);
    retained_refusal_at("project SLDPRT pattern seeds", |ctx| {
        super::pattern::project_pattern(ctx, &source, &by_source, &HashMap::new())
    });
}

#[test]
fn accepted_curve_pattern_refuses_path_retention_limit() {
    let mut source = feature("pattern", None, 0);
    source.kind = "CurveDrivenPattern".into();
    property(&mut source, "Path", "guide".into());
    source
        .parameters
        .insert("Count".try_into().unwrap(), "2".into());
    source
        .parameters
        .insert("Spacing".try_into().unwrap(), "4mm".into());
    retained_refusal_at("retain SLDPRT pattern path", |ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    });
}

#[test]
fn unresolved_curve_pattern_does_not_read_unused_path() {
    let mut source = feature("pattern", None, 0);
    source.kind = "CurveDrivenPattern".into();
    property(&mut source, "Path", "x".repeat(100_000));
    let definition = limited(|ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    })
    .unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if pattern == cadmpeg_ir::features::patterns::PatternKind::UNRESOLVED_CURVE_DRIVEN)
    );
}

#[test]
fn accepted_variable_fillet_refuses_control_retention_limit() {
    let mut source = feature("fillet", None, 0);
    source.kind = "VarFillet".into();
    for (key, value) in [
        ("Position0", "0"),
        ("Position1", "1"),
        ("Radius0", "2mm"),
        ("Radius1", "3mm"),
    ] {
        source
            .parameters
            .insert(key.try_into().unwrap(), value.into());
    }
    retained_refusal_at("collect SLDPRT variable fillet controls", |ctx| {
        super::modify::project_fillet(ctx, &source)
    });
}

#[test]
fn accepted_equation_curve_refuses_expression_retention_limit() {
    let mut source = feature("equation", None, 0);
    for (key, value) in [
        ("Parameter", "t"),
        ("XEquation", "t"),
        ("YEquation", "t"),
        ("ZEquation", "t"),
        ("Start", "0"),
        ("End", "1"),
    ] {
        property(&mut source, key, value.into());
    }
    retained_refusal_at("retain SLDPRT equation curve", |ctx| {
        super::datum::project_equation_curve(ctx, &source)
    });
}

#[test]
fn invalid_composite_closed_flag_stops_at_first_nonblank_character() {
    let mut source = feature("composite", None, 0);
    property(&mut source, "Segments", format!("; \u{2003};{};second", "x".repeat(100_000)));
    property(&mut source, "Closed", "invalid".into());
    assert!(limited(|ctx| super::datum::project_composite_curve(ctx, &source, &HashMap::new())).unwrap().is_none());
    crate::test_support::work_refusal_at("project SLDPRT composite curve segments", |ctx| {
        super::datum::project_composite_curve(ctx, &source, &HashMap::new())
    });
}

#[test]
fn composite_segments_preserve_unicode_trimming_and_source_order() {
    let mut source = feature("composite", None, 0);
    property(&mut source, "Segments", "; \u{2003};first\u{2003} ; second ;".into());
    property(&mut source, "Closed", "true".into());
    let definition = super::datum::project_composite_curve(&cadmpeg_test_support::service_decode_context(), &source, &HashMap::from([("first", "native-first")])).unwrap();
    let Some(FeatureDefinition::Operation(FeatureOperation::CompositeCurve { segments, closed })) = definition else {
        panic!("expected composite curve");
    };
    assert!(closed);
    assert_eq!(segments.as_slice(), [cadmpeg_ir::features::PathRef::Native("native-first".into()), cadmpeg_ir::features::PathRef::Native("second".into())]);
}

#[test]
fn empty_composite_segments_do_not_read_closed_flag() {
    let mut source = feature("composite", None, 0);
    property(&mut source, "Segments", "; \u{2003}; \t;".into());
    property(&mut source, "Closed", "x".repeat(100_000));
    assert!(limited(|ctx| super::datum::project_composite_curve(ctx, &source, &HashMap::new())).unwrap().is_none());
}

#[test]
fn missing_fillet_position_leaves_remaining_radii_pass_unpaid() {
    let mut source = feature("fillet", None, 0);
    source.kind = "VarFillet".into();
    source.parameters.insert(cadmpeg_core::nonblank_literal!("Radius0"), "2mm".into());
    for index in 0..12_000 {
        source.parameters.insert(format!("z{index:05}").try_into().unwrap(), "unused".into());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 20_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let definition = super::modify::project_fillet(&ctx, &source).unwrap();
    assert!(matches!(definition, FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if matches!(groups[0].radius, cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) })));
    crate::test_support::work_refusal_at("scan SLDPRT variable fillet radii", |ctx| super::modify::project_fillet(ctx, &source));
}

#[test]
fn flex_reads_only_the_operand_of_its_selected_form() {
    use cadmpeg_ir::features::{FlexForm, FlexMode};
    for (mode, required, value, form) in [
        ("bending", "Angle", "1rad", Some(FlexForm::Bending)),
        ("twisting", "Angle", "1rad", Some(FlexForm::Twisting)),
        ("tapering", "Factor", "2", Some(FlexForm::Tapering)),
        ("stretching", "Distance", "1mm", Some(FlexForm::Stretching)),
        ("invalid", "", "", None),
    ] {
        let mut source = feature("flex", None, 0);
        property(&mut source, "Mode", mode.into());
        for name in ["Angle", "Factor", "Distance"] {
            source.parameters.insert(name.try_into().unwrap(), if name == required { value.into() } else { "x".repeat(100_000) });
        }
        let definition = limited(|ctx| super::modify::project_flex(ctx, &source)).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Flex { mode, .. }) = definition else {
            panic!("expected flex");
        };
        match (form, mode) {
            (Some(FlexForm::Bending), FlexMode::Bending { angle }) | (Some(FlexForm::Twisting), FlexMode::Twisting { angle }) => assert_eq!(angle.get(), 1.0),
            (Some(FlexForm::Tapering), FlexMode::Tapering { factor }) => assert_eq!(factor.get(), 2.0),
            (Some(FlexForm::Stretching), FlexMode::Stretching { distance }) => assert_eq!(distance.get(), 1.0),
            (None, FlexMode::Unresolved { form: None }) => {},
            (_, mode) => panic!("unexpected flex mode: {mode:?}"),
        }
        if let Some(form) = form {
            source.parameters.remove(required);
            assert!(matches!(limited(|ctx| super::modify::project_flex(ctx, &source)).unwrap(), FeatureDefinition::Operation(FeatureOperation::Flex { mode: FlexMode::Unresolved { form: Some(actual) }, .. }) if actual == form));
            source.parameters.insert(required.try_into().unwrap(), "invalid".into());
            assert!(matches!(limited(|ctx| super::modify::project_flex(ctx, &source)).unwrap(), FeatureDefinition::Operation(FeatureOperation::Flex { mode: FlexMode::Unresolved { form: Some(actual) }, .. }) if actual == form));
        }
    }
}

fn projected_plane(ordinal: u64, operation: FeatureOperation) -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: FeatureId::mint(format!("synthetic:test:feature#plane-{ordinal}")).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some(format!("synthetic:history:plane-{ordinal}")),
    }
}

#[test]
fn offset_candidate_index_is_not_built_without_a_search() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    use cadmpeg_ir::{features::{DatumPlaneReference, PrincipalPlane}, scalar::Length};
    let base = projected_plane(0, FeatureOperation::DatumPrincipalPlane { plane: PrincipalPlane::Top });
    for offsets in [Vec::new(), vec![projected_plane(1, FeatureOperation::DatumOffsetPlane { reference: Some(DatumPlaneReference::Feature { feature: base.id.clone() }), distance: Length::new(1.0).unwrap() })], vec![projected_plane(1, FeatureOperation::DatumOffsetPlane { reference: None, distance: Length::new(1.0).unwrap() })]] {
        let mut features = vec![base.clone()];
        features.extend(offsets);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "index SLDPRT offset plane candidates", None);
        super::bind_offset_plane_references(&ctx, &mut features).unwrap();
        assert!(ctx.resource_refusal().is_none());
        assert!(matches!(features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane: PrincipalPlane::Top })));
        if let Some(offset) = features.get(1) {
            assert!(matches!(offset.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. })));
        }
    }
}

#[test]
fn offset_candidate_index_refuses_at_first_search_and_binds_same_plane() {
    use cadmpeg_ir::{features::{DatumPlaneReference, PrincipalPlane}, scalar::Length};
    let base = projected_plane(0, FeatureOperation::DatumPrincipalPlane { plane: PrincipalPlane::Top });
    let mut offset = projected_plane(1, FeatureOperation::DatumOffsetPlane { reference: None, distance: Length::new(1.0).unwrap() });
    for (name, value) in [("Origin", "0mm,0mm,1mm"), ("Normal", "0,0,1"), ("UAxis", "1,0,0")] {
        offset.source_properties.insert(name.try_into().unwrap(), value.into());
    }
    crate::test_support::work_refusal_at("index SLDPRT offset plane candidates", |ctx| {
        let mut features = [base.clone(), offset.clone()];
        super::bind_offset_plane_references(ctx, &mut features)
    });
    let mut features = [base.clone(), offset];
    super::bind_offset_plane_references(&cadmpeg_test_support::service_decode_context(), &mut features).unwrap();
    assert!(matches!(features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { reference: Some(DatumPlaneReference::Feature { feature }), distance }) if feature == &base.id && distance.get() == 1.0));
    assert_eq!(features[1].dependencies.as_slice(), [base.id]);
}
