// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::framing::node_kind::NodeKind;
use crate::test_support::test_bytes::put_f64;
use crate::test_support::test_bytes::put_ref;
use crate::test_support::test_bytes::put_vec3;
use crate::test_support::test_bytes::record;
use crate::test_support::test_deltas::partnered_trimmed_topology_partition_stream;
use crate::test_support::test_deltas::variable_status_framed_deltas_stream;
use crate::test_support::test_prt::prt_with_partition;
use crate::test_support::test_streams::charted_intersection_with_edge_endpoint_witnesses_stream;
use crate::test_support::test_streams::deltas_intersection_curve_stream;
use crate::test_support::test_streams::offset_surface_topology_partition_stream;
use crate::test_support::test_streams::topology_partition_stream;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::NxLossCode;
use crate::topology::{
    intersection_data_curves, FaceLoopError, FaceLoopFailure, Graph, Node, NodeCandidate,
    TYPE_38_SCHEMA_HEADER,
};
use crate::NxCodec;
use cadmpeg_core::decode::View;

#[test]
fn topology_graph_parse_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = topology_partition_stream();

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = Graph::parse(ctx, &bytes).expect_err("collection refusal");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems)
            );
        },
    );
}

#[test]
fn topology_graph_parse_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = topology_partition_stream();

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let error = Graph::parse(ctx, &bytes).expect_err("retained refusal");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes)
            );
        },
    );
}

#[test]
fn topology_graph_parse_refuses_scoped_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = topology_partition_stream();

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            let error = Graph::parse(ctx, &bytes).expect_err("scoped refusal");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes)
            );
        },
    );
}

#[test]
fn topology_graph_parse_refuses_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = topology_partition_stream();

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_work_units = 0;
        },
        |ctx| {
            let error = Graph::parse(ctx, &bytes).expect_err("work refusal");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits)
            );
        },
    );
}

#[test]
fn topology_rejects_shell_with_broken_face_ownership_chain() {
    crate::test_support::with_decode_context(|ctx| {
        let valid = topology_partition_stream();
        let graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &valid)
        })
        .unwrap();
        assert_eq!(graph.body_shape_shells(ctx).unwrap().len(), 1);
        assert!(graph.has_body_shape_shell(ctx).unwrap());

        let mut broken = valid;
        let face = broken
            .windows(2)
            .position(|window| window == [0, 14])
            .expect("face record");
        put_ref(&mut broken, face + 24, 99);
        let broken_graph =
            crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &broken)).unwrap();
        assert!(broken_graph.body_shape_shells(ctx).unwrap().is_empty());
        assert!(!broken_graph.has_body_shape_shell(ctx).unwrap());

        let mut independent_previous = topology_partition_stream();
        let face = independent_previous
            .windows(2)
            .position(|window| window == [0, 14])
            .expect("face record");
        put_ref(&mut independent_previous, face + 20, 99);
        let independent_previous_graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &independent_previous)
        })
        .unwrap();
        assert_eq!(independent_previous_graph.body_shape_shells(ctx).unwrap().len(), 1);
        assert!(independent_previous_graph
            .has_body_shape_shell(ctx)
            .unwrap());
    });
}

#[test]
fn topology_retains_shell_body_identity_without_body_record() {
    crate::test_support::with_decode_context(|ctx| {
        let mut stream = topology_partition_stream();
        let body = stream
            .windows(4)
            .position(|window| window == [0, 12, 0, 2])
            .expect("body record");
        stream[body..body + 24].fill(0xff);

        let graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &stream)
        })
        .unwrap();
        assert!(graph.node(NodeKind::Body, 2).is_none());
        assert_eq!(graph.body_shape_shells(ctx).unwrap().len(), 1);

        let mut input = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut input, &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.bodies[0].id.as_str(), "nx:s0:body#2");
        assert_eq!(result.ir().model.faces.len(), 1);
        let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    });
}

#[test]
fn topology_accepts_complete_fixed_nodes_across_the_u32_identifier_domain() {
    crate::test_support::with_decode_context(|ctx| {
        let mut stream = topology_partition_stream();
        let fixed_nodes = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &stream)
        })
        .unwrap()
        .test_nodes()
        .filter(|node| node.kind != NodeKind::Fin)
        .map(|node| (node.pos(), node.shift))
        .collect::<Vec<_>>();
        for (ordinal, (pos, shift)) in fixed_nodes.into_iter().enumerate() {
            let node_id = u32::MAX - u32::try_from(ordinal).unwrap();
            stream[pos + 4 + shift..pos + 8 + shift].copy_from_slice(&node_id.to_be_bytes());
        }

        let graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &stream)
        })
        .unwrap();

        assert_eq!(graph.body_shape_shells(ctx).unwrap().len(), 1);
        assert_eq!(graph.body_shape_face_count(ctx).unwrap(), 1);
        assert!(crate::test_support::with_decode_context(
            |ctx| graph.has_complete_body_topology(ctx)
        )
        .unwrap());
        assert!(graph
            .test_nodes()
            .filter(|node| node.kind != NodeKind::Fin)
            .all(|node| View::u32_be_at(&node.bytes, 4).is_some_and(|id| id > 1_000_000)));

        let mut input = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut input, &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.faces.len(), 1);
        let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    });
}

