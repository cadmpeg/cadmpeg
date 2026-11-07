// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::document::CadIr;
use crate::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, SketchFeatureBinding,
};
use crate::history_body::{BodyTopology, HistoryBody, HistoryStateRef};
use crate::ids::HistoryBodyId;
use crate::report::check::{Check, Finding};
use crate::schema::rewrite::typed::IdentityMap;
use crate::validate::{validate_neutral, Severity};

/// Move an identity into the history-body scope.
fn nest_scope(id: &str) -> String {
    let (format, rest) = id.split_once(':').expect("identity has a format");
    let (_, rest) = rest.split_once(':').expect("identity has a scope");
    format!("{format}:history-body:{rest}")
}

/// The cube of the examples as a nest in the history-body scope.
fn cube_nest() -> BodyTopology {
    let model = crate::examples::unit_cube().expect("cube").model;
    let mut nest = BodyTopology::new(model.bodies[0].clone());
    nest.regions = model.regions;
    nest.shells = model.shells;
    nest.faces = model.faces;
    nest.loops = model.loops;
    nest.coedges = model.coedges;
    nest.edges = model.edges;
    nest.vertices = model.vertices;
    nest.surfaces = model.surfaces;
    nest.curves = model.curves;
    nest.pcurves = model.pcurves;
    nest.points = model.points;
    nest
}

fn scoped(nest: BodyTopology) -> BodyTopology {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut map = IdentityMap::new(|id: &str| Ok(nest_scope(id)));
    let rewritten =
        crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities(nest, &ctx, &mut map)
            .expect("rewrite into the history-body scope");
    map.finish().expect("no refusal");
    rewritten
}

fn history_body(key: &str, topology: BodyTopology) -> HistoryBody {
    HistoryBody {
        id: HistoryBodyId::mint(format!("test:history-body:nest#{key}")).expect("identity"),
        produced_by: None,
        consumed_by: None,
        state: HistoryStateRef {
            ordinal: 0,
            identifier: 7,
        },
        topology: Box::new(topology),
    }
}

fn feature(id: &str) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Unresolved,
            },
        )),
        native_ref: None,
    }
}

fn findings(ir: &CadIr) -> Vec<Finding> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = crate::index::ModelIndex::build(ir);
    let mut findings = Vec::new();
    super::check_history_bodies(&ctx, ir, &index, &mut findings).expect("validation runs");
    findings
}

fn errors(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .collect()
}

#[test]
fn a_closed_nest_in_its_own_scope_has_no_finding() {
    let mut ir = crate::examples::unit_cube().expect("cube");
    ir.model.features.push(feature("test:model:feature#base"));
    let mut body = history_body("0", scoped(cube_nest()));
    body.produced_by = Some(FeatureId::mint("test:model:feature#base").unwrap());
    ir.model.history_bodies.push(body);
    assert!(findings(&ir).is_empty(), "{:?}", findings(&ir));
    let report = validate_neutral(&ir, Vec::new()).expect("validation runs");
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error),
        "{:?}",
        report.findings
    );
    assert_eq!(report.entity_counts["history_bodies"], 1);
}

#[test]
fn a_reference_that_leaves_the_nest_is_reported() {
    let mut ir = crate::examples::unit_cube().expect("cube");
    let mut nest = scoped(cube_nest());
    let dropped = nest.surfaces.remove(0);
    ir.model.history_bodies.push(history_body("0", nest));
    let found = findings(&ir);
    assert!(
        errors(&found).iter().any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.message.contains(dropped.id.as_str())
        }),
        "{found:?}"
    );
}

#[test]
fn a_reference_into_the_top_level_arenas_is_not_a_closure() {
    let mut ir = crate::examples::unit_cube().expect("cube");
    let top_surface = ir.model.surfaces[0].clone();
    let mut nest = scoped(cube_nest());
    let dropped = nest.surfaces.remove(0);
    for face in &mut nest.faces {
        if face.surface == dropped.id {
            face.surface = top_surface.id.clone();
        }
    }
    ir.model.history_bodies.push(history_body("0", nest));
    let found = findings(&ir);
    assert!(
        errors(&found).iter().any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.message.contains(top_surface.id.as_str())
        }),
        "{found:?}"
    );
}

#[test]
fn a_nest_id_that_repeats_a_top_level_id_is_reported() {
    let mut ir = crate::examples::unit_cube().expect("cube");
    ir.model.history_bodies.push(history_body("0", cube_nest()));
    let found = findings(&ir);
    assert!(
        errors(&found).iter().any(|finding| {
            finding.check == Check::Identity
                && finding.message.contains("repeats a top-level identity")
        }),
        "{found:?}"
    );
}

#[test]
fn a_nest_id_that_repeats_in_another_nest_is_reported() {
    let mut ir = CadIr::empty();
    ir.model
        .history_bodies
        .push(history_body("0", scoped(cube_nest())));
    ir.model
        .history_bodies
        .push(history_body("1", scoped(cube_nest())));
    let found = findings(&ir);
    assert!(
        errors(&found).iter().any(|finding| {
            finding.check == Check::Identity
                && finding
                    .message
                    .contains("repeats an identity of another history body")
        }),
        "{found:?}"
    );
}

#[test]
fn a_producer_or_consumer_that_names_no_feature_is_reported() {
    let mut ir = CadIr::empty();
    let mut body = history_body("0", scoped(cube_nest()));
    body.produced_by = Some(FeatureId::mint("test:model:feature#absent").unwrap());
    body.consumed_by = Some(FeatureId::mint("test:model:feature#gone").unwrap());
    ir.model.history_bodies.push(body);
    let found = findings(&ir);
    let messages = errors(&found)
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("produced_by test:model:feature#absent")));
    assert!(messages
        .iter()
        .any(|message| message.contains("consumed_by test:model:feature#gone")));
}

#[test]
fn nest_records_are_not_seen_by_the_top_level_reference_check() {
    let mut ir = CadIr::empty();
    ir.model
        .history_bodies
        .push(history_body("0", scoped(cube_nest())));
    let report = validate_neutral(&ir, Vec::new()).expect("validation runs");
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error),
        "{:?}",
        report.findings
    );
    assert!(ir.model.bodies.is_empty());
}

#[test]
fn a_document_with_a_nest_round_trips_through_json() {
    let mut ir = CadIr::empty();
    ir.model
        .history_bodies
        .push(history_body("0", scoped(cube_nest())));
    let json = serde_json::to_string(&ir).expect("serialize");
    let back: CadIr = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.model.history_bodies, ir.model.history_bodies);
    let empty = serde_json::to_value(CadIr::empty()).expect("serialize");
    assert!(empty["model"].get("history_bodies").is_none());
}
