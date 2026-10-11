// SPDX-License-Identifier: Apache-2.0
//! Source topology indices preserve traversal and placement identity.

use super::{geometry_for_kind, translation};
use crate::brep::{Tables, TextLocation, TextOrientation, TextShapeKind, TextShapeUse, TextTShape};
use crate::topology_transfer::{source_topology_indices, SourceOccurrenceKey};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::transform::Transform;

#[test]
fn source_indices_span_root_order_and_deduplicate_repeated_placements() {
    let translated = translation(10.0, 0.0, 0.0);
    let locations = [TextLocation {
        factors: Vec::new(),
        transform: translated,
    }];
    let tshapes = [TextTShape {
        geometry: geometry_for_kind(TextShapeKind::Edge),
        flags: [false; 7],
        children: Vec::new(),
    }];
    let roots = [
        TextShapeUse {
            shape: 1,
            orientation: TextOrientation::Forward,
            location: 0.into(),
        },
        TextShapeUse {
            shape: 1,
            orientation: TextOrientation::Reversed,
            location: 0.into(),
        },
        TextShapeUse {
            shape: 1,
            orientation: TextOrientation::Forward,
            location: 1.into(),
        },
    ];
    let tables = Tables {
        locations: &locations,
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &crate::brep::TextTShapes::from(tshapes.to_vec()),
        triangulations: &[],
        roots: &roots,
    };

    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    let indices = source_topology_indices(&ctx, tables).expect("valid locations");

    assert_eq!(
        indices.get(&(
            TextShapeKind::Edge,
            SourceOccurrenceKey::new(
                &cadmpeg_test_support::service_decode_context(),
                1,
                Transform::identity()
            )
            .unwrap(),
        )),
        Some(&1)
    );
    assert_eq!(
        indices.get(&(
            TextShapeKind::Edge,
            SourceOccurrenceKey::new(
                &cadmpeg_test_support::service_decode_context(),
                1,
                translated
            )
            .unwrap(),
        )),
        Some(&2)
    );
}

#[test]
fn source_topology_stack_refuses_at_caller_limit() {
    let tshapes = crate::brep::TextTShapes::from(vec![TextTShape {
        geometry: geometry_for_kind(TextShapeKind::Edge),
        flags: [false; 7],
        children: Vec::new(),
    }]);
    let roots = [TextShapeUse {
        shape: 1,
        orientation: TextOrientation::Forward,
        location: 0.into(),
    }];
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &tshapes,
        triangulations: &[],
        roots: &roots,
    };
    crate::test_support::assert_collection_refusal_at(
        &[],
        "FreeCAD source topology stack",
        |ctx| source_topology_indices(ctx, tables),
    );
}

