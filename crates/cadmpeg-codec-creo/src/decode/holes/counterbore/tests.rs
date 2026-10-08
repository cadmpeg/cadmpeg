// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

fn service_boundary_circle(
    scan: &crate::container::ContainerScan<'_>,
    ir: &cadmpeg_ir::document::CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    cylinder_ids: &[u32],
    radius: f64,
) -> Option<(u32, Point3, [f64; 3])> {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::counterbore_source_boundary_circle(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            cylinder_ids,
            radius,
        )
    })
    .expect("service boundary circle admitted")
}

fn service_source_patch_geometries(
    sources: &[Vec<u32>],
    existing: &BTreeMap<u32, SurfaceGeometry>,
    bore_diameter: f64,
    counterbore_diameter: f64,
) -> Option<Vec<(u32, cadmpeg_ir::geometry::analytic::CylinderSurface)>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::counterbore_source_patch_geometries(
            ctx,
            sources,
            existing,
            bore_diameter,
            counterbore_diameter,
        )
    })
    .expect("service resources")
}

fn service_corner_patch_geometries(
    sources: &[Vec<u32>],
    corners: &[[[[f64; 3]; 2]; 2]],
    bore_diameter: f64,
    counterbore_diameter: f64,
    depth: f64,
) -> Option<Vec<(u32, cadmpeg_ir::geometry::analytic::CylinderSurface)>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::counterbore_source_corner_patch_geometries(
            ctx,
            sources,
            corners,
            bore_diameter,
            counterbore_diameter,
            depth,
        )
    })
    .expect("service resources")
}

fn service_unique_model_surface_geometries(
    ir: &cadmpeg_ir::document::CadIr,
) -> Option<BTreeMap<u32, &SurfaceGeometry>> {
    crate::decode::with_test_decode_ctx(|ctx| super::unique_model_surface_geometries(ctx, ir))
        .expect("service resources")
}

fn limit_ctx<'a>(
    arena: &'a cadmpeg_core::decode::DecodeArena,
    policy: &'a cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::decode::DecodeContext<'a> {
    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, policy)
        .expect("empty root admitted")
        .0
}

fn counterbore_source_limit_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let entry = |entity_id| crate::feature::entity::FeatureEntityTableEntry {
        entity_id,
        payload: crate::feature::entity::entry_payload(200, Some(5), None, None),
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            40,
            29,
            vec![entry(10), entry(11)],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([10, 11]),
    );
    for id in [10, 11] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 40,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan
}

fn counterbore_source_limit_error(operation: &'static str) -> cadmpeg_core::CodecError {
        let run = |limit| {
    let scan = counterbore_source_limit_scan();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let ctx = limit_ctx(&arena, &policy);
    super::counterbore_cylinder_sources(&ctx, &scan, 40)
        };
        let limit = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some(operation), &run);
        run(limit).expect_err("named collection boundary")
    }

#[test]
fn counterbore_source_nodes_refuse_collection_limit() {
    assert!(matches!(counterbore_source_limit_error("creo counterbore source nodes"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore source nodes"));
}

#[test]
fn counterbore_source_cylinder_ids_refuse_collection_limit() {
    assert!(matches!(counterbore_source_limit_error("creo counterbore source cylinder IDs"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore source cylinder IDs"));
}

#[test]
fn counterbore_source_groups_refuse_collection_limit() {
    assert!(matches!(counterbore_source_limit_error("creo counterbore source groups"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore source groups"));
    assert_eq!(
        service_counterbore_source_ids(&counterbore_source_limit_scan()),
        Some(vec![vec![10, 11]])
    );
}

fn service_counterbore_source_ids(
    scan: &crate::container::ContainerScan<'_>,
) -> Option<Vec<Vec<u32>>> {
    crate::decode::with_test_decode_ctx(|ctx| super::counterbore_cylinder_sources(ctx, scan, 40))
        .expect("service resources")
}

#[test]
fn counterbore_model_surface_nodes_refuse_collection_limit() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_plane([0.0; 3]));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo counterbore model surface nodes"), |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::unique_model_surface_geometries(&ctx, &ir)
    });
    let ctx = limit_ctx(&arena, &policy);
    let error =
        super::unique_model_surface_geometries(&ctx, &ir).expect_err("map node exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore model surface nodes")
    );
}


fn counterbore_dimension_limit_error(operation: &'static str) -> cadmpeg_core::CodecError {
        let run = |limit| {
    let scan = counterbore_source_limit_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#10").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                0.5,
            )
            .expect("cylinder geometry"),
        )),
        source_object: None,
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let ctx = limit_ctx(&arena, &policy);
    super::counterbore_dimensions(&ctx, &scan, &ir, 40)
        };
        let limit = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some(operation), &run);
        run(limit).expect_err("named collection boundary")
    }

