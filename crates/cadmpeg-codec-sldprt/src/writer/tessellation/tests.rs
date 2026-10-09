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