#[test]
fn topology_accepts_high_node_identity_among_low_identity_neighbors() {
    crate::test_support::with_decode_context(|ctx| {
        let mut stream = topology_partition_stream();
        let initial_graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &stream)
        })
        .unwrap();
        let face = initial_graph.node(NodeKind::Face, 4).unwrap();
        let node_id_offset = face.pos() + 4 + face.shift;
        stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());

        let graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &stream)
        })
        .unwrap();

        assert_eq!(
            graph
                .node(NodeKind::Face, 4)
                .and_then(crate::topology::Node::node_id),
            Some(u32::MAX)
        );
        assert_eq!(graph.body_shape_face_count(ctx).unwrap(), 1);
        assert!(crate::test_support::with_decode_context(
            |ctx| graph.has_complete_body_topology(ctx)
        )
        .unwrap());
    });
}

#[test]
fn topology_admits_high_identity_carriers_from_typed_topology_slots() {
    let mut stream = topology_partition_stream();
    let initial_graph =
        crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    for (kind, xmt) in [(50, 6), (30, 9), (29, 11)] {
        let node = initial_graph
            .node(NodeKind::try_from(kind).unwrap(), xmt)
            .unwrap();
        let node_id_offset = node.pos() + 4 + node.shift;
        stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    }

    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    for (kind, xmt) in [(50, 6), (30, 9), (29, 11)] {
        assert_eq!(
            graph
                .node(NodeKind::try_from(kind).unwrap(), xmt)
                .and_then(|node| node.u32_at(4)),
            Some(u32::MAX)
        );
    }
    assert!(
        crate::test_support::with_decode_context(|ctx| graph.has_complete_body_topology(ctx))
            .unwrap()
    );

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(result.ir().model.points.len(), 1);
}

#[test]
fn topology_rejects_unreferenced_high_identity_carrier() {
    let mut stream = topology_partition_stream();
    let plane_pos = stream
        .windows(4)
        .position(|window| window == [0, 50, 0, 6])
        .unwrap();
    let mut unreferenced = stream[plane_pos..plane_pos + 91].to_vec();
    put_ref(&mut unreferenced, 2, 99);
    unreferenced[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
    stream.extend(unreferenced);
    let line_pos = stream
        .windows(4)
        .position(|window| window == [0, 30, 0, 9])
        .unwrap();
    let mut successor = stream[line_pos..line_pos + 67].to_vec();
    put_ref(&mut successor, 2, 100);
    stream.extend(successor);

    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    assert!(graph.node(NodeKind::Plane, 99).is_none());
    assert!(graph.node(NodeKind::Line, 100).is_some());
    assert!(
        crate::test_support::with_decode_context(|ctx| graph.has_complete_body_topology(ctx))
            .unwrap()
    );
}

#[test]
fn topology_admits_high_identity_region_from_shell_ownership() {
    let mut stream = topology_partition_stream();
    let initial_graph =
        crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let region = initial_graph.node(NodeKind::Region, 12).unwrap();
    let node_id_offset = region.pos + 4 + region.shift;
    stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());

    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    assert_eq!(
        graph.node(NodeKind::Region, 12).and_then(Node::node_id),
        Some(u32::MAX)
    );
    assert!(
        crate::test_support::with_decode_context(|ctx| graph.has_complete_body_topology(ctx))
            .unwrap()
    );
}

#[test]
fn topology_closes_high_identity_procedural_surface_dependencies() {
    let mut stream = offset_surface_topology_partition_stream();
    let initial_graph =
        crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let offset = initial_graph.node(NodeKind::OffsetSurface, 12).unwrap();
    let node_id_offset = offset.pos + 4 + offset.shift;
    stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());

    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    assert_eq!(
        graph
            .node(NodeKind::OffsetSurface, 12)
            .and_then(|node| node.u32_at(4)),
        Some(u32::MAX)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| graph.offset_surfaces(ctx))
            .unwrap()
            .len(),
        1
    );

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
}

#[test]
fn topology_projection_route_refuses_collection_limit() {
    let stream = offset_surface_topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    crate::test_support::with_decode_context_over(
        &stream,
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = graph
                .offset_surfaces(ctx)
                .expect_err("offset surface collection refusal");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
            );
        },
    );
}

#[test]
fn topology_projection_route_refuses_retained_limit() {
    let stream = offset_surface_topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    crate::test_support::with_decode_context_over(
        &stream,
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let error = graph
                .offset_surfaces(ctx)
                .expect_err("offset surface retained refusal");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
            );
        },
    );
}

#[test]
fn topology_carrier_references_refuse_collection_limit() {
    let stream = topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    crate::test_support::with_decode_context_over(
        &stream,
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = graph
                .referenced_carrier_xmts(ctx)
                .expect_err("carrier reference refusal");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
            );
        },
    );
}

#[test]
fn topology_body_shells_refuse_at_shell_face_identities() {
    let stream = topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let error = crate::test_support::resource_refusal_at(
        &stream,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "NX shell face identities",
        |ctx| graph.body_shape_shells(ctx),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn body_shape_projections_do_not_retain_face_id_vectors() {
    let stream = topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            assert!(graph.has_body_shape_shell(ctx).unwrap());
            assert_eq!(graph.body_shape_face_count(ctx).unwrap(), 1);
            let mut body_id = None;
            graph
                .visit_body_shape_body_ids(ctx, |candidate| {
                    body_id = Some(candidate);
                    Ok(())
                })
                .unwrap();
            assert_eq!(body_id, Some(2));
        },
    );
}

