// SPDX-License-Identifier: Apache-2.0
//! Topology requires an admitted carrier; geometric endpoint points are independent.
use super::*;

fn member(shared: bool) -> Vec<u8> {
    let total = if shared { 5 } else { 4 };
    let vertices = "Ve 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\n";
    let edge = |curve| {
        format!(
            "Ed 0.001 1 1 0 1 {curve} 0 0 1 0 1001000 +{total} 0 -{} 0 *\n",
            total - 1
        )
    };
    let edges = if shared {
        format!("{}{}", edge(1), edge(2))
    } else {
        edge(2)
    };
    let uses = if shared { "+3 0 +2 0" } else { "+2 0" };
    format!("CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 2\n1 0 0 0 1 0 0\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes {total}\n{vertices}{edges}Wi 1001000 {uses} *\n+1 0 *\n").into_bytes()
}

#[test]
fn lost_sole_carrier_keeps_points_and_shared_healthy_carrier_keeps_vertices() {
    for shared in [false, true] {
        let bytes = fixtures::archive(false, &member(shared));
        let before = decode(&bytes);
        let boundaries = assert_parts_agree(&bytes, &before);
        assert_eq!(before.ir().model.edges.len(), if shared { 2 } else { 1 });
        assert_eq!(before.ir().model.vertices.len(), 2);
        assert_eq!(before.ir().model.points.len(), 2);
        let target = boundaries
            .iter()
            .filter(|part| part.level == "freecad.brep_text_curve")
            .nth(1)
            .unwrap();
        let damaged = reseal_damage(&bytes, target, 255);
        let after = decode(&damaged);
        let damaged_boundaries = assert_parts_agree(&damaged, &after);
        let streams = recovery::stream_pairs(&boundaries, &damaged_boundaries);
        assert_eq!(after.ir().model.edges.len(), usize::from(shared));
        assert_eq!(after.ir().model.vertices.len(), if shared { 2 } else { 0 });
        assert_eq!(after.ir().model.points, before.ir().model.points);
        let intersects = |id: &str| {
            before.source_fidelity().record_layout()[id]
                .reads()
                .get(&target.stream)
                .into_iter()
                .flatten()
                .any(|read| read.start < target.offset + target.length && target.offset < read.end)
        };
        let dependent = before.ir().model.edges.last().unwrap();
        assert!(intersects(dependent.id.as_str()));
        if !shared {
            assert!(before
                .ir()
                .model
                .vertices
                .iter()
                .all(|vertex| intersects(vertex.id.as_str())));
            assert!(before
                .ir()
                .model
                .bodies
                .iter()
                .map(|row| row.id.as_str())
                .chain(before.ir().model.regions.iter().map(|row| row.id.as_str()))
                .chain(before.ir().model.shells.iter().map(|row| row.id.as_str()))
                .all(intersects));
        }
        assert!(before
            .ir()
            .model
            .points
            .iter()
            .all(|point| !intersects(point.id.as_str())));
        crate::test_support::test_archive::assert_valid_document(after.ir());
        compare_damage(&bytes, &before, &after, &damaged, &streams, Some(target));
    }
}

#[test]
fn lost_edge_carrier_drops_its_pcurve_and_keeps_independent_uv_and_surface_values() {
    let member = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 1\n1 0 0 1 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 6\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +6 0 -5 0 *\nWi 1001000 +4 0 *\nFa 0 0.001 1 0 1001000 +3 0 *\nSh 1001000 +2 0 *\n+1 0 *\n";
    let bytes = fixtures::archive(false, member);
    let before = decode(&bytes);
    assert_eq!(before.ir().model.pcurves.len(), 1);
    let boundaries = assert_parts_agree(&bytes, &before);
    let target = boundaries
        .iter()
        .find(|part| part.level == "freecad.brep_text_curve")
        .unwrap();
    let damaged = reseal_damage(&bytes, target, 255);
    let after = decode(&damaged);
    assert!(after.ir().model.pcurves.is_empty());
    assert_eq!(after.ir().model.surfaces, before.ir().model.surfaces);
    assert_eq!(after.ir().model.points, before.ir().model.points);
    let native = |decoded: &DecodeResult| {
        crate::brep::records::payloads_untracked(decoded.ir().native.namespace("fcstd").unwrap())
            .unwrap()
    };
    let old = native(&before);
    let new = native(&after);
    let a = crate::brep::Tables::from_payload(&old[0]).unwrap();
    let b = crate::brep::Tables::from_payload(&new[0]).unwrap();
    assert_eq!(a.curve2ds, b.curve2ds);
    assert_eq!(a.surfaces, b.surfaces);
    let layout =
        &before.source_fidelity().record_layout()[before.ir().model.pcurves[0].id.as_str()];
    assert!(layout.reads()[&target.stream]
        .iter()
        .any(|read| read.start < target.offset + target.length && target.offset < read.end));
    let damaged_boundaries = assert_parts_agree(&damaged, &after);
    let streams = recovery::stream_pairs(&boundaries, &damaged_boundaries);
    crate::test_support::test_archive::assert_valid_document(after.ir());
    compare_damage(&bytes, &before, &after, &damaged, &streams, Some(target));
}
