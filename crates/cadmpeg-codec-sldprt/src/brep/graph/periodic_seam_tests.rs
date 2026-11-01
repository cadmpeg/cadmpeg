// SPDX-License-Identifier: Apache-2.0
#[test]
fn degree_one_nurbs_cache_pcurve_continues_across_a_periodic_seam() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    // A closed ring in v whose seam at v = 0 lies inside the flat side x = 1.
    let ring = [(1.0, 0.0), (1.0, 1.0), (-1.0, 0.0), (1.0, -1.0), (1.0, 0.0)];
    let rows = [0.0, 1.0].map(|z| {
        ring.map(|(x, y)| cadmpeg_ir::math::Point3::new(x, y, z))
            .to_vec()
    });
    let surface = cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            1,
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
            true,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(rows.to_vec(), None),
        false,
    )
    .expect("closed ring surface");
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.5),
            cadmpeg_ir::math::Point3::new(1.0, -0.5, 0.5),
        ],
        None,
        false,
    )
    .expect("seam curve");

    let (lanes, fit_tolerance) =
        super::nurbs_degree_one_cache_lanes(&ctx, &surface, &curve, [0.0, 1.0])
            .unwrap_or_else(|_| panic!("the seam curve projects"))
            .expect("the seam curve has lanes");

    assert!((lanes[0].v - 0.5).abs() < 1.0e-9);
    assert!((lanes[1].v + 0.5).abs() < 1.0e-9);
    assert!((lanes[0].u - 0.5).abs() < 1.0e-9);
    assert!((lanes[1].u - 0.5).abs() < 1.0e-9);
    assert!(fit_tolerance < 1.0e-9);
}
