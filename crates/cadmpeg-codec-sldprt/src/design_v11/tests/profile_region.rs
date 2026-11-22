// SPDX-License-Identifier: Apache-2.0
//! The profile an extrude picks: the regions of its sketch's profile loops,
//! and the curves of state `0` that bound none (§6.1, "Flags" and "Profiles
//! and regions").

use std::collections::HashSet;

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::Extrude;
use cadmpeg_ir::design::selection::{BoundaryUse, PickTarget, Picks, SketchRegion};
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::SketchEntityId;

use super::{assert_no_design_findings, sketch_and_extrude, timeline_feature};

fn entity(key: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("sldprt:model:sketch-entity#test:{key}")).unwrap()
}

/// Sets the profile loops of the fixture's sketch, each a list of entity
/// keys and their `reversed` flags.
fn set_profiles(ir: &mut CadIr, loops: &[&[(&str, bool)]]) {
    let loops = loops
        .iter()
        .map(|uses| {
            uses.iter()
                .map(|(key, reversed)| {
                    serde_json::json!({"entity": entity(key).as_str(), "reversed": reversed})
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    ir.model.sketches[0].profiles = serde_json::from_value(serde_json::json!(loops)).unwrap();
}

/// Projects the fixture's design with `outside` read from curve records of
/// state `0`, states it in `ir`, and returns the typed extrude and the
/// projection's notes.
fn project(
    ir: &mut CadIr,
    outside: &HashSet<SketchEntityId>,
) -> (Extrude, super::super::typed::Typed) {
    let native = crate::native::SldprtNative::load(ir.native.namespace("sldprt").unwrap()).unwrap();
    let mut timeline = super::super::tree::timeline(&native.feature_histories).unwrap();
    let definition = ir.design.scopes[0].definition.clone();
    let entries =
        super::super::parameters::Entries::new(&native.feature_histories, &timeline).unwrap();
    let mut typed = super::super::typed::project(
        &ir.model,
        &native.feature_histories,
        &[],
        &mut timeline,
        &entries,
        &super::super::typed::Scope {
            definition: &definition,
            outside_profile: outside,
        },
    );
    assert!(typed.loss.is_none(), "{:?}", typed.loss);
    let FeatureOperation::Extrude(extrude) =
        &timeline_feature(&timeline, "Boss").definition.operation
    else {
        panic!("the extrude is typed");
    };
    let extrude = extrude.as_ref().clone();
    let (parameters, _) = entries.parameters(
        &definition,
        &super::super::parameters::Readings {
            decoded: super::super::parameters::Decoded::new(&ir.model.parameters, &[]),
            length_unit: Some(UnitSymbol::Mm),
            bound: std::mem::take(&mut typed.bound),
            measured: std::mem::take(&mut typed.measured),
            driven: std::mem::take(&mut typed.references),
        },
    );
    ir.design.features = timeline.features;
    ir.design.parameters = parameters;
    ir.design.sketches = std::mem::take(&mut typed.sketches.records);
    ir.design.sketch_entities = std::mem::take(&mut typed.sketches.entities);
    ir.design.sketch_constraints = std::mem::take(&mut typed.sketches.constraints);
    (extrude, typed)
}

fn targets(extrude: &Extrude) -> Vec<PickTarget> {
    let Picks::Named(picks) = &extrude.profile.picks else {
        panic!("the profile names its picks");
    };
    picks
        .as_slice()
        .iter()
        .map(|pick| pick.target.clone())
        .collect()
}

fn ring(keys: &[&str]) -> Vec<BoundaryUse> {
    keys.iter()
        .map(|key| BoundaryUse {
            entity: entity(key),
            fragment: None,
        })
        .collect()
}

/// The fixture's rectangle runs counterclockwise from `a` at the origin, and
/// its circle lies inside it: one region, the circle its hole.
#[test]
fn an_extrude_picks_the_region_of_its_sketch_profile_loops() {
    let (mut ir, _) = sketch_and_extrude();
    set_profiles(
        &mut ir,
        &[
            &[("l0", false), ("l1", false), ("l2", false), ("l3", false)],
            &[("hole", false)],
        ],
    );
    let (extrude, typed) = project(&mut ir, &HashSet::new());
    let sketch = ir.model.sketches[0].id.clone();
    assert_eq!(
        targets(&extrude),
        vec![PickTarget::SketchRegion {
            sketch,
            region: SketchRegion {
                outer: ring(&["l0", "l1", "l2", "l3"]),
                holes: vec![ring(&["hole"])],
            },
        }]
    );
    assert!(typed.whole_sketch.is_none(), "{:?}", typed.whole_sketch);
    assert_no_design_findings(&ir);
}

/// A loop stored clockwise is an outer ring in reverse order.
#[test]
fn an_outer_ring_runs_counterclockwise() {
    let (mut ir, _) = sketch_and_extrude();
    set_profiles(
        &mut ir,
        &[&[("l3", true), ("l2", true), ("l1", true), ("l0", true)]],
    );
    let (extrude, _) = project(&mut ir, &HashSet::new());
    let picked = targets(&extrude);
    let [PickTarget::SketchRegion { region, .. }] = picked.as_slice() else {
        panic!("one region");
    };
    assert_eq!(region.outer, ring(&["l0", "l1", "l2", "l3"]));
    assert!(region.holes.is_empty());
}

/// An extrude of a sketch whose profile loops the decoder did not resolve
/// picks the whole sketch and says so.
#[test]
fn an_extrude_of_a_sketch_without_profile_loops_picks_the_sketch() {
    let (mut ir, _) = sketch_and_extrude();
    let (extrude, typed) = project(&mut ir, &HashSet::new());
    let sketch = ir.model.sketches[0].id.clone();
    assert_eq!(targets(&extrude), vec![PickTarget::Sketch(sketch)]);
    let note = typed
        .whole_sketch
        .expect("the whole-sketch pick is reported");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignExtrudeSketchPicked.kind()
    );
    let boss = ir
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(
        note.message.contains(&format!(
            "{} (the decoder resolved no profile loop of its sketch)",
            boss.id.as_str()
        )),
        "{}",
        note.message
    );
}

/// A diagonal read from a curve record of state `0` is construction, bounds
/// no region, and is reported; the rectangle and the circle state no flag.
#[test]
fn a_curve_of_state_zero_is_construction() {
    let (mut ir, _) = sketch_and_extrude();
    let mut diagonal = serde_json::to_value(&ir.model.sketch_entities[4]).unwrap();
    diagonal["id"] = serde_json::json!(entity("diagonal").as_str());
    diagonal["geometry"]["end"] = serde_json::json!({"u": 50.0, "v": 30.0});
    ir.model
        .sketch_entities
        .push(serde_json::from_value(diagonal).unwrap());
    set_profiles(
        &mut ir,
        &[
            &[("l0", false), ("l1", false), ("l2", false), ("l3", false)],
            &[("hole", false)],
        ],
    );
    let (extrude, typed) = project(&mut ir, &HashSet::from([entity("diagonal")]));
    let construction = |key: &str| {
        ir.design
            .sketch_entities
            .iter()
            .find(|candidate| candidate.id == entity(key))
            .unwrap()
            .construction
    };
    assert_eq!(construction("diagonal"), Some(true));
    assert_eq!(construction("l0"), None);
    assert_eq!(construction("hole"), None);
    assert_eq!(targets(&extrude).len(), 1);
    let note = typed
        .state_construction
        .expect("the construction reading is reported");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignSketchConstructionAssumed.kind()
    );
    assert!(note.message.contains(entity("diagonal").as_str()));
    assert_no_design_findings(&ir);
}
