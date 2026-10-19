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

#[test]
fn legacy_visible_surface_rows_are_aggregated() {
    let bytes = legacy_surface_row_fixture("Sld_VisGeom", "active_geom");
    assert_eq!(
        crate::container::scan_bytes_ok(bytes).surfaces.rows.len(),
        1,
        "the legacy fixture supplies one visible row"
    );
}

#[test]
fn legacy_nonvisible_surface_rows_are_aggregated() {
    let bytes = legacy_surface_row_fixture("Sld_NonVisGeom", "inactive_geom");
    assert_eq!(
        crate::container::scan_bytes_ok(bytes)
            .surfaces
            .nonvisible_rows
            .len(),
        1,
        "the legacy fixture supplies one nonvisible row"
    );
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

#[test]
fn placement_outline_planes_add_positional_planes_missing_from_outline() {
    let rows = crate::container::placement_outline_planes(
        &[placement_plane_fixture(7)],
        &[placement_plane_fixture(7), placement_plane_fixture(8)],
    );
    assert_eq!(
        rows.iter().map(|row| row.surface_id).collect::<Vec<_>>(),
        [7, 8]
    );
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
fn prototype_topology_rows_append_into_empty_rows() {
    let mut rows = Vec::new();
    crate::container::append_topology_rows(&mut rows, [topology_row_fixture(7)].into_iter());
    assert_eq!(rows[0].id, 7);
}

#[test]
fn legacy_curve_witnesses_append_topology_rows_and_pcurves() {
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
    let mut topology_rows = Vec::new();
    let mut pcurves = Vec::new();
    crate::container::append_legacy_curve_witnesses(
        &mut topology_rows,
        &mut pcurves,
        &legacy.topology_rows,
        &legacy.pcurves,
    );
    assert_eq!((topology_rows[0].id, pcurves[0].curve_id), (7, 7));
}

#[test]
fn section_owner_range_refuses_end_overflow() {
    let row = crate::feature::rows::FeatureRow {
        feature_id: 1,
        root_schema_class: None,
        stream_offset: 0,
        body: vec![0xeb, 0x04]
            .try_into()
            .expect("two fixture header bytes"),
        body_offset: usize::MAX - 1,
        offset: usize::MAX - 2,
    };
    assert!(matches!(
        crate::container::section_owner_ranges(&[], &[row]),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}
