// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_validator_checks_solved_matrix_and_source_offset_for_each_form() {
    use crate::design::decode::placement_construction::work_planes::bind;
    use crate::test_support::work_plane::{placements, scope};
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for placement in placements() {
            let mut scope = scope(&placement);
            let native = crate::native::F3dNative {
                design_placement_constructions: vec![placement],
                ..Default::default()
            };
            assert!(bind(
                std::slice::from_mut(&mut scope),
                &native.design_placement_constructions
            )
            .is_empty());
            let scope = serde_json::from_value::<
                crate::records::feature::scope::DesignParameterScope,
            >(serde_json::to_value(scope).unwrap())
            .unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            assert!(super::super::valid_work_plane_construction(
                &ctx,
                &scope,
                "f3d:Design/BulkStream.dat"
            ));
            let mut bad_offset = scope.clone();
            bad_offset
                .work_plane_frame_mut()
                .unwrap()
                .work_plane_transform_offset += 1;
            assert!(!super::super::valid_work_plane_construction(
                &ctx,
                &bad_offset,
                "f3d:Design/BulkStream.dat"
            ));
            let mut bad_matrix = scope;
            let frame = bad_matrix.work_plane_frame_mut().unwrap();
            let mut rows = frame.work_plane_transform.rows();
            rows[1][3] += 7.0;
            frame.work_plane_transform = rows.try_into().unwrap();
            assert!(!super::super::valid_work_plane_construction(
                &ctx,
                &bad_matrix,
                "f3d:Design/BulkStream.dat"
            ));
        }
    });
}
