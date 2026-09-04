use super::super::*;
use super::*;

#[test]
fn a8_class21_jet_decodes_a_piecewise_quintic_pcurve() {
    let mut payload = a8_class21_test_payload();

    let pcurve = parse_a8_class21_pcurve(7, &payload).expect("complete class-21 jet");
    assert_eq!(pcurve.object_id, 7);
    assert_eq!(pcurve.surface, 3);
    assert_eq!(pcurve.distinct_knots, [10.0, 20.0]);
    assert_eq!(pcurve.multiplicities, [6, 6]);
    assert_eq!(pcurve.control_points.len(), 6);
    assert_eq!(pcurve.parameter_range, Some([10.0, 20.0]));
    assert_eq!(pcurve.class_21_suffix_scalar, Some(10.0));

    payload[6] = 0x0d;
    assert_eq!(parse_a8_class21_pcurve(7, &payload), None);
}

fn a8_class21_large_test_payload(knot_count: usize) -> Vec<u8> {
    let mut payload = vec![0x81, 0x83, 0x01, 0x15, 0x01, 0x01, 0x08, 0x01, 0x20, 0x01];
    for index in 0..knot_count {
        payload.extend_from_slice(
            &f64::from(u32::try_from(index).expect("test knot index fits u32")).to_le_bytes(),
        );
    }
    payload.push(0x19);
    payload.extend(std::iter::repeat_n(0x0d, knot_count - 2));
    payload.push(0x19);
    for _ in 0..6 {
        for _ in 0..knot_count {
            payload.extend_from_slice(&0.0f64.to_le_bytes());
        }
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [0.0f64, 10.0, 1.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    payload
}

#[test]
fn a8_class21_jet_uses_frame_extent_for_knot_count() {
    let knot_count = 8193;
    let payload = a8_class21_large_test_payload(knot_count);
    let pcurve = parse_a8_class21_pcurve(7, &payload).expect("frame-sized class-21 jet");

    assert_eq!(pcurve.distinct_knots.len(), knot_count);
    assert_eq!(pcurve.multiplicities.len(), knot_count);
    assert_eq!(pcurve.control_points.len(), 6 * (knot_count - 1));
}

#[test]
fn a8_class21_jet_rejects_count_without_frame_extent() {
    let payload = vec![
        0x81, 0x83, 0x01, 0x15, 0x01, 0x01, 0x10, 0xff, 0xff, 0xff, 0xff, 0x01,
    ];

    assert_eq!(parse_a8_class21_pcurve(7, &payload), None);
}

#[test]
fn a8_class21_scan_ignores_marker_shaped_nested_payload() {
    let child_payload = a8_class21_test_payload();
    let mut nested = vec![0xa8, 0x03, 0x21];
    nested.extend_from_slice(
        &u32::try_from(child_payload.len())
            .expect("small nested A8 payload")
            .to_le_bytes(),
    );
    nested.extend_from_slice(&7u32.to_le_bytes());
    nested.extend_from_slice(&child_payload);

    let mut wrapper = vec![0xa8, 0x03, 0x34];
    wrapper.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("small wrapper A8 payload")
            .to_le_bytes(),
    );
    wrapper.extend_from_slice(&8u32.to_le_bytes());
    wrapper.extend_from_slice(&nested);

    let mut peer = vec![0xa8, 0x03, 0x21];
    peer.extend_from_slice(
        &u32::try_from(child_payload.len())
            .expect("small peer A8 payload")
            .to_le_bytes(),
    );
    peer.extend_from_slice(&9u32.to_le_bytes());
    peer.extend_from_slice(&child_payload);
    wrapper.extend_from_slice(&peer);

    let pcurves = a8_class21_pcurves(&wrapper);
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].object_id, 9);
}

#[test]
fn object_stream_frame_walk_descends_only_into_a8_b5_children() {
    let b5 = |class: u8, object_id: u32, payload: &[u8]| {
        let mut frame = vec![0xb5, 0x03, class, payload.len() as u8];
        frame.extend_from_slice(&object_id.to_le_bytes());
        frame.extend_from_slice(payload);
        frame
    };
    let nested_b5 = b5(0x5e, 7, &[0x00]);
    let mut wrapper = vec![0xa8, 0x03, 0x34];
    wrapper.extend_from_slice(
        &u32::try_from(nested_b5.len())
            .expect("small wrapper payload")
            .to_le_bytes(),
    );
    wrapper.extend_from_slice(&8u32.to_le_bytes());
    wrapper.extend_from_slice(&nested_b5);

    let mut nested_a8 = vec![0xa8, 0x03, 0x21];
    nested_a8.extend_from_slice(&1u32.to_le_bytes());
    nested_a8.extend_from_slice(&10u32.to_le_bytes());
    nested_a8.push(0x00);
    let peer_b5 = b5(0x5e, 9, &nested_a8);
    wrapper.extend_from_slice(&peer_b5);

    let frames = object_stream_frames(&wrapper);
    assert_eq!(
        frames
            .iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 8), (0xb5, 0x5e, 7), (0xb5, 0x5e, 9)]
    );
}

#[test]
fn object_stream_frame_walk_ignores_marker_shaped_a8_payload_bytes() {
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);

    let mut payload = vec![0x00; 4];
    payload.extend_from_slice(&nested_b5);
    let mut wrapper = vec![0xa8, 0x03, 0x34];
    wrapper.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("small wrapper payload")
            .to_le_bytes(),
    );
    wrapper.extend_from_slice(&8u32.to_le_bytes());
    wrapper.extend_from_slice(&payload);

    assert_eq!(
        object_stream_frames(&wrapper)
            .iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 8)]
    );
}

#[test]
fn object_stream_frame_walk_requires_a_length_closed_a8_child_run() {
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);
    let mut wrapper = vec![0xa8, 0x03, 0x34];
    let payload_len = nested_b5.len() + 1;
    wrapper.extend_from_slice(
        &u32::try_from(payload_len)
            .expect("small wrapper payload")
            .to_le_bytes(),
    );
    wrapper.extend_from_slice(&8u32.to_le_bytes());
    wrapper.extend_from_slice(&nested_b5);
    wrapper.push(0x00);

    assert_eq!(
        object_stream_frames(&wrapper)
            .iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 8)]
    );
}

