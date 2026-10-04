// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::CodecError;
use cadmpeg_ir::units::UnitVector3;
use crate::surface::{BoundaryType, ExtrusionVariant, OutlinePlane, SurfaceKind, SurfaceRow};

#[test]
fn surface_row_cost_counts_active_extrusion_variant() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let offset_cost = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>());
        // Three identifiers, an orientation flag and two enum tags precede the offset.
        for (kind, fields_cost) in [
            (SurfaceKind::Plane, 15), (SurfaceKind::Cylinder, 15), (SurfaceKind::Cone, 15),
            (SurfaceKind::TorusOrSphere, 15), (SurfaceKind::Spline, 15), (SurfaceKind::Fillet, 15),
            (SurfaceKind::Extrusion(ExtrusionVariant::Linear), 16),
            (SurfaceKind::Extrusion(ExtrusionVariant::TabulatedCylinder), 16),
        ] {
            let row = SurfaceRow { id: 7, kind, feature_id: 4, reversed: false, boundary_type: BoundaryType::Code00, next_surface: 0, offset: 15 };
            assert_eq!(row.decode_cost(ctx, "surface row cost")?, fields_cost + offset_cost);
        }
        Ok::<(), CodecError>(())
    }).expect("surface row costs fit service work");
}

#[test]
fn outline_plane_cost_excludes_struct_padding() {
    let plane = OutlinePlane { surface_id: 7, origin: [0.0; 3], normal: UnitVector3::Y_AXIS, u_axis: UnitVector3::X_AXIS, offset: 15 };
    crate::decode::with_test_decode_ctx(|ctx| {
        // One identifier, nine coordinates and one offset are the complete value.
        let expected = 4 + 9 * 8 + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>());
        assert_eq!(plane.decode_cost(ctx, "outline plane cost")?, expected);
        Ok::<(), CodecError>(())
    }).expect("outline plane cost fits service work");
}
