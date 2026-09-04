use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};

use crate::families::standard::fbb::FbbPopulationLayout;
use crate::families::standard::records::{
    StandardCurveGeometry, StandardCurveSupport, StandardFaceBounds, StandardSurfacePopulation,
    StandardSurfaceRecord, SurfacePrefix,
};
use crate::test_support::{
    a8_freeform_curve_stream, a8_surface_stream, append_b5_record, b5_closed_triangle_stream,
    compact_standard_triangle_topology_stream, fbb_mixed_boundary_topology_stream,
    fbb_only_quad_topology_stream, le_f32, le_f64, standard_quad_topology_stream,
};

#[test]
fn standard_torus_major_sign_selects_the_axis_hemisphere() {
    let mut bytes = vec![0x00, 0x33, 0x38];
    for value in [0.0_f32, 0.0, 7.0, 0.0, 0.0, -20.0, 5.0] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    let surface = crate::families::standard::records::decode_curved(
        &bytes,
        &crate::families::standard::records::SurfacePrefix {
            pos: 0,
            target: 0,
            kind: 0x38,
        },
    )
    .expect("signed torus carrier");
    let SurfaceGeometry::Torus {
        axis,
        major_radius,
        minor_radius,
        ..
    } = surface
    else {
        panic!("torus geometry");
    };
    assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
    assert_eq!(major_radius, 20.0);
    assert_eq!(minor_radius, 5.0);
}

#[test]
fn standard_analytic_carriers_have_no_model_size_cutoff() {
    for (kind, values, expected_radius) in [
        (0x35, vec![0.0_f32, 0.0, 0.0, 2_000_000.0], 2_000_000.0),
        (
            0x33,
            vec![0.0_f32, 0.0, 0.0, 0.0, 0.0, 2_000_000.0],
            2_000_000.0,
        ),
    ] {
        let mut bytes = vec![0x00, 0x33, kind];
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        let surface = crate::families::standard::records::decode_curved(
            &bytes,
            &crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target: 0,
                kind,
            },
        )
        .expect("large analytic carrier");
        let (SurfaceGeometry::Sphere { radius, .. } | SurfaceGeometry::Cylinder { radius, .. }) =
            surface
        else {
            panic!("expected sphere or cylinder");
        };
        assert_eq!(radius, expected_radius);
    }

    let mut bytes = vec![0x00, 0x33, 0x38];
    for value in [0.0_f32, 0.0, 0.0, 0.0, 0.0, 2_000_000.0, 1_500_000.0] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    assert!(matches!(
        crate::families::standard::records::decode_curved(
            &bytes,
            &crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target: 0,
                kind: 0x38,
            },
        ),
        Some(SurfaceGeometry::Torus {
            major_radius: 2_000_000.0,
            minor_radius: 1_500_000.0,
            ..
        })
    ));
}

#[test]
fn standard_f32_frames_canonicalize_to_orthonormal_ir() {
    let component = (0.5_f64 + 4.0e-6).sqrt() as f32;
    let mut bytes = vec![0x00, 0x33, 0x33];
    for value in [0.0_f32, 0.0, 0.0, component, component, 5.0] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    let surface = crate::families::standard::records::decode_curved(
        &bytes,
        &crate::families::standard::records::SurfacePrefix {
            pos: 0,
            target: 0,
            kind: 0x33,
        },
    )
    .expect("near-unit cylinder carrier");
    let SurfaceGeometry::Cylinder {
        axis,
        ref_direction,
        ..
    } = surface
    else {
        panic!("cylinder geometry");
    };
    assert!((axis.norm() - 1.0).abs() < 1.0e-12);
    assert!((ref_direction.norm() - 1.0).abs() < 1.0e-12);
    assert!(axis.dot(ref_direction).abs() < 1.0e-12);

    let plane = crate::families::standard::records::decode_plane(
        &crate::families::standard::records::PlaneParams {
            target: 0,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 0.999_999),
        },
    )
    .expect("near-unit plane carrier");
    let SurfaceGeometry::Plane { normal, u_axis, .. } = plane else {
        panic!("plane geometry");
    };
    assert!((normal.norm() - 1.0).abs() < 1.0e-12);
    assert!((u_axis.norm() - 1.0).abs() < 1.0e-12);
    assert!(normal.dot(u_axis).abs() < 1.0e-12);
    assert!(crate::families::standard::records::decode_plane(
        &crate::families::standard::records::PlaneParams {
            target: 0,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 0.0),
        },
    )
    .is_none());
}

#[test]
fn standard_topology_recovers_a_quad_boundary_and_port_vertices() {
    let topology = crate::families::standard::fbb::parse_standard(&standard_quad_topology_stream())
        .expect("valid standard topology");

    assert_eq!(topology.face_count(), 1);
    assert_eq!(topology.edge_rows().len(), 4);
    assert_eq!(topology.vertex_points().len(), 4);
    let boundary = &topology.faces()[0].boundaries[0];
    assert_eq!(boundary.coedges.len(), 4);
    assert_eq!(
        boundary
            .coedges
            .iter()
            .map(|use_| use_.edge_row)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert!(boundary.coedges.iter().all(|use_| !use_.reversed));
    assert_eq!(topology.logical_vertex_count(), 4);
}

#[test]
fn compact_standard_topology_recovers_a_triangle_boundary_and_vertices() {
    let bytes = compact_standard_triangle_topology_stream();
    let topology = crate::families::standard::fbb::parse_standard(&bytes)
        .expect("valid compact standard topology");

    assert_eq!(topology.face_count(), 1);
    assert_eq!(topology.edge_rows().len(), 3);
    assert_eq!(topology.vertex_points().len(), 3);
    assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 3);
    assert_eq!(
        crate::families::standard::fbb::standard_edge_count(&bytes),
        Some(3)
    );
    assert_eq!(
        crate::families::standard::fbb::standard_vertex_points(&bytes)
            .expect("compact standard vertex table")
            .len(),
        3
    );
    assert_eq!(
        crate::solve::missing_edge::standard_mesh_edge_ports(&bytes)
            .expect("compact standard mesh port quotient"),
        vec![[0, 1], [1, 2], [2, 0]]
    );
}

