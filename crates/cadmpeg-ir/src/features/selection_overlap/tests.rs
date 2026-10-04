// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

use crate::features::{
    BodyMember, BodyMembers, BodySelection, DistinctMembers, FaceSelection, FeatureId,
    GeneratedBodyRef, GeneratedFaceRef, NativeSelections, NonEmptyMembers, SelectionMembers,
};
use crate::ids::{BodyId, FaceId, FeatureInputTopologyId, HistoricalBodyId, HistoricalFaceId};

fn state(name: &str) -> FeatureInputTopologyId {
    FeatureInputTopologyId::mint(format!("test:model:state#{name}")).expect("state identity")
}

fn faces(name: &str) -> FaceSelection {
    FaceSelection::Faces(vec![
        FaceId::mint(format!("test:model:face#{name}")).expect("face identity")
    ])
}

fn bodies(name: &str) -> BodySelection {
    BodySelection::Bodies(DistinctMembers(vec![BodyId::mint(format!(
        "test:model:body#{name}"
    ))
    .expect("body identity")]))
}

#[test]
fn selection_overlap_preserves_each_visit_and_identity_comparison_refusal() {
    let first_face = faces("same");
    let first_body = bodies("same");
    for body in [false, true] {
        let identity_len = if body {
            "test:model:body#same".len()
        } else {
            "test:model:face#same".len()
        };
        let required = 3 + u64_from_index(identity_len);
        for cap in 0..required {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = if body {
                super::body_selections_overlap(&ctx, &first_body, &first_body)
            } else {
                super::face_selections_overlap(&ctx, &first_face, &first_face)
            };
            let limit = result.expect_err("every visit and identity comparison requires admission");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR selection membership overlap");
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = required;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = if body {
            super::body_selections_overlap(&ctx, &first_body, &first_body)
        } else {
            super::face_selections_overlap(&ctx, &first_face, &first_face)
        };
        assert!(result.expect("exact comparison cap"));
        ctx.finish_session().expect("no storage or depth needed");
    }
}

#[test]
fn selection_overlap_preserves_every_membership_form() {
    let native =
        || cadmpeg_core::text::NonBlankString::new("native").expect("nonblank native selection");
    let historical_face = |partial, other_state| {
        let id = HistoricalFaceId::mint("test:model:face#historical").expect("historical face");
        let state = state(if other_state { "other" } else { "same" });
        if partial {
            FaceSelection::HistoricalPartial {
                state,
                faces: DistinctMembers(vec![id]),
                unresolved: NativeSelections(vec!["unresolved".into()]),
                native: native(),
            }
        } else {
            FaceSelection::Historical {
                state,
                faces: SelectionMembers(vec![id]),
                native: native(),
            }
        }
    };
    let generated_face = |feature: &str, local: &str| FaceSelection::Generated {
        faces: NonEmptyMembers(vec![GeneratedFaceRef {
            feature: FeatureId::mint(format!("test:model:feature#{feature}"))
                .expect("feature identity"),
            local_id: local.to_owned().try_into().expect("local identity"),
        }]),
        native: "native".to_owned().try_into().expect("native selection"),
    };
    let same_face = FaceId::mint("test:model:face#same").expect("face identity");
    let face_cases = [
        (
            faces("same"),
            FaceSelection::Resolved {
                faces: vec![same_face],
                native: "native".into(),
            },
            true,
        ),
        (faces("first"), faces("other"), false),
        (
            historical_face(false, false),
            historical_face(true, false),
            true,
        ),
        (
            historical_face(false, false),
            historical_face(true, true),
            false,
        ),
        (
            generated_face("same", "same"),
            generated_face("same", "same"),
            true,
        ),
        (
            generated_face("same", "same"),
            generated_face("other", "same"),
            false,
        ),
        (
            generated_face("same", "same"),
            generated_face("same", "other"),
            false,
        ),
        (
            FaceSelection::Native("same".into()),
            FaceSelection::Native("same".into()),
            false,
        ),
    ];
    let paired_body = |historical, other_state| {
        if historical {
            BodySelection::HistoricalSet {
                state: state(if other_state { "other" } else { "same" }),
                members: BodyMembers(vec![BodyMember::new(
                    HistoricalBodyId::mint("test:model:body#historical").expect("historical body"),
                    native(),
                )]),
            }
        } else {
            BodySelection::ResolvedSet {
                members: BodyMembers(vec![BodyMember::new(
                    BodyId::mint("test:model:body#same").expect("body identity"),
                    native(),
                )]),
            }
        }
    };
    let historical_body = || BodySelection::Historical {
        state: state("same"),
        bodies: SelectionMembers(vec![
            HistoricalBodyId::mint("test:model:body#historical").expect("historical body")
        ]),
        native: "native".to_owned().try_into().expect("native selection"),
    };
    let generated_body = |feature: &str, local: &str| BodySelection::Generated {
        bodies: SelectionMembers(vec![GeneratedBodyRef {
            feature: FeatureId::mint(format!("test:model:feature#{feature}"))
                .expect("feature identity"),
            local_id: local.to_owned().try_into().expect("local identity"),
        }]),
        native: "native".to_owned().try_into().expect("native selection"),
    };
    let local_body = |name: &str| BodySelection::Local {
        bodies: NativeSelections(vec![name.into()]),
        native: "native".to_owned().try_into().expect("native selection"),
    };
    let body_cases = [
        (bodies("same"), paired_body(false, false), true),
        (paired_body(false, false), bodies("same"), true),
        (paired_body(false, false), paired_body(false, false), true),
        (bodies("first"), bodies("other"), false),
        (historical_body(), paired_body(true, false), true),
        (paired_body(true, false), historical_body(), true),
        (historical_body(), paired_body(true, true), false),
        (
            generated_body("same", "same"),
            generated_body("same", "same"),
            true,
        ),
        (
            generated_body("same", "same"),
            generated_body("other", "same"),
            false,
        ),
        (
            generated_body("same", "same"),
            generated_body("same", "other"),
            false,
        ),
        (local_body("same"), local_body("same"), true),
        (local_body("same"), local_body("other"), false),
        (bodies("same"), historical_body(), false),
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for (first, second, expected) in face_cases {
        assert_eq!(
            super::face_selections_overlap(&ctx, &first, &second).expect("decode comparisons"),
            expected
        );
        assert_eq!(
            super::standard_result(super::face_selections_overlap(
                &super::StandardAdmission,
                &first,
                &second
            )),
            expected
        );
    }
    for (first, second, expected) in body_cases {
        assert_eq!(
            super::body_selections_overlap(&ctx, &first, &second).expect("decode comparisons"),
            expected
        );
        assert_eq!(
            super::standard_result(super::body_selections_overlap(
                &super::StandardAdmission,
                &first,
                &second
            )),
            expected
        );
    }
    ctx.finish_session().expect("only comparison work");
}

#[test]
fn selection_overlap_stops_at_the_first_shared_member() {
    let first_id = FaceId::mint("test:model:face#first").expect("face identity");
    let first = FaceSelection::Faces(vec![
        first_id.clone(),
        FaceId::mint("test:model:face#second").expect("face identity"),
    ]);
    let second = FaceSelection::Faces(vec![
        first_id.clone(),
        FaceId::mint("test:model:face#third").expect("face identity"),
    ]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3 + u64_from_index(first_id.as_str().len());
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        super::face_selections_overlap(&ctx, &first, &second).expect("first comparison is enough")
    );
    ctx.finish_session().expect("unvisited members use no work");
}
