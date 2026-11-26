// SPDX-License-Identifier: Apache-2.0
//! The give-up of a record: the part it leaves, the records it drops, and
//! the records it states in a partial form.

use cadmpeg_core::serde_erase::json;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use serde_json::json;

use super::{give_up, Arena, References, Seed};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use crate::document::CadIr;
use crate::ids::Identity;
use crate::report::loss::LossNote;
use crate::source_fidelity::SourceFidelity;
use crate::topology::{FaceLoops, RegionUnstated, Shell, Unstated};
use crate::unknown::part::record_unreadable_kind;
use crate::NativeUnknownRecord;

const CODEC: &str = "test";

/// What one give-up left: its stub, the records it dropped and marked, the
/// records of the delta of each stored state it dropped and marked, by the
/// state's path, and the tail of its loss message.
#[derive(Debug, Default)]
struct Outcome {
    stub: String,
    path: String,
    dropped: BTreeSet<String>,
    marked: BTreeSet<String>,
    states: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    tail: String,
}

fn seed(home: &str, field: &str, id: &str) -> Seed {
    Seed {
        arena: Arena::home(home, field).expect("a registered arena"),
        id: id.to_owned(),
    }
}

/// Gives up `seed` from `ir` and returns what the give-up did and the
/// source fidelity that holds its retained record.
fn run(ir: &mut CadIr, seed: Seed) -> (Outcome, SourceFidelity) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).expect("root");
    let mut fidelity = SourceFidelity::for_root(root);
    let mut outcome = Outcome::default();
    let losses = give_up(
        &ctx,
        ir,
        &mut fidelity,
        CODEC,
        vec![seed],
        |_, released, retained| {
            outcome = Outcome {
                stub: retained.stub.clone(),
                path: released.path.clone(),
                dropped: released.dropped.clone(),
                marked: released.marked.clone(),
                states: released
                    .states
                    .iter()
                    .map(|(path, state)| {
                        (path.clone(), (state.dropped.clone(), state.marked.clone()))
                    })
                    .collect(),
                tail: released.tail(&[]),
            };
            LossNote::new(record_unreadable_kind(), released.tail(&[]))
        },
    )
    .expect("the give-up commits");
    assert_eq!(losses.len(), 1);
    (outcome, fidelity)
}

/// Every id the give-up removed that no record of `ir` still holds.
fn gone(ir: &CadIr, seed: &str, outcome: &Outcome) -> BTreeSet<String> {
    outcome
        .dropped
        .iter()
        .chain([&seed.to_owned()])
        .filter(|id| !holds(ir, id))
        .cloned()
        .collect()
}

fn holds(ir: &CadIr, id: &str) -> bool {
    ir.references_from(References::default())
        .held
        .contains_key(id)
}

/// No referential-integrity finding of `ir` names an id the give-up
/// removed.
#[track_caller]
fn assert_closed(ir: &CadIr, seed: &str, outcome: &Outcome) {
    let gone = gone(ir, seed, outcome);
    let names: Vec<String> = crate::validate::validate_neutral(ir, Vec::new())
        .expect("validation runs")
        .findings
        .into_iter()
        .filter(|finding| finding.check == crate::report::check::Check::ReferentialIntegrity)
        .filter(|finding| {
            gone.iter()
                .any(|id| finding.message.contains(id.as_str()) || finding.related.contains(id))
        })
        .map(|finding| finding.message)
        .collect();
    assert!(
        names.is_empty(),
        "the document names a removed id: {names:?}"
    );
}

#[test]
fn a_given_up_point_leaves_its_part_and_drops_each_record_that_needs_it() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let point = ir.geometry.points[0].clone();
    let vertex = ir.geometry.vertices[0].id.as_str().to_owned();
    let (outcome, fidelity) = run(&mut ir, seed("geometry", "points", point.id.as_str()));

    assert!(!holds(&ir, point.id.as_str()));
    assert!(outcome.dropped.contains(&vertex));
    let lost_edges: BTreeSet<String> = crate::examples::unit_cube()
        .unwrap()
        .geometry
        .edges
        .iter()
        .filter(|edge| {
            edge.ends
                .as_ref()
                .is_some_and(|ends| ends.start.as_str() == vertex || ends.end.as_str() == vertex)
        })
        .map(|edge| edge.id.as_str().to_owned())
        .collect();
    assert_eq!(lost_edges.len(), 3);
    assert!(outcome.dropped.is_superset(&lost_edges));
    assert_eq!(ir.geometry.points.len(), 7);
    assert_eq!(ir.geometry.faces.len(), 6);
    for face in &ir.geometry.faces {
        if outcome.marked.contains(face.id.as_str()) {
            assert!(matches!(
                face.loops,
                FaceLoops::PartialClassified { .. } | FaceLoops::PartialUnspecified { .. }
            ));
        }
    }
    assert_closed(&ir, point.id.as_str(), &outcome);

    assert_eq!(
        outcome.path,
        format!("geometry.points/{}", point.id.as_str())
    );
    let stubs = ir.native_unknowns(CODEC).expect("the codec's stubs");
    assert_eq!(stubs.len(), 1);
    assert_eq!(stubs[0].id.as_str(), outcome.stub);
    assert!(stubs[0].links.is_empty());
    let retained = fidelity
        .retained_record(&outcome.stub)
        .expect("the retained record");
    assert_eq!(retained.stream(), outcome.path);
    assert_eq!(retained.offset(), 0);
    let text = crate::hash::finite_json::to_compact_canonical_json_string(&point).unwrap();
    assert_eq!(retained.data(), Some(text.as_bytes()));
    let classes: BTreeSet<_> = Arena::home("geometry", "points")
        .unwrap()
        .classes()
        .into_iter()
        .collect();
    assert!(!classes.is_empty());
    assert_eq!(retained.untyped(), &classes);
}

