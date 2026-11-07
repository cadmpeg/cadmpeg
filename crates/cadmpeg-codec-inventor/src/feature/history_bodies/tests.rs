// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::topology::BodyKind;

use super::*;
use crate::feature::history_states::{StateRef, UnboundReason};
use crate::kernel::parse_carrier;

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
    let mut outcome = Outcome::default();
    let wants = wants(&records, &mut outcome);
    assert_eq!(wants.len(), MAX_HISTORY_BODIES);
    assert_eq!(outcome.requested, MAX_HISTORY_BODIES + 1);
    assert_eq!(outcome.bounded, 1);
}

// Extraction of one body from a decoded state.

fn cube() -> cadmpeg_ir::document::Model {
    cadmpeg_ir::examples::unit_cube().expect("cube").model
}

fn extract_cube(model: &cadmpeg_ir::document::Model) -> Extract {
    let ctx = cadmpeg_test_support::service_decode_context();
    let body = model.bodies[0].id.as_str().to_owned();
    extract(&ctx, model, &body).expect("extraction runs")
}

#[test]
fn a_closed_solid_extracts_with_every_record_it_reaches() {
    let model = cube();
    let Extract::Nest(nest) = extract_cube(&model) else {
        panic!("a closed cube must extract");
    };
    assert_eq!(nest.body.id, model.bodies[0].id);
    assert_eq!(nest.faces.len(), model.faces.len());
    assert_eq!(nest.edges.len(), model.edges.len());
    assert_eq!(nest.coedges.len(), model.coedges.len());
    assert_eq!(nest.surfaces.len(), model.surfaces.len());
    assert_eq!(nest.vertices.len(), model.vertices.len());
    assert_eq!(nest.points.len(), model.points.len());
}

#[test]
fn a_reference_that_names_no_record_is_not_closed() {
    let mut model = cube();
    model.surfaces.remove(0);
    assert!(matches!(
        extract_cube(&model),
        Extract::NotClosed(Open::Reference)
    ));
}

#[test]
fn a_sheet_is_not_closed() {
    let mut model = cube();
    model.bodies[0].kind = BodyKind::Sheet;
    assert!(matches!(
        extract_cube(&model),
        Extract::NotClosed(Open::Kind)
    ));
}

#[test]
fn an_edge_that_joins_one_coedge_is_not_closed() {
    let mut model = cube();
    let other = model.coedges[1].edge.clone();
    assert_ne!(model.coedges[0].edge, other);
    model.coedges[0].edge = other;
    assert!(matches!(
        extract_cube(&model),
        Extract::NotClosed(Open::Edge)
    ));
}

#[test]
fn records_of_other_bodies_stay_out_of_the_nest() {
    let mut model = cube();
    let mut other = model.bodies[0].clone();
    other.id = "inventor:brep:entity#other".try_into().expect("identity");
    other.regions.clear();
    model.bodies.push(other);
    let Extract::Nest(nest) = extract_cube(&model) else {
        panic!("a closed cube must extract");
    };
    assert_eq!(nest.regions.len(), model.regions.len());
}

#[test]
fn a_procedural_carrier_keeps_its_solved_cache() {
    let mut model = cube();
    let solved = model.surfaces[0].geometry.clone();
    let SurfaceGeometry::Solved(cache) = solved else {
        panic!("the cube states solved surfaces");
    };
    model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: "inventor:brep:procedural-surface#1"
            .try_into()
            .expect("identity"),
        cache: Some(cache.clone()),
    };
    let Extract::Nest(nest) = extract_cube(&model) else {
        panic!("a procedural carrier with a cache must extract");
    };
    assert!(matches!(
        &nest.surfaces[0].geometry,
        SurfaceGeometry::Solved(found) if *found == cache
    ));
}

#[test]
fn a_procedural_carrier_without_a_cache_is_absent() {
    let mut model = cube();
    model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: "inventor:brep:procedural-surface#1"
            .try_into()
            .expect("identity"),
        cache: None,
    };
    assert!(matches!(extract_cube(&model), Extract::Absent));
}

#[test]
fn a_carrier_of_unknown_shape_is_absent() {
    let mut model = cube();
    model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    assert!(matches!(extract_cube(&model), Extract::Absent));
}

