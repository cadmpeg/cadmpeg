use crate::families::b5::graph::controls::{B5EdgeTerminalControl, B5VertexIncidenceControl};
use crate::families::b5::graph::tests::object_stream_pcurve;
use crate::families::b5::graph::{
    a8_class21_pcurves, admit_dependency_records, collect_object_stream_frames,
    edge_support_pcurve_references_from_frames, edge_vertex_references, face_loop_owner_counts,
    face_surface_references_from_frames, framed_records, implicit_pcurve_bindings,
    is_referenced_geometry_class, object_stream_frames, object_stream_populations,
    object_stream_run_ranges, parameter_incidence, parse, parse_a8_class21_pcurve, parse_edge,
    parse_flat, parse_from_frames, parse_from_records, parse_supported_surface,
    parse_vertex_incidence_link, records, records_from_frames, records_from_frames_budgeted,
    select_object_stream_population, supported_surface_parameters_match_carrier, surface_node,
    targeted_geometry_graph, targeted_geometry_graph_from_frames, topology_root_run_ranges,
    topology_runs, topology_surface_references, typed_class_21_pcurves,
    typed_class_21_pcurves_from_records, typed_edge_records, typed_edge_records_from_records,
    typed_face_records, typed_face_records_from_records, typed_loop_records,
    typed_loop_records_from_records, typed_parameter_incidences,
    typed_parameter_incidences_from_records, typed_vertex_incidence_links,
    typed_vertex_incidence_links_from_records, typed_vertex_incidence_rosters,
    typed_vertex_incidence_rosters_from_records, B5Edge, B5ExtrusionDirectrix, B5ExtrusionSurface,
    B5IncidenceLane, B5OffsetSurface, B5Record, B5RecordBuf, B5SupportedSurface,
    B5SupportedSurfaceParameters, B5Surface, B5VertexIncidenceLink, ObjectFrame,
};
use std::collections::{BTreeMap, HashMap};

/// Borrow each owned record under the same identity.
fn record_views<'a>(records: &HashMap<u32, &'a B5RecordBuf>) -> HashMap<u32, B5Record<'a>> {
    records
        .iter()
        .map(|(&object_id, record)| (object_id, record.record()))
        .collect()
}

/// Index borrowed records by reference, as the parsers read them.
fn record_refs<'r, 'a>(views: &'r HashMap<u32, B5Record<'a>>) -> HashMap<u32, &'r B5Record<'a>> {
    views
        .iter()
        .map(|(&object_id, record)| (object_id, record))
        .collect()
}