#[test]
fn standard_counted_vertex_table_excludes_incidental_markers() {
    let mut bytes = standard_quad_topology_stream();
    bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
    bytes.extend_from_slice(&le_f32(10.0));
    bytes.extend_from_slice(&le_f32(20.0));
    bytes.extend_from_slice(&le_f32(30.0));

    assert_eq!(
        crate::families::standard::fbb::standard_vertex_points(&bytes)
            .expect("required invariant")
            .len(),
        4
    );
}

#[test]
fn standard_topology_accepts_delimiters_between_counted_edge_tables() {
    let mut bytes = standard_quad_topology_stream();
    let header = bytes
        .windows(3)
        .position(|window| window == [0x01, 0x01, 0x04])
        .expect("edge table header");
    bytes[header + 2] = 2;
    let second_table = header + 3 + 2 * 8;
    bytes.splice(
        second_table..second_table,
        [
            0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02,
        ],
    );

    let topology = crate::families::standard::fbb::parse_standard(&bytes).expect("two edge tables");
    assert_eq!(
        crate::families::standard::fbb::standard_edge_count(&bytes),
        Some(4)
    );
    assert_eq!(
        topology
            .edge_rows()
            .iter()
            .map(|row| row.kind)
            .collect::<Vec<_>>(),
        vec![1, 1, 2, 2]
    );
    assert_eq!(
        crate::solve::missing_edge::standard_edge_rows(&bytes)
            .expect("edge rows")
            .iter()
            .map(|row| row.kind)
            .collect::<Vec<_>>(),
        vec![1, 1, 2, 2]
    );
}

#[test]
fn fbb_topology_reads_u24_mesh_and_edge_handles() {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 10, 0, 0, 0, 10];
    for handle in [
        1u32, 0x01_0010, 0x01_0011, 0x01_0012, 0x01_0013, 0x01_0014, 0x01_0015, 0x01_0016,
        0x01_0017, 0x01_0010,
    ] {
        bytes.extend_from_slice(&handle.to_be_bytes()[1..]);
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    for (kind, rows) in [
        (
            1,
            [
                [0x01_0010u32, 0x01_0011],
                [0x01_0011, 0x01_0012],
                [0x01_0012, 0x01_0013],
                [0x01_0013, 0x01_0014],
            ],
        ),
        (
            2,
            [
                [0x01_0014u32, 0x01_0015],
                [0x01_0015, 0x01_0016],
                [0x01_0016, 0x01_0017],
                [0x01_0017, 0x01_0010],
            ],
        ),
    ] {
        bytes.extend_from_slice(&[0x01, kind, 4]);
        for row in rows {
            bytes.extend_from_slice(&[0x02, 2]);
            for handle in row {
                bytes.extend_from_slice(&handle.to_be_bytes()[1..]);
            }
        }
        bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 4]);
    for index in 0..4 {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [index as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let topology =
        crate::families::standard::topology::parse_fbb(&bytes).expect("valid FBB topology");
    assert_eq!(topology.edge_rows()[0].handles, vec![0x01_0010, 0x01_0011]);
    assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 8);
    assert_eq!(topology.logical_vertex_count(), 8);
    assert_eq!(topology.vertex_points().len(), 4);
    let table_ports = crate::solve::missing_edge::fbb_global_edge_port_identities(&bytes)
        .expect("global FBB handle ports");
    assert_eq!(table_ports[0][1], table_ports[1][0]);
    assert_eq!(table_ports[1][1], table_ports[2][0]);
    assert_eq!(table_ports[2][1], table_ports[3][0]);
    assert_eq!(table_ports[3][1], table_ports[4][0]);
    assert_eq!(table_ports[4][1], table_ports[5][0]);
    assert_eq!(table_ports[5][1], table_ports[6][0]);
    assert_eq!(table_ports[6][1], table_ports[7][0]);
    let native_ports = [
        [100, 101],
        [101, 102],
        [102, 103],
        [103, 100],
        [100, 101],
        [101, 102],
        [102, 103],
        [103, 100],
    ];
    let quotient =
        crate::families::standard::topology::parse_fbb_with_native_vertices(&bytes, &native_ports)
            .expect("native endpoint quotient");
    assert_eq!(quotient.logical_vertex_count(), 4);
    assert_eq!(
        quotient.edge_vertices().expect("edge vertices"),
        native_ports.map(|pair| pair
            .map(|identity| usize::try_from(identity - 100).expect("required invariant")))
    );
    assert_eq!(
        quotient
            .bind_vertex_points(&[
                [0, 1],
                [1, 2],
                [2, 3],
                [3, 0],
                [0, 1],
                [1, 2],
                [2, 3],
                [3, 0],
            ])
            .expect("coordinate binding"),
        vec![0, 1, 2, 3]
    );
    let runs = crate::solve::missing_edge::standard_mesh_edge_runs(&bytes).expect("u24 edge runs");
    assert_eq!(runs.len(), 8);
    assert!(runs.iter().all(|run| run.segment_count == 1));
}

#[test]
fn fbb_only_topology_uses_complete_boundary_runs_and_scoped_ports() {
    let bytes = fbb_only_quad_topology_stream();
    assert_eq!(
        crate::families::standard::fbb::fbb_only_edge_count(&bytes),
        Some(4)
    );
    assert_eq!(
        crate::families::standard::fbb::fbb_only_vertex_points(&bytes)
            .expect("counted FBB-only vertices")
            .len(),
        4
    );
    let topology =
        crate::families::standard::topology::parse_fbb(&bytes).expect("valid FBB-only topology");
    assert_eq!(topology.face_count(), 1);
    assert_eq!(topology.edge_rows().len(), 4);
    assert!(topology.edge_rows().iter().all(|row| {
        row.boundary_layout
            == crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun
    }));
    let ports = crate::solve::missing_edge::standard_mesh_edge_ports(&bytes)
        .expect("FBB-only mesh port quotient");
    assert_eq!(ports, vec![[0, 1], [1, 2], [2, 3], [3, 0]]);
    let topology =
        crate::families::standard::topology::parse_fbb_with_native_vertices(&bytes, &ports)
            .expect("FBB-only native endpoint quotient");
    assert_eq!(topology.logical_vertex_count(), 4);
    assert_eq!(
        topology.edge_vertices().expect("FBB-only edge endpoints"),
        ports
            .into_iter()
            .map(|pair| pair.map(|identity| identity as usize))
            .collect::<Vec<_>>()
    );
}

