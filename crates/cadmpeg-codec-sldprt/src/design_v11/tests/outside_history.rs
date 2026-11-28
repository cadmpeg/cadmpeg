// SPDX-License-Identifier: Apache-2.0
//! Sketches that no feature of the timeline restates: each is the operation
//! of a feature of one unordered `Group`, a root of the part's scope, and a
//! part with no feature history states only them, its bodies unproduced
//! (§5.2, §5.5).

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use cadmpeg_core::serde_erase::json;
use std::collections::{BTreeSet, HashSet};

use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::SketchRelation;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::{assert_no_design_findings, sketch_and_extrude, ROOT};
use crate::loss::SldprtLossCode;

/// Project the design of `ir` again from `histories`, with the rollback at
/// the end of a timeline whose last feature is `Boss`, and return the losses.
pub(super) fn project(
    ir: &mut CadIr,
    histories: &[crate::records::FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) -> Vec<LossNote> {
    project_with(
        ir,
        histories,
        lanes,
        super::super::EquationManagers::default(),
    )
}

/// [`project`] with the equation managers `equations`.
pub(in crate::design_v11) fn project_with(
    ir: &mut CadIr,
    histories: &[crate::records::FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    equations: super::super::EquationManagers,
) -> Vec<LossNote> {
    ir.design = cadmpeg_ir::document::Design::default();
    ir.model
        .product_definitions
        .retain(|definition| definition.id.as_str() != ROOT);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let maps = crate::resolved_features::assembly::ConfigurationMaps::new();
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    let mut losses = Vec::new();
    super::super::project(
        &ctx,
        ir,
        super::super::Part {
            histories,
            lanes,
            configuration_maps: &maps,
            bodies,
            persistent: super::super::Persistent {
                faces: &[],
                bodies: &[],
            },
            length: None,
            equations,
            length_places: None,
            rollback: crate::rollback::Reading::End {
                last: "Boss".to_owned(),
                recorded: BTreeSet::new(),
            },
            outside_profile: &HashSet::new(),
            given_up_configurations: &BTreeSet::new(),
            scan: None,
            marks: &crate::rollback::StoredMarks::new(),
        },
        &mut losses,
    )
    .unwrap();
    losses
}

fn named(losses: &[LossNote], code: SldprtLossCode, text: &str) -> bool {
    losses
        .iter()
        .any(|note| note.code == code.kind() && note.message.contains(text))
}

pub(super) fn group(ir: &CadIr) -> (&cadmpeg_ir::design::ids::ContainerId, &BTreeSet<Member>) {
    let groups = ir
        .design
        .containers
        .iter()
        .filter_map(|container| match container {
            Container::Unordered {
                id,
                kind: UnorderedKind::Group,
                members,
            } => Some((id, members)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [group] = groups.as_slice() else {
        panic!("one group: {:#?}", ir.design.containers);
    };
    *group
}

#[test]
fn a_sketch_no_timeline_feature_lists_is_owned_by_a_member_of_one_group() {
    const EXTRA: &str = "sldprt:model:sketch#extra";
    let (mut ir, native) = sketch_and_extrude();
    let mut sketches = json::to_value(&ir.model.sketches).unwrap();
    sketches.as_array_mut().unwrap().push(cadmpeg_core::json!({
        "id": EXTRA, "name": "Loose",
        "placement": {"kind": "resolved", "frame": {"origin": {"x": 0.0, "y": 0.0, "z": 5.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}}},
        "profiles": []
    }));
    ir.model.sketches = json::from_value(sketches).unwrap();
    ir.model.sketch_entities.push(
        json::from_value(cadmpeg_core::json!({
            "id": "sldprt:model:sketch-entity#extra:c", "sketch": EXTRA,
            "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "circle", "center": {"u": 1.0, "v": 2.0}, "radius": 3.0}}))
        .unwrap(),
    );
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let (group_id, members) = group(&ir);
    let listed = members.iter().collect::<Vec<_>>();
    let [Member::Feature(owner)] = listed.as_slice() else {
        panic!("one member: {members:#?}");
    };
    assert_eq!(scope.roots.len(), 2);
    assert_eq!(scope.roots.last(), Some(group_id));
    assert!(
        owner.as_str().starts_with("sldprt:design:sketch-owner#"),
        "{owner}"
    );
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| &feature.id == owner)
        .unwrap();
    assert_eq!(feature.name.as_deref(), Some("Loose"));
    let extra = ir
        .design
        .sketches
        .iter()
        .find(|sketch| sketch.id.as_str() == EXTRA)
        .unwrap();
    assert_eq!(
        feature.definition.operation,
        FeatureOperation::Sketch(extra.id.clone())
    );
    assert_eq!(&extra.owner, owner);
    assert!(ir
        .design
        .sketches
        .iter()
        .any(|sketch| sketch.id.as_str() == "sldprt:model:sketch#test"));
    assert!(
        losses.iter().all(|note| !note.message.contains(EXTRA)),
        "{losses:#?}"
    );
    assert_no_design_findings(&ir);
}

#[test]
fn a_part_without_history_states_its_sketches_in_one_group_with_its_bodies_unproduced() {
    const SKETCH: &str = "sldprt:model:sketch#test";
    const WIDTH: &str = "sldprt:model:sketch-constraint#test:1";
    let (mut ir, _) = sketch_and_extrude();
    let constraints = json::to_value(&ir.model.sketch_constraints).unwrap();
    let width = constraints
        .as_array()
        .unwrap()
        .iter()
        .find(|constraint| constraint["id"] == WIDTH)
        .unwrap()["definition"]["parameter"]
        .clone();
    let mut parameters = json::to_value(&ir.model.parameters).unwrap();
    for parameter in parameters.as_array_mut().unwrap() {
        if parameter["id"] == width {
            parameter["value"] = cadmpeg_core::json!({"kind": "length", "value": 50.0});
        }
    }
    ir.model.parameters = json::from_value(parameters).unwrap();
    let losses = project(&mut ir, &[], &[]);

    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope: {:#?}", ir.design.scopes);
    };
    assert_eq!(scope.definition.as_str(), ROOT);
    let (group_id, members) = group(&ir);
    assert_eq!(scope.roots, vec![group_id.clone()]);
    assert!(scope.current.is_empty());
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    assert!(!bodies.is_empty());
    assert_eq!(scope.unproduced, bodies);
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.id.as_str() == ROOT)
        .unwrap();
    assert_eq!(root.bodies.iter().cloned().collect::<BTreeSet<_>>(), bodies);

    let model_owner = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap()
        .id
        .clone();
    assert_eq!(
        members.iter().collect::<Vec<_>>(),
        vec![&Member::Feature(model_owner.clone())]
    );
    let [feature] = ir.design.features.as_slice() else {
        panic!("one feature: {:#?}", ir.design.features);
    };
    assert_eq!(feature.id, model_owner);
    let [sketch] = ir.design.sketches.as_slice() else {
        panic!("one sketch");
    };
    assert_eq!(sketch.id.as_str(), SKETCH);
    assert_eq!(
        feature.definition.operation,
        FeatureOperation::Sketch(sketch.id.clone())
    );
    let width = ir
        .design
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.id.as_str() == WIDTH)
        .unwrap();
    let SketchRelation::Distance {
        value: Dimension::Literal(length),
        ..
    } = &width.relation
    else {
        panic!("a distance of the stored value: {:#?}", width.relation);
    };
    assert_eq!(length.get(), 50.0);
    assert!(
        named(&losses, SldprtLossCode::DesignSketchDimensionUnbound, WIDTH),
        "{losses:#?}"
    );
    assert_no_design_findings(&ir);
}