#[test]
fn body_shape_presence_stops_at_the_first_valid_shell() {
    use cadmpeg_core::decode::ResourceDimension;

    // Use an unrelated node in the prefix graph to match the trailing graph's
    // index size. The work cap then measures the same prefix lookups.
    let mut first_stream = topology_partition_stream();
    let mut unrelated_point = record(29, 40);
    put_ref(&mut unrelated_point, 2, 24);
    put_vec3(&mut unrelated_point, 16, [0.01, 0.02, 0.03]);
    first_stream.extend(unrelated_point);
    let first_graph = crate::test_support::with_decode_context(|ctx| {
        Graph::parse(ctx, &first_stream)
    })
    .unwrap();

    let mut trailing_stream = topology_partition_stream();
    let mut trailing_shell = record(13, 24);
    for (offset, reference) in [
        (2, 13),
        (8, 1),
        (10, 2),
        (12, 1),
        (14, 1),
        (16, 1),
        (18, 1),
        (20, 12),
        (22, 1),
    ] {
        put_ref(&mut trailing_shell, offset, reference);
    }
    trailing_stream.extend(trailing_shell);
    let trailing_graph = crate::test_support::with_decode_context(|ctx| {
        Graph::parse(ctx, &trailing_stream)
    })
    .unwrap();
    assert_eq!(first_graph.of_kind(NodeKind::Shell).len(), 1);
    assert_eq!(trailing_graph.of_kind(NodeKind::Shell).len(), 2);
    assert_eq!(first_graph.keys.len(), trailing_graph.keys.len());
    assert_eq!(
        first_graph.of_kind(NodeKind::Face).len(),
        trailing_graph.of_kind(NodeKind::Face).len()
    );

    let allows_presence = |graph: &Graph, work_limit| {
        match crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = work_limit,
            |ctx| graph.has_body_shape_shell(ctx),
        ) {
            Ok(true) => true,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits =>
            {
                false
            }
            result => panic!("presence query did not return true or a work refusal: {result:?}"),
        }
    };

    let mut high = 1_u64;
    while !allows_presence(&first_graph, high) {
        high = high.checked_mul(2).expect("presence work bound");
    }
    let mut low = 0_u64;
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if allows_presence(&first_graph, middle) {
            high = middle;
        } else {
            low = middle;
        }
    }

    assert!(allows_presence(&trailing_graph, high));
}

fn face_ring_refusal(
    adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> FaceLoopError {
    let stream = topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();

    crate::test_support::with_decode_context_over(&stream, adjust, |ctx| {
        graph
            .face_loop_rings(ctx, 4)
            .expect_err("face ring resource refusal")
    })
}

#[test]
fn topology_face_ring_refuses_collection_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_collection_items = 0;
    };
    assert!(matches!(face_ring_refusal(adjust_policy),
        FaceLoopError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn topology_face_ring_refuses_retained_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = 0;
    };
    assert!(matches!(face_ring_refusal(adjust_policy),
        FaceLoopError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn topology_face_ring_refuses_scoped_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_materialized_bytes = 0;
    };
    assert!(matches!(face_ring_refusal(adjust_policy),
        FaceLoopError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn topology_face_ring_refuses_work_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_work_units = 0;
    };
    assert!(matches!(face_ring_refusal(adjust_policy),
        FaceLoopError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn topology_resolves_kernel_node_identity_only_within_one_unique_family() {
    let mut stream = topology_partition_stream();
    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    let face = graph.node(NodeKind::Face, 4).unwrap();
    let node_id = face.node_id().unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| graph.unique_xmt_by_node_id(
            ctx,
            NodeKind::Face,
            node_id
        ))
        .unwrap(),
        Some(4)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| graph.unique_xmt_by_node_id(
            ctx,
            NodeKind::Edge,
            node_id
        ))
        .unwrap(),
        Some(8)
    );

    let mut duplicate = face.bytes.clone();
    duplicate[3] = 39;
    stream.extend(duplicate);
    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert_eq!(
        graph.node(NodeKind::Face, 39).and_then(Node::node_id),
        Some(node_id)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| graph.unique_xmt_by_node_id(
            ctx,
            NodeKind::Face,
            node_id
        ))
        .unwrap(),
        None
    );
}

#[test]
fn topology_accepts_cached_last_face_and_implicit_region_identity() {
    crate::test_support::with_decode_context(|ctx| {
        let mut stream = topology_partition_stream();
        let shell = stream
            .windows(4)
            .position(|window| window == [0, 13, 0, 3])
            .expect("shell record");
        put_ref(&mut stream, shell + 22, 4);
        let region = stream
            .windows(4)
            .position(|window| window == [0, 19, 0, 12])
            .expect("region record");
        stream[region..region + 16].fill(0xff);
        let mut second_face = record(14, 39);
        put_ref(&mut second_face, 2, 20);
        put_f64(&mut second_face, 10, 0.000_2);
        put_ref(&mut second_face, 18, 1);
        put_ref(&mut second_face, 20, 1);
        put_ref(&mut second_face, 22, 1);
        put_ref(&mut second_face, 24, 3);
        put_ref(&mut second_face, 26, 6);
        second_face[28] = b'+';
        stream.extend(second_face);

        let graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &stream)
        })
        .unwrap();
        assert!(graph.node(NodeKind::Region, 12).is_none());
        assert_eq!(graph.body_shape_shells(ctx).unwrap().len(), 1);
        assert_eq!(graph.body_shape_face_count(ctx).unwrap(), 2);
        assert!(graph.has_body_shape_shell(ctx).unwrap());
        let mut body_id = None;
        graph
            .visit_body_shape_body_ids(ctx, |candidate| {
                body_id = Some(candidate);
                Ok(())
            })
            .unwrap();
        assert_eq!(body_id, Some(2));

        let mut input = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec
            .decode(&mut input, &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model.regions.len(), 1);
        assert_eq!(result.ir().model.regions[0].id.as_str(), "nx:s0:region#12");
        assert_eq!(result.ir().model.faces.len(), 2);
        let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    });
}

