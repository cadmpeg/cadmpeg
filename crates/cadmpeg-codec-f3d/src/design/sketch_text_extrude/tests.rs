// SPDX-License-Identifier: Apache-2.0

use super::{bind_sketch_text_extrude_profiles, SketchTextExtrudeCandidate, SketchTextPick};
use crate::loss::F3dLossCode;
use crate::records::sketch_geometry::SketchText;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::NonZeroLength;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use std::collections::BTreeMap;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const SCOPE: &str = "f3d:Design/BulkStream.dat:scope#12";

fn code(code: F3dLossCode) -> String {
    code.note("").code.local_code().to_owned()
}

fn sketch(name: &str) -> SketchId {
    SketchId::mint(format!("synthetic:test:id#f3d:sketch:{name}")).unwrap()
}

fn text(stream: &str, record_index: u32, owner: u32) -> SketchText {
    let mut text = crate::test_support::sketch_text::neutral_text_record();
    text.id = format!("{stream}:sketch-text#{record_index}");
    text.record_index = record_index;
    text.owner_reference = owner;
    text
}

fn entity(sketch_name: &str, text: &SketchText) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#f3d:text:{}", text.record_index)).unwrap(),
        sketch(sketch_name),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(text.id.clone()))
}

fn feature(definition: FeatureDefinition) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#f3d:feature:extrude").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: Some(SCOPE.to_owned()),
    }
}

fn whole_sketch_cut() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch("plate"))),
        direction: ExtrudeDirection::ReversedProfileNormal {},
        start: ExtrudeStart::default(),
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: NonZeroLength::new(0.2).unwrap(),
                },
                draft: None,
            },
        },
        op: BooleanOp::Cut,
        solid: Some(true),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    })
}

fn native_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "Extrude".into(),
        parameters: BTreeMap::new(),
    })
}

fn candidate(picks: &[(u64, u32)]) -> SketchTextExtrudeCandidate {
    SketchTextExtrudeCandidate {
        scope_id: SCOPE.to_owned(),
        picks: picks
            .iter()
            .map(|&(sketch_entity, text_record)| SketchTextPick {
                sketch_entity,
                text_record,
            })
            .collect(),
    }
}

fn bind(
    definition: FeatureDefinition,
    picks: &[(u64, u32)],
    texts: &[SketchText],
    entities: &[SketchEntity],
) -> (FeatureDefinition, Vec<String>) {
    let mut features = [feature(definition)];
    let losses =
        bind_sketch_text_extrude_profiles(&mut features, &[candidate(picks)], texts, entities);
    (
        features[0].evaluation.definition().clone(),
        losses
            .iter()
            .map(|loss| loss.code.local_code().to_owned())
            .collect(),
    )
}

fn profile(definition: &FeatureDefinition) -> &ProfileRef {
    let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = definition else {
        panic!("an Extrude definition");
    };
    profile
}

#[test]
fn text_picks_become_a_profile_of_their_text_entities_in_pick_order() {
    let texts = [text(STREAM, 900, 172), text(STREAM, 901, 172)];
    let entities = [entity("letters", &texts[0]), entity("letters", &texts[1])];
    let (definition, losses) = bind(
        whole_sketch_cut(),
        &[(172, 901), (172, 900)],
        &texts,
        &entities,
    );
    assert!(losses.is_empty());
    assert_eq!(
        profile(&definition),
        &ProfileRef::Planar(
            PlanarProfileRef::sketch_entities(
                sketch("letters"),
                vec![entities[1].id().clone(), entities[0].id().clone()],
            )
            .unwrap()
        )
    );
}

#[test]
fn picks_that_name_no_text_of_the_operand_sketch_keep_the_profile() {
    let cases = [
        ("owner", text(STREAM, 900, 173)),
        ("stream", text("f3d:Other/BulkStream.dat", 900, 172)),
        ("record", text(STREAM, 901, 172)),
    ];
    for (label, text) in cases {
        let entities = [entity("letters", &text)];
        let texts = [text];
        let (definition, losses) = bind(whole_sketch_cut(), &[(172, 900)], &texts, &entities);
        assert_eq!(definition, whole_sketch_cut(), "{label}");
        assert!(losses.is_empty(), "{label}");
        let (definition, losses) = bind(
            whole_sketch_cut(),
            &[(172, 900), (172, 900)],
            &texts,
            &entities,
        );
        assert_eq!(definition, whole_sketch_cut(), "{label}");
        assert_eq!(
            losses,
            [code(F3dLossCode::FeatureExtrudeProfileGroupNative)],
            "{label}"
        );
    }
}

#[test]
fn text_without_one_entity_on_one_sketch_leaves_a_native_profile() {
    let texts = [text(STREAM, 900, 172), text(STREAM, 901, 172)];
    let cases = [
        ("unprojected", vec![entity("letters", &texts[0])]),
        (
            "two sketches",
            vec![entity("letters", &texts[0]), entity("other", &texts[1])],
        ),
        (
            "duplicate",
            vec![
                entity("letters", &texts[0]),
                entity("letters", &texts[1]),
                entity("letters", &texts[1]),
            ],
        ),
    ];
    for (label, entities) in cases {
        let (definition, losses) = bind(
            whole_sketch_cut(),
            &[(172, 900), (172, 901)],
            &texts,
            &entities,
        );
        assert_eq!(
            profile(&definition),
            &ProfileRef::Planar(PlanarProfileRef::Native(SCOPE.to_owned())),
            "{label}"
        );
        assert_eq!(
            losses,
            [code(F3dLossCode::FeatureExtrudeSketchTextSkipped)],
            "{label}"
        );
    }
}

#[test]
fn a_native_text_extrude_keeps_its_definition_with_a_loss() {
    let texts = [text(STREAM, 900, 172)];
    let entities = [entity("letters", &texts[0])];
    let (definition, losses) = bind(native_definition(), &[(172, 900)], &texts, &entities);
    assert_eq!(definition, native_definition());
    assert_eq!(losses, [code(F3dLossCode::FeatureExtrudeSketchTextSkipped)]);
}