#[test]
fn a_region_that_loses_a_void_states_voids_and_one_that_loses_its_exterior_states_exterior() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let region = ir.geometry.regions[0].clone();
    let exterior = ir.geometry.shells[0].id.clone();
    let void = crate::ids::ShellId::mint("synthetic:cube:shell#void").unwrap();
    let edge = ir.geometry.edges[0].id.clone();
    ir.geometry
        .shells
        .push(Shell::with_wire_edge(void.clone(), region.id.clone(), edge));
    ir.geometry.regions[0].push_shell(void.clone());

    let mut lost_void = ir.clone();
    let (outcome, _) = run(&mut lost_void, seed("geometry", "shells", void.as_str()));
    assert!(outcome.marked.contains(region.id.as_str()));
    let region_after = &lost_void.geometry.regions[0];
    assert_eq!(region_after.unstated, Some(RegionUnstated::Voids));
    assert_eq!(
        region_after.stated_shells().collect::<Vec<_>>(),
        [&exterior]
    );
    assert_closed(&lost_void, void.as_str(), &outcome);

    let (outcome, _) = run(&mut ir, seed("geometry", "shells", exterior.as_str()));
    assert!(outcome.marked.contains(region.id.as_str()));
    assert_eq!(
        ir.geometry.regions[0].unstated,
        Some(RegionUnstated::Exterior { voids: vec![void] })
    );
    assert!(ir.geometry.faces.is_empty());
    assert_closed(&ir, exterior.as_str(), &outcome);
}

#[test]
fn a_shell_that_loses_a_wire_edge_states_its_members_unstated() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let region = ir.geometry.regions[0].id.clone();
    let wire = crate::ids::ShellId::mint("synthetic:cube:shell#wire").unwrap();
    let edge = ir.geometry.edges[0].id.clone();
    ir.geometry
        .shells
        .push(Shell::with_wire_edge(wire.clone(), region, edge.clone()));
    let (outcome, _) = run(&mut ir, seed("geometry", "edges", edge.as_str()));
    assert!(outcome.marked.contains(wire.as_str()));
    let shell = ir
        .geometry
        .shells
        .iter()
        .find(|shell| shell.id == wire)
        .expect("the wire shell stays");
    assert!(shell.members.is_empty());
    assert_eq!(shell.unstated, Some(Unstated::Members));
    assert_closed(&ir, edge.as_str(), &outcome);
}

#[test]
fn a_given_up_face_marks_its_shell_and_a_given_up_region_marks_its_body() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let face = ir.geometry.faces[0].clone();
    let (outcome, _) = run(&mut ir, seed("geometry", "faces", face.id.as_str()));
    assert!(outcome.marked.contains(face.shell.as_str()));
    assert_eq!(ir.geometry.shells[0].unstated, Some(Unstated::Members));
    assert_eq!(ir.geometry.faces.len(), 5);
    assert_closed(&ir, face.id.as_str(), &outcome);

    let mut ir = crate::examples::unit_cube().unwrap();
    let region = ir.geometry.regions[0].clone();
    let (outcome, _) = run(&mut ir, seed("geometry", "regions", region.id.as_str()));
    assert!(outcome.marked.contains(region.body.as_str()));
    assert_eq!(ir.geometry.bodies[0].unstated, Some(Unstated::Members));
    assert!(ir.geometry.shells.is_empty());
    assert_closed(&ir, region.id.as_str(), &outcome);
}

/// A design feature named `name` with `operation`.
fn feature(name: &str, operation: serde_json::Value) -> Feature {
    Feature {
        id: crate::features::FeatureId::mint(format!("test:design:feature#{name}")).unwrap(),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: json::from_value(operation).unwrap(),
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

#[test]
fn a_feature_whose_operation_names_a_lost_record_states_its_operation_unresolved() {
    let mut ir = CadIr::empty();
    let base = feature(
        "base",
        json!({"kind": "unresolved", "value": {"family": "extrude"}}),
    );
    let pick = json!({"target": {"kind": "datum", "value": {"feature": base.id.as_str()}}});
    let plane = feature(
        "plane",
        json!({
            "kind": "datum",
            "value": {
                "element": {"kind": "plane", "value": {
                    "after": [],
                    "resolved_in": {},
                    "inputs": [{
                        "role": {"subject": "whole", "relation": {"kind": "coincident"}},
                        "reference": {"kind": "picks", "value": {
                            "at": {"kind": "pinned", "value": {"kind": "after", "value": {"kind": "container", "value": "test:design:container#body"}}},
                            "picks": {"kind": "named", "value": [pick]}
                        }}
                    }]
                }},
                "native_options": {},
                "source_settings": {}
            }
        }),
    );
    ir.design.features = vec![base.clone(), plane.clone()];
    let (outcome, _) = run(&mut ir, seed("design", "features", base.id.as_str()));
    assert!(outcome.dropped.is_empty());
    assert_eq!(
        outcome.marked,
        BTreeSet::from([plane.id.as_str().to_owned()])
    );
    assert_eq!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Datum
        }
    );
    assert_closed(&ir, base.id.as_str(), &outcome);
}

#[test]
fn a_stub_link_to_a_dropped_record_leaves_the_stub() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let point = ir.geometry.points[0].id.clone();
    let vertex = ir.geometry.vertices[0].id.clone();
    let kept = ir.geometry.vertices[1].id.clone();
    let stub = NativeUnknownRecord {
        id: crate::ids::UnknownId::mint("test:source:part#earlier").unwrap(),
        links: vec![
            Identity::new(vertex.as_str()).unwrap(),
            Identity::new(kept.as_str()).unwrap(),
        ],
    };
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).expect("root");
    ir.set_native_unknowns(&ctx, CODEC, std::slice::from_ref(&stub))
        .unwrap();
    let (outcome, _) = run(&mut ir, seed("geometry", "points", point.as_str()));
    assert!(outcome.dropped.contains(vertex.as_str()));
    let earlier = ir
        .native_unknowns(CODEC)
        .unwrap()
        .into_iter()
        .find(|record| record.id == stub.id)
        .expect("the earlier stub stays");
    assert_eq!(earlier.links, [Identity::new(kept.as_str()).unwrap()]);
}

#[test]
fn an_id_another_arena_holds_keeps_the_records_that_name_it() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let point = ir.geometry.points[0].id.clone();
    let record = crate::NativeRecord::new(
        Identity::new(point.as_str()).unwrap(),
        serde_json::Map::new(),
    )
    .unwrap();
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("other").unwrap())
        .set_arena_records("restated", vec![record])
        .unwrap();
    let vertices = ir.geometry.vertices.clone();
    let (outcome, _) = run(&mut ir, seed("geometry", "points", point.as_str()));
    assert!(outcome.dropped.is_empty());
    assert!(outcome.marked.is_empty());
    assert_eq!(ir.geometry.vertices, vertices);
}