#[test]
fn topology_rejects_nonreciprocal_fin_ring() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 8, 99);
    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert!(crate::test_support::with_decode_context(|ctx| graph.face_loop_rings(ctx, 4)).is_err());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.loops.is_empty());
    assert!(result.ir().model.coedges.is_empty());
    assert!(result.ir().model.edges.is_empty());

    let mut broken_partner = topology_partition_stream();
    let fin = broken_partner
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut broken_partner, fin + 14, 99);
    let graph =
        crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &broken_partner)).unwrap();
    assert!(crate::test_support::with_decode_context(|ctx| graph.face_loop_rings(ctx, 4)).is_err());
}

#[test]
fn unresolved_fin_edge_records_face_boundary_loss() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("FIN");
    put_ref(&mut stream, fin + 16, 99);
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    assert!(matches!(
        crate::test_support::with_decode_context(|ctx| graph.face_loop_rings(ctx, 4)),
        Err(FaceLoopError::Invalid(FaceLoopFailure::UnresolvedFinEdge {
            loop_xmt: 5,
            fin_xmt: 7,
            edge_xmt: Some(99),
        }))
    ));

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .expect("decode");
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result.ir().model.loops.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == NxLossCode::TopologyFaceLoopUnresolved.kind()
            && loss.message.contains("FIN 7")
            && loss.message.contains("EDGE 99")
    }));
}

#[test]
fn admitted_loop_records_loss_when_its_edge_cannot_emit() {
    let mut stream = topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let vertex = graph.node(NodeKind::Vertex, 10).expect("vertex");
    put_ref(&mut stream, vertex.pos() + 16, 99);
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    assert!(crate::test_support::with_decode_context(|ctx| graph.face_loop_rings(ctx, 4)).is_ok());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .expect("decode");
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result.ir().model.loops.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == NxLossCode::TopologyLoopRingUnresolved.kind()
            && loss.message.contains("LOOP 5")
    }));
}

#[test]
fn linked_fin_ring_order_is_emitted() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let loop_node = graph.node(NodeKind::Loop, 5).expect("loop node");
    put_ref(&mut stream, loop_node.pos() + 10, 13);
    let first_fin = graph.node(NodeKind::Fin, 7).expect("first FIN");
    put_ref(&mut stream, first_fin.pos + 8, 6);
    let second_fin = graph.node(NodeKind::Fin, 13).expect("second FIN");
    put_ref(&mut stream, second_fin.pos + 10, 6);
    let mut third_fin = record(17, 23);
    put_ref(&mut third_fin, 2, 6);
    put_ref(&mut third_fin, 6, 5);
    put_ref(&mut third_fin, 8, 13);
    put_ref(&mut third_fin, 10, 7);
    put_ref(&mut third_fin, 12, 14);
    put_ref(&mut third_fin, 14, 1);
    put_ref(&mut third_fin, 16, 8);
    put_ref(&mut third_fin, 18, 12);
    third_fin[22] = b'+';
    stream.extend(third_fin);
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| graph.face_loop_rings(ctx, 4))
            .expect("source ring")[0]
            .1,
        vec![13, 7, 6]
    );

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .expect("decode");
    let ids = result.ir().model.loops[0]
        .coedges()
        .iter()
        .map(cadmpeg_ir::ids::CoedgeId::as_str)
        .collect::<Vec<_>>();
    assert!(
        ids[0].ends_with("#13") && ids[1].ends_with("#7") && ids[2].ends_with("#6"),
        "{ids:?}"
    );
}

#[test]
fn face_loop_chain_order_is_emitted() {
    let mut stream = partnered_trimmed_topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let face = graph.node(NodeKind::Face, 4).expect("first face");
    let first_loop = graph.node(NodeKind::Loop, 5).expect("first loop");
    let second_loop = graph.node(NodeKind::Loop, 21).expect("second loop");
    put_ref(&mut stream, face.pos() + 22, 21);
    put_ref(&mut stream, second_loop.pos + 12, 4);
    put_ref(&mut stream, second_loop.pos + 14, 5);
    put_ref(&mut stream, first_loop.pos + 14, 1);
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let source = crate::test_support::with_decode_context(|ctx| graph.face_loop_rings(ctx, 4))
        .expect("linked loop chain");
    assert_eq!(
        source.iter().map(|(xmt, _)| *xmt).collect::<Vec<_>>(),
        vec![21, 5]
    );

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .expect("decode");
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str().ends_with("#4"))
        .expect("face");
    let ids = face
        .loops
        .iter()
        .map(cadmpeg_ir::ids::LoopId::as_str)
        .collect::<Vec<_>>();
    assert!(ids[0].ends_with("#21") && ids[1].ends_with("#5"), "{ids:?}");
}

