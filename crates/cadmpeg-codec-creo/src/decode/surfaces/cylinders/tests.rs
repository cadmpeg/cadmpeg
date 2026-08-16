// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::SurfaceGeometry;

fn slot_fillet_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 913,
        header: [0, 0],
        root_schema_class: Some(913),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4, 5, 6],
            offset: 0,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 913,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 7,
    });
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 1,
        },
        crate::surface::OutlinePlane {
            surface_id: 2,
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 2,
        },
        crate::surface::OutlinePlane {
            surface_id: 3,
            origin: [0.0, -1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 3,
        },
        crate::surface::OutlinePlane {
            surface_id: 4,
            origin: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 4,
        },
        crate::surface::OutlinePlane {
            surface_id: 5,
            origin: [0.0, 0.0, -1.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 5,
        },
        crate::surface::OutlinePlane {
            surface_id: 6,
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 6,
        },
    ]);
    scan
}

fn model_plane(id: u32, origin: [f64; 3], normal: [f64; 3]) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin: origin.into(),
            normal: normal.into(),
            u_axis: [1.0, 0.0, 0.0].into(),
        },
        source_object: None,
    }
}

#[test]
fn constrained_slot_fillet_uses_native_plane_carriers_when_model_planes_are_absent() {
    let scan = slot_fillet_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let transferred = super::transfer_constrained_slot_fillet_cylinders(
        &scan,
        &mut ir,
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
    );

    assert_eq!(transferred, 1);
    let [surface] = ir.model.surfaces.as_slice() else {
        panic!("one generated cylinder");
    };
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        radius,
        ..
    } = surface.geometry
    else {
        panic!("generated cylinder: {:?}", surface.geometry);
    };
    assert_eq!(origin, [0.0, 0.0, 0.0].into());
    assert_eq!(axis, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 1.0);
}

#[test]
fn constrained_slot_fillet_uses_transferred_plane_carriers_when_native_planes_are_absent() {
    let mut scan = slot_fillet_scan();
    scan.planes.positional_frames.clear();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.extend([
        model_plane(1, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        model_plane(2, [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        model_plane(3, [0.0, -1.0, 0.0], [0.0, 1.0, 0.0]),
        model_plane(4, [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]),
        model_plane(5, [0.0, 0.0, -1.0], [0.0, 0.0, 1.0]),
        model_plane(6, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]),
    ]);

    assert_eq!(
        super::transfer_constrained_slot_fillet_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        ),
        1
    );
}

#[test]
fn constrained_slot_fillet_rejects_conflicting_model_plane_carriers() {
    let scan = slot_fillet_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model
        .surfaces
        .push(model_plane(3, [0.0, -0.5, 0.0], [0.0, 1.0, 0.0]));

    assert_eq!(
        super::transfer_constrained_slot_fillet_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        ),
        0
    );
    assert!(ir
        .model
        .surfaces
        .iter()
        .all(|surface| { surface.id.as_str() != "creo:visibgeom:surface#7" }));
}
