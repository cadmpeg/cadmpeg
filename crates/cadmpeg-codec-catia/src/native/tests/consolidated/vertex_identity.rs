// SPDX-License-Identifier: Apache-2.0
//! vertex identity tests.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::{
    a5_native_edge_identity_stream, a5_native_edge_run_stream, a5_sphere_bound_edge_stream,
    a5_torus_bound_edge_stream, b2_cone_face_parameter_point_stream, b2_cone_face_stream,
    b2_cylinder_stream, b2_edge_node_stream, b2_owner_chart_stream, b2_plane_carrier_stream,
    grouped_surface_alias_stream,
};

#[test]
fn native_namespace_merges_shared_consolidated_vertex_identity() {
    let mut bytes = a5_native_edge_run_stream(6, 139, 142);
    bytes.extend_from_slice(&a5_native_edge_run_stream(9, 142, 151));
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_edge_runs.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 3);
    let shared = native
        .consolidated_vertex_identities
        .iter()
        .find(|vertex| vertex.identity == 142)
        .expect("shared consolidated vertex identity");
    assert_eq!(
        shared.incident_edge_nodes,
        [
            "catia:consolidated:edge-node#0",
            "catia:consolidated:edge-node#1"
        ]
    );
    assert_eq!(
        native.vertex_identity_ids(&native.consolidated_edge_nodes[0])[1],
        native.vertex_identity_ids(&native.consolidated_edge_nodes[1])[0]
    );
}

#[test]
fn native_vertex_identity_namespace_is_bounded_by_record_source() {
    let first = a5_native_edge_run_stream(6, 14, 15);
    let mut bytes = first.clone();
    bytes.extend_from_slice(&a5_native_edge_run_stream(9, 15, 16));
    let native = crate::native::CatiaNative::decode_with_record_ranges(
        &bytes,
        &[0..first.len(), first.len()..bytes.len()],
    );

    assert_eq!(native.consolidated_edge_runs.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 4);
    assert_eq!(
        native
            .consolidated_edge_nodes
            .iter()
            .map(|node| node.source_index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let repeated = native
        .consolidated_vertex_identities
        .iter()
        .filter(|vertex| vertex.identity == 15)
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2);
    assert_eq!(repeated[0].source_index, 0);
    assert_eq!(repeated[1].source_index, 1);
    assert_ne!(
        native.vertex_identity_ids(&native.consolidated_edge_nodes[0])[1],
        native.vertex_identity_ids(&native.consolidated_edge_nodes[1])[0]
    );
}

#[test]
fn explicit_vertex_encodings_share_one_complete_run_identity_namespace() {
    fn replace_node(mut stream: Vec<u8>, payload: &[u8]) -> Vec<u8> {
        let node = stream
            .windows(3)
            .position(|window| window == [0xb2, 0x03, 0x5e])
            .expect("edge node frame");
        stream.truncate(node);
        stream.extend_from_slice(&[
            0xb2,
            0x03,
            0x5e,
            u8::try_from(payload.len()).expect("bounded edge payload"),
            0x05,
        ]);
        stream.extend_from_slice(payload);
        stream
    }

    let mut bytes = a5_native_edge_run_stream(6, 14, 15);
    let tagged_u16 = replace_node(
        a5_native_edge_run_stream(9, 15, 16),
        &[37, 0x0a, 15, 0, 0x0a, 16, 0, 9, 5, 0x21],
    );
    bytes.extend_from_slice(&tagged_u16);
    let selector2 = replace_node(
        a5_native_edge_run_stream(12, 16, 17),
        &[49, 4 * 16 + 2, 4 * 17 + 2, 9, 5, 0x21],
    );
    bytes.extend_from_slice(&selector2);

    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_edge_runs.len(), 3);
    assert_eq!(native.consolidated_vertex_identities.len(), 4);
    assert_eq!(
        native.consolidated_edge_nodes[0].reference_encodings[1..3],
        [
            crate::native::CatiaAllocationReferenceEncoding::TaggedU8,
            crate::native::CatiaAllocationReferenceEncoding::TaggedU8,
        ]
    );
    assert_eq!(
        native.consolidated_edge_nodes[1].reference_encodings[1..3],
        [
            crate::native::CatiaAllocationReferenceEncoding::TaggedU16,
            crate::native::CatiaAllocationReferenceEncoding::TaggedU16,
        ]
    );
    assert_eq!(
        native.vertex_identity_ids(&native.consolidated_edge_nodes[0])[1],
        native.vertex_identity_ids(&native.consolidated_edge_nodes[1])[0]
    );
    assert_eq!(
        native.consolidated_edge_nodes[2].reference_encodings[1..3],
        [
            crate::native::CatiaAllocationReferenceEncoding::Selector2,
            crate::native::CatiaAllocationReferenceEncoding::Selector2,
        ]
    );
    assert_eq!(
        native.vertex_identity_ids(&native.consolidated_edge_nodes[1])[1],
        native.vertex_identity_ids(&native.consolidated_edge_nodes[2])[0]
    );
}

