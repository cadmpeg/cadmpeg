// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::scalar::PositiveReal;
use crate::subd::{
    SubdCage, SubdError, SubdGripDirection, SubdGripWedge, SubdPlaneFrame, SubdRadialMapSelector,
    SubdRadialSymmetryMap, SubdSecondaryGrip, SubdSymmetry, SubdSymmetryKind, SubdVertexGripLayout,
};

fn gripped_cage() -> SubdCage {
    let mut cage = super::triangle_cage();
    let grip = |source_index| {
        Some(SubdSecondaryGrip {
            source_index,
            point: crate::features::FinitePoint3::ZERO,
            weight: PositiveReal::new(1.0).unwrap(),
        })
    };
    cage.vertices[0].secondary_grips = Some(SubdVertexGripLayout {
        direction: SubdGripDirection::North,
        wedges: vec![SubdGripWedge::Slot {
            edge: Some(0),
            sector_face: Some(0),
            spokes: vec![grip(42)],
            sectors: vec![grip(43)],
        }],
    });
    cage
}

fn plane() -> SubdPlaneFrame {
    SubdPlaneFrame::new(
        crate::math::Point3::new(0.0, 0.0, 0.0),
        crate::math::Vector3::new(1.0, 0.0, 0.0),
        crate::math::Vector3::new(0.0, 1.0, 0.0),
    )
    .unwrap()
}

