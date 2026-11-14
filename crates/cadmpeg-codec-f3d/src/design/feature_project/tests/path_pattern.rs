// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::path_pattern::project_path_pattern;
use crate::design::test_support::parameter_record;
use crate::records::{
    feature::scope::{DesignFeatureKind, DesignParameterScope},
    identity::{Located, ReferenceRun},
    parameters::DesignParameter,
    topology::{
        construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
        extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::features::{
    patterns::{PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const SCOPE: u32 = 315;
const SEED_GROUP: u32 = 323;
const PATH_GROUP: u32 = 333;

fn settings(
    total_extent: bool,
    flipped: bool,
    path_oriented: bool,
    start_parameter: f64,
) -> crate::records::feature::patterns::DesignPathPatternSettings {
    crate::records::feature::patterns::DesignPathPatternSettings {
        reordered_owners: false,
        settings_record_index: Some(322),
        settings_byte_offset: Some(1_000),
        total_extent: Some(Located {
            value: total_extent,
            offset: 0,
        }),
        flipped: Some(Located {
            value: flipped,
            offset: 0,
        }),
        path_oriented: Some(Located {
            value: path_oriented,
            offset: 0,
        }),
        path_group_record_index: Some(PATH_GROUP),
        start_record_index: Some(343),
        start_frame_end: Some(197),
        start_parameter: Some(Located {
            value: cadmpeg_ir::scalar::FiniteReal::new(start_parameter).unwrap(),
            offset: 0,
        }),
    }
}

fn scope() -> DesignParameterScope {
    scope_with(Some(settings(false, false, false, 0.0)))
}

fn scope_with(
    settings: Option<crate::records::feature::patterns::DesignPathPatternSettings>,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#38353"),
        DesignFeatureKind::PathPattern,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            let members = [319, SEED_GROUP, 326, 331, PATH_GROUP, 336, 341, 343, 346];
            let shift = 11 * (u64::try_from(members.len()).unwrap() - 1);
            draft.reference_members = ReferenceRun::located(
                members
                    .into_iter()
                    .zip((14..).step_by(11))
                    .map(|(value, offset)| Located { value, offset })
                    .collect(),
            );
            draft.kind_offset += shift;
            draft.feature_ordinal_offset += shift;
            draft.frame_length += shift;
            draft.paired_byte_offset += shift;
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::PathPattern(slot) =
        scope.payload_mut()
    {
        *slot = settings;
    }
    scope
}

fn group(
    record_index: u32,
    scope_reference_ordinal: u32,
    role: DesignOperandRole,
    members: &[u32],
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record_index}"),
        scope_record_index: SCOPE,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned()).unwrap(),
        members: members
            .iter()
            .zip((26..).step_by(11))
            .map(|(value, offset)| Located {
                value: *value,
                offset,
            })
            .collect(),
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 0,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 99,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn parameter(source_kind: &str, unit: Option<&str>, value: f64) -> DesignParameter {
    parse_design_parameter_record(&parameter_record(
        Some(331),
        "4",
        source_kind,
        unit,
        "d3",
        value,
    ))
    .unwrap()
}

fn count(value: f64) -> DesignParameter {
    parameter("countU", None, value)
}

fn distance(value: f64) -> DesignParameter {
    parameter("PathPatternDelta", Some("mm"), value)
}

fn groups(seed_role: DesignOperandRole) -> Vec<DesignConstructionOperandGroup> {
    vec![
        group(SEED_GROUP, 1, seed_role, &[326]),
        group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
    ]
}

fn pattern(definition: FeatureDefinition) -> (Vec<PatternSeed>, PatternTransform) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("path pattern definition");
    };
    (seeds, pattern.definition().clone())
}

#[test]
fn path_pattern_reads_seed_path_count_and_spacing() {
    let (count, distance) = (count(4.0), distance(2.5));
    let parameters = [(0, &count), (1, &distance)];
    for (role, seed) in [
        (
            DesignOperandRole::BODIES_B,
            PatternSeed::Bodies(BodySelection::Native(format!(
                "{STREAM}:design-construction-operand-group#{SEED_GROUP}"
            ))),
        ),
        (
            DesignOperandRole::ROLE_0X10,
            PatternSeed::Faces(FaceSelection::Native(format!(
                "{STREAM}:design-construction-operand-group#{SEED_GROUP}"
            ))),
        ),
    ] {
        let projected =
            project_path_pattern(&scope(), &groups(role), &parameters).expect("path pattern");
        assert!(projected.losses.is_empty());
        let (seeds, transform) = pattern(projected.definition);
        assert_eq!(seeds, vec![seed]);
        let PatternTransform::CurveDriven {
            path,
            spacing,
            count,
        } = transform
        else {
            panic!("curve-driven transform");
        };
        assert_eq!(
            path,
            Some(PathRef::Native(format!(
                "{STREAM}:design-construction-operand-group#{PATH_GROUP}"
            )))
        );
        assert!((spacing.get() - 25.0).abs() <= 1.0e-12);
        assert_eq!(count, 4);
    }
}

#[test]
fn path_pattern_without_usable_parameters_keeps_an_unresolved_curve_driven_form() {
    let valid_count = count(4.0);
    let valid_distance = distance(2.5);
    let fractional = count(2.5);
    let zero = count(0.0);
    let measured_count = parameter("countU", Some("mm"), 4.0);
    let unitless_distance = parameter("PathPatternDelta", None, 2.5);
    let other_kind = parameter("Radius", Some("mm"), 2.5);
    for parameters in [
        vec![(1, &valid_distance)],
        vec![(0, &valid_count)],
        vec![(1, &valid_count), (0, &valid_distance)],
        vec![(0, &fractional), (1, &valid_distance)],
        vec![(0, &zero), (1, &valid_distance)],
        vec![(0, &measured_count), (1, &valid_distance)],
        vec![(0, &valid_count), (1, &unitless_distance)],
        vec![(0, &valid_count), (1, &other_kind)],
        vec![(0, &valid_count), (0, &valid_count), (1, &valid_distance)],
    ] {
        let projected =
            project_path_pattern(&scope(), &groups(DesignOperandRole::BODIES_B), &parameters)
                .expect("path pattern");
        assert!(projected.losses.is_empty());
        let (seeds, transform) = pattern(projected.definition);
        assert_eq!(seeds.len(), 1);
        assert!(matches!(
            transform,
            PatternTransform::Unresolved {
                form: Some(cadmpeg_ir::features::patterns::PatternForm::CurveDriven)
            }
        ));
    }
}

#[test]
fn path_pattern_rejects_groups_outside_its_form() {
    let (count, distance) = (count(4.0), distance(2.5));
    let parameters = [(0, &count), (1, &distance)];
    for groups in [
        vec![group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326])],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_A, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X10, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[331]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 2, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326, 331]),
            group(331, 3, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
            group(343, 7, DesignOperandRole::ROLE_0X5, &[346]),
        ],
    ] {
        assert!(project_path_pattern(&scope(), &groups, &parameters).is_none());
    }
    let mut unclosed = scope();
    unclosed.unclosed_construction_operand_groups = vec![343];
    assert!(
        project_path_pattern(&unclosed, &groups(DesignOperandRole::BODIES_B), &parameters)
            .is_none()
    );
}