#[test]
fn topology_accepts_fixed_record_envelope_escape() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    stream.insert(fin + 2, 0xff);
    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert_eq!(
        graph
            .node(NodeKind::Fin, 7)
            .unwrap()
            .attribute_field_offset(),
        Some(fin + 5)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| graph.face_loop_rings(ctx, 4))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn topology_prefers_escaped_body_shape_over_direct_extended_xmt() {
    crate::test_support::with_decode_context(|ctx| {
        let mut stream = topology_partition_stream();
        let shell = stream
            .windows(4)
            .position(|window| window == [0, 13, 0, 3])
            .expect("shell record");
        stream.insert(shell + 2, 0xff);

        let graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &stream)
        })
        .unwrap();
        assert_eq!(
            graph.node(NodeKind::Shell, 3).map(super::Node::pos),
            Some(shell)
        );
        assert_eq!(graph.body_shape_shells(ctx).unwrap().len(), 1);
        assert_eq!(graph.body_shape_face_count(ctx).unwrap(), 1);
    });
}

#[test]
fn topology_iterates_each_record_family_in_physical_order() {
    let mut stream = Vec::new();
    for (xmt, x) in [(77, 0.01), (3, 0.02)] {
        let mut point = record(29, 40);
        put_ref(&mut point, 2, xmt);
        put_vec3(&mut point, 16, [x, 0.0, 0.0]);
        stream.extend(point);
    }

    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert_eq!(
        graph
            .of_kind(NodeKind::Point)
            .iter()
            .map(super::Node::xmt)
            .collect::<Vec<_>>(),
        vec![77, 3]
    );
}

#[test]
fn topology_invalid_candidate_cannot_shadow_later_valid_record() {
    let mut stream = record(14, 39);
    put_ref(&mut stream, 2, 4);
    stream.extend(topology_partition_stream());

    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    let face = graph.node(NodeKind::Face, 4).expect("valid later FACE");
    assert!(face.pos() >= 39);
    assert!(face.face_fields().is_some());
}

#[test]
fn topology_selects_one_candidate_at_an_ambiguous_record_offset() {
    let mut stream = vec![0; 26];
    stream[..7].copy_from_slice(&[0, 12, 0xff, 0xfe, 0x00, 0x02, 0x01]);
    let mut successor = record(12, 24);
    put_ref(&mut successor, 2, 3);
    stream.extend_from_slice(&successor);
    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert_eq!(graph.of_kind(NodeKind::Body).len(), 2);
    assert_eq!(graph.node_at(0).map(super::Node::xmt), Some(65_536));
    assert_eq!(graph.node_at(26).map(super::Node::xmt), Some(3));
}

#[test]
fn topology_disambiguates_direct_large_index_from_escaped_compact_record() {
    let mut stream = vec![0; 25];
    stream[..6].copy_from_slice(&[0, 17, 0xff, 0x7f, 0x00, 0x01]);
    for index in 0..8 {
        put_ref(&mut stream, 6 + index * 2, 2);
    }
    stream[22..24].copy_from_slice(b"++");
    stream[24] = b'+';

    let mut successor = record(17, 23);
    put_ref(&mut successor, 2, 7);
    for index in 0..9 {
        put_ref(&mut successor, 4 + index * 2, 2);
    }
    successor[22] = b'+';
    stream.extend_from_slice(&successor);

    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert_eq!(graph.node_at(0).map(super::Node::xmt), Some(32_896));
    assert_eq!(graph.node_at(0).map(crate::topology::Node::end), Some(25));
    assert_eq!(graph.node_at(25).map(super::Node::xmt), Some(7));

    let mut ambiguous = stream[..25].to_vec();
    ambiguous.extend_from_slice(&[0; 5]);
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(
            ctx, &ambiguous
        ))
        .unwrap()
        .node_at(0)
        .is_none()
    );
}

#[test]
fn topology_rejects_duplicate_fixed_record_identity() {
    let mut first = record(29, 40);
    put_ref(&mut first, 2, 11);
    put_vec3(&mut first, 16, [0.01, 0.02, 0.03]);
    let mut duplicate = record(29, 40);
    put_ref(&mut duplicate, 2, 11);
    put_vec3(&mut duplicate, 16, [0.04, 0.05, 0.06]);
    first.extend(duplicate);

    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &first))
            .unwrap();
    assert!(graph.node(NodeKind::Point, 11).is_none());
    assert!(graph.of_kind(NodeKind::Point).is_empty());
}

#[test]
fn topology_rejects_duplicate_identity_instead_of_preferring_body_shape() {
    let mut stream = topology_partition_stream();
    let mut duplicate = record(13, 24);
    put_ref(&mut duplicate, 2, 3);
    put_ref(&mut duplicate, 8, 2);
    put_ref(&mut duplicate, 10, 2);
    put_ref(&mut duplicate, 12, 2);
    put_ref(&mut duplicate, 14, 4);
    put_ref(&mut duplicate, 16, 0);
    put_ref(&mut duplicate, 18, 0);
    put_ref(&mut duplicate, 20, 12);
    put_ref(&mut duplicate, 22, 0);
    stream.extend(duplicate);

    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert!(graph.node(NodeKind::Shell, 3).is_none());
}