#[test]
fn fbb_topology_recovers_unique_flanking_rows_without_reclassifying_complete_rows() {
    let bytes = fbb_mixed_boundary_topology_stream();
    let topology = crate::families::standard::topology::parse_fbb(&bytes)
        .expect("mixed FBB boundary topology");
    assert_eq!(topology.face_count(), 1);
    assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 5);
    assert_eq!(
        topology.edge_rows()[0].boundary_layout,
        crate::families::standard::topology::EdgeBoundaryLayout::InteriorWithFlankingCorners
    );
    assert!(topology.edge_rows()[1..].iter().all(|row| {
        row.boundary_layout
            == crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun
    }));
}

#[test]
fn fbb_topology_reads_u16_mesh_and_edge_handles() {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 6, 0, 0, 0, 6];
    for handle in [1u16, 0x1010, 0x1011, 0x1012, 0x1013, 0x1010] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    for (kind, rows) in [
        (1, [[0x1010u16, 0x1011], [0x1011, 0x1012]]),
        (2, [[0x1012u16, 0x1013], [0x1013, 0x1010]]),
    ] {
        bytes.extend_from_slice(&[0x01, kind, 2]);
        for row in rows {
            bytes.extend_from_slice(&[0x02, 2]);
            for handle in row {
                bytes.extend_from_slice(&handle.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&[0x10, 0xa4, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 4]);
    for index in 0..4 {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [index as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let topology =
        crate::families::standard::topology::parse_fbb(&bytes).expect("valid u16 FBB topology");
    assert_eq!(topology.edge_rows()[0].handles, vec![0x1010, 0x1011]);
    assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 4);
    assert_eq!(topology.vertex_points().len(), 4);
}

#[test]
fn fbb_topology_reads_u8_mesh_and_edge_handles() {
    let mut bytes = vec![0x01, 0x49, 0x02, 0xff, 6, 0, 0, 0];
    for value in [0.0f32, 0.0, 1.0] {
        bytes.extend_from_slice(&le_f32(value));
    }
    bytes.extend_from_slice(&[0x10, 0x11, 0x12, 0x10, 0x12, 0x13]);
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    for (kind, rows) in [
        (1, [[0x10u8, 0x11], [0x11, 0x12]]),
        (2, [[0x12u8, 0x13], [0x13, 0x10]]),
    ] {
        bytes.extend_from_slice(&[0x01, kind, 2]);
        for row in rows {
            bytes.extend_from_slice(&[0x02, 2]);
            bytes.extend_from_slice(&row);
        }
        bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 4]);
    for index in 0..4 {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [index as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let topology =
        crate::families::standard::topology::parse_fbb(&bytes).expect("valid u8 FBB topology");
    assert_eq!(topology.edge_rows()[0].handles, vec![0x10, 0x11]);
    assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 4);
    assert_eq!(topology.vertex_points().len(), 4);
    assert_eq!(
        crate::families::standard::fbb::standard_face_frame_vectors(&bytes, 1),
        [Some([0.0, 0.0, 1.0])]
    );
}

#[test]
fn fbb_frame_vectors_concatenate_unique_population_chains() {
    let mut bytes = vec![0x01, 0x49, 0x02, 0xff, 6, 0, 0, 0];
    for value in [0.0f32, 0.0, 1.0] {
        bytes.extend_from_slice(&le_f32(value));
    }
    for handle in [0x10u16, 0x11, 0x12] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x49, 0x01, 0xff, 3, 0, 0, 0]);
    for value in [0.0f32, 0.0, 1.0] {
        bytes.extend_from_slice(&le_f32(value));
    }
    bytes.extend_from_slice(&[0x20, 0x21, 0x22]);
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd3, 0xd3, 0xd3, 0xd3]);

    assert_eq!(
        crate::families::standard::fbb::standard_face_frame_vectors(&bytes, 2),
        [Some([0.0, 0.0, 1.0]), Some([0.0, 0.0, 1.0]),]
    );
}

