// SPDX-License-Identifier: Apache-2.0
use crate::test_support::{test_deltas as d, test_prt, test_streams as s};
use crate::NxCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn every_unselected_deltas_family_preserves_its_paired_primary_model() {
    let cases: Vec<(Vec<u8>, Vec<u8>)> = vec![
        (
            s::topology_partition_stream(),
            d::deltas_point_partition_stream(),
        ),
        (
            s::topology_partition_stream(),
            d::deltas_edge_partition_stream(),
        ),
        (
            s::topology_partition_stream(),
            d::deltas_face_vertex_partition_stream(),
        ),
        (
            s::topology_partition_stream(),
            d::deltas_fin_partition_stream(),
        ),
        (
            s::topology_partition_stream(),
            d::deltas_loop_partition_stream(),
        ),
        (
            s::topology_partition_stream(),
            d::deltas_shell_partition_stream(),
        ),
        (
            s::topology_partition_stream(),
            d::deltas_line_partition_stream(),
        ),
        (
            s::topology_partition_stream(),
            d::deltas_plane_partition_stream(),
        ),
        (
            s::offset_surface_topology_partition_stream(),
            d::deltas_offset_surface_partition_stream(),
        ),
        (
            s::blend_surface_topology_partition_stream(),
            d::deltas_blend_surface_partition_stream(),
        ),
        (
            d::trimmed_topology_partition_stream(),
            d::deltas_trimmed_curve_partition_stream(),
        ),
        (
            s::surface_curve_topology_partition_stream(),
            d::deltas_surface_curve_partition_stream(),
        ),
        (
            d::circle_topology_partition_stream(),
            d::deltas_circle_partition_stream(),
        ),
        (
            d::ellipse_topology_partition_stream(),
            d::deltas_ellipse_partition_stream(),
        ),
        (
            d::cylinder_topology_partition_stream(),
            d::deltas_cylinder_partition_stream(),
        ),
        (
            d::cone_topology_partition_stream(),
            d::deltas_cone_partition_stream(),
        ),
        (
            d::sphere_topology_partition_stream(),
            d::deltas_sphere_partition_stream(),
        ),
        (
            d::torus_topology_partition_stream(),
            d::deltas_torus_partition_stream(),
        ),
        (
            s::charted_intersection_curve_topology_partition_stream(),
            s::ext11_intersection_deltas(&s::ext11_charted_intersection_curve_stream()),
        ),
    ];
    for (primary, delta) in cases {
        let baseline = NxCodec
            .decode(
                &mut Cursor::new(test_prt::prt_with_partition(&primary)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let paired = NxCodec
            .decode(
                &mut Cursor::new(test_prt::prt_with_streams(&[&primary, &delta])),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&paired.ir().model).unwrap(),
            serde_json::to_value(&baseline.ir().model).unwrap()
        );
        assert!(
            cadmpeg_ir::validate::validate_neutral(paired.ir(), Vec::new())
                .unwrap()
                .is_ok()
        );
    }
}

#[test]
fn unpaired_deltas_and_schema_only_streams_contribute_no_geometry() {
    // Container classification requires the TRANSMIT FILE spelling of the
    // Deltas description; the owner fixtures use a shorter description.
    let transmit_file = |bytes: Vec<u8>| {
        let short = b"Transmit (deltas)";
        let long = b": TRANSMIT FILE (deltas)";
        assert_eq!(&bytes[6..6 + short.len()], short);
        let mut relabeled = b"PS".to_vec();
        relabeled.extend_from_slice(&u32::try_from(long.len()).unwrap().to_be_bytes());
        relabeled.extend_from_slice(long);
        relabeled.extend_from_slice(&bytes[6 + short.len()..]);
        relabeled
    };
    let mut schema_only = crate::test_support::test_pmarks::prefix(1);
    schema_only.extend(crate::test_support::owned_deltas::body(36).0);
    for bytes in [
        d::deltas_point_partition_stream(),
        d::status_framed_deltas_stream(),
        d::variable_status_framed_deltas_stream(),
        transmit_file(schema_only),
        transmit_file(crate::test_support::test_pmarks::complete(
            crate::test_support::owned_deltas::body_stream(36),
        )),
    ] {
        let decoded = NxCodec
            .decode(
                &mut Cursor::new(test_prt::prt_with_partition(&bytes)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(decoded.ir().model.points.is_empty());
        assert!(decoded.ir().model.bodies.is_empty());
        assert!(decoded.ir().model.curves.is_empty());
        assert!(decoded.ir().model.surfaces.is_empty());
        let attributes = &decoded.ir().source.as_ref().unwrap().attributes;

        assert_eq!(
            attributes.get("deltas.0.selection").map(String::as_str),
            Some("unresolved")
        );
    }
}