#[test]
fn a_body_that_the_state_lacks_is_absent() {
    let model = cube();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(matches!(
        extract(&ctx, &model, "inventor:brep:entity#missing").expect("extraction runs"),
        Extract::Absent
    ));
}

#[test]
fn a_nest_moves_into_its_own_scope_and_validates_as_a_document() {
    let model = cube();
    let Extract::Nest(topology) = extract_cube(&model) else {
        panic!("a closed cube must extract");
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    let want = Want {
        record: 0,
        ordinal: 2,
        state_id: 44,
        key: 9,
        role: Role::Split,
    };
    let consumer = feature("split");
    let body = nest(&ctx, 3, &want, Some(consumer.id.clone()), *topology).expect("nest");
    assert_eq!(body.id.as_str(), "inventor:history-body:nest#h0003");
    assert_eq!(body.consumed_by.as_ref(), Some(&consumer.id));
    assert!(body.produced_by.is_none());
    assert_eq!((body.state.ordinal, body.state.identifier), (2, 44));
    let nest_records = body.topology.record_count();
    assert_eq!(
        nest_records,
        model.bodies.len()
            + model.regions.len()
            + model.shells.len()
            + model.faces.len()
            + model.loops.len()
            + model.coedges.len()
            + model.edges.len()
            + model.vertices.len()
            + model.surfaces.len()
            + model.curves.len()
            + model.pcurves.len()
            + model.points.len()
    );
    let mut ir = CadIr::empty();
    ir.model.features.push(consumer);
    ir.model.history_bodies.push(body);
    let in_scope = ir.model.history_bodies[0].topology.to_model();
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
    assert!(ir.model.bodies.is_empty());
}

// Materialization of a history state from bytes.

const SPHERE_BODY_KEY: i64 = 9;
const CURRENT_BODY_KEY: i64 = 7;

fn put_ref(out: &mut Vec<u8>, width: RefWidth, tag: u8, value: i64) {
    out.push(tag);
    match width {
        RefWidth::Four => out.extend_from_slice(
            &i32::try_from(value)
                .expect("fixture value fits i32")
                .to_le_bytes(),
        ),
        RefWidth::Eight => out.extend_from_slice(&value.to_le_bytes()),
    }
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    let parts: Vec<&str> = name.split('-').collect();
    for (index, part) in parts.iter().enumerate() {
        out.push(if index + 1 == parts.len() { 0x0d } else { 0x0e });
        out.push(u8::try_from(part.len()).expect("fixture name fits a byte"));
        out.extend_from_slice(part.as_bytes());
    }
}

/// A record whose payload is references and integers: `(tag, value)` pairs.
fn put_plain(out: &mut Vec<u8>, width: RefWidth, name: &str, fields: &[(u8, i64)]) {
    put_fields(out, width, name, fields);
    out.push(0x11);
}

/// The name and fields of a record, before its terminator.
fn put_fields(out: &mut Vec<u8>, width: RefWidth, name: &str, fields: &[(u8, i64)]) {
    put_name(out, name);
    for &(tag, value) in fields {
        put_ref(out, width, tag, value);
    }
}

const REF: u8 = 0x0c;
const INT: u8 = 0x04;

fn put_header(out: &mut Vec<u8>, width: RefWidth) {
    let (magic, len): (&[u8], usize) = match width {
        RefWidth::Four => (b"ASM BinaryFile4", 31),
        RefWidth::Eight => (b"ASM BinaryFile8", 47),
    };
    out.extend_from_slice(magic);
    out.extend_from_slice(&23_200_u32.to_le_bytes());
    out.resize(len, 0);
    for _ in 0..3 {
        out.extend_from_slice(&[0x07, 0]);
    }
    for _ in 0..3 {
        out.push(0x06);
        out.extend_from_slice(&1.0f64.to_le_bytes());
    }
}

/// The records of a sphere solid below a body, from slot `body + 1` on:
/// lump, shell, face, sphere surface. `body` is the slot of the body record.
fn put_sphere_solid(out: &mut Vec<u8>, width: RefWidth, body: i64, key: i64, refer: i64) {
    let (lump, shell, face, surface) = (body + 1, body + 2, body + 3, body + 4);
    put_plain(
        out,
        width,
        "body",
        &[
            (REF, -1),
            (INT, key),
            (REF, refer),
            (REF, lump),
            (REF, -1),
            (REF, -1),
        ],
    );
    put_plain(
        out,
        width,
        "lump",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, shell),
            (REF, body),
        ],
    );
    put_plain(
        out,
        width,
        "shell",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (REF, face),
            (REF, -1),
            (REF, lump),
        ],
    );
    put_fields(
        out,
        width,
        "face",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (REF, shell),
            (REF, -1),
            (REF, surface),
        ],
    );
    out.extend_from_slice(&[0x0b, 0x0b, 0x11]);
    put_name(out, "sphere-surface");
    put_ref(out, width, REF, -1);
    put_ref(out, width, INT, -1);
    put_ref(out, width, REF, -1);
    out.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.push(0x06);
    out.extend_from_slice(&2.5f64.to_le_bytes());
    for triple in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        out.push(0x14);
        for value in triple {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&[0x0b; 5]);
    out.push(0x11);
}