#[test]
fn path_pattern_settings_select_the_spacing_and_name_dropped_placement() {
    let (count, distance) = (count(4.0), distance(6.0));
    let parameters = [(0, &count), (1, &distance)];
    let spacing_of = |scope: DesignParameterScope| {
        let projected =
            project_path_pattern(&scope, &groups(DesignOperandRole::BODIES_B), &parameters)
                .expect("path pattern");
        let (_, transform) = pattern(projected.definition);
        match transform {
            PatternTransform::CurveDriven { spacing, .. } => Some(spacing.get()),
            _ => None,
        }
    };
    assert!(
        spacing_of(scope_with(Some(settings(false, false, false, 0.0))))
            .is_some_and(|spacing| (spacing - 60.0).abs() <= 1.0e-12)
    );
    assert!(
        spacing_of(scope_with(Some(settings(true, false, false, 0.0))))
            .is_some_and(|spacing| (spacing - 20.0).abs() <= 1.0e-12)
    );
    assert_eq!(spacing_of(scope_with(None)), None);
    let one = self::count(1.0);
    assert!(project_path_pattern(
        &scope_with(Some(settings(true, false, false, 0.0))),
        &groups(DesignOperandRole::BODIES_B),
        &[(0, &one), (1, &distance)],
    )
    .is_some_and(|projected| matches!(
        pattern(projected.definition).1,
        PatternTransform::Unresolved { .. }
    )));

    let projected = project_path_pattern(
        &scope_with(Some(settings(false, true, true, 0.5))),
        &groups(DesignOperandRole::BODIES_B),
        &parameters,
    )
    .expect("path pattern");
    let [loss] = projected.losses.as_slice() else {
        panic!("one placement loss");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeaturePathPatternPlacementNotTransferred.kind()
    );
    for setting in [
        "flipped direction",
        "path-direction orientation",
        "path parameter 0.5",
    ] {
        assert!(loss.message.contains(setting), "{}", loss.message);
    }
}

