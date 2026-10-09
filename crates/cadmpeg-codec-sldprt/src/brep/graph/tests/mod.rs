// SPDX-License-Identifier: Apache-2.0

use crate::brep::topology::Bridge;
use crate::brep::topology::Coedge;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::topology::Sense;
use crate::brep::topology::EdgeReferences;
use crate::brep::topology::EdgeUse;
use crate::brep::topology::Loop;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use crate::brep::topology::Tables;
use crate::brep::entity;
use crate::brep::graph::unique_face_colors;
use crate::brep::graph::test_support::with_test_context;

mod admission;

#[test]
fn line_edge_parameters_convert_from_metres_to_millimetres() {
    let carrier = crate::brep::CurveCarrier {
        attr: 1,
        offset: 0,
        end: 0,
        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 17.5, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0),
            )
            .unwrap(),
        )),
        parameter_range: Some(
            cadmpeg_ir::units::FiniteVector::new([-0.014, 0.0165]).expect("finite fixture range"),
        ),
    };

    let endpoints = [
        cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
        cadmpeg_ir::math::Point3::new(0.0, 31.5, 0.0),
    ];
    assert_eq!(
        crate::brep::graph::edge_parameter_range(
            &cadmpeg_test_support::service_decode_context(),
            &carrier,
            Some(endpoints)
        )
        .unwrap(),
        Some(([-14.0, 16.5], true))
    );
}

fn bridge_record(attr: u16, refs: [u16; 5]) -> Bridge {
    Bridge {
        attr,
        sequence: 0,
        refs,
        sense: Sense::Forward,
        owner: None,
        offset: 0,
    }
}

fn loop_record(attr: u16, refs: [u16; 4]) -> Loop {
    Loop {
        attr,
        refs,
        offset: 0,
    }
}

fn coedge_record(attr: u16, refs: [u16; 9]) -> Coedge {
    Coedge {
        attr,
        refs,
        sense: Sense::Forward,
        offset: 0,
    }
}

#[test]
fn face_walk_rejects_a_loop_owned_by_another_bridge() {
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    let mut tables = Tables::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    tables
        .insert_loop(&ctx, loop_record(20, [0, 40, 11, 0]))
        .expect("loop");
    tables
        .insert_coedge(&ctx, coedge_record(40, [0, 0, 0, 40, 0, 0, 0, 0, 0]))
        .expect("coedge");

    let face = crate::brep::graph::walk_face(&ctx, &bridge, &tables).expect("face walk");

    assert!(face.loops.is_empty());
}

#[test]
fn face_walk_rejects_a_ring_owned_by_another_loop() {
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    let mut tables = Tables::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    tables
        .insert_loop(&ctx, loop_record(20, [0, 40, 10, 0]))
        .expect("loop");
    tables
        .insert_coedge(&ctx, coedge_record(40, [0, 21, 0, 40, 0, 0, 0, 0, 0]))
        .expect("coedge");

    let face = crate::brep::graph::walk_face(&ctx, &bridge, &tables).expect("face walk");

    assert!(face.loops.is_empty());
}

#[test]
fn face_walk_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    assert!(matches!(
        crate::brep::graph::walk_face(&ctx, &bridge, &Tables::default()),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn face_walk_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    assert!(matches!(
        crate::brep::graph::walk_face(&ctx, &bridge, &Tables::default()),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

fn face_color(face_attr: u16, color_attr: u16, face_seq: u32, rgb: [f32; 3]) -> entity::FaceColor {
    entity::FaceColor {
        face_attr,
        color_attr,
        face_seq,
        stream_order: 0,
        color: Color::new(rgb[0], rgb[1], rgb[2], 1.0).expect("valid color"),
        offset: usize::from(face_attr),
        target: None,
    }
}

fn face_color_version(face_attr: u16, seq: u32, stream_order: usize) -> entity::FaceColorVersion {
    entity::FaceColorVersion {
        face_attr,
        seq,
        stream_order,
    }
}

#[test]
fn current_uncolored_face_version_removes_an_older_color() {
    let colors = vec![face_color(700, 900, 1, [0.25, 0.5, 0.75])];

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            colors,
            vec![face_color_version(700, 1, 0), face_color_version(700, 2, 0)],
        )
        .expect("face colors")
    });

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 0);
}

