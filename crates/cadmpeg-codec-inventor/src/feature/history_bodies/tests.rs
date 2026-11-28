// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::topology::BodyKind;

use super::*;
use crate::feature::history_states::{StateRef, UnboundReason};
use crate::kernel::parse_carrier;
use crate::test_support::history_carrier::{
    carrier_bytes, carrier_bytes_with_name, framed_carrier, CURRENT_BODY_KEY, SPHERE_BODY_KEY,
};

fn feature(native: &str) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(format!("inventor:model:feature#{native}")).expect("identity"),
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
        native_ref: Some(native.to_owned()),
    }
}

fn record(kind: &str, native: &str, before: Option<u32>, key: Option<i64>) -> FeatureHistoryState {
    FeatureHistoryState {
        id: format!("inventor:pmdc:feature-history-state#{native}"),
        feature: native.to_owned(),
        source_index: 1,
        kind: kind.to_owned(),
        state_after: Some(StateRef {
            ordinal: before.map_or(0, |ordinal| ordinal + 1),
            state_id: 0,
        }),
        state_before: before,
        body_key: key,
        history_body: None,
        binding: Binding::Bound,
    }
}

// Selection of the bodies to extract.

#[test]
fn a_split_takes_the_state_before_and_a_base_the_state_after() {
    let records = [
        record(SPLIT_TAG, "split", Some(3), Some(9)),
        record(BASE_TAG, "base", Some(1), Some(4)),
        record(NATIVE_BASE_KIND, "native-base", None, Some(5)),
        record("extrusion", "other", Some(2), Some(6)),
    ];
    let mut outcome = Outcome::default();
    let wants = wants(&records, &mut outcome);
    let found = wants
        .iter()
        .map(|want| (want.record, want.role, want.ordinal, want.key))
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            (0, Role::Split, 3, 9),
            (1, Role::Base, 2, 4),
            (2, Role::Base, 0, 5)
        ]
    );
    assert_eq!(outcome.requested, 3);
}

#[test]
fn unbound_features_and_a_split_at_the_oldest_state_are_not_requested() {
    let mut unbound = record(SPLIT_TAG, "a", Some(1), Some(9));
    unbound.binding = Binding::Unbound {
        reason: UnboundReason::BodyKeyMismatch,
    };
    let mut keyless = record(BASE_TAG, "b", Some(1), None);
    keyless.body_key = None;
    let oldest = record(SPLIT_TAG, "c", None, Some(9));
    let mut outcome = Outcome::default();
    assert!(wants(&[unbound, keyless, oldest], &mut outcome).is_empty());
    assert_eq!(outcome.requested, 0);
}

#[test]
fn requests_over_the_cap_are_counted_as_bounded() {
    let records = (0..=MAX_HISTORY_BODIES)
        .map(|index| record(BASE_TAG, &format!("base{index}"), Some(0), Some(7)))
        .collect::<Vec<_>>();
    let mut built = Built::default();
    let requested = wants(&records, &mut built.outcome);
    let wants = bound(requested, &mut built, &records).expect("bound");
    assert_eq!(wants.len(), MAX_HISTORY_BODIES);
    assert_eq!(built.outcome.requested, MAX_HISTORY_BODIES + 1);
    assert_eq!(built.outcome.bounded, 1);
    assert_eq!(
        given_up(&built),
        [("inventor:history-body:nest#h0008", Unbuilt::Bounded)]
    );
}

/// The nests that `built` gives up, by identity.
fn given_up(built: &Built) -> Vec<(&str, Unbuilt)> {
    built
        .unbuilt
        .iter()
        .map(|(id, why, _)| (id.as_str(), *why))
        .collect()
}

// Extraction of one body from a decoded state.

fn cube() -> cadmpeg_ir::document::Topology {
    cadmpeg_ir::examples::unit_cube().expect("cube").geometry
}