#[test]
fn a_record_that_projects_a_given_up_native_record_keeps_its_values() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let native = "test:native:face#1";
    let record =
        crate::NativeRecord::new(Identity::new(native).unwrap(), serde_json::Map::new()).unwrap();
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
        .set_arena_records("faces", vec![record])
        .unwrap();
    ir.geometry.faces[0].native_ref = Some(native.to_owned());
    let face = ir.geometry.faces[0].clone();
    let (outcome, _) = run(
        &mut ir,
        Seed {
            arena: Arena::Native {
                codec: "test".to_owned(),
                arena: "faces".to_owned(),
            },
            id: native.to_owned(),
        },
    );
    assert!(outcome.dropped.is_empty());
    assert_eq!(
        outcome.marked,
        BTreeSet::from([face.id.as_str().to_owned()])
    );
    let kept = ir
        .geometry
        .faces
        .iter()
        .find(|kept| kept.id == face.id)
        .expect("the face stays");
    assert_eq!(kept.native_ref, None);
    assert_eq!(kept.loops, face.loops);
    assert_closed(&ir, native, &outcome);
}

fn unresolved(name: &str) -> Feature {
    feature(
        name,
        json!({"kind": "unresolved", "value": {"family": "extrude"}}),
    )
}

fn native_name(text: &str) -> serde_json::Value {
    json!({"codec": "test", "value": {"kind": "text", "value": text}})
}

fn datum_pick(of: &Feature) -> serde_json::Value {
    json!({"target": {"kind": "datum", "value": {"feature": of.id.as_str()}}})
}

/// A datum plane coincident with the picks `picks`, an `Ordered` list.
fn datum_plane(name: &str, picks: &[serde_json::Value]) -> Feature {
    feature(
        name,
        json!({
            "kind": "datum",
            "value": {
                "element": {"kind": "plane", "value": {
                    "after": [],
                    "resolved_in": {},
                    "inputs": [{
                        "role": {"subject": "whole", "relation": {"kind": "coincident"}},
                        "reference": {"kind": "picks", "value": {
                            "at": {"kind": "current"},
                            "picks": {"kind": "named", "value": picks}
                        }}
                    }]
                }},
                "native_options": {},
                "source_settings": {}
            }
        }),
    )
}

/// The picks of the first input of `feature`, a datum plane.
fn plane_picks(feature: &Feature) -> serde_json::Value {
    let operation = crate::hash::finite_json::to_json_value(&feature.definition.operation).unwrap();
    operation
        .pointer("/value/element/value/inputs/0/reference/value/picks/value")
        .cloned()
        .expect("the plane's picks")
}

#[test]
fn a_pick_of_a_lost_record_with_a_native_name_states_its_target_native() {
    let mut ir = CadIr::empty();
    let base = unresolved("base");
    let mut pick = datum_pick(&base);
    pick["native"] = native_name("Plane1");
    let plane = datum_plane("plane", &[pick]);
    ir.design.features = vec![base.clone(), plane.clone()];
    let (outcome, _) = run(&mut ir, seed("design", "features", base.id.as_str()));
    assert!(outcome.dropped.is_empty());
    assert_eq!(
        outcome.marked,
        BTreeSet::from([plane.id.as_str().to_owned()])
    );
    let native =
        json::to_value(crate::design::selection::PickTarget::Native { within: None }).unwrap();
    assert_eq!(
        plane_picks(&ir.design.features[0]),
        json!([{"target": native, "native": native_name("Plane1")}])
    );
    assert_closed(&ir, base.id.as_str(), &outcome);
}

#[test]
fn an_unordered_selection_loses_only_the_pick_of_a_lost_record() {
    let mut ir = CadIr::empty();
    let base = unresolved("base");
    let other = unresolved("other");
    let picks: Vec<crate::design::selection::Picked> =
        json::from_value(json!([datum_pick(&base), datum_pick(&other)])).unwrap();
    let picks =
        crate::design::selection::Members::<_, crate::design::selection::Unordered>::new(picks)
            .unwrap();
    let cut = feature(
        "cut",
        json!({"kind": "delete_bodies", "value": {
            "bodies": {"at": {"kind": "current"}, "picks": {"kind": "named", "value": picks}},
            "retain": "delete_selected",
            "native_options": {},
            "source_settings": {}
        }}),
    );
    ir.design.features = vec![base.clone(), cut.clone(), other.clone()];
    let (outcome, _) = run(&mut ir, seed("design", "features", base.id.as_str()));
    assert!(outcome.dropped.is_empty());
    assert_eq!(outcome.marked, BTreeSet::from([cut.id.as_str().to_owned()]));
    let kept = ir
        .design
        .features
        .iter()
        .find(|feature| feature.id == cut.id)
        .expect("the cut stays");
    let operation = crate::hash::finite_json::to_json_value(&kept.definition.operation).unwrap();
    assert_eq!(
        operation.pointer("/value/bodies/picks/value"),
        Some(&json!([datum_pick(&other)]))
    );
    assert_closed(&ir, base.id.as_str(), &outcome);
}

#[test]
fn an_expression_that_names_a_lost_parameter_states_it_source() {
    use crate::design::expr::{Expression, ExpressionDialect};
    use crate::design::parameter::{DesignParameter, ParameterDefinition};
    let parameter = |name: &str, definition: serde_json::Value| -> DesignParameter {
        json::from_value(json!({
            "id": format!("test:design:parameter#{name}"),
            "scope": "test:model:definition#part",
            "kind": {"kind": "integer"},
            "definition": definition,
            "native_options": {},
            "source_settings": {}
        }))
        .unwrap()
    };
    let mut ir = CadIr::empty();
    let a = parameter("a", json!({"kind": "driven"}));
    let b = parameter(
        "b",
        json!({"kind": "expression", "value": {"kind": "neutral", "value": {
            "text": "$0",
            "refs": [{"target": {"kind": "parameter", "value": a.id.as_str()}}],
            "measures": []
        }}}),
    );
    ir.design.parameters = vec![a.clone(), b.clone()];
    let (outcome, _) = run(&mut ir, seed("design", "parameters", a.id.as_str()));
    assert!(outcome.dropped.is_empty());
    assert_eq!(outcome.marked, BTreeSet::from([b.id.as_str().to_owned()]));
    assert_eq!(
        ir.design.parameters[0].definition,
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Native(
                cadmpeg_core::text::NonBlankString::new(CODEC).unwrap()
            ),
            tokens: Vec::new(),
        })
    );
    assert_closed(&ir, a.id.as_str(), &outcome);
}

