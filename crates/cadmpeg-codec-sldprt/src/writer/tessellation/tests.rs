// SPDX-License-Identifier: Apache-2.0
//! Borrowed display-list emission and exact channel bytes.

use std::cell::Cell;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};

use crate::tessellation::mesh_rows::MeshRows;

use super::{append_triples, tessellation_payload};

fn strip_document(shaded: bool) -> CadIr {
    let positions = (0..7).map(|index| {
        let value = f64::from(index);
        Point3::new(value, 2.0 * value, -value)
    }).collect();
    let normals = shaded.then(|| (0..7).map(|_| Vector3::new(0.0, 0.0, 1.0)).collect());
    let mesh = TessellationMesh::from_strip_lanes(positions, normals, &[3, 4]).unwrap();
    let mut document = CadIr::empty();
    document.model.tessellations.push(Tessellation::new(
        TessellationId::mint("synthetic:test:tessellation#borrowed-emission").unwrap(),
        mesh,
        Vec::new(),
    ).unwrap());
    document
}

fn expected_channel(out: &mut Vec<u8>, header: [u32; 4], data: &[u8]) {
    for value in header {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(data);
}

#[test]
fn borrowed_strip_emission_preserves_complete_channel_bytes() {
    for shaded in [false, true] {
        let document = strip_document(shaded);
        let ctx = cadmpeg_test_support::service_decode_context();
        let actual = tessellation_payload(&ctx, &document, 0.5).unwrap();
        let mut expected = b"uoTempBodyTessData_c".to_vec();
        expected.extend_from_slice(&[0; 8]);
        expected.extend_from_slice(b"uoTempFaceTessData_c");
        // Runs of three and four vertices span one and two triangles.
        expected.extend_from_slice(&3_u32.to_le_bytes());
        expected.extend_from_slice(&2_u32.to_le_bytes());
        expected_channel(&mut expected, [4, 8, 2, 2], &[3, 0, 0, 0, 4, 0, 0, 0]);
        let mut positions = Vec::new();
        for index in 0_u16..7 {
            let value = f32::from(index);
            for scalar in [0.5 * value, value, -0.5 * value] {
                positions.extend_from_slice(&scalar.to_le_bytes());
            }
        }
        expected_channel(&mut expected, [12, 100, 2, 7], &positions);
        let mut normals = Vec::new();
        if shaded {
            for _ in 0..7 {
                for scalar in [0.0_f32, 0.0, 1.0] {
                    normals.extend_from_slice(&scalar.to_le_bytes());
                }
            }
        }
        expected_channel(&mut expected, [12, 100, 2, if shaded { 7 } else { 0 }], &normals);
        expected_channel(&mut expected, [4, 8, 2, 0], &[]);
        // Each run states twice its vertex count less two endpoints.
        expected_channel(&mut expected, [4, 8, 2, 2], &[4, 0, 0, 0, 6, 0, 0, 0]);
        expected_channel(&mut expected, [1, 8, 2, 0], &[]);
        assert_eq!(actual, expected);
    }
}

#[test]
fn borrowed_triple_emission_needs_only_the_existing_output_backing() {
    for shaded in [false, true] {
        let document = strip_document(shaded);
        let mesh = document.model.tessellations[0].mesh();
        // Seven triples of three four-byte components; allocate outside observation.
        let mut positions = Vec::with_capacity(7 * 3 * 4);
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            append_triples(&mut positions, MeshRows::new(mesh).map(|(point, _)| {
                [point.x, point.y, point.z]
            }), "position")
        });
        result.unwrap();
        assert_eq!(allocations, 0);
        assert_eq!(positions.len(), 7 * 3 * 4);
        let mut normals = Vec::with_capacity(if shaded { 7 * 3 * 4 } else { 0 });
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            append_triples(&mut normals, MeshRows::new(mesh).filter_map(|(_, normal)| {
                normal.map(|normal| [normal.x, normal.y, normal.z])
            }), "normal")
        });
        result.unwrap();
        assert_eq!(allocations, 0);
        assert_eq!(normals.len(), if shaded { 7 * 3 * 4 } else { 0 });
    }
}

#[test]
fn triple_overflow_refuses_before_visiting_the_next_row() {
    for role in ["position", "normal"] {
        let visited = Cell::new(0);
        let triples = [[1.0, f64::MAX, 3.0], [4.0, 5.0, 6.0]].into_iter()
            .inspect(|_| visited.set(visited.get() + 1));
        let mut out = Vec::with_capacity(24);
        let error = append_triples(&mut out, triples, role).unwrap_err();
        assert_eq!(visited.get(), 1);
        assert_eq!(out, 1.0_f32.to_le_bytes());
        assert_eq!(error.to_string(), format!("malformed container: SLDPRT tessellation {role} exceeds f32 range"));
    }
}

#[test]
fn empty_display_emission_keeps_the_zero_mesh_header() {
    let document = CadIr::empty();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut expected = b"uoTempBodyTessData_c".to_vec();
    expected.extend_from_slice(&[0; 8]);
    expected.extend_from_slice(b"uoTempFaceTessData_c");
    expected.extend_from_slice(&[0; 8]);
    assert_eq!(tessellation_payload(&ctx, &document, 0.5).unwrap(), expected);
}

