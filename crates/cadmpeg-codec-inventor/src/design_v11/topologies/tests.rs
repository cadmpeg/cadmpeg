// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};
use cadmpeg_ir::features::evaluation::{
    FeatureEvaluation, FeatureInputTopology, HistoricalBodyWriter, HistoricalEdgeGeometry,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureContent, FeatureDefinition, FeatureId};
use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalBodyId, HistoricalEdgeId};
use cadmpeg_ir::units::UnitVector3;

use super::*;

const FILLET: &str = "inventor:design:feature#fillet";
const STATE: &str = "inventor:design:feature-input#fillet";

fn edge(selector: u32) -> HistoricalEdgeId {
    HistoricalEdgeId::mint(format!("inventor:input-edge#fillet-{selector}")).expect("edge")
}

fn sample(selector: u32) -> HistoricalEdgeGeometry {
    let point = |values| json::from_value(cadmpeg_core::json!(values)).expect("point");
    HistoricalEdgeGeometry {
        edge: edge(selector),
        interior: point([1., 2., 3.]),
        tangent: Some(UnitVector3::X_AXIS),
        start: Some(point([0., 2., 3.])),
        end: Some(point([2., 2., 3.])),
        faces: DistinctMembers::default(),
        carrier: None,
        supports: Vec::new(),
    }
}

/// The input state of the fillet, which holds `edges` with one sample each.
fn input(edges: &[u32]) -> FeatureInputTopology {
    FeatureInputTopology {
        id: FeatureInputTopologyId::mint(STATE).expect("state"),
        input_of: FeatureId::mint(FILLET).expect("feature"),
        bodies: DistinctMembers::default(),
        faces: DistinctMembers::default(),
        edges: edges.iter().map(|selector| edge(*selector)).collect(),
        vertices: DistinctMembers::default(),
        face_owners: Vec::new(),
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        edge_geometry: edges.iter().map(|selector| sample(*selector)).collect(),
        native_ref: None,
    }
}

/// A part whose one fillet rounds edge 7 of its input state `input`.
fn part(input: FeatureInputTopology) -> CadIr {
    let operation = json::from_value(cadmpeg_core::json!({
        "kind": "fillet",
        "value": {"groups": [{
            "edges": {"kind": "historical", "value": {
                "state": STATE,
                "edges": [edge(7).as_str()],
                "native": "inventor:pmdc:selection#fillet",
            }},
            "radius": {"kind": "constant", "radius": 2.0},
        }]},
    }))
    .expect("fillet");
    let mut ir = crate::test_support::root_part_ir();
    ir.model.features.push(LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(FILLET).expect("id"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some("inventor:pmdc:feature#fillet".into()),
    });
    ir.model.feature_input_topologies.push(input);
    ir
}

fn project_part(ir: &mut CadIr) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let sketches = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        text_members: Vec::new(),
        issues: Vec::new(),
    };
    let mut losses = Vec::new();
    super::super::project(
        &ctx,
        ir,
        &crate::feature::FeatureInventory::default(),
        &sketches,
        &[],
        &[],
        &mut losses,
    )
    .expect("projection");
    assert!(
        !matches!(
            ir.design.features[0].definition.operation,
            FeatureOperation::Native { .. } | FeatureOperation::Unresolved { .. }
        ),
        "the fillet projects: {losses:?}"
    );
}

/// The `arena.omitted` losses of a CADIR write of `ir`.
fn omitted(ir: &CadIr) -> Vec<String> {
    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
        .expect("the CADIR writer does not refuse");
    plan.report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "arena.omitted")
        .map(|loss| loss.message.clone())
        .collect()
}

/// The picks of a typed fillet hold the source selector, interior point,
/// tangent and end points of each edge of its input state.
#[test]
fn a_typed_fillet_restates_its_input_state() {
    let mut ir = part(input(&[7]));
    project_part(&mut ir);
    let restating = ir.model.restated().get(STATE).expect("restated");
    assert_eq!(restating.by(), &Restater::Record(FILLET.to_owned()));
    assert!(restating.cut().is_empty(), "{:?}", restating.cut());
    let omitted = omitted(&ir);
    assert!(
        !omitted.iter().any(|message| message.contains(STATE)),
        "{omitted:?}"
    );
}

/// An edge that no pick names, and a body writer, stay charged as the
/// fields of the state the fillet states in part.
#[test]
fn unpicked_edges_and_body_writers_are_stated_in_part() {
    let mut state = input(&[7, 8]);
    state.bodies =
        std::iter::once(HistoricalBodyId::mint("inventor:input-body#fillet-1").expect("body"))
            .collect();
    state.body_writers = vec![HistoricalBodyWriter {
        body: HistoricalBodyId::mint("inventor:input-body#fillet-1").expect("body"),
        writer: FeatureId::mint(FILLET).expect("feature"),
    }];
    let mut ir = part(state);
    project_part(&mut ir);
    let restating = ir.model.restated().get(STATE).expect("restated");
    assert_eq!(
        restating.cut().iter().copied().collect::<Vec<_>>(),
        ["bodies", "body_writers", "edge_geometry", "edges"]
    );
    let omitted = omitted(&ir);
    assert!(
        omitted.iter().any(|message| message.contains(&format!(
            "stated in part: {STATE} (bodies, body_writers, edge_geometry, edges)"
        ))),
        "{omitted:?}"
    );
}

/// A feature whose operation stays native restates nothing of its input.
#[test]
fn a_native_operation_leaves_its_input_state_omitted() {
    let mut ir = part(input(&[7]));
    ir.model.feature_input_topologies[0].edges = std::iter::once(edge(8)).collect();
    ir.model.feature_input_topologies[0].edge_geometry = vec![sample(8)];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    super::super::project_history(&ctx, &mut ir, &BTreeMap::default(), false).expect("history");
    super::super::operations::project(&ctx, &mut ir, &mut Vec::new()).expect("operations");
    assert!(matches!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Native { .. }
    ));
    restate(&ctx, &mut ir).expect("restate");
    assert!(ir.model.restated().get(STATE).is_none());
    let omitted = omitted(&ir);
    assert!(
        omitted.iter().any(|message| message.contains(STATE)),
        "{omitted:?}"
    );
}
