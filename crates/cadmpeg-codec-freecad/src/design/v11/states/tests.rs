// SPDX-License-Identifier: Apache-2.0
//! Body states as evaluations, with full-snapshot ids (§5.4).

use std::io::Cursor;

use cadmpeg_ir::design::evaluation::StateBase;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position, RecordRef};
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use super::{snapshot, Base, Resolved};
use crate::FcstdCodec;

fn topology(value: &serde_json::Value) -> Topology {
    let mut wire = serde_json::Map::new();
    for (arena, _) in super::ARENAS {
        wire.insert(arena.to_owned(), serde_json::Value::Array(Vec::new()));
    }
    for (arena, records) in value.as_object().unwrap() {
        wire.insert(arena.clone(), records.clone());
    }
    cadmpeg_core::serde_erase::json::from_value(serde_json::Value::Object(wire)).unwrap()
}

/// A body of one region and one shell, and the points `points`, all keyed by
/// `key`.
fn state(key: &str, points: &[(&str, f64)]) -> Topology {
    let id = |kind: &str, child: &str| format!("fcstd:model:{kind}#{key}:{child}");
    topology(&serde_json::json!({
        "bodies": [{"id": id("body", "1"), "kind": "solid"}],
        "regions": [{"id": id("region", "1"), "body": id("body", "1"), "shells": [id("shell", "1")]}],
        "shells": [{"id": id("shell", "1"), "region": id("region", "1"), "members": []}],
        "points": points
            .iter()
            .map(|(child, x)| serde_json::json!({"id": id("point", child), "position": {"x": x, "y": 0.0, "z": 0.0}}))
            .collect::<Vec<_>>(),
    }))
}

#[test]
fn a_state_keeps_its_base_body_by_position_and_shares_equal_records() {
    let first = snapshot(&state("Pad:Shape:A.brp", &[("1", 0.0), ("2", 1.0)]), None).unwrap();
    assert_eq!(first.records.bodies.len(), 1);
    assert_eq!(first.records.points.len(), 2);
    assert!(first.removed.is_empty());
    let base = Base {
        reference: StateBase::Empty,
        resolved: first.resolved,
    };
    let second = snapshot(
        &state("Pocket:Shape:B.brp", &[("1", 1.0), ("2", 2.0)]),
        Some(&base),
    )
    .unwrap();
    // The body, its region, and its shell keep the base's ids and equal its
    // records, so they are shared; the point at x = 1 equals the base's
    // second point and is shared under its id.
    assert!(second.records.bodies.is_empty());
    assert!(second.records.regions.is_empty());
    assert!(second.records.shells.is_empty());
    let [new] = second.records.points.as_slice() else {
        panic!("{:?}", second.records.points)
    };
    assert_eq!(new.id.as_str(), "fcstd:model:point#Pocket:Shape:B.brp:2");
    assert!(second.renamed.contains(&(
        "fcstd:model:point#Pocket:Shape:B.brp:1".to_owned(),
        "fcstd:model:point#Pad:Shape:A.brp:2".to_owned()
    )));
    assert!(second.renamed.contains(&(
        "fcstd:model:body#Pocket:Shape:B.brp:1".to_owned(),
        "fcstd:model:body#Pad:Shape:A.brp:1".to_owned()
    )));
    let [RecordRef::Point(removed)] = second.removed.as_slice() else {
        panic!("{:?}", second.removed)
    };
    assert_eq!(removed.as_str(), "fcstd:model:point#Pad:Shape:A.brp:1");
}

/// A record equal to a base record but for `source_object` is shared and
/// keeps the base's `source_object` (§5.4, Full snapshots).
#[test]
fn a_record_that_differs_only_in_its_source_object_is_shared() {
    let with_source = |key: &str, object: &str| {
        let mut topology = state(key, &[("1", 3.0)]);
        let mut wire = cadmpeg_core::serde_erase::json::to_value(&topology).unwrap();
        wire["points"][0]["source_object"] = serde_json::json!({
            "format": "fcstd",
            "object_id": format!("fcstd:native:object#{object}"),
            "instance_path": [],
        });
        topology = cadmpeg_core::serde_erase::json::from_value(wire).unwrap();
        topology
    };
    let first = snapshot(&with_source("Pad:Shape:A.brp", "Pad"), None).unwrap();
    let base = Base {
        reference: StateBase::Empty,
        resolved: first.resolved,
    };
    let second = snapshot(&with_source("Pocket:Shape:B.brp", "Pocket"), Some(&base)).unwrap();
    assert!(
        second.records.points.is_empty(),
        "{:?}",
        second.records.points
    );
    assert!(second.removed.is_empty());
    let kept = &second.resolved.arenas["points"]["fcstd:model:point#Pad:Shape:A.brp:1"];
    assert_eq!(
        kept["source_object"]["object_id"],
        "fcstd:native:object#Pad"
    );
}