fn extract_cube(geometry: &cadmpeg_ir::document::Topology) -> Extract {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let body = geometry.bodies[0].id.as_str().to_owned();
    extract(&ctx, geometry, &body).expect("extraction runs")
}

#[test]
fn a_closed_solid_extracts_with_every_record_it_reaches() {
    let geometry = cube();
    let Extract::Nest(nest) = extract_cube(&geometry) else {
        panic!("a closed cube must extract");
    };
    assert_eq!(nest.body.id, geometry.bodies[0].id);
    assert_eq!(nest.faces.len(), geometry.faces.len());
    assert_eq!(nest.edges.len(), geometry.edges.len());
    assert_eq!(nest.coedges.len(), geometry.coedges.len());
    assert_eq!(nest.surfaces.len(), geometry.surfaces.len());
    assert_eq!(nest.vertices.len(), geometry.vertices.len());
    assert_eq!(nest.points.len(), geometry.points.len());
}

#[test]
fn a_reference_that_names_no_record_is_not_closed() {
    let mut geometry = cube();
    geometry.surfaces.remove(0);
    assert!(matches!(
        extract_cube(&geometry),
        Extract::NotClosed(Open::Reference)
    ));
}

#[test]
fn a_sheet_is_not_closed() {
    let mut geometry = cube();
    geometry.bodies[0].kind = BodyKind::Sheet;
    assert!(matches!(
        extract_cube(&geometry),
        Extract::NotClosed(Open::Kind)
    ));
}

#[test]
fn an_edge_that_joins_one_coedge_is_not_closed() {
    let mut geometry = cube();
    let other = geometry.coedges[1].edge.clone();
    assert_ne!(geometry.coedges[0].edge, other);
    geometry.coedges[0].edge = other;
    assert!(matches!(
        extract_cube(&geometry),
        Extract::NotClosed(Open::Edge)
    ));
}

#[test]
fn records_of_other_bodies_stay_out_of_the_nest() {
    let mut geometry = cube();
    let mut other = geometry.bodies[0].clone();
    other.id = "inventor:brep:entity#other".try_into().expect("identity");
    geometry.bodies.push(other);
    let Extract::Nest(nest) = extract_cube(&geometry) else {
        panic!("a closed cube must extract");
    };
    assert_eq!(nest.regions.len(), geometry.regions.len());
}

#[test]
fn a_procedural_carrier_keeps_its_solved_cache() {
    let mut geometry = cube();
    let solved = geometry.surfaces[0].geometry.clone();
    let SurfaceGeometry::Solved(cache) = solved else {
        panic!("the cube states solved surfaces");
    };
    geometry.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: "inventor:brep:procedural-surface#1"
            .try_into()
            .expect("identity"),
        cache: Some(cache.clone()),
    };
    let Extract::Nest(nest) = extract_cube(&geometry) else {
        panic!("a procedural carrier with a cache must extract");
    };
    assert!(matches!(
        &nest.surfaces[0].geometry,
        SurfaceGeometry::Solved(found) if *found == cache
    ));
}

#[test]
fn a_procedural_carrier_without_a_cache_is_absent() {
    let mut geometry = cube();
    geometry.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: "inventor:brep:procedural-surface#1"
            .try_into()
            .expect("identity"),
        cache: None,
    };
    assert!(matches!(extract_cube(&geometry), Extract::Unsolved));
}

#[test]
fn a_carrier_of_unknown_shape_is_absent() {
    let mut geometry = cube();
    geometry.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    assert!(matches!(extract_cube(&geometry), Extract::Unsolved));
}

#[test]
fn a_body_that_the_state_lacks_is_absent() {
    let geometry = cube();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    assert!(matches!(
        extract(&ctx, &geometry, "inventor:brep:entity#missing").expect("extraction runs"),
        Extract::Absent
    ));
}