#[test]
fn path_pattern_settings_naming_another_path_group_leave_the_form_unresolved() {
    let (count, distance) = (count(4.0), distance(2.5));
    let mut other_group = settings(false, false, false, 0.0);
    other_group.path_group_record_index = Some(PATH_GROUP + 1);
    let projected = project_path_pattern(
        &scope_with(Some(other_group)),
        &groups(DesignOperandRole::BODIES_B),
        &[(0, &count), (1, &distance)],
    )
    .expect("path pattern");
    assert!(matches!(
        pattern(projected.definition).1,
        PatternTransform::Unresolved { .. }
    ));
}

fn partial(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    parameters: &[(u32, &DesignParameter)],
) -> (
    Vec<PatternSeed>,
    cadmpeg_ir::features::patterns::curve_driven::CurveDrivenPatternParameters,
) {
    let projected = crate::design::feature_project::path_pattern::project_partial_path_pattern(
        scope,
        groups,
        parameters,
        &[],
        &[],
        &[],
        &std::collections::HashMap::new(),
    )
    .unwrap();
    assert!(projected
        .losses
        .iter()
        .any(|loss| loss.code
            == crate::loss::F3dLossCode::FeaturePathPatternDefinitionPartial.kind()));
    let (seeds, PatternTransform::CurveDrivenPartial { parameters }) =
        pattern(projected.definition)
    else {
        panic!("partial curve-driven pattern");
    };
    (seeds, parameters)
}

