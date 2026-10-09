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
    for (cap, operation) in [
        (0, "validate SubD edge rows"),
        (1, "validate SubD edge vertices"),
        (3, "validate SubD edge rows"),
        (9, "validate SubD face rows"),
        (10, "validate SubD face edge references"),
        (12, "validate SubD directed ring"),
        (16, "validate SubD vertex rows"),
        (17, "validate SubD grip wedges"),
        (18, "validate SubD grip edge"),
        (19, "validate SubD grip edge owner"),
        (20, "validate SubD grip face"),
        (21, "validate SubD grip face edges"),
        (22, "validate SubD grip face owner"),
        (23, "validate SubD grip slots"),
        (24, "validate SubD grip slots"),
        // The second grip admits membership and insertion in a one-key tree.
        (25, "SubD validation member search"),
        (26, "SubD validation member search"),
        (27, "validate SubD vertex rows"),
        (28, "validate SubD vertex rows"),
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
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
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
    for (dimension, cap, edited) in [
        (ResourceDimension::MaterializedBytes, 0, false),
        (ResourceDimension::CollectionItems, 0, false),
        (ResourceDimension::WorkUnits, 0, false),
        (ResourceDimension::WorkUnits, 9, true),
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