#[test]
fn object_stream_frame_walk_ignores_marker_shaped_inline_surface_poles() {
    let mut bytes = crate::test_support::a8_surface_stream();
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);
    bytes[11 + 100..11 + 100 + nested_b5.len()].copy_from_slice(&nested_b5);

    assert_eq!(
        object_stream_frames(&bytes)
            .iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 0xdeca_fbad)]
    );
}

#[test]
fn object_stream_frame_walk_descends_after_inline_surface_poles() {
    let mut bytes = crate::test_support::a8_surface_stream();
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);
    bytes.extend_from_slice(&nested_b5);
    let payload_len = u32::try_from(bytes.len() - 11).expect("small surface payload");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    assert_eq!(
        object_stream_frames(&bytes)
            .iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 0xdeca_fbad), (0xb5, 0x5e, 7)]
    );
}

#[test]
fn object_stream_frame_walk_descends_after_inline_surface_tail() {
    let mut bytes = crate::test_support::a8_inline_tail_surface_stream();
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);
    bytes.extend_from_slice(&nested_b5);
    let payload_len = u32::try_from(bytes.len() - 11).expect("small surface payload");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    assert_eq!(
        object_stream_frames(&bytes)
            .iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 0xdeca_fbad), (0xb5, 0x5e, 7)]
    );
}

#[test]
fn object_stream_runs_end_at_non_frame_bytes() {
    let frame = |object_id: u32| {
        let mut bytes = vec![0xb5, 0x03, 0x5e, 0x01];
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.push(0x00);
        bytes
    };
    let first = frame(7);
    let second = frame(9);
    let mut bytes = first.clone();
    bytes.push(0xff);
    bytes.extend_from_slice(&second);

    assert_eq!(
        object_stream_run_ranges(&bytes),
        vec![0..first.len(), first.len() + 1..bytes.len()]
    );
}

#[test]
fn object_stream_runs_cross_complete_vertex_allocations() {
    let mut bytes = crate::test_support::b5_closed_triangle_stream();
    crate::test_support::append_b5_record(&mut bytes, 0x5e, 900, &[]);

    assert_eq!(object_stream_run_ranges(&bytes), vec![0..bytes.len()]);
}

#[test]
fn object_stream_runs_cross_support_bound_external_pole_allocations() {
    let bytes = crate::test_support::a8_elided_surface_stream_with_native_vertex_chain();

    assert_eq!(object_stream_run_ranges(&bytes), vec![0..bytes.len()]);
}

#[test]
fn topology_parse_does_not_join_records_across_object_stream_runs() {
    let original = crate::test_support::b5_closed_triangle_stream();
    let frames = object_stream_frames(&original);
    let split = frames[frames.len() / 2].start;
    let mut separated = original.clone();
    separated.insert(split, 0xff);

    let merged = parse_flat(&separated).expect("flat scan can join the separated records");
    assert!(merged.complete);
    assert_ne!(parse(&separated), Some(merged));
}

#[test]
fn topology_runs_retain_only_their_own_vertex_allocations() {
    let first = crate::test_support::b5_closed_triangle_stream();
    let mut bytes = first.clone();
    bytes.push(0xff);
    bytes.extend_from_slice(&first);

    let graphs = topology_runs(&bytes);
    assert_eq!(graphs.len(), 2);
    assert!(graphs
        .iter()
        .all(|(_, graph)| graph.vertex_points.len() == 3));
}

#[test]
fn topology_parse_admits_one_referenced_isolated_geometry_frame() {
    let original = crate::test_support::b5_closed_triangle_stream();
    let expected = parse(&original).expect("closed source graph");
    let isolated = object_stream_frames(&original)
        .into_iter()
        .find(|frame| is_referenced_geometry_class(frame.family, frame.class))
        .expect("referenced geometry frame");
    let isolated_bytes = original[isolated.start..isolated.end].to_vec();
    let mut separated = original.clone();
    separated.drain(isolated.start..isolated.end);
    separated.push(0xff);
    separated.extend_from_slice(&isolated_bytes);

    assert_eq!(parse(&separated), Some(expected));
    assert_eq!(object_stream_populations(&separated).len(), 1);
}

#[test]
fn topology_parse_does_not_borrow_geometry_from_another_population() {
    let original = crate::test_support::b5_closed_triangle_stream();
    let expected = parse(&original).expect("closed source graph");
    let geometry = object_stream_frames(&original)
        .into_iter()
        .find(|frame| is_referenced_geometry_class(frame.family, frame.class))
        .expect("referenced geometry frame");
    let geometry_bytes = original[geometry.start..geometry.end].to_vec();
    let mut separated = original.clone();
    separated.drain(geometry.start..geometry.end);
    separated.push(0xff);
    separated.extend_from_slice(&geometry_bytes);
    crate::test_support::append_b5_record(&mut separated, 0x5e, 900, &[]);

    assert_ne!(parse(&separated), Some(expected));
    assert_eq!(object_stream_populations(&separated).len(), 2);
}

#[test]
fn framed_records_ignore_marker_shaped_bytes_inside_b5_payloads() {
    let mut nested_a8 = vec![0xa8, 0x03, 0x62];
    nested_a8.extend_from_slice(&0u32.to_le_bytes());
    nested_a8.extend_from_slice(&7u32.to_le_bytes());

    let mut bytes = vec![0xb5, 0x03, 0x5f, nested_a8.len() as u8];
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&nested_a8);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x01]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.push(0x00);

    let frames = object_stream_frames(&bytes);
    let records = framed_records(&bytes, &frames);
    assert_eq!(
        records
            .iter()
            .map(|record| (record.family, record.class, record.object_id))
            .collect::<Vec<_>>(),
        vec![(0xb5, 0x5f, 8), (0xb5, 0x5e, 9)]
    );
}

#[test]
fn wide_header_loop_is_a_topology_root_for_population_selection() {
    let mut bytes = vec![0xa8, 0x03, 0x62];
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());

    assert_eq!(topology_root_run_ranges(&bytes), vec![0..bytes.len()]);
    let selection = select_object_stream_population(&[bytes], None);
    assert!(selection.selected());
    assert!(!selection.source().is_empty());
}

