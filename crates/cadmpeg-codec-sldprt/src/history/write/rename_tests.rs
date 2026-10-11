// SPDX-License-Identifier: Apache-2.0
//! Qualified expression renames preserve dependency-order suffix selection.

use super::apply_feature_name_changes;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, FeatureId, ParameterId};
use std::collections::{BTreeMap, HashMap};

fn parameter(id: &str, owner: Option<FeatureId>, expression: &str) -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint(id).unwrap(),
        owner,
        ordinal: 0,
        name: "Width".into(),
        expression: expression.into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

#[test]
fn qualified_feature_renames_preserve_first_dependency_suffix_match() {
    let long = FeatureId::mint("synthetic:test:feature#long").unwrap();
    let short = FeatureId::mint("synthetic:test:feature#short").unwrap();
    let changes = HashMap::from([
        (long.clone(), ("A@B".into(), "Long".into())),
        (short.clone(), ("B".into(), "Short".into())),
    ]);
    for (reverse, expected) in [
        (false, "Width@Long + \"Width@Long\""),
        (true, "Width@A@Short + \"Width@A@Short\""),
    ] {
        let mut parameters = [
            parameter("synthetic:test:parameter#long", Some(long.clone()), "1"),
            parameter("synthetic:test:parameter#short", Some(short.clone()), "2"),
            parameter(
                "synthetic:test:parameter#consumer",
                None,
                "Width@A@B + \"Width@A@B\"",
            ),
        ];
        let mut dependencies = vec![parameters[0].id.clone(), parameters[1].id.clone()];
        if reverse {
            dependencies.reverse();
        }
        parameters[2].dependencies = DistinctMembers::try_from(
            dependencies,
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap();
        parameters[0].properties.insert(
            cadmpeg_core::nonblank_literal!("EquationId"),
            "Width@A@B".into(),
        );
        apply_feature_name_changes(&mut parameters, &changes).unwrap();
        assert_eq!(parameters[2].expression, expected);
        assert_eq!(parameters[0].properties["EquationId"], "Width@Long");
    }
}

#[test]
fn qualified_feature_renames_escape_replacement_quotes_and_accept_empty_owner() {
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    for (old, expression, expected) in [
        ("", "Width@", "\"Width@δ\"\"owner\""),
        ("β", "\"Width@β\"", "\"Width@δ\"\"owner\""),
    ] {
        let mut parameters = [
            parameter("synthetic:test:parameter#owned", Some(owner.clone()), "1"),
            parameter("synthetic:test:parameter#consumer", None, expression),
        ];
        parameters[1].dependencies = DistinctMembers::try_from(
            vec![parameters[0].id.clone()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap();
        apply_feature_name_changes(
            &mut parameters,
            &HashMap::from([(owner.clone(), (old.into(), "δ\"owner".into()))]),
        )
        .unwrap();
        assert_eq!(parameters[1].expression, expected);
    }
}

#[test]
fn feature_rename_without_dependencies_does_not_copy_parameter_identity() {
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    let id = format!("synthetic:test:parameter#{}", "x".repeat(17 * 1024 * 1024));
    let mut parameters = [parameter(&id, Some(owner.clone()), "1")];
    parameters[0].properties.insert(
        cadmpeg_core::nonblank_literal!("EquationId"),
        "Width@Old".into(),
    );
    apply_feature_name_changes(
        &mut parameters,
        &HashMap::from([(owner, ("Old".into(), "New".into()))]),
    )
    .unwrap();
    assert_eq!(parameters[0].properties["EquationId"], "Width@New");
    assert_eq!(parameters[0].expression, "1");
}

#[test]
fn feature_rename_duplicate_parameter_without_owner_cancels_earlier_owner() {
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    let mut parameters = [
        parameter(
            "synthetic:test:parameter#duplicate",
            Some(owner.clone()),
            "1",
        ),
        parameter("synthetic:test:parameter#duplicate", None, "2"),
        parameter("synthetic:test:parameter#consumer", None, "Width@Old"),
    ];
    parameters[2].dependencies = DistinctMembers::try_from(
        vec![parameters[0].id.clone()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap();
    apply_feature_name_changes(
        &mut parameters,
        &HashMap::from([(owner, ("Old".into(), "New".into()))]),
    )
    .unwrap();
    assert_eq!(parameters[2].expression, "Width@Old");
}

#[test]
fn feature_rename_without_identifier_consumers_does_not_scan_owner_suffixes() {
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    let changes = HashMap::from([(owner.clone(), ("x".repeat(17 * 1024 * 1024), "New".into()))]);
    for expression in ["", "\"unterminated", "pi + 1", "\"\""] {
        let mut parameters = [
            parameter("synthetic:test:parameter#owned", Some(owner.clone()), "1"),
            parameter("synthetic:test:parameter#consumer", None, expression),
        ];
        parameters[1].dependencies = DistinctMembers::try_from(
            vec![parameters[0].id.clone()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap();
        let expected = parameters.clone();
        apply_feature_name_changes(&mut parameters, &changes).unwrap();
        assert_eq!(parameters, expected);
    }
}

#[test]
fn feature_rename_without_changed_dependency_owner_leaves_expression_unread() {
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    for live_owner in [false, true] {
        let mut parameters = vec![parameter(
            "synthetic:test:parameter#consumer",
            None,
            &"A+".repeat(1_000_001),
        )];
        parameters[0].dependencies = DistinctMembers::try_from(
            vec![ParameterId::mint("synthetic:test:parameter#missing").unwrap()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap();
        if live_owner {
            parameters.push(parameter(
                "synthetic:test:parameter#owned",
                Some(owner.clone()),
                "1",
            ));
        }
        let expected = parameters.clone();
        apply_feature_name_changes(
            &mut parameters,
            &HashMap::from([(owner.clone(), ("Old".into(), "New".into()))]),
        )
        .unwrap();
        assert_eq!(parameters, expected);
    }
}

#[test]
fn feature_rename_with_only_empty_dependency_expressions_builds_no_owner_index() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let fixture_policy = DecodePolicy::desktop();
    let (fixture_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &fixture_policy).unwrap();
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    let id = format!("synthetic:test:parameter#{}", "x".repeat(17 * 1024 * 1024));
    let owned = parameter(&id, Some(owner.clone()), "1");
    let mut consumer = parameter("synthetic:test:parameter#consumer", None, "");
    consumer.dependencies =
        DistinctMembers::try_from(vec![owned.id.clone()], &fixture_ctx).unwrap();
    let mut parameters = [owned, consumer];
    parameters[0].properties.insert(
        cadmpeg_core::nonblank_literal!("EquationId"),
        "Width@Old".into(),
    );
    let mut expected = parameters.clone();
    expected[0].properties.insert(
        cadmpeg_core::nonblank_literal!("EquationId"),
        "Width@New".into(),
    );
    apply_feature_name_changes(
        &mut parameters,
        &HashMap::from([(owner, ("Old".into(), "New".into()))]),
    )
    .unwrap();
    assert_eq!(parameters, expected);
}
