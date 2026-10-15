// SPDX-License-Identifier: Apache-2.0
use crate::test_support::test_e5::{append_e5_record, e5_torus_stream, e5_torus_topology_stream};
use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

#[test]
fn e5_rollback_surface_append_keeps_unused_torus_and_refuses_move_work() {
    let mut stream = e5_torus_topology_stream();
    let unused_torus = e5_torus_stream();
    // Record zero belongs to the parameter-bound lane in the topology stream.
    append_e5_record(&mut stream, 0xcc, 150, &unused_torus[13..]);
    let mut second_loop = vec![0x89];
    for index in 0..4u8 {
        second_loop.extend_from_slice(&[0xbc + index, 0xd0 + index]);
    }
    second_loop.push(0xb2);
    append_e5_record(&mut stream, 0x09, 97, &second_loop);
    append_e5_record(&mut stream, 0x00, 94, &[0x82, 0xb2, 0xe1, 1, 0]);
    append_e5_record(&mut stream, 0x08, 95, &[0x81, 0xde, 0x81, 1, 0, 1, 0, 1, 0]);
    append_e5_record(&mut stream, 0x01, 96, &[0x81, 0xdf]);
    let topology = crate::test_support::with_service_context(|ctx| {
        crate::families::e5::graph::parse_topology(ctx, &stream)
    })
    .expect("source reference graph admitted")
    .expect("distinct record identities resolve");
    assert_eq!(topology.faces.len(), 2);
    assert_eq!(topology.bodies.len(), 2);
    let file = crate::test_support::test_container::object_main_catpart(&stream);
    let decode = |ctx: &DecodeContext<'_>| {
        let scan = crate::container::scan_bytes(ctx, file.clone())?;
        let result =
            super::super::try_decode_e5(ctx, &scan, &mut crate::nurbs::LaneRefusals::new());
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    };
    let decoded = crate::test_support::with_service_context(decode)
        .expect("service profile admits rollback")
        .expect("E5 family decoded");
    assert_eq!(decoded.ir.model.surfaces.len(), 2);
    for radius in [10.0, 12.0] {
        assert!(decoded.ir.model.surfaces.iter().any(|surface| {
            matches!(&surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus))
                if torus.major_radius().get() == radius && torus.minor_radius().get() == 2.0)
        }));
    }
    assert!(decoded.ir.model.faces.is_empty());
    assert!(decoded.ir.model.loops.is_empty());
    assert!(decoded.ir.model.edges.is_empty());
    assert!(decoded.ir.model.pcurves.is_empty());
    assert_eq!(decoded.ir.model.vertices.len(), 4);
    assert_eq!(decoded.ir.model.points.len(), 4);
    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(
        decoded.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    let refusal = crate::test_support::with_work_refusal("catia_e5_rollback_surfaces", decode);
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_rollback_surfaces"));
}