#[test]
fn partial_path_pattern_retains_independent_parameters_and_controls() {
    let count = count(5.0);
    let distance = distance(8.0);
    let groups = groups(DesignOperandRole::BODIES_B);
    let (seeds, parameters) = partial(
        &scope_with(Some(settings(true, true, true, 0.75))),
        &[],
        &[(0, &count), (1, &distance)],
    );
    assert!(seeds.is_empty());
    assert_eq!(
        parameters.path,
        Some(PathRef::Native(format!(
            "{STREAM}:design-record#{PATH_GROUP}"
        )))
    );
    assert_eq!(parameters.count, Some(5));
    assert_eq!(
        parameters
            .distance
            .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(80.0)
    );
    assert_eq!(
        parameters
            .spacing
            .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(20.0)
    );
    assert_eq!(parameters.total_extent, Some(true));
    assert_eq!(parameters.reversed, Some(true));
    assert_eq!(parameters.path_oriented, Some(true));
    assert_eq!(
        parameters
            .start_parameter
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(0.75)
    );
    let (_, missing_count) = partial(&scope(), &groups, &[(1, &distance)]);
    assert_eq!(missing_count.count, None);
    assert_eq!(
        missing_count
            .spacing
            .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(80.0)
    );
    let (_, missing_distance) = partial(&scope(), &groups, &[(0, &count)]);
    assert_eq!(missing_distance.count, Some(5));
    assert_eq!(missing_distance.distance, None);
    assert_eq!(missing_distance.spacing, None);
    let (_, missing_settings) = partial(&scope_with(None), &groups, &[(0, &count), (1, &distance)]);
    assert_eq!(missing_settings.count, Some(5));
    assert!(missing_settings.distance.is_some());
    assert!(missing_settings.path.is_some());
    assert_eq!(missing_settings.spacing, None);
    assert_eq!(missing_settings.reversed, None);
    let mut incomplete = settings(true, true, true, 0.75);
    incomplete.flipped = None;
    incomplete.start_parameter = None;
    let (_, retained) = partial(
        &scope_with(Some(incomplete)),
        &[],
        &[(0, &count), (1, &distance)],
    );
    assert_eq!(retained.reversed, None);
    assert_eq!(retained.start_parameter, None);
    assert_eq!(retained.path_oriented, Some(true));
    assert_eq!(
        retained
            .spacing
            .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(20.0)
    );
}

#[test]
fn partial_path_pattern_keeps_ambiguous_and_foreign_inputs_native() {
    let count = count(4.0);
    let distance = distance(3.0);
    let parameters = [(0, &count), (1, &distance)];
    let (seeds, retained) = partial(&scope(), &groups(DesignOperandRole::BODIES_A), &parameters);
    assert!(matches!(seeds.as_slice(), [PatternSeed::Native(_)]));
    assert!(retained.path.is_some());
    for invalid in [
        parameter("countU", None, 2.5),
        parameter("countU", None, 4_294_967_296.0),
        parameter("countU", Some("mm"), 4.0),
    ] {
        let pairs = [(0, &invalid), (1, &distance)];
        let (_, retained) = partial(&scope(), &[], &pairs);
        assert_eq!(retained.count, None);
        assert!(retained.distance.is_some());
    }
    let mut foreign = scope();
    foreign.id = "unclassified-scope".into();
    let (seeds, retained) = partial(&foreign, &groups(DesignOperandRole::BODIES_B), &parameters);
    assert!(seeds.is_empty());
    assert!(retained.path.is_none());
    assert_eq!(retained.count, Some(4));
    let mismatched = [
        group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[331]),
        group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
    ];
    assert!(matches!(
        partial(&scope(), &mismatched, &parameters).0.as_slice(),
        [PatternSeed::Native(_)]
    ));
    let (_, ambiguous_count) = partial(&scope(), &[], &[(0, &count), (0, &count), (1, &distance)]);
    assert_eq!(ambiguous_count.count, None);
    assert!(ambiguous_count.distance.is_some());
    let mut ambiguous_path = groups(DesignOperandRole::BODIES_B);
    ambiguous_path.push(group(343, 7, DesignOperandRole::ROLE_0X5, &[346]));
    let (seeds, retained) = partial(&scope_with(None), &ambiguous_path, &parameters);
    assert!(matches!(seeds.as_slice(), [PatternSeed::Bodies(_)]));
    assert!(retained.path.is_none());
    assert_eq!(retained.count, Some(4));
}

