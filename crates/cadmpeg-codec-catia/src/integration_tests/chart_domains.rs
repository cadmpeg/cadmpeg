// SPDX-License-Identifier: Apache-2.0

use super::decode;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{FaceLoops, Sense};

#[test]
fn torus_sheet_trims_use_both_full_chart_domains() {
    let bytes = include_bytes!("../../tests/fixtures/chart_domains/torus-trim-domains.catpart");
    let decoded = decode(bytes.to_vec());
    let ir = decoded.ir();
    let [face] = ir.model.faces.as_slice() else {
        panic!("one bounded torus face");
    };
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .expect("face carrier");
    assert!(matches!(
        surface.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
    ));
    assert!(matches!(face.loops, FaceLoops::Classified { .. }));
    assert_eq!(face.sense, Sense::Forward);
    for (u, v) in [(0.25_f64, -0.6_f64), (1.0, -0.6), (1.0, -0.2), (0.25, -0.2)] {
        let radius = 7.0 + 2.0 * v.cos();
        let expected = Point3::new(
            1.0 + radius * u.cos(),
            2.0 + radius * u.sin(),
            3.0 + 2.0 * v.sin(),
        );
        assert!(ir.model.vertices.iter().any(|vertex| {
            let point = ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .expect("vertex point");
            point.position().get().distance(expected) <= 1.0e-6
        }));
    }
    assert_eq!(ir.model.edges.len(), 4);
    assert_eq!(ir.model.coedges.len(), 4);
    assert_eq!(ir.model.vertices.len(), 4);
    assert_eq!(ir.model.pcurves.len(), 4);
}

#[test]
fn torus_sheet_inside_active_ranges_still_decodes() {
    let bytes = include_bytes!("../../tests/fixtures/chart_domains/torus-active-ranges.catpart");
    let decoded = decode(bytes.to_vec());
    assert_eq!(decoded.ir().model.faces.len(), 1);
}

#[test]
fn torus_sheet_trims_outside_full_domains_are_rejected() {
    let fixtures: [&[u8]; 2] = [
        include_bytes!("../../tests/fixtures/chart_domains/torus-outside-major-domain.catpart"),
        include_bytes!("../../tests/fixtures/chart_domains/torus-outside-minor-domain.catpart"),
    ];
    for bytes in fixtures {
        let decoded = decode(bytes.to_vec());
        assert!(decoded.ir().model.faces.is_empty());
    }
}
