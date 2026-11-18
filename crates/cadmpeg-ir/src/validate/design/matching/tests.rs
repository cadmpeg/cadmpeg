// SPDX-License-Identifier: Apache-2.0
//! The matching rule of §5.7 on the fixture's cube: `Bounded` and `Meeting`
//! names through the state's adjacency, and the narrowing by `index`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::super::fixture::{design_findings, document};
use super::super::selections::Stored;
use super::super::states::States;
use super::Matchers;
use crate::design::configuration::Publication;
use crate::design::evaluation::StateBase;
use crate::design::names::{NameEntry, NameTable, TopoName};
use crate::design::refs::{EntityRef, RecordRef};
use crate::design::selection::{PickTarget, Picked};
use crate::document::CadIr;
use crate::ids::FaceId;
use crate::report::Severity;

fn face_name(face: usize, index: Option<u32>) -> TopoName {
    let mut value = json!({"kind": "face", "feature": "test:design:feature#extrude", "role": {"kind": "other", "value": format!("test:face{face}")}});
    if let Some(index) = index {
        value["index"] = json!(index);
    }
    serde_json::from_value(json!({"kind": "generated", "value": value})).unwrap()
}

fn bounded(faces: &[TopoName], ends: &[TopoName]) -> TopoName {
    TopoName::Bounded {
        faces: faces.iter().cloned().collect(),
        ends: ends.iter().cloned().collect(),
        index: None,
    }
}

fn meeting(faces: &[TopoName]) -> TopoName {
    TopoName::Meeting {
        faces: faces.iter().cloned().collect(),
        index: None,
    }
}

/// The cube with each face named `face_name(i, None)` by its position.
fn named() -> CadIr {
    let mut ir = document();
    let entries: BTreeMap<EntityRef, NameEntry> = ir
        .geometry
        .faces
        .iter()
        .enumerate()
        .map(|(at, face)| {
            (
                EntityRef::Face(face.id.clone()),
                NameEntry {
                    name: Some(face_name(at, None)),
                    native: None,
                },
            )
        })
        .collect();
    ir.design.names = NameTable::from(entries);
    ir
}

/// The positions of the faces that use each edge, by edge position.
fn edge_faces(ir: &CadIr) -> Vec<BTreeSet<usize>> {
    let face_at: BTreeMap<&FaceId, usize> = ir
        .geometry
        .faces
        .iter()
        .enumerate()
        .map(|(at, face)| (&face.id, at))
        .collect();
    ir.geometry
        .edges
        .iter()
        .map(|edge| {
            ir.geometry
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
                .filter_map(|coedge| {
                    let record = ir.geometry.loops.iter().find(|record| record.id == coedge.owner_loop)?;
                    face_at.get(&record.face).copied()
                })
                .collect()
        })
        .collect()
}

/// Two faces that share an edge, and a face that shares none with the first.
fn adjacent_and_opposite(ir: &CadIr) -> (usize, usize, usize) {
    let sides = edge_faces(ir);
    let pair: Vec<usize> = sides[0].iter().copied().collect();
    let (first, second) = (pair[0], pair[1]);
    let opposite = (0..ir.geometry.faces.len())
        .find(|face| {
            *face != first
                && !sides
                    .iter()
                    .any(|side| side.contains(&first) && side.contains(face))
        })
        .unwrap();
    (first, second, opposite)
}

fn publish(ir: &mut CadIr, name: TopoName) {
    ir.design.publications.push(Publication {
        id: super::super::fixture::id("publication", "edge"),
        scope: super::super::fixture::id("definition", "part"),
        name: cadmpeg_core::text::NonBlankString::new("edge").unwrap(),
        target: Picked {
            target: PickTarget::Entity(name),
            context: None,
            native: None,
            domain: None,
            witness: None,
            resolved: None,
        },
    });
}

fn lost(ir: &CadIr) -> bool {
    design_findings(ir).iter().any(|finding| {
        finding.severity == Severity::Warning && finding.message.contains("a lost operand")
    })
}

#[track_caller]
fn assert_matches(ir: &CadIr, name: &TopoName, expected: bool) {
    let states = States::new(ir);
    let matchers = Matchers::new(&states);
    let extents = states.extents.iter().collect();
    let matcher = matchers.get(&Stored::Extents(extents)).unwrap();
    assert_eq!(matcher.matches_any(name), expected, "{name:?}");
    let evaluation = ir.design.evaluations[0].id.as_str();
    let matcher = matchers.get(&Stored::Evaluation(evaluation)).unwrap();
    assert_eq!(matcher.matches_any(name), expected, "{name:?} in {evaluation}");
}