#[test]
fn fbb_topology_requires_one_u16_delimiter_family() {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 6, 0, 0, 0, 6];
    for handle in [1u16, 0x1010, 0x1011, 0x1012, 0x1013, 0x1010] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    for (kind, family, rows) in [
        (1, 0x94, [[0x1010u16, 0x1011], [0x1011, 0x1012]]),
        (2, 0xa4, [[0x1012u16, 0x1013], [0x1013, 0x1010]]),
    ] {
        bytes.extend_from_slice(&[0x01, kind, 2]);
        for row in rows {
            bytes.extend_from_slice(&[0x02, 2]);
            for handle in row {
                bytes.extend_from_slice(&handle.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&[0x10, family, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0]);

    assert!(crate::families::standard::topology::parse_fbb(&bytes).is_none());
}

#[test]
fn standard_vertex_roster_preserves_native_identity_order() {
    let mut bytes = vec![0x54, 0x02, 0x00, 0x00, 0, 0, 0, 0xff];
    for identity in [0x01_0203u32, 0x01_0206, 0x01_0209] {
        bytes.push(0x54);
        bytes.extend_from_slice(&identity.to_le_bytes()[..3]);
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.extend_from_slice(&[0x54, 0x01, 0x00, 0x00, 0, 0, 0]);

    assert_eq!(
        crate::families::standard::records::standard_vertex_roster(&bytes, 3),
        Some(vec![0x01_0203, 0x01_0206, 0x01_0209])
    );
}

#[test]
fn standard_topology_matches_edge_interiors_and_collapses_endpoint_ports() {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 11, 0, 0, 0, 11];
    for handle in [1u16, 10, 11, 12, 13, 14, 15, 16, 17, 18, 10] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x01, 3]);
    for row in [
        [101u16, 12, 11, 100],
        [101, 14, 15, 102],
        [102, 17, 18, 100],
    ] {
        bytes.extend_from_slice(&[0x02, 4]);
        for handle in row {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 3]);
    for index in 0..3 {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [index as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let topology =
        crate::families::standard::fbb::parse_standard(&bytes).expect("interior-run topology");
    let coedges = &topology.faces()[0].boundaries[0].coedges;
    assert_eq!(
        coedges.iter().map(|use_| use_.edge_row).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(coedges[0].reversed);
    assert_eq!(topology.logical_vertex_count(), 3);
}

#[test]
fn standard_legacy_two_strip_packet_recovers_two_face_boundaries() {
    let mut bytes = vec![0x01, 0x42, 0x02, 0xff, 12, 0, 0, 0, 6, 6];
    for handle in [10u16, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 25] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x01, 6]);
    for row in [
        [100u16, 11, 101],
        [101, 15, 102],
        [102, 12, 100],
        [200, 21, 201],
        [201, 25, 202],
        [202, 22, 200],
    ] {
        bytes.extend_from_slice(&[0x02, 3]);
        for handle in row {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 6]);
    for index in 0..6 {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [index as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let topology =
        crate::families::standard::fbb::parse_standard(&bytes).expect("legacy B=2 packet");
    assert_eq!(topology.faces()[0].boundaries.len(), 2);
    assert!(topology.faces()[0]
        .boundaries
        .iter()
        .all(|boundary| boundary.coedges.len() == 3));
    assert_eq!(topology.logical_vertex_count(), 6);
}

#[test]
fn standard_two_strip_packet_uses_raw_lengths_at_three_byte_width() {
    let mut bytes = vec![0x01, 0x42, 0x02, 0xff, 6, 0, 0, 0, 3, 3];
    let handles = [
        0x01_0203u32,
        0x02_0304,
        0x03_0405,
        0x04_0506,
        0x05_0607,
        0x06_0708,
    ];
    for handle in handles {
        let encoded = handle.to_be_bytes();
        bytes.extend_from_slice(&encoded[1..]);
    }

    let layout = crate::families::standard::fbb::parse_trim_record_layout(&bytes, 0, 3)
        .expect("three-byte packet layout");
    assert_eq!(layout.handle_offset, 8);
    assert_eq!(layout.stored_count, handles.len());
    assert_eq!(layout.end, bytes.len());

    let record =
        crate::families::standard::fbb::parse_trim_record(&bytes, 0, 3).expect("three-byte packet");
    assert_eq!(record.handles, handles);
    assert_eq!(record.strip_lengths, [3, 3]);
    assert!(record.fan_lengths.is_empty());
}

#[test]
fn standard_two_strip_packet_treats_ff_length_as_raw_u8_at_three_byte_width() {
    let handle_count = 256u32;
    let mut bytes = vec![0x01, 0x42, 0x02, 0xff];
    bytes.extend_from_slice(&handle_count.to_le_bytes());
    bytes.extend_from_slice(&[0xff, 0x01]);
    for handle in 0..handle_count {
        let encoded = handle.to_be_bytes();
        bytes.extend_from_slice(&encoded[1..]);
    }

    let record = crate::families::standard::fbb::parse_trim_record(&bytes, 0, 3)
        .expect("raw 0xff strip length");
    assert_eq!(record.handles.len(), handle_count as usize);
    assert_eq!(record.strip_lengths, [255, 1]);
    assert!(record.fan_lengths.is_empty());
}

#[test]
fn standard_curve_support_table_recovers_leading_spline_and_widened_faces() {
    let mut bytes = vec![0x60, 1, 2, 3, 0, 0, 0, 0xff];
    bytes.extend_from_slice(&260u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0x60, 4, 5, 6, 0, 2, 0, 0x33, 0x36, 0xff]);
    bytes.extend_from_slice(&260u32.to_le_bytes());
    bytes.push(2);

    let rows = crate::families::standard::records::standard_curve_supports(&bytes, 300, Some(2));
    assert_eq!(rows.len(), 2);
    assert!(matches!(
        rows[0].geometry,
        crate::families::standard::records::StandardCurveGeometry::Bspline
    ));
    assert!(matches!(
        rows[1].geometry,
        crate::families::standard::records::StandardCurveGeometry::Line
    ));
    assert_eq!(rows[0].faces, [260, 1]);
    assert_eq!(rows[1].faces, [260, 2]);
}

#[test]
fn standard_curve_support_fallback_requires_one_complete_edge_run() {
    let line_row = |tag: u32, faces: [u8; 2]| {
        let tag = tag.to_le_bytes();
        vec![
            0x60, tag[0], tag[1], tag[2], 0x00, 0x02, 0x00, 0x33, 0x36, faces[0], faces[1],
        ]
    };

    let mut longer_run = line_row(1, [0, 1]);
    longer_run.extend(line_row(2, [0, 1]));
    assert!(
        crate::families::standard::records::standard_curve_supports(&longer_run, 2, Some(1))
            .is_empty()
    );

    let mut separate_runs = line_row(3, [0, 1]);
    separate_runs.push(0xa5);
    separate_runs.extend(line_row(4, [0, 1]));
    assert!(crate::families::standard::records::standard_curve_supports(
        &separate_runs,
        2,
        Some(1),
    )
    .is_empty());

    let unique_run = line_row(5, [0, 1]);
    let rows = crate::families::standard::records::standard_curve_supports(&unique_run, 2, None);
    assert_eq!(rows.len(), 1);
}

#[test]
fn topology_binds_logical_vertices_from_exact_edge_endpoint_pairs() {
    let topology = crate::families::standard::fbb::parse_standard(&standard_quad_topology_stream())
        .expect("quad topology");
    let assignment = topology
        .bind_vertex_points(&[[0, 1], [1, 2], [2, 3], [3, 0]])
        .expect("unique point assignment");

    assert_eq!(assignment, vec![0, 1, 2, 3]);
}

#[test]
fn standard_circle_parser_rejects_non_support_marker() {
    let mut bytes = vec![0x61, 0, 0, 0, 0, 0x12, 0, 0x33, 0x37];
    bytes.extend_from_slice(&[0; 18]);
    assert!(crate::families::standard::records::standard_circles(&bytes, 1, Some(1)).is_empty());
}

#[test]
fn standard_circle_parser_has_no_model_size_cutoff() {
    let mut bytes = vec![0x60, 1, 2, 3, 0x00, 0x12, 0x00, 0x33, 0x37];
    for value in [0.0_f32, 0.0, 0.0, 2_000_000.0] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.extend_from_slice(&[0, 1]);
    assert_eq!(
        crate::families::standard::records::standard_circles(&bytes, 2, Some(1))[0].radius,
        2_000_000.0
    );
}

#[test]
fn standard_surface_roster_walks_freeform_and_analytic_records() {
    use crate::families::standard::records::StandardSurfaceRecord;

    let mut bytes = vec![0x34, 0x12, 0, 0, 0, 0];
    for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
        bytes.extend_from_slice(&le_f32(value));
    }
    bytes.push(0x01);
    let analytic = bytes.len();
    bytes.extend_from_slice(&[0x78, 0x56, 0, 0, 0x1a, 0, 0x33, 0x33]);
    bytes.resize(analytic + 72, 0);
    bytes.push(0xff);
    bytes.extend_from_slice(&[0x60, 1, 0, 0, 0x00, 0x02, 0x00, 0x33, 0x36, 0, 1]);

    let records = crate::families::standard::records::standard_surface_records(&bytes, 2)
        .expect("surface roster");
    assert!(matches!(
        records[0],
        StandardSurfaceRecord::Freeform {
            pos: 0,
            tag: 0x1234,
            forward: true,
            ..
        }
    ));
    let StandardSurfaceRecord::Freeform { bounds, .. } = &records[0] else {
        unreachable!("freeform roster row")
    };
    assert_eq!(bounds.aabb_center, [0.0, 0.0, 0.0]);
    assert_eq!(bounds.aabb_half_extents, [1.0, 1.0, 1.0]);
    assert_eq!(bounds.sphere_center, [0.0, 0.0, 0.0]);
    assert_eq!(bounds.sphere_radius, 2.0);
    assert!(matches!(
        &records[1],
        StandardSurfaceRecord::Analytic(prefix)
            if prefix.pos == analytic + 5 && prefix.target == 0x5678 && prefix.kind == 0x33
    ));
    assert_eq!(
        crate::families::standard::records::standard_surface_record_groups(&bytes).len(),
        1
    );
    let populations = crate::families::standard::records::standard_surface_populations(&bytes);
    assert_eq!(populations.len(), 1);
    assert_eq!(populations[0].records.len(), 2);
    assert_eq!(populations[0].supports.len(), 1);
    assert_eq!(populations[0].supports[0].faces, [0, 1]);
}

#[test]
fn source_order_pairs_only_source_closed_populations_with_matching_cardinalities() {
    let population = |faces: usize, edges: usize, seed: usize| StandardSurfacePopulation {
        records: (0..faces)
            .map(|offset| {
                StandardSurfaceRecord::Analytic(SurfacePrefix {
                    pos: seed + offset,
                    target: u32::try_from(seed + offset).expect("small test target"),
                    kind: 0x33,
                })
            })
            .collect(),
        supports: (0..edges)
            .map(|offset| StandardCurveSupport {
                pos: seed + offset,
                tag: u32::try_from(seed + offset).expect("small test tag"),
                faces: [0, faces.saturating_sub(1)],
                geometry: StandardCurveGeometry::Line,
            })
            .collect(),
    };
    let layout = |face_count: usize, edge_count: usize, vertex_count: usize, start: usize| {
        FbbPopulationLayout {
            face_start: start,
            face_count,
            after_faces: start + face_count * 8,
            edge_count,
            vertex_count,
            fbb_edge_table: true,
        }
    };
    let layouts = vec![layout(2, 3, 4, 10), layout(2, 3, 4, 20)];
    let populations = vec![population(2, 3, 100), population(2, 3, 200)];

    let pairs =
        crate::families::standard::records::pair_standard_populations(&layouts, &populations)
            .expect("source-ordered population relation");
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].0.face_start, 10);
    assert!(matches!(
        &pairs[0].1.records[0],
        StandardSurfaceRecord::Analytic(prefix) if prefix.pos == 100
    ));
    assert_eq!(pairs[1].0.face_start, 20);
    assert!(matches!(
        &pairs[1].1.records[0],
        StandardSurfaceRecord::Analytic(prefix) if prefix.pos == 200
    ));

    let mut mismatched = populations.clone();
    mismatched[1] = population(2, 2, 200);
    assert!(
        crate::families::standard::records::pair_standard_populations(&layouts, &mismatched,)
            .is_none()
    );
    assert!(
        crate::families::standard::records::pair_standard_populations(&layouts[..1], &populations,)
            .is_none()
    );
}

