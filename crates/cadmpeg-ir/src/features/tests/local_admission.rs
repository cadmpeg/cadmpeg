use cadmpeg_test_support::edit;

use crate::features::FeatureOperation;
use crate::features::{
    BinderTarget, BodyMembers, BodySelection, CombineOperands, Feature, FeatureDefinition,
    FeatureId, FuzzyTolerance, GeometryImportPath, LoftPointSection, NonEmptyMembers, PathRef,
    PlanarProfileRef, ProfileRef, SectionOperands, SelectionMembers, SewBodySelection,
    SplitFacePlanes, ThreePointSelection, TreeChildren, TrimBodyOperands, VertexSelection,
};
use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalVertexId};

/// Scoped-byte limit that admits every membership index here. A two-member index is
/// eight buckets of 32-byte member slots with their control bytes, 295 bytes, and is
/// held twice while its table is filled; body members hold one such index while a
/// second fills, a peak of three. Reserving all of it afterwards shows the indexes
/// released.
const INDEX_ROOM: u64 = 3 * 295;

#[test]
fn invalid_membership_constructors_preserve_an_existing_refusal() {
    use crate::features::{
        BodySelectionError, EdgeSelection, FaceSelection, FeatureCollectionError, NativeSelections,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimit};

    fn refusal<T>(result: Result<Result<T, BodySelectionError>, ResourceLimit>) -> ResourceLimit {
        result.err().expect("original resource refusal")
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx
        .charge_work_limit(1, "original membership refusal")
        .unwrap_err();
    assert_eq!(
        refusal(SelectionMembers::<BodyId>::new(
            Vec::new(),
            &ctx,
            "empty selection"
        )),
        original
    );
    assert_eq!(
        refusal(NativeSelections::new(
            Vec::new(),
            &ctx,
            "empty native selection"
        )),
        original
    );
    assert_eq!(
        refusal(BodyMembers::<BodyId>::try_from_rows(Vec::new(), &ctx)),
        original
    );
    assert_eq!(
        refusal(EdgeSelection::generated(Vec::new(), String::new(), &ctx)),
        original
    );
    assert_eq!(
        refusal(FaceSelection::generated(Vec::new(), String::new(), &ctx)),
        original
    );
    assert_eq!(
        refusal(PlanarProfileRef::generated(Vec::new(), String::new(), &ctx)),
        original
    );
    assert_eq!(
        TreeChildren::new(Vec::new(), Some(feature_id("missing")), &ctx).unwrap_err(),
        FeatureCollectionError::Resource(original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn membership_constructors_preserve_refusals_and_release_scoped_indexes() {
    use crate::features::{
        DistinctMembers, FeatureContent, FeatureSourceContent, NativeSelections,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn retain_output<T>(ctx: &DecodeContext<'_>, output: T) -> Result<(), CodecError> {
        let storage = ctx.reserve_scoped_limit(INDEX_ROOM, "membership index released")?;
        drop(storage);
        drop(output);
        Ok(())
    }

    for owner in 0..5 {
        for dimension in [
            Some(ResourceDimension::MaterializedBytes),
            Some(ResourceDimension::CollectionItems),
            Some(ResourceDimension::WorkUnits),
            None,
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = INDEX_ROOM;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_recursion_depth = 0;
            policy.limits.max_collection_items = 8;
            match dimension {
                Some(ResourceDimension::MaterializedBytes) => {
                    policy.limits.max_materialized_bytes = 0;
                }
                Some(ResourceDimension::CollectionItems) => policy.limits.max_collection_items = 0,
                Some(ResourceDimension::WorkUnits) => policy.limits.max_work_units = 0,
                None => {}
                Some(_) => unreachable!(),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = (|| -> Result<(), CodecError> {
                match owner {
                    0 => retain_output(
                        &ctx,
                        DistinctMembers::try_from(vec![body_id("first"), body_id("second")], &ctx)?,
                    ),
                    1 => retain_output(
                        &ctx,
                        SelectionMembers::new(
                            vec![body_id("first"), body_id("second")],
                            &ctx,
                            "selection members",
                        )??,
                    ),
                    2 => retain_output(
                        &ctx,
                        NativeSelections::new(
                            vec!["first".into(), "second".into()],
                            &ctx,
                            "native members",
                        )??,
                    ),
                    3 => retain_output(
                        &ctx,
                        FeatureContent::new(
                            vec![
                                FeatureSourceContent::Feature(feature_id("first")),
                                FeatureSourceContent::Text("same".into()),
                                FeatureSourceContent::Text("same".into()),
                                FeatureSourceContent::Feature(feature_id("second")),
                            ],
                            &ctx,
                            "source content",
                        )?,
                    ),
                    4 => retain_output(
                        &ctx,
                        BodyMembers::try_from_rows(
                            vec![
                                crate::features::BodyMember::new(
                                    body_id("first"),
                                    cadmpeg_core::text::NonBlankString::try_from(
                                        "native-first".to_owned(),
                                    )
                                    .unwrap(),
                                ),
                                crate::features::BodyMember::new(
                                    body_id("second"),
                                    cadmpeg_core::text::NonBlankString::try_from(
                                        "native-second".to_owned(),
                                    )
                                    .unwrap(),
                                ),
                            ],
                            &ctx,
                        )??,
                    ),
                    _ => unreachable!(),
                }
            })();
            if let Some(dimension) = dimension {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("owner {owner}: resource refusal required");
                };
                assert_eq!(limit.dimension, dimension);
                assert_ne!(limit.operation, "membership index released");
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
                );
            } else {
                result.unwrap();
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn charged_native_selections_refuse_uniqueness_index_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::features::NativeSelections::new(
        vec!["first".into(), "second".into()],
        &ctx,
        "test native selection uniqueness",
    );
    assert!(matches!(result, Err(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "test native selection uniqueness"));
}

fn feature_id(suffix: &str) -> FeatureId {
    FeatureId::mint(format!("test:model:feature#{suffix}")).unwrap()
}

#[test]
fn local_and_generated_body_constructors_use_the_caller_session() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for generated in [false, true] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if generated {
                let member = crate::features::GeneratedBodyRef::new(
                    feature_id("producer"),
                    "body".into(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .unwrap();
                BodySelection::generated(vec![member], "native".into(), &ctx)
            } else {
                BodySelection::local(vec!["body".into()], "native".into(), &ctx)
            };
            let limit = result.unwrap_err();
            assert_eq!(limit.dimension, dimension);
            assert!(
                matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
            );
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let selection = if generated {
            let member = crate::features::GeneratedBodyRef::new(
                feature_id("producer"),
                "body".into(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .unwrap();
            BodySelection::generated(vec![member], "native".into(), &ctx)
        } else {
            BodySelection::local(vec!["body".into()], "native".into(), &ctx)
        }
        .unwrap()
        .unwrap();
        assert!(matches!(
            selection,
            BodySelection::Local { .. } | BodySelection::Generated { .. }
        ));
        ctx.finish_session().unwrap();
    }
}

fn body_id(suffix: &str) -> BodyId {
    BodyId::mint(format!("test:model:body#{suffix}")).unwrap()
}

#[test]
fn selection_reference_constructors_admit_text_before_validation() {
    use crate::features::{
        BodySelectionError, EdgeSelection, FaceSelection, GeneratedBodyRef, GeneratedCurveRef,
        GeneratedEdgeRef, GeneratedFaceRef, GeneratedVertexRef, SelectionReference,
    };
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit,
    };

    fn finish<T>(
        result: Result<Result<T, BodySelectionError>, ResourceLimit>,
    ) -> Result<Result<(), BodySelectionError>, ResourceLimit> {
        result.map(|result| result.map(|_| ()))
    }
    for owner in 0..12 {
        let construct = |ctx: &DecodeContext<'_>, text: String| {
            let reference = || "local".to_owned().try_into().unwrap();
            match owner {
                0 => finish(SelectionReference::new(text, ctx)),
                1 => finish(GeneratedBodyRef::new(feature_id("producer"), text, ctx)),
                2 => finish(GeneratedFaceRef::new(feature_id("producer"), text, ctx)),
                3 => finish(GeneratedEdgeRef::new(feature_id("producer"), text, ctx)),
                4 => finish(GeneratedVertexRef::new(feature_id("producer"), text, ctx)),
                5 => finish(GeneratedCurveRef::new(feature_id("producer"), text, ctx)),
                6 => finish(FaceSelection::generated(
                    vec![GeneratedFaceRef {
                        feature: feature_id("producer"),
                        local_id: reference(),
                    }],
                    text,
                    ctx,
                )),
                7 => finish(EdgeSelection::generated(
                    vec![GeneratedEdgeRef {
                        feature: feature_id("producer"),
                        local_id: reference(),
                    }],
                    text,
                    ctx,
                )),
                8 => finish(PlanarProfileRef::generated(
                    vec![GeneratedCurveRef {
                        feature: feature_id("producer"),
                        local_id: reference(),
                    }],
                    text,
                    ctx,
                )),
                9 => finish(VertexSelection::generated(
                    GeneratedVertexRef {
                        feature: feature_id("producer"),
                        local_id: reference(),
                    },
                    text,
                    ctx,
                )),
                10 => finish(VertexSelection::historical(
                    FeatureInputTopologyId::mint("test:model:feature-input#state").unwrap(),
                    HistoricalVertexId::mint("test:model:historical-vertex#one").unwrap(),
                    text,
                    ctx,
                )),
                11 => finish(VertexSelection::native(text, ctx)),
                _ => unreachable!(),
            }
        };
        let mut short_need = None;
        for text in ["  face  ".to_owned(), format!("  f{}", " ".repeat(4096))] {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                "selection reference complete",
                |cap| {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    policy.limits.max_materialized_bytes = 0;
                    policy.limits.max_retained_bytes = 0;
                    policy.limits.max_collection_items = 0;
                    policy.limits.max_recursion_depth = 0;
                    let arena = DecodeArena::new();
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    construct(&ctx, text.clone())?.unwrap();
                    ctx.charge_work(1, "selection reference complete")
                },
            );
            let cadmpeg_core::CodecError::ResourceLimit(complete) = error else {
                panic!("completion boundary");
            };
            let need = complete.used;
            if let Some(short_need) = short_need {
                assert_eq!(need, short_need, "validation stops at the first nonblank character");
            } else {
                short_need = Some(need);
            }
            for allowance in 0..=need.max(8) {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = allowance;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                policy.limits.max_recursion_depth = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = construct(&ctx, text.clone());
                if allowance < need {
                    let limit = result.unwrap_err();
                    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                    assert!(
                        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
                    );
                } else {
                    result.unwrap().unwrap();
                    ctx.finish_session().unwrap();
                }
            }
        }
    }
}

#[test]
fn feature_result_members_admit_each_arena_and_release_the_index() {
    use crate::features::{FeatureResultMemberError, FeatureResultMembers, FeatureResultTopology};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for lane in 0..4 {
        for dimension in [
            Some(ResourceDimension::MaterializedBytes),
            Some(ResourceDimension::CollectionItems),
            Some(ResourceDimension::WorkUnits),
            None,
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = INDEX_ROOM;
            policy.limits.max_collection_items = 2;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_recursion_depth = 0;
            match dimension {
                Some(ResourceDimension::MaterializedBytes) => {
                    policy.limits.max_materialized_bytes = 0;
                }
                Some(ResourceDimension::CollectionItems) => policy.limits.max_collection_items = 0,
                Some(ResourceDimension::WorkUnits) => policy.limits.max_work_units = 0,
                None => {}
                Some(_) => unreachable!(),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut lanes = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            lanes[lane] = vec![
                cadmpeg_core::nonblank_literal!("second"),
                cadmpeg_core::nonblank_literal!("first"),
            ];
            let [bodies, faces, edges, vertices] = lanes;
            let result = FeatureResultMembers::new(
                bodies,
                faces,
                edges,
                vertices,
                &ctx,
                "result membership",
            );
            if let Some(dimension) = dimension {
                let limit = result.unwrap_err();
                assert_eq!(limit.dimension, dimension);
                assert_eq!(limit.operation, "result membership");
                assert!(
                    matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
                );
            } else {
                let members = result.unwrap().unwrap();
                let result = FeatureResultTopology::new(
                    crate::ids::FeatureResultTopologyId::mint("test:model:feature-result#one")
                        .unwrap(),
                    feature_id("producer"),
                    members,
                    None,
                );
                assert_eq!(
                    [
                        result.bodies(),
                        result.faces(),
                        result.edges(),
                        result.vertices()
                    ][lane]
                        .iter()
                        .map(cadmpeg_core::text::NonBlankString::as_str)
                        .collect::<Vec<_>>(),
                    vec!["second", "first"]
                );
                let storage = ctx
                    .reserve_scoped_limit(INDEX_ROOM, "result membership index released")
                    .unwrap();
                drop(storage);
                ctx.finish_session().unwrap();
            }
        }
        let mut lanes = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        lanes[lane] = vec![
            cadmpeg_core::nonblank_literal!("same"),
            cadmpeg_core::nonblank_literal!("same"),
        ];
        let [bodies, faces, edges, vertices] = lanes;
        let error = FeatureResultMembers::new(
            bodies,
            faces,
            edges,
            vertices,
            &cadmpeg_test_support::service_decode_context(),
            "result membership",
        )
        .unwrap()
        .unwrap_err();
        assert_eq!(
            error,
            [
                FeatureResultMemberError::RepeatedBody,
                FeatureResultMemberError::RepeatedFace,
                FeatureResultMemberError::RepeatedEdge,
                FeatureResultMemberError::RepeatedVertex
            ][lane]
        );
    }
}

#[test]
fn split_face_planes_wire_preserves_arity_uniqueness_and_order() {
    let first = feature_id("first");
    let second = feature_id("second");
    for (planes, message) in [
        (Vec::new(), "planes must contain at least two planes"),
        (
            vec![first.clone()],
            "planes must contain at least two planes",
        ),
        (
            vec![first.clone(), first.clone()],
            "planes must be distinct",
        ),
    ] {
        assert_eq!(
            SplitFacePlanes::try_from(planes.clone()).unwrap_err(),
            message
        );
        let error =
            serde_json::from_value::<SplitFacePlanes>(serde_json::to_value(planes).unwrap())
                .unwrap_err()
                .to_string();
        assert!(error.contains(message), "{error}");
    }
    let planes = vec![second, first];
    let wire = serde_json::to_value(&planes).unwrap();
    let reconstructed = serde_json::from_value::<SplitFacePlanes>(wire.clone()).unwrap();
    assert_eq!(reconstructed.as_ref(), planes.as_slice());
    assert_eq!(serde_json::to_value(reconstructed).unwrap(), wire);
}

#[test]
fn local_collection_admission_preserves_order_and_rejects_invalid_membership() {
    let first = feature_id("first");
    let second = feature_id("second");
    assert!(NonEmptyMembers::<PathRef>::try_from(Vec::new()).is_err());
    assert!(SelectionMembers::<String>::try_from(vec!["mesh".into(), "mesh".into()]).is_err());
    assert!(SplitFacePlanes::try_from(vec![first.clone()]).is_err());
    assert!(SplitFacePlanes::try_from(vec![first.clone(), first.clone()]).is_err());
    let planes = SplitFacePlanes::try_from(vec![second.clone(), first.clone()]).unwrap();
    assert_eq!(
        serde_json::to_value(&planes).unwrap(),
        serde_json::json!([second, first])
    );
    assert!(TreeChildren::new(
        vec![first.clone(), first.clone()],
        None,
        &cadmpeg_test_support::service_decode_context()
    )
    .is_err());
    assert!(TreeChildren::new(
        vec![first.clone()],
        Some(second.clone()),
        &cadmpeg_test_support::service_decode_context()
    )
    .is_err());
    let mut children = TreeChildren::new(
        vec![first.clone()],
        Some(first),
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap();
    let before = children.clone();
    assert!({
        let active = Some(second.clone());
        edit::replace(&mut children, |previous| {
            crate::features::TreeChildren::new(
                previous.to_vec(),
                active,
                &cadmpeg_test_support::service_decode_context(),
            )
        })
    }
    .is_err());
    assert_eq!(children, before);
    children
        .insert(
            &cadmpeg_test_support::service_decode_context(),
            second.clone(),
            "insert fixture member",
        )
        .expect("member insertion admission");
    children
        .insert(
            &cadmpeg_test_support::service_decode_context(),
            second.clone(),
            "insert fixture member",
        )
        .expect("member insertion admission");
    assert_eq!(children.len(), 2);
    {
        let active = Some(second);
        edit::replace(&mut children, |previous| {
            crate::features::TreeChildren::new(
                previous.to_vec(),
                active,
                &cadmpeg_test_support::service_decode_context(),
            )
        })
    }
    .unwrap();
}

#[test]
fn tree_children_charged_insert_refuses_collection_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(b"children", &arena, &policy).unwrap();
    let mut children = TreeChildren::default();
    let error = children
        .insert(&ctx, feature_id("child"), "collect tree children")
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    assert!(children.is_empty());
}

#[test]
fn charged_selection_members_refuse_uniqueness_index_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        SelectionMembers::new(
            vec!["first", "second"], &ctx, "selection uniqueness",
        ),
        Err(failure)
            if failure.operation == "selection uniqueness"
                && failure.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn selection_owners_enforce_local_arity_and_atomic_nonoverlap() {
    let first = BodySelection::Bodies(
        crate::features::DistinctMembers::try_from(
            vec![body_id("first")],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct bodies"),
    );
    let second = BodySelection::Bodies(
        crate::features::DistinctMembers::try_from(
            vec![body_id("second")],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct bodies"),
    );
    let pair = BodySelection::Bodies(
        crate::features::DistinctMembers::try_from(
            vec![body_id("first"), body_id("second")],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct bodies"),
    );
    assert!(SewBodySelection::try_from(first.clone()).is_err());
    assert!(SewBodySelection::try_from(pair.clone()).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Unresolved).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Native("native".into())).is_ok());
    assert!(CombineOperands::new(
        pair,
        BodySelection::Unresolved,
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .is_err());
    assert!(CombineOperands::new(
        first.clone(),
        first.clone(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .is_err());
    assert!(SectionOperands::new(
        first.clone(),
        first.clone(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .is_err());
    assert!(TrimBodyOperands::new(
        first.clone(),
        first.clone(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .is_err());
    let mut operands = CombineOperands::new(
        first,
        second,
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .unwrap();
    let before = operands.clone();
    assert!(operands
        .try_edit(|first, second| *second = first.clone())
        .is_err());
    assert_eq!(operands, before);
}

#[test]
fn resolved_body_selection_wire_rejects_repeated_bodies_before_sew_arity() {
    let body = body_id("repeated");
    for wire in [
        serde_json::json!({"kind": "bodies", "value": [body.as_str(), body.as_str()]}),
        serde_json::json!({"kind": "resolved", "value": {
            "bodies": [body.as_str(), body.as_str()], "native": "native-selection"
        }}),
    ] {
        let error = serde_json::from_value::<BodySelection>(wire.clone())
            .expect_err("repeated resolved bodies")
            .to_string();
        assert!(error.contains("members must be distinct"), "{error}");
        let error = serde_json::from_value::<SewBodySelection>(wire)
            .expect_err("repeated bodies do not meet sew arity")
            .to_string();
        assert!(error.contains("members must be distinct"), "{error}");
    }
}

#[test]
fn feature_evaluation_rejects_duplicate_outputs_at_admission() {
    let body = body_id("output");
    let duplicate = vec![body.clone(), body.clone()];
    assert!(crate::features::DistinctMembers::try_from(
        duplicate.clone(),
        &cadmpeg_test_support::service_decode_context()
    )
    .is_err());

    let definition = FeatureDefinition::Operation(FeatureOperation::BaseFeature {
        bodies: BodySelection::Unresolved,
    });
    let mut evaluation = crate::features::FeatureEvaluation::new(
        definition.clone(),
        crate::features::DistinctMembers::try_from(
            vec![body.clone()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap(),
    );
    evaluation.edit(|_, outputs| {
        assert!(!outputs
            .insert(
                &cadmpeg_test_support::service_decode_context(),
                body.clone(),
                "insert fixture output"
            )
            .expect("output insertion admission"));
    });
    assert_eq!(evaluation.outputs(), &vec![body.clone()]);
    evaluation.set_outputs(
        crate::features::DistinctMembers::try_from(
            vec![body.clone()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap(),
    );
    let prior = evaluation.clone();
    assert!(crate::features::DistinctMembers::try_from(
        duplicate,
        &cadmpeg_test_support::service_decode_context()
    )
    .is_err());
    assert_eq!(evaluation, prior);
}

#[test]
fn feature_wire_rejects_duplicate_outputs_in_standalone_and_model_routes() {
    let body = body_id("output");
    let definition = FeatureDefinition::Operation(FeatureOperation::BaseFeature {
        bodies: BodySelection::Unresolved,
    });
    let wire = serde_json::json!({
        "id": feature_id("wire").as_str(),
        "ordinal": 0,
        "suppressed": null,
        "definition": serde_json::to_value(definition).unwrap(),
        "outputs": [body.as_str(), body.as_str()]
    });
    let error = serde_json::from_value::<Feature>(wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("distinct"), "{error}");

    let mut model = serde_json::to_value(crate::document::Model::default()).unwrap();
    model["features"] = serde_json::json!([{
        "id": feature_id("wire").as_str(),
        "ordinal": 0,
        "suppressed": null,
        "definition": serde_json::to_value(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature { bodies: BodySelection::Unresolved }
        )).unwrap(),
        "outputs": [body.as_str(), body.as_str()]
    }]);
    let error = serde_json::from_value::<crate::document::Model>(model)
        .unwrap_err()
        .to_string();
    assert!(error.contains("distinct"), "{error}");
}

#[test]
fn historical_body_overlap_spans_direct_and_paired_member_selections() {
    use crate::ids::{FeatureInputTopologyId, HistoricalBodyId};

    let state =
        FeatureInputTopologyId::mint("test:model:entity#test:input").expect("valid identity");
    let target = BodySelection::historical(
        state.clone(),
        vec![HistoricalBodyId::mint("test:body:4").expect("valid identity")],
        "target".into(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection storage is admitted")
    .unwrap();
    let overlapping = BodySelection::HistoricalSet {
        state: state.clone(),
        members: BodyMembers::try_from_rows(
            vec![
                crate::features::BodyMember::new(
                    HistoricalBodyId::mint("test:body:2").expect("valid identity"),
                    cadmpeg_core::text::NonBlankString::try_from("tool-a")
                        .expect("valid historical body selection row"),
                ),
                crate::features::BodyMember::new(
                    HistoricalBodyId::mint("test:body:4").expect("valid identity"),
                    cadmpeg_core::text::NonBlankString::try_from("tool-b")
                        .expect("valid historical body selection row"),
                ),
            ],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection storage is admitted")
        .expect("valid historical body selection rows"),
    };
    let disjoint = BodySelection::HistoricalSet {
        state,
        members: BodyMembers::try_from_rows(
            vec![crate::features::BodyMember::new(
                HistoricalBodyId::mint("test:body:5").expect("valid identity"),
                cadmpeg_core::text::NonBlankString::try_from("tool")
                    .expect("valid historical body selection row"),
            )],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection storage is admitted")
        .expect("valid historical body selection rows"),
    };

    assert!(SectionOperands::new(
        target.clone(),
        overlapping,
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .is_err());
    assert!(SectionOperands::new(
        target,
        disjoint,
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .is_ok());
}

#[test]
fn three_point_constructor_admits_each_target_and_state_comparison() {
    use crate::features::GeneratedVertexRef;
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    let producer = feature_id("producer");
    let state = FeatureInputTopologyId::mint("test:model:feature-input#state").unwrap();
    let historical = HistoricalVertexId::mint("test:model:historical-vertex#a").unwrap();
    for kind in 0..3 {
        let work = match kind {
            0 => 9,
            1 => 3 + 3 * (u64_from_index(producer.as_str().len()) + 3),
            2 => {
                3 + 5 * (u64_from_index(state.as_str().len()) + 1)
                    + 3 * (u64_from_index(historical.as_str().len()) + 1)
            }
            _ => unreachable!(),
        };
        for allowance in 0..=work {
            let points = Box::new(["a", "b", "c"].map(|local| {
                match kind {
                    0 => VertexSelection::native(
                        local.to_owned(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .unwrap()
                    .unwrap(),
                    1 => VertexSelection::generated(
                        GeneratedVertexRef {
                            feature: producer.clone(),
                            local_id: local.to_owned().try_into().unwrap(),
                        },
                        "native".into(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .unwrap()
                    .unwrap(),
                    2 => VertexSelection::historical(
                        state.clone(),
                        HistoricalVertexId::mint(format!("test:model:historical-vertex#{local}"))
                            .unwrap(),
                        "native".into(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .unwrap()
                    .unwrap(),
                    _ => unreachable!(),
                }
            }));
            let pointer = points.as_ptr();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = ThreePointSelection::new(points, &ctx);
            if allowance < work {
                let limit = result.unwrap_err();
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert!(
                    matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
                );
            } else {
                let points = result.unwrap().unwrap();
                assert_eq!(points.as_ptr(), pointer);
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn three_point_constructor_stops_at_a_duplicate_and_preserves_fused_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 5;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let selection = |text: String| {
        VertexSelection::native(text, &cadmpeg_test_support::service_decode_context())
            .unwrap()
            .unwrap()
    };
    let result = ThreePointSelection::new(
        Box::new([
            selection("same".into()),
            selection("same".into()),
            selection("long".repeat(1000)),
        ]),
        &ctx,
    )
    .unwrap();
    assert_eq!(
        result.unwrap_err(),
        "points must select three distinct vertex targets"
    );
    ctx.finish_session().unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx
        .charge_work_limit(1, "original three point refusal")
        .unwrap_err();
    let points = Box::new([
        VertexSelection::Unresolved,
        VertexSelection::Unresolved,
        VertexSelection::Unresolved,
    ]);
    assert_eq!(
        ThreePointSelection::new(points, &ctx).unwrap_err(),
        original
    );
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn three_point_admission_compares_targets_and_historical_states() {
    let state = FeatureInputTopologyId::mint("test:model:feature-input#first").unwrap();
    let other = FeatureInputTopologyId::mint("test:model:feature-input#second").unwrap();
    let vertex = |state: &FeatureInputTopologyId, suffix: &str, native: &str| {
        VertexSelection::historical(
            state.clone(),
            HistoricalVertexId::mint(format!("test:model:historical-vertex#{suffix}")).unwrap(),
            native.into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .unwrap()
    };
    assert!(ThreePointSelection::new(
        Box::new([
            vertex(&state, "a", "one"),
            vertex(&state, "a", "two"),
            vertex(&state, "b", "three"),
        ]),
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("three point admission")
    .is_err());
    assert!(ThreePointSelection::new(
        Box::new([
            vertex(&state, "a", "one"),
            vertex(&state, "b", "two"),
            vertex(&other, "c", "three"),
        ]),
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("three point admission")
    .is_err());
    let mixed = ThreePointSelection::new(
        Box::new([
            vertex(&state, "a", "one"),
            VertexSelection::native(
                "two".into(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .unwrap(),
            VertexSelection::Unresolved,
        ]),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("three point admission")
    .unwrap();
    assert_eq!(
        serde_json::from_value::<ThreePointSelection>(serde_json::to_value(&mixed).unwrap())
            .unwrap(),
        mixed
    );
}

#[test]
fn an_inserted_body_selection_does_not_restate_the_feature_outputs() {
    let body = body_id("inserted");
    let definition = FeatureDefinition::Operation(FeatureOperation::InsertBodies {
        bodies: crate::features::InsertedBodies::Resolved {
            native: "copied".into(),
        },
    });
    let mut feature = Feature {
        id: feature_id("insert"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    assert!(feature.evaluation.outputs().is_empty());
    feature.evaluation.set_outputs(
        crate::features::DistinctMembers::try_from(
            vec![body.clone()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap(),
    );
    let wire = serde_json::to_value(&feature).unwrap();
    assert!(wire.get("evaluation").is_none());
    assert_eq!(wire["outputs"], serde_json::json!([body.as_str()]));
    assert!(wire["definition"]["bodies"]["value"]
        .get("bodies")
        .is_none());
    assert_eq!(
        serde_json::from_value::<Feature>(wire.clone()).unwrap(),
        feature
    );

    let mut restated = wire;
    restated["definition"]["bodies"]["value"]["bodies"] = serde_json::json!([body.as_str()]);
    assert!(serde_json::from_value::<Feature>(restated.clone()).is_err());
    let error =
        serde_json::from_value::<crate::features::FeatureOperation>(restated["definition"].clone())
            .unwrap_err()
            .to_string();
    assert!(error.contains("bodies"), "{error}");
}

#[test]
fn local_feature_wire_rejects_empty_collections_and_invalid_strings() {
    for wire in [
        serde_json::json!({"definition":"mesh_import","tessellations":[]}),
        serde_json::json!({"definition":"mesh_import","tessellations":["same","same"]}),
        serde_json::json!({"definition":"fillet","groups":[]}),
        serde_json::json!({"definition":"chamfer","groups":[]}),
        serde_json::json!({"definition":"full_round_fillet","groups":[]}),
        serde_json::json!({"definition":"composite_curve","segments":[],"closed":false}),
        serde_json::json!({"definition":"boundary_fill","tools":{"kind":"unresolved"},"cells":[]}),
        serde_json::json!({"definition":"imported_geometry","path":"","format":"step"}),
        serde_json::json!({"definition":"imported_geometry","path":"a\u{0}b","format":"step"}),
    ] {
        assert!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    assert!(serde_json::from_value::<LoftPointSection>(
        serde_json::json!({"kind":"native_point","value":""})
    )
    .is_err());
    assert!(serde_json::from_value::<LoftPointSection>(
        serde_json::json!({"kind":"native_point","value":" p "})
    )
    .is_ok());
    assert!(GeometryImportPath::try_from(" ".to_owned()).is_ok());
    for wire in [
        serde_json::json!({"kind":"external","document":"","object":"object"}),
        serde_json::json!({"kind":"external","document":"document","object":""}),
        serde_json::json!({"kind":"native","reference":""}),
    ] {
        assert!(serde_json::from_value::<BinderTarget>(wire).is_err());
    }
}

#[test]
fn a_post_process_inside_a_post_process_is_refused_as_an_unknown_variant() {
    let spatial = ProfileRef::SpatialSketchProfiles {
        sketch: crate::sketches::SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap(),
        profiles: vec![0].try_into().unwrap(),
    };
    assert!(spatial.planar().is_none());
    assert!(
        serde_json::from_value::<PlanarProfileRef>(serde_json::to_value(&spatial).unwrap())
            .is_err()
    );

    let inner = FeatureDefinition::PostProcess {
        operation: FeatureOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        },
        refine: true,
        fuzzy_tolerance: FuzzyTolerance::KernelDefault,
    };
    let wire = serde_json::to_value(&inner).unwrap();
    assert_eq!(wire["definition"], "post_process");
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        inner
    );

    let mut nested = wire.clone();
    nested["operation"] = wire.clone();
    assert!(serde_json::from_value::<FeatureDefinition>(nested).is_err());
    let error = serde_json::from_value::<FeatureOperation>(wire)
        .expect_err("a post-processed operation carries no post-processing layer of its own");
    assert!(error.to_string().contains("unknown variant"), "{error}");
}

#[test]
fn local_wire_errors_name_the_rejected_field() {
    for (field, wire) in [
        (
            "tessellations",
            serde_json::json!({"definition":"mesh_import","tessellations":[]}),
        ),
        (
            "groups",
            serde_json::json!({"definition":"fillet","groups":[]}),
        ),
        (
            "groups",
            serde_json::json!({"definition":"chamfer","groups":[]}),
        ),
        (
            "groups",
            serde_json::json!({"definition":"full_round_fillet","groups":[]}),
        ),
        (
            "segments",
            serde_json::json!({"definition":"composite_curve","segments":[],"closed":false}),
        ),
        (
            "cells",
            serde_json::json!({"definition":"boundary_fill","tools":{"kind":"unresolved"},"cells":[]}),
        ),
        (
            "path",
            serde_json::json!({"definition":"imported_geometry","path":"","format":"step"}),
        ),
    ] {
        let error = serde_json::from_value::<FeatureOperation>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
    for (field, wire) in [
        (
            "document",
            serde_json::json!({"kind":"external","document":"","object":"object"}),
        ),
        (
            "object",
            serde_json::json!({"kind":"external","document":"document","object":""}),
        ),
        (
            "reference",
            serde_json::json!({"kind":"native","reference":""}),
        ),
    ] {
        let error = serde_json::from_value::<BinderTarget>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
}

#[test]
fn selection_operand_parts_move_retained_storage() {
    let target_text = String::from("target-native");
    let tools_text = String::from("tools-native");
    let target_pointer = target_text.as_ptr();
    let tools_pointer = tools_text.as_ptr();
    let operands = CombineOperands::new(
        BodySelection::Native(target_text),
        BodySelection::Native(tools_text),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .unwrap();
    let (target, tools) = operands.into_parts();
    let BodySelection::Native(target) = target else {
        panic!("native target");
    };
    let BodySelection::Native(tools) = tools else {
        panic!("native tools");
    };
    assert_eq!(target.as_ptr(), target_pointer);
    assert_eq!(tools.as_ptr(), tools_pointer);

    let targets_text = String::from("targets-native");
    let replacements_text = String::from("replacements-native");
    let targets_pointer = targets_text.as_ptr();
    let replacements_pointer = replacements_text.as_ptr();
    let operands = crate::features::ReplaceFaceOperands::new(
        crate::features::FaceSelection::Native(targets_text),
        crate::features::FaceSelection::Native(replacements_text),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("operand admission")
    .unwrap();
    let (targets, replacements) = operands.into_parts();
    let crate::features::FaceSelection::Native(targets) = targets else {
        panic!("native targets");
    };
    let crate::features::FaceSelection::Native(replacements) = replacements else {
        panic!("native replacements");
    };
    assert_eq!(targets.as_ptr(), targets_pointer);
    assert_eq!(replacements.as_ptr(), replacements_pointer);
}

#[test]
fn tree_children_wire_and_decode_share_membership_validation() {
    use crate::features::FeatureCollectionError;
    let first = feature_id("first");
    let second = feature_id("second");
    let ctx = cadmpeg_test_support::service_decode_context();
    for (children, active, message) in [
        (
            vec![first.clone(), first.clone()],
            Some(first.clone()),
            "members must be distinct",
        ),
        (
            vec![first.clone(), first.clone()],
            Some(second.clone()),
            "active_child must belong to children",
        ),
        (
            Vec::new(),
            Some(first.clone()),
            "active_child must belong to children",
        ),
    ] {
        let wire = serde_json::json!({"children": children, "active_child": active});
        let error = serde_json::from_value::<TreeChildren>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{error}");
        assert_eq!(
            TreeChildren::new(children, active, &ctx).unwrap_err(),
            FeatureCollectionError::Invalid(message)
        );
    }
    let children = TreeChildren::new(vec![second, first.clone()], Some(first), &ctx).unwrap();
    let wire = serde_json::to_value(&children).unwrap();
    assert_eq!(
        serde_json::from_value::<TreeChildren>(wire).unwrap(),
        children
    );
    ctx.finish_session().unwrap();
}

#[test]
fn tree_child_admission_preserves_first_and_later_active_comparison_refusals() {
    use crate::features::FeatureCollectionError;
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    let first = feature_id("left");
    let second = feature_id("next");
    for cap in [
        0,
        1,
        u64_from_index(first.as_str().len()) + 1,
        u64_from_index(first.as_str().len()) + 2,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(FeatureCollectionError::Resource(limit)) = TreeChildren::new(
            vec![first.clone(), second.clone()],
            Some(second.clone()),
            &ctx,
        ) else {
            panic!("active child lookup must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "validate active tree child");
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
        );
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let children = TreeChildren::new(
        vec![second.clone(), first.clone()],
        Some(first.clone()),
        &ctx,
    )
    .unwrap();
    assert_eq!(&children[..], &[second.clone(), first.clone()]);
    assert_eq!(children.active_child(), &Some(first.clone()));
    assert_eq!(
        TreeChildren::new(vec![first], Some(second), &ctx).unwrap_err(),
        FeatureCollectionError::Invalid("active_child must belong to children")
    );
    ctx.finish_session().unwrap();
}

#[test]
fn operand_constructors_preserve_each_overlap_refusal_in_the_caller_session() {
    use crate::features::edge_treatments::{FullRoundFilletGroup, FullRoundSideSelection};
    use crate::features::{FaceBlendOperands, FaceSelection, ReplaceFaceOperands};
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };

    let face = |name: &str| {
        FaceSelection::Faces(vec![crate::ids::FaceId::mint(format!(
            "test:model:face#{name}"
        ))
        .unwrap()])
    };
    let body = |name: &str| {
        BodySelection::Bodies(
            crate::features::DistinctMembers::try_from(
                vec![BodyId::mint(format!("test:model:body#{name}")).unwrap()],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        )
    };
    let first_face = face("first");
    let other_face = face("other");
    let third_face = face("third");
    let first_body = body("first");
    let other_body = body("other");
    let comparison_work = 3 + u64_from_index("test:model:face#first".len());
    for kind in 0..6 {
        let required = if kind == 5 {
            3 * comparison_work
        } else {
            comparison_work
        };
        for cap in 0..=required {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = match kind {
                0 => FaceBlendOperands::new(first_face.clone(), other_face.clone(), &ctx)
                    .map(|result| result.map(|_| ())),
                1 => ReplaceFaceOperands::new(first_face.clone(), other_face.clone(), &ctx)
                    .map(|result| result.map(|_| ())),
                2 => SectionOperands::new(first_body.clone(), other_body.clone(), &ctx)
                    .map(|result| result.map(|_| ())),
                3 => CombineOperands::new(first_body.clone(), other_body.clone(), &ctx)
                    .map(|result| result.map(|_| ())),
                4 => TrimBodyOperands::new(first_body.clone(), other_body.clone(), &ctx)
                    .map(|result| result.map(|_| ())),
                _ => FullRoundFilletGroup::new(
                    first_face.clone(),
                    FullRoundSideSelection::Explicit(other_face.clone()),
                    FullRoundSideSelection::Explicit(third_face.clone()),
                    &ctx,
                )
                .map(|result| result.map(|_| ())),
            };
            if cap == required {
                result
                    .expect("exact comparison admission")
                    .expect("disjoint operands");
                ctx.finish_session().expect("no storage or depth needed");
            } else {
                let limit = result.expect_err("every visit and comparison must be admitted");
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, "IR selection membership overlap");
                assert!(
                    matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
                );
            }
        }
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    FullRoundFilletGroup::new(
        first_face,
        FullRoundSideSelection::Automatic,
        FullRoundSideSelection::Automatic,
        &ctx,
    )
    .unwrap()
    .unwrap();
    ctx.finish_session()
        .expect("automatic sides have no membership comparisons");
}

#[test]
fn persistent_selection_reference_preserves_owned_text_and_wire() {
    use crate::features::SelectionReference;
    for text in [
        String::new(),
        " \t\r\n\u{2003}".into(),
        "\u{2003}x  ".into(),
        format!("  f{}", " ".repeat(1024)),
    ] {
        let expected = SelectionReference::try_from(text.clone());
        let pointer = text.as_ptr();
        let ctx = cadmpeg_test_support::service_decode_context();
        let result = SelectionReference::new(text.clone(), &ctx).unwrap();
        assert_eq!(result, expected);
        let moved = SelectionReference::new(text, &ctx).unwrap();
        if let Ok(value) = moved {
            assert_eq!(value.as_str().as_ptr(), pointer);
            assert_eq!(
                serde_json::to_value(&value).unwrap(),
                serde_json::Value::String(value.as_str().into())
            );
            assert_eq!(
                serde_json::from_value::<SelectionReference>(serde_json::to_value(&value).unwrap())
                    .unwrap(),
                value
            );
        }
        ctx.finish_session().unwrap();
    }
}