fn indexed_document(shading: u8) -> CadIr {
    use cadmpeg_ir::tessellation::{ChannelAddressing, TessellationChannel};
    let positions = vec![
        Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0), Point3::new(0.0, 1.0, 0.0),
    ];
    let triangles = vec![[2, 0, 1], [2, 1, 3]];
    let geometry = match shading {
        0 => TessellationMesh::from_list_lanes(positions, triangles, None),
        1 => TessellationMesh::from_list_lanes(positions, triangles, Some(vec![
            Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0), Vector3::new(-1.0, 0.0, 0.0),
        ])),
        _ => TessellationMesh::from_corner_lanes(positions, triangles, Some(vec![
            Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0), Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0), Vector3::new(0.0, 0.0, -1.0),
        ])),
    }.unwrap();
    let mut mesh = Tessellation::new(
        TessellationId::mint("synthetic:test:tessellation#indexed-borrow").unwrap(), geometry,
        vec![TessellationChannel::new(ChannelAddressing::Vertex {}, 1, 7, 2, vec![10, 11, 12, 13]).unwrap()],
    ).unwrap();
    mesh.body = Some(cadmpeg_ir::ids::BodyId::mint("synthetic:test:body#indexed-borrow").unwrap());
    mesh.faces.push(cadmpeg_ir::ids::FaceId::mint("synthetic:test:face#indexed-borrow").unwrap());
    let mut document = CadIr::empty();
    document.model.tessellations.push(mesh);
    document
}

fn assert_indexed_expansion(shading: u8) {
        let document = indexed_document(shading);
        let mesh = &document.model.tessellations[0];
        let (facts, allocations) = crate::test_support::allocation::count_allocations(|| {
            let source = super::IndexedMesh::new(mesh.mesh()).unwrap();
            (source.vertex_count(), source.triangle_count(), source.triangle(0), source.triangle(1),
                source.position(0), source.position(2))
        });
        assert_eq!(allocations, 0);
        assert_eq!(facts.0, 4);
        assert_eq!(facts.1, 2);
        assert_eq!(facts.2, [2, 0, 1]);
        assert_eq!(facts.3, [2, 1, 3]);
        assert_eq!(facts.4.get(), Point3::new(0.0, 0.0, 0.0));
        assert_eq!(facts.5.get(), Point3::new(1.0, 1.0, 0.0));
        let expanded = super::sequential_tessellation(mesh).unwrap();
        assert!(matches!(expanded, std::borrow::Cow::Owned(_)));
        assert_eq!(expanded.id, mesh.id);
        assert_eq!(expanded.body, mesh.body);
        assert_eq!(expanded.faces, mesh.faces);
        assert_eq!(expanded.vertices().iter().map(|p| p.get()).collect::<Vec<_>>(), vec![
            Point3::new(1.0, 1.0, 0.0), Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0),
        ]);
        assert_eq!(expanded.triangles(), [[0, 1, 2], [3, 4, 5]]);
        assert_eq!(expanded.strip_lengths(), [3, 3]);
        assert_eq!(expanded.channels()[0].data(), [12, 10, 11, 12, 11, 13]);
        let expected_normals = match shading {
            0 => Vec::new(),
            1 => vec![Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 1.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)],
            _ => vec![Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0), Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, -1.0, 0.0), Vector3::new(0.0, 0.0, -1.0)],
        };
        assert_eq!(expanded.vertex_normals().iter().map(|n| n.get()).collect::<Vec<_>>(), expected_normals);
        assert!(expanded.per_corner_normals().is_empty());
}

#[test]
fn indexed_positions_expand_from_borrowed_lanes() {
    assert_indexed_expansion(0);
}

#[test]
fn indexed_vertex_normals_expand_from_borrowed_lanes() {
    assert_indexed_expansion(1);
}

#[test]
fn indexed_corner_normals_expand_from_borrowed_lanes() {
    assert_indexed_expansion(2);
}

#[test]
fn empty_indexed_forms_emit_no_rows_or_normals() {
    for shading in 0..3 {
        let geometry = match shading {
            0 => TessellationMesh::from_list_lanes(Vec::new(), Vec::new(), None),
            1 => TessellationMesh::from_list_lanes(Vec::new(), Vec::new(), Some(Vec::new())),
            _ => TessellationMesh::from_corner_lanes(Vec::new(), Vec::new(), Some(Vec::new())),
        }.unwrap();
        let mesh = Tessellation::new(
            TessellationId::mint("synthetic:test:tessellation#empty-indexed").unwrap(), geometry, Vec::new(),
        ).unwrap();
        let expanded = super::sequential_tessellation(&mesh).unwrap();
        assert_eq!(expanded.vertex_count(), 0);
        assert_eq!(expanded.triangle_count(), 0);
        assert!(expanded.vertex_normals().is_empty());
        assert!(expanded.per_corner_normals().is_empty());
        assert!(expanded.strip_lengths().is_empty());
        assert!(matches!(expanded.mesh(), TessellationMesh::List { .. }));
    }
}