#[test]
fn native_edge_node_storage_refuses_collection_and_retained_limits() {
    let bytes = b2_edge_node_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let mut operations = std::collections::HashSet::new();
    for limit in 0..128 {
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            super::super::super::consolidated_edge_nodes(ctx, &bytes, &records, &[])
        });
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result {
            operations.insert(refusal.operation);
        }
    }
    for operation in [
        "catia_native_edge_frames",
        "catia_native_consolidated_edge_nodes",
    ] {
        assert!(
            operations.contains(operation),
            "missing charge for {operation}"
        );
    }
    let limited = crate::test_support::with_retained_limit(0, |ctx| {
        super::super::super::consolidated_edge_nodes(ctx, &bytes, &records, &[])
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
        if refusal.operation == "catia_native_edge_node_id")
    );
    let nodes = crate::test_support::with_service_context(|ctx| {
        super::super::super::consolidated_edge_nodes(ctx, &bytes, &records, &[])
    })
    .expect("service context admits the edge node");
    assert_eq!(nodes.len(), 1);
}

#[test]
fn native_namespace_retains_standalone_consolidated_edge_nodes() {
    let bytes = b2_edge_node_stream();
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one standalone consolidated edge node");
    };
    assert_eq!(u8::from(node.width), 1);
    assert_eq!(u8::from(node.flag), 0x03);
    assert_eq!(node.header_token, 5);
    assert_eq!(node.terminal_value, 8);
    assert_eq!(
        node.terminal_encoding,
        crate::native::CatiaAllocationReferenceEncoding::BackwardDistance,
    );
    assert_eq!(node.vertex_refs, [889, 895]);
    assert!(node.uses.is_none());
    assert_eq!(native.vertex_identity_ids(node), ["", ""]);
    assert!(native.consolidated_vertex_identities.is_empty());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store standalone consolidated edge node");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load standalone consolidated edge node"),
        native
    );
}

#[test]
fn native_namespace_attaches_oriented_uses_without_pcurves() {
    let bytes = a5_native_edge_identity_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one consolidated edge node");
    };
    let uses = node.uses.as_ref().expect("standalone edge-owned uses");
    assert_eq!(uses.references, [[4, 5], [5, 6]]);
}