#[test]
fn indexed_frame_parse_matches_one_shot_parse() {
    let bytes = crate::test_support::b5_closed_triangle_stream();
    let frames = object_stream_frames(&bytes);
    let records = records_from_frames(&bytes, &frames);

    assert_eq!(parse(&bytes), parse_from_frames(&bytes, &frames));
    assert_eq!(
        parse(&bytes),
        parse_from_records(&bytes, &records, &frames, true)
    );
    assert_eq!(
        typed_face_records(&bytes),
        typed_face_records_from_records(&records)
    );
    assert_eq!(
        typed_loop_records(&bytes),
        typed_loop_records_from_records(&records)
    );
    assert_eq!(
        typed_edge_records(&bytes),
        typed_edge_records_from_records(&records)
    );
    assert_eq!(
        typed_vertex_incidence_links(&bytes),
        typed_vertex_incidence_links_from_records(&records)
    );
    assert_eq!(
        typed_class_21_pcurves(&bytes),
        typed_class_21_pcurves_from_records(&records)
    );
    assert_eq!(
        typed_parameter_incidences(&bytes),
        typed_parameter_incidences_from_records(&records)
    );
    assert_eq!(
        typed_vertex_incidence_rosters(&bytes),
        typed_vertex_incidence_rosters_from_records(&records)
    );
}

#[test]
fn budgeted_dependency_admission_matches_one_shot_records() {
    let bytes = crate::test_support::b5_closed_triangle_stream();
    let frames = object_stream_frames(&bytes);
    let expected = records_from_frames(&bytes, &frames);
    let budget = cadmpeg_core::decode::WorkBudget::new(10_000);

    let actual = records_from_frames_budgeted(&bytes, &frames, Some(&budget));

    assert_eq!(actual, expected);
    assert!(!budget.exhausted());
}

#[test]
fn indexed_population_selection_preserves_records_and_census() {
    let topology = crate::test_support::b5_closed_triangle_stream();
    let expected = select_object_stream_population(std::slice::from_ref(&topology), None);
    let budget = cadmpeg_core::decode::WorkBudget::new(100_000);
    let actual = select_object_stream_population(std::slice::from_ref(&topology), Some(&budget));

    assert!(actual.selected());
    assert!(!actual.exhausted());
    assert_eq!(actual.source(), expected.source());
    assert_eq!(actual.records(), expected.records());
    assert_eq!(actual.census_records(), expected.census_records());
}

fn a8_class21_test_payload() -> Vec<u8> {
    let mut payload = vec![0x81, 0x83, 0x01, 0x15, 0x01, 0x01, 0x09, 0x01];
    for value in [10.0f64, 20.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x19, 0x19]);
    for channel in 0..6 {
        for station in 0..2 {
            payload.extend_from_slice(&(f64::from(channel * 2 + station)).to_le_bytes());
        }
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [0.0f64, 10.0, 1.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    payload
}

#[test]
fn targeted_geometry_graph_closes_a_four_span_extrusion_without_topology() {
    let append_b5 = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
        bytes.extend_from_slice(&[
            0xb5,
            0x03,
            class,
            u8::try_from(payload.len()).expect("small B5 payload"),
        ]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    };
    let append_a8 = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
        bytes.extend_from_slice(&[0xa8, 0x03, class]);
        bytes.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("small A8 payload")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    };
    let mut bytes = Vec::new();
    let mut plane = vec![0; 121];
    plane[0] = 0x80;
    for (offset, value) in [
        (25usize, 1.0f64),
        (57, 1.0),
        (73, 1.0),
        (81, 1.0),
        (89, -1.0),
        (97, 1.0),
        (105, -1.0),
        (113, 1.0),
    ] {
        plane[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    append_b5(&mut bytes, 0x27, 7, &plane);

    let knots = [0.0f64, 10.0, 20.0, 30.0, 40.0];
    let mut pcurve = vec![0x81, 0x87, 0x01, 0x15, 0x01, 0x01, 0x15, 0x01];
    for value in knots {
        pcurve.extend_from_slice(&value.to_le_bytes());
    }
    pcurve.extend_from_slice(&[0x19, 0x0d, 0x0d, 0x0d, 0x19]);
    for channel in 0..6 {
        for station in 0..knots.len() {
            pcurve.extend_from_slice(
                &f64::from(
                    u32::try_from(channel * knots.len() + station).expect("small channel station"),
                )
                .to_le_bytes(),
            );
        }
    }
    pcurve.extend_from_slice(&[0x05, 0x05]);
    for value in [0.0f64, 10.0, 1.0, 0.0] {
        pcurve.extend_from_slice(&value.to_le_bytes());
    }
    pcurve.extend_from_slice(&[0x00, 0x07]);
    append_a8(&mut bytes, 0x21, 3, &pcurve);

    let mut wrapper = vec![0x81, 0x83, 0x81, 0x01];
    for value in [0.0f64, 40.0, 0.0] {
        wrapper.extend_from_slice(&value.to_le_bytes());
    }
    wrapper.push(0x01);
    append_b5(&mut bytes, 0x24, 2, &wrapper);

    let mut extrusion = vec![0x81, 0x82];
    for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, 0.0, 10.0] {
        extrusion.extend_from_slice(&value.to_le_bytes());
    }
    extrusion.extend_from_slice(&[0x05, 0x11]);
    append_b5(&mut bytes, 0x2c, 8, &extrusion);

    let graph = targeted_geometry_graph(&bytes).expect("geometry-only graph");
    assert!(graph.faces.is_empty());
    assert_eq!(
        graph
            .extrusion_surfaces
            .get(&8)
            .map(|surface| surface.parameter_bounds),
        Some([[-2.0, 6.0], [0.0, 10.0]])
    );
    assert!(graph.pcurves.contains_key(&3));
}

#[test]
fn extrusion_reparameters_a_class21_surface_curve_from_validated_knot_spans() {
    let mut wrapper_payload = vec![0x81, 0x83, 0x81, 0x01];
    for value in [10.0f64, 20.0, 0.0] {
        wrapper_payload.extend_from_slice(&value.to_le_bytes());
    }
    wrapper_payload.push(0x01);
    let wrapper = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x24,
        object_id: 2,
        payload: wrapper_payload,
    };
    let records = HashMap::from([(2, &wrapper)]);
    let pcurves = BTreeMap::from([(
        3,
        object_stream_pcurve(7, vec![-10.0, 10.0, 20.0, 30.0], Some(10.0)),
    )]);
    let mut payload = vec![0x81, 0x82];
    for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, 0.0, 10.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2c,
        object_id: 8,
        payload,
    };

    assert_eq!(
        parse_extrusion_surface(&record, &records, &pcurves),
        Some(B5ExtrusionSurface {
            object_id: 8,
            direction: [0.0, 0.0, 1.0],
            parameter_bounds: [[-2.0, 6.0], [0.0, 10.0]],
            directrix: B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 2,
                support: (7, 3, [10.0, 20.0]),
                parameter_range: [0.0, 10.0],
            },
        })
    );

    let mut nonunit_direction = record.clone();
    nonunit_direction.payload[2..10].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(
        parse_extrusion_surface(&nonunit_direction, &records, &pcurves),
        None
    );

    let mut translated_wrapper = wrapper.clone();
    translated_wrapper.payload[12..20].copy_from_slice(&50.0f64.to_le_bytes());
    let translated_records = HashMap::from([(2, &translated_wrapper)]);
    let translated_pcurves = BTreeMap::from([(
        3,
        object_stream_pcurve(
            7,
            vec![-10.0, 10.0, 20.0, 30.0, 40.0, 50.0, 70.0],
            Some(10.0),
        ),
    )]);
    let mut translated_control = record.clone();
    let tail = translated_control.payload.len() - 2;
    translated_control.payload[tail..].copy_from_slice(&[0x05, 0x11]);
    assert_eq!(
        parse_extrusion_surface(
            &translated_control,
            &translated_records,
            &translated_pcurves,
        ),
        Some(B5ExtrusionSurface {
            object_id: 8,
            direction: [0.0, 0.0, 1.0],
            parameter_bounds: [[-2.0, 6.0], [0.0, 10.0]],
            directrix: B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 2,
                support: (7, 3, [10.0, 50.0]),
                parameter_range: [0.0, 10.0],
            },
        })
    );

    let missing_suffix = BTreeMap::from([(
        3,
        object_stream_pcurve(7, vec![-10.0, 10.0, 20.0, 30.0, 40.0, 50.0, 70.0], None),
    )]);
    assert_eq!(
        parse_extrusion_surface(&translated_control, &translated_records, &missing_suffix),
        None
    );
    let mismatched_suffix = BTreeMap::from([(
        3,
        object_stream_pcurve(
            7,
            vec![-10.0, 10.0, 20.0, 30.0, 40.0, 50.0, 70.0],
            Some(9.0),
        ),
    )]);
    assert_eq!(
        parse_extrusion_surface(&translated_control, &translated_records, &mismatched_suffix,),
        None
    );
    let mut mismatched_span = translated_control.clone();
    let active_end = 2 + 8 * 8;
    mismatched_span.payload[active_end..active_end + 8].copy_from_slice(&9.0f64.to_le_bytes());
    assert_eq!(
        parse_extrusion_surface(&mismatched_span, &translated_records, &translated_pcurves,),
        None
    );

    let nonuniform_pcurve = BTreeMap::from([(
        3,
        object_stream_pcurve(
            7,
            vec![-10.0, 10.0, 20.0, 31.0, 40.0, 50.0, 70.0],
            Some(10.0),
        ),
    )]);
    assert_eq!(
        parse_extrusion_surface(&translated_control, &translated_records, &nonuniform_pcurve,),
        None,
        "05 11 requires four uniform source spans"
    );
}

