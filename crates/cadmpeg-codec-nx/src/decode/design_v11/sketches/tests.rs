// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::feature::{FeatureOperation as DesignOperation, OperationFamily};
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureContent, FeatureDefinition, SketchFeatureBinding};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchEntityId, SketchGeometry, SketchPlacement, SketchProfiles,
};

use super::*;

const SKETCH: &str = "nx:feature-history:sketch#1";

fn sketch_id() -> SketchId {
    SketchId::mint(SKETCH).unwrap()
}

fn feature(key: &str, ordinal: u64, binding: SketchFeatureBinding) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(format!("nx:feature-history:feature#{key}")).unwrap(),
        ordinal,
        name: Some("SKETCH".to_owned()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: Some("SKETCH".to_owned()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch { sketch: binding },
        )),
        native_ref: None,
    }
}

fn owning(key: &str, ordinal: u64) -> LegacyFeature {
    feature(
        key,
        ordinal,
        SketchFeatureBinding::Planar(Some(sketch_id())),
    )
}

fn model_sketch(placement: SketchPlacement) -> LegacySketch {
    LegacySketch {
        id: sketch_id(),
        name: Some("SKETCH_000".to_owned()),
        configuration: None,
        visible: None,
        placement,
        profiles: SketchProfiles::default(),
        native_ref: Some("nx:feature-history:sketch-record#1".to_owned()),
    }
}

fn point(key: &str, kind: &str) -> LegacyEntity {
    let mut entity = LegacyEntity::new(
        SketchEntityId::mint(format!("nx:feature-history:sketch-entity#{key}")).unwrap(),
        sketch_id(),
        SketchGeometry::native(NonBlankString::new(kind.to_owned()).unwrap()),
    );
    entity.native_ref = Some(format!("nx:feature-history:point#{key}"));
    entity
}

fn constraint() -> SketchConstraint {
    SketchConstraint {
        id: SketchConstraintId::mint("nx:feature-history:sketch-constraint#1").unwrap(),
        sketch: sketch_id(),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Disabled {},
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    }
}

/// Projects the design of `ir` and returns the losses it charges.
fn project_ir(ir: &mut CadIr) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let (definition, _) = super::super::root_definition(ir).unwrap();
    super::super::project(ir, definition, &mut losses).unwrap();
    losses
}