#[test]
fn native_namespace_retains_resolved_consolidated_edge_supports_and_loci() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::Cylinder { .. })
    )));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));
    assert_eq!(
        run.endpoint_loci,
        run.shared_loci
            .as_ref()
            .map(|loci| [loci[0], loci[loci.len() - 1]])
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store resolved CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load resolved CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "consolidated_cylinders",
            &Vec::<crate::native::CatiaConsolidatedCylinder>::new(),
        )
        .expect("remove retained cylinders");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_plane_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let plane_stream = b2_plane_carrier_stream();
    let plane_carriers = crate::families::b2::records::b2_plane_carriers(&plane_stream);
    let plane_end = plane_carriers[0].end;
    let mut bytes = plane_stream[..plane_end].to_vec();
    for point in [[10.0f32, 20.0, 0.0], [11.0, 20.0, 1.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));
    bytes.extend_from_slice(&plane_stream[plane_carriers[2].pos..plane_carriers[2].end]);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated plane-bound edge run");
    };
    assert!(run
        .support_bindings
        .iter()
        .all(|binding| matches!(binding, Some(CatiaConsolidatedSupportBinding::Plane { .. }))));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store plane-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load plane-bound CATIA edge run"),
        native
    );

    let mut invalid = native.clone();
    let directionless_offset = invalid
        .consolidated_plane_carriers
        .iter()
        .find(|carrier| carrier.payload.selector() == 0xec)
        .expect("directionless class-27 carrier")
        .byte_offset;
    invalid.consolidated_edge_runs[0].support_bindings[0] =
        Some(CatiaConsolidatedSupportBinding::Plane {
            byte_offset: directionless_offset,
        });
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid directionless plane binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "consolidated_plane_carriers",
            &Vec::<crate::native::CatiaConsolidatedPlaneCarrier>::new(),
        )
        .expect("remove retained plane carriers");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_torus_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let native = crate::native::CatiaNative::decode(&a5_torus_bound_edge_stream());
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated torus edge run");
    };
    assert!(run
        .support_bindings
        .iter()
        .all(|binding| matches!(binding, Some(CatiaConsolidatedSupportBinding::Torus { .. }))));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store torus-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load torus-bound CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "consolidated_tori",
            &Vec::<crate::native::CatiaConsolidatedTorus>::new(),
        )
        .expect("remove retained tori");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_sphere_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let native = crate::native::CatiaNative::decode(&a5_sphere_bound_edge_stream());
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated sphere edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::Sphere { .. })
    )));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store sphere-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load sphere-bound CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "consolidated_spheres",
            &Vec::<crate::native::CatiaConsolidatedSphere>::new(),
        )
        .expect("remove retained spheres");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn cone_face_followed_by_spanning_parameter_point_terminates() {
    let bytes = b2_cone_face_parameter_point_stream();
    let split = b2_cone_face_stream().len() + 6;
    let records = crate::wire::records::consolidated_records_in_range_sources(
        &bytes,
        [[0..split, split..bytes.len()]],
    );
    let faces = crate::test_support::with_service_context(|ctx| {
        crate::native::consolidated_cone_faces(ctx, &bytes, &records, &[]).expect("service decode")
    });
    assert_eq!(faces.len(), 1);
    assert!(faces[0].parameter_points.is_empty());
}

#[test]
fn a_loaded_plane_carrier_direction_is_admitted_unit() {
    let native = crate::native::CatiaNative::decode(&b2_plane_carrier_stream());
    let direction3 = &native.consolidated_plane_carriers[1];
    let mut value = serde_json::to_value(direction3).expect("plane carrier JSON");
    assert_eq!(
        value["payload"]["direction"],
        serde_json::json!([1.0, 0.0, 0.0])
    );
    value["payload"]["direction"] = serde_json::json!([2.0, 0.0, 0.0]);
    let error = serde_json::from_value::<crate::native::CatiaConsolidatedPlaneCarrier>(value)
        .expect_err("a non-unit plane-carrier direction");
    assert!(
        error
            .to_string()
            .contains("direction is not a finite unit vector"),
        "{error}"
    );
}

#[test]
fn owner_chart_alias_binding_refuses_retained_limit() {
    let mut bytes = b2_owner_chart_stream(0x2b);
    bytes.extend(grouped_surface_alias_stream(0, 100, 0x148));
    bytes.extend(grouped_surface_alias_stream(1, 200, 0x148));
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut packets = native.consolidated_owner_packets.clone();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("owner chart fixture fits the input limit");
    assert!(matches!(
        super::super::super::resolve_owner_chart_support_aliases(
            &ctx,
            &mut packets,
            &native.alias_rows
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn owner_chart_alias_index_refuses_collection_limit() {
    let mut bytes = b2_owner_chart_stream(0x2b);
    bytes.extend(grouped_surface_alias_stream(0, 100, 0x148));
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut packets = native.consolidated_owner_packets.clone();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("owner chart fixture fits the input limit");
    assert!(matches!(
        super::super::super::resolve_owner_chart_support_aliases(
            &ctx,
            &mut packets,
            &native.alias_rows
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}
