// SPDX-License-Identifier: Apache-2.0

use super::transfer_fc05_cap_circles;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{CurveId, SurfaceId};

fn one_fc05_cap_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    for (id, kind) in [
        (1, crate::surface::SurfaceKind::Plane),
        (2, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code01,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 1,
        origin: [0.0, 0.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 1,
    });
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 7,
            type_byte: 5,
            feature_id: 4,
            directions: [0; 2],
            faces: [1, 2].map(std::num::NonZeroU32::new),
            next_edges: [0; 2],
            offset: 7,
        });
    scan.curves.fc05_circles.push(crate::curve::Fc05Circle {
        curve_id: 7,
        center_row_frame: [0.0, 0.0],
        radius_mm: 1.0,
        sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length([
            1.0, 0.0,
        ])
        .expect("unit sample direction")
        .0,
        angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
            sense: crate::curve::ParameterSense::Increasing,
            reference_direction_row_frame: [1.0, 0.0],
        },
        cap_ordinate_row_frame: Some(0.0),
        point_count: 8,
        max_residual: 0.0,
        offset: 7,
    });
    scan
}

fn service_fc05_ir() -> CadIr {
    let scan = one_fc05_cap_scan();
    crate::decode::with_service_context(|ctx| {
        let mut ir = CadIr::empty();
        transfer_fc05_cap_circles(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )?;
        Ok::<CadIr, CodecError>(ir)
    })
    .expect("service FC05 cap transfer admitted")
}

#[test]
fn fc05_cap_transfer_preserves_curve_and_surface_geometry() {
    let ir = service_fc05_ir();
    assert_eq!(ir.model.curves.len(), 1);
    assert_eq!(ir.model.surfaces.len(), 1);
    assert_eq!(
        ir.model.curves[0].id,
        CurveId::compose(&crate::identity::VISIBGEOM_CURVE, 7)
    );
    assert_eq!(
        ir.model.surfaces[0].id,
        SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, 2)
    );
}
