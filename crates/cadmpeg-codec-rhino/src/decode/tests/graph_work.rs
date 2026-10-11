// SPDX-License-Identifier: Apache-2.0

use super::*;

fn face_graph(count: usize, pairs: &[(usize, usize)]) -> crate::brep::ResolvedBrep {
    use crate::brep::{
        BrepTolerance, ResolvedBrep, ResolvedEdge, ResolvedFace, ResolvedLoop, ResolvedTrim,
    };
    let tolerance = BrepTolerance::new(0.0).unwrap();
    let mut resolved = ResolvedBrep {
        faces: (0..count)
            .map(|face| ResolvedFace {
                surface: 0,
                loops: vec![face],
            })
            .collect(),
        loops: (0..count)
            .map(|face| ResolvedLoop {
                face,
                trims: Vec::new(),
            })
            .collect(),
        ..ResolvedBrep::default()
    };
    for &(left, right) in pairs {
        let edge = resolved.edges.len();
        let first = resolved.trims.len();
        for face in [left, right] {
            let trim = resolved.trims.len();
            resolved.trims.push(ResolvedTrim {
                curve: None,
                edge: Some(edge),
                vertices: [0, 0],
                loop_index: face,
                tolerances: [tolerance; 2],
            });
            resolved.loops[face].trims.push(trim);
        }
        resolved.edges.push(ResolvedEdge {
            curve: 0,
            vertices: [0, 0],
            trims: vec![first, first + 1],
            tolerance,
        });
    }
    resolved
}

#[test]
fn face_components_share_visited_state_across_cycles_and_roots() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    for count in [64, 1024] {
        let split = count / 2;
        let mut pairs = (0..split - 1)
            .rev()
            .map(|face| (face, face + 1))
            .collect::<Vec<_>>();
        pairs.push((split - 1, 0));
        pairs.extend((split..count - 2).rev().map(|face| (face, face + 1)));
        pairs.push((count - 2, split));
        let resolved = face_graph(count, &pairs);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 32 * cadmpeg_core::decode::u64_from_index(count);
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let labels = super::super::face_components(&ctx, &resolved).unwrap();
        assert!(labels[..split].iter().all(|&label| label == 0));
        assert!(labels[split..count - 1].iter().all(|&label| label == 1));
        assert_eq!(labels[count - 1], 2);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn instance_definition_uniqueness_is_memoized_across_root_attempts() {
    use crate::test_support::test_dump as bytes;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    for case in 0..3 {
        let archive = ArchiveVersion::V5;
        let definition = [0x10; 16];
        let member = [3; 16];
        let members = if case == 1 {
            vec![member, member]
        } else {
            vec![member]
        };
        let objects = [
            bytes::object_record_with_payload(
                archive,
                0x1000,
                bytes::INSTANCE_REFERENCE_CLASS,
                &bytes::instance_reference_payload(definition, bytes::transform(1.0, [0.0; 3])),
            ),
            bytes::object_record_with_payload(
                archive,
                0x1000,
                bytes::INSTANCE_REFERENCE_CLASS,
                &bytes::instance_reference_payload(
                    definition,
                    bytes::transform(1.0, [10.0, 0.0, 0.0]),
                ),
            ),
            if case == 2 {
                bytes::object_record_with_payload(
                    archive,
                    0x1000,
                    bytes::INSTANCE_REFERENCE_CLASS,
                    &bytes::instance_reference_payload(definition, bytes::transform(1.0, [0.0; 3])),
                )
            } else {
                bytes::object_record_with_payload(
                    archive,
                    1,
                    POINT_CLASS,
                    &bytes::point_payload([1.0, 2.0, 3.0]),
                )
            },
        ];
        let data = bytes::document_with_definitions(
            "50",
            archive,
            &[bytes::definition_record(
                archive,
                &bytes::v5_definition_payload(archive, 7, definition, &members, false),
            )],
            &objects,
        );
        let mut scan = crate::container::scan_owned(data).unwrap();
        bytes::set_test_units(&mut scan, 1.0);
        for (order, id, key) in [
            (0, [1; 16], "first"),
            (1, [2; 16], "second"),
            (2, member, "member"),
        ] {
            bytes::set_identity(&mut scan, order, id, key, None, true);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .unwrap();
        let mut transaction =
            DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).unwrap();
        assert_eq!(transaction.expand_reference(0).unwrap(), case == 0);
        {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "Rhino instance members are unique traversal",
                None,
            );
            assert_eq!(transaction.expand_reference(1).unwrap(), case == 0);
            assert!(ctx.resource_refusal().is_none());
        }
        if case == 0 {
            assert_eq!(transaction.session.document().model.points.len(), 2);
            assert_eq!(
                transaction.session.document().model.points[0]
                    .position()
                    .get()
                    .x,
                1.0
            );
            assert_eq!(
                transaction.session.document().model.points[1]
                    .position()
                    .get()
                    .x,
                11.0
            );
        } else {
            let message = if case == 1 {
                "contains duplicate member UUIDs"
            } else {
                "definition cycle reaches"
            };
            assert_eq!(
                transaction
                    .report
                    .phase_warnings
                    .iter()
                    .filter(|warning| warning.message.contains(message))
                    .count(),
                2
            );
        }
        drop(transaction);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn generated_link_rollback_does_not_revisit_the_existing_prefix() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    for count in [1, 128] {
        let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .unwrap();
        let mut transaction =
            DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).unwrap();
        for value in (0..count).rev() {
            transaction
                .append_link(0, &format!("rhino:test:curve#prefix-{value}"))
                .unwrap();
        }
        let prefix = transaction.unknown(0).unwrap().links().to_vec();
        transaction.instance_journal = Some(super::super::InstanceJournal::new(&ctx).unwrap());
        transaction
            .append_links(
                0,
                &[
                    "rhino:test:curve#new-a".to_owned(),
                    "rhino:test:curve#new-b".to_owned(),
                    "rhino:test:curve#new-a".to_owned(),
                ],
            )
            .unwrap();
        {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "Rhino instance rollback links",
                None,
            );
            transaction.rollback_instance_rows().unwrap();
            assert!(ctx.resource_refusal().is_none());
        }
        assert_eq!(transaction.unknown(0).unwrap().links(), prefix);
        assert!(transaction
            .append_link(0, "rhino:test:curve#new-a")
            .unwrap());
        assert_eq!(transaction.unknown(0).unwrap().links().len(), count + 1);
        drop(transaction);
        ctx.finish_session().unwrap();
    }
}
