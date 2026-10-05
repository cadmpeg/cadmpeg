// SPDX-License-Identifier: Apache-2.0
//! Tests: datum planes and coordinate systems from feature carriers.

use crate::decode::feature_history::draft::schema_feature_definition;
use crate::feature::schema::SchemaClass;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
    UnresolvedFamily,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn finite_local_system(values: [f64; 12]) -> cadmpeg_ir::units::FiniteVector<12> {
    cadmpeg_ir::units::FiniteVector::new(values).expect("finite local system fixture")
}

#[test]
fn datum_feature_uses_its_unique_transferred_plane_carrier() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture"),
        })
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            None,
            "Native Feature"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture"),
        })
    );

    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 1,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane
        })
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            None,
            "Native Feature"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Native { .. })
    ));
}

#[test]
fn datum_feature_preserves_its_unique_transferred_plane_chart() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 6,
        origin: [0.0, 1.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        offset: 1,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane",
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture"),
        })
    );
}

#[test]
fn datum_feature_uses_its_unique_complete_local_system() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(5),
                owner_feature_id: Some(5),
            },
            body: Vec::new(),
            parameter_frames: vec![
                crate::feature::definitions::FeatureParameterFrame {
                    kind: crate::feature::definitions::FeatureParameterFrameKind::LocalSystem,
                    body: Vec::new(),
                    decoded_values: Some(finite_local_system([
                        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 3.0, 4.0, 5.0,
                    ])),
                    offset: 1,
                },
                crate::feature::definitions::FeatureParameterFrame {
                    kind: crate::feature::definitions::FeatureParameterFrameKind::LocalSystem,
                    body: vec![0xff],
                    decoded_values: None,
                    offset: 2,
                },
            ],
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(3.0, 4.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .expect("valid test fixture"),
        })
    );
}

#[test]
fn coordinate_system_feature_uses_its_unique_complete_local_system() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: Some(7),
            },
            body: Vec::new(),
            parameter_frames: vec![
                crate::feature::definitions::FeatureParameterFrame {
                    kind: crate::feature::definitions::FeatureParameterFrameKind::LocalSystem,
                    body: Vec::new(),
                    decoded_values: Some(finite_local_system([
                        0.0, 2.0, 0.0, -3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 5.0, 6.0, 7.0,
                    ])),
                    offset: 1,
                },
                crate::feature::definitions::FeatureParameterFrame {
                    kind: crate::feature::definitions::FeatureParameterFrameKind::LocalSystem,
                    body: vec![0xff],
                    decoded_values: None,
                    offset: 2,
                },
            ],
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            Some(SchemaClass::CoordinateSystem),
            "PRT_CSYS_DEF"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::DatumCoordinateSystem {
            frame: cadmpeg_ir::features::FeatureCoordinateFrame::new(
                Point3::new(5.0, 6.0, 7.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture")
        })
    );
}

#[test]
fn coordinate_system_feature_rejects_a_reflected_local_system() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: Some(7),
            },
            body: Vec::new(),
            parameter_frames: vec![crate::feature::definitions::FeatureParameterFrame {
                kind: crate::feature::definitions::FeatureParameterFrameKind::LocalSystem,
                body: Vec::new(),
                decoded_values: Some(finite_local_system([
                    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, 5.0, 6.0, 7.0,
                ])),
                offset: 1,
            }],
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            Some(SchemaClass::CoordinateSystem),
            "PRT_CSYS_DEF"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem
        })
    );
}

#[test]
fn coordinate_system_feature_rejects_a_local_system_outside_the_record_tolerance() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: Some(7),
            },
            body: Vec::new(),
            parameter_frames: vec![crate::feature::definitions::FeatureParameterFrame {
                kind: crate::feature::definitions::FeatureParameterFrameKind::LocalSystem,
                body: Vec::new(),
                decoded_values: Some(finite_local_system([
                    1.0, 0.0, 0.0, 1.0e-10, 1.0, 0.0, 0.0, 0.0, 1.0, 5.0, 6.0, 7.0,
                ])),
                offset: 1,
            }],
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        });

    // The normalized columns reach the IR frame constructor as unit vectors whose largest
    // pairwise dot is 1.0e-10, and that constructor admits them.
    assert!(cadmpeg_ir::features::FeatureCoordinateFrame::new(
        Point3::new(5.0, 6.0, 7.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0e-10, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0)
    )
    .is_some());
    // The record is written to a tighter bound, so the feature stays unresolved.
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
            Some(SchemaClass::CoordinateSystem),
            "PRT_CSYS_DEF"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem
        })
    );
}
