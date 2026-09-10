use crate::features::*;
use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalVertexId};
use crate::scalar::{InteriorAngle, PositiveLength};

fn feature_id(suffix: &str) -> FeatureId {
    FeatureId::mint(format!("test:model:feature#{suffix}")).unwrap()
}

fn body_id(suffix: &str) -> BodyId {
    BodyId::mint(format!("test:model:body#{suffix}")).unwrap()
}

fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
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
    assert!(TreeChildren::new(vec![first.clone(), first.clone()], None).is_err());
    assert!(TreeChildren::new(vec![first.clone()], Some(second.clone())).is_err());
    let mut children = TreeChildren::new(vec![first.clone()], Some(first)).unwrap();
    let before = children.clone();
    assert!(children.set_active_child(Some(second.clone())).is_err());
    assert_eq!(children, before);
    children.insert(second.clone());
    children.insert(second.clone());
    assert_eq!(children.len(), 2);
    children.set_active_child(Some(second)).unwrap();
}

#[test]
fn selection_owners_enforce_local_arity_and_atomic_nonoverlap() {
    let first = BodySelection::Bodies(vec![body_id("first")]);
    let second = BodySelection::Bodies(vec![body_id("second")]);
    let pair = BodySelection::Bodies(vec![body_id("first"), body_id("second")]);
    assert!(SewBodySelection::try_from(first.clone()).is_err());
    assert!(SewBodySelection::try_from(pair.clone()).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Unresolved).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Native("native".into())).is_ok());
    assert!(CombineOperands::new(pair, BodySelection::Unresolved).is_err());
    assert!(CombineOperands::new(first.clone(), first.clone()).is_err());
    assert!(SectionOperands::new(first.clone(), first.clone()).is_err());
    assert!(TrimBodyOperands::new(first.clone(), first.clone()).is_err());
    let mut operands = CombineOperands::new(first, second).unwrap();
    let before = operands.clone();
    assert!(operands
        .try_edit(|first, second| *second = first.clone())
        .is_err());
    assert_eq!(operands, before);
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
    ]))
    .is_err());
    assert!(ThreePointSelection::try_from(Box::new([
        vertex(&state, "a", "one"),
        vertex(&state, "b", "two"),
        vertex(&other, "c", "three"),
    ]))
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
fn hole_and_sweep_edits_preserve_the_previous_admitted_shape() {
    assert!(CounterdrillDiameters::new(positive(4.0), Some(positive(4.0))).is_err());
    let treatment = HoleConstruction::form(HoleKind::Counterbore {
        diameter: positive(4.0),
        depth: positive(2.0),
    });
    assert!(HoleShape::new(treatment.clone(), None, None).is_err());
    assert!(HoleShape::new(treatment.clone(), None, Some(positive(4.0))).is_err());
    let mut hole = HoleShape::new(treatment, None, Some(positive(2.0))).unwrap();
    let before = hole.clone();
    assert!(hole
        .try_edit(|_, _, diameter| *diameter = Some(positive(5.0)))
        .is_err());
    assert_eq!(hole, before);
    assert!(HoleShape::new(
        HoleConstruction::form(HoleKind::Simple),
        Some(HoleKind::Countersink {
            diameter: positive(2.0),
            angle: InteriorAngle::new(1.0).unwrap(),
        }),
        Some(positive(2.0))
    )
    .is_err());
    assert!(HoleShape::new(
        HoleConstruction::form(HoleKind::PartialCounterbore(
            crate::features::PartialPair::First(positive(1.0)),
        )),
        None,
        Some(positive(2.0))
    )
    .is_ok());
    let circular = SweepSection::Generated(GeneratedSweepSection::CircularRegion {
        region: SweepCircularRegion::new(positive(2.0), Some(positive(1.0))).unwrap(),
    });
    assert!(SweepShape::new(
        SweepSection::Unresolved(None),
        vec![circular.clone()],
        SweepMode::Unresolved
    )
    .is_err());
    let mut sweep = SweepShape::new(
        SweepSection::Unresolved(None),
        vec![circular],
        SweepMode::NewBody,
    )
    .unwrap();
    let before = sweep.clone();
    assert!(sweep
        .try_edit(|_, _, mode| *mode = SweepMode::Surface)
        .is_err());
    assert_eq!(sweep, before);
}

#[test]
fn feature_evaluation_admits_matching_insert_outputs_on_every_wire_door() {
    let body = body_id("inserted");
    let definition = FeatureDefinition::InsertBodies {
        bodies: BodySelection::Resolved {
            bodies: vec![body.clone()],
            native: "copied".into(),
        },
    };
    assert!(FeatureEvaluation::new(definition.clone(), Vec::new()).is_err());
    let mut evaluation = FeatureEvaluation::new(definition.clone(), vec![body.clone()]).unwrap();
    let before = evaluation.clone();
    assert!(evaluation.try_edit(|_, outputs| outputs.clear()).is_err());
    assert_eq!(evaluation, before);
    let feature = Feature::new(feature_id("insert"), 0, definition);
    assert_eq!(feature.evaluation.outputs(), &[body]);
    let wire = serde_json::to_value(&feature).unwrap();
    assert!(wire.get("evaluation").is_none());
    assert_eq!(
        serde_json::from_value::<Feature>(wire.clone()).unwrap(),
        feature
    );
    let mut invalid = wire;
    invalid["outputs"] = serde_json::json!([]);
    assert!(serde_json::from_value::<Feature>(invalid.clone()).is_err());
    let mut ir = crate::CadIr::empty();
    ir.model.features.push(feature);
    let mut document = serde_json::to_value(ir).unwrap();
    document["model"]["features"][0] = invalid;
    assert!(serde_json::from_value::<crate::CadIr>(document).is_err());
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
        serde_json::json!({"kind":"native_point","value":" "})
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
fn planar_profiles_and_post_processing_reject_invalid_edits() {
    let spatial = ProfileRef::SpatialSketchProfiles {
        sketch: crate::sketches::SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap(),
        profiles: vec![0].try_into().unwrap(),
    };
    assert!(PlanarProfileRef::try_from(spatial.clone()).is_err());
    assert!(
        serde_json::from_value::<PlanarProfileRef>(serde_json::to_value(&spatial).unwrap())
            .is_err()
    );
    let mut planar = PlanarProfileRef::native("profile".into());
    let before = planar.clone();
    assert!(planar
        .try_edit(|profile| *profile = spatial.clone())
        .is_err());
    assert_eq!(planar, before);
    let loft = FeatureDefinition::Loft {
        sections: vec![LoftSection::Profile(spatial)],
        guidance: LoftGuidance::default(),
        op: BooleanOp::NewBody,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
        closed: false,
        solid: true,
    };
    assert!(UnprocessedFeature::try_from(loft).is_err());
    let operation = FeatureDefinition::BaseFeature {
        bodies: BodySelection::Unresolved,
    };
    let mut unprocessed = UnprocessedFeature::try_from(operation).unwrap();
    let nested = FeatureDefinition::PostProcess {
        operation: unprocessed.clone(),
        refine: true,
        fuzzy_tolerance: FuzzyTolerance::KernelDefault,
    };
    assert!(UnprocessedFeature::try_from(nested.clone()).is_err());
    assert!(
        serde_json::from_value::<UnprocessedFeature>(serde_json::to_value(&nested).unwrap())
            .is_err()
    );
    let before = unprocessed.clone();
    assert!(unprocessed
        .try_edit(|operation| *operation = nested)
        .is_err());
    assert_eq!(unprocessed, before);
}

#[test]
fn face_operand_and_full_round_owners_reject_each_overlap() {
    let face = |name: &str| {
        FaceSelection::Faces(vec![crate::ids::FaceId::mint(format!(
            "test:model:face#{name}"
        ))
        .unwrap()])
    };
    let center = face("center");
    let first = face("first");
    let second = face("second");
    assert!(FaceBlendOperands::new(center.clone(), center.clone()).is_err());
    assert!(ReplaceFaceOperands::new(center.clone(), center.clone()).is_err());
    for (one, two) in [
        (center.clone(), second.clone()),
        (first.clone(), center.clone()),
        (first.clone(), first.clone()),
    ] {
        assert!(FullRoundFilletGroup::new(
            center.clone(),
            FullRoundSideSelection::Explicit(one),
            FullRoundSideSelection::Explicit(two)
        )
        .is_err());
    }
    let mut group = FullRoundFilletGroup::new(
        center,
        FullRoundSideSelection::Explicit(first),
        FullRoundSideSelection::Explicit(second),
    )
    .unwrap();
    let before = group.clone();
    assert!(group
        .try_edit(|center, one, _| *one = FullRoundSideSelection::Explicit(center.clone()))
        .is_err());
    assert_eq!(group, before);
    assert!(SewBodySelection::try_from(BodySelection::NativeSet(
        vec!["single".into()].try_into().unwrap()
    ))
    .is_ok());
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
        let error = serde_json::from_value::<FeatureDefinition>(wire).unwrap_err();
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