#[test]
fn a_bounded_name_matches_the_edge_between_its_faces() {
    let ir = named();
    let (first, second, opposite) = adjacent_and_opposite(&ir);
    let edge = bounded(&[face_name(first, None), face_name(second, None)], &[]);
    assert_matches(&ir, &edge, true);
    let states = States::new(&ir);
    let matchers = Matchers::new(&states);
    let matcher = matchers
        .get(&Stored::Extents(states.extents.iter().collect()))
        .unwrap();
    assert_eq!(
        matcher.matched(&edge).into_iter().collect::<Vec<_>>(),
        vec![ir.geometry.edges[0].id.as_str()]
    );
    assert_matches(
        &ir,
        &bounded(&[face_name(first, None), face_name(opposite, None)], &[]),
        false,
    );

    let mut published = ir.clone();
    publish(&mut published, edge);
    assert!(!lost(&published), "{:#?}", design_findings(&published));
    let mut published = ir;
    publish(
        &mut published,
        bounded(&[face_name(first, None), face_name(opposite, None)], &[]),
    );
    assert!(lost(&published), "{:#?}", design_findings(&published));
}

#[test]
fn ends_narrow_only_among_several_candidates_and_ignore_unknown_faces() {
    let ir = named();
    let (first, second, opposite) = adjacent_and_opposite(&ir);
    let one_side = bounded(&[face_name(first, None)], &[]);
    let states = States::new(&ir);
    let matchers = Matchers::new(&states);
    let matcher = matchers
        .get(&Stored::Extents(states.extents.iter().collect()))
        .unwrap();
    assert_eq!(matcher.matched(&one_side).len(), 4);
    let narrowed = bounded(&[face_name(first, None)], &[face_name(second, None)]);
    assert_eq!(matcher.matched(&narrowed).len(), 3);
    let unknown = bounded(&[face_name(first, None)], &[face_name(99, None)]);
    assert_eq!(matcher.matched(&unknown).len(), 4);
    let single = bounded(
        &[face_name(first, None), face_name(second, None)],
        &[face_name(opposite, None)],
    );
    assert_eq!(matcher.matched(&single).len(), 1);
}

#[test]
fn a_meeting_name_matches_the_vertex_where_its_faces_meet() {
    let ir = named();
    let sides = edge_faces(&ir);
    let (first, second, opposite) = adjacent_and_opposite(&ir);
    let third = (0..ir.geometry.faces.len())
        .find(|face| {
            ![first, second, opposite].contains(face)
                && sides.iter().any(|side| side.contains(&first) && side.contains(face))
                && sides.iter().any(|side| side.contains(&second) && side.contains(face))
        })
        .unwrap();
    let corner = meeting(&[
        face_name(first, None),
        face_name(second, None),
        face_name(third, None),
    ]);
    assert_matches(&ir, &corner, true);
    let states = States::new(&ir);
    let matchers = Matchers::new(&states);
    let matcher = matchers
        .get(&Stored::Extents(states.extents.iter().collect()))
        .unwrap();
    assert_eq!(matcher.matched(&corner).len(), 1);
    assert_matches(
        &ir,
        &meeting(&[face_name(first, None), face_name(opposite, None)]),
        false,
    );
}

#[test]
fn an_index_narrows_only_where_an_entry_states_one() {
    let mut ir = named();
    let unindexed = face_name(0, Some(3));
    assert_matches(&ir, &unindexed, true);

    let mut entries = ir.design.names.entries().clone();
    for (at, entry) in entries.values_mut().take(2).enumerate() {
        entry.name = Some(face_name(0, Some(u32::try_from(at).unwrap())));
    }
    ir.design.names = entries.into();
    assert_matches(&ir, &face_name(0, Some(1)), true);
    assert_matches(&ir, &face_name(0, Some(3)), false);
    assert_matches(&ir, &face_name(0, None), true);
}

#[test]
fn a_nested_index_is_matched_under_either_reading() {
    let mut ir = named();
    let modified = |index| TopoName::Modified {
        feature: super::super::fixture::feature("fillet"),
        from: Box::new(face_name(0, Some(index))),
    };
    let mut entries = ir.design.names.entries().clone();
    entries.values_mut().next().unwrap().name = Some(modified(1));
    ir.design.names = entries.into();
    assert_matches(&ir, &modified(1), true);
    assert_matches(&ir, &modified(2), true);
    let states = States::new(&ir);
    let matchers = Matchers::new(&states);
    let matcher = matchers
        .get(&Stored::Extents(states.extents.iter().collect()))
        .unwrap();
    assert!(matcher.matched(&modified(2)).is_empty());
    assert_eq!(matcher.matched(&modified(1)).len(), 1);
}

#[test]
fn a_face_a_delta_removes_bounds_no_edge_in_that_state() {
    let mut ir = named();
    let (first, second, _) = adjacent_and_opposite(&ir);
    let removed = ir.geometry.faces[second].id.clone();
    let content = ir.design.evaluations[0].content.as_mut().unwrap();
    assert!(matches!(content.base, StateBase::Current(_)));
    content.delta.removed.push(RecordRef::Face(removed));
    let edge = bounded(&[face_name(first, None), face_name(second, None)], &[]);
    let states = States::new(&ir);
    let matchers = Matchers::new(&states);
    let current = matchers
        .get(&Stored::Extents(states.extents.iter().collect()))
        .unwrap();
    assert!(current.matches_any(&edge));
    let evaluation = matchers
        .get(&Stored::Evaluation(ir.design.evaluations[0].id.as_str()))
        .unwrap();
    assert!(!evaluation.matches_any(&edge));
}