#[test]
fn extrusion_selects_the_terminal_span_of_a_direct_class20_pcurve() {
    let records = HashMap::new();
    for (controls, knots) in [
        ([0x05, 0x15], vec![0.0, 2.0, 5.0, 9.0, 12.0, 14.5]),
        ([0x05, 0x19], vec![0.0, 1.0, 3.0, 6.0, 10.0, 13.0, 15.5]),
    ] {
        let mut pcurve = object_stream_pcurve(7, knots.clone(), None);
        pcurve.class = 0x20;
        let pcurves = BTreeMap::from([(3, pcurve)]);
        let mut payload = vec![0x81, 0x83];
        for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, 0.0, 2.5] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&controls);
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2c,
            object_id: 8,
            payload,
        };
        let source_range = [knots[knots.len() - 2], knots[knots.len() - 1]];

        assert_eq!(
            parse_extrusion_surface(&record, &records, &pcurves),
            Some(B5ExtrusionSurface {
                object_id: 8,
                direction: [0.0, 0.0, 1.0],
                parameter_bounds: [[-2.0, 6.0], [0.0, 2.5]],
                directrix: B5ExtrusionDirectrix::SurfaceCurve {
                    object_id: 3,
                    support: (7, 3, source_range),
                    parameter_range: [0.0, 2.5],
                },
            })
        );

        let wrong_class = BTreeMap::from([(3, object_stream_pcurve(7, knots.clone(), None))]);
        assert_eq!(
            parse_extrusion_surface(&record, &records, &wrong_class),
            None
        );

        let mut wrong_span = record.clone();
        let active_end = 2 + 8 * 8;
        wrong_span.payload[active_end..active_end + 8].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(
            parse_extrusion_surface(&wrong_span, &records, &pcurves),
            None
        );

        let mut extra_knot = knots;
        extra_knot.insert(1, 0.5);
        let mut pcurve = object_stream_pcurve(7, extra_knot, None);
        pcurve.class = 0x20;
        let wrong_cardinality = BTreeMap::from([(3, pcurve)]);
        assert_eq!(
            parse_extrusion_surface(&record, &records, &wrong_cardinality),
            None
        );
    }
}

