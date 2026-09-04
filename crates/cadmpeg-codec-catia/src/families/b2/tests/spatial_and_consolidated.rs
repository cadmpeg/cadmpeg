use super::*;

#[test]
fn b2_spatial_circle_parser_reads_the_model_space_frame_and_range() {
    let circles = crate::families::b2::records::b2_spatial_circles(&b2_spatial_circle_stream());
    let [circle] = circles.as_slice() else {
        panic!("one spatial circle");
    };
    assert_eq!(
        circle.center,
        cadmpeg_ir::math::Point3::new(17.0, 23.0, 13.0)
    );
    assert!((circle.axis.z - 1.0).abs() < 1.0e-12);
    assert_eq!(circle.radius, 7.0);
    assert_eq!(circle.range, [0.0, 11.2]);
    assert_eq!(circle.chart_shift, -16.391_148_575_128_55);
}

#[test]
fn b2_spatial_circle_parser_rejects_nonorthonormal_invalid_charts_and_nonfinite_payload() {
    for scalar in [3usize, 6, 9, 11, 12] {
        let mut broken = b2_spatial_circle_stream();
        let offset = 5 + scalar * 8;
        broken[offset..offset + 8].copy_from_slice(&0.0f64.to_le_bytes());
        assert!(
            crate::families::b2::records::b2_spatial_circles(&broken).is_empty(),
            "scalar {scalar}"
        );
    }

    for scalar in [0usize, 3, 9, 10, 13] {
        let mut broken = b2_spatial_circle_stream();
        let offset = 5 + scalar * 8;
        broken[offset..offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(
            crate::families::b2::records::b2_spatial_circles(&broken).is_empty(),
            "nonfinite scalar {scalar}"
        );
    }
}

#[test]
fn b2_composite_parser_reads_embedded_cylinder_frame() {
    let bytes = b2_embedded_cylinder_stream();
    let cylinders = crate::families::b2::records::b2_embedded_cylinders(&bytes);
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].object_id, 0x5678);
    assert_eq!(cylinders[0].wrapper_pos, 0);
    assert_eq!(
        cylinders[0].cylinder.u_range,
        [0.0, 4.0 * std::f64::consts::PI]
    );
    assert!(crate::families::b2::records::b2_cylinders(&bytes).is_empty());
}

#[test]
fn b2_composite_parser_reads_the_complete_type_three_group() {
    let one = b2_embedded_cylinder_stream();
    let frame = one[7..].to_vec();
    let mut bytes = one;
    for _ in 0..30 {
        bytes.extend_from_slice(&frame);
    }

    let cylinders = crate::families::b2::records::b2_embedded_cylinders(&bytes);
    assert_eq!(cylinders.len(), 31);
    assert!(cylinders.iter().all(|cylinder| cylinder.wrapper_pos == 0));
}

#[test]
fn decode_inner_no_directory_transfers_b2_cylinder() {
    assert_eq!(
        crate::container::scan_bytes(inner_no_directory_b2_catpart()).variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_b2_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Cylinder { radius: 2.0, .. }
    ));
}

#[test]
fn offset_support_binds_by_native_domain_knot_limits() {
    let mut carriers = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    let mut decoy = carriers[0].clone();
    let SurfaceGeometry::Nurbs(surface) = &mut decoy.geometry else {
        panic!("NURBS fixture");
    };
    for knot in surface.v_knots_mut() {
        *knot += 10.0;
    }
    carriers.push(decoy);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let offset = crate::families::b2::records::B2OffsetSupport {
        pos: 0,
        support_id: 7,
        distance: 2.0,
        domain: [
            surface.u_knots()[0],
            surface.v_knots()[0],
            *surface.u_knots().last().unwrap(),
            *surface.v_knots().last().unwrap(),
        ],
    };

    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[offset], &carriers),
        [Some(0)]
    );
}

#[test]
fn consolidated_edge_nodes_require_canonical_headers_and_terminal_controls() {
    let bytes = b2_edge_node_stream();
    assert_eq!(crate::families::b2::records::b2_edge_nodes(&bytes).len(), 1);

    let mut noncanonical_header = bytes.clone();
    noncanonical_header[0] = 0xb3;
    noncanonical_header[4] = 0x04;
    noncanonical_header.insert(5, 1);
    assert!(crate::families::b2::records::b2_edge_nodes(&noncanonical_header).is_empty());

    let mut wide_header = bytes.clone();
    wide_header[0] = 0xb3;
    wide_header[4] = 0x04;
    wide_header.insert(5, 0x40);
    let wide_nodes = crate::families::b2::records::b2_edge_nodes(&wide_header);
    let [wide_node] = wide_nodes.as_slice() else {
        panic!("canonical wide-header edge node")
    };
    assert_eq!(wide_node.header_token, 0x4004);

    let mut invalid_terminal = bytes;
    *invalid_terminal.last_mut().expect("edge terminal") = 0x03;
    assert!(crate::families::b2::records::b2_edge_nodes(&invalid_terminal).is_empty());
}

#[test]
fn consolidated_edge_nodes_accept_width_coded_terminal_two() {
    let mut bytes = b2_edge_node_stream();
    *bytes.last_mut().expect("edge terminal") = 0x02;

    let nodes = crate::families::b2::records::b2_edge_nodes(&bytes);
    let [node] = nodes.as_slice() else {
        panic!("width-coded terminal-two edge node")
    };
    assert_eq!(node.tail, 0x02);

    let mut object_stream_only_terminal = bytes;
    *object_stream_only_terminal
        .last_mut()
        .expect("edge terminal") = 0x26;
    assert!(crate::families::b2::records::b2_edge_nodes(&object_stream_only_terminal).is_empty());
}

#[test]
fn consolidated_edge_nodes_decode_terminal_allocation_reference_forms() {
    use crate::wire::bytes::AllocationReferenceEncoding;

    for (tail, value, encoding) in [
        (0x01, 0, AllocationReferenceEncoding::BackwardDistance),
        (0x02, 0, AllocationReferenceEncoding::Selector2),
        (0x21, 8, AllocationReferenceEncoding::BackwardDistance),
        (0x22, 8, AllocationReferenceEncoding::Selector2),
        (0x25, 9, AllocationReferenceEncoding::BackwardDistance),
        (0x29, 10, AllocationReferenceEncoding::BackwardDistance),
        (0x2a, 10, AllocationReferenceEncoding::Selector2),
    ] {
        let mut bytes = b2_edge_node_stream();
        *bytes.last_mut().expect("edge terminal") = tail;
        let nodes = crate::families::b2::records::b2_edge_nodes(&bytes);
        let [node] = nodes.as_slice() else {
            panic!("one terminal allocation-reference edge node")
        };
        assert_eq!(node.terminal_value, value);
        assert_eq!(node.terminal_encoding, encoding);
        assert_eq!(node.tail, tail);
    }
}