#[test]
fn source_indices_follow_depth_first_topology_order() {
    let use_shape = |shape: usize| TextShapeUse {
        shape,
        orientation: TextOrientation::Forward,
        location: 0.into(),
    };
    let empty = |kind: TextShapeKind, children: Vec<usize>| TextTShape {
        geometry: geometry_for_kind(kind),
        flags: [false; 7],
        children: children.into_iter().map(use_shape).collect(),
    };
    let tshapes = vec![
        empty(TextShapeKind::Compound, vec![2, 3]),
        empty(TextShapeKind::Solid, vec![4]),
        empty(TextShapeKind::Solid, vec![5]),
        empty(TextShapeKind::Shell, vec![6]),
        empty(TextShapeKind::Shell, vec![7]),
        empty(TextShapeKind::Face, vec![8]),
        empty(TextShapeKind::Face, vec![9]),
        empty(TextShapeKind::Wire, vec![10]),
        empty(TextShapeKind::Wire, vec![11]),
        empty(TextShapeKind::Edge, vec![12, 13]),
        empty(TextShapeKind::Edge, vec![14, 15]),
        empty(TextShapeKind::Vertex, Vec::new()),
        empty(TextShapeKind::Vertex, Vec::new()),
        empty(TextShapeKind::Vertex, Vec::new()),
        empty(TextShapeKind::Vertex, Vec::new()),
    ];
    let roots = [use_shape(1)];
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &crate::brep::TextTShapes::from(tshapes),
        triangulations: &[],
        roots: &roots,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    let indices = source_topology_indices(&ctx, tables).expect("valid locations");
    let index = |kind, shape| {
        indices.get(&(
            kind,
            SourceOccurrenceKey::new(
                &cadmpeg_test_support::service_decode_context(),
                shape,
                Transform::identity(),
            )
            .unwrap(),
        ))
    };

    assert_eq!(index(TextShapeKind::Compound, 1), Some(&1));
    assert_eq!(index(TextShapeKind::Solid, 2), Some(&1));
    assert_eq!(index(TextShapeKind::Solid, 3), Some(&2));
    assert_eq!(index(TextShapeKind::Shell, 4), Some(&1));
    assert_eq!(index(TextShapeKind::Shell, 5), Some(&2));
    assert_eq!(index(TextShapeKind::Face, 6), Some(&1));
    assert_eq!(index(TextShapeKind::Face, 7), Some(&2));
    assert_eq!(index(TextShapeKind::Wire, 8), Some(&1));
    assert_eq!(index(TextShapeKind::Wire, 9), Some(&2));
    assert_eq!(index(TextShapeKind::Edge, 10), Some(&1));
    assert_eq!(index(TextShapeKind::Edge, 11), Some(&2));
    assert_eq!(index(TextShapeKind::Vertex, 12), Some(&1));
    assert_eq!(index(TextShapeKind::Vertex, 13), Some(&2));
    assert_eq!(index(TextShapeKind::Vertex, 14), Some(&3));
    assert_eq!(index(TextShapeKind::Vertex, 15), Some(&4));
}

#[test]
fn source_indices_stop_at_nested_same_kind_shapes() {
    let use_shape = |shape: usize| TextShapeUse {
        shape,
        orientation: TextOrientation::Forward,
        location: 0.into(),
    };
    let empty = |kind: TextShapeKind, children: Vec<usize>| TextTShape {
        geometry: geometry_for_kind(kind),
        flags: [false; 7],
        children: children.into_iter().map(use_shape).collect(),
    };
    let tshapes = vec![
        empty(TextShapeKind::Compound, vec![2, 2, 4]),
        empty(TextShapeKind::Compound, vec![3]),
        empty(TextShapeKind::Solid, Vec::new()),
        empty(TextShapeKind::Compound, Vec::new()),
    ];
    let roots = [use_shape(1)];
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &crate::brep::TextTShapes::from(tshapes),
        triangulations: &[],
        roots: &roots,
    };

    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    let indices = source_topology_indices(&ctx, tables).expect("valid locations");

    assert_eq!(
        indices.get(&(
            TextShapeKind::Compound,
            SourceOccurrenceKey::new(
                &cadmpeg_test_support::service_decode_context(),
                1,
                Transform::identity()
            )
            .unwrap(),
        )),
        Some(&1)
    );
    assert_eq!(
        indices.get(&(
            TextShapeKind::Compound,
            SourceOccurrenceKey::new(
                &cadmpeg_test_support::service_decode_context(),
                2,
                Transform::identity()
            )
            .unwrap(),
        )),
        None
    );
    assert_eq!(
        indices.get(&(
            TextShapeKind::Compound,
            SourceOccurrenceKey::new(
                &cadmpeg_test_support::service_decode_context(),
                4,
                Transform::identity()
            )
            .unwrap(),
        )),
        None
    );
    assert_eq!(
        indices.get(&(
            TextShapeKind::Solid,
            SourceOccurrenceKey::new(
                &cadmpeg_test_support::service_decode_context(),
                3,
                Transform::identity()
            )
            .unwrap(),
        )),
        Some(&1)
    );
}