#[test]
fn standard_surface_roster_rejects_payload_freeform_collisions() {
    let analytic_record = |tag: [u8; 3]| {
        let mut record = vec![tag[0], tag[1], tag[2], 0, 0x1a, 0, 0x33, 0x33];
        record.resize(73, 0);
        record[72] = 0xff;
        record
    };
    let mut bytes = analytic_record([0x78, 0x56, 0]);
    bytes.extend(analytic_record([0x79, 0x56, 0]));

    let mut colliding_freeform = vec![0x9a, 0x78, 0x56, 0, 0, 0];
    for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
        colliding_freeform.extend_from_slice(&le_f32(value));
    }
    colliding_freeform.push(0x01);
    assert_eq!(colliding_freeform.len(), 47);
    bytes[26..73].copy_from_slice(&colliding_freeform);
    bytes.push(0x60);

    let records = crate::families::standard::records::standard_surface_records(&bytes, 2)
        .expect("surface roster");
    assert_eq!(records.len(), 2);
    assert!(matches!(
        &records[0],
        StandardSurfaceRecord::Analytic(prefix) if prefix.target == 0x5678
    ));
    assert!(matches!(
        &records[1],
        StandardSurfaceRecord::Analytic(prefix) if prefix.target == 0x5679
    ));
}