#[test]
fn offset_curve_directrix_binds_source_support_and_exact_ranges() {
    let mut source_payload = vec![0x81, 0x83, 0x81, 0x01];
    for value in [-3.0f64, 4.0, 0.0] {
        source_payload.extend_from_slice(&value.to_le_bytes());
    }
    source_payload.push(0x01);
    let source = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x24,
        object_id: 2,
        payload: source_payload,
    };
    let records = HashMap::from([(2, &source)]);
    let pcurves = BTreeMap::from([(3, object_stream_pcurve(7, vec![-3.0, 4.0], None))]);
    let mut payload = vec![0x81, 0x82];
    for value in [-3.0f64, 4.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(0x05);
    for value in [-1.5f64, 0.0, 0.0, 1.0, -5.0, 6.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x14,
        object_id: 4,
        payload,
    };

    assert_eq!(
        parse_extrusion_directrix(&record, &records, &pcurves),
        Some(B5ExtrusionDirectrix::Offset {
            object_id: 4,
            source: Box::new(B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 2,
                support: (7, 3, [-3.0, 4.0]),
                parameter_range: [-3.0, 4.0],
            }),
            source_parameter_range: [-3.0, 4.0],
            distance: -1.5,
            direction: [0.0, 0.0, 1.0],
            parameter_range: [-5.0, 6.0],
        })
    );

    let mut nonunit_direction = record.clone();
    nonunit_direction.payload[27..35].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(
        parse_extrusion_directrix(&nonunit_direction, &records, &pcurves),
        None
    );

    let mut wrong_control = record.clone();
    wrong_control.payload[18] = 0x01;
    assert_eq!(
        parse_extrusion_directrix(&wrong_control, &records, &pcurves),
        None
    );
    let mut mismatched_range = record;
    mismatched_range.payload[1..9].copy_from_slice(&(-2.0f64).to_le_bytes());
    assert_eq!(
        parse_extrusion_directrix(&mismatched_range, &records, &pcurves),
        None
    );
}

#[test]
fn contextual_offset_extrusion_uses_the_class30_result_chart() {
    let mut source_payload = vec![0x81, 0x83, 0x81, 0x01];
    for value in [-3.0f64, 4.0, 0.0] {
        source_payload.extend_from_slice(&value.to_le_bytes());
    }
    source_payload.push(0x01);
    let source = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x24,
        object_id: 2,
        payload: source_payload,
    };
    let mut offset_payload = vec![0x81, 0x82];
    for value in [-3.0f64, 4.0] {
        offset_payload.extend_from_slice(&value.to_le_bytes());
    }
    offset_payload.push(0x05);
    for value in [-1.5f64, 0.0, 0.0, 1.0, -5.0, 6.0] {
        offset_payload.extend_from_slice(&value.to_le_bytes());
    }
    let offset_directrix = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x14,
        object_id: 4,
        payload: offset_payload,
    };
    let records = HashMap::from([(2, &source), (4, &offset_directrix)]);
    let pcurves = BTreeMap::from([(3, object_stream_pcurve(7, vec![-3.0, 4.0], None))]);
    let source_extrusion = B5ExtrusionSurface {
        object_id: 10,
        direction: [0.0, 0.0, 1.0],
        parameter_bounds: [[0.0, 1.0], [0.0, 7.0]],
        directrix: B5ExtrusionDirectrix::SurfaceCurve {
            object_id: 2,
            support: (7, 3, [-3.0, 4.0]),
            parameter_range: [0.0, 7.0],
        },
    };
    let source_extrusions = BTreeMap::from([(10, source_extrusion)]);
    let offset_construction = B5OffsetSurface {
        object_id: 11,
        carrier_surface: 8,
        source_surface: 10,
        distance: -1.5,
        carrier_kind: 0x21,
        parameter_bounds: [[-5.0, 6.0], [2.0, 9.0]],
    };
    let mut carrier_payload = vec![0x81, 0x84];
    for value in [0.0f64, 0.0, 1.0, 2.0, 9.0, 1.0, 0.0, 35.0, 7.0] {
        carrier_payload.extend_from_slice(&value.to_le_bytes());
    }
    carrier_payload.extend_from_slice(&[0x01, 0x09]);
    let carrier = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2c,
        object_id: 8,
        payload: carrier_payload,
    };

    assert_eq!(
        parse_extrusion_surface_with_context(
            &carrier,
            &records,
            &pcurves,
            std::slice::from_ref(&offset_construction),
            &source_extrusions,
        ),
        Some(B5ExtrusionSurface {
            object_id: 8,
            direction: [0.0, 0.0, 1.0],
            parameter_bounds: [[2.0, 9.0], [-5.0, 6.0]],
            directrix: B5ExtrusionDirectrix::Offset {
                object_id: 4,
                source: Box::new(B5ExtrusionDirectrix::SurfaceCurve {
                    object_id: 2,
                    support: (7, 3, [-3.0, 4.0]),
                    parameter_range: [-3.0, 4.0],
                }),
                source_parameter_range: [-3.0, 4.0],
                distance: -1.5,
                direction: [0.0, 0.0, 1.0],
                parameter_range: [-5.0, 6.0],
            },
        })
    );

    let mut wrong_distance = offset_construction.clone();
    wrong_distance.distance = -1.0;
    assert_eq!(
        parse_extrusion_surface_with_context(
            &carrier,
            &records,
            &pcurves,
            &[wrong_distance],
            &source_extrusions,
        ),
        None
    );

    let mut wrong_bounds = offset_construction.clone();
    wrong_bounds.parameter_bounds[0][1] = 7.0;
    assert_eq!(
        parse_extrusion_surface_with_context(
            &carrier,
            &records,
            &pcurves,
            &[wrong_bounds],
            &source_extrusions,
        ),
        None
    );

    let mut increasing_auxiliary_scalars = carrier;
    let auxiliary_start = 2 + 7 * 8;
    increasing_auxiliary_scalars.payload[auxiliary_start..auxiliary_start + 8]
        .copy_from_slice(&7.0f64.to_le_bytes());
    increasing_auxiliary_scalars.payload[auxiliary_start + 8..auxiliary_start + 16]
        .copy_from_slice(&35.0f64.to_le_bytes());
    assert_eq!(
        parse_extrusion_surface_with_context(
            &increasing_auxiliary_scalars,
            &records,
            &pcurves,
            std::slice::from_ref(&offset_construction),
            &source_extrusions,
        ),
        None
    );
}

