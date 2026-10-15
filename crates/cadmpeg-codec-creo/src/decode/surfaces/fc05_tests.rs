// SPDX-License-Identifier: Apache-2.0

use super::transfer_fc05_cap_circles;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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

fn transfer_with_retained_limit(limit: u64, existing_curve: bool) -> Result<CadIr, CodecError> {
    let scan = one_fc05_cap_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let mut ir = CadIr::empty();
    if existing_curve {
        ir.model
            .curves
            .push(service_fc05_ir().model.curves[0].clone());
    }
    transfer_fc05_cap_circles(
        &ctx,
        &scan,
        &mut ir,
        &mut AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )?;
    Ok(ir)
}

fn service_fc05_ir() -> CadIr {
    let scan = one_fc05_cap_scan();
    crate::decode::with_test_decode_ctx(|ctx| {
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

#[test]
fn fc05_cap_curve_identity_refuses_retained_limit() {
    let error = transfer_with_retained_limit(
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo FC05 cap circle identity"),
            |cap| transfer_with_retained_limit(cap, false),
        ),
        false,
    )
    .expect_err("curve identity exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo FC05 cap circle identity"));
}

#[test]
fn fc05_cap_curve_source_object_refuses_retained_limit() {
    let run = |limit| transfer_with_retained_limit(limit, false);
    let error = run(crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        Some("creo FC05 cap circle source object ID"),
        run,
    ))
    .expect_err("named resource boundary");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo FC05 cap circle source object ID"));
}

#[test]
fn fc05_axis_cylinder_identity_refuses_retained_limit() {
    let error = transfer_with_retained_limit(
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo FC05 axis cylinder identity"),
            |cap| transfer_with_retained_limit(cap, true),
        ),
        true,
    )
    .expect_err("axis cylinder identity exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo FC05 axis cylinder identity"));
}

#[test]
fn fc05_axis_cylinder_source_object_refuses_retained_limit() {
    let run = |limit| transfer_with_retained_limit(limit, true);
    let error = run(crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        Some("creo FC05 axis cylinder source object ID"),
        run,
    ))
    .expect_err("named resource boundary");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo FC05 axis cylinder source object ID"));
}

#[test]
fn fc05_cap_transfer_uses_first_duplicate_pair_for_shared_circles() {
    let mut scan = one_fc05_cap_scan();
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 3,
        origin: [0.0, 2.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 3,
    });
    let pair = crate::curve::Fc05CylinderCapPair {
        surface_id: 2,
        cap_edges: vec![
            crate::curve::Fc05CapEdge {
                curve_id: 7,
                cap_plane_id: 1,
                cap_ordinate_row_frame: 0.0,
            },
            crate::curve::Fc05CapEdge {
                curve_id: 8,
                cap_plane_id: 3,
                cap_ordinate_row_frame: 2.0,
            },
        ],
        center_row_frame: [0.0, 0.0],
        radius_mm: 1.0,
        reference_direction_row_frame: [1.0, 0.0],
        parameter_sense: crate::curve::ParameterSense::Increasing,
        cap_ordinates_row_frame: vec![0.0, 2.0],
        offset: 4,
    };
    let mut reversed = pair.clone();
    reversed.cap_edges[0].cap_plane_id = 3;
    reversed.cap_edges[1].cap_plane_id = 1;
    scan.curves.fc05_cylinder_cap_pairs.extend([pair, reversed]);
    let mut circle = scan.curves.fc05_circles[0].clone();
    circle.curve_id = 8;
    scan.curves.fc05_circles.push(circle);
    let mut row = scan.curves.topology_rows[0].clone();
    row.id = 8;
    scan.curves.topology_rows.push(row);
    crate::test_support::assert_work_boundaries(
        &[
            "creo circle cap pair index traversal",
            "creo cap pair placed edge traversal",
        ],
        |ctx| {
            let mut ir = CadIr::empty();
            transfer_fc05_cap_circles(
                ctx,
                &scan,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )?;
            assert_eq!(ir.model.curves.len(), 2);
            for curve in &ir.model.curves {
                let cadmpeg_ir::geometry::CurveGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedCurveGeometry::Circle(circle),
                ) = &curve.geometry
                else {
                    panic!("circle carrier");
                };
                assert_eq!(
                    *circle.frame().axis().as_raw(),
                    cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
                );
            }
            assert_eq!(ir.model.surfaces.len(), 1);
            Ok(())
        },
    );
}