#[test]
fn standard_surface_roster_rejects_multiple_complete_chains() {
    let analytic_record = |tag: [u8; 3]| {
        let mut record = vec![tag[0], tag[1], tag[2], 0, 0x1a, 0, 0x33, 0x33];
        record.resize(73, 0);
        record[72] = 0xff;
        record
    };
    let mut bytes = analytic_record([0x78, 0x56, 0]);
    bytes.extend(analytic_record([0x79, 0x56, 0]));
    bytes.push(0x60);
    bytes.extend(analytic_record([0x7a, 0x56, 0]));
    bytes.extend(analytic_record([0x7b, 0x56, 0]));
    bytes.push(0x60);

    assert!(crate::families::standard::records::standard_surface_records(&bytes, 2).is_none());
}

#[test]
fn standard_surface_roster_rejects_zero_faces() {
    assert!(crate::families::standard::records::standard_surface_records(&[0x60], 0).is_none());
}

#[test]
fn standard_surface_roster_does_not_stop_at_a_record_tag_byte() {
    let analytic_record = |tag: [u8; 3]| {
        let mut record = vec![tag[0], tag[1], tag[2], 0, 0x1a, 0, 0x33, 0x33];
        record.resize(73, 0);
        record[72] = 0xff;
        record
    };
    let mut bytes = analytic_record([0x78, 0x56, 0]);
    bytes.extend(analytic_record([0x60, 0x56, 0]));
    bytes.push(0x60);

    let records = crate::families::standard::records::standard_surface_records(&bytes, 2)
        .expect("two-record roster");
    assert_eq!(records.len(), 2);
}

#[test]
fn plane_bounds_bind_normals_by_persistent_carrier_tag() {
    let mut bytes =
        plane_bounds_record(0x0001_0203, [1.0, 2.0, 3.0], [1.0; 3], [1.0, 2.0, 3.0], 4.0);
    bytes.extend(plane_bounds_record(
        0x0004_0506,
        [4.0, 5.0, 6.0],
        [1.0; 3],
        [4.0, 5.0, 6.0],
        4.0,
    ));
    bytes.extend(plane_bounds_record(
        0x0007_0809,
        [0.0, 0.0, 50.0],
        [2.5, 2.5, 0.0],
        [0.0, 0.0, 50.0],
        2.5,
    ));
    let normals = HashMap::from([
        (0x0004_0506, [0.0, 1.0, 0.0]),
        (0x0001_0203, [1.0, 0.0, 0.0]),
        (0x0007_0809, [0.0, 0.0, 1.0]),
    ]);
    let planes = crate::families::standard::records::plane_params(&bytes, &normals);

    assert_eq!(planes.len(), 3);
    assert_eq!(planes[0].target, 0x0001_0203);
    assert_eq!(planes[0].normal, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(planes[1].target, 0x0004_0506);
    assert_eq!(planes[1].normal, Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(planes[2].target, 0x0007_0809);
    assert_eq!(planes[2].origin, Point3::new(0.0, 0.0, 50.0));
}

fn plane_bounds_record(
    tag: u32,
    center: [f32; 3],
    half: [f32; 3],
    sphere: [f32; 3],
    radius: f32,
) -> Vec<u8> {
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&tag.to_le_bytes()[..3]);
    bytes.extend_from_slice(&[0x00, 0x02, 0x00, 0x33, 0x32]);
    for value in [
        center[0], center[1], center[2], half[0], half[1], half[2], sphere[0], sphere[1],
        sphere[2], radius,
    ] {
        bytes.extend_from_slice(&le_f32(value));
    }
    bytes
}

#[test]
fn plane_bounds_withhold_duplicates_and_excessive_containment_error() {
    let duplicate_tag = 0x0001_0203;
    let invalid_tag = 0x0004_0506;
    let valid_tag = 0x0007_0809;
    let rounded_tag = 0x000a_0b0c;
    let mut bytes = plane_bounds_record(
        duplicate_tag,
        [1.0, 2.0, 3.0],
        [1.0; 3],
        [1.0, 2.0, 3.0],
        4.0,
    );
    bytes.extend(plane_bounds_record(
        duplicate_tag,
        [9.0, 9.0, 9.0],
        [1.0; 3],
        [9.0, 9.0, 9.0],
        4.0,
    ));
    bytes.extend(plane_bounds_record(
        invalid_tag,
        [0.0, 0.0, 0.0],
        [1.0; 3],
        [5.2e-7, 0.0, 0.0],
        1.0,
    ));
    bytes.extend(plane_bounds_record(
        valid_tag,
        [0.0, 0.0, 5.0],
        [1.0; 3],
        [0.0, 0.0, 5.0],
        2.0,
    ));
    bytes.extend(plane_bounds_record(
        rounded_tag,
        [0.0, 0.0, 50.0],
        [2.5, 2.5, 0.0],
        [5.210_867_5e-7, 1.554_135_7e-7, 50.0],
        2.5,
    ));
    let normals = HashMap::from([
        (duplicate_tag, [1.0, 0.0, 0.0]),
        (invalid_tag, [0.0, 1.0, 0.0]),
        (valid_tag, [0.0, 0.0, 1.0]),
        (rounded_tag, [0.0, 0.0, 1.0]),
    ]);

    let planes = crate::families::standard::records::plane_params(&bytes, &normals);

    assert_eq!(planes.len(), 2);
    assert_eq!(planes[0].target, valid_tag);
    assert_eq!(planes[0].origin, Point3::new(0.0, 0.0, 5.0));
    assert_eq!(planes[1].target, rounded_tag);
    assert_eq!(
        planes[1].origin,
        Point3::new(
            f64::from(5.210_867_5e-7_f32),
            f64::from(1.554_135_7e-7_f32),
            50.0,
        )
    );
}

#[test]
fn analytic_surface_records_retain_trimmed_face_bounds_after_their_parameters() {
    for (kind, relative) in [(0x32, 3), (0x33, 27), (0x34, 27), (0x35, 19), (0x38, 31)] {
        let mut bytes = vec![0; 96];
        for (index, value) in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 1.0, 2.0, 3.0, 8.0]
            .into_iter()
            .enumerate()
        {
            let at = 5 + relative + 4 * index;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let record =
            StandardSurfaceRecord::Analytic(crate::families::standard::records::SurfacePrefix {
                pos: 5,
                target: 1,
                kind,
            });
        assert_eq!(
            crate::families::standard::records::standard_face_bounds(&bytes, &record),
            Some(StandardFaceBounds {
                aabb_center: [1.0, 2.0, 3.0],
                aabb_half_extents: [4.0, 5.0, 6.0],
                sphere_center: [1.0, 2.0, 3.0],
                sphere_radius: 8.0,
            })
        );
    }
    assert_eq!(
        crate::families::standard::records::standard_face_bounds(
            &[0; 96],
            &StandardSurfaceRecord::Analytic(crate::families::standard::records::SurfacePrefix {
                pos: 5,
                target: 1,
                kind: 0x34,
            },),
        ),
        None
    );
}

#[test]
fn standard_face_witness_requires_an_analytic_marker() {
    let mut record = vec![0u8; 48];
    record[5..8].copy_from_slice(&[0x00, 0x33, 0x33]);
    for (index, value) in [1.0f32, 2.0, 3.0].into_iter().enumerate() {
        record[32 + index * 4..36 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        crate::families::standard::records::standard_face_witness(&record, 5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
    );
    record[6] = 0x32;
    assert!(crate::families::standard::records::standard_face_witness(&record, 5).is_none());
}

#[test]
fn standard_curve_supports_begin_after_the_surface_roster() {
    let mut bytes = vec![
        0x60, 1, 0, 0, 0, 2, 0, 0x33, 0x36, 0, 0, // earlier valid-looking rows
    ];
    bytes.extend_from_slice(&[0x60, 3, 0, 0, 0, 2, 0, 0x33, 0x36, 0, 0]);
    bytes.extend_from_slice(&[0x34, 0x12, 0, 0, 0, 0]);
    for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
        bytes.extend_from_slice(&le_f32(value));
    }
    bytes.push(0x01);
    bytes.extend_from_slice(&[
        0x60, 2, 0, 0, 0, 2, 0, 0x33, 0x36, 0, 0, // roster-adjacent row
    ]);

    let rows = crate::families::standard::records::standard_curve_supports(&bytes, 1, Some(1));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tag, 2);
    assert!(
        crate::families::standard::records::standard_curve_supports(&bytes, 1, Some(2)).is_empty()
    );
}

#[test]
fn standard_freeform_tag_resolves_direct_and_face_carriers() {
    let mut stream = b5_closed_triangle_stream();
    let vertex_start = stream
        .windows(3)
        .position(|bytes| bytes == [0x05, 0x08, 0x01])
        .expect("required invariant");
    let mut unresolved_face = Vec::new();
    append_b5_record(
        &mut unresolved_face,
        0x5f,
        501,
        &[0x82, 0x18, 100, 0, 0x18, 231, 3, 0x05],
    );
    stream.splice(vertex_start..vertex_start, unresolved_face);
    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [stream],
        &HashSet::from([100, 501]),
        &HashSet::new(),
    );
    assert!(matches!(
        evidence.surface_geometries.get(&100),
        Some(SurfaceGeometry::Plane { .. })
    ));
    assert!(matches!(
        evidence.surface_geometries.get(&501),
        Some(SurfaceGeometry::Plane { .. })
    ));
}

