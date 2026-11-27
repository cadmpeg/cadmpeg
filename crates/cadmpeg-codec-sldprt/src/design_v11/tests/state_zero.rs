// SPDX-License-Identifier: Apache-2.0
//! The profile an extrude picks: its whole sketch, whose curves of state
//! `0` are construction and bound no region, and the note charged where the
//! sketch's closed regions differ from the stored profile (§6 "Sources";
//! §6.1 "Flags" and "Profiles and regions").

use cadmpeg_core::serde_erase::json;
use std::collections::HashSet;

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::Extrude;
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::SketchEntityId;

use super::{assert_no_design_findings, sketch_and_extrude, timeline_feature};

pub(super) fn entity(key: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("sldprt:model:sketch-entity#test:{key}")).unwrap()
}

/// Sets the profile loops of the fixture's sketch, each a list of entity
/// keys and their `reversed` flags.
pub(super) fn set_profiles(ir: &mut CadIr, loops: &[&[(&str, bool)]]) {
    let loops = loops
        .iter()
        .map(|uses| {
            uses.iter()
                .map(|(key, reversed)| {
                    cadmpeg_core::json!({"entity": entity(key).as_str(), "reversed": reversed})
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    ir.model.sketches[0].profiles = json::from_value(cadmpeg_core::json!(loops)).unwrap();
}

/// Projects the fixture's design with `outside` read from curve records of
/// state `0`, states it in `ir`, and returns the typed extrude and the
/// projection's notes.
pub(super) fn project(
    ir: &mut CadIr,
    outside: &HashSet<SketchEntityId>,
) -> (Extrude, super::super::typed::Typed) {
    let native = crate::native::SldprtNative::load(
        ir.native.namespace("sldprt").unwrap(),
        ir.tolerances.linear,
    )
    .unwrap();
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
            surfaces: &ir.geometry.surfaces,
            angular: ir.tolerances.angular.get(),
        },
    );
    assert!(typed.loss.is_none(), "{:?}", typed.loss);
    let FeatureOperation::Extrude(extrude) =
        &timeline_feature(&timeline, "Boss").definition.operation
    else {
        panic!("the extrude is typed");
    };
    let extrude = extrude.as_ref().clone();
    let (parameters, _, _) = entries.parameters(
        &definition,
        &super::super::parameters::Readings {
            decoded: super::super::parameters::Decoded::new(
                &ir.model.parameters,
                &[],
                &crate::resolved_features::assembly::ConfigurationMaps::new(),
            ),
            length_unit: Some(UnitSymbol::Mm),
            length_places: None,
            bound: std::mem::take(&mut typed.bound),
            measured: std::mem::take(&mut typed.measured),
            driven: std::mem::take(&mut typed.references),
            manager: std::collections::HashMap::new(),
            confirmed: HashSet::new(),
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

/// The fixture's rectangle and the circle inside it: the decoder resolved
/// both loops, and the extrude still picks the whole sketch, which keeps its
/// meaning when an edit adds a loop.
#[test]
fn an_extrude_picks_its_whole_sketch() {
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
    assert_eq!(targets(&extrude), vec![PickTarget::Sketch(sketch)]);
    assert!(
        typed.state_construction.is_none(),
        "{:?}",
        typed.state_construction
    );
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
    assert_no_design_findings(&ir);
}

/// A diagonal read from a curve record of state `0` is construction, so it
/// bounds no region of the whole-sketch pick, and is reported; the rectangle
/// and the circle state no flag.
#[test]
fn a_curve_of_state_zero_is_construction() {
    let (mut ir, _) = sketch_and_extrude();
    let mut diagonal = json::to_value(&ir.model.sketch_entities[4]).unwrap();
    diagonal["id"] = cadmpeg_core::json!(entity("diagonal").as_str());
    diagonal["geometry"]["end"] = cadmpeg_core::json!({"u": 50.0, "v": 30.0});
    ir.model
        .sketch_entities
        .push(json::from_value(diagonal).unwrap());
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
    assert_eq!(
        targets(&extrude),
        vec![PickTarget::Sketch(ir.model.sketches[0].id.clone())]
    );
    let note = typed
        .state_construction
        .expect("the construction reading is reported");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignSketchConstructionAssumed.kind()
    );
    assert!(note.message.contains(entity("diagonal").as_str()));
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
    assert_no_design_findings(&ir);
}

/// Adds the upper half of a ring about (100, 0), radii 5 and 10, to the
/// fixture's sketch: two arcs and the two lines that join their ends.
fn add_c_ring(ir: &mut CadIr) {
    let sketch = ir.model.sketches[0].id.as_str().to_owned();
    let arc = |key: &str, radius: f64| {
        cadmpeg_core::json!({"id": entity(key).as_str(), "sketch": sketch, "construction": false,
            "endpoint_refs": [], "geometry": {"kind": "arc", "center": {"u": 100.0, "v": 0.0},
            "radius": radius, "start_angle": 0.0, "end_angle": std::f64::consts::PI}})
    };
    for added in [
        arc("outer", 10.0),
        arc("inner", 5.0),
        line(&sketch, "left", [90.0, 0.0], [95.0, 0.0]),
        line(&sketch, "right", [105.0, 0.0], [110.0, 0.0]),
    ] {
        ir.model
            .sketch_entities
            .push(json::from_value(added).unwrap());
    }
}

fn line(sketch: &str, key: &str, from: [f64; 2], to: [f64; 2]) -> serde_json::Value {
    cadmpeg_core::json!({"id": entity(key).as_str(), "sketch": sketch, "construction": false,
        "endpoint_refs": [], "geometry": {"kind": "line",
        "start": {"u": from[0], "v": from[1]}, "end": {"u": to[0], "v": to[1]}}})
}

const PROFILE: &[&[(&str, bool)]] = &[
    &[("l0", false), ("l1", false), ("l2", false), ("l3", false)],
    &[("hole", false)],
    &[
        ("outer", false),
        ("left", false),
        ("inner", true),
        ("right", false),
    ],
];

/// The stored profile holds the rectangle, its circle and a C-shaped ring,
/// and the sketch holds nothing else: the whole-sketch pick's regions are
/// that profile, and no note is charged.
#[test]
fn a_sketch_whose_regions_are_the_stored_profile_charges_nothing() {
    let (mut ir, _) = sketch_and_extrude();
    add_c_ring(&mut ir);
    set_profiles(&mut ir, PROFILE);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
}

/// A line joins the ends of the C-shaped ring's inner arc, which the stored
/// profile leaves out: the whole-sketch pick also takes the half disc the
/// line closes, outside the stored profile, and the note names the extrude
/// and the extra region.
#[test]
fn a_line_closing_a_c_ring_charges_an_extra_region() {
    let (mut ir, _) = sketch_and_extrude();
    add_c_ring(&mut ir);
    let sketch = ir.model.sketches[0].id.as_str().to_owned();
    ir.model
        .sketch_entities
        .push(json::from_value(line(&sketch, "chord", [95.0, 0.0], [105.0, 0.0])).unwrap());
    set_profiles(&mut ir, PROFILE);
    let (extrude, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(
        targets(&extrude),
        vec![PickTarget::Sketch(ir.model.sketches[0].id.clone())]
    );
    let note = typed
        .profile_differs
        .expect("the difference from the stored profile is reported");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignExtrudeProfileDiffers.kind()
    );
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Warning);
    let boss = ir
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(note.message.contains(boss.id.as_str()), "{}", note.message);
    assert!(
        note.message
            .contains("(extra region outside the stored profile)"),
        "{}",
        note.message
    );
    assert_no_design_findings(&ir);
}

/// The top edge of the rectangle is read from a curve record of state `0`,
/// and the rectangle closes only through it: it stays profile geometry and
/// is not reported as construction. The stored profile holds only the
/// circle, so the note names the rectangle's region as extra.
#[test]
fn a_curve_of_state_zero_that_closes_the_profile_stays_profile_geometry() {
    let (mut ir, _) = sketch_and_extrude();
    set_profiles(&mut ir, &[&[("hole", false)]]);
    let (_, typed) = project(&mut ir, &HashSet::from([entity("l2")]));
    let l2 = ir
        .design
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id == entity("l2"))
        .unwrap();
    assert_eq!(l2.construction, None);
    assert!(
        typed.state_construction.is_none(),
        "{:?}",
        typed.state_construction
    );
    let note = typed
        .profile_differs
        .expect("the difference from the stored profile is reported");
    assert!(
        note.message
            .contains("(extra region outside the stored profile)"),
        "{}",
        note.message
    );
}

/// A sketch whose decoder read no profile loop is not compared, and the
/// note says why.
#[test]
fn an_extrude_whose_sketch_has_no_stored_loop_is_reported_unchecked() {
    let (mut ir, _) = sketch_and_extrude();
    set_profiles(&mut ir, &[]);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
    let note = typed
        .profile_unchecked
        .expect("the uncompared profile is reported");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignExtrudeProfileUnchecked.kind()
    );
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Info);
    assert!(
        note.message.contains("(no stored profile loop)"),
        "{}",
        note.message
    );
    assert!(typed.sketch_empty.is_none(), "{:?}", typed.sketch_empty);
}

/// Every curve of the sketch is read from a curve record of state `0`, and
/// no other curve closes through them: all are construction, the sketch
/// holds no profile curve, and the note says the pick takes no region.
#[test]
fn an_extrude_whose_sketch_holds_no_profile_curve_is_reported_empty() {
    let (mut ir, _) = sketch_and_extrude();
    set_profiles(&mut ir, &[]);
    let outside = ["l0", "l1", "l2", "l3", "hole"]
        .into_iter()
        .map(entity)
        .collect::<HashSet<_>>();
    let (_, typed) = project(&mut ir, &outside);
    let note = typed.sketch_empty.expect("the empty profile is reported");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignExtrudeSketchEmpty.kind()
    );
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Warning);
    let boss = ir
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(note.message.contains(boss.id.as_str()), "{}", note.message);
    assert!(
        typed.profile_unchecked.is_none(),
        "{:?}",
        typed.profile_unchecked
    );
}