/// A `delta_state` record with one board of `(old, new)` changes.
fn put_state(
    out: &mut Vec<u8>,
    width: RefWidth,
    ids: (i64, i64, i64, i64),
    changes: &[(i64, i64)],
) {
    let (id, previous, next, node) = ids;
    put_name(out, "delta_state");
    for value in [id, 1, 0] {
        put_ref(out, width, INT, value);
    }
    for value in [previous, next, node, -1, -1] {
        put_ref(out, width, REF, value);
    }
    out.push(0x0b);
    put_ref(out, width, INT, 1);
    put_ref(out, width, REF, node);
    put_ref(out, width, INT, 1);
    for &(old, new) in changes {
        put_ref(out, width, INT, 1);
        put_ref(out, width, REF, old);
        put_ref(out, width, REF, new);
    }
    put_ref(out, width, INT, 0);
    put_ref(out, width, INT, 0);
    out.push(0x11);
}

/// A carrier whose current body has key 7 and whose older state holds the same
/// sphere solid under key 9. `dangling` makes the archived body refer to a slot
/// that no state holds.
fn carrier_bytes(width: RefWidth, dangling: bool) -> Vec<u8> {
    let mut out = Vec::new();
    put_header(&mut out, width);
    put_plain(&mut out, width, "asmheader", &[]);
    put_sphere_solid(&mut out, width, 1, CURRENT_BODY_KEY, -1);
    // Slot 1 of the older state is the archived revision 6.
    put_state(&mut out, width, (13, -1, 1, 0), &[(6, 1)]);
    let refer = if dangling { 99 } else { -1 };
    put_plain(
        &mut out,
        width,
        "body",
        &[
            (REF, -1),
            (INT, SPHERE_BODY_KEY),
            (REF, refer),
            (REF, 2),
            (REF, -1),
            (REF, -1),
        ],
    );
    put_state(
        &mut out,
        width,
        (12, 0, -1, 1),
        &[(-1, 1), (-1, 2), (-1, 3), (-1, 4), (-1, 5)],
    );
    out
}

fn framed_carrier(bytes: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&2_u16.to_le_bytes());
    payload.extend_from_slice(&3_u32.to_le_bytes());
    payload.extend_from_slice(&4_u32.to_le_bytes());
    payload.extend_from_slice(bytes);
    payload.extend_from_slice(&5_u32.to_le_bytes());
    payload.push(1);
    payload.extend_from_slice(&(-1_i32).to_le_bytes());
    payload.extend_from_slice(&6_u32.to_le_bytes());
    payload.extend_from_slice(&u32::MAX.to_le_bytes());
    payload
}

fn build_from(
    width: RefWidth,
    dangling: bool,
    records: &[FeatureHistoryState],
    features: &[Feature],
) -> Built {
    let payload = framed_carrier(&carrier_bytes(width, dangling));
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::default())
        .expect("carrier fits policy");
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
    build(&ctx, &carrier, *chain, records, features).expect("build")
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
        let model = body.topology.to_model();
        assert!(ids.insert(body.id.as_str().to_owned()));
        assert!(ids.insert(model.bodies[0].id.as_str().to_owned()));
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