/// The cylinder `key:1`, its basis `key:1:basis`, and, where `subset`, the
/// construction `key:1:construction` that bounds the basis to it.
fn cylinder(key: &str, subset: bool) -> Topology {
    let surface = |child: &str| format!("fcstd:model:surface#{key}:{child}");
    let geometry = serde_json::json!({"kind": "solved", "value": {"kind": "cylinder",
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0}, "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
        "ref_direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "radius": 2.0}});
    let constructions = if subset {
        vec![
            serde_json::json!({"id": surface("1:construction"), "surface": surface("1"),
            "definition": {"kind": "subset", "support": surface("1:basis"),
                "parameter_ranges": [[0.0, 3.0], [0.0, 5.0]]}}),
        ]
    } else {
        Vec::new()
    };
    topology(&serde_json::json!({
        "surfaces": [{"id": surface("1"), "geometry": geometry},
                     {"id": surface("1:basis"), "geometry": geometry}],
        "procedural_surfaces": constructions,
    }))
}

/// A carrier matches a base carrier only with the constructions that
/// produce it, so a shared carrier keeps its construction in the state.
#[test]
fn a_carrier_is_shared_with_the_constructions_that_produce_it() {
    let first = snapshot(&cylinder("Pad:Shape:A.brp", true), None).unwrap();
    let base = Base {
        reference: StateBase::Empty,
        resolved: first.resolved,
    };
    let bounded = snapshot(&cylinder("Mirrored:Shape:B.brp", true), Some(&base)).unwrap();
    assert!(bounded.records.surfaces.is_empty());
    assert!(bounded.records.procedural_surfaces.is_empty());
    assert!(bounded.removed.is_empty());
    let plain = snapshot(&cylinder("Mirrored:Shape:B.brp", false), Some(&base)).unwrap();
    let construction = "fcstd:model:surface#Pad:Shape:A.brp:1:construction";
    assert!(plain.removed.iter().any(|removed| matches!(
        removed,
        RecordRef::ProceduralSurface(id) if id.as_str() == construction
    )));
    assert!(
        !plain.resolved.arenas["surfaces"].contains_key("fcstd:model:surface#Pad:Shape:A.brp:1")
    );
    assert!(plain.resolved.arenas["surfaces"]
        .contains_key("fcstd:model:surface#Pad:Shape:A.brp:1:basis"));
}

#[test]
fn an_empty_base_shares_nothing() {
    let empty = Base {
        reference: StateBase::Empty,
        resolved: Resolved::default(),
    };
    let state = snapshot(&state("Pad:Shape:A.brp", &[("1", 0.0)]), Some(&empty)).unwrap();
    assert_eq!(state.records.bodies.len(), 1);
    assert_eq!(state.records.points.len(), 1);
    assert!(state.renamed.iter().all(|(old, new)| old == new));
}

fn decode(name: &str) -> CadIr {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/freecad_fcstd/fixtures")
            .join(name),
    )
    .unwrap();
    FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

fn feature(name: &str) -> FeatureId {
    FeatureId::mint(format!("fcstd:design:feature#{name}")).unwrap()
}

#[test]
fn every_stored_body_state_but_the_tip_is_an_evaluation_on_its_nearest_earlier_state() {
    let ir = decode("core_operations.FCStd");
    let after = |name: &str| Evaluated::State(Position::After(Member::Feature(feature(name))));
    let evaluation = |of: &Evaluated| {
        ir.design
            .evaluations
            .iter()
            .find(|evaluation| &evaluation.of == of)
    };
    // A Body's first stored state is based on `Empty`; the `Tip`'s state is
    // the current state and has no evaluation.
    let draft_base = evaluation(&after("DraftBase")).expect("DraftBase state");
    assert_eq!(draft_base.content.as_ref().unwrap().base, StateBase::Empty);
    assert!(evaluation(&after("Draft")).is_none());
    assert!(evaluation(&after("Hole")).is_none());
    // A state after the `Tip` is based on the current state.
    let pattern = evaluation(&after("LinearPattern")).expect("LinearPattern state");
    assert_eq!(
        pattern.content.as_ref().unwrap().base,
        StateBase::Current(Position::After(Member::Feature(feature("PatternBase"))))
    );
    // A hole's tool body is a state of its own.
    let tool = evaluation(&Evaluated::Tool(feature("Hole"))).expect("Hole tool body");
    assert_eq!(tool.content.as_ref().unwrap().base, StateBase::Empty);
    // Each state resolves, and no body of the geometry is a Body member's.
    for evaluation in &ir.design.evaluations {
        let state = ir
            .resolved_evaluation_state(
                &evaluation.id,
                &cadmpeg_core::decode::ResourceLimits::default(),
            )
            .unwrap();
        assert!(
            !state.records().bodies.is_empty(),
            "{}",
            evaluation.id.as_str()
        );
    }
    for body in &ir.geometry.bodies {
        for member in [
            "DraftBase:",
            "Draft:",
            "HoleBase:",
            "Hole:",
            "PatternBase:",
            "LinearPattern:",
        ] {
            assert!(
                !body.id.as_str().contains(&format!("#{member}")),
                "{}",
                body.id.as_str()
            );
        }
    }
}

#[test]
fn the_appearance_of_a_shape_outside_the_current_geometry_is_a_loss() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/freecad_fcstd/fixtures/named_constraint_0.21.2_P12a.FCStd"),
    )
    .unwrap();
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let code = crate::loss::FreecadLossCode::AppearanceStateNotTransferred
        .note("")
        .code;
    assert!(decoded.report().losses.iter().any(|loss| loss.code == code));
    // No binding the decode keeps names a record outside the geometry.
    let ir = decoded.ir();
    let records = super::record_ids(&ir.geometry)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    for binding in &ir.model.appearance_bindings {
        let value = cadmpeg_core::serde_erase::json::to_value(&binding.target.entity).unwrap();
        if let Some(id) = value["value"].as_str() {
            if id.starts_with("fcstd:model:") && !id.contains(":product_definition#") {
                assert!(records.contains(id), "{id}");
            }
        }
    }
}