#[test]
fn a_procedural_curve_on_a_lost_surface_states_its_definition_unknown() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let surface = ir.geometry.surfaces[0].id.as_str().to_owned();
    let spine: crate::geometry::ProceduralCurve = json::from_value(json!({
        "id": "test:geometry:procedural-curve#spine",
        "definition": {"kind": "blend_spine", "blend_surface": surface}
    }))
    .unwrap();
    let owner = ir.geometry.curves.last_mut().unwrap();
    let crate::geometry::CurveGeometry::Solved(solved) = owner.geometry.clone() else {
        panic!("the unit cube's curves are solved");
    };
    owner.geometry = crate::geometry::CurveGeometry::Procedural {
        construction: spine.id.clone(),
        cache: Some(solved),
    };
    ir.geometry.procedural_curves.push(spine.clone());
    let (outcome, _) = run(&mut ir, seed("geometry", "surfaces", &surface));
    assert!(outcome.marked.contains(spine.id.as_str()));
    assert!(!outcome.dropped.contains(spine.id.as_str()));
    let kept = ir
        .geometry
        .procedural_curves
        .iter()
        .find(|kept| kept.id == spine.id)
        .expect("the spine stays");
    assert_eq!(
        crate::hash::finite_json::to_json_value(kept.definition()).unwrap(),
        json!({"kind": "unknown"})
    );
    assert_closed(&ir, &surface, &outcome);
}

#[test]
fn a_name_of_a_lost_feature_keeps_its_native_name_or_is_released() {
    let mut ir = CadIr::empty();
    let base = unresolved("base");
    ir.design.features = vec![base.clone()];
    let generated =
        json!({"kind": "generated", "value": {"kind": "face", "feature": base.id.as_str()}});
    ir.design.names = json::from_value(json!([
        [{"kind": "face", "value": "test:geometry:face#1"},
         {"name": generated, "native": native_name("Face1")}],
        [{"kind": "face", "value": "test:geometry:face#2"}, {"name": generated}]
    ]))
    .unwrap();
    let (outcome, _) = run(&mut ir, seed("design", "features", base.id.as_str()));
    assert!(outcome.dropped.is_empty());
    assert_eq!(
        crate::hash::finite_json::to_json_value(&ir.design.names).unwrap(),
        json!([[{"kind": "face", "value": "test:geometry:face#1"}, {"native": native_name("Face1")}]])
    );
    assert_closed(&ir, base.id.as_str(), &outcome);
}

#[test]
fn a_native_record_that_names_a_lost_record_is_dropped() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let point = ir.geometry.points[0].id.as_str().to_owned();
    let native = "test:native:mark#1";
    let record = crate::NativeRecord::new(
        Identity::new(native).unwrap(),
        [("at".to_owned(), json!(point))].into_iter().collect(),
    )
    .unwrap();
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
        .set_arena_records("marks", vec![record])
        .unwrap();
    let (outcome, _) = run(&mut ir, seed("geometry", "points", &point));
    assert!(outcome.dropped.contains(native));
    assert!(!holds(&ir, native));
    assert_closed(&ir, &point, &outcome);
}

#[test]
fn the_schema_states_each_definition_a_partial_form_keys_on() {
    use super::restate::{
        family, EXPRESSION, NAME_ENTRY, NAME_TABLE, ORDERED_MEMBERS, PICKED, PROCEDURAL_CURVE,
        PROCEDURAL_SURFACE, UNORDERED_MEMBERS, UNORDERED_PICKS,
    };
    let schema: serde_json::Value = serde_json::from_str(crate::CADIR_JSON_SCHEMA).unwrap();
    let definitions = schema["$defs"]
        .as_object()
        .expect("the schema's definitions");
    for keyed in [
        PICKED,
        EXPRESSION,
        PROCEDURAL_CURVE,
        PROCEDURAL_SURFACE,
        NAME_ENTRY,
        UNORDERED_MEMBERS,
        ORDERED_MEMBERS,
        NAME_TABLE,
    ] {
        assert!(
            definitions.keys().any(|name| family(name) == keyed),
            "the CADIR schema has no definition of the family {keyed}"
        );
    }
    assert!(
        definitions.contains_key(UNORDERED_PICKS),
        "the CADIR schema has no definition {UNORDERED_PICKS}"
    );
}

#[test]
fn a_construction_goes_with_the_carrier_that_owns_it() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let spine: crate::geometry::ProceduralCurve = json::from_value(json!({
        "id": "test:geometry:procedural-curve#spine",
        "definition": {"kind": "unknown"}
    }))
    .unwrap();
    let owner = ir.geometry.curves.last_mut().unwrap();
    let crate::geometry::CurveGeometry::Solved(solved) = owner.geometry.clone() else {
        panic!("the unit cube's curves are solved");
    };
    owner.geometry = crate::geometry::CurveGeometry::Procedural {
        construction: spine.id.clone(),
        cache: Some(solved),
    };
    let owner = owner.id.as_str().to_owned();
    ir.geometry.procedural_curves.push(spine.clone());
    let (outcome, _) = run(&mut ir, seed("geometry", "curves", &owner));
    assert!(outcome.dropped.contains(spine.id.as_str()));
    assert!(ir.geometry.procedural_curves.is_empty());
    assert_closed(&ir, &owner, &outcome);
}

