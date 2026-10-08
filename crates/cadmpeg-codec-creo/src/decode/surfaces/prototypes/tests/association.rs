use crate::decode::surfaces::prototypes::{first_instance_surface_row, PrototypeRows};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn row(offset: usize, id: u32, kind: crate::surface::SurfaceKind) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 1,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset,
    }
}

#[test]
fn prototype_uses_the_preceding_same_family_row() {
    let rows = [
        row(100, 10, crate::surface::SurfaceKind::Plane),
        row(200, 20, crate::surface::SurfaceKind::Plane),
    ];

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| first_instance_surface_row(
            ctx,
            &PrototypeRows::new(ctx, &rows)?,
            100,
            300,
            150,
            crate::surface::SurfaceKind::Plane
        ))
        .expect("prototype row selection")
        .map(|row| row.id),
        Some(10)
    );
}

#[test]
fn prototype_before_frame_rows_uses_the_following_same_family_row() {
    let rows = [row(100, 10, crate::surface::SurfaceKind::Plane)];

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| first_instance_surface_row(
            ctx,
            &PrototypeRows::new(ctx, &rows)?,
            100,
            300,
            50,
            crate::surface::SurfaceKind::Plane
        ))
        .expect("prototype row selection")
        .map(|row| row.id),
        Some(10)
    );
}

#[test]
fn prototype_after_a_different_family_uses_the_following_family_row() {
    let rows = [
        row(100, 10, crate::surface::SurfaceKind::Cylinder),
        row(200, 20, crate::surface::SurfaceKind::Plane),
    ];

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| first_instance_surface_row(
            ctx,
            &PrototypeRows::new(ctx, &rows)?,
            100,
            300,
            150,
            crate::surface::SurfaceKind::Plane
        ))
        .expect("prototype row selection"),
        Some(&rows[1])
    );
}

#[test]
fn prototype_row_selection_refuses_before_preceding_scan() {
    let rows = [row(100, 10, crate::surface::SurfaceKind::Plane)];
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo prototype row selection",
        |ctx| {
            first_instance_surface_row(
                ctx,
                &PrototypeRows::new(ctx, &rows)?,
                100,
                300,
                150,
                crate::surface::SurfaceKind::Plane,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo prototype row selection"));
}

fn prototype_association_scan() -> crate::container::ContainerScan<'static> {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff,
    ]);
    payload.push(0xe3);
    crate::test_support::push_named_analytic_prototype(
        &mut payload,
        "torus",
        &[("radius1", 1.0), ("radius2", 2.0)],
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    crate::container::scan_bytes_ok(crate::test_support::build_prt(
        "prototype-association-limit",
        &[("ND:0:VisibGeom:0", payload)],
    ))
}

fn association_result(limit: u64) -> Result<usize, CodecError> {
    let scan = prototype_association_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
    Ok(super::super::unique_surface_prototype_associations(&ctx, &scan)?.len())
}

#[test]
fn prototype_association_vec_refuses_before_growth() {
    assert_eq!(
        association_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            None,
            |cap| association_result(cap)
        ))
        .expect("service limit admits association"),
        1
    );
    let error = association_result(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo surface prototype associations"),
        |cap| association_result(cap),
    ))
    .expect_err("one association needs a vector item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo surface prototype associations"
    ));
}

#[test]
fn prototype_association_row_count_refuses_before_node_insertion() {
    let error = association_result(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo surface prototype row counts"),
        |cap| association_result(cap),
    ))
    .expect_err("row count follows association vector");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo surface prototype row counts"
    ));
}

#[test]
fn prototype_association_retention_refuses_work_and_preserves_result() {
    let scan = prototype_association_scan();
    let associated_rows = crate::test_support::assert_work_boundaries(
        &["creo unique surface prototype associations retention"],
        |ctx| {
            Ok(
                super::super::unique_surface_prototype_associations(ctx, &scan)?
                    .iter()
                    .map(|(_, row, _)| row.id)
                    .collect::<Vec<_>>(),
            )
        },
    );
    assert_eq!(associated_rows, [7]);
}

#[test]
fn prototype_row_index_preserves_unsorted_rows_and_equal_offset_ties() {
    let rows = [
        row(200, 1, crate::surface::SurfaceKind::Plane),
        row(100, 2, crate::surface::SurfaceKind::Plane),
        row(100, 3, crate::surface::SurfaceKind::Plane),
        row(200, 4, crate::surface::SurfaceKind::Plane),
    ];
    crate::test_support::assert_work_boundaries(
        &[
            "creo prototype row index",
            "creo prototype row selection",
            "creo prototype following row selection",
        ],
        |ctx| {
            let index = PrototypeRows::new(ctx, &rows)?;
            assert_eq!(
                first_instance_surface_row(
                    ctx,
                    &index,
                    100,
                    300,
                    150,
                    crate::surface::SurfaceKind::Plane
                )?,
                Some(&rows[2])
            );
            assert_eq!(
                first_instance_surface_row(
                    ctx,
                    &index,
                    100,
                    300,
                    50,
                    crate::surface::SurfaceKind::Plane
                )?,
                Some(&rows[1])
            );
            assert_eq!(
                first_instance_surface_row(
                    ctx,
                    &index,
                    150,
                    300,
                    175,
                    crate::surface::SurfaceKind::Plane
                )?,
                Some(&rows[0])
            );
            Ok(())
        },
    );
}