#[test]
fn standard_freeform_tag_resolves_standalone_a8_carrier() {
    let mut stream = a8_surface_stream();
    stream[7..11].copy_from_slice(&100u32.to_le_bytes());
    append_b5_record(&mut stream, 0x2e, 501, &[0x18, 100, 0]);
    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [stream],
        &HashSet::from([100, 501]),
        &HashSet::new(),
    );
    for tag in [100, 501] {
        assert!(matches!(
            evidence.surface_geometries.get(&tag),
            Some(SurfaceGeometry::Nurbs(surface))
                if surface.u_degree() == 2
                    && surface.v_degree() == 2
                    && surface.u_count() == 3
                    && surface.v_count() == 3
        ));
    }
}

#[test]
fn standard_freeform_tag_rejects_conflicting_standalone_a8_carriers() {
    let mut first = a8_surface_stream();
    first[7..11].copy_from_slice(&100u32.to_le_bytes());
    let mut second = first.clone();
    second[67..75].copy_from_slice(&1.0f64.to_le_bytes());
    first.extend(second);

    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [first],
        &HashSet::from([100]),
        &HashSet::new(),
    );
    assert!(!evidence.surface_geometries.contains_key(&100));
}

#[test]
fn standard_freeform_tag_collapses_repeated_standalone_a8_carrier() {
    let mut stream = a8_surface_stream();
    stream[7..11].copy_from_slice(&100u32.to_le_bytes());
    stream.extend(stream.clone());

    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [stream],
        &HashSet::from([100]),
        &HashSet::new(),
    );
    assert!(matches!(
        evidence.surface_geometries.get(&100),
        Some(SurfaceGeometry::Nurbs(_))
    ));
}

#[test]
fn standard_freeform_tag_resolves_standalone_a8_rolling_ball() {
    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [a8_freeform_curve_stream()],
        &HashSet::from([0x1234_5678]),
        &HashSet::new(),
    );
    assert!(matches!(
        evidence.procedural_surfaces.get(&0x1234_5678),
        Some(
            crate::families::standard::decode::StandardSurfaceProcedure::RollingBall {
                carrier_object_id: 0x1234_5678,
                definition: cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
                    degree: 5,
                    ..
                },
                source:
                    crate::families::standard::decode::StandardRollingBallSource::ObjectStreamA8,
            }
        )
    ));
}