/// A carrier that names a given-up construction is stated in the form that
/// holds only what was read (§3.3): its retained cache where it holds one,
/// and `Unknown` otherwise. It keeps its place: the edge on it stays.
#[test]
fn a_carrier_on_a_lost_construction_states_its_retained_cache() {
    use crate::geometry::{CurveGeometry, SolvedCurveGeometry};

    for retained in [true, false] {
        let mut ir = crate::examples::unit_cube().unwrap();
        let spine: crate::geometry::ProceduralCurve = json::from_value(json!({
            "id": "test:geometry:procedural-curve#spine",
            "definition": {"kind": "unknown"}
        }))
        .unwrap();
        let owner = ir.geometry.curves.last_mut().unwrap();
        let CurveGeometry::Solved(solved) = owner.geometry.clone() else {
            panic!("the unit cube's curves are solved");
        };
        owner.geometry = CurveGeometry::Procedural {
            construction: spine.id.clone(),
            cache: retained.then(|| solved.clone()),
        };
        let owner = owner.id.clone();
        ir.geometry.procedural_curves.push(spine.clone());
        let curves = ir.geometry.curves.len();
        let edges = ir.geometry.edges.len();
        let (outcome, _) = run(
            &mut ir,
            seed("geometry", "procedural_curves", spine.id.as_str()),
        );
        assert!(outcome.marked.contains(owner.as_str()), "{outcome:?}");
        assert!(outcome.dropped.is_empty(), "{outcome:?}");
        assert_eq!(ir.geometry.curves.len(), curves);
        assert_eq!(ir.geometry.edges.len(), edges);
        let stated = ir
            .geometry
            .curves
            .iter()
            .find(|curve| curve.id == owner)
            .unwrap();
        let expected = if retained {
            solved
        } else {
            SolvedCurveGeometry::Unknown { record: None }
        };
        assert_eq!(stated.geometry, CurveGeometry::Solved(expected));
        assert_closed(&ir, spine.id.as_str(), &outcome);
    }
}

/// A carrier whose construction stays and whose cache alone names a
/// given-up record loses only its cache (§3.3).
#[test]
fn a_cache_that_names_a_lost_record_is_cleared_and_the_construction_stays() {
    use crate::geometry::{CurveGeometry, SolvedCurveGeometry};

    let mut ir = crate::examples::unit_cube().unwrap();
    let native = "test:native:curve#1";
    let record =
        crate::NativeRecord::new(Identity::new(native).unwrap(), serde_json::Map::new()).unwrap();
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
        .set_arena_records("curves", vec![record])
        .unwrap();
    let spine: crate::geometry::ProceduralCurve = json::from_value(json!({
        "id": "test:geometry:procedural-curve#spine",
        "definition": {"kind": "unknown"}
    }))
    .unwrap();
    let owner = ir.geometry.curves.last_mut().unwrap();
    owner.geometry = CurveGeometry::Procedural {
        construction: spine.id.clone(),
        cache: Some(SolvedCurveGeometry::Unknown {
            record: Some(crate::ids::UnknownId::mint(native).unwrap()),
        }),
    };
    let owner = owner.id.clone();
    ir.geometry.procedural_curves.push(spine.clone());
    let (outcome, _) = run(
        &mut ir,
        Seed {
            arena: Arena::Native {
                codec: "test".to_owned(),
                arena: "curves".to_owned(),
            },
            id: native.to_owned(),
        },
    );
    assert!(outcome.marked.contains(owner.as_str()), "{outcome:?}");
    assert!(outcome.dropped.is_empty(), "{outcome:?}");
    assert_eq!(ir.geometry.procedural_curves, std::slice::from_ref(&spine));
    let stated = ir
        .geometry
        .curves
        .iter()
        .find(|curve| curve.id == owner)
        .unwrap();
    assert_eq!(
        stated.geometry,
        CurveGeometry::Procedural {
            construction: spine.id,
            cache: None,
        }
    );
    assert_closed(&ir, native, &outcome);
}

/// Each Error finding of `ir`, by rule and record.
fn errors(ir: &CadIr) -> BTreeSet<(String, Option<String>)> {
    crate::validate::validate_neutral(ir, Vec::new())
        .expect("validation runs")
        .findings
        .into_iter()
        .filter(|finding| finding.severity >= crate::report::Severity::Error)
        .map(|finding| (finding.rule.as_str().to_owned(), finding.entity))
        .collect()
}

/// A shell whose cage is given up holds no member and states its members
/// unstated (§5.6), and the document holds no cage outside a shell.
#[test]
fn a_shell_that_loses_its_cage_states_its_members_unstated() {
    let mut ir = crate::examples::directed_subd_sum().unwrap();
    let cage = ir.geometry.subds[0].id.clone();
    let holder = ir
        .geometry
        .shells
        .iter()
        .find(|shell| shell.subds().any(|held| *held == cage))
        .map(|shell| shell.id.clone())
        .unwrap();
    let before = errors(&ir);

    let (outcome, _) = run(&mut ir, seed("geometry", "subds", cage.as_str()));
    assert!(outcome.marked.contains(holder.as_str()), "{outcome:?}");
    let shell = ir
        .geometry
        .shells
        .iter()
        .find(|shell| shell.id == holder)
        .expect("the cage's shell stays");
    assert!(shell.members.is_empty());
    assert_eq!(shell.unstated, Some(Unstated::Members));
    assert!(ir.geometry.subds.is_empty());
    let after = errors(&ir);
    assert!(
        after.is_subset(&before),
        "new errors: {:?}",
        after.difference(&before).collect::<Vec<_>>()
    );
}

/// Each finding of `ir` of the checks of `checks`, by rule and record.
fn findings(
    ir: &CadIr,
    checks: &[crate::report::check::Check],
) -> BTreeSet<(String, Option<String>)> {
    crate::validate::validate_neutral(ir, Vec::new())
        .expect("validation runs")
        .findings
        .into_iter()
        .filter(|finding| checks.contains(&finding.check))
        .map(|finding| (finding.rule.as_str().to_owned(), finding.entity))
        .collect()
}

#[track_caller]
fn assert_no_dangling_reference(ir: &CadIr) {
    let dangling = findings(ir, &[crate::report::check::Check::ReferentialIntegrity]);
    assert!(dangling.is_empty(), "{dangling:#?}");
}