#[test]
fn counterbore_generated_cylinder_nodes_refuse_collection_limit() {
    assert!(matches!(counterbore_dimension_limit_error("creo counterbore generated cylinder nodes"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore generated cylinder nodes"));
}

#[test]
fn counterbore_generated_radii_refuse_collection_limit() {
    assert!(matches!(counterbore_dimension_limit_error("creo counterbore generated radii"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore generated radii"));
}

fn boundary_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 0,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 42,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 2,
        },
    ]);
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 0,
            feature_id: 42,
            directions: [1, 1],
            faces: [std::num::NonZeroU32::new(2), std::num::NonZeroU32::new(1)],
            next_edges: [11, 11],
            offset: 11,
        });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 1,
        });
    scan
}

fn boundary_circle() -> cadmpeg_ir::geometry::Curve {
    cadmpeg_ir::geometry::Curve {
        id: CurveId::mint("creo:visibgeom:curve#11".to_string()).expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("valid CircleCurve fixture"),
        )),
        source_object: None,
    }
}

fn model_plane(origin: [f64; 3]) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#1".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                origin.into(),
                [0.0, 0.0, 1.0].into(),
                [1.0, 0.0, 0.0].into(),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    }
}

#[test]
fn model_surface_geometry_lookup_rejects_duplicate_native_ids() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.0]));
    assert!(service_unique_model_surface_geometries(&ir).is_some());

    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.5]));
    assert!(service_unique_model_surface_geometries(&ir).is_none());
}

#[test]
fn boundary_circle_uses_native_plane_carrier_when_model_plane_is_absent() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());

    assert_eq!(
        service_boundary_circle(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            42,
            &[2],
            1.0
        ),
        Some((1, Point3::new(0.0, 0.0, 0.0), [0.0, 0.0, 1.0]))
    );
}

#[test]
fn boundary_circle_uses_model_plane_carrier_when_native_plane_is_absent() {
    let mut scan = boundary_scan();
    scan.planes.positional_frames.clear();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.0]));

    assert_eq!(
        service_boundary_circle(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            42,
            &[2],
            1.0
        ),
        Some((1, Point3::new(0.0, 0.0, 0.0), [0.0, 0.0, 1.0]))
    );
}

#[test]
fn boundary_circle_rejects_conflicting_model_plane_carrier() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.5]));

    assert_eq!(
        service_boundary_circle(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            42,
            &[2],
            1.0
        ),
        None
    );
}

#[test]
fn boundary_circle_rejects_duplicate_model_curves() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .curves
        .extend([boundary_circle(), boundary_circle()]);

    assert_eq!(
        service_boundary_circle(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            42,
            &[2],
            1.0
        ),
        None
    );
}

#[test]
fn boundary_circle_rejects_duplicate_surface_rows() {
    let mut scan = boundary_scan();
    let duplicate = scan.surfaces.rows[0].clone();
    scan.surfaces.rows.push(duplicate);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());

    assert_eq!(
        service_boundary_circle(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            42,
            &[2],
            1.0
        ),
        None
    );
}

