// SPDX-License-Identifier: Apache-2.0
use super::super::{
    edge_recipe_entries_with_context, edge_recipe_local_topology_references_with_context,
    edge_recipe_structure_with_context, face_recipe_nodes_with_context,
    face_recipe_structure_with_context, surface_patch_recipe_structure_with_context,
    FaceRecipeProgramKind,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EDGE_RECIPE: [i32; 24] = [
    0, 0, 0, 0, 0, 0, 0, 1, -1, 2, 0, -1, 1, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4,
];

#[test]
fn edge_recipe_structure_refuses_each_collection_growth() {
    for (limit, operation) in [
        (0, "f3d recipe scalars"),
        (1, "f3d recipe topology entry"),
        (2, "f3d recipe candidate scalars"),
        (3, "f3d recipe payload prefix"),
        (4, "f3d recipe side candidate"),
        (5, "f3d recipe empty side sequence"),
        (6, "f3d recipe copied scalars"),
        (7, "f3d recipe copied payload prefix"),
        (8, "f3d recipe copied entries"),
        (9, "f3d recipe following side"),
        (10, "f3d recipe side sequence"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = edge_recipe_structure_with_context(&ctx, &EDGE_RECIPE);
        assert!(
            matches!(result,
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "limit {limit}, operation {operation}"
        );
    }
}

#[test]
fn edge_recipe_structure_refuses_recursion_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        edge_recipe_structure_with_context(&ctx, &EDGE_RECIPE),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RecursionDepth
                && failure.operation == "f3d recipe side recursion"
    ));
}

#[test]
fn edge_recipe_structure_refuses_candidate_work_limit() {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d recipe payload candidates",
        0,
        |ctx| edge_recipe_structure_with_context(ctx, &EDGE_RECIPE),
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == "f3d recipe payload candidates"
    ));
}

#[test]
fn edge_recipe_payload_prefix_scans_refuse_work_limits() {
    let program = [
        0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0, 5, 0, 1, -1, 0, 0, -1, 1, 1, 4, 1, 1,
        1, 4, 4, 4,
    ];
    for operation in [
        "scan F3D recipe payload prefix delimiters",
        "validate F3D recipe payload prefix words",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| edge_recipe_structure_with_context(ctx, &program),
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == operation
        ));
    }
}

#[test]
fn edge_recipe_topology_references_refuse_collection_limit() {
    let structure = crate::test_support::with_decode_context(|ctx| {
        super::super::edge_recipe_structure_with_context(ctx, &EDGE_RECIPE)
            .expect("recipe structure")
    })
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        edge_recipe_local_topology_references_with_context(&ctx, &structure, 1),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d recipe topology reference"
    ));
}

#[test]
fn edge_recipe_topology_reference_scans_refuse_work_limit() {
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 1,
        sides: vec![crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
            header_value: 0,
            scalars: vec![1],
            payload_prefix: Vec::new(),
            entries: Vec::new(),
        }],
    };
    let result = crate::test_support::with_decode_context(|ctx| {
        edge_recipe_local_topology_references_with_context(ctx, &structure, 1)
    })
    .expect("valid topology reference");
    assert_eq!(
        result,
        Some(vec![std::num::NonZeroU32::new(1).expect("non-zero ordinal")])
    );

    for operation in [
        "scan F3D edge recipe topology sides",
        "scan F3D edge recipe side scalars",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| edge_recipe_local_topology_references_with_context(ctx, &structure, 1),
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == operation
                    && failure.additional == 1
        ));
    }
    let mut invalid_header = structure;
    invalid_header.sides[0].header_value = -1;
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let result = crate::test_support::with_decode_policy(&policy, |ctx| {
        edge_recipe_local_topology_references_with_context(ctx, &invalid_header, 1)
    });
    assert_eq!(result.unwrap(), None);
}

#[test]
fn edge_recipe_entries_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        edge_recipe_entries_with_context(&ctx, &[1, 4, 1, 1, 1, 4, 4, 4]),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d recipe topology entry"
    ));
}

#[test]
fn edge_recipe_entries_preserve_tail_and_work_refusal() {
    let words = [1, 4, 1, 1, 1, 4, 4, 4, -1];
    let operation = "scan F3D topology recipe entry words";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits, operation, 0,
        |ctx| edge_recipe_entries_with_context(ctx, &words),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == operation
            && limit.additional == 9));
    let parsed = edge_recipe_entries_with_context(
        &cadmpeg_test_support::service_decode_context(), &words,
    ).unwrap().expect("one complete topology entry");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].selector, 1);
}

#[test]
fn face_recipe_structure_refuses_collection_limit() {
    let program = [
        0, -1, 1, -1, 2, -1, 3, 0, -1, 0, -1, 0, -1, 0, 1, 1, 4, 1, -2, 1, 4, 4, 4, -1, 3, 0, -1,
        0, -1, 0, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4, -1,
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        face_recipe_structure_with_context(&ctx, &program),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d recipe scalars"
    ));
}

#[test]
fn surface_patch_recipe_entries_refuse_collection_limit() {
    let program = [
        0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0, -1, 1, 0, 2, 1,
        1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3, -1, 0, 0, -1, 0, -1,
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // Two clause slots, six field slots and nine field words precede the topology entry.
    policy.limits.max_collection_items = 2 + 6 + 9;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        surface_patch_recipe_structure_with_context(&ctx, &program, 4),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d recipe topology entry"
    ));
}

#[test]
fn surface_patch_recipe_scans_refuse_work_limits() {
    let program = [
        0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0,
        -1, 1, 0, 2, 1, 1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3,
        -1, 0, 0, -1, 0, -1,
    ];
    for operation in [
        "scan F3D SurfacePatch field delimiters",
        "validate F3D SurfacePatch field words",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| surface_patch_recipe_structure_with_context(ctx, &program, 4),
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == operation
        ));
    }
}

#[test]
fn face_recipe_nodes_refuse_work_limits() {
    let program = [0, -1, 4, -1, -1, 2, 7];
    let kind = FaceRecipeProgramKind::Counted { header_value: 4 };
    for operation in [
        "scan F3D face recipe marker windows",
        "scan F3D face recipe node range starts",
        "scan F3D face recipe node range ends",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| face_recipe_nodes_with_context(ctx, &program, 0, kind),
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == operation
        ));
    }
}

#[test]
fn face_recipe_nodes_refuse_each_collection_growth() {
    let program = [0, -1, 4, -1, -1, 2, 7];
    for (limit, operation) in [
        (0, "f3d face recipe node index"),
        (1, "f3d face recipe node program"),
        (5, "f3d face recipe node"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(
                face_recipe_nodes_with_context(
                    &ctx, &program, 0, FaceRecipeProgramKind::Counted { header_value: 4 },
                ),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "limit {limit}, operation {operation}"
        );
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let nodes = face_recipe_nodes_with_context(
        &ctx,
        &program,
        0,
        FaceRecipeProgramKind::Counted { header_value: 4 },
    )
    .unwrap()
    .unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].program, [-1, -1, 2, 7]);
    assert_eq!(nodes[0].byte_offset, 12);
}