#[test]
fn topology_rejects_overlapping_candidates_without_ranking() {
    let first = NodeCandidate {
        kind: NodeKind::Point,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(11).unwrap(),
        pos: 0,
        shift: 0,
        end: 24,
    };
    let second = NodeCandidate {
        kind: NodeKind::Point,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(12).unwrap(),
        pos: 8,
        shift: 0,
        end: 32,
    };

    assert!(crate::test_support::with_decode_context(|ctx| {
        Graph::select_non_overlapping_candidates(ctx, &[], &[first, second])
            .map(|(nodes, _reservation)| nodes)
    })
    .unwrap()
    .is_empty());
}

#[test]
fn topology_ownership_candidate_cannot_suppress_typed_candidate() {
    let mut face = record(14, 39);
    put_ref(&mut face, 2, 4);
    put_f64(&mut face, 10, 0.000_2);
    put_ref(&mut face, 18, 1);
    put_ref(&mut face, 20, 1);
    put_ref(&mut face, 22, 1);
    put_ref(&mut face, 24, 3);
    put_ref(&mut face, 26, 6);
    face[28] = b'+';

    let mut stream = vec![0, 12];
    stream.extend(face);

    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    assert!(graph.node(NodeKind::Face, 4).is_some());
    assert!(graph.node(NodeKind::Body, 14).is_none());
}

#[test]
fn topology_resolves_ownership_overlap_before_duplicate_identity() {
    let mut outer = record(12, 24);
    put_ref(&mut outer, 2, 7);
    outer[8..10].copy_from_slice(&[0, 12]);
    put_ref(&mut outer, 10, 7);
    let mut successor = record(12, 24);
    put_ref(&mut successor, 2, 8);

    let mut stream = outer;
    stream.extend(successor);

    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    assert_eq!(graph.node(NodeKind::Body, 7).map(super::Node::pos), Some(0));
    assert_eq!(
        graph.node(NodeKind::Body, 8).map(super::Node::pos),
        Some(24)
    );
}

#[test]
fn topology_retains_non_overlapping_ownership_records() {
    let graph = crate::test_support::with_decode_context(|ctx| {
        Graph::parse(ctx, &topology_partition_stream())
    })
    .unwrap();

    assert!(graph.node(NodeKind::Body, 2).is_some());
    assert!(graph.node(NodeKind::Region, 12).is_some());
}

#[test]
fn topology_resolves_overlap_before_duplicate_identity() {
    let stream = vec![0; 40];
    let outer = NodeCandidate {
        kind: NodeKind::Point,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(11).unwrap(),
        pos: 0,
        shift: 0,
        end: 40,
    };
    let embedded = NodeCandidate {
        kind: NodeKind::Point,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(11).unwrap(),
        pos: 8,
        shift: 0,
        end: 32,
    };

    assert!(
        crate::test_support::with_decode_context(|ctx| Graph::select_unique_candidates(
            ctx,
            &[outer, embedded]
        )
        .map(|(nodes, _reservation)| nodes))
        .unwrap()
        .is_empty()
    );
    let selected = crate::test_support::with_decode_context(|ctx| {
        let (non_overlapping, _reservation) =
            Graph::select_non_overlapping_candidates(ctx, &stream, &[outer, embedded])?;
        Graph::select_unique_candidates(ctx, &non_overlapping).map(|(nodes, _reservation)| nodes)
    })
    .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].pos, outer.pos);
    assert_eq!(selected[0].end(), outer.end());
}

#[test]
fn topology_rejects_status_framed_delta_as_fixed_record() {
    let graph = crate::test_support::with_decode_context(|ctx| {
        Graph::parse(ctx, &variable_status_framed_deltas_stream())
    })
    .unwrap();

    assert!(graph.of_kind(NodeKind::Loop).is_empty());
}

#[test]
fn intersection_data_requires_complete_schema_header() {
    let source = deltas_intersection_curve_stream();
    let header_start = source
        .windows(TYPE_38_SCHEMA_HEADER.len())
        .position(|window| window == TYPE_38_SCHEMA_HEADER)
        .expect("schema header");
    let after_header = header_start + TYPE_38_SCHEMA_HEADER.len();
    let record_start = source[after_header..]
        .iter()
        .position(|byte| *byte == 0x5a)
        .map(|offset| after_header + offset)
        .expect("standalone intersection-data record");

    let mut incomplete_header =
        source[header_start..header_start + TYPE_38_SCHEMA_HEADER.len() - 1].to_vec();
    incomplete_header.push(0xfe);
    incomplete_header.extend_from_slice(&source[record_start..]);
    assert!(
        crate::test_support::with_decode_context(|ctx| intersection_data_curves(
            ctx,
            &incomplete_header
        ))
        .unwrap()
        .is_empty()
    );
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx,
            &incomplete_header
        ))
        .unwrap()
        .records
        .iter()
        .all(|record| record.kind() != 90)
    );
}