#[test]
fn partial_path_pattern_reads_compact_feature_seed_in_the_owning_stream() {
    use crate::design::feature_project::path_pattern::project_partial_path_pattern;
    let seed = super::pattern::seed_scope(5, Some(2));
    let mut selection = super::pattern::compact_entity_selection(SEED_GROUP, 326, 5);
    selection.scope_record_index = SCOPE;
    let groups = groups(DesignOperandRole::BODIES_A);
    let ordinals = std::collections::HashMap::from([((STREAM, SCOPE), 3), ((STREAM, 5), 1)]);
    let projected = project_partial_path_pattern(
        &scope(),
        &groups,
        &[],
        &[],
        std::slice::from_ref(&selection),
        std::slice::from_ref(&seed),
        &ordinals,
    )
    .unwrap();
    assert_eq!(
        pattern(projected.definition).0,
        vec![PatternSeed::Feature(crate::ids::neutral_feature_id(&seed))]
    );
    assert!(projected
        .losses
        .iter()
        .any(|loss| loss.code
            == crate::loss::F3dLossCode::FeaturePathPatternFeatureSeedAssumed.kind()));
    let mut foreign = seed.clone();
    foreign.id = "f3d:Other/BulkStream.dat:design-parameter-scope#5".into();
    for scopes in [vec![], vec![foreign], vec![seed.clone(), seed]] {
        let projected = project_partial_path_pattern(
            &scope(),
            &groups,
            &[],
            &[],
            std::slice::from_ref(&selection),
            &scopes,
            &ordinals,
        )
        .unwrap();
        assert!(matches!(
            pattern(projected.definition).0.as_slice(),
            [PatternSeed::Native(_)]
        ));
    }
}

#[test]
fn partial_path_pattern_retains_a_declared_path_outside_the_scope_reference_list() {
    use crate::design::feature_project::path_pattern::project_partial_path_pattern;
    let mut stored = settings(false, true, false, 0.25);
    stored.path_group_record_index = Some(999);
    let projected = project_partial_path_pattern(
        &scope_with(Some(stored)),
        &[],
        &[],
        &[],
        &[],
        &[],
        &std::collections::HashMap::new(),
    )
    .unwrap();
    assert!(
        projected
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::FeaturePathPatternPathUnresolved.kind())
    );
    let (_, PatternTransform::CurveDrivenPartial { parameters }) = pattern(projected.definition)
    else {
        panic!("partial curve-driven");
    };
    assert_eq!(
        parameters.path,
        Some(PathRef::Native(format!("{STREAM}:design-record#999")))
    );
    assert_eq!(parameters.reversed, Some(true));
    assert_eq!(
        parameters
            .start_parameter
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(0.25)
    );
}

#[test]
fn partial_path_pattern_keeps_later_and_unordered_feature_candidates_native() {
    use crate::design::feature_project::path_pattern::project_partial_path_pattern;
    let seed = super::pattern::seed_scope(5, Some(2));
    let mut selection = super::pattern::compact_entity_selection(SEED_GROUP, 326, 5);
    selection.scope_record_index = SCOPE;
    let count = count(4.0);
    let distance = distance(2.5);
    for ordinals in [
        std::collections::HashMap::from([((STREAM, SCOPE), 3), ((STREAM, 5), 3)]),
        std::collections::HashMap::from([((STREAM, SCOPE), 3), ((STREAM, 5), 4)]),
        std::collections::HashMap::from([((STREAM, SCOPE), 3)]),
        std::collections::HashMap::from([((STREAM, 5), 1)]),
    ] {
        let projected = project_partial_path_pattern(
            &scope(),
            &groups(DesignOperandRole::BODIES_A),
            &[(0, &count), (1, &distance)],
            &[],
            std::slice::from_ref(&selection),
            std::slice::from_ref(&seed),
            &ordinals,
        )
        .unwrap();
        assert!(projected
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::FeaturePathPatternSeedUnresolved.kind()));
        let (seeds, PatternTransform::CurveDrivenPartial { parameters }) =
            pattern(projected.definition)
        else {
            panic!("partial curve-driven");
        };
        assert!(matches!(seeds.as_slice(), [PatternSeed::Native(_)]));
        assert_eq!(parameters.count, Some(4));
        assert_eq!(
            parameters
                .spacing
                .map(cadmpeg_ir::scalar::PositiveLength::get),
            Some(25.0)
        );
        assert!(parameters.path.is_some());
    }
}