#[test]
fn a_nest_moves_into_its_own_scope_and_validates_as_a_document() {
    let geometry = cube();
    let Extract::Nest(topology) = extract_cube(&geometry) else {
        panic!("a closed cube must extract");
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let want = Want {
        index: 3,
        record: 0,
        ordinal: 2,
        state_id: 44,
        key: 9,
        role: Role::Split,
    };
    let consumer = feature("split");
    let body = nest(&ctx, &want, consumer.id.clone(), *topology).expect("nest");
    assert_eq!(body.id.as_str(), "inventor:history-body:nest#h0003");
    assert_eq!(body.consumed_by.as_ref(), Some(&consumer.id));
    assert!(body.produced_by.is_none());
    assert_eq!((body.state.ordinal, body.state.identifier), (2, 44));
    let nest_records = body.topology.record_count();
    assert_eq!(
        nest_records,
        geometry.bodies.len()
            + geometry.regions.len()
            + geometry.shells.len()
            + geometry.faces.len()
            + geometry.loops.len()
            + geometry.coedges.len()
            + geometry.edges.len()
            + geometry.vertices.len()
            + geometry.surfaces.len()
            + geometry.curves.len()
            + geometry.pcurves.len()
            + geometry.points.len()
    );
    let mut ir = CadIr::empty();
    ir.model.features.push(consumer);
    ir.model.history_bodies.push(body);
    let in_scope = ir.model.history_bodies[0].topology.to_topology();
    for id in in_scope.bodies.iter().map(|record| record.id.as_str()) {
        assert_eq!(id.split(':').nth(1), Some("history-body"), "{id}");
        assert!(id.contains("#h0003-"), "{id}");
    }
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validation runs");
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.severity == cadmpeg_ir::report::Severity::Error),
        "{:?}",
        report.findings
    );
    assert!(ir.geometry.bodies.is_empty());
}

fn build_from(
    width: RefWidth,
    dangling: bool,
    records: &[FeatureHistoryState],
    features: &[Feature],
) -> Built {
    let payload = framed_carrier(&carrier_bytes(width, dangling));
    build_with_budget(&payload, &DecodePolicy::default(), records, features)
}

fn build_with_budget(
    payload: &[u8],
    policy: &DecodePolicy,
    records: &[FeatureHistoryState],
    features: &[Feature],
) -> Built {
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(payload, &arena, policy).expect("carrier fits policy");
    let carrier =
        parse_carrier(view, &cadmpeg_ir::identity_key!("token"), 1, 0, 18).expect("carrier parses");
    let header = carrier.header.as_ref().expect("header parses");
    let window = carrier.bytes.window();
    let crate::feature::history_states::Framing::Framed(framed) =
        crate::feature::history_states::frame_chain(&ctx, window, header.width).expect("frame")
    else {
        panic!("the chain frames");
    };
    let crate::feature::history_states::Prepared::Ready(chain) =
        crate::feature::history_states::prepare(&ctx, &framed, window, header.width)
            .expect("prepare")
    else {
        panic!("the chain closes");
    };
    let closed = crate::feature::history_states::Closed::new(
        *chain,
        crate::feature::history_states::StateIndex::default(),
    );
    let built = build(&ctx, &carrier, &closed, records, features).expect("build");
    assert!(ctx.resource_refusal().is_none());
    built
}

fn split(key: i64) -> FeatureHistoryState {
    // The state after the Split is the head, ordinal 1; the state before is 0.
    let mut state = record(SPLIT_TAG, "split", Some(0), Some(key));
    state.state_after = Some(StateRef {
        ordinal: 1,
        state_id: 13,
    });
    state
}