#[test]
fn supported_surface_preserves_ordered_support_pcurves() {
    let pcurve0 = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x18,
        object_id: 5,
        payload: vec![0x81, 0x83],
    };
    let mut payload = vec![0x85, 0x82, 0x83, 0x84, 0x85, 0x86, 0x09, 0x05];
    payload.extend_from_slice(&2.5f64.to_le_bytes());
    payload.extend_from_slice(&[0x03, 0x05]);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x01, 0x05]);
    let record = B5Record {
        class: 0x37,
        object_id: 7,
        payload,
        ..pcurve0.clone()
    };
    assert_eq!(
        parse_supported_surface(&record),
        Some(B5SupportedSurface {
            object_id: 7,
            carrier_surface: 2,
            support_surfaces: [3, 4],
            support_pcurves: [5, 6],
            parameters: B5SupportedSurfaceParameters::Radius {
                controls: [0x09, 0x05, 0x03, 0x05, 0x01, 0x05],
                construction_radius: 2.5,
            },
        })
    );
    let mut scalar_pair_payload = vec![0x85, 0x82, 0x83, 0x84, 0x85, 0x86];
    scalar_pair_payload.extend_from_slice(&[0x09, 0x01, 0x01, 0x05, 0x05, 0x0d]);
    scalar_pair_payload.extend_from_slice(&101.6f64.to_le_bytes());
    scalar_pair_payload.extend_from_slice(&20.0f64.to_le_bytes());
    let scalar_pair = B5Record {
        class: 0x3b,
        payload: scalar_pair_payload,
        ..record.clone()
    };
    assert_eq!(
        parse_supported_surface(&scalar_pair),
        Some(B5SupportedSurface {
            object_id: 7,
            carrier_surface: 2,
            support_surfaces: [3, 4],
            support_pcurves: [5, 6],
            parameters: B5SupportedSurfaceParameters::ScalarPair {
                controls: [0x09, 0x01, 0x01, 0x05, 0x05, 0x0d],
                scalars: [101.6, 20.0],
            },
        })
    );
    let scalar_pair = parse_supported_surface(&scalar_pair).expect("two-scalar supported surface");
    let plane = B5Surface::Plane {
        origin: [0.0; 3],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    assert!(supported_surface_parameters_match_carrier(
        &scalar_pair.parameters,
        &plane
    ));
    let cone_parameters = B5SupportedSurfaceParameters::ScalarPair {
        controls: [0x05, 0x05, 0x01, 0x03, 0x05, 0x11],
        scalars: [0.76, std::f64::consts::FRAC_PI_4],
    };
    let cone = B5Surface::Cone {
        apex: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        half_angle: std::f64::consts::FRAC_PI_4,
        reference_radius: 0.0,
        angular_range: [0.0, std::f64::consts::TAU],
        slant_range: [0.0, 1.0],
        angular_scale: 1.0,
        angular_domain: [0.0, std::f64::consts::TAU],
    };
    assert!(supported_surface_parameters_match_carrier(
        &cone_parameters,
        &cone
    ));
    let wrong_cone = B5Surface::Cone {
        apex: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        half_angle: std::f64::consts::FRAC_PI_6,
        reference_radius: 0.0,
        angular_range: [0.0, std::f64::consts::TAU],
        slant_range: [0.0, 1.0],
        angular_scale: 1.0,
        angular_domain: [0.0, std::f64::consts::TAU],
    };
    assert!(!supported_surface_parameters_match_carrier(
        &cone_parameters,
        &wrong_cone
    ));
    let construction = parse_supported_surface(&record).expect("supported surface");
    let pcurve0 = B5Record {
        object_id: 5,
        payload: vec![0x81, 0x83],
        ..pcurve0.clone()
    };
    let pcurve1 = B5Record {
        object_id: 6,
        payload: vec![0x81, 0x84],
        ..pcurve0.clone()
    };
    let records = HashMap::from([(5, &pcurve0), (6, &pcurve1)]);
    assert!(supported_surface_pcurves_match(
        &construction,
        &records,
        &HashMap::new()
    ));

    let wrong = B5Record {
        payload: vec![0x81, 0x82],
        ..pcurve1
    };
    assert!(!supported_surface_pcurves_match(
        &construction,
        &HashMap::from([(5, &pcurve0), (6, &wrong)]),
        &HashMap::new()
    ));
}

#[test]
fn supported_surface_parameter_matching_is_scale_independent() {
    let radius = 1e-200_f64;
    let parameters = B5SupportedSurfaceParameters::Radius {
        controls: [0; 6],
        construction_radius: radius,
    };
    let cylinder = |carrier_radius| B5Surface::Cylinder {
        origin: [0.0; 3],
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: carrier_radius,
        u_range: [0.0, std::f64::consts::TAU * carrier_radius],
        v_range: [-1.0, 1.0],
        angular_scale: carrier_radius,
        chart_origin: 0.0,
    };
    let torus = |carrier_radius| B5Surface::Torus {
        center: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        major_radius: 1.0,
        minor_radius: carrier_radius,
        major_angular_range: [0.0, std::f64::consts::TAU],
        major_angular_domain: [0.0, std::f64::consts::TAU],
        minor_angular_range: [0.0, std::f64::consts::TAU],
        minor_angular_domain: [0.0, std::f64::consts::TAU],
        major_scale: 1.0,
        minor_scale: carrier_radius,
    };
    let sphere = |carrier_radius| B5Surface::Sphere {
        center: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 1.0,
        azimuth_range: [0.0, 1.0],
        latitude_range: [-1.0, 1.0],
        construction_radius: carrier_radius,
        chart_origin: 0.0,
    };

    for carrier in [cylinder(radius), torus(radius), sphere(radius)] {
        assert!(supported_surface_parameters_match_carrier(
            &parameters,
            &carrier
        ));
    }
    for carrier in [
        cylinder(2.0 * radius),
        torus(2.0 * radius),
        sphere(2.0 * radius),
    ] {
        assert!(!supported_surface_parameters_match_carrier(
            &parameters,
            &carrier
        ));
    }

    let half_angle = 1e-200_f64;
    let cone_parameters = B5SupportedSurfaceParameters::ScalarPair {
        controls: [0; 6],
        scalars: [1.0, half_angle],
    };
    let cone = |carrier_half_angle| B5Surface::Cone {
        apex: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        half_angle: carrier_half_angle,
        reference_radius: 0.0,
        angular_range: [0.0, std::f64::consts::TAU],
        slant_range: [0.0, 1.0],
        angular_scale: 1.0,
        angular_domain: [0.0, std::f64::consts::TAU],
    };
    assert!(supported_surface_parameters_match_carrier(
        &cone_parameters,
        &cone(half_angle)
    ));
    assert!(!supported_surface_parameters_match_carrier(
        &cone_parameters,
        &cone(2.0 * half_angle)
    ));
}