#[test]
fn partial_path_pattern_retains_ordered_known_and_unknown_seed_members() {
    use crate::design::feature_project::path_pattern::project_partial_path_pattern;
    let earlier = super::pattern::seed_scope(5, Some(2));
    let mut selection = super::pattern::compact_entity_selection(SEED_GROUP, 326, 5);
    selection.scope_record_index = SCOPE;
    let groups = [
        group(SEED_GROUP, 1, DesignOperandRole::BODIES_A, &[326, 331]),
        group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
    ];
    let ordinals = std::collections::HashMap::from([((STREAM, SCOPE), 3), ((STREAM, 5), 1)]);
    let projected = project_partial_path_pattern(
        &scope(),
        &groups,
        &[],
        &[],
        &[selection],
        &[earlier.clone()],
        &ordinals,
    )
    .unwrap();
    assert_eq!(
        pattern(projected.definition).0,
        vec![
            PatternSeed::Feature(crate::ids::neutral_feature_id(&earlier)),
            PatternSeed::Native(format!("{STREAM}:design-record#331")),
        ]
    );
    assert!(projected
        .losses
        .iter()
        .any(|loss| loss.code
            == crate::loss::F3dLossCode::FeaturePathPatternFeatureSeedAssumed.kind()));
    assert!(
        projected
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::FeaturePathPatternSeedUnresolved.kind())
    );
}

#[test]
fn partial_path_pattern_retains_unclosed_candidates_and_range_losses() {
    use crate::design::feature_project::path_pattern::project_partial_path_pattern;
    let mut scoped = scope();
    scoped.unclosed_construction_operand_groups.push(SEED_GROUP);
    let huge = count(4_294_967_296.0);
    for source in [0.0, -2.0] {
        let source_distance = distance(source);
        let projected = project_partial_path_pattern(
            &scoped,
            &groups(DesignOperandRole::BODIES_B)[1..],
            &[(0, &huge), (1, &source_distance)],
            &[],
            &[],
            &[],
            &std::collections::HashMap::new(),
        )
        .unwrap();
        let (seeds, PatternTransform::CurveDrivenPartial { parameters }) =
            pattern(projected.definition)
        else {
            panic!("partial")
        };
        assert_eq!(
            seeds,
            vec![PatternSeed::Native(format!(
                "{STREAM}:design-record#{SEED_GROUP}"
            ))]
        );
        assert!(parameters.count.is_none() && parameters.distance.is_none());
        assert!(projected
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::FeaturePathPatternCountOutOfRange.kind()));
        assert!(projected.losses.iter().any(|loss| loss.code
            == crate::loss::F3dLossCode::FeaturePathPatternDistanceNonpositive.kind()));
    }
    let mut competing = scope();
    competing.unclosed_construction_operand_groups.push(331);
    let projected = project_partial_path_pattern(
        &competing,
        &groups(DesignOperandRole::BODIES_B),
        &[],
        &[],
        &[],
        &[],
        &std::collections::HashMap::new(),
    )
    .unwrap();
    assert_eq!(
        pattern(projected.definition).0,
        vec![
            PatternSeed::Native(format!(
                "{STREAM}:design-construction-operand-group#{SEED_GROUP}"
            )),
            PatternSeed::Native(format!("{STREAM}:design-record#331")),
        ]
    );
    assert!(
        projected
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::FeaturePathPatternSeedUnresolved.kind())
    );
}