#[test]
fn the_state_before_a_split_gives_the_consumed_body() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let built = build_from(width, false, &[split(SPHERE_BODY_KEY)], &[feature("split")]);
        assert_eq!(
            built.outcome,
            Outcome {
                requested: 1,
                built: 1,
                ..Outcome::default()
            },
            "{width:?}"
        );
        let [body] = built.bodies.as_slice() else {
            panic!("one nest at {width:?}");
        };
        assert_eq!((body.state.ordinal, body.state.identifier), (0, 12));
        assert_eq!(
            body.consumed_by.as_ref().map(cadmpeg_ir::FeatureId::as_str),
            Some("inventor:model:feature#split")
        );
        assert!(body.produced_by.is_none());
        let nest = &body.topology;
        assert_eq!(nest.body.kind, BodyKind::Solid);
        assert_eq!(
            (
                nest.regions.len(),
                nest.shells.len(),
                nest.faces.len(),
                nest.surfaces.len()
            ),
            (1, 1, 1, 1)
        );
        assert!(
            matches!(
                &nest.surfaces[0].geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
            ),
            "{:?}",
            nest.surfaces[0].geometry
        );
        assert_eq!(built.links.get(&0), Some(&body.id));
    }
}

#[test]
fn a_second_named_nest_gives_up_only_the_feature_that_cannot_keep_it() {
    let name = "n".repeat(1200);
    let payload = framed_carrier(&carrier_bytes_with_name(
        RefWidth::Eight,
        false,
        Some(&name),
    ));
    let records = [
        record(BASE_TAG, "base", Some(0), Some(CURRENT_BODY_KEY)),
        split(SPHERE_BODY_KEY),
    ];
    let features = [feature("base"), feature("split")];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1800;
    let built = build_with_budget(&payload, &policy, &records, &features);
    assert_eq!(built.outcome.requested, 2);
    assert_eq!(built.outcome.built, 1);
    assert_eq!(built.outcome.bounded, 1);
    assert_eq!(
        built.bodies[0].topology.body.name.as_deref(),
        Some(name.as_str())
    );
    let [(id, Unbuilt::ResourceBounded(limit), feature)] = built.unbuilt.as_slice() else {
        panic!("one named nest gave up: {:?}", built.unbuilt);
    };
    assert!(id.as_str().ends_with("h0001"));
    assert_eq!(feature, "split");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    let notes = super::super::history_states::losses(&super::super::history_states::Projection {
        schema_assumed: 0,
        records: records.to_vec(),
        unread: None,
        bodies: built,
    });
    assert!(notes
        .iter()
        .any(|note| note.message.contains("split history body")
            && note.message.contains("RetainedBytes")));
}

#[test]
fn a_body_key_that_the_older_state_lacks_gives_no_nest() {
    let built = build_from(
        RefWidth::Eight,
        false,
        &[split(CURRENT_BODY_KEY)],
        &[feature("split")],
    );
    assert!(built.bodies.is_empty());
    assert_eq!(built.outcome.requested, 1);
    assert_eq!(built.outcome.undecoded, 1);
    assert!(built.unbuilt.is_empty(), "{:?}", built.unbuilt);
}

#[test]
fn a_state_with_a_reference_to_no_slot_gives_no_nest() {
    let built = build_from(
        RefWidth::Eight,
        true,
        &[split(SPHERE_BODY_KEY)],
        &[feature("split")],
    );
    assert!(built.bodies.is_empty());
    assert_eq!(built.outcome.undecoded, 1);
    assert_eq!(
        given_up(&built),
        [("inventor:history-body:nest#h0000", Unbuilt::Undecoded)]
    );
}

#[test]
fn a_decoded_body_of_an_unprojected_feature_gives_no_nest() {
    let built = build_from(RefWidth::Eight, false, &[split(SPHERE_BODY_KEY)], &[]);
    assert!(built.bodies.is_empty());
    assert_eq!(
        built.outcome,
        Outcome {
            requested: 1,
            unlinked: 1,
            ..Outcome::default()
        }
    );
    assert_eq!(
        given_up(&built),
        [("inventor:history-body:nest#h0000", Unbuilt::Unlinked)]
    );
}