#[test]
fn record_walk_includes_wide_header_loop_nodes() {
    let mut bytes = vec![0xa8, 0x03, 0x62];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[0x83, 0x81, 0x82]);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        records(&bytes),
        vec![
            B5Record {
                offset: 0,
                family: 0xa8,
                class: 0x62,
                object_id: 7,
                payload: vec![0x83, 0x81, 0x82],
            },
            B5Record {
                offset: 14,
                family: 0xb5,
                class: 0x5e,
                object_id: 8,
                payload: Vec::new(),
            },
        ]
    );
}

#[test]
fn record_walk_retains_opaque_a8_surface_nodes() {
    let mut bytes = vec![0xa8, 0x03, 0x34];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 2, 3]);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    let records = records(&bytes);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].family, 0xa8);
    assert_eq!(records[0].class, 0x34);
    assert_eq!(records[0].object_id, 7);
    assert_eq!(records[0].payload, [1, 2, 3]);
    assert_eq!(
        surface_node(&records[0], None),
        Some(B5Surface::Unknown {
            family: 0xa8,
            class: 0x34,
            payload: vec![1, 2, 3],
        })
    );
}

#[test]
fn record_walk_descends_into_length_bounded_a8_wrappers() {
    let mut payload = vec![0xb5, 0x03, 0x27, 0x00];
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    payload.extend_from_slice(&2u32.to_le_bytes());

    let mut bytes = vec![0xa8, 0x03, 0x34];
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&3u32.to_le_bytes());

    assert_eq!(
        records(&bytes)
            .iter()
            .map(|record| (record.offset, record.object_id, record.class))
            .collect::<Vec<_>>(),
        vec![(11, 1, 0x27), (19, 2, 0x5e), (0, 7, 0x34), (27, 3, 0x5e)]
    );
}

#[test]
fn record_walk_crosses_alternate_flag_bridge_records() {
    let mut bytes = vec![0xb5, 0x03, 0x27, 0x00];
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0xb5, 0x13, 0x5b, 0x00]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        records(&bytes)
            .iter()
            .map(|record| (record.object_id, record.class))
            .collect::<Vec<_>>(),
        vec![(1, 0x27), (3, 0x5e)]
    );
}

#[test]
fn record_walk_admits_unique_isolated_geometry_by_topology_reference() {
    fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
        bytes.extend_from_slice(&[
            0xb5,
            0x03,
            class,
            u8::try_from(payload.len()).expect("test payload fits the B5 length lane"),
        ]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    }

    let mut bytes = Vec::new();
    append(&mut bytes, 0x27, 1, &[]);
    bytes.push(0xff);
    append(&mut bytes, 0x19, 2, &[]);
    bytes.push(0xff);
    append(&mut bytes, 0x62, 4, &[0x83, 0x82, 0x83, 0x81]);
    append(&mut bytes, 0x5e, 3, &[]);
    append(&mut bytes, 0x5f, 5, &[0x82, 0x81, 0x84]);

    let parsed = records(&bytes);
    assert_eq!(
        parsed
            .iter()
            .map(|record| (record.object_id, record.class))
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([(1, 0x27), (2, 0x19), (3, 0x5e), (4, 0x62), (5, 0x5f),])
    );

    bytes.push(0xff);
    append(&mut bytes, 0x19, 2, &[0x01]);
    assert!(!records(&bytes).iter().any(|record| record.object_id == 2));
}

#[test]
fn record_walk_closes_native_vertex_incidence_dependencies() {
    fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
        bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    }

    let mut bytes = Vec::new();
    append(&mut bytes, 0x18, 2, &[0x81, 0x81]);
    bytes.push(0xff);
    append(&mut bytes, 0x5d, 6, &[0x81, 0x87, 0x00]);
    bytes.push(0xff);
    append(&mut bytes, 0x05, 7, &[0x81, 0x88]);
    bytes.push(0xff);
    let mut parameter = vec![0x81, 0x82, 0x81];
    parameter.extend_from_slice(&0.5f64.to_le_bytes());
    parameter.push(0x05);
    append(&mut bytes, 0x06, 8, &parameter);
    bytes.push(0xff);
    append(
        &mut bytes,
        0x5e,
        10,
        &[0x85, 0x82, 0x86, 0x86, 0x88, 0x88, 0x21],
    );
    append(&mut bytes, 0x5f, 11, &[]);

    assert_eq!(
        records(&bytes)
            .iter()
            .map(|record| (record.object_id, record.class))
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            (2, 0x18),
            (6, 0x5d),
            (7, 0x05),
            (8, 0x06),
            (10, 0x5e),
            (11, 0x5f),
        ])
    );
}

#[test]
fn native_vertex_graph_rejects_inconsistent_ordered_loci() {
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 5e-3, 0.0],
        [0.0, 5e-3, 0.0],
    ];
    let constraints = [([10, 11], [0, 1]), ([11, 12], [2, 3]), ([12, 10], [3, 0])];
    let adjacency = HashMap::from([(10, vec![0, 2]), (11, vec![0, 1]), (12, vec![1, 2])]);
    let mapping = propagate_vertex_points(&constraints, &adjacency, &points);
    assert!(mapping.is_empty());
}