fn unprojected(losses: &[LossNote]) -> Vec<&str> {
    losses
        .iter()
        .filter(|loss| loss.code == NxLossCode::SketchDesignUnprojected.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

fn design_findings(ir: &CadIr) -> Vec<String> {
    cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn the_feature_that_holds_a_sketch_states_it_with_its_native_points() {
    let mut ir = CadIr::empty();
    ir.model.features = vec![owning("a", 0)];
    ir.model.sketches = vec![model_sketch(SketchPlacement::Unresolved {})];
    let mut fixed = point("p1", "nx-fixed-point");
    fixed.construction = true;
    ir.model.sketch_entities = vec![point("g0", "nx-point-group"), fixed];
    let losses = project_ir(&mut ir);

    assert_eq!(
        ir.design.features[0].definition.operation,
        DesignOperation::Sketch(sketch_id())
    );
    let [sketch] = ir.design.sketches.as_slice() else {
        panic!("one design sketch");
    };
    assert_eq!(sketch.owner.as_str(), "nx:feature-history:feature#a");
    assert_eq!(sketch.name.as_deref(), Some("SKETCH_000"));
    assert_eq!(
        sketch.native_ref.as_deref(),
        Some("nx:feature-history:sketch-record#1")
    );
    let SketchPlane::Placed(plane) = &sketch.plane else {
        panic!("a placed plane");
    };
    assert_eq!(plane.resolved, None);
    let [ConstructionInput {
        role: None,
        reference: Some(InputReference::Picks(selection)),
        value: None,
        reversed: None,
    }] = plane.inputs.as_slice()
    else {
        panic!("one unread support input");
    };
    assert_eq!(selection.at, Anchor::Input);
    assert_eq!(selection.picks, Picks::Unread);

    let entities = ir
        .design
        .sketch_entities
        .iter()
        .map(|entity| {
            let EntityDefinition::Defined { form, solved: None } = &entity.definition else {
                panic!("a defined entity with no solved curve");
            };
            let EntityForm::Native(kind) = form.as_ref() else {
                panic!("a native entity");
            };
            (
                entity.id.as_str(),
                entity.sketch.as_str(),
                kind.as_str(),
                entity.construction,
                entity.native_ref.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entities,
        [
            (
                "nx:feature-history:sketch-entity#g0",
                SKETCH,
                "nx:nx-point-group",
                None,
                Some("nx:feature-history:point#g0")
            ),
            (
                "nx:feature-history:sketch-entity#p1",
                SKETCH,
                "nx:nx-fixed-point",
                Some(true),
                Some("nx:feature-history:point#p1")
            ),
        ]
    );
    assert_eq!(unprojected(&losses), Vec::<&str>::new());
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn an_entity_with_a_planar_curve_states_the_curve() {
    let mut ir = CadIr::empty();
    ir.model.features = vec![owning("a", 0)];
    ir.model.sketches = vec![model_sketch(SketchPlacement::Unresolved {})];
    let geometry = json::from_value::<SketchGeometry>(cadmpeg_core::json!({"kind": "line",
        "start": {"u": 0.0, "v": 0.0}, "end": {"u": 2.0, "v": 0.0}}))
    .unwrap();
    ir.model.sketch_entities = vec![LegacyEntity::new(
        SketchEntityId::mint("nx:feature-history:sketch-entity#l0").unwrap(),
        sketch_id(),
        geometry,
    )];
    let losses = project_ir(&mut ir);
    let [entity] = ir.design.sketch_entities.as_slice() else {
        panic!("one design entity");
    };
    let EntityDefinition::Free(PlanarCurve::Line { start, end }) = &entity.definition else {
        panic!("a free line: {:?}", entity.definition);
    };
    assert_eq!([start.get().u, end.get().u], [0.0, 2.0]);
    assert_eq!(unprojected(&losses), Vec::<&str>::new());
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn a_resolved_placement_is_the_plane_frame() {
    let mut ir = CadIr::empty();
    ir.model.features = vec![owning("a", 0)];
    ir.model.sketches = vec![model_sketch(
        SketchPlacement::try_resolved(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 2.0),
            Vector3::new(3.0, 0.0, 0.0),
        )
        .unwrap(),
    )];
    project_ir(&mut ir);
    let SketchPlane::Placed(plane) = &ir.design.sketches[0].plane else {
        panic!("a placed plane");
    };
    assert!(plane.inputs.is_empty());
    let frame = plane.resolved.as_ref().expect("a resolved frame");
    assert_eq!(frame.origin.get(), Point3::new(1.0, 2.0, 3.0));
    assert_eq!(Vector3::from(frame.x_axis), Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(Vector3::from(frame.y_axis), Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(Vector3::from(frame.z_axis), Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn a_later_feature_naming_a_held_sketch_keeps_its_family() {
    let mut ir = CadIr::empty();
    ir.model.features = vec![owning("b", 1), owning("a", 0)];
    ir.model.sketches = vec![model_sketch(SketchPlacement::Unresolved {})];
    let losses = project_ir(&mut ir);
    let operations = ir
        .design
        .features
        .iter()
        .map(|feature| (feature.id.as_str(), &feature.definition.operation))
        .collect::<Vec<_>>();
    assert_eq!(
        operations,
        [
            (
                "nx:feature-history:feature#a",
                &DesignOperation::Sketch(sketch_id())
            ),
            (
                "nx:feature-history:feature#b",
                &DesignOperation::Unresolved {
                    family: OperationFamily::Sketch
                }
            ),
        ]
    );
    assert_eq!(
        ir.design.sketches[0].owner.as_str(),
        "nx:feature-history:feature#a"
    );
    assert_eq!(
        unprojected(&losses),
        ["NX sketch feature nx:feature-history:feature#b names sketch nx:feature-history:sketch#1, which the earlier sketch feature nx:feature-history:feature#a holds, so its design feature states no sketch."]
    );
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn a_record_with_no_design_record_charges_a_loss_naming_it() {
    let mut ir = CadIr::empty();
    ir.model.features = vec![feature("a", 0, SketchFeatureBinding::Unresolved)];
    ir.model.sketches = vec![model_sketch(SketchPlacement::Unresolved {})];
    ir.model.sketch_entities = vec![point("g0", "nx-point-group")];
    ir.model.sketch_constraints = vec![constraint()];
    let losses = project_ir(&mut ir);
    assert!(ir.design.sketches.is_empty());
    assert!(ir.design.sketch_entities.is_empty());
    assert_eq!(
        unprojected(&losses),
        [
            "NX sketch feature nx:feature-history:feature#a holds no decoded sketch, so its design feature states no sketch.",
            "NX sketch nx:feature-history:sketch#1 has no design record and is not written.",
            "NX sketch entity nx:feature-history:sketch-entity#g0 has no design record and is not written.",
            "NX sketch constraint nx:feature-history:sketch-constraint#1 has no design record and is not written.",
        ]
    );
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn a_held_sketch_charges_a_loss_for_each_constraint() {
    let mut ir = CadIr::empty();
    ir.model.features = vec![owning("a", 0)];
    ir.model.sketches = vec![model_sketch(SketchPlacement::Unresolved {})];
    ir.model.sketch_constraints = vec![constraint()];
    let losses = project_ir(&mut ir);
    assert_eq!(ir.design.sketches.len(), 1);
    assert_eq!(
        unprojected(&losses),
        ["NX sketch constraint nx:feature-history:sketch-constraint#1 has no design record and is not written."]
    );
}