#[test]
fn radius_anchored_counterbore_accepts_signed_depth() {
    let table = crate::feature::definitions::FeatureDimensionTable {
        declared_count: 4,
        entity_ref: Some(88),
        rows: [
            (2, 0.098, 0),
            (2, 0.463_628_944_932_919_5, 1),
            (1, -0.15, 2),
            (2, 0.3125, 3),
        ]
        .into_iter()
        .map(
            |(dimension_type, value, external_id)| crate::feature::definitions::FeatureDimension {
                dimension_type,
                value: crate::feature::definitions::DimensionValue::Resolved(value),
                value_body: Vec::new(),
                direction_byte: 0,
                auxiliary_value: Some(0.0),
                auxiliary_body: Vec::new(),
                external_id,
                references: None,
                offset: 0,
            },
        )
        .collect(),
        offset: 0,
    };

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::counterbore_dimension_values(ctx, std::iter::once(&table).map(Some), &[0.3125])).expect("service resources"),
        Some((0.196, 0.625, 0.15))
    );
}

#[test]
fn overflowing_corner_spans_do_not_match_counterbore_dimensions() {
    let table = crate::feature::definitions::FeatureDimensionTable {
        declared_count: 4,
        entity_ref: Some(88),
        rows: [(0, 1, 8.0), (1, 2, 20.0), (2, 2, 60.0), (3, 2, -295.661)]
            .into_iter()
            .map(|(external_id, dimension_type, value)| {
                crate::feature::definitions::FeatureDimension {
                    dimension_type,
                    value: crate::feature::definitions::DimensionValue::Resolved(value),
                    value_body: Vec::new(),
                    direction_byte: 0,
                    auxiliary_value: Some(0.0),
                    auxiliary_body: Vec::new(),
                    external_id,
                    references: None,
                    offset: 0,
                }
            })
            .collect(),
        offset: 0,
    };
    let corners = [[-f64::MAX, -f64::MAX, 0.0], [f64::MAX, f64::MAX, 0.0]];
    let spans = super::paired_corner_envelope_axis_spans(corners, corners)
        .expect("finite counterbore corner coordinates");

    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::counterbore_envelope_dimension_values(ctx, std::iter::once(&table).map(Some), &[Some(spans), None])
    })
    .expect("admitted counterbore envelope values")
    .is_none());
}

#[test]
fn counterbore_dimension_tuple_restricts_source_radii() {
    let dimensions = (40.0, 120.0, 8.0);

    assert!(super::counterbore_dimension_tuple_matches_radius(
        dimensions, 20.0
    ));
    assert!(super::counterbore_dimension_tuple_matches_radius(
        dimensions, 60.0
    ));
    assert!(!super::counterbore_dimension_tuple_matches_radius(
        dimensions, 24.5
    ));
}

#[test]
fn counterbore_source_patches_require_a_complete_carrier_pair() {
    let carrier = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            0.3125,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let sources = vec![vec![10, 11], vec![30, 31]];
    let existing = BTreeMap::from([(30, carrier)]);

    assert!(service_source_patch_geometries(&sources, &existing, 0.196, 0.625,).is_none());
}

fn counterbore_corner_pairs() -> [[[[f64; 3]; 2]; 2]; 2] {
    [
        [
            [[-20.0, 763.0, -160.0], [20.0, 812.0, -140.0]],
            [[-20.0, 763.0, -140.0], [20.0, 812.0, -120.0]],
        ],
        [
            [[-60.0, 812.0, -200.0], [60.0, 820.0, -140.0]],
            [[-60.0, 812.0, -140.0], [60.0, 820.0, -80.0]],
        ],
    ]
}