/// The design document whose evaluation `after-extrude` stores a delta
/// that holds its own version of the cube's first face, a face of its own
/// on the cube's shell, and the removal of that shell, with the ids of the
/// shell, the first face, and the delta's own face.
fn stored_state() -> (CadIr, String, String, String) {
    use crate::design::refs::RecordRef;
    let mut ir = crate::validate::design::fixture::document();
    let face = ir.geometry.faces[0].clone();
    let shell = face.shell.clone();
    let own = crate::topology::Face {
        id: crate::ids::FaceId::mint("test:design:face#delta").unwrap(),
        loops: FaceLoops::PartialUnspecified { loops: Vec::new() },
        ..face.clone()
    };
    let delta = &mut ir.design.evaluations[0]
        .content
        .as_mut()
        .expect("the fixture stores the state")
        .delta;
    delta.records.faces = vec![face.clone(), own.clone()];
    delta.removed = vec![RecordRef::Shell(shell.clone())];
    ir.finalize();
    (
        ir,
        shell.as_str().to_owned(),
        face.id.as_str().to_owned(),
        own.id.as_str().to_owned(),
    )
}

fn after_extrude(ir: &CadIr) -> &crate::design::evaluation::Evaluation {
    ir.design
        .evaluations
        .iter()
        .find(|evaluation| evaluation.id.as_str() == "test:design:evaluation#after-extrude")
        .expect("the evaluation stays")
}

fn delta(ir: &CadIr) -> &crate::design::evaluation::StateDelta {
    &after_extrude(ir)
        .content
        .as_ref()
        .expect("the evaluation keeps its stored state")
        .delta
}

/// A stored state loses only the records of its delta that need a given-up
/// id, and keeps each record of its own of that id (§3.3, §5.4).
#[test]
fn a_stored_state_loses_only_what_of_its_delta_needs_a_given_up_record() {
    let (mut ir, shell, face, own) = stored_state();
    assert_no_dangling_reference(&ir);
    let (outcome, _) = run(&mut ir, seed("geometry", "shells", &shell));
    let evaluation = after_extrude(&ir).id.as_str().to_owned();
    assert!(!outcome.dropped.contains(&evaluation), "{outcome:?}");
    assert!(outcome.marked.contains(&evaluation), "{outcome:?}");
    assert!(!outcome.dropped.contains(&own), "{outcome:?}");
    assert!(
        outcome.states[&state_path(&evaluation)].0.contains(&own),
        "{outcome:?}"
    );
    assert!(delta(&ir).records.faces.is_empty());
    assert!(delta(&ir).removed.is_empty());
    assert_no_dangling_reference(&ir);

    let (mut ir, _, face_id, _) = stored_state();
    let (outcome, _) = run(&mut ir, seed("geometry", "faces", &face_id));
    let evaluation = after_extrude(&ir).id.as_str().to_owned();
    assert!(!outcome.dropped.contains(&evaluation), "{outcome:?}");
    let held: BTreeSet<&str> = delta(&ir)
        .records
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .collect();
    assert_eq!(held, BTreeSet::from([face.as_str(), own.as_str()]));
    assert_eq!(delta(&ir).removed.len(), 1);
    assert_no_dangling_reference(&ir);
}

/// A configuration and an arrangement that override a given-up parameter
/// lose that override: their overrides are keyed by their members' own ids
/// (§3.3).
#[test]
fn an_override_keyed_by_a_given_up_record_is_released() {
    use crate::design::configuration::{
        Arrangement, ArrangementKind, Overrides, ParameterOverride,
    };
    use crate::design::expr::{Expression, Neutral, Node};
    use crate::validate::design::fixture::{id, parameter};

    let mut ir = crate::validate::design::fixture::document();
    let length = parameter("length", crate::design::units::ValueKind::Boolean);
    let overrides = || Overrides {
        parameters: [(
            length.id.clone(),
            ParameterOverride {
                expression: Some(Expression::Neutral(Neutral(Node::Boolean(true)))),
                value: None,
            },
        )]
        .into_iter()
        .collect(),
        ..Overrides::default()
    };
    for configuration in &mut ir.design.configurations {
        configuration.overrides = overrides();
    }
    ir.design.arrangements.push(Arrangement {
        id: id("arrangement", "shown"),
        scope: id("definition", "part"),
        name: cadmpeg_core::text::NonBlankString::new("shown").unwrap(),
        kind: ArrangementKind::Display,
        configuration: None,
        overrides: overrides(),
        appearances: Vec::new(),
        moves: Vec::new(),
        nested: Vec::new(),
        native_ref: None,
    });
    ir.design.parameters.push(length.clone());
    ir.finalize();
    assert_no_dangling_reference(&ir);

    let (outcome, _) = run(&mut ir, seed("design", "parameters", length.id.as_str()));
    assert!(outcome.dropped.is_empty(), "{outcome:?}");
    let holders: BTreeSet<String> = ir
        .design
        .configurations
        .iter()
        .map(|configuration| configuration.id.as_str().to_owned())
        .chain(
            ir.design
                .arrangements
                .iter()
                .map(|arrangement| arrangement.id.as_str().to_owned()),
        )
        .collect();
    assert_eq!(holders.len(), 3);
    assert_eq!(outcome.marked, holders);
    assert!(ir
        .design
        .configurations
        .iter()
        .all(|configuration| configuration.overrides.parameters.is_empty()));
    assert!(ir.design.arrangements[0].overrides.parameters.is_empty());
    assert_no_dangling_reference(&ir);
}

/// A carrier that the give-up leaves with no user is dropped with the
/// records that needed the given-up record.
#[test]
fn a_carrier_left_with_no_user_is_dropped() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let face = ir.geometry.faces[0].clone();
    let checks = [
        crate::report::check::Check::ReferentialIntegrity,
        crate::report::check::Check::CarrierReachability,
    ];
    assert!(findings(&ir, &checks).is_empty());

    let (outcome, _) = run(&mut ir, seed("geometry", "faces", face.id.as_str()));
    assert!(
        outcome.dropped.contains(face.surface.as_str()),
        "{outcome:?}"
    );
    assert!(!holds(&ir, face.surface.as_str()));
    let after = findings(&ir, &checks);
    assert!(after.is_empty(), "{after:#?}");
}

