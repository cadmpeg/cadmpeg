use cadmpeg_test_support::edit;

use crate::features::FeatureOperation;
use crate::features::{
    BinderTarget, BodyMembers, BodySelection, CombineOperands, Feature, FeatureDefinition,
    FeatureId, FuzzyTolerance, GeometryImportPath, LoftPointSection, NonEmptyMembers, PathRef,
    PlanarProfileRef, ProfileRef, SectionOperands, SelectionMembers, SewBodySelection,
    SplitFacePlanes, ThreePointSelection, TreeChildren, TrimBodyOperands, VertexSelection,
};
use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalVertexId};

fn feature_id(suffix: &str) -> FeatureId {
    FeatureId::mint(format!("test:model:feature#{suffix}")).unwrap()
}

fn body_id(suffix: &str) -> BodyId {
    BodyId::mint(format!("test:model:body#{suffix}")).unwrap()
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
    assert!(TreeChildren::new(vec![first.clone(), first.clone()], None,).is_err());
    assert!(TreeChildren::new(vec![first.clone()], Some(second.clone()),).is_err());
    let mut children = TreeChildren::new(vec![first.clone()], Some(first)).unwrap();
    let before = children.clone();
    assert!({
        let active = Some(second.clone());
        edit::replace(&mut children, |previous| {
            crate::features::TreeChildren::new(previous.to_vec(), active)
        })
    }
    .is_err());
    assert_eq!(children, before);
    children.insert(second.clone());
    children.insert(second.clone());
    assert_eq!(children.len(), 2);
    {
        let active = Some(second);
        edit::replace(&mut children, |previous| {
            crate::features::TreeChildren::new(previous.to_vec(), active)
        })
    }
    .unwrap();
}

#[test]
fn selection_owners_enforce_local_arity_and_atomic_nonoverlap() {
    let first = BodySelection::Bodies(
        crate::features::DistinctMembers::try_from(vec![body_id("first")])
            .expect("distinct bodies"),
    );
    let second = BodySelection::Bodies(
        crate::features::DistinctMembers::try_from(vec![body_id("second")])
            .expect("distinct bodies"),
    );
    let pair = BodySelection::Bodies(
        crate::features::DistinctMembers::try_from(vec![body_id("first"), body_id("second")])
            .expect("distinct bodies"),
    );
    assert!(SewBodySelection::try_from(first.clone()).is_err());
    assert!(SewBodySelection::try_from(pair.clone()).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Unresolved).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Native("native".into())).is_ok());
    assert!(CombineOperands::new(pair, BodySelection::Unresolved,).is_err());
    assert!(CombineOperands::new(first.clone(), first.clone(),).is_err());
    assert!(SectionOperands::new(first.clone(), first.clone(),).is_err());
    assert!(TrimBodyOperands::new(first.clone(), first.clone(),).is_err());
    let mut operands = CombineOperands::new(first, second).unwrap();
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
    assert!(crate::features::DistinctMembers::try_from(duplicate.clone(),).is_err());

    let definition = FeatureDefinition::Operation(FeatureOperation::BaseFeature {
        bodies: BodySelection::Unresolved,
    });
    let mut evaluation = crate::features::FeatureEvaluation::new(
        definition.clone(),
        crate::features::DistinctMembers::try_from(vec![body.clone()]).unwrap(),
    );
    evaluation.edit(|_, outputs| {
        assert!(!outputs.insert(body.clone(),));
    });
    assert_eq!(evaluation.outputs(), &vec![body.clone()]);
    evaluation.set_outputs(crate::features::DistinctMembers::try_from(vec![body.clone()]).unwrap());
    let prior = evaluation.clone();
    assert!(crate::features::DistinctMembers::try_from(duplicate,).is_err());
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
    )
    .unwrap();
    let overlapping = BodySelection::HistoricalSet {
        state: state.clone(),
        members: BodyMembers::try_from_rows(vec![
            crate::features::BodyMember::new(
                HistoricalBodyId::mint("test:body:2").expect("valid identity"),
                cadmpeg_core::text::NonBlankString::new("tool-a")
                    .expect("valid historical body selection row"),
            ),
            crate::features::BodyMember::new(
                HistoricalBodyId::mint("test:body:4").expect("valid identity"),
                cadmpeg_core::text::NonBlankString::new("tool-b")
                    .expect("valid historical body selection row"),
            ),
        ])
        .expect("valid historical body selection rows"),
    };
    let disjoint = BodySelection::HistoricalSet {
        state,
        members: BodyMembers::try_from_rows(vec![crate::features::BodyMember::new(
            HistoricalBodyId::mint("test:body:5").expect("valid identity"),
            cadmpeg_core::text::NonBlankString::new("tool")
                .expect("valid historical body selection row"),
        )])
        .expect("valid historical body selection rows"),
    };

    assert!(SectionOperands::new(target.clone(), overlapping,).is_err());
    assert!(SectionOperands::new(target, disjoint,).is_ok());
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
        )
        .unwrap()
    };
    assert!(ThreePointSelection::try_from(Box::new([
        vertex(&state, "a", "one"),
        vertex(&state, "a", "two"),
        vertex(&state, "b", "three"),
    ]),)
    .is_err());
    assert!(ThreePointSelection::try_from(Box::new([
        vertex(&state, "a", "one"),
        vertex(&state, "b", "two"),
        vertex(&other, "c", "three"),
    ]),)
    .is_err());
    let mixed = ThreePointSelection::try_from(Box::new([
        vertex(&state, "a", "one"),
        VertexSelection::native("two".into()).unwrap(),
        VertexSelection::Unresolved,
    ]))
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
    feature
        .evaluation
        .set_outputs(crate::features::DistinctMembers::try_from(vec![body.clone()]).unwrap());
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
    )
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
    )
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
            TreeChildren::new(children, active).unwrap_err(),
            FeatureCollectionError::Invalid(message)
        );
    }
    let children = TreeChildren::new(vec![second, first.clone()], Some(first)).unwrap();
    let wire = serde_json::to_value(&children).unwrap();
    assert_eq!(
        serde_json::from_value::<TreeChildren>(wire).unwrap(),
        children
    );
    ctx.finish_session().unwrap();
}