#[test]
fn counterbore_source_patches_refuse_collection_limit() {
    let carrier = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            0.3125,
        )
        .expect("cylinder carrier"),
    ));
    let sources = vec![vec![10, 11], vec![30, 31]];
    let existing = BTreeMap::from([(30, carrier.clone()), (31, carrier)]);
    assert_eq!(
        service_source_patch_geometries(&sources, &existing, 0.196, 0.625)
            .expect("complete carrier")
            .len(),
        4
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo counterbore source patches"), |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::counterbore_source_patch_geometries(&ctx, &sources, &existing, 0.196, 0.625)
    });
    let ctx = limit_ctx(&arena, &policy);
    let error = super::counterbore_source_patch_geometries(&ctx, &sources, &existing, 0.196, 0.625)
        .expect_err("first source patch exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore source patches")
    );
}

#[test]
fn counterbore_corner_patches_refuse_collection_limit() {
    let sources = vec![vec![2636, 2662], vec![2640, 2666]];
    let corners = counterbore_corner_pairs();
    assert_eq!(
        service_corner_patch_geometries(&sources, &corners, 40.0, 120.0, 8.0)
            .expect("complete corners")
            .len(),
        4
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo counterbore corner patches"), |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::counterbore_source_corner_patch_geometries(
        &ctx, &sources, &corners, 40.0, 120.0, 8.0,
    )
    });
    let ctx = limit_ctx(&arena, &policy);
    let error = super::counterbore_source_corner_patch_geometries(
        &ctx, &sources, &corners, 40.0, 120.0, 8.0,
    )
    .expect_err("first corner patch exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore corner patches")
    );
}

#[test]
fn counterbore_source_corner_envelopes_refuse_collection_limit() {
    let replay = [
        24, 45, 82, 36, 168, 193, 84, 201, 135, 18, 45, 89, 164, 168, 193, 84, 201, 135, 47, 34, 0,
        47, 32, 0, 47, 20, 0, 47, 36, 0, 47, 67, 0, 47, 24, 247, 24,
    ];
    let mut scan = crate::test_support::empty_container_scan();
    for id in [10, 11] {
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend(replay);
        payload.push(0xe3);
        let mut records = crate::decode::with_test_decode_ctx(|ctx| {
            crate::surface::parameter_records(ctx, &payload)
        })
        .expect("parameter record fixture");
        assert_eq!(records.len(), 1);
        let mut record = records.remove(0);
        record.surface_id = id;
        record.offset = usize::try_from(id).expect("fixture index fits usize");
        assert_eq!(record.surface_id, id);
        assert!(record.type24_terminal_corner_envelope().is_some());
        scan.surfaces.parameters.push(record);
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 40,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    let sources = [vec![10, 11]];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::counterbore_source_corner_envelopes(ctx, &scan, &sources)
        })
        .expect("service resources")
        .expect("corner pair")
        .len(),
        1
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo counterbore source corner envelopes"), |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::counterbore_source_corner_envelopes(&ctx, &scan, &sources)
        .map(|result| {
            result.map(|sources| {
                sources
                    .into_iter()
                    .map(|source| [source.first, source.second])
                    .collect::<Vec<_>>()
            })
        })
    });
    let ctx = limit_ctx(&arena, &policy);
    let error = super::counterbore_source_corner_envelopes(&ctx, &scan, &sources)
        .map(|result| {
            result.map(|sources| {
                sources
                    .into_iter()
                    .map(|source| [source.first, source.second])
                    .collect::<Vec<_>>()
            })
        })
        .expect_err("corner envelope exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo counterbore source corner envelopes")
    );
}