fn parse_extrusion_directrix(
    record: &B5RecordBuf,
    records: &HashMap<u32, &B5RecordBuf>,
    pcurves: &BTreeMap<u32, super::super::B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionDirectrix> {
    let views = record_views(records);
    let refs = record_refs(&views);
    crate::test_support::with_service_context(|ctx| {
        super::super::parse_extrusion_directrix(ctx, &record.record(), &refs, pcurves)
    })
    .expect("service budget")
}

fn parse_extrusion_surface(
    record: &B5RecordBuf,
    records: &HashMap<u32, &B5RecordBuf>,
    pcurves: &BTreeMap<u32, super::super::B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionSurface> {
    let views = record_views(records);
    super::super::parse_extrusion_surface(&record.record(), &record_refs(&views), pcurves)
}

fn parse_extrusion_surface_with_context(
    record: &B5RecordBuf,
    records: &HashMap<u32, &B5RecordBuf>,
    pcurves: &BTreeMap<u32, super::super::B5ObjectStreamPcurve>,
    offset_constructions: &[B5OffsetSurface],
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
) -> Option<B5ExtrusionSurface> {
    let views = record_views(records);
    let refs = record_refs(&views);
    crate::test_support::with_service_context(|ctx| {
        let offsets = super::super::extrusion_offsets_by_carrier(ctx, offset_constructions)?;
        super::super::parse_extrusion_surface_with_context(
            ctx,
            &record.record(),
            &refs,
            pcurves,
            &offsets,
            extrusion_surfaces,
        )
    })
    .expect("service budget")
}

fn supported_surface_pcurves_match(
    construction: &B5SupportedSurface,
    records: &HashMap<u32, &B5RecordBuf>,
    object_stream_pcurves: &BTreeMap<u32, super::super::B5Pcurve>,
) -> bool {
    let views = record_views(records);
    let refs = record_refs(&views);
    crate::test_support::with_service_context(|ctx| {
        super::super::supported_surface_pcurves_match(
            ctx,
            construction,
            &refs,
            object_stream_pcurves,
        )
    })
    .expect("service budget")
}

fn propagate_vertex_points(
    constraints: &[([u32; 2], [usize; 2])],
    adjacency: &HashMap<u32, Vec<usize>>,
    points: &[cadmpeg_ir::features::FinitePoint3],
) -> std::collections::BTreeMap<u32, cadmpeg_ir::features::FinitePoint3> {
    crate::test_support::with_service_context(|ctx| {
        super::super::propagate_vertex_points(ctx, constraints, adjacency, points)
    })
    .expect("service budget")
}

#[test]
fn retained_object_frames_refuse_the_caller_collection_limit() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        collect_object_stream_frames(ctx, &bytes)
    });
    assert!(matches!(limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_b5_object_frames"));
    let admitted =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service collection budget");
    let scanned = crate::test_support::with_service_context(|ctx| {
        ctx.try_collect_vec(object_stream_frames(ctx, &bytes)?, "catia_b5_object_frames")
    })
    .expect("service frame scan budget");
    assert_eq!(admitted.len(), scanned.len());
    assert!(admitted.iter().zip(scanned).all(|(left, right)| {
        (
            left.start,
            left.end,
            left.family,
            left.class,
            left.object_id,
        ) == (
            right.start,
            right.end,
            right.family,
            right.class,
            right.object_id,
        )
    }));
}

#[test]
fn streaming_object_frame_scan_refuses_work_before_the_first_visit() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    crate::test_support::with_work_limit(0, |ctx| {
        let mut frames = object_stream_frames(ctx, &bytes).expect("lazy frame source");
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            frames.next().expect("first visit refuses")
        else {
            panic!("frame source admission must refuse");
        };
        assert_eq!(limit.operation, "catia_b5_object_frame_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn object_population_copy_refuses_the_caller_retained_limit() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut cap = 0;
    let mut reached = false;
    for _ in 0..128 {
        match crate::test_support::with_retained_limit(cap, |ctx| {
            object_stream_populations(ctx, &bytes)
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_b5_topology_run_bytes" =>
            {
                reached = true;
                break;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                cap = error
                    .used
                    .checked_add(error.additional)
                    .expect("bounded fixture");
            }
            other => panic!("topology copy not reached: {other:?}"),
        }
    }
    assert!(reached, "topology copy limit was not reached");
    let populations =
        crate::test_support::with_service_context(|ctx| object_stream_populations(ctx, &bytes))
            .expect("service retained budget");
    assert_eq!(populations, vec![bytes]);
}

#[test]
fn object_population_selection_refuses_before_indexing_runs() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut cap = 0;
    let mut reached = false;
    for _ in 0..128 {
        match crate::test_support::with_collection_limit(cap, |ctx| {
            let mut storage = ctx.reserve_scoped(0, "test_b5_selection")?;
            select_object_stream_population(ctx, std::slice::from_ref(&bytes), None, &mut storage)
                .map(|_| ())
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_b5_selected_stream_ranges" =>
            {
                reached = true;
                break;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                cap = error
                    .used
                    .checked_add(error.additional)
                    .expect("bounded fixture");
            }
            Ok(()) => panic!("selection admitted before its index limit"),
            Err(error) => panic!("unexpected selection refusal: {error}"),
        }
    }
    assert!(reached, "selection index limit was not reached");
    let (selected, source) = crate::test_support::with_service_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_b5_selection")?;
        let selection =
            select_object_stream_population(ctx, std::slice::from_ref(&bytes), None, &mut storage)?;
        Ok::<_, cadmpeg_core::CodecError>((selection.selected(), selection.source().to_vec()))
    })
    .expect("service collection budget");
    assert!(selected);
    assert_eq!(source, bytes);
}

#[test]
fn topology_run_ranges_refuse_each_caller_collection_limit() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut refused = std::collections::BTreeSet::new();
    for limit in 0..32 {
        if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) =
            crate::test_support::with_collection_limit(limit, |ctx| {
                topology_root_run_ranges(ctx, &bytes)
            })
        {
            refused.insert(error.operation);
        }
    }
    for operation in ["catia_b5_object_run_ranges", "catia_b5_topology_run_ranges"] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
    let ranges =
        crate::test_support::with_service_context(|ctx| topology_root_run_ranges(ctx, &bytes))
            .expect("service collection budget");
    assert_eq!(ranges, vec![0..bytes.len()]);
}

#[test]
fn a8_class21_jet_decodes_a_piecewise_quintic_pcurve() {
    let mut payload = a8_class21_test_payload();

    let pcurve =
        crate::test_support::with_service_context(|ctx| parse_a8_class21_pcurve(ctx, 7, &payload))
            .expect("service resource budget")
            .expect("complete class-21 jet");
    assert_eq!(pcurve.object_id, 7);
    assert_eq!(pcurve.surface, 3);
    assert_eq!(
        pcurve.distinct_knots,
        crate::test_support::test_b5::finite_lane(&[10.0, 20.0])
    );
    assert_eq!(pcurve.multiplicities, [6, 6]);
    assert_eq!(pcurve.control_points.len(), 6);
    assert_eq!(
        pcurve.parameter_range,
        Some(crate::test_support::test_b5::finite_pair([10.0, 20.0]))
    );
    assert_eq!(
        pcurve.class_21_suffix_scalar,
        Some(crate::test_support::test_b5::positive(10.0))
    );

    payload[6] = 0x0d;
    assert_eq!(
        crate::test_support::with_service_context(|ctx| parse_a8_class21_pcurve(ctx, 7, &payload))
            .expect("service resource budget"),
        None
    );
}

pub(super) fn a8_class21_large_test_payload(knot_count: usize) -> Vec<u8> {
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
    let pcurve =
        crate::test_support::with_service_context(|ctx| parse_a8_class21_pcurve(ctx, 7, &payload))
            .expect("service resource budget")
            .expect("frame-sized class-21 jet");

    assert_eq!(pcurve.distinct_knots.len(), knot_count);
    assert_eq!(pcurve.multiplicities.len(), knot_count);
    assert_eq!(pcurve.control_points.len(), 6 * (knot_count - 1));
}

#[test]
fn a8_class21_pcurve_multiplicities_propagate_collection_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = a8_class21_test_payload();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two knots use thirty-four items before the retained multiplicity vector.
    policy.limits.max_collection_items = 34;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("fixture fits the input limit");
    let error = parse_a8_class21_pcurve(&ctx, 7, &payload)
        .expect_err("multiplicity array exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia B5 pcurve multiplicities"));
}

