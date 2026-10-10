// SPDX-License-Identifier: Apache-2.0

use super::{put_ref, record, Graph, NodeKind};

#[test]
fn topology_body_census_stops_after_the_first_face_without_loops() {
    let mut stream = Vec::new();
    for (shell_xmt, face_xmt) in [(10, 11), (12, 13)] {
        let mut shell = record(13, crate::layout::shell_node::LEN);
        put_ref(&mut shell, 2, shell_xmt);
        for (offset, target) in [
            (8, 1),
            (10, 2),
            (12, 1),
            (14, face_xmt),
            (16, 1),
            (18, 1),
            (20, 3),
            (22, 1),
        ] {
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
    // The first callback visits one face and its four-key lookup, then fails
    // before a FIN ring. Classification still visits the second shell: one
    // two-owner lookup, one face-chain visit and one four-key graph lookup.
    // The second callback counts its face but does not inspect its loops.
    let lookup_work = 4 * cadmpeg_core::decode::u64_from_index(
        std::mem::size_of::<NodeKind>() + std::mem::size_of::<u32>(),
    );
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = limit.used
                + 1
                + lookup_work
                + 1
                + 2 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<u32>())
                + 1
                + lookup_work;
        },
        |ctx| {
            assert_eq!(graph.body_topology_census(ctx).unwrap(), (false, 2));
            assert!(ctx.resource_refusal().is_none());
        },
    );
}

#[test]
fn topology_body_census_charges_only_visited_faces_in_one_shell() {
    let mut stream = record(13, crate::layout::shell_node::LEN);
    put_ref(&mut stream, 2, 10);
    for (offset, target) in [
        (8, 1),
        (10, 2),
        (12, 1),
        (14, 11),
        (16, 1),
        (18, 1),
        (20, 3),
        (22, 1),
    ] {
        put_ref(&mut stream, offset, target);
    }
    for (face_xmt, next_face) in [(11, 12), (12, 1)] {
        let mut face = record(14, crate::layout::face_node::LEN);
        put_ref(&mut face, 2, face_xmt);
        for offset in [8, 18, 20, 22, 26, 29, 31, 33, 35, 37] {
            put_ref(&mut face, offset, 1);
        }
        put_ref(&mut face, 18, next_face);
        put_ref(&mut face, 24, 10);
        face[28] = b'+';
        stream.extend(face);
    }
    let graph = crate::test_support::with_decode_context(|ctx| Graph::parse(ctx, &stream)).unwrap();
    assert_eq!(graph.keys.len(), 3);
    assert_eq!(graph.of_kind(NodeKind::Shell).len(), 1);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX body shell faces",
        |ctx| graph.body_topology_census(ctx),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("the first face visit must refuse");
    };
    assert_eq!(limit.additional, 1);
    // Shell classification has already counted both faces. The callback
    // visits only the first face and its three-key lookup before finding
    // that its loop chain is empty.
    let lookup_work = 3 * cadmpeg_core::decode::u64_from_index(
        std::mem::size_of::<NodeKind>() + std::mem::size_of::<u32>(),
    );
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = limit.used + 1 + lookup_work,
        |ctx| {
            assert_eq!(graph.body_topology_census(ctx).unwrap(), (false, 2));
            assert!(ctx.resource_refusal().is_none());
        },
    );
}