#[test]
fn feature_result_members_keep_lane_order_and_name_the_repeated_lane() {
    use crate::features::{FeatureResultMemberError, FeatureResultMembers, FeatureResultTopology};
    for lane in 0..4 {
        let mut lanes = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        lanes[lane] = vec![
            cadmpeg_core::nonblank_literal!("second"),
            cadmpeg_core::nonblank_literal!("first"),
        ];
        let [bodies, faces, edges, vertices] = lanes;
        let members = FeatureResultMembers::new(bodies, faces, edges, vertices).unwrap();
        let result = FeatureResultTopology::new(
            crate::ids::FeatureResultTopologyId::mint("test:model:feature-result#one").unwrap(),
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
        let mut lanes = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        lanes[lane] = vec![
            cadmpeg_core::nonblank_literal!("same"),
            cadmpeg_core::nonblank_literal!("same"),
        ];
        let [bodies, faces, edges, vertices] = lanes;
        let error = FeatureResultMembers::new(bodies, faces, edges, vertices).unwrap_err();
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
fn three_point_selection_moves_its_points_and_names_a_repeated_target() {
    use crate::features::GeneratedVertexRef;
    let producer = feature_id("producer");
    let state = FeatureInputTopologyId::mint("test:model:feature-input#state").unwrap();
    for kind in 0..3 {
        let points = Box::new(["a", "b", "c"].map(|local| {
            match kind {
                0 => VertexSelection::native(local.to_owned()).unwrap(),
                1 => VertexSelection::generated(
                    GeneratedVertexRef {
                        feature: producer.clone(),
                        local_id: local.to_owned().try_into().unwrap(),
                    },
                    "native".into(),
                )
                .unwrap(),
                _ => VertexSelection::historical(
                    state.clone(),
                    HistoricalVertexId::mint(format!("test:model:historical-vertex#{local}"))
                        .unwrap(),
                    "native".into(),
                )
                .unwrap(),
            }
        }));
        let pointer = points.as_ptr();
        let points = ThreePointSelection::try_from(points).unwrap();
        assert_eq!(points.as_ptr(), pointer);
    }
    let selection = |text: String| VertexSelection::native(text).unwrap();
    assert_eq!(
        ThreePointSelection::try_from(Box::new([
            selection("same".into()),
            selection("same".into()),
            selection("long".repeat(1000)),
        ]),)
        .unwrap_err(),
        "points must select three distinct vertex targets"
    );
}

#[test]
fn tree_children_keep_source_order_and_the_active_child() {
    use crate::features::FeatureCollectionError;
    let first = feature_id("left");
    let second = feature_id("next");
    let children =
        TreeChildren::new(vec![second.clone(), first.clone()], Some(first.clone())).unwrap();
    assert_eq!(&children[..], &[second.clone(), first.clone()]);
    assert_eq!(children.active_child(), &Some(first.clone()));
    assert_eq!(
        TreeChildren::new(vec![first], Some(second)).unwrap_err(),
        FeatureCollectionError::Invalid("active_child must belong to children")
    );
}

#[test]
fn selection_reference_constructors_keep_padded_text() {
    use crate::features::{
        BodySelectionError, EdgeSelection, FaceSelection, GeneratedBodyRef, GeneratedCurveRef,
        GeneratedEdgeRef, GeneratedFaceRef, GeneratedVertexRef, SelectionReference,
    };

    fn finish<T>(result: Result<T, BodySelectionError>) -> Result<(), BodySelectionError> {
        result.map(|_| ())
    }
    assert_eq!(
        SelectionReference::new("  face  ".to_owned())
            .unwrap()
            .as_str(),
        "  face  "
    );
    for owner in 0..12 {
        let text = "  face  ".to_owned();
        let reference = || "local".to_owned().try_into().unwrap();
        let result = match owner {
            0 => finish(SelectionReference::new(text)),
            1 => finish(GeneratedBodyRef::new(feature_id("producer"), text)),
            2 => finish(GeneratedFaceRef::new(feature_id("producer"), text)),
            3 => finish(GeneratedEdgeRef::new(feature_id("producer"), text)),
            4 => finish(GeneratedVertexRef::new(feature_id("producer"), text)),
            5 => finish(GeneratedCurveRef::new(feature_id("producer"), text)),
            6 => finish(FaceSelection::generated(
                vec![GeneratedFaceRef {
                    feature: feature_id("producer"),
                    local_id: reference(),
                }],
                text,
            )),
            7 => finish(EdgeSelection::generated(
                vec![GeneratedEdgeRef {
                    feature: feature_id("producer"),
                    local_id: reference(),
                }],
                text,
            )),
            8 => finish(PlanarProfileRef::generated(
                vec![GeneratedCurveRef {
                    feature: feature_id("producer"),
                    local_id: reference(),
                }],
                text,
            )),
            9 => finish(VertexSelection::generated(
                GeneratedVertexRef {
                    feature: feature_id("producer"),
                    local_id: reference(),
                },
                text,
            )),
            10 => finish(VertexSelection::historical(
                FeatureInputTopologyId::mint("test:model:feature-input#state").unwrap(),
                HistoricalVertexId::mint("test:model:historical-vertex#one").unwrap(),
                text,
            )),
            _ => finish(VertexSelection::native(text)),
        };
        result.unwrap();
    }
}
