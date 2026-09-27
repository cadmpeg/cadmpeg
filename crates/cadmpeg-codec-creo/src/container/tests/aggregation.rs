// SPDX-License-Identifier: Apache-2.0

fn legacy_surface_row_fixture(root: &str, branch: &str) -> Vec<u8> {
    format!(
        "#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
@{root} 1 0\n@{branch} 2 0\n@srf_array 3 0\n\
@geom_type 4 1\n@geom_id 5 1\n@feat_id 6 1\n\
@boundary_type 7 1\n@next_geom_ptr 8 1\n@orient 9 1\n\
0 1 ->\n1 2 ->\n2 3 [1]\n3 3 ->\n\
4 4 36\n4 5 42\n4 6 7\n4 7 0\n4 8 0\n4 9 1\n\
#END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n"
    )
    .into_bytes()
}

fn scan_legacy_surface_with_limit(
    bytes: &[u8],
    limit: u64,
) -> Result<crate::container::ContainerScan<'static>, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("legacy fixture root");
    crate::container::scan_bytes(&ctx, bytes.to_vec())
}

#[test]
fn legacy_visible_surface_row_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = legacy_surface_row_fixture("Sld_VisGeom", "active_geom");
    assert_eq!(
        crate::container::scan_bytes_ok(bytes.clone())
            .surfaces
            .rows
            .len(),
        1,
        "the legacy fixture supplies one visible row"
    );
    let refusal = (0..128).find_map(|limit| {
        let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) =
            scan_legacy_surface_with_limit(&bytes, limit)
        else {
            return None;
        };
        (refusal.operation == "creo legacy surface row aggregation").then_some(refusal)
    });
    assert!(matches!(
        refusal,
        Some(limit) if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn legacy_nonvisible_surface_row_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = legacy_surface_row_fixture("Sld_NonVisGeom", "inactive_geom");
    assert_eq!(
        crate::container::scan_bytes_ok(bytes.clone())
            .surfaces
            .nonvisible_rows
            .len(),
        1,
        "the legacy fixture supplies one nonvisible row"
    );
    let refusal = (0..128).find_map(|limit| {
        let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) =
            scan_legacy_surface_with_limit(&bytes, limit)
        else {
            return None;
        };
        (refusal.operation == "creo legacy nonvisible surface row aggregation").then_some(refusal)
    });
    assert!(matches!(
        refusal,
        Some(limit) if limit.dimension == ResourceDimension::CollectionItems
    ));
}

fn placement_plane_fixture(surface_id: u32) -> crate::surface::OutlinePlane {
    crate::surface::OutlinePlane {
        surface_id,
        origin: [0.0; 3],
        normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: usize::try_from(surface_id).expect("small fixture ID"),
    }
}

fn placement_plane_result(
    limit: u64,
) -> Result<Vec<crate::surface::OutlinePlane>, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    crate::container::placement_outline_planes(
        &ctx,
        &[placement_plane_fixture(7)],
        &[placement_plane_fixture(7), placement_plane_fixture(8)],
    )
}

#[test]
fn placement_outline_plane_copy_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let rows = placement_plane_result(2).expect("two plane copies are admitted");
    assert_eq!(
        rows.iter().map(|row| row.surface_id).collect::<Vec<_>>(),
        [7, 8]
    );
    assert!(matches!(
        placement_plane_result(0),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo placement outline plane copies"
    ));
}

#[test]
fn positional_placement_plane_copy_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(matches!(
        placement_plane_result(1),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo positional placement plane copies"
    ));
}

fn topology_row_fixture(id: u32) -> crate::curve::CurveTopologyRow {
    crate::curve::CurveTopologyRow {
        id,
        type_byte: 0x13,
        feature_id: 1,
        directions: [0; 2],
        faces: [None; 2],
        next_edges: [0; 2],
        offset: usize::try_from(id).expect("small fixture ID"),
    }
}

#[test]
fn prototype_topology_row_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let mut rows = Vec::new();
        crate::container::append_topology_rows(
            &ctx,
            &mut rows,
            [topology_row_fixture(7)].into_iter(),
            "creo prototype topology row aggregation",
        )?;
        Ok::<_, cadmpeg_core::CodecError>(rows)
    };
    assert_eq!(run(1).expect("one topology row is admitted")[0].id, 7);
    assert!(matches!(
        run(0),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo prototype topology row aggregation"
    ));
}

fn legacy_curve_witnesses_result(
    limit: u64,
) -> Result<
    (
        Vec<crate::curve::CurveTopologyRow>,
        Vec<crate::curve::PcurveEndpoints>,
    ),
    cadmpeg_core::CodecError,
> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let legacy = crate::legacy_geometry::LegacyGeometryScan {
        topology_rows: vec![topology_row_fixture(7)],
        pcurves: vec![crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [None; 2],
            face_0_endpoints: [[0.0; 2]; 2],
            face_1_endpoints: [[0.0; 2]; 2],
            offset: 7,
        }],
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut topology_rows = Vec::new();
    let mut pcurves = Vec::new();
    crate::container::append_legacy_curve_witnesses(
        &ctx,
        &mut topology_rows,
        &mut pcurves,
        &legacy.topology_rows,
        &legacy.pcurves,
    )?;
    Ok((topology_rows, pcurves))
}

#[test]
fn legacy_topology_row_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let (rows, pcurves) = legacy_curve_witnesses_result(2).expect("two witnesses are admitted");
    assert_eq!((rows[0].id, pcurves[0].curve_id), (7, 7));
    assert!(matches!(
        legacy_curve_witnesses_result(0),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo legacy topology row aggregation"
    ));
}

#[test]
fn legacy_pcurve_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(matches!(
        legacy_curve_witnesses_result(1),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo legacy pcurve aggregation"
    ));
}