#[test]
fn intersection_data_duplicates_admit_identity_search_and_keep_only_output_storage() {
    use cadmpeg_core::decode::ResourceDimension;
    let mut stream = deltas_intersection_curve_stream();
    let start = stream.iter().rposition(|byte| *byte == 0x5a).unwrap();
    let duplicate = stream[start..].to_vec();
    stream.extend_from_slice(&duplicate);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes =
                4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<super::CompositeCurve>());
        },
        |ctx| {
            let curves = intersection_data_curves(ctx, &stream).unwrap();
            assert_eq!(curves.len(), 1);
            assert_eq!(curves[0].xmt, 12);
        },
    );
    // Core admits the first B-tree insertion as three node passes. The node
    // bound has eleven keys, sixteen pointer words, and two alignment pads.
    let first_insert_work = cadmpeg_core::decode::u64_from_index(3 * (
        11 * std::mem::size_of::<u32>()
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<usize>()
    ));
    let before_duplicate = cadmpeg_core::decode::u64_from_index(stream.len()) + first_insert_work;
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = before_duplicate + 3,
        |ctx| {
            let error = intersection_data_curves(ctx, &stream).unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "NX intersection identities"
                    && limit.used == before_duplicate && limit.additional == 4));
        },
    );
    for dimension in [ResourceDimension::WorkUnits, ResourceDimension::MaterializedBytes] {
        let error = crate::test_support::resource_refusal_at(
            &[], dimension, "NX intersection identities",
            |ctx| intersection_data_curves(ctx, &stream),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == "NX intersection identities"));
    }
}

#[test]
fn topology_preservation_stops_before_the_unused_node_suffix() {
    let mut baseline = Graph::default();
    for xmt in 2..4098 {
        baseline.kinds[NodeKind::Point.ordinal()].push(Node {
            kind: NodeKind::Point,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(xmt).unwrap(),
            pos: 0,
            end: 0,
            shift: 0,
            bytes: Vec::new(),
        });
    }
    let other = Graph::default();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 1,
        |ctx| {
            assert!(!baseline.is_preserved_by(ctx, &other).unwrap());
            assert!(ctx.resource_refusal().is_none());
        },
    );
}

#[test]
fn topology_ambiguous_boundaries_stop_before_the_unused_candidate_suffix() {
    let stream = vec![0_u8; 4098];
    let nodes: Vec<_> = (0..4096).map(|pos| NodeCandidate {
        kind: NodeKind::Point,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(2).unwrap(),
        pos,
        end: if pos == 0 { stream.len() - 1 } else { stream.len() },
        shift: 0,
    }).collect();
    crate::test_support::with_decode_context_over(
        &[],
        // The full overlap cluster requires 4096 visits. Its boundary test
        // stops after the unbounded first candidate and two bounded candidates.
        |policy| policy.limits.max_work_units = 4099,
        |ctx| {
            let (selected, storage) = Graph::select_non_overlapping_candidates(ctx, &stream, &nodes).unwrap();
            assert!(selected.is_empty());
            assert!(ctx.resource_refusal().is_none());
            drop(selected);
            drop(storage);
        },
    );
}

#[test]
fn topology_reference_views_reserve_only_one_as_null() {
    for raw in [0_u16, 1, 2, 32_767] {
        for (kind, len, offsets, sense) in [
            (NodeKind::Face, 39, &[8, 18, 20, 22, 24, 26][..], Some(28)),
            (
                NodeKind::Edge,
                32,
                &[8, 18, 20, 22, 24, 26, 28, 30][..],
                None,
            ),
            (
                NodeKind::Shell,
                24,
                &[8, 10, 12, 14, 16, 18, 20, 22][..],
                None,
            ),
            (NodeKind::Loop, 16, &[8, 10, 12, 14][..], None),
            (
                NodeKind::Fin,
                23,
                &[4, 6, 8, 10, 12, 14, 16, 18, 20][..],
                Some(22),
            ),
            (NodeKind::Vertex, 28, &[8, 10, 12, 14, 16][..], None),
        ] {
            let mut bytes = vec![0; len];
            for &offset in offsets {
                put_ref(&mut bytes, offset, raw);
            }
            if let Some(offset) = sense {
                bytes[offset] = b'+';
            }
            let node = Node {
                kind,
                xmt: crate::framing::xmt_reference::NonNullXmt::try_from(2).unwrap(),
                pos: 0,
                end: bytes.len(),
                shift: 0,
                bytes,
            };
            let references = match kind {
                NodeKind::Face => {
                    let f = node.face_fields().unwrap();
                    vec![f.attributes, f.next_face, f.loop_xmt, f.shell, f.surface]
                }
                NodeKind::Edge => {
                    let f = node.edge_fields().unwrap();
                    vec![f.attributes, f.fin, f.curve]
                }
                NodeKind::Shell => {
                    let f = node.shell_fields().unwrap();
                    vec![
                        f.attributes,
                        f.body,
                        f.next_shell,
                        f.first_face,
                        f.sentinel_0,
                        f.sentinel_1,
                        f.region,
                        f.last_face,
                    ]
                }
                NodeKind::Loop => {
                    let f = node.loop_fields().unwrap();
                    vec![f.attributes, f.fin, f.face, f.next_loop]
                }
                NodeKind::Fin => {
                    let f = node.fin_fields().unwrap();
                    vec![
                        f.attributes,
                        f.loop_xmt,
                        f.forward,
                        f.backward,
                        f.vertex,
                        f.edge,
                        f.other,
                        f.curve_xmt,
                    ]
                }
                NodeKind::Vertex => {
                    let f = node.vertex_fields().unwrap();
                    vec![f.attributes, f.point]
                }
                _ => panic!("topology reference family"),
            };
            for reference in references {
                assert_eq!(
                    reference.map(u32::from),
                    (raw != 1).then_some(u32::from(raw))
                );
            }
        }
    }
}