#[test]
fn a8_class21_multiplicity_scan_preserves_work_refusal() {
    let payload = a8_class21_test_payload();
    let service =
        crate::test_support::with_service_context(|ctx| parse_a8_class21_pcurve(ctx, 7, &payload))
            .expect("service work budget")
            .expect("complete class-21 jet");
    assert_eq!(service.multiplicities, [6, 6]);

    let operation = "catia_b5_a8_class21_multiplicity_scan";
    let refused = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = parse_a8_class21_pcurve(ctx, 7, &payload);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == operation
    ));
}

#[test]
fn a8_class21_jet_refuses_before_each_admitted_lane() {
    let payload = a8_class21_test_payload();
    let limited =
        crate::test_support::with_work_limit(0, |ctx| parse_a8_class21_pcurve(ctx, 7, &payload));
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_b5_a8_class21_scalar_lane_bytes"
    ));

    let mut refused = std::collections::HashSet::new();
    for cap in 0..64 {
        match crate::test_support::with_collection_limit(cap, |ctx| {
            parse_a8_class21_pcurve(ctx, 7, &payload)
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            Ok(None) => panic!("class-21 fixture must decode"),
            Err(error) => panic!("unexpected pcurve refusal: {error}"),
        }
    }
    for operation in [
        "catia B5 pcurve distinct knots",
        "catia B5 pcurve knot values",
        "catia B5 pcurve u jet",
        "catia B5 pcurve v jet",
        "catia B5 pcurve du jet",
        "catia B5 pcurve dv jet",
        "catia B5 pcurve ddu jet",
        "catia B5 pcurve ddv jet",
        "catia B5 pcurve point jets",
        "catia B5 pcurve first jets",
        "catia B5 pcurve second jets",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
    assert!(
        crate::test_support::with_service_context(|ctx| parse_a8_class21_pcurve(ctx, 7, &payload))
            .expect("service resource budget")
            .is_some()
    );
}

#[test]
fn a8_class21_jet_rejects_count_without_frame_extent() {
    let payload = vec![
        0x81, 0x83, 0x01, 0x15, 0x01, 0x01, 0x10, 0xff, 0xff, 0xff, 0xff, 0x01,
    ];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| parse_a8_class21_pcurve(ctx, 7, &payload))
            .expect("service resource budget"),
        None
    );
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

    let pcurves =
        crate::test_support::with_service_context(|ctx| a8_class21_pcurves(ctx, &wrapper))
            .expect("service resource budget");
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].object_id, 9);
}

#[test]
fn object_stream_frame_walk_descends_only_into_a8_b5_children() {
    let b5 = |class: u8, object_id: u32, payload: &[u8]| {
        let mut frame = vec![
            0xb5,
            0x03,
            class,
            u8::try_from(payload.len()).expect("fixture value fits u8"),
        ];
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

    let frames = crate::test_support::with_service_context(|ctx| {
        collect_object_stream_frames(ctx, &wrapper)
    })
    .expect("service frame scan budget");
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
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(
            ctx, &wrapper
        ))
        .expect("service frame scan budget")
        .into_iter()
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
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(
            ctx, &wrapper
        ))
        .expect("service frame scan budget")
        .into_iter()
        .map(|frame| (frame.family, frame.class, frame.object_id))
        .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 8)]
    );
}

#[test]
fn object_stream_frame_walk_ignores_marker_shaped_inline_surface_poles() {
    let mut bytes = crate::test_support::test_a5a8::a8_surface_stream();
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);
    bytes[11 + 100..11 + 100 + nested_b5.len()].copy_from_slice(&nested_b5);

    assert_eq!(
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service frame scan budget")
            .into_iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 0xdeca_fbad)]
    );
}

#[test]
fn object_stream_frame_walk_descends_after_inline_surface_poles() {
    let mut bytes = crate::test_support::test_a5a8::a8_surface_stream();
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);
    bytes.extend_from_slice(&nested_b5);
    let payload_len = u32::try_from(bytes.len() - 11).expect("small surface payload");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    assert_eq!(
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service frame scan budget")
            .into_iter()
            .map(|frame| (frame.family, frame.class, frame.object_id))
            .collect::<Vec<_>>(),
        vec![(0xa8, 0x34, 0xdeca_fbad), (0xb5, 0x5e, 7)]
    );
}

#[test]
fn object_stream_frame_walk_descends_after_inline_surface_tail() {
    let mut bytes = crate::test_support::test_a5a8::a8_inline_tail_surface_stream();
    let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
    nested_b5.extend_from_slice(&7u32.to_le_bytes());
    nested_b5.push(0x00);
    bytes.extend_from_slice(&nested_b5);
    let payload_len = u32::try_from(bytes.len() - 11).expect("small surface payload");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    assert_eq!(
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service frame scan budget")
            .into_iter()
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
        crate::test_support::with_service_context(|ctx| object_stream_run_ranges(ctx, &bytes))
            .expect("service collection budget"),
        vec![0..first.len(), first.len() + 1..bytes.len()]
    );
}

#[test]
fn object_stream_runs_cross_complete_vertex_allocations() {
    let mut bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    crate::test_support::test_b5::append_b5_record(&mut bytes, 0x5e, 900, &[]);

    assert_eq!(
        crate::test_support::with_service_context(|ctx| object_stream_run_ranges(ctx, &bytes))
            .expect("service collection budget"),
        vec![0..bytes.len()]
    );
}