#[test]
fn partial_path_pattern_keeps_face_evidence_and_ambiguous_members() {
    use crate::design::feature_project::path_pattern::project_partial_path_pattern;
    use crate::records::recipes::ConstructionRecipeKind;
    use crate::records::topology::{body_recipe::DesignOperandGroup, face::DesignFaceOperand};
    let mut face =
        super::pattern::seed_face_operand(ConstructionRecipeKind::BoundedFace).into_draft();
    face.id = format!("{STREAM}:face-operand#326");
    face.scope_record_index = SCOPE;
    face.group = Some(DesignOperandGroup {
        group_record_index: SEED_GROUP,
        group_member_ordinal: 0,
    });
    face.record_index = 326;
    face.recipe_record_index = 329;
    face.resolved_face_slots = vec![27];
    let face = DesignFaceOperand::try_new(face).unwrap();
    let mut scope = scope();
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups = groups(DesignOperandRole::BODIES_A);
    let earlier = super::pattern::seed_scope(5, Some(2));
    let mut selection = super::pattern::compact_entity_selection(SEED_GROUP, 326, 5);
    selection.scope_record_index = SCOPE;
    let ordinals = std::collections::HashMap::from([((STREAM, SCOPE), 3), ((STREAM, 5), 1)]);
    let face_only = project_partial_path_pattern(
        &scope,
        &groups,
        &[],
        std::slice::from_ref(&face),
        &[],
        &[],
        &ordinals,
    )
    .unwrap();
    assert!(matches!(
        pattern(face_only.definition).0.as_slice(),
        [PatternSeed::Faces(FaceSelection::Historical { .. })]
    ));
    let ambiguous = project_partial_path_pattern(
        &scope,
        &groups,
        &[],
        &[face],
        &[selection],
        &[earlier],
        &ordinals,
    )
    .unwrap();
    assert_eq!(
        pattern(ambiguous.definition).0,
        vec![PatternSeed::Native(format!("{STREAM}:design-record#326"))]
    );
    assert!(
        ambiguous
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::FeaturePathPatternSeedUnresolved.kind())
    );
}

#[test]
fn path_pattern_dispatch_retains_partial_data_for_unclosed_seed() {
    let mut scope = scope();
    scope.unclosed_construction_operand_groups.push(SEED_GROUP);
    let count = count(4.0);
    let distance = distance(3.0);
    let groups = groups(DesignOperandRole::BODIES_B);
    let inputs = crate::design::feature_project::ProjectInputs {
        construction_groups: &groups[1..],
        ..Default::default()
    };
    let mut losses = Vec::new();
    let definition = crate::design::feature_project::path_pattern::path_pattern_definition(
        &scope,
        &inputs,
        &[(0, &count), (1, &distance)],
        &std::collections::HashMap::new(),
        &mut losses,
    )
    .unwrap();
    let (seeds, PatternTransform::CurveDrivenPartial { parameters }) = pattern(definition) else {
        panic!("partial")
    };
    assert_eq!(
        seeds,
        vec![PatternSeed::Native(format!(
            "{STREAM}:design-record#{SEED_GROUP}"
        ))]
    );
    assert_eq!(parameters.count, Some(4));
    assert_eq!(
        parameters
            .distance
            .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(30.0)
    );
}

#[test]
fn partial_path_pattern_reports_missing_controls_and_count_one_nonpositive_distance() {
    use crate::design::feature_project::path_pattern::project_partial_path_pattern;
    let count = count(1.0);
    for value in [0.0, -2.0] {
        let distance = distance(value);
        let projected = project_partial_path_pattern(
            &scope_with(None),
            &groups(DesignOperandRole::BODIES_B),
            &[(0, &count), (1, &distance)],
            &[],
            &[],
            &[],
            &std::collections::HashMap::new(),
        )
        .unwrap();
        let (_, PatternTransform::CurveDrivenPartial { parameters }) =
            pattern(projected.definition)
        else {
            panic!("partial")
        };
        assert_eq!(parameters.count, Some(1));
        assert!(parameters.distance.is_none() && parameters.spacing.is_none());
        for code in [
            crate::loss::F3dLossCode::FeaturePathPatternDistanceNonpositive,
            crate::loss::F3dLossCode::FeaturePathPatternSettingsUnresolved,
            crate::loss::F3dLossCode::FeaturePathPatternStartUnresolved,
        ] {
            assert!(projected.losses.iter().any(|loss| loss.code == code.kind()));
        }
    }
}
