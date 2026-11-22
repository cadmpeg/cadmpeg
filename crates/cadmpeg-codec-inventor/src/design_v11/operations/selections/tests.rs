// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::design::refs::EntityRef;
use cadmpeg_ir::features::evaluation::{FeatureInputTopology, HistoricalEdgeGeometry};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{EdgeId, FeatureInputTopologyId, HistoricalEdgeId};
use cadmpeg_ir::units::UnitVector3;

#[test]
fn resolved_edges_keep_the_source_name_and_entity_cache() {
    let id = EdgeId::mint("inventor:brep:edge#synthetic").expect("id");
    let value = edge(
        &EdgeSelection::Resolved {
            edges: vec![id.clone()],
            native: "inventor:pmdc:selection#synthetic".into(),
        },
        &[],
    )
    .expect("selection");
    let Picks::Named(picks) = value.picks else {
        panic!("named picks")
    };
    let pick = &picks.as_slice()[0];
    assert_eq!(pick.resolved, Some(vec![EntityRef::Edge(id)]));
    assert_eq!(
        pick.native.as_ref().expect("name").value,
        NativeNameValue::Text(cadmpeg_core::nonblank_literal!(
            "inventor:pmdc:selection#synthetic"
        ))
    );
}

#[test]
fn historical_edges_keep_samples_and_require_one_input_state() {
    let state =
        FeatureInputTopologyId::mint("inventor:design:feature-input#synthetic").expect("id");
    let id = HistoricalEdgeId::mint("inventor:input-edge#synthetic").expect("id");
    let point = |values| serde_json::from_value(serde_json::json!(values)).expect("point");
    let sample = HistoricalEdgeGeometry {
        edge: id.clone(),
        interior: point([1., 2., 3.]),
        tangent: Some(UnitVector3::X_AXIS),
        start: Some(point([0., 2., 3.])),
        end: Some(point([2., 2., 3.])),
        faces: DistinctMembers::default(),
        carrier: None,
        supports: Vec::new(),
    };
    let input = FeatureInputTopology {
        id: state.clone(),
        input_of: FeatureId::mint("inventor:design:feature#fillet").expect("id"),
        bodies: DistinctMembers::default(),
        faces: DistinctMembers::default(),
        edges: std::iter::once(id.clone()).collect(),
        vertices: DistinctMembers::default(),
        edge_geometry: vec![sample.clone()],
        native_ref: None,
    };
    let source = EdgeSelection::historical(
        state,
        vec![id.clone()],
        "inventor:pmdc:selection#synthetic".into(),
    )
    .expect("source selection");
    let value = edge(&source, std::slice::from_ref(&input)).expect("selection");
    let Picks::Named(picks) = value.picks else {
        panic!("named picks")
    };
    let pick = &picks.as_slice()[0];
    assert_eq!(
        pick.native.as_ref().expect("name").value,
        NativeNameValue::Text(
            cadmpeg_core::text::NonBlankString::new(id.to_string()).expect("name")
        )
    );
    let witness = pick.witness.as_ref().expect("witness");
    assert_eq!(witness.point, Some(sample.interior));
    assert_eq!(witness.direction, sample.tangent);
    assert_eq!(
        witness.endpoints,
        Some([sample.start.expect("start"), sample.end.expect("end")])
    );
    assert!(edge(&source, &[]).is_err());
    assert!(edge(&source, &[input.clone(), input]).is_err());
}