#[test]
fn later_stream_replaces_an_equal_sequence_face_color() {
    let old = face_color(700, 900, 2, [0.25, 0.5, 0.75]);
    let mut current = face_color(700, 901, 2, [0.75, 0.5, 0.25]);
    current.stream_order = 1;

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            vec![old, current],
            vec![face_color_version(700, 2, 0), face_color_version(700, 2, 1)],
        )
        .expect("face colors")
    });

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].color_attr, 901);
    assert_eq!(unresolved, 0);
}

#[test]
fn conflicting_current_face_colors_remain_unresolved() {
    let colors = vec![
        face_color(700, 900, 2, [0.25, 0.5, 0.75]),
        face_color(700, 901, 2, [0.75, 0.5, 0.25]),
    ];

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            colors,
            vec![face_color_version(700, 2, 0), face_color_version(700, 2, 0)],
        )
        .expect("face colors")
    });

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 1);
}

#[test]
fn conflicting_reuse_of_one_color_identity_remains_unresolved() {
    let colors = vec![
        face_color(700, 900, 2, [0.25, 0.5, 0.75]),
        face_color(701, 900, 2, [0.75, 0.5, 0.25]),
    ];

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            colors,
            vec![face_color_version(700, 2, 0), face_color_version(701, 2, 0)],
        )
        .expect("face colors")
    });

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 2);
}

#[test]
fn canonical_edge_direction_uses_explicit_or_unique_forward_coedge() {
    use std::collections::BTreeMap;

    let record = |attr, refs, sense| Coedge {
        attr,
        refs,
        sense,
        offset: 0,
    };
    let edge = |attr, references| EdgeUse {
        attr,
        references,
        sequence: 0,
        offset: 0,
    };
    let mut coedges = BTreeMap::from([
        (
            10,
            record(10, [0, 0, 0, 0, 101, 0, 7, 0, 0], Sense::Reversed),
        ),
        (
            11,
            record(11, [0, 0, 0, 0, 102, 10, 7, 0, 0], Sense::Forward),
        ),
    ]);
    let prefixed_edge = edge(7, EdgeReferences::Compact { curve: 300 });
    let ctx = cadmpeg_test_support::service_decode_context();
    let canonical = |edge_use: &EdgeUse, coedges: &BTreeMap<u16, Coedge>| {
        let forward = crate::brep::graph::forward_coedges_by_edge(&ctx, coedges).expect("forward coedge index");
        crate::brep::graph::canonical_coedge_attr(&ctx, 7, Some(edge_use), coedges, &forward)
            .expect("canonical coedge lookup")
    };

    assert_eq!(canonical(&prefixed_edge, &coedges), Some(11));

    let bare_edge = edge(7, EdgeReferences::Bare([11, 0, 0, 300, 0, 0]));
    assert_eq!(canonical(&bare_edge, &coedges), Some(11));

    let sentinel_edge = edge(7, EdgeReferences::Bare([1, 0, 0, 300, 0, 0]));
    assert_eq!(canonical(&sentinel_edge, &coedges), Some(11));

    let reversed_edge = edge(7, EdgeReferences::Bare([10, 0, 0, 300, 0, 0]));
    assert_eq!(canonical(&reversed_edge, &coedges), None);

    coedges.insert(
        12,
        record(12, [0, 0, 0, 0, 103, 0, 7, 0, 0], Sense::Forward),
    );
    assert_eq!(canonical(&prefixed_edge, &coedges), None);
}

#[test]
fn boundary_coedge_uses_ring_endpoint_but_reciprocal_twin_supplies_edge_end() {
    use std::collections::BTreeMap;

    let record = |attr, refs| Coedge {
        attr,
        refs,
        sense: Sense::Forward,
        offset: 0,
    };
    let boundary = BTreeMap::from([(10, record(10, [0, 0, 0, 0, 101, 10, 7, 0, 0]))]);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        crate::brep::graph::edge_end_vuse(&ctx, 10, 102, &boundary).expect("edge end lookup"),
        102
    );

    let reciprocal = BTreeMap::from([
        (10, record(10, [0, 0, 0, 0, 101, 11, 7, 0, 0])),
        (11, record(11, [0, 0, 0, 0, 102, 10, 7, 0, 0])),
    ]);
    assert_eq!(
        crate::brep::graph::edge_end_vuse(&ctx, 10, 103, &reciprocal).expect("edge end lookup"),
        102
    );
}