#[test]
fn standard_object_evidence_rejects_cross_stream_edge_owner_conflicts() {
    let first = b5_closed_triangle_stream();
    let mut second = first.clone();
    let face = second
        .windows(3)
        .position(|bytes| bytes == [0xb5, 0x03, 0x5f])
        .expect("face record");
    second[face + 4..face + 8].copy_from_slice(&501u32.to_le_bytes());

    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [first, second],
        &HashSet::new(),
        &HashSet::new(),
    );
    assert!(evidence.edge_owner_faces.is_empty());
}

#[test]
fn standard_object_evidence_keeps_face_owner_from_unresolved_surface() {
    let mut stream = b5_closed_triangle_stream();
    let face = crate::families::b5::graph::object_stream_frames(&stream)
        .into_iter()
        .find(|frame| frame.class == 0x5f)
        .expect("face frame")
        .start;
    stream[face + 10..face + 12].copy_from_slice(&999u16.to_le_bytes());

    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [stream],
        &HashSet::new(),
        &HashSet::new(),
    );

    assert_eq!(
        evidence.edge_owner_faces.get(&300),
        Some(&HashSet::from([500]))
    );
}

#[test]
fn standard_object_evidence_rejects_repeated_topology_namespaces() {
    let stream = b5_closed_triangle_stream();
    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [stream.clone(), stream],
        &HashSet::new(),
        &HashSet::new(),
    );

    assert!(evidence.edge_owner_faces.is_empty());
    assert!(evidence.surface_geometries.is_empty());
}

#[test]
fn standard_object_evidence_does_not_join_topology_across_runs() {
    let mut stream = b5_closed_triangle_stream();
    let loop_start = crate::families::b5::graph::object_stream_frames(&stream)
        .into_iter()
        .find(|frame| frame.class == 0x62)
        .expect("loop frame")
        .start;
    stream.insert(loop_start, 0xff);

    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [stream],
        &HashSet::new(),
        &HashSet::new(),
    );
    assert!(evidence.edge_owner_faces.is_empty());
}

#[test]
fn standard_face_resolves_a_rolling_ball_result_carrier() {
    let mut stream = b5_closed_triangle_stream();
    let vertex_start = stream
        .windows(3)
        .position(|bytes| bytes == [0x05, 0x08, 0x01])
        .expect("vertex run");
    let mut records = a8_freeform_curve_stream();
    records[7..11].copy_from_slice(&110u32.to_le_bytes());
    let mut offset = vec![0x82, 0xee, 0xef];
    offset.extend_from_slice(&le_f64(-0.5));
    offset.push(0x19);
    for bound in [-2.0, 3.0, -4.0, 5.0] {
        offset.extend_from_slice(&le_f64(bound));
    }
    append_b5_record(&mut records, 0x30, 102, &offset);
    append_b5_record(&mut records, 0x5f, 501, &[0x82, 0xe6, 0x18, 231, 3, 0x05]);
    stream.splice(vertex_start..vertex_start, records);

    let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
        [stream],
        &HashSet::from([501]),
        &HashSet::new(),
    );
    assert!(!evidence.surface_geometries.contains_key(&501));
    assert!(matches!(
        evidence.procedural_surfaces.get(&501),
        Some(
            crate::families::standard::decode::StandardSurfaceProcedure::RollingBall {
                carrier_object_id: 110,
                definition: cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet { .. },
                source:
                    crate::families::standard::decode::StandardRollingBallSource::ObjectStreamA8,
            }
        )
    ));
}

#[test]
fn standard_duplicate_edge_face_uses_object_stream_owner_identity() {
    use crate::families::standard::records::{
        StandardCurveGeometry, StandardCurveSupport, StandardSurfaceRecord,
    };

    let mut edge_faces = vec![[0, 0]];
    let supports = vec![StandardCurveSupport {
        pos: 0,
        tag: 700,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    }];
    let records = [10u32, 20]
        .into_iter()
        .map(|target| {
            StandardSurfaceRecord::Analytic(crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target,
                kind: 0x33,
            })
        })
        .collect::<Vec<_>>();
    crate::families::standard::decode::apply_standard_native_edge_faces(
        &mut edge_faces,
        &supports,
        &records,
        &HashMap::from([(700, HashSet::from([20, 900]))]),
    );
    assert_eq!(edge_faces, [[0, 1]]);

    let mut ambiguous = vec![[0, 0]];
    let mut repeated_records = records;
    repeated_records.push(StandardSurfaceRecord::Analytic(
        crate::families::standard::records::SurfacePrefix {
            pos: 0,
            target: 20,
            kind: 0x33,
        },
    ));
    crate::families::standard::decode::apply_standard_native_edge_faces(
        &mut ambiguous,
        &supports,
        &repeated_records,
        &HashMap::from([(700, HashSet::from([20]))]),
    );
    assert_eq!(ambiguous, [[0, 0]]);
}

#[test]
fn standard_duplicate_edge_face_keeps_second_slot_open_for_one_owner_occurrence() {
    use crate::families::standard::records::{
        StandardCurveGeometry, StandardCurveSupport, StandardSurfaceRecord,
    };

    let mut edge_faces = vec![[0, 0]];
    let supports = [StandardCurveSupport {
        pos: 0,
        tag: 700,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    }];
    let records = [10u32, 20]
        .into_iter()
        .map(|target| {
            StandardSurfaceRecord::Analytic(crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target,
                kind: 0x33,
            })
        })
        .collect::<Vec<_>>();

    crate::families::standard::decode::apply_standard_native_edge_faces(
        &mut edge_faces,
        &supports,
        &records,
        &HashMap::from([(700, HashSet::from([10]))]),
    );

    assert_eq!(edge_faces, [[0, 0]]);
}

#[test]
fn standard_line_parser_reads_face_incidence() {
    let bytes = [0x60, 1, 2, 3, 0, 2, 0, 0x33, 0x36, 0, 1];
    let lines = crate::families::standard::records::standard_lines(&bytes, 2, Some(1));
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].tag, 0x03_0201);
    assert_eq!(lines[0].faces, [0, 1]);
}