/// The path in a loss of the stored state `evaluation`.
fn state_path(evaluation: &str) -> String {
    format!("design.evaluations/{evaluation}")
}

/// A scope that loses one of its roots keeps its other roots in their
/// order (§5.2). A scope left with no root and no record that names it is
/// dropped (§5.1).
#[test]
fn a_scope_that_loses_a_root_keeps_its_other_roots() {
    use crate::design::container::{Container, UnorderedKind};
    use crate::design::scope::DesignScope;
    use crate::validate::design::fixture::id;

    let group = |key: &str| Container::Unordered {
        id: id("container", key),
        kind: UnorderedKind::Group,
        members: BTreeSet::new(),
    };
    let mut ir = crate::validate::design::fixture::document();
    let part: String = id("definition", "part");
    let timeline: String = id("container", "timeline");
    let shown: String = id("container", "shown");
    let features = ir.design.features.clone();
    ir.design.containers.push(group("shown"));
    ir.design
        .scopes
        .iter_mut()
        .find(|scope| scope.definition.as_str() == part)
        .expect("the fixture's scope")
        .roots
        .push(id("container", "shown"));
    ir.finalize();
    assert_no_dangling_reference(&ir);

    let (outcome, _) = run(&mut ir, seed("design", "containers", &shown));
    assert!(outcome.dropped.is_empty(), "{outcome:?}");
    assert_eq!(outcome.marked, BTreeSet::from([part.clone()]));
    let scope = ir
        .design
        .scopes
        .iter()
        .find(|scope| scope.definition.as_str() == part)
        .expect("the scope stays");
    let roots: Vec<&str> = scope
        .roots
        .iter()
        .map(crate::design::ids::ContainerId::as_str)
        .collect();
    assert_eq!(roots, [timeline.as_str()]);
    assert_eq!(ir.design.features, features);
    assert_no_dangling_reference(&ir);

    let mut ir = crate::validate::design::fixture::document();
    let other: String = id("definition", "other");
    ir.design.containers.push(group("other"));
    ir.design.scopes.push(DesignScope {
        definition: id("definition", "other"),
        roots: vec![id("container", "other")],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir.finalize();
    let (outcome, _) = run(
        &mut ir,
        seed("design", "containers", &id::<String>("container", "other")),
    );
    assert_eq!(outcome.dropped, BTreeSet::from([other.clone()]));
    assert!(outcome.marked.is_empty(), "{outcome:?}");
    assert!(ir
        .design
        .scopes
        .iter()
        .all(|scope| scope.definition.as_str() != other));
}

/// The edges of the loops of `face`.
fn edges_of(ir: &CadIr, face: &str) -> BTreeSet<String> {
    let loops: BTreeSet<&str> = ir
        .geometry
        .loops
        .iter()
        .filter(|held| held.face.as_str() == face)
        .map(|held| held.id.as_str())
        .collect();
    ir.geometry
        .coedges
        .iter()
        .filter(|coedge| loops.contains(coedge.owner_loop.as_str()))
        .map(|coedge| coedge.edge.as_str().to_owned())
        .collect()
}

/// A record a delta drops is the state's own: the loss names it under its
/// state, and the document's record of the same id keeps its own mark
/// (§3.3, §5.4).
#[test]
fn a_record_a_delta_drops_leaves_the_document_record_of_its_id() {
    let mut ir = crate::validate::design::fixture::document();
    let face = ir.geometry.faces[0].clone();
    let edges = edges_of(&ir, face.id.as_str());
    let neighbour = ir
        .geometry
        .faces
        .iter()
        .find(|other| other.id != face.id && !edges_of(&ir, other.id.as_str()).is_disjoint(&edges))
        .expect("a face that shares an edge")
        .clone();
    ir.design.evaluations[0]
        .content
        .as_mut()
        .expect("the fixture stores the state")
        .delta
        .records
        .faces = vec![crate::topology::Face {
        surface: neighbour.surface.clone(),
        ..face.clone()
    }];
    ir.finalize();

    let (outcome, _) = run(
        &mut ir,
        seed("geometry", "surfaces", neighbour.surface.as_str()),
    );
    let evaluation = after_extrude(&ir).id.as_str().to_owned();
    let id = face.id.as_str().to_owned();
    assert!(
        outcome.dropped.contains(neighbour.id.as_str()),
        "{outcome:?}"
    );
    assert!(!outcome.dropped.contains(&id), "{outcome:?}");
    assert!(outcome.marked.contains(&id), "{outcome:?}");
    assert!(outcome.marked.contains(&evaluation), "{outcome:?}");
    let (dropped, _) = &outcome.states[&state_path(&evaluation)];
    assert_eq!(dropped, &BTreeSet::from([id.clone()]));
    assert!(ir.geometry.faces.iter().any(|held| held.id == face.id));
    assert!(delta(&ir).records.faces.is_empty());
    assert_no_dangling_reference(&ir);
}

/// A carrier of a delta that the delta's drops leave with no user is
/// dropped with them and named under its state.
#[test]
fn a_carrier_a_delta_leaves_with_no_user_is_dropped() {
    let (mut ir, shell, face, own) = stored_state();
    let carrier = crate::geometry::Surface {
        id: crate::ids::SurfaceId::mint("test:design:surface#delta").unwrap(),
        ..ir.geometry.surfaces[0].clone()
    };
    let records = &mut ir.design.evaluations[0]
        .content
        .as_mut()
        .expect("the fixture stores the state")
        .delta
        .records;
    records.surfaces = vec![carrier.clone()];
    for held in &mut records.faces {
        if held.id.as_str() == own {
            held.surface = carrier.id.clone();
        }
    }
    ir.finalize();

    let (outcome, _) = run(&mut ir, seed("geometry", "shells", &shell));
    let evaluation = after_extrude(&ir).id.as_str().to_owned();
    let (dropped, _) = &outcome.states[&state_path(&evaluation)];
    assert_eq!(
        dropped,
        &BTreeSet::from([face, own, carrier.id.as_str().to_owned()])
    );
    assert!(!outcome.dropped.contains(carrier.id.as_str()));
    assert!(delta(&ir).records.surfaces.is_empty());
    let naming: Vec<String> = crate::validate::validate_neutral(&ir, Vec::new())
        .expect("validation runs")
        .findings
        .into_iter()
        .map(|finding| finding.message)
        .filter(|message| message.contains(carrier.id.as_str()))
        .collect();
    assert!(naming.is_empty(), "{naming:#?}");
}

/// The validation findings on resolved states.
fn resolved_findings(ir: &CadIr) -> BTreeSet<String> {
    crate::validate::validate_neutral(ir, Vec::new())
        .expect("validation runs")
        .findings
        .into_iter()
        .map(|finding| finding.message)
        .filter(|message| message.contains("resolved state"))
        .collect()
}

/// Asserts that the fixture's stored state lists every record of the
/// document geometry as removed, and keeps no entry that names one: the
/// geometry's one body reaches each of its records, so a state without that
/// body holds none of them (§5.4, §5.6, §8.1).
fn assert_base_withdrawn(ir: &CadIr) {
    let ids = crate::design::edit::states::ids(&ir.geometry);
    let removed: BTreeSet<String> = delta(ir)
        .removed
        .iter()
        .map(|record| crate::design::edit::states::record_ref_id(record).to_owned())
        .collect();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(removed, ids);
    assert_eq!(delta(ir).removed.len(), ids.len());
    assert!(delta(ir).owners.is_empty(), "{:?}", delta(ir).owners);
    assert!(delta(ir).members.is_empty(), "{:?}", delta(ir).members);
    assert!(delta(ir).instance_bodies.is_empty());
    for (entity, entry) in &delta(ir).names {
        assert!(
            !ids.contains(entity.id()) || entry.is_none(),
            "the state names the withdrawn {entity:?}"
        );
    }
    for entity in ir.design.names.entries().keys() {
        if ids.contains(entity.id()) {
            assert_eq!(delta(ir).names.get(entity), Some(&None), "{entity:?}");
        }
    }
}

/// An owner entry of a delta that names a given-up definition takes its body
/// out of the state: the delta drops its own body, lists the base body and
/// every record only it reaches as removed, and keeps no owner entry of
/// either (§5.4, §5.6, §8.1).
#[test]
fn an_owner_entry_of_a_given_up_definition_takes_its_body_out_of_the_state() {
    use crate::ids::{BodyId, ProductDefinitionId};
    use crate::products::{ProductDefinition, Storage};
    use crate::validate::design::fixture::id;

    let mut ir = crate::validate::design::fixture::document();
    let other: ProductDefinitionId = id("definition", "other");
    ir.model
        .product_definitions
        .push(ProductDefinition::new(other.clone(), Storage::Embedded));
    let base = ir.geometry.bodies[0].id.clone();
    let own = crate::topology::Body {
        id: BodyId::mint("test:design:body#delta").unwrap(),
        ..ir.geometry.bodies[0].clone()
    };
    let stored = &mut ir.design.evaluations[0]
        .content
        .as_mut()
        .expect("the fixture stores the state")
        .delta;
    stored.records.bodies = vec![own.clone()];
    stored.owners = BTreeMap::from([
        (base.clone(), Some(other.clone())),
        (own.id.clone(), Some(other.clone())),
    ]);
    ir.finalize();
    assert_no_dangling_reference(&ir);
    let before = resolved_findings(&ir);

    let (outcome, _) = run(
        &mut ir,
        seed("model", "product_definitions", other.as_str()),
    );
    let evaluation = after_extrude(&ir).id.as_str().to_owned();
    assert!(outcome.dropped.is_empty(), "{outcome:?}");
    assert_eq!(outcome.marked, BTreeSet::from([evaluation.clone()]));
    let (dropped, _) = &outcome.states[&state_path(&evaluation)];
    assert!(dropped.contains(own.id.as_str()), "{dropped:?}");
    assert!(dropped.contains(base.as_str()), "{dropped:?}");
    assert!(delta(&ir).records.bodies.is_empty());
    assert_base_withdrawn(&ir);
    assert!(resolved_findings(&ir).is_subset(&before));
    assert_no_dangling_reference(&ir);
}

/// A member entry of a delta that names a given-up definition, for an
/// occurrence that stays, has no form: the state holds no occurrence without
/// its owner (§8.1), and no delta states that an occurrence leaves a state
/// (§5.4, §6.7). The give-up drops the whole stored state and names the
/// occurrence in its loss.
#[test]
fn a_member_entry_of_a_given_up_definition_drops_the_state() {
    use crate::design::selection::{OccurrencePath, PathStep, StepRef};
    use crate::ids::{OccurrenceId, ProductDefinitionId};
    use crate::products::{ProductDefinition, Storage};
    use crate::validate::design::fixture::id;

    let mut ir = crate::validate::design::fixture::document();
    let other: ProductDefinitionId = id("definition", "other");
    ir.model
        .product_definitions
        .push(ProductDefinition::new(other.clone(), Storage::Embedded));
    let base = ir.geometry.bodies[0].id.clone();
    let occurrence: OccurrenceId = id("occurrence", "member");
    let path = OccurrencePath(vec![PathStep {
        occurrence: StepRef::Occurrence(occurrence.clone()),
        configuration: None,
    }]);
    let stored = &mut ir.design.evaluations[0]
        .content
        .as_mut()
        .expect("the fixture stores the state")
        .delta;
    stored.members = BTreeMap::from([(occurrence.clone(), Some(other.clone()))]);
    stored.instance_bodies = BTreeMap::from([(path, Some(vec![base]))]);
    ir.finalize();
    let evaluation = after_extrude(&ir).id.as_str().to_owned();

    let (outcome, _) = run(
        &mut ir,
        seed("model", "product_definitions", other.as_str()),
    );
    assert!(outcome.dropped.contains(&evaluation), "{outcome:?}");
    assert!(!ir
        .design
        .evaluations
        .iter()
        .any(|stored| stored.id.as_str() == evaluation));
    assert!(
        outcome.tail.contains(&format!(
            "state {evaluation} dropped: its delta cannot state that [{}] leave it",
            occurrence.as_str()
        )),
        "{}",
        outcome.tail
    );
    assert_closed(&ir, other.as_str(), &outcome);
}
