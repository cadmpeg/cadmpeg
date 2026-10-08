// SPDX-License-Identifier: Apache-2.0
//! B-rep diagnostics retain scratch storage through the build result.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn built_ir_diagnostic_storage_survives_build_and_releases_with_result() {
    const PROBE: &str = "test built IR diagnostic live storage";
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.declared_body_count = Some(1);
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    scan.planes.positional_frames.push(crate::surface::OutlinePlane {
        surface_id: 5,
        origin: [0.0, 0.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 0,
    });
    scan.topology.face_components.push(
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![5], Vec::new())
        })
        .expect("component admission")
        .expect("one-face component"),
    );

    let run = |cap: u64, release: bool, probe: Option<u64>| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let classification = crate::dialect::classify(&ctx, &scan)?;
        let built = super::super::build_ir(&ctx, &scan, &classification)?;
        assert_eq!(built.brep_diagnostics.face_rejection_diagnostics.len(), 1);
        if release {
            drop(built);
        }
        if let Some(bytes) = probe {
            let _probe = ctx.reserve_scoped(bytes, PROBE)?;
        }
        Ok::<_, CodecError>(())
    };
    let setup_peak = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| run(cap, false, None),
    );
    let probe = setup_peak.checked_add(1).expect("probe exceeds setup peak");
    let live_bytes = |release| {
        crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            Some(PROBE),
            |cap| run(cap, release, Some(probe)),
        )
        .checked_add(1)
        .expect("probe boundary")
        .checked_sub(probe)
        .expect("probe exceeds setup peak")
    };
    assert!(live_bytes(false) > 0);
    assert_eq!(live_bytes(true), 0);
}