#[test]
fn body_shape_classification_refuses_at_the_shell_visit() {
    let stream = topology_partition_stream();
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "classify NX body shells",
        |ctx| graph.body_shape_shells(ctx),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn ownership_selection_searches_the_ordered_typed_spans() {
    let candidate = |kind, xmt, pos, end| super::NodeCandidate {
        kind,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(xmt).unwrap(),
        pos,
        shift: 0,
        end,
    };
    let selected = [
        candidate(NodeKind::Point, 3, 24, 60),
        candidate(NodeKind::Point, 4, 80, 116),
    ];
    let ownership = [
        candidate(NodeKind::Body, 2, 0, 24),
        candidate(NodeKind::Body, 5, 60, 81),
        candidate(NodeKind::Region, 6, 116, 140),
    ];
    let admitted = crate::test_support::with_decode_context(|ctx| {
        Graph::admit_disjoint_ownership(ctx, &ownership, &selected).map(|(admitted, _)| admitted)
    })
    .unwrap();
    assert_eq!(
        admitted
            .iter()
            .map(|candidate| candidate.xmt())
            .collect::<Vec<_>>(),
        [2, 6]
    );
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare NX ownership overlaps",
        |ctx| Graph::admit_disjoint_ownership(ctx, &ownership, &selected).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn topology_field_tolerances_require_finite_native_values() {
    let bytes = topology_partition_stream();
    crate::test_support::with_decode_context(|ctx| {
        let graph = Graph::parse(ctx, &bytes).unwrap();
        for (kind, identity, offset) in [
            (NodeKind::Face, 4, 10),
            (NodeKind::Edge, 8, 10),
            (NodeKind::Vertex, 10, 18),
        ] {
            let mut node = graph.node(kind, identity).unwrap().clone();
            for value in [
                -1.0,
                0.0,
                f64::MAX,
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ] {
                put_f64(&mut node.bytes, offset, value);
                let tolerance = match kind {
                    NodeKind::Face => node.face_fields().map(super::FaceFields::tolerance),
                    NodeKind::Edge => node.edge_fields().map(super::EdgeFields::tolerance),
                    NodeKind::Vertex => node.vertex_fields().map(super::VertexFields::tolerance),
                    _ => panic!("test family"),
                };
                if value.is_finite() {
                    assert_eq!(tolerance, Some(value));
                } else {
                    assert_eq!(tolerance, None);
                }
            }
        }
        assert!(graph
            .unique_curve_edge_witness(ctx, 9)
            .unwrap()
            .unwrap()
            .tolerance()
            .is_finite());
    });
}

#[test]
fn topology_body_census_stops_after_the_first_face_without_loops() {
    let mut stream = Vec::new();
    for (shell_xmt, face_xmt) in [(10, 11), (12, 13)] {
        let mut shell = record(13, crate::layout::shell_node::LEN);
        put_ref(&mut shell, 2, shell_xmt);
        for (offset, target) in [(8, 1), (10, 2), (12, 1), (14, face_xmt), (16, 1), (18, 1), (20, 3), (22, 1)] {
            put_ref(&mut shell, offset, target);
        }
        stream.extend(shell);
        let mut face = record(14, crate::layout::face_node::LEN);
        put_ref(&mut face, 2, face_xmt);
        for offset in [8, 18, 20, 22, 26, 29, 31, 33, 35, 37] {
            put_ref(&mut face, offset, 1);
        }
        put_ref(&mut face, 24, shell_xmt);
        face[28] = b'+';
        stream.extend(face);
    }
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    assert_eq!(graph.keys.len(), 4);
    assert_eq!(graph.of_kind(NodeKind::Shell).len(), 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX body shell faces",
        |ctx| graph.body_topology_census(ctx),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("the first shell visit must refuse");
    };
    assert_eq!(limit.additional, 1);
    // After complete shell classification and face counting, the census reads
    // one shell, one face, and one four-key lookup. The first face has no loops.
    let lookup_work = 4 * cadmpeg_core::decode::u64_from_index(
        std::mem::size_of::<NodeKind>() + std::mem::size_of::<u32>(),
    );
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = limit.used + 2 + lookup_work,
        |ctx| {
            assert_eq!(graph.body_topology_census(ctx).unwrap(), (false, 2));
            assert!(ctx.resource_refusal().is_none());
        },
    );
}

#[test]
fn topology_fin_visit_refuses_before_building_the_identity_index() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let error = Graph::default().fin_ring(
                ctx,
                3,
                crate::framing::xmt_reference::XmtTarget::from_wire(2).unwrap(),
            ).unwrap_err();
            let FaceLoopError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = error else {
                panic!("the FIN visit must refuse before its identity insertion");
            };
            assert_eq!(limit.operation, "walk NX FIN ring");
            assert_eq!(limit.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        },
    );
}