#[test]
fn edge_record_retains_references_and_each_admitted_terminal_control() {
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x5e,
        object_id: 17,
        payload: vec![0x85, 0x92, 0x8f, 0x95, 0x93, 0x94, 0x21],
    };
    assert_eq!(
        parse_edge(&record),
        Some(B5Edge {
            object_id: 17,
            support: 18,
            vertices: [15, 21],
            parameter_incidences: [19, 20],
            terminal_control: 0x21,
        })
    );

    let mut standard = record;
    for terminal_control in [0x01, 0x02, 0x21, 0x22, 0x25, 0x26, 0x29, 0x2a] {
        *standard.payload.last_mut().expect("tail") = terminal_control;
        assert_eq!(
            parse_edge(&standard).map(|edge| edge.terminal_control),
            Some(terminal_control)
        );
    }
    standard.payload.pop();
    assert!(parse_edge(&standard).is_none());
    standard.payload.extend_from_slice(&[0x21, 0x00]);
    assert!(parse_edge(&standard).is_none());
    standard.payload.truncate(6);
    standard.payload.push(0x03);
    assert!(parse_edge(&standard).is_none());
    *standard.payload.last_mut().expect("tail") = 0x01;

    let mut bytes = vec![0xb5, 0x03, 0x5e, 7];
    bytes.extend_from_slice(&standard.object_id.to_le_bytes());
    bytes.extend_from_slice(&standard.payload);
    assert_eq!(
        edge_vertex_references(&bytes),
        BTreeMap::from([(17, [15, 21])])
    );
}

#[test]
fn referenced_edge_vertex_references_excludes_unreferenced_allocations() {
    let mut graph = parse(&crate::test_support::b5_closed_triangle_stream()).expect("B5 graph");
    assert!(graph.complete);
    graph.edges.insert(
        301,
        B5Edge {
            object_id: 301,
            support: 600,
            vertices: [10, 11],
            parameter_incidences: [20, 21],
            terminal_control: 0x01,
        },
    );
    graph.edges.insert(
        900,
        B5Edge {
            object_id: 900,
            support: 601,
            vertices: [12, 13],
            parameter_incidences: [22, 23],
            terminal_control: 0x01,
        },
    );

    assert_eq!(
        graph.referenced_edge_vertex_references(),
        Some(BTreeMap::from([(301, [10, 11])]))
    );

    graph.complete = false;
    assert_eq!(graph.referenced_edge_vertex_references(), None);
}

#[test]
fn duplicate_face_loop_ownership_does_not_close_the_graph() {
    let mut bytes = crate::test_support::b5_closed_triangle_stream();
    let mut face_payload = vec![0x82];
    face_payload.extend_from_slice(&crate::test_support::b5_object_ref(100));
    face_payload.extend_from_slice(&crate::test_support::b5_object_ref(400));
    face_payload.push(0x03);
    crate::test_support::append_b5_record(&mut bytes, 0x5f, 902, &face_payload);

    let graph = parse(&bytes).expect("structurally parseable B5 graph");

    assert_eq!(graph.faces.len(), 2);
    assert_eq!(graph.loops.len(), 1);
    assert!(!graph.complete);
    assert_eq!(face_loop_owner_counts(&graph.faces).get(&400), Some(&2));
}

#[test]
fn vertex_incidence_link_accepts_both_exact_terminal_controls() {
    let record = |terminal_control| B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x5d,
        object_id: 17,
        payload: vec![0x81, 0x92, terminal_control],
    };
    for terminal_control in [0x00, 0x04] {
        assert_eq!(
            parse_vertex_incidence_link(&record(terminal_control)),
            Some(B5VertexIncidenceLink {
                object_id: 17,
                incidence: 18,
                terminal_control,
            })
        );
    }
    assert_eq!(parse_vertex_incidence_link(&record(0x01)), None);

    let mut missing = record(0x00);
    missing.payload.pop();
    assert_eq!(parse_vertex_incidence_link(&missing), None);

    let mut residual = record(0x04);
    residual.payload.push(0);
    assert_eq!(parse_vertex_incidence_link(&residual), None);
}

#[test]
fn loop_and_endpoint_incidences_bind_an_unframed_pcurve_occurrence() {
    let incidence_payload = |parameter: f64, control| {
        let mut payload = vec![0x81, 0x89, 0x81];
        payload.extend_from_slice(&parameter.to_le_bytes());
        payload.push(control);
        payload
    };
    let records = vec![
        B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x62,
            object_id: 1,
            payload: vec![
                0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
                0x01,
            ],
        },
        B5Record {
            offset: 1,
            family: 0xb5,
            class: 0x5e,
            object_id: 10,
            payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x21],
        },
        B5Record {
            offset: 2,
            family: 0xb5,
            class: 0x06,
            object_id: 15,
            payload: incidence_payload(0.0, 0x15),
        },
        B5Record {
            offset: 3,
            family: 0xb5,
            class: 0x06,
            object_id: 16,
            payload: incidence_payload(1.0, 0x05),
        },
    ];
    let by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect();
    let surfaces = BTreeMap::from([(
        11,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x34,
            payload: Vec::new(),
        },
    )]);

    assert_eq!(
        implicit_pcurve_bindings(
            &records,
            &by_id,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &surfaces,
        ),
        BTreeMap::from([(9, 11)])
    );
}

#[test]
fn parameter_incidence_retains_aligned_compact_controls() {
    let mut payload = vec![0x82, 0x89, 0x8a, 0x82];
    payload.extend_from_slice(&1.25f64.to_le_bytes());
    payload.push(0x15);
    payload.extend_from_slice(&2.5f64.to_le_bytes());
    payload.push(0x2d);
    let incidence = parameter_incidence(&B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x06,
        object_id: 17,
        payload,
    })
    .expect("parameter incidence");

    assert_eq!(incidence.object_id, 17);
    assert_eq!(
        incidence.lanes,
        [
            B5IncidenceLane {
                curve: 9,
                parameter: 1.25,
                control: 5,
            },
            B5IncidenceLane {
                curve: 10,
                parameter: 2.5,
                control: 11,
            },
        ]
    );
}

#[test]
fn loop_and_edge_curve_wrapper_bind_an_unframed_pcurve_occurrence() {
    let records = vec![
        B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x62,
            object_id: 1,
            payload: vec![
                0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
                0x01,
            ],
        },
        B5Record {
            offset: 1,
            family: 0xb5,
            class: 0x5e,
            object_id: 10,
            payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x22],
        },
        B5Record {
            offset: 2,
            family: 0xb5,
            class: 0x25,
            object_id: 12,
            payload: vec![0x82, 0x89, 0x91, 0x81],
        },
    ];
    let by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect();
    let surfaces = BTreeMap::from([(
        11,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x34,
            payload: Vec::new(),
        },
    )]);

    assert_eq!(
        implicit_pcurve_bindings(
            &records,
            &by_id,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &surfaces,
        ),
        BTreeMap::from([(9, 11)])
    );
}