#[test]
fn counterbore_patch_rows_preserve_source_order() {
    let mut scan = crate::test_support::empty_container_scan();
    let entry = |entity_id, source_id| crate::feature::entity::FeatureEntityTableEntry {
        entity_id,
        payload: crate::feature::entity::entry_payload(200, Some(source_id), None, None),
        prefixed: false,
        offset: usize::try_from(entity_id).expect("fixture index fits usize"),
        end_offset: usize::try_from(entity_id).expect("fixture index fits usize") + 1,
    };
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            40,
            29,
            vec![entry(1, 100), entry(2, 100), entry(3, 101), entry(4, 101)],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([1, 2, 3, 4]),
    );
    for id in 1..=4 {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 40,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(911),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::definitions::FeatureDimensionTable {
                declared_count: 4,
                entity_ref: Some(88),
                rows: [(2, 20.0, 0), (2, 1.0, 1), (1, 8.0, 2), (2, 60.0, 3)]
                    .into_iter()
                    .map(|(dimension_type, value, external_id)| {
                        crate::feature::definitions::FeatureDimension {
                            dimension_type,
                            value: crate::feature::definitions::DimensionValue::Resolved(value),
                            value_body: Vec::new(),
                            direction_byte: 0,
                            auxiliary_value: Some(0.0),
                            auxiliary_body: Vec::new(),
                            external_id,
                            references: None,
                            offset: 0,
                        }
                    })
                    .collect(),
                offset: 0,
            }),
            relations: None,
            saved_section: None,
            offset: 0,
        });
    let model_cylinder = |id| cadmpeg_ir::geometry::Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                60.0,
            )
            .expect("cylinder geometry"),
        )),
        source_object: None,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .extend([model_cylinder(1), model_cylinder(2)]);
    let service = crate::decode::with_test_decode_ctx(|ctx| {
        super::counterbore_patch_geometries(ctx, &scan, &ir, 40)
    })
    .expect("service resources")
    .expect("source patches");
    assert_eq!(
        service.iter().map(|(row, _)| row.id).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );

}

#[test]
fn corner_envelopes_construct_dimensioned_source_cylinders() {
    let sources = vec![vec![2636, 2662], vec![2640, 2666]];
    let geometries =
        service_corner_patch_geometries(&sources, &counterbore_corner_pairs(), 40.0, 120.0, 8.0)
            .expect("complete paired corner envelopes select one counterbore assignment");
    let expected = |radius| {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 820.0, -140.0),
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .expect("valid CylinderSurface fixture"),
        ))
    };
    assert_eq!(
        geometries
            .into_iter()
            .map(|(id, geometry)| (
                id,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(geometry))
            ))
            .collect::<Vec<_>>(),
        vec![
            (2636, expected(20.0)),
            (2662, expected(20.0)),
            (2640, expected(60.0)),
            (2666, expected(60.0)),
        ]
    );
}

#[test]
fn corner_envelopes_reject_incomplete_or_inconsistent_source_joins() {
    let sources = vec![vec![2636, 2662], vec![2640, 2666]];
    let corners = counterbore_corner_pairs();
    assert!(service_corner_patch_geometries(&sources, &corners, 40.0, 120.0, 7.0,).is_none());

    let mut shifted = corners;
    shifted[1][0][0][0] = -59.0;
    assert!(service_corner_patch_geometries(&sources, &shifted, 40.0, 120.0, 8.0).is_none());

    assert!(service_corner_patch_geometries(
        &[vec![2636], vec![2640, 2666]],
        &corners,
        40.0,
        120.0,
        8.0,
    )
    .is_none());
    assert!(service_corner_patch_geometries(
        &[vec![2636, 2662], vec![2662, 2666]],
        &corners,
        40.0,
        120.0,
        8.0,
    )
    .is_none());
}

#[test]
fn counterbore_surface_identity_prefix_refuses_work() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.0]));
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo counterbore surface identity prefix",
        |ctx| super::unique_model_surface_geometries(ctx, &ir),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo counterbore surface identity prefix")
    );
}

#[test]
fn counterbore_surface_index_borrows_geometry() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.0]));
    let indexed = crate::test_support::assert_work_boundaries(
        &["creo counterbore model surface scan", "creo counterbore surface identity prefix", "creo scalar text parsing"],
        |ctx| super::unique_model_surface_geometries(ctx, &ir),
    ).expect("unique surface");
    let geometry = indexed.values().next().expect("one surface");
    assert!(std::ptr::eq(*geometry, &ir.model.surfaces[0].geometry));
}