#[test]
fn a_base_reads_the_state_after_it() {
    let mut base = record(BASE_TAG, "base", Some(0), Some(CURRENT_BODY_KEY));
    base.state_after = Some(StateRef {
        ordinal: 1,
        state_id: 13,
    });
    let built = build_from(RefWidth::Eight, false, &[base], &[feature("base")]);
    let [body] = built.bodies.as_slice() else {
        panic!("one nest");
    };
    assert_eq!((body.state.ordinal, body.state.identifier), (1, 13));
    assert_eq!(
        body.produced_by.as_ref().map(cadmpeg_ir::FeatureId::as_str),
        Some("inventor:model:feature#base")
    );
    assert!(body.consumed_by.is_none());
}

#[test]
fn two_requests_give_nests_in_distinct_scopes() {
    let mut base = record(BASE_TAG, "base", Some(0), Some(CURRENT_BODY_KEY));
    base.state_after = Some(StateRef {
        ordinal: 1,
        state_id: 13,
    });
    let built = build_from(
        RefWidth::Eight,
        false,
        &[split(SPHERE_BODY_KEY), base],
        &[feature("split"), feature("base")],
    );
    assert_eq!(built.bodies.len(), 2);
    let mut ids = std::collections::BTreeSet::new();
    for body in &built.bodies {
        let geometry = body.topology.to_topology();
        assert!(ids.insert(body.id.as_str().to_owned()));
        assert!(ids.insert(geometry.bodies[0].id.as_str().to_owned()));
    }
}

#[test]
fn the_losses_name_each_count() {
    let notes = super::super::history_states::losses(&super::super::history_states::Projection {
        schema_assumed: 0,
        records: Vec::new(),
        unread: None,
        bodies: Built {
            outcome: Outcome {
                requested: 6,
                built: 1,
                undecoded: 2,
                open: [1, 0, 1],
                bounded: 1,
                unlinked: 1,
            },
            ..Built::default()
        },
    });
    let codes = notes
        .iter()
        .map(|note| note.code.local_code().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        [
            "feature.history-body-undecoded",
            "feature.history-body-not-closed",
            "feature.history-body-unlinked",
            "feature.history-body-bounded"
        ]
    );
    assert!(
        notes[1].message.contains("2 body(ies)")
            && notes[1].message.contains(
                "(1 with a reference outside the body, 0 not solid bodies, 1 with an edge"
            ),
        "{}",
        notes[1].message
    );
}

#[test]
fn each_given_up_body_is_charged_by_its_nest() {
    let nest = |index| nest_id(index).expect("nest id");
    let notes = super::super::history_states::losses(&super::super::history_states::Projection {
        schema_assumed: 0,
        records: Vec::new(),
        unread: None,
        bodies: Built {
            unbuilt: vec![
                (nest(1), Unbuilt::NotClosed(Open::Edge), "split".to_string()),
                (nest(9), Unbuilt::Bounded, "base".to_string()),
            ],
            outcome: Outcome {
                requested: 10,
                open: [0, 0, 1],
                bounded: 1,
                ..Outcome::default()
            },
            ..Built::default()
        },
    });
    let charges = notes
        .iter()
        .filter(|note| note.code == cadmpeg_ir::unknown::part::record_undecoded_kind())
        .map(|note| note.message.as_str())
        .collect::<Vec<_>>();
    let [closed, bounded] = charges.as_slice() else {
        panic!("one charge per given-up body: {notes:#?}");
    };
    for (message, nest, code) in [
        (
            closed,
            "h0001",
            crate::loss::InventorLossCode::FeatureHistoryBodyNotClosed,
        ),
        (
            bounded,
            "h0009",
            crate::loss::InventorLossCode::FeatureHistoryBodyBounded,
        ),
    ] {
        assert!(
            message.starts_with(&format!(
                "record inventor:history-body:nest#{nest} given up: {}: ",
                code.kind()
            )),
            "{message}"
        );
    }
}