#[test]
fn object_stream_runs_cross_support_bound_external_pole_allocations() {
    let bytes = crate::test_support::test_b5::a8_elided_surface_stream_with_native_vertex_chain();

    assert_eq!(
        crate::test_support::with_service_context(|ctx| object_stream_run_ranges(ctx, &bytes))
            .expect("service collection budget"),
        vec![0..bytes.len()]
    );
}

#[test]
fn topology_parse_does_not_join_records_across_object_stream_runs() {
    let original = crate::test_support::test_b5::b5_closed_triangle_stream();
    let frames = crate::test_support::with_service_context(|ctx| {
        collect_object_stream_frames(ctx, &original)
    })
    .expect("service frame scan budget");
    let split = frames[frames.len() / 2].start;
    let mut separated = original.clone();
    separated.insert(split, 0xff);

    let merged = crate::test_support::with_service_context(|ctx| {
        parse_flat(ctx, &separated, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("flat scan can join the separated records");
    assert!(merged.complete);
    assert_ne!(
        crate::test_support::with_service_context(|ctx| parse(
            ctx,
            &separated,
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service resource budget"),
        Some(merged)
    );
}

#[test]
fn topology_runs_retain_only_their_own_vertex_allocations() {
    let first = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut bytes = first.clone();
    bytes.push(0xff);
    bytes.extend_from_slice(&first);

    let graphs = crate::test_support::with_service_context(|ctx| {
        topology_runs(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget");
    assert_eq!(graphs.len(), 2);
    assert!(graphs
        .iter()
        .all(|(_, graph)| graph.vertices.raw_points().len() == 3));
}

#[test]
fn topology_parse_admits_one_referenced_isolated_geometry_frame() {
    let original = crate::test_support::test_b5::b5_closed_triangle_stream();
    let expected = crate::test_support::with_service_context(|ctx| {
        parse(ctx, &original, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed source graph");
    let isolated = crate::test_support::with_service_context(|ctx| {
        collect_object_stream_frames(ctx, &original)
    })
    .expect("service frame scan budget")
    .into_iter()
    .find(|frame| is_referenced_geometry_class(frame.family, frame.class))
    .expect("referenced geometry frame");
    let isolated_bytes = original[isolated.start..isolated.end].to_vec();
    let mut separated = original.clone();
    separated.drain(isolated.start..isolated.end);
    separated.push(0xff);
    separated.extend_from_slice(&isolated_bytes);

    assert_eq!(
        crate::test_support::with_service_context(|ctx| parse(
            ctx,
            &separated,
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service resource budget"),
        Some(expected)
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| object_stream_populations(ctx, &separated))
            .expect("service collection budget")
            .len(),
        1
    );
}

#[test]
fn topology_parse_does_not_borrow_geometry_from_another_population() {
    let original = crate::test_support::test_b5::b5_closed_triangle_stream();
    let expected = crate::test_support::with_service_context(|ctx| {
        parse(ctx, &original, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed source graph");
    let geometry = crate::test_support::with_service_context(|ctx| {
        collect_object_stream_frames(ctx, &original)
    })
    .expect("service frame scan budget")
    .into_iter()
    .find(|frame| is_referenced_geometry_class(frame.family, frame.class))
    .expect("referenced geometry frame");
    let geometry_bytes = original[geometry.start..geometry.end].to_vec();
    let mut separated = original.clone();
    separated.drain(geometry.start..geometry.end);
    separated.push(0xff);
    separated.extend_from_slice(&geometry_bytes);
    crate::test_support::test_b5::append_b5_record(&mut separated, 0x5e, 900, &[]);

    assert_ne!(
        crate::test_support::with_service_context(|ctx| parse(
            ctx,
            &separated,
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service resource budget"),
        Some(expected)
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| object_stream_populations(ctx, &separated))
            .expect("service collection budget")
            .len(),
        2
    );
}

#[test]
fn framed_records_ignore_marker_shaped_bytes_inside_b5_payloads() {
    let mut nested_a8 = vec![0xa8, 0x03, 0x62];
    nested_a8.extend_from_slice(&0u32.to_le_bytes());
    nested_a8.extend_from_slice(&7u32.to_le_bytes());

    let mut bytes = vec![
        0xb5,
        0x03,
        0x5f,
        u8::try_from(nested_a8.len()).expect("fixture value fits u8"),
    ];
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&nested_a8);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x01]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.push(0x00);

    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service frame scan budget");
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

    assert_eq!(
        crate::test_support::with_service_context(|ctx| topology_root_run_ranges(ctx, &bytes))
            .expect("service collection budget"),
        vec![0..bytes.len()]
    );
    let streams = [bytes];
    let (selected, source_empty) = crate::test_support::with_service_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_b5_selection")?;
        let selection = select_object_stream_population(ctx, &streams, None, &mut storage)?;
        Ok::<_, cadmpeg_core::CodecError>((selection.selected(), selection.source().is_empty()))
    })
    .expect("service collection budget");
    assert!(selected);
    assert!(!source_empty);
}

#[test]
fn indexed_frame_parse_matches_one_shot_parse() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, &bytes, &frames))
            .expect("service budget");

    assert_eq!(
        crate::test_support::with_service_context(|ctx| parse(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service resource budget"),
        crate::test_support::with_service_context(|ctx| parse_from_frames(
            ctx,
            &bytes,
            &frames,
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service resource budget")
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| parse(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service resource budget"),
        crate::test_support::with_service_context(|ctx| parse_from_records(
            ctx,
            &bytes,
            &records,
            &frames,
            true,
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service resource budget")
    );
    assert_eq!(
        typed_face_records(&bytes),
        crate::test_support::with_service_context(|ctx| {
            typed_face_records_from_records(ctx, &records).expect("service budget")
        })
    );
    assert_eq!(
        typed_loop_records(&bytes),
        crate::test_support::with_service_context(|ctx| {
            typed_loop_records_from_records(ctx, &records).expect("service budget")
        })
    );
    assert_eq!(
        typed_edge_records(&bytes),
        crate::test_support::with_service_context(|ctx| {
            typed_edge_records_from_records(ctx, &records).expect("service budget")
        })
    );
    assert_eq!(
        typed_vertex_incidence_links(&bytes),
        crate::test_support::with_service_context(|ctx| {
            typed_vertex_incidence_links_from_records(ctx, &records).expect("service budget")
        })
    );
    assert_eq!(
        typed_class_21_pcurves(&bytes),
        crate::test_support::with_service_context(|ctx| {
            typed_class_21_pcurves_from_records(ctx, &records)
        })
        .expect("service budget")
    );
    assert_eq!(
        typed_parameter_incidences(&bytes),
        crate::test_support::with_service_context(|ctx| {
            typed_parameter_incidences_from_records(ctx, &records).expect("service budget")
        })
    );
    assert_eq!(
        typed_vertex_incidence_rosters(&bytes),
        crate::test_support::with_service_context(|ctx| {
            typed_vertex_incidence_rosters_from_records(ctx, &records).expect("service budget")
        })
    );
}

#[test]
fn typed_edge_record_index_refuses_collection_limit() {
    let records = [B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5e,
        object_id: 17,
        payload: vec![0x85, 0x92, 0x8f, 0x95, 0x93, 0x94, 0x21],
    }];
    let records = records.each_ref().map(B5RecordBuf::record);
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        typed_edge_records_from_records(ctx, &records)
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    let edges = crate::test_support::with_service_context(|ctx| {
        typed_edge_records_from_records(ctx, &records)
    })
    .expect("service budget");
    assert!(!edges.is_empty());
}

#[test]
fn typed_vertex_incidence_link_index_refuses_collection_limit() {
    let records = [B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5d,
        object_id: 17,
        payload: vec![0x81, 0x92, 0x04],
    }];
    let records = records.each_ref().map(B5RecordBuf::record);
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        typed_vertex_incidence_links_from_records(ctx, &records)
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    let links = crate::test_support::with_service_context(|ctx| {
        typed_vertex_incidence_links_from_records(ctx, &records)
    })
    .expect("service budget");
    assert!(!links.is_empty());
}

#[test]
fn budgeted_dependency_admission_matches_one_shot_records() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service frame scan budget");
    let expected =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, &bytes, &frames))
            .expect("service budget");
    let budget = cadmpeg_core::decode::WorkBudget::new(10_000);

    let actual = crate::test_support::with_service_context(|ctx| {
        records_from_frames_budgeted(ctx, &bytes, &frames, Some(&budget))
    })
    .expect("service budget");

    assert_eq!(actual, expected);
    assert!(!budget.exhausted());
}

#[test]
fn b5_record_payload_and_dependency_closure_refuse_caller_limits() {
    let bytes = [0u8; 9];
    let frame = ObjectFrame {
        start: 0,
        end: bytes.len(),
        family: 0xb5,
        class: 0x18,
        object_id: 9,
    };
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5f,
        object_id: 7,
        payload: vec![0x81, 0x89],
    };
    let candidates = HashMap::from([(9, Some(frame))]);
    let closure = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut scratch = ctx.reserve_scoped(0, "test_b5_dependency_scratch")?;
        admit_dependency_records(
            ctx,
            &bytes,
            vec![record.record()],
            &candidates,
            None,
            &mut scratch,
        )
        .map(|records| records.len())
    };
    let mut refused = std::collections::BTreeSet::new();
    for limit in 0..16 {
        if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) =
            crate::test_support::with_collection_limit(limit, |ctx| closure(ctx))
        {
            refused.insert(error.operation);
        }
    }
    for operation in [
        "catia_b5_visited_dependency_ids",
        "catia_b5_pending_dependency_ids",
        "catia_b5_found_dependency_records",
        "catia_b5_admitted_dependency_records",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
    let surfaces = crate::test_support::with_collection_limit(0, |ctx| {
        topology_surface_references(ctx, &[record.record()])
    });
    assert!(
        matches!(surfaces, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_topology_surface_references")
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| closure(ctx)).expect("service budget"),
        2
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            topology_surface_references(ctx, &[record.record()])
        })
        .expect("service budget"),
        std::collections::BTreeSet::from([9])
    );
}

#[test]
fn targeted_geometry_record_candidates_refuse_each_collection_limit() {
    let mut bytes = Vec::new();
    crate::test_support::test_b5::append_b5_record(&mut bytes, 0x18, 9, &[0x81, 0x89]);
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, &bytes))
            .expect("service budget");
    // The records borrow the stream; the candidate index and the record
    // list are the collections the caller's limit can refuse.
    let mut refused = std::collections::BTreeSet::new();
    for limit in 0..16 {
        if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) =
            crate::test_support::with_collection_limit(limit, |ctx| {
                targeted_geometry_graph_from_frames(
                    ctx,
                    &bytes,
                    &frames,
                    &mut crate::nurbs::LaneRefusals::new(),
                )
            })
        {
            refused.insert(error.operation);
        }
    }
    for operation in [
        "catia_b5_targeted_geometry_candidates",
        "catia_b5_targeted_geometry_records",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
    crate::test_support::with_service_context(|ctx| {
        targeted_geometry_graph_from_frames(
            ctx,
            &bytes,
            &frames,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service budget");
}

#[test]
fn indexed_population_selection_preserves_records_and_census() {
    let topology = crate::test_support::test_b5::b5_closed_triangle_stream();
    let budget = cadmpeg_core::decode::WorkBudget::new(100_000);
    crate::test_support::with_service_context(|ctx| {
        let streams = std::slice::from_ref(&topology);
        let mut expected_storage = ctx.reserve_scoped(0, "test_b5_selection")?;
        let expected = select_object_stream_population(ctx, streams, None, &mut expected_storage)?;
        let mut actual_storage = ctx.reserve_scoped(0, "test_b5_selection")?;
        let actual =
            select_object_stream_population(ctx, streams, Some(&budget), &mut actual_storage)?;
        assert!(actual.selected());
        assert!(!actual.exhausted());
        assert_eq!(actual.source(), expected.source());
        assert_eq!(actual.records(), expected.records());
        assert_eq!(actual.census_records(), expected.census_records());
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("service collection budget");
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
fn a8_class21_distinct_knot_push_preserves_collection_refusal() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    const OPERATION: &str = "catia B5 pcurve distinct knots";
    let payload = a8_class21_test_payload();
    let service =
        crate::test_support::with_service_context(|ctx| parse_a8_class21_pcurve(ctx, 7, &payload))
            .expect("service resource budget")
            .expect("complete class-21 jet");
    assert_eq!(
        service.distinct_knots,
        crate::test_support::test_b5::finite_lane(&[10.0, 20.0])
    );

    let refusal = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        OPERATION,
        |cap| {
            crate::test_support::with_collection_limit(cap, |ctx| {
                let result = parse_a8_class21_pcurve(ctx, 7, &payload);
                if let Err(CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            })
        },
    );
    assert!(matches!(
        refusal,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == OPERATION
    ));
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

    let graph =
        crate::test_support::with_service_context(|ctx| targeted_geometry_graph(ctx, &bytes))
            .expect("service resource budget")
            .expect("geometry-only graph");
    assert!(graph.faces.is_empty());
    assert_eq!(
        graph
            .extrusion_surfaces
            .get(&8)
            .map(|surface| surface.parameter_bounds),
        Some(crate::test_support::test_b5::increasing_bounds([
            [-2.0, 6.0],
            [0.0, 10.0]
        ]))
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
    let wrapper = B5RecordBuf {
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
    let record = B5RecordBuf {
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
            direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
            parameter_bounds: crate::test_support::test_b5::increasing_bounds([
                [-2.0, 6.0],
                [0.0, 10.0]
            ]),
            directrix: B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 2,
                support: (
                    7,
                    3,
                    crate::test_support::test_b5::finite_pair([10.0, 20.0])
                ),
                parameter_range: crate::test_support::test_b5::increasing([0.0, 10.0]),
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
            direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
            parameter_bounds: crate::test_support::test_b5::increasing_bounds([
                [-2.0, 6.0],
                [0.0, 10.0]
            ]),
            directrix: B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 2,
                support: (
                    7,
                    3,
                    crate::test_support::test_b5::finite_pair([10.0, 50.0])
                ),
                parameter_range: crate::test_support::test_b5::increasing([0.0, 10.0]),
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
fn invalid_extrusion_span_controls_remain_malformed_candidates() {
    let interval = crate::test_support::test_b5::increasing([0.0, 1.0]);
    let directrix = B5ExtrusionDirectrix::SurfaceCurve {
        object_id: 2,
        support: (7, 3, crate::test_support::test_b5::finite_pair([0.0, 1.0])),
        parameter_range: interval,
    };
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::terminal_span_directrix(ctx, 3, interval, [0x00, 0x15], &BTreeMap::new())
        })
        .expect("service budget"),
        None
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::translated_directrix_span_count(
                ctx,
                &directrix,
                [0.0, 1.0],
                [0x00, 0x11],
                &BTreeMap::new(),
            )
        })
        .expect("service budget"),
        None
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
        let record = B5RecordBuf {
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
                direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
                parameter_bounds: crate::test_support::test_b5::increasing_bounds([
                    [-2.0, 6.0],
                    [0.0, 2.5]
                ]),
                directrix: B5ExtrusionDirectrix::SurfaceCurve {
                    object_id: 3,
                    support: (
                        7,
                        3,
                        crate::test_support::test_b5::finite_pair(source_range)
                    ),
                    parameter_range: crate::test_support::test_b5::increasing([0.0, 2.5]),
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
    let source = B5RecordBuf {
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
    let record = B5RecordBuf {
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
                support: (7, 3, crate::test_support::test_b5::finite_pair([-3.0, 4.0])),
                parameter_range: crate::test_support::test_b5::increasing([-3.0, 4.0]),
            }),
            source_parameter_range: crate::test_support::test_b5::increasing([-3.0, 4.0]),
            distance: crate::test_support::test_b5::finite(-1.5),
            direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
            parameter_range: crate::test_support::test_b5::increasing([-5.0, 6.0]),
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
    let source = B5RecordBuf {
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
    let offset_directrix = B5RecordBuf {
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
        direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
        parameter_bounds: crate::test_support::test_b5::increasing_bounds([[0.0, 1.0], [0.0, 7.0]]),
        directrix: B5ExtrusionDirectrix::SurfaceCurve {
            object_id: 2,
            support: (7, 3, crate::test_support::test_b5::finite_pair([-3.0, 4.0])),
            parameter_range: crate::test_support::test_b5::increasing([0.0, 7.0]),
        },
    };
    let source_extrusions = BTreeMap::from([(10, source_extrusion)]);
    let offset_construction = B5OffsetSurface {
        object_id: 11,
        carrier_surface: 8,
        source_surface: 10,
        distance: crate::test_support::test_b5::finite(-1.5),
        carrier_kind: crate::families::b5::graph::B5OffsetCarrierKind::Extrusion,
        parameter_bounds: crate::test_support::test_b5::increasing_bounds([
            [-5.0, 6.0],
            [2.0, 9.0],
        ]),
    };
    let mut carrier_payload = vec![0x81, 0x84];
    for value in [0.0f64, 0.0, 1.0, 2.0, 9.0, 1.0, 0.0, 35.0, 7.0] {
        carrier_payload.extend_from_slice(&value.to_le_bytes());
    }
    carrier_payload.extend_from_slice(&[0x01, 0x09]);
    let carrier = B5RecordBuf {
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
            direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
            parameter_bounds: crate::test_support::test_b5::increasing_bounds([
                [2.0, 9.0],
                [-5.0, 6.0]
            ]),
            directrix: B5ExtrusionDirectrix::Offset {
                object_id: 4,
                source: Box::new(B5ExtrusionDirectrix::SurfaceCurve {
                    object_id: 2,
                    support: (7, 3, crate::test_support::test_b5::finite_pair([-3.0, 4.0])),
                    parameter_range: crate::test_support::test_b5::increasing([-3.0, 4.0]),
                }),
                source_parameter_range: crate::test_support::test_b5::increasing([-3.0, 4.0]),
                distance: crate::test_support::test_b5::finite(-1.5),
                direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
                parameter_range: crate::test_support::test_b5::increasing([-5.0, 6.0]),
            },
        })
    );

    let mut wrong_distance = offset_construction.clone();
    wrong_distance.distance = crate::test_support::test_b5::finite(-1.0);
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
    wrong_bounds.parameter_bounds[0] = crate::test_support::test_b5::increasing([-5.0, 7.0]);
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
    let pcurve0 = B5RecordBuf {
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
    let record = B5RecordBuf {
        class: 0x37,
        object_id: 7,
        payload,
        ..pcurve0.clone()
    };
    assert_eq!(
        parse_supported_surface(&record.record()),
        Some(B5SupportedSurface {
            object_id: 7,
            carrier_surface: 2,
            support_surfaces: [3, 4],
            support_pcurves: [5, 6],
            parameters: B5SupportedSurfaceParameters::Radius {
                controls: [0x09, 0x05, 0x03, 0x05, 0x01, 0x05],
                construction_radius: crate::test_support::test_b5::positive_length(2.5),
            },
        })
    );
    let mut scalar_pair_payload = vec![0x85, 0x82, 0x83, 0x84, 0x85, 0x86];
    scalar_pair_payload.extend_from_slice(&[0x09, 0x01, 0x01, 0x05, 0x05, 0x0d]);
    scalar_pair_payload.extend_from_slice(&101.6f64.to_le_bytes());
    scalar_pair_payload.extend_from_slice(&20.0f64.to_le_bytes());
    let scalar_pair = B5RecordBuf {
        class: 0x3b,
        payload: scalar_pair_payload,
        ..record.clone()
    };
    assert_eq!(
        parse_supported_surface(&scalar_pair.record()),
        Some(B5SupportedSurface {
            object_id: 7,
            carrier_surface: 2,
            support_surfaces: [3, 4],
            support_pcurves: [5, 6],
            parameters: B5SupportedSurfaceParameters::ScalarPair {
                controls: [0x09, 0x01, 0x01, 0x05, 0x05, 0x0d],
                scalars: [
                    crate::test_support::test_b5::positive(101.6),
                    crate::test_support::test_b5::positive(20.0)
                ],
            },
        })
    );
    let scalar_pair =
        parse_supported_surface(&scalar_pair.record()).expect("two-scalar supported surface");
    let plane = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    assert!(supported_surface_parameters_match_carrier(
        &scalar_pair.parameters,
        &plane
    ));
    let cone_parameters = B5SupportedSurfaceParameters::ScalarPair {
        controls: [0x05, 0x05, 0x01, 0x03, 0x05, 0x11],
        scalars: [
            crate::test_support::test_b5::positive(0.76),
            crate::test_support::test_b5::positive(std::f64::consts::FRAC_PI_4),
        ],
    };
    let cone = B5Surface::Cone {
        apex: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        direction_y: crate::test_support::test_b5::unit([0.0, 1.0, 0.0]),
        half_angle: crate::test_support::test_b5::positive_angle(std::f64::consts::FRAC_PI_4),
        reference_radius: crate::test_support::test_b5::finite(0.0),
        angular_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        slant_range: crate::test_support::test_b5::increasing([0.0, 1.0]),
        angular_scale: crate::test_support::test_b5::positive(1.0),
        angular_domain: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        surface: None,
    };
    assert!(supported_surface_parameters_match_carrier(
        &cone_parameters,
        &cone
    ));
    let wrong_cone = B5Surface::Cone {
        apex: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        direction_y: crate::test_support::test_b5::unit([0.0, 1.0, 0.0]),
        half_angle: crate::test_support::test_b5::positive_angle(std::f64::consts::FRAC_PI_6),
        reference_radius: crate::test_support::test_b5::finite(0.0),
        angular_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        slant_range: crate::test_support::test_b5::increasing([0.0, 1.0]),
        angular_scale: crate::test_support::test_b5::positive(1.0),
        angular_domain: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        surface: None,
    };
    assert!(!supported_surface_parameters_match_carrier(
        &cone_parameters,
        &wrong_cone
    ));
    let construction = parse_supported_surface(&record.record()).expect("supported surface");
    let pcurve0 = B5RecordBuf {
        object_id: 5,
        payload: vec![0x81, 0x83],
        ..pcurve0.clone()
    };
    let pcurve1 = B5RecordBuf {
        object_id: 6,
        payload: vec![0x81, 0x84],
        ..pcurve0.clone()
    };
    let records = HashMap::from([(5, &pcurve0), (6, &pcurve1)]);
    assert!(supported_surface_pcurves_match(
        &construction,
        &records,
        &BTreeMap::new()
    ));

    let wrong = B5RecordBuf {
        payload: vec![0x81, 0x82],
        ..pcurve1
    };
    assert!(!supported_surface_pcurves_match(
        &construction,
        &HashMap::from([(5, &pcurve0), (6, &wrong)]),
        &BTreeMap::new()
    ));
}

#[test]
fn supported_surface_parameter_matching_is_scale_independent() {
    let radius = 1e-200_f64;
    let parameters = B5SupportedSurfaceParameters::Radius {
        controls: [0; 6],
        construction_radius: crate::test_support::test_b5::positive_length(radius),
    };
    let cylinder = |carrier_radius| B5Surface::Cylinder {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        radius: crate::test_support::test_b5::positive_length(carrier_radius),
        u_range: crate::test_support::test_b5::increasing([
            0.0,
            std::f64::consts::TAU * carrier_radius,
        ]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        angular_scale: crate::test_support::test_b5::finite(carrier_radius),
        chart_origin: crate::test_support::test_b5::finite(0.0),
    };
    let torus = |carrier_radius| B5Surface::Torus {
        center: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        direction_y: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        major_radius: crate::test_support::test_b5::positive_length(1.0),
        minor_radius: crate::test_support::test_b5::positive_length(carrier_radius),
        major_angular_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        major_angular_domain: crate::test_support::test_b5::increasing([
            0.0,
            std::f64::consts::TAU,
        ]),
        minor_angular_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        minor_angular_domain: crate::test_support::test_b5::increasing([
            0.0,
            std::f64::consts::TAU,
        ]),
        major_scale: crate::test_support::test_b5::positive(1.0),
        minor_scale: crate::test_support::test_b5::positive(carrier_radius),
    };
    let sphere = |carrier_radius| B5Surface::Sphere {
        center: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        direction_y: crate::test_support::test_b5::unit([0.0, 1.0, 0.0]),
        radius: crate::test_support::test_b5::positive_length(1.0),
        azimuth_range: crate::test_support::test_b5::increasing([0.0, 1.0]),
        latitude_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        construction_radius: carrier_radius,
        chart_origin: crate::test_support::test_b5::finite(0.0),
    };

    for carrier in [
        cylinder(radius),
        torus(radius),
        sphere(crate::test_support::test_b5::positive_length(radius)),
    ] {
        assert!(supported_surface_parameters_match_carrier(
            &parameters,
            &carrier
        ));
    }
    for carrier in [
        cylinder(2.0 * radius),
        torus(2.0 * radius),
        sphere(crate::test_support::test_b5::positive_length(2.0 * radius)),
    ] {
        assert!(!supported_surface_parameters_match_carrier(
            &parameters,
            &carrier
        ));
    }

    let half_angle = 1e-200_f64;
    let cone_parameters = B5SupportedSurfaceParameters::ScalarPair {
        controls: [0; 6],
        scalars: [
            crate::test_support::test_b5::positive(1.0),
            crate::test_support::test_b5::positive(half_angle),
        ],
    };
    let cone = |carrier_half_angle| B5Surface::Cone {
        apex: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        direction_y: crate::test_support::test_b5::unit([0.0, 1.0, 0.0]),
        half_angle: carrier_half_angle,
        reference_radius: crate::test_support::test_b5::finite(0.0),
        angular_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        slant_range: crate::test_support::test_b5::increasing([0.0, 1.0]),
        angular_scale: crate::test_support::test_b5::positive(1.0),
        angular_domain: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        surface: None,
    };
    assert!(supported_surface_parameters_match_carrier(
        &cone_parameters,
        &cone(crate::test_support::test_b5::positive_angle(half_angle))
    ));
    assert!(!supported_surface_parameters_match_carrier(
        &cone_parameters,
        &cone(crate::test_support::test_b5::positive_angle(
            2.0 * half_angle
        ))
    ));
}

#[test]
fn a8_class21_strict_knot_refusal_stays_in_the_outer_result() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let payload = a8_class21_test_payload();
    let refused = crate::test_support::with_work_refusal("IR strict knot order", |ctx| {
        let result = parse_a8_class21_pcurve(ctx, 7, &payload);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    let Err(CodecError::ResourceLimit(original)) = refused else {
        panic!("strict knot refusal must not disappear as a missing candidate");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!(original.operation, "IR strict knot order");
}

mod topology_walk;

mod jet_admission;