#[test]
fn cage_validation_admits_each_topology_and_grip_walk_before_visiting() {
    // Core's BTreeSet<u32> node bound is 11 u32 key lanes + 16 pointer widths + two max-alignment pads.
    // On 64-bit targets: 752 first-node + 1,504 second-insert + 88 key-comparison + 27 prior work = 2,371 before two final vertex visits, for 2,373 total.
    const NODE_ALIGNMENT: usize = if std::mem::align_of::<u32>() > std::mem::align_of::<usize>() {
        std::mem::align_of::<u32>()
    } else {
        std::mem::align_of::<usize>()
    };
    const NODE_BYTES: u64 = cadmpeg_core::decode::u64_from_index(
        11 * std::mem::size_of::<u32>() + 16 * std::mem::size_of::<usize>() + 2 * NODE_ALIGNMENT,
    );
    let first_node_work = 4 * NODE_BYTES;
    let second_insert_work = 8 * NODE_BYTES;
    let after_first_node = 24 + first_node_work;
    let before_final_vertices = 27 + first_node_work + second_insert_work + 2 * 44;
    let full_work = before_final_vertices + 2;

    for (cap, operation, used, additional) in [
        (0, "validate SubD edge rows", 0, 1),
        (1, "validate SubD edge vertices", 1, 1),
        (3, "validate SubD edge rows", 3, 1),
        (9, "validate SubD face rows", 9, 1),
        (10, "validate SubD face edge references", 10, 1),
        (12, "validate SubD directed ring", 12, 1),
        (16, "validate SubD vertex rows", 16, 1),
        (17, "validate SubD grip wedges", 17, 1),
        (18, "validate SubD grip edge", 18, 1),
        (19, "validate SubD grip edge owner", 19, 1),
        (20, "validate SubD grip face", 20, 1),
        (21, "validate SubD grip face edges", 21, 1),
        (22, "validate SubD grip face owner", 22, 1),
        (23, "validate SubD grip slots", 23, 1),
        (24, "SubD validation members", 24, first_node_work),
        (25, "SubD validation members", 24, first_node_work),
        (26, "SubD validation members", 24, first_node_work),
        (27, "SubD validation members", 24, first_node_work),
        (
            after_first_node,
            "validate SubD grip slots",
            after_first_node,
            1,
        ),
        (
            after_first_node + 1,
            "SubD validation member search",
            after_first_node + 1,
            1,
        ),
        (
            after_first_node + 2,
            "SubD validation members",
            after_first_node + 2,
            1,
        ),
        (
            before_final_vertices - 1,
            "SubD validation members",
            before_final_vertices - 44,
            44,
        ),
        (
            before_final_vertices,
            "validate SubD vertex rows",
            before_final_vertices,
            1,
        ),
        (
            before_final_vertices + 1,
            "validate SubD vertex rows",
            before_final_vertices + 1,
            1,
        ),
    ] {
        let cage = gripped_cage();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) =
            SubdCage::new(cage.vertices, cage.edges, cage.faces, cage.symmetries, &ctx)
        else {
            panic!("cage walk must refuse at {cap}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used, used);
        assert_eq!(limit.additional, additional);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
    // This admits both grip slots, searches, key comparisons, B-tree mutations and remaining vertex rows.
    let cage = gripped_cage();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = full_work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    SubdCage::new(cage.vertices, cage.edges, cage.faces, cage.symmetries, &ctx)
        .unwrap()
        .unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn cage_validation_releases_scoped_members_and_preserves_missing_edge_precedence() {
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let cage = gripped_cage();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => panic!("collection dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) =
            SubdCage::new(cage.vertices, cage.edges, cage.faces, cage.symmetries, &ctx)
        else {
            panic!("member storage must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "SubD validation members");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
    let cage = gripped_cage();
    let expected = cage.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // Membership indexes admit backing B-tree nodes within scoped storage.
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        SubdCage::new(cage.vertices, cage.edges, cage.faces, cage.symmetries, &ctx)
            .unwrap()
            .unwrap(),
        expected
    );
    drop(
        ctx.reserve_scoped(4096, "reuse cage validation storage")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
    let mut cage = super::triangle_cage();
    cage.faces[0].edges[1].edge = 2;
    cage.faces[0].edges[2].edge = u32::MAX;
    let wire = serde_json::to_value(&cage).unwrap();
    let error = SubdCage::new(
        cage.vertices,
        cage.edges,
        cage.faces,
        cage.symmetries,
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "faces[0].edges references a missing edge"
    );
    assert!(serde_json::from_value::<SubdCage>(wire)
        .unwrap_err()
        .to_string()
        .contains("faces[0].edges references a missing edge"));
    let mut cage = super::triangle_cage();
    cage.edges[0].vertices[0] = u32::MAX;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        SubdCage::new(cage.vertices, cage.edges, cage.faces, cage.symmetries, &ctx)
    else {
        panic!("cage diagnostic must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "SubD admission error");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn subdivision_symmetry_and_layout_constructors_use_the_caller_session() {
    for radial in [false, true] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("member dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if radial {
                SubdSymmetryKind::radial_from_parts(
                    std::num::NonZeroU32::new(2).unwrap(),
                    crate::scalar::FiniteReal::ZERO,
                    vec![SubdRadialSymmetryMap {
                        selector: SubdRadialMapSelector::Ef,
                        pairs: vec![[0, 1]],
                    }],
                    &ctx,
                )
                .map(|result| result.map(|_| ()))
            } else {
                SubdSymmetry::new(
                    SubdSymmetryKind::Correspondence {},
                    plane(),
                    vec![[0, 1]],
                    Vec::new(),
                    Vec::new(),
                    &ctx,
                )
                .map(|result| result.map(|_| ()))
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("symmetry admission must refuse");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(
                limit.operation,
                if dimension == ResourceDimension::WorkUnits {
                    if radial {
                        "validate SubD radial maps"
                    } else {
                        "validate SubD correspondence pairs"
                    }
                } else {
                    "SubD validation members"
                }
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }
    for cap in [0, 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = SubdVertexGripLayout::new(
            SubdGripDirection::North,
            vec![SubdGripWedge::Phantom {}, SubdGripWedge::Phantom {}],
            &ctx,
        ) else {
            panic!("wedge walk must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "validate SubD wedge arity");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn subdivision_vertex_edit_refusals_are_atomic_and_release_candidates() {
    // The edit callback follows 4 vertex iterator steps, 2 each for wedge, spoke and sector iterators, and 3 edit slots: 13 work units.
    for (dimension, cap, edited) in [
        (ResourceDimension::MaterializedBytes, 0, false),
        (ResourceDimension::CollectionItems, 0, false),
        (ResourceDimension::WorkUnits, 0, false),
        (ResourceDimension::WorkUnits, 13, true),
        (ResourceDimension::RetainedBytes, 0, true),
    ] {
        let mut cage = gripped_cage();
        let expected = cage.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            _ => panic!("edit dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut invoked = false;
        let Err(CodecError::ResourceLimit(limit)) = cage.edit_vertices(
            |vertices| {
                invoked = true;
                vertices[0].tag = crate::subd::SubdVertexTag::Corner;
                Ok(())
            },
            &ctx,
        ) else {
            panic!("edit admission must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(invoked, edited);
        assert_eq!(cage, expected);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
    let mut cage = gripped_cage();
    let expected = cage.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = cage
        .edit_vertices(
            |vertices| {
                let SubdGripWedge::Slot { spokes, .. } =
                    &mut vertices[0].secondary_grips.as_mut().unwrap().wedges[0]
                else {
                    panic!("grip slot");
                };
                spokes[0].as_mut().unwrap().source_index = 99;
                Err(SubdError::EditRefused("reject edited grips".into()))
            },
            &ctx,
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(error.to_string(), "reject edited grips");
    assert_eq!(cage, expected);
    drop(
        ctx.reserve_scoped(4096, "reuse vertex candidate storage")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
    cage.edit_vertices(
        |vertices| {
            vertices[0].tag = crate::subd::SubdVertexTag::Corner;
            Ok(())
        },
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(cage.vertices[0].tag, crate::subd::SubdVertexTag::Corner);
    assert_eq!(
        cage.vertices[0].secondary_grips,
        expected.vertices[0].secondary_grips
    );
}
