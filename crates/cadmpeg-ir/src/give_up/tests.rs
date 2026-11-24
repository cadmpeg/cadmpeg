// SPDX-License-Identifier: Apache-2.0
//! The give-up of a record: the part it leaves, the records it drops, and
//! the records it states in a partial form.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use serde_json::json;

use super::{give_up, Arena, Seed};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use crate::document::CadIr;
use crate::ids::Identity;
use crate::report::loss::LossNote;
use crate::source_fidelity::SourceFidelity;
use crate::topology::{FaceLoops, RegionUnstated, Shell, Unstated};
use crate::unknown::part::record_unreadable_kind;
use crate::NativeUnknownRecord;

const CODEC: &str = "test";

/// What one give-up left: its stub, and the records it dropped and marked.
#[derive(Debug, Default)]
struct Outcome {
    stub: String,
    path: String,
    dropped: BTreeSet<String>,
    marked: BTreeSet<String>,
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
    ir.references().held.contains_key(id)
}

#[track_caller]
fn assert_closed(ir: &CadIr, seed: &str, outcome: &Outcome) {
    let gone = gone(ir, seed, outcome);
    let document = crate::hash::finite_json::to_json_value(ir).unwrap();
    let mut names = Vec::new();
    crate::reference_walk::visit_edges(
        &document,
        "",
        true,
        &|text| gone.get(text).map(String::as_str).into_iter().collect(),
        &mut |path, target: &str| {
            names.push(format!("{path} -> {target}"));
            true
        },
    );
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
    let text = crate::hash::finite_json::to_canonical_json_string(&point).unwrap();
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
            operation: serde_json::from_value(operation).unwrap(),
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