#[test]
fn normalized_surface_parameter_reversal_toggles_face_sense() {
    use cadmpeg_ir::topology::Sense;

    assert_eq!(crate::brep::graph::surface_sense(Sense::Forward, false), Sense::Forward);
    assert_eq!(
        crate::brep::graph::surface_sense(Sense::Reversed, false),
        Sense::Reversed
    );
    assert_eq!(crate::brep::graph::surface_sense(Sense::Forward, true), Sense::Reversed);
    assert_eq!(crate::brep::graph::surface_sense(Sense::Reversed, true), Sense::Forward);
}

#[test]
fn geometry_free_stream_does_not_report_synthetic_body_grouping() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let decoded = crate::brep::graph::decode_body(&ctx, &[], &cadmpeg_ir::stream_name!("empty"))
        .expect("valid exactness fields");

    assert!(decoded.faces.is_empty());
    assert!(!decoded.stats.synthetic_body_grouping);
}

#[test]
fn native_brep_carrier_collection_refuses_before_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let body = crate::test_support::parasolid::triangle_body();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "index SLDPRT surface carriers",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy)?;
            crate::brep::graph::decode_body(
                &ctx,
                &body,
                &cadmpeg_ir::stream_name!("candidate-admission"),
            )
        },
    );
    assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index SLDPRT surface carriers"));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&body, &arena, &DecodePolicy::service()).expect("root");
    crate::brep::graph::decode_body(
        &ctx,
        &body,
        &cadmpeg_ir::stream_name!("candidate-admission"),
    )
    .expect("service profile admits native B-rep candidates");
}

#[test]
fn typed_body_records_preserve_stored_sheet_kind_and_links() {
    use crate::brep::typed::{BodyNode, FaceNode, Facts, RegionNode, ShellNode};
    use cadmpeg_ir::topology::BodyKind;
    use std::collections::BTreeSet;

    let facts = Facts {
        bodies: vec![BodyNode {
            attr: 3,
            node_id: 7,
            topology_refs: [7, 8, 9, 10, 1, 12, 11],
            ownership_refs: Vec::new(),
            kind: BodyKind::Sheet,
            offset: 1,
            end: 2,
        }],
        shells: vec![ShellNode {
            attr: 7,
            node_id: 814,
            refs: [1, 3, 1, 38, 1, 1, 39, 1],
            offset: 3,
            end: 4,
        }],
        regions: vec![
            RegionNode {
                attr: 11,
                node_id: 244,
                refs: [1, 3, 39, 1, 44],
                offset: 5,
                end: 6,
            },
            RegionNode {
                attr: 39,
                node_id: 815,
                refs: [1, 3, 1, 11, 7],
                offset: 7,
                end: 8,
            },
        ],
        faces: vec![FaceNode {
            attr: 100,
            node_id: 900,
            refs: [1, 1, 49, 7, 8],
            sense: Sense::Forward,
            offset: 9,
            end: 10,
        }],
    };
    let mut tables = Tables::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    tables
        .insert_bridge(
            &ctx,
            Bridge {
                attr: 100,
                sequence: 0,
                refs: [1, 1, 49, 7, 8],
                sense: Sense::Forward,
                owner: None,
                offset: 11,
            },
        )
        .expect("bridge");

    let records = crate::brep::graph::typed_body_records(&ctx, &facts, &tables)
        .expect("body record allocation")
        .expect("typed body records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, BodyKind::Sheet);
    assert_eq!(records[0].regions.len(), 2);
    assert!(records[0].refs.contains(&100));
    assert_eq!(records[0].regions[1].shells[0].refs, vec![100]);
    assert_eq!(
        facts
            .hierarchies(&ctx, &BTreeSet::from([100]))
            .expect("hierarchy allocation")
            .expect("typed hierarchy")[0]
            .body
            .kind,
        BodyKind::Sheet
    );
}
