use crate::families::b5::graph::controls::{B5FramingControl, B5VertexIncidenceControl};
use crate::families::b5::graph::tests::extended_loop_metadata;
use crate::families::b5::graph::tests::test_pcurve;
use crate::families::b5::graph::vertex_refs::B5VertexRef;
use crate::families::b5::graph::{
    canonical_point, canonical_surface_id, counted_references, distance_squared,
    edge_support_pcurve_references, face_surface_references, incidence_vertex_coordinates,
    loop_chain_closes, loop_metadata as parse_loop_metadata, loop_references,
    loop_references_and_metadata, parameter_incidence, parse_face_record, parse_loop,
    parse_loop_record, pcurve_parameter_domain, sphere_great_circle_point,
    typed_face_records_from_records, typed_loop_records_from_records, B5FaceRecord,
    B5IncidenceLane, B5LogicalVertex, B5Loop, B5LoopMetadata, B5LoopMetadataExtension,
    B5OpaquePcurve, B5ParameterIncidence, B5Pcurve, B5PcurveContext, B5PcurveParameterization,
    B5Record, B5RecordBuf, B5SphereGreatCirclePcurve, B5Surface, B5VertexIncidenceLink,
};
use crate::families::b5::tests::test_loop_members;
use crate::families::b5::tests::test_loop_metadata;
use cadmpeg_ir::geometry::{nurbs::NurbsSurface, ProceduralSurfaceDefinition};
use std::collections::{BTreeMap, HashMap, HashSet};

fn parse_face(
    record: &B5FaceRecord,
    loops: &BTreeMap<u32, B5Loop>,
    surfaces: &BTreeMap<u32, B5Surface>,
    aliases: &BTreeMap<u32, u32>,
) -> Option<super::super::B5Face> {
    crate::test_support::with_service_context(|ctx| {
        super::super::parse_face(ctx, record, loops, surfaces, aliases)
    })
    .expect("service budget")
}

fn evaluate_pcurve(pcurve: &B5Pcurve, parameter: f64) -> Option<[f64; 2]> {
    crate::test_support::with_service_context(|ctx| {
        super::super::evaluate_pcurve(ctx, pcurve, parameter)
    })
    .expect("service budget")
}

fn pcurve_nurbs_knots(pcurve: &B5Pcurve) -> Option<Vec<cadmpeg_ir::scalar::FiniteReal>> {
    crate::test_support::with_service_context(|ctx| super::super::pcurve_nurbs_knots(ctx, pcurve))
        .expect("service budget")
}

fn pcurve_endpoints(
    pcurve_id: u32,
    edge_id: u32,
    geometry: &B5PcurveContext<'_>,
) -> Option<[cadmpeg_ir::features::FinitePoint3; 2]> {
    crate::test_support::with_service_context(|ctx| {
        super::super::pcurve_endpoints(ctx, pcurve_id, edge_id, geometry)
    })
    .expect("service budget")
}

fn lift_parameter_incidence(
    pcurve_id: u32,
    parameter: cadmpeg_ir::scalar::FiniteReal,
    geometry: &B5PcurveContext<'_>,
) -> Option<cadmpeg_ir::features::FinitePoint3> {
    crate::test_support::with_service_context(|ctx| {
        super::super::lift_parameter_incidence(ctx, pcurve_id, parameter, geometry)
    })
    .expect("service budget")
}

#[test]
fn pcurve_knot_expansion_and_evaluation_refuse_the_caller_limit() {
    let pcurve = test_pcurve(1, 2);
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::pcurve_nurbs_knots(ctx, &pcurve)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_expanded_pcurve_knots")
    );
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::evaluate_pcurve(ctx, &pcurve, 0.5)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_expanded_pcurve_knots")
    );
    assert_eq!(evaluate_pcurve(&pcurve, 0.5), Some([0.5, 0.0]));
    let domain_limited =
        crate::test_support::with_work_limit(0, |ctx| pcurve_parameter_domain(ctx, &pcurve));
    assert!(matches!(
        domain_limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_b5_pcurve_domain_knot_count"
    ));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| pcurve_parameter_domain(ctx, &pcurve))
            .expect("service budget"),
        Some([
            crate::test_support::test_b5::finite(0.0),
            crate::test_support::test_b5::finite(1.0),
        ])
    );
}

fn bind_edge_vertices(
    loops: &BTreeMap<u32, B5Loop>,
    geometry: &B5PcurveContext<'_>,
    points: &[cadmpeg_ir::features::FinitePoint3],
) -> BTreeMap<u32, [usize; 2]> {
    crate::test_support::with_service_context(|ctx| {
        super::super::bind_edge_vertices(ctx, loops, geometry, points)
    })
    .expect("service budget")
}

fn bind_native_vertices(
    loops: &BTreeMap<u32, B5Loop>,
    geometry: &B5PcurveContext<'_>,
    native_edges: &BTreeMap<u32, [u32; 2]>,
    geometric_edges: &BTreeMap<u32, [usize; 2]>,
    native_coordinates: &BTreeMap<u32, cadmpeg_ir::features::FinitePoint3>,
    points: &[cadmpeg_ir::features::FinitePoint3],
) -> super::super::BoundNativeVertices {
    crate::test_support::with_service_context(|ctx| {
        super::super::bind_native_vertices(
            ctx,
            loops,
            geometry,
            native_edges,
            geometric_edges,
            native_coordinates,
            points,
        )
    })
    .expect("service budget")
}

fn point_index(points: &[cadmpeg_ir::features::FinitePoint3]) -> HashMap<[i64; 3], Vec<usize>> {
    crate::test_support::with_service_context(|ctx| super::super::point_index(ctx, points))
        .expect("service budget")
}

#[test]
fn topology_binding_refuses_the_caller_collection_limit() {
    let point = crate::test_support::test_b5::point([0.0, 0.0, 0.0]);
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::point_index(ctx, &[point])
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_point_index_cells")
    );

    let loops = BTreeMap::new();
    let pcurves = BTreeMap::new();
    let opaque_pcurves = BTreeMap::new();
    let surfaces = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::bind_edge_vertices(ctx, &loops, &geometry, &[point])
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_point_index_cells")
    );

    let native = BTreeMap::from([(7, [11, 12])]);
    let geometric = BTreeMap::from([(7, [0, 0])]);
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::bind_native_vertices(
            ctx,
            &loops,
            &geometry,
            &native,
            &geometric,
            &BTreeMap::new(),
            &[point],
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_vertex_constraints")
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        super::super::bind_native_vertices(
            ctx,
            &loops,
            &geometry,
            &native,
            &geometric,
            &BTreeMap::new(),
            &[point],
        )
    })
    .is_ok());
}

fn merge_pcurve_candidate(
    pcurves: &mut BTreeMap<u32, B5Pcurve>,
    conflicts: &mut HashSet<u32>,
    candidate: B5Pcurve,
) {
    crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_b5_conflicts")?;
        super::super::merge_pcurve_candidate(ctx, pcurves, (conflicts, &mut scratch), candidate)
    })
    .expect("service budget");
}

fn merge_surface_candidate(
    surfaces: &mut BTreeMap<u32, B5Surface>,
    conflicts: &mut HashSet<u32>,
    object_id: u32,
    candidate: B5Surface,
) -> bool {
    crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_b5_conflicts")?;
        super::super::merge_surface_candidate(
            ctx,
            surfaces,
            (conflicts, &mut scratch),
            object_id,
            candidate,
        )
    })
    .expect("service budget")
}

/// Borrow each owned record under the same identity map.
fn record_views<'a>(by_id: &HashMap<u32, &'a B5RecordBuf>) -> HashMap<u32, B5Record<'a>> {
    by_id
        .iter()
        .map(|(&object_id, record)| (object_id, record.record()))
        .collect()
}

fn resolve_surface_aliases(
    records: &[B5RecordBuf],
    by_id: &HashMap<u32, &B5RecordBuf>,
    surfaces: &mut BTreeMap<u32, B5Surface>,
    conflicts: &mut HashSet<u32>,
) -> bool {
    let views = records.iter().map(B5RecordBuf::record).collect::<Vec<_>>();
    let aliases = views
        .iter()
        .filter(|record| super::super::surface_alias_target(record).is_some())
        .collect::<Vec<_>>();
    let by_id_views = record_views(by_id);
    let by_id = by_id_views
        .iter()
        .map(|(&object_id, record)| (object_id, record))
        .collect::<HashMap<_, _>>();
    crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_b5_conflicts")?;
        let terminals =
            super::super::surface_alias_terminals(ctx, &aliases, &by_id, None, &mut scratch)?
                .expect("no local ceiling");
        super::super::resolve_surface_aliases(
            ctx,
            &aliases,
            &terminals,
            surfaces,
            (conflicts, &mut scratch),
            None,
        )
        .map(|changed| changed.expect("no local ceiling"))
    })
    .expect("service budget")
}

fn surface_alias_carrier(
    object_id: u32,
    by_id: &HashMap<u32, &B5RecordBuf>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Surface> {
    let by_id_views = record_views(by_id);
    let by_id_refs = by_id_views
        .iter()
        .map(|(&object_id, record)| (object_id, record))
        .collect::<HashMap<_, _>>();
    crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_alias_terminals")?;
        let aliases = by_id_views
            .values()
            .filter(|record| super::super::surface_alias_target(record).is_some())
            .collect::<Vec<_>>();
        let terminals =
            super::super::surface_alias_terminals(ctx, &aliases, &by_id_refs, None, &mut scratch)?
                .expect("no local ceiling");
        super::super::resolved_surface_alias_terminal(ctx, object_id, &terminals, surfaces)
    })
    .expect("service budget")
    .map(|terminal| surfaces[&terminal].clone())
}

/// Borrow each indexed owned record.
fn targeted_record_views(
    records: &HashMap<u32, Option<B5RecordBuf>>,
) -> HashMap<u32, Option<B5Record<'_>>> {
    records
        .iter()
        .map(|(&object_id, record)| (object_id, record.as_ref().map(B5RecordBuf::record)))
        .collect()
}

fn resolve_targeted_surface(
    object_id: u32,
    records: &HashMap<u32, Option<B5RecordBuf>>,
    headers: &BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
    resolved: &HashMap<u32, Option<B5Surface>>,
    rolling: &HashMap<u32, Option<B5Surface>>,
) -> Option<B5Surface> {
    let records = targeted_record_views(records);
    crate::test_support::with_service_context(|ctx| {
        super::super::resolve_targeted_surface(ctx, object_id, &records, headers, resolved, rolling)
    })
    .expect("service budget")
}

#[test]
fn targeted_surface_records_and_resolution_refuse_caller_limits() {
    let mut bytes = Vec::new();
    crate::test_support::test_b5::append_b5_record(&mut bytes, 0x27, 9, &[0x80]);
    let frames = crate::test_support::with_service_context(|ctx| {
        super::super::collect_object_stream_frames(ctx, &bytes)
    })
    .expect("service budget");
    let object_ids = std::collections::BTreeSet::from([9]);
    let mut refused = std::collections::BTreeSet::new();
    for limit in 0..16 {
        if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) =
            crate::test_support::with_collection_limit(limit, |ctx| {
                super::super::targeted_surfaces_from_frames(
                    ctx,
                    &bytes,
                    &object_ids,
                    &frames,
                    &mut crate::nurbs::LaneRefusals::new(),
                )
            })
        {
            refused.insert(error.operation);
        }
    }
    for operation in [
        "catia_b5_targeted_surface_records",
        "catia_b5_targeted_surface_visited",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
    let surface = B5Surface::Unknown {
        family: 0xb5,
        class: 0x27,
        payload: vec![1, 2, 3],
    };
    let candidate = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::merge_targeted_surface(ctx, &mut HashMap::new(), 9, surface.clone())
    });
    assert!(
        matches!(candidate, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_targeted_surface_candidates")
    );
    let copied = crate::test_support::with_retained_limit(2, |ctx| {
        super::super::copy_surface(ctx, &surface)
    });
    assert!(
        matches!(copied, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_copied_unknown_surface_payload")
    );
    let resolved = HashMap::from([(9, Some(surface.clone()))]);
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::resolve_targeted_surface(
            ctx,
            9,
            &HashMap::new(),
            &BTreeMap::new(),
            &resolved,
            &HashMap::new(),
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_targeted_surface_visited")
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::resolve_targeted_surface(
                ctx,
                9,
                &HashMap::new(),
                &BTreeMap::new(),
                &resolved,
                &HashMap::new(),
            )
        })
        .expect("service budget"),
        Some(surface)
    );
}

fn loop_metadata(bytes: &[u8], edge_count: usize) -> Option<(B5LoopMetadata, Vec<[i16; 3]>)> {
    crate::test_support::with_service_context(|ctx| {
        parse_loop_metadata(ctx, bytes, edge_count).expect("service budget")
    })
}

#[test]
fn loop_metadata_accepts_exact_base_and_extended_forms() {
    let base = [
        0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff,
        0x01,
    ];
    assert_eq!(
        loop_metadata(&base, 2),
        Some((
            B5LoopMetadata {
                framing_controls: [B5FramingControl::Control05; 2],
                extension: None,
            },
            vec![[1, -1, 1], [-1, 1, -1]],
        ))
    );

    for metadata_control in [0x05, 0x09, 0x21, 0x41, 0x71] {
        let extended = extended_loop_metadata(metadata_control);
        let (metadata, edge_controls) =
            loop_metadata(&extended, 1).expect("complete extended metadata");
        assert_eq!(
            metadata.framing_controls.map(B5FramingControl::as_byte),
            [0x03, 0x05]
        );
        assert_eq!(edge_controls, [[1, -1, 1]]);
        assert_eq!(
            metadata.extension,
            Some(B5LoopMetadataExtension {
                scalars: [
                    crate::test_support::test_b5::finite(1.0),
                    crate::test_support::test_b5::finite(-2.0),
                    crate::test_support::test_b5::finite(3.5),
                    crate::test_support::test_b5::finite(4.25)
                ],
                control: metadata_control,
                floats: [1.0, -2.0, 3.5, 4.25, 5.5, -6.75],
            })
        );
    }

    let alternate_framing_control = [
        0x05, 0x03, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff,
        0x01,
    ];
    let (metadata, edge_controls) =
        loop_metadata(&alternate_framing_control, 2).expect("alternate framing control");
    assert_eq!(
        metadata.framing_controls.map(B5FramingControl::as_byte),
        [0x05, 0x03]
    );
    assert_eq!(edge_controls, [[1, -1, 1], [-1, 1, -1]]);
    assert_eq!(metadata.extension, None);
}

#[test]
fn loop_references_require_exact_matching_edge_count_and_metadata() {
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x62,
        object_id: 400,
        payload: vec![
            0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
            0x01,
        ],
    };
    let (references, _, edge_controls) = crate::test_support::with_service_context(|ctx| {
        loop_references_and_metadata(ctx, &record.record()).expect("service budget")
    })
    .expect("exact loop payload");
    assert_eq!(references, [9, 10, 11]);
    assert_eq!(edge_controls, [[1, -1, 1]]);

    let mut mismatched = record.clone();
    mismatched.payload[4] = 0x82;
    assert!(loop_references(&mismatched.record()).is_none());

    let mut residual = record;
    residual.payload.push(0);
    assert!(loop_references(&residual.record()).is_none());
}

#[test]
fn loop_record_nested_collections_refuse_each_limit() {
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x62,
        object_id: 400,
        payload: vec![
            0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
            0x01,
        ],
    };
    for (limit, operation) in [
        (2, "catia_b5_loop_references"),
        (3, "catia_b5_loop_edge_controls"),
        (4, "catia_b5_loop_members"),
    ] {
        let limited = crate::test_support::with_collection_limit(limit, |ctx| {
            parse_loop_record(ctx, &record.record())
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == operation)
        );
    }
    let limited_index = crate::test_support::with_collection_limit(5, |ctx| {
        typed_loop_records_from_records(ctx, &[record.record()])
    });
    assert!(
        matches!(limited_index, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_typed_loop_records")
    );
    let parsed = crate::test_support::with_service_context(|ctx| {
        typed_loop_records_from_records(ctx, &[record.record()])
    })
    .expect("service budget");
    assert_eq!(parsed.get(&400).map(|loop_| loop_.members.len()), Some(1));
}

#[test]
fn loop_metadata_rejects_every_malformed_boundary_and_numeric_domain() {
    assert!(loop_metadata(&[], 0).is_none());
    assert!(loop_metadata(&[0x05, 0x05, 0x03], 0).is_none());
    assert!(loop_metadata(&[0x05, 0x05, 0x03, 0x01, 0x00, 0x01], 0).is_none());
    assert!(loop_metadata(&[0x09, 0x05, 0x03, 0x01], 0).is_none());
    assert!(loop_metadata(&[0x05, 0x03, 0x05, 0x01], 0).is_none());
    assert!(loop_metadata(
        &[0x05, 0x05, 0x03, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01],
        1
    )
    .is_none());

    for metadata_control in [0x00, 0x04, 0x20, 0x70] {
        assert!(loop_metadata(&extended_loop_metadata(metadata_control), 1).is_none());
    }

    let mut non_finite_scalar = extended_loop_metadata(0x05);
    non_finite_scalar[10..18].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(loop_metadata(&non_finite_scalar, 1).is_none());

    let mut non_finite_float = extended_loop_metadata(0x05);
    non_finite_float[47..51].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert!(loop_metadata(&non_finite_float, 1).is_none());
}

#[test]
fn pcurve_candidate_merge_collapses_repeats_and_permanently_rejects_conflicts() {
    let mut pcurves = BTreeMap::new();
    let mut conflicts = HashSet::new();
    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
    assert_eq!(pcurves.get(&1), Some(&test_pcurve(1, 10)));

    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 11));
    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
    assert!(!pcurves.contains_key(&1));
    assert!(conflicts.contains(&1));
}

#[test]
fn loop_rejects_a_pcurve_bound_to_another_surface() {
    let loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[2], &[3]),
        metadata: test_loop_metadata(),
        surface: 10,
    };
    let edge = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5e,
        object_id: 3,
        payload: Vec::new(),
    };
    let records = HashMap::from([(3, &edge)]);
    let pcurves = BTreeMap::from([(2, test_pcurve(2, 11))]);
    let surfaces = BTreeMap::from([(
        10,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x27,
            payload: Vec::new(),
        },
    )]);

    let record_views = record_views(&records);
    let record_refs = record_views
        .iter()
        .map(|(&object_id, record)| (object_id, record))
        .collect::<HashMap<_, _>>();
    let parsed = crate::test_support::with_service_context(|ctx| {
        parse_loop(
            ctx,
            loop_.clone(),
            &record_refs,
            &pcurves,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &surfaces,
        )
    })
    .expect("service budget");
    assert!(parsed.is_none());

    let limited =
        crate::test_support::with_work_refusal("catia_b5_parse_loop_member_scan", |ctx| {
            parse_loop(
                ctx,
                loop_.clone(),
                &record_refs,
                &pcurves,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &surfaces,
            )
        });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_b5_parse_loop_member_scan"
    ));
}

#[test]
fn b5_candidate_indexes_and_alias_walk_refuse_collection_limits() {
    let pcurve = test_pcurve(1, 10);
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::merge_pcurve_candidate(
            ctx,
            &mut BTreeMap::new(),
            (
                &mut HashSet::new(),
                &mut ctx.reserve_scoped(0, "test_b5_conflicts")?,
            ),
            pcurve.clone(),
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_pcurve_candidates")
    );
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::merge_pcurve_candidate(
            ctx,
            &mut BTreeMap::from([(1, pcurve.clone())]),
            (
                &mut HashSet::new(),
                &mut ctx.reserve_scoped(0, "test_b5_conflicts")?,
            ),
            test_pcurve(1, 11),
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_conflicting_pcurves")
    );
    let plane = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::merge_surface_candidate(
            ctx,
            &mut BTreeMap::new(),
            (
                &mut HashSet::new(),
                &mut ctx.reserve_scoped(0, "test_b5_conflicts")?,
            ),
            1,
            plane.clone(),
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_surface_candidates")
    );
    let mut other = plane.clone();
    if let B5Surface::Plane { origin, .. } = &mut other {
        *origin = crate::test_support::test_b5::point([1.0, 0.0, 0.0]);
    }
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::merge_surface_candidate(
            ctx,
            &mut BTreeMap::from([(1, plane.clone())]),
            (
                &mut HashSet::new(),
                &mut ctx.reserve_scoped(0, "test_b5_conflicts")?,
            ),
            1,
            other.clone(),
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_conflicting_surfaces")
    );
    let surfaces = BTreeMap::from([(
        1,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x27,
            payload: vec![1, 2, 3],
        },
    )]);
    let limited = crate::test_support::with_work_refusal("catia_b5_surface_alias_step", |ctx| {
        super::super::resolved_surface_alias_terminal(ctx, 1, &HashMap::new(), &surfaces)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_surface_alias_step")
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        super::super::merge_surface_candidate(
            ctx,
            &mut BTreeMap::new(),
            (
                &mut HashSet::new(),
                &mut ctx.reserve_scoped(0, "test_b5_conflicts")?,
            ),
            1,
            plane,
        )
    })
    .expect("service budget"));
}

#[test]
fn loop_edge_senses_refuse_the_caller_collection_limit() {
    let loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[2, 3], &[4, 5]),
        metadata: test_loop_metadata(),
        surface: 10,
    };
    let edges = crate::test_support::with_collection_limit(1, |ctx| loop_.edge_senses(ctx));
    assert!(
        matches!(edges, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_loop_edge_senses")
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| loop_.edge_senses(ctx))
            .expect("service budget")
            .len(),
        2
    );
}

#[test]
fn pcurve_requires_one_complete_clamped_bezier_frame() {
    let payload = crate::test_support::test_b5::b5_linear_pcurve_payload(1, [0.0, 0.0], [1.0, 0.0]);
    let record = |payload| B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x21,
        object_id: 2,
        payload,
    };
    let parse_pcurve = |record: &B5RecordBuf| {
        crate::test_support::with_service_context(|ctx| {
            super::super::parse_pcurve(ctx, &record.record())
        })
        .expect("service budget")
    };
    assert_eq!(
        parse_pcurve(&record(payload.clone()))
            .expect("complete class-21 pcurve")
            .class_21_suffix_scalar,
        Some(crate::test_support::test_b5::positive(1.0))
    );
    let tail = payload.len() - 36;
    let mut alternate_scalar = payload.clone();
    alternate_scalar[tail + 10..tail + 18].copy_from_slice(&2.5_f64.to_le_bytes());
    assert!(parse_pcurve(&record(alternate_scalar.clone())).is_none());
    let base = parse_pcurve(&record(payload.clone())).expect("base pcurve");
    for parameter in [0.0, 0.25, 0.5, 1.0] {
        assert_eq!(
            evaluate_pcurve(&base, parameter),
            Some([parameter, 0.0]),
            "zero-origin class-21 pcurve evaluates at its local station {parameter}"
        );
    }
    let mut wrong_family = record(payload.clone());
    wrong_family.family = 0xa8;
    assert!(parse_pcurve(&wrong_family).is_none());

    for (offset, value) in [(6, 0), (7, 0), (8, 0x0d), (9, 0x08), (26, 0x0d)] {
        let mut malformed = payload.clone();
        malformed[offset] = value;
        assert!(parse_pcurve(&record(malformed)).is_none());
    }

    let mut truncated = payload.clone();
    truncated.pop();
    assert!(parse_pcurve(&record(truncated)).is_none());

    let mut residual = payload.clone();
    residual.push(0);
    assert!(parse_pcurve(&record(residual)).is_none());

    for (offset, value) in [
        (tail + 2, 1.0_f64),
        (tail + 10, 0.0),
        (tail + 18, 0.0),
        (tail + 26, 1.0),
    ] {
        let mut malformed = payload.clone();
        malformed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(parse_pcurve(&record(malformed)).is_none());
    }

    let mut non_finite = payload;
    non_finite[tail + 10..tail + 18].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(parse_pcurve(&record(non_finite)).is_none());
}

#[test]
fn class21_pcurve_lanes_and_typed_index_refuse_collection_limit() {
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x21,
        object_id: 2,
        payload: crate::test_support::test_b5::b5_linear_pcurve_payload(1, [0.0, 0.0], [1.0, 0.0]),
    };
    for (limit, operation) in [
        (1, "catia_b5_class21_distinct_knots"),
        (2, "catia_b5_class21_multiplicities"),
        (4, "catia_b5_class21_control_points"),
        (6, "catia_b5_typed_class21_pcurves"),
    ] {
        let limited = crate::test_support::with_collection_limit(limit, |ctx| {
            super::super::typed_class_21_pcurves_from_records(ctx, &[record.record()])
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == operation)
        );
    }
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::super::typed_class_21_pcurves_from_records(ctx, &[record.record()])
        })
        .expect("service budget")
        .len(),
        1
    );
}

#[test]
fn class21_pcurve_multiplicity_range_collector_preserves_work_refusal() {
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x21,
        object_id: 2,
        payload: crate::test_support::test_b5::b5_linear_pcurve_payload(1, [0.0, 0.0], [1.0, 0.0]),
    };
    let service = crate::test_support::with_service_context(|ctx| {
        super::super::parse_pcurve(ctx, &record.record())
    })
    .expect("service pcurve budget");
    let pcurve = service.expect("service fixture produces a pcurve");
    assert_eq!(pcurve.multiplicities, [2, 2]);

    let refused =
        crate::test_support::with_work_refusal("catia_b5_class21_multiplicities", |ctx| {
            let result = super::super::parse_pcurve(ctx, &record.record());
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        });
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "catia_b5_class21_multiplicities"
    ));
}

#[test]
fn class21_pcurve_rebases_nonzero_origin_to_zero_based_stations() {
    let parse_pcurve = |record: &B5RecordBuf| {
        crate::test_support::with_service_context(|ctx| {
            super::super::parse_pcurve(ctx, &record.record())
        })
        .expect("service budget")
    };
    let payload = crate::test_support::test_b5::b5_linear_pcurve_payload_with_knots(
        7,
        [10.0, 20.0],
        [0.0, 0.0],
        [1.0, 0.0],
    );
    let pcurve = parse_pcurve(&B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x21,
        object_id: 2,
        payload,
    })
    .expect("translated class-21 pcurve");

    assert_eq!(
        pcurve.parameterization,
        B5PcurveParameterization::Translated {
            native_origin: crate::test_support::test_b5::finite(10.0),
        }
    );
    assert_eq!(
        pcurve.distinct_knots,
        crate::test_support::test_b5::finite_lane(&[10.0, 20.0])
    );
    assert_eq!(
        pcurve.class_21_suffix_scalar,
        Some(crate::test_support::test_b5::positive(10.0))
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| pcurve_parameter_domain(ctx, &pcurve))
            .expect("service budget"),
        Some(crate::test_support::test_b5::finite_pair([0.0, 10.0]))
    );
    assert_eq!(
        pcurve_nurbs_knots(&pcurve),
        Some(crate::test_support::test_b5::finite_lane(&[
            0.0, 0.0, 10.0, 10.0
        ]))
    );
    assert_eq!(evaluate_pcurve(&pcurve, 0.0), Some([0.0, 0.0]));
    assert_eq!(evaluate_pcurve(&pcurve, 10.0), Some([1.0, 0.0]));

    let pcurves = BTreeMap::from([(2, pcurve)]);
    let surfaces = BTreeMap::from([(
        7,
        B5Surface::Plane {
            origin: crate::test_support::test_b5::point([0.0, 0.0, 0.0]),
            frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
            u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
            v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        },
    )]);
    let opaque_pcurves = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    assert_eq!(
        lift_parameter_incidence(2, crate::test_support::test_b5::finite(0.0), &geometry),
        Some(crate::test_support::test_b5::point([0.0, 0.0, 0.0]))
    );
    assert_eq!(
        lift_parameter_incidence(2, crate::test_support::test_b5::finite(10.0), &geometry),
        Some(crate::test_support::test_b5::point([1.0, 0.0, 0.0]))
    );
}

#[test]
fn surface_candidate_merge_refines_opaque_wrappers_and_rejects_exact_conflicts() {
    let unknown = B5Surface::Unknown {
        family: 0xb5,
        class: 0x2e,
        payload: vec![0x81, 0x82],
    };
    let plane = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    let cylinder = B5Surface::Cylinder {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        radius: crate::test_support::test_b5::positive_length(1.0),
        u_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        angular_scale: crate::test_support::test_b5::finite(1.0),
        chart_origin: crate::test_support::test_b5::finite(0.0),
    };
    let mut surfaces = BTreeMap::from([(1, unknown)]);
    let mut conflicts = HashSet::new();
    assert!(merge_surface_candidate(
        &mut surfaces,
        &mut conflicts,
        1,
        plane.clone(),
    ));
    assert_eq!(surfaces.get(&1), Some(&plane));

    assert!(!merge_surface_candidate(
        &mut surfaces,
        &mut conflicts,
        1,
        cylinder,
    ));
    assert!(!merge_surface_candidate(
        &mut surfaces,
        &mut conflicts,
        1,
        plane,
    ));
    assert!(!surfaces.contains_key(&1));
    assert!(conflicts.contains(&1));
}

#[test]
fn full_surface_alias_closure_is_order_independent_unbounded_and_cycle_safe() {
    let alias = |object_id: u32, target: u32| B5RecordBuf {
        offset: usize::try_from(object_id).expect("small object id"),
        family: 0xb5,
        class: 0x2e,
        object_id,
        payload: vec![0x81, 0x80 + u8::try_from(target).expect("compact target")],
    };
    let mut records = (1..30)
        .rev()
        .map(|object_id| alias(object_id, object_id + 1))
        .collect::<Vec<_>>();
    let cycle_start = records.len();
    records.push(alias(40, 41));
    records.push(alias(41, 40));
    let by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect::<HashMap<_, _>>();
    let plane = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    let mut surfaces = BTreeMap::from([(30, plane.clone())]);
    let mut conflicts = HashSet::new();
    assert!(resolve_surface_aliases(
        &records,
        &by_id,
        &mut surfaces,
        &mut conflicts,
    ));
    assert_eq!(surfaces.get(&1), Some(&plane));
    assert_eq!(surfaces.get(&29), Some(&plane));
    assert_eq!(
        surface_alias_carrier(records[cycle_start].object_id, &by_id, &surfaces),
        None
    );
    assert!(!surfaces.contains_key(&40));
    assert!(!surfaces.contains_key(&41));
}

#[test]
fn canonical_surface_identity_follows_unbounded_aliases_and_rejects_cycles() {
    let aliases = (1..30)
        .map(|object_id| (object_id, object_id + 1))
        .chain([(40, 41), (41, 40)])
        .collect();
    let identities = crate::test_support::with_service_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>([
            canonical_surface_id(ctx, &aliases, 1)?,
            canonical_surface_id(ctx, &aliases, 29)?,
            canonical_surface_id(ctx, &aliases, 30)?,
            canonical_surface_id(ctx, &aliases, 40)?,
            canonical_surface_id(ctx, &aliases, 41)?,
        ])
    })
    .expect("service alias traversal budget");

    assert_eq!(identities, [Some(30), Some(30), Some(30), None, None]);
    let limited =
        crate::test_support::with_work_limit(0, |ctx| canonical_surface_id(ctx, &aliases, 1));
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_b5_surface_alias_step"
    ));
}

#[test]
fn surface_alias_closes_after_its_terminal_construction_resolves() {
    let records = vec![B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x2e,
        object_id: 1,
        payload: vec![0x81, 0x82],
    }];
    let by_id = HashMap::from([(1, &records[0])]);
    let mut surfaces = BTreeMap::new();
    let mut conflicts = HashSet::new();
    assert!(!resolve_surface_aliases(
        &records,
        &by_id,
        &mut surfaces,
        &mut conflicts,
    ));

    let plane = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    surfaces.insert(2, plane.clone());
    assert!(resolve_surface_aliases(
        &records,
        &by_id,
        &mut surfaces,
        &mut conflicts,
    ));
    assert_eq!(surfaces.get(&1), Some(&plane));
}

#[test]
fn targeted_surface_resolution_follows_a_supported_surface_to_a_rolling_ball_carrier() {
    let mut payload = vec![0x85];
    for reference in 1u16..=5 {
        payload.push(0x18);
        payload.extend_from_slice(&reference.to_le_bytes());
    }
    payload.extend_from_slice(&[0x01, 0x02]);
    payload.extend_from_slice(&2.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x03, 0x04]);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x05, 0x06]);
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x37,
        object_id: 10,
        payload,
    };
    let records = HashMap::from([(10, Some(record))]);
    let rolling = HashMap::from([(
        1,
        Some(B5Surface::RollingBall {
            carrier_object_id: 1,
            definition: Box::new(ProceduralSurfaceDefinition::Unknown {
                record: None,
                cache: None,
            }),
        }),
    )]);
    assert_eq!(
        resolve_targeted_surface(10, &records, &BTreeMap::new(), &HashMap::new(), &rolling),
        rolling.get(&1).cloned().flatten()
    );
}

#[test]
fn targeted_surface_resolution_validates_an_analytic_offset_carrier() {
    let carrier = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    let source = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0, 0.0, 0.5]),
        frame: crate::test_support::test_b5::plane_frame([0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([-1.0, 0.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    let mut payload = vec![0x82, 0x82, 0x83];
    payload.extend_from_slice(&(-0.5f64).to_le_bytes());
    payload.push(0x15);
    for value in [-2.0f64, 3.0, -4.0, 5.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let offset = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x30,
        object_id: 9,
        payload,
    };
    let records = HashMap::from([(9, Some(offset.clone()))]);
    let resolved = HashMap::from([(2, Some(carrier.clone())), (3, Some(source))]);

    assert_eq!(
        resolve_targeted_surface(9, &records, &BTreeMap::new(), &resolved, &HashMap::new(),),
        Some(carrier)
    );
    let record_views = targeted_record_views(&records);
    let mut refused = std::collections::BTreeSet::new();
    for limit in 0..20 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            super::super::resolve_targeted_surface(
                ctx,
                9,
                &record_views,
                &BTreeMap::new(),
                &resolved,
                &HashMap::new(),
            )
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                refused.insert(error.operation);
            }
            Ok(Some(_)) => break,
            other => panic!("unexpected offset resolution result: {other:?}"),
        }
    }
    for operation in [
        "catia_b5_targeted_surface_visited",
        "catia_b5_targeted_visited_copy",
        "catia_b5_targeted_offset_surfaces",
    ] {
        assert!(
            refused.contains(operation),
            "missing refusal at {operation}"
        );
    }

    let mut wrong_distance = offset.clone();
    wrong_distance.payload[3..11].copy_from_slice(&(-0.25f64).to_le_bytes());
    assert!(resolve_targeted_surface(
        9,
        &HashMap::from([(9, Some(wrong_distance))]),
        &BTreeMap::new(),
        &resolved,
        &HashMap::new(),
    )
    .is_none());
    let mut wrong_kind = offset;
    wrong_kind.payload[11] = 0x05;
    assert!(resolve_targeted_surface(
        9,
        &HashMap::from([(9, Some(wrong_kind))]),
        &BTreeMap::new(),
        &resolved,
        &HashMap::new(),
    )
    .is_none());
}

#[test]
fn targeted_surface_resolution_has_no_alias_depth_limit() {
    let records = (1u32..=20)
        .map(|object_id| {
            (
                object_id,
                Some(B5RecordBuf {
                    offset: usize::try_from(object_id).expect("small object id"),
                    family: 0xb5,
                    class: 0x2e,
                    object_id,
                    payload: vec![
                        0x81,
                        0x80 + u8::try_from(object_id + 1).expect("compact target"),
                    ],
                }),
            )
        })
        .collect();
    let rolling = HashMap::from([(
        21,
        Some(B5Surface::RollingBall {
            carrier_object_id: 21,
            definition: Box::new(ProceduralSurfaceDefinition::Unknown {
                record: None,
                cache: None,
            }),
        }),
    )]);
    assert_eq!(
        resolve_targeted_surface(1, &records, &BTreeMap::new(), &HashMap::new(), &rolling),
        rolling.get(&21).cloned().flatten()
    );
}

#[test]
fn targeted_surface_resolution_rejects_alias_cycles() {
    let alias = |object_id, target| {
        Some(B5RecordBuf {
            offset: usize::try_from(object_id).expect("small object id"),
            family: 0xb5,
            class: 0x2e,
            object_id,
            payload: vec![0x81, 0x80 + u8::try_from(target).expect("compact target")],
        })
    };
    let records = HashMap::from([(1, alias(1, 2)), (2, alias(2, 1))]);
    assert!(resolve_targeted_surface(
        1,
        &records,
        &BTreeMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    )
    .is_none());
}

#[test]
fn targeted_surface_resolution_rejects_conflicting_exact_carriers() {
    let rolling = HashMap::from([(
        1,
        Some(B5Surface::RollingBall {
            carrier_object_id: 1,
            definition: Box::new(ProceduralSurfaceDefinition::Unknown {
                record: None,
                cache: None,
            }),
        }),
    )]);
    let resolved = HashMap::from([(
        1,
        Some(B5Surface::Nurbs(
            NurbsSurface::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    [cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 4]
                        .chunks(2_usize)
                        .map(<[_]>::to_vec)
                        .collect(),
                    None,
                ),
                false,
            )
            .expect("fixture constructor admission")
            .expect("valid bilinear NURBS"),
        )),
    )]);
    assert!(
        resolve_targeted_surface(1, &HashMap::new(), &BTreeMap::new(), &resolved, &rolling,)
            .is_none()
    );
}

#[test]
fn requested_edge_support_scan_closes_through_its_unique_wrapper() {
    let mut bytes = Vec::new();
    let append = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
        bytes.extend_from_slice(&[
            0xb5,
            0x03,
            class,
            u8::try_from(payload.len()).expect("fixture value fits u8"),
        ]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    };
    append(
        &mut bytes,
        0x23,
        20,
        &[0x82, 0x18, 30, 0, 0x18, 31, 0, 0x01],
    );
    append(
        &mut bytes,
        0x5e,
        40,
        &[
            0x85, 0x18, 20, 0, 0x18, 1, 0, 0x18, 2, 0, 0x18, 3, 0, 0x18, 4, 0, 0x22,
        ],
    );
    let (supported, absent) = crate::test_support::with_service_context(|ctx| {
        let supported = edge_support_pcurve_references(ctx, &bytes, &HashSet::from([40]))?;
        let absent = edge_support_pcurve_references(ctx, &bytes, &HashSet::from([41]))?;
        Ok::<_, cadmpeg_core::CodecError>((supported, absent))
    })
    .expect("service edge-support scan budget");
    assert_eq!(supported, BTreeMap::from([(40, [30, 31])]));
    assert!(absent.is_empty());
}

#[test]
fn face_surface_references_do_not_require_resolved_loops() {
    let mut bytes = Vec::new();
    for (object_id, surface_id) in [(500u32, 100u8), (501, 100), (500, 101)] {
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5f, 5]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(&[0x82, 0x08, surface_id, 0x00, 0x05]);
    }
    assert_eq!(
        face_surface_references(&bytes),
        vec![(500, 100), (501, 100), (500, 101)]
    );
}

#[test]
fn counted_face_references_accept_both_exact_terminal_controls() {
    for terminal_control in [0x03, 0x05] {
        let record = B5RecordBuf {
            offset: 0,
            family: 0xb5,
            class: 0x5f,
            object_id: 3,
            payload: vec![0x82, 0x81, 0x82, terminal_control],
        };
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                parse_face_record(ctx, &record.record()).expect("service budget")
            }),
            Some(B5FaceRecord {
                object_id: 3,
                references: vec![1, 2],
                terminal_control: Some(
                    B5FramingControl::from_byte(terminal_control)
                        .expect("declared terminal control")
                ),
            })
        );

        let mut overlong = record;
        overlong.payload.push(terminal_control);
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                parse_face_record(ctx, &overlong.record()).expect("service budget")
            }),
            None
        );
    }
}

#[test]
fn counted_face_references_reject_unknown_terminal_controls() {
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5f,
        object_id: 3,
        payload: vec![0x82, 0x81, 0x82, 0x04],
    };
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            parse_face_record(ctx, &record.record()).expect("service budget")
        }),
        None
    );

    let mut empty = record;
    empty.payload.clear();
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            parse_face_record(ctx, &empty.record()).expect("service budget")
        }),
        None
    );
    empty.payload.extend_from_slice(&[0x80, 0x03]);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            parse_face_record(ctx, &empty.record()).expect("service budget")
        }),
        None
    );
}

#[test]
fn counted_face_reference_list_refuses_collection_limit() {
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5f,
        object_id: 3,
        payload: vec![0x82, 0x81, 0x82, 0x03],
    };
    let limited = crate::test_support::with_collection_limit(1, |ctx| {
        parse_face_record(ctx, &record.record())
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    let parsed =
        crate::test_support::with_service_context(|ctx| parse_face_record(ctx, &record.record()))
            .expect("service budget");
    assert_eq!(
        parsed.as_ref().map(|face| face.references.as_slice()),
        Some([1, 2].as_slice())
    );
}

#[test]
fn typed_face_record_index_refuses_collection_limit() {
    let records = [B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5f,
        object_id: 3,
        payload: vec![0x82, 0x81, 0x82, 0x03],
    }];
    let records = records.each_ref().map(B5RecordBuf::record);
    let limited = crate::test_support::with_collection_limit(2, |ctx| {
        typed_face_records_from_records(ctx, &records)
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    let parsed = crate::test_support::with_service_context(|ctx| {
        typed_face_records_from_records(ctx, &records)
    })
    .expect("service budget");
    assert_eq!(
        parsed.get(&3).map(|face| face.references.as_slice()),
        Some([1, 2].as_slice())
    );
}

#[test]
fn face_references_can_repeat_one_carrier_through_an_alias() {
    let plane = B5Surface::Plane {
        origin: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
        u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
    };
    let record = B5FaceRecord {
        object_id: 30,
        references: vec![10, 11, 20],
        terminal_control: Some(B5FramingControl::Control05),
    };
    let loops = BTreeMap::from([(
        20,
        B5Loop {
            object_id: 20,
            members: Vec::new(),
            metadata: test_loop_metadata(),
            surface: 10,
        },
    )]);
    let surfaces = BTreeMap::from([(10, plane.clone()), (11, plane)]);
    let aliases = BTreeMap::from([(11, 10)]);

    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::parse_face(ctx, &record, &loops, &surfaces, &aliases)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_face_loop_ids")
    );

    let face = parse_face(&record, &loops, &surfaces, &aliases).expect("aliased face");
    assert_eq!(face.surface, 10);
    assert_eq!(face.loops, vec![20]);

    assert!(parse_face(&record, &loops, &surfaces, &BTreeMap::new()).is_none());
}

#[test]
fn one_edge_loop_closes_on_one_native_vertex() {
    let loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[2], &[3]),
        metadata: test_loop_metadata(),
        surface: 4,
    };

    crate::test_support::with_service_context(|ctx| {
        assert!(loop_chain_closes(
            ctx,
            &loop_,
            &BTreeMap::from([(3, [B5VertexRef::Raw(0); 2])])
        )
        .expect("service budget"));
        assert!(!loop_chain_closes(
            ctx,
            &loop_,
            &BTreeMap::from([(3, [B5VertexRef::Raw(0), B5VertexRef::Raw(1)])])
        )
        .expect("service budget"));
    });
}

#[test]
fn loop_chain_requires_each_source_native_edge_sense() {
    let mut loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[4, 5, 6], &[1, 2, 3]),
        metadata: test_loop_metadata(),
        surface: 7,
    };
    loop_.members[1].controls[0] = -1;
    let edge_vertices = BTreeMap::from(
        [(1, [0, 1]), (2, [2, 1]), (3, [2, 0])]
            .map(|(edge, vertices)| (edge, vertices.map(B5VertexRef::Raw))),
    );
    crate::test_support::with_service_context(|ctx| {
        assert!(loop_chain_closes(ctx, &loop_, &edge_vertices).expect("service budget"));
    });

    loop_.members[1].controls[0] = 1;
    crate::test_support::with_service_context(|ctx| {
        assert!(!loop_chain_closes(ctx, &loop_, &edge_vertices).expect("service budget"));
    });
}

#[test]
fn opaque_pcurve_occurrences_defer_endpoint_binding_to_native_edges() {
    let loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[2], &[3]),
        metadata: test_loop_metadata(),
        surface: 4,
    };
    let pcurves = BTreeMap::new();
    let opaque_pcurves = BTreeMap::new();
    let surfaces = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    assert_eq!(
        bind_edge_vertices(&BTreeMap::from([(1, loop_)]), &geometry, &[],),
        BTreeMap::new()
    );
}

#[test]
fn sphere_great_circle_pcurve_binds_endpoint_rows() {
    let chart_scale = 8.0;
    let parameter_end = chart_scale * std::f64::consts::FRAC_PI_2;
    let pcurve = B5OpaquePcurve {
        object_id: 2,
        surface: 4,
        class: 0x1d,
        payload: Vec::new(),
        sphere_great_circle: Some(B5SphereGreatCirclePcurve {
            u_bounds: crate::test_support::test_b5::increasing([0.0, parameter_end]),
            v_bounds: crate::test_support::test_b5::finite_pair([
                0.0,
                chart_scale * std::f64::consts::TAU,
            ]),
            chart_shift: crate::test_support::test_b5::finite(0.0),
            chart_scale: crate::test_support::test_b5::positive(chart_scale),
            slope: crate::test_support::test_b5::finite(0.0),
            phase: crate::test_support::test_b5::finite(0.0),
        }),
    };
    let loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[2], &[3]),
        metadata: test_loop_metadata(),
        surface: 4,
    };
    let surface = B5Surface::Sphere {
        center: crate::test_support::test_b5::point([0.0, 0.0, 0.0]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        direction_y: crate::test_support::test_b5::unit([0.0, 1.0, 0.0]),
        radius: crate::test_support::test_b5::positive_length(5.0),
        azimuth_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
        latitude_range: crate::test_support::test_b5::increasing([
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        ]),
        construction_radius: crate::test_support::test_b5::positive_length(chart_scale),
        chart_origin: crate::test_support::test_b5::finite(0.0),
    };
    let opaque_pcurves = BTreeMap::from([(2, pcurve.clone())]);
    let surfaces = BTreeMap::from([(4, surface)]);
    let pcurves = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let endpoints = pcurve_endpoints(2, 3, &geometry)
        .expect("validated sphere pcurve endpoints")
        .map(crate::test_support::test_b5::coordinates);
    assert!(distance_squared(endpoints[0], [5.0, 0.0, 0.0]) < 1e-24);
    assert!(distance_squared(endpoints[1], [0.0, 5.0, 0.0]) < 1e-24);

    assert_eq!(
        bind_edge_vertices(
            &BTreeMap::from([(1, loop_.clone())]),
            &geometry,
            &crate::test_support::test_b5::points([[5.0, 0.0, 0.0], [0.0, 5.0, 0.0]]),
        ),
        BTreeMap::from([(3, [0, 1])])
    );

    let trimmed_start = parameter_end * 0.25;
    let trimmed_end = parameter_end * 0.75;
    let edge_parameter_incidences = BTreeMap::from([(3, [20, 21])]);
    let parameter_incidences = BTreeMap::from([
        (
            20,
            B5ParameterIncidence {
                object_id: 20,
                lanes: vec![B5IncidenceLane {
                    curve: 2,
                    parameter: crate::test_support::test_b5::finite(trimmed_start),
                    control: 1,
                }],
            },
        ),
        (
            21,
            B5ParameterIncidence {
                object_id: 21,
                lanes: vec![B5IncidenceLane {
                    curve: 2,
                    parameter: crate::test_support::test_b5::finite(trimmed_end),
                    control: 1,
                }],
            },
        ),
    ]);
    let trimmed_geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let trimmed_points = [
        sphere_great_circle_point(
            opaque_pcurves[&2]
                .sphere_great_circle
                .as_ref()
                .expect("great circle"),
            &surfaces[&4],
            crate::test_support::test_b5::finite(trimmed_start),
        )
        .expect("trimmed start"),
        sphere_great_circle_point(
            opaque_pcurves[&2]
                .sphere_great_circle
                .as_ref()
                .expect("great circle"),
            &surfaces[&4],
            crate::test_support::test_b5::finite(trimmed_end),
        )
        .expect("trimmed end"),
    ];
    assert_eq!(
        pcurve_endpoints(2, 3, &trimmed_geometry),
        Some(trimmed_points)
    );
    assert_eq!(
        bind_edge_vertices(
            &BTreeMap::from([(1, loop_)]),
            &trimmed_geometry,
            &trimmed_points,
        ),
        BTreeMap::from([(3, [0, 1])])
    );
}

#[test]
fn native_vertex_identity_retains_finite_separated_lifts_with_tolerance() {
    let endpoints = [[1.0e8, 2.0e8, 3.0e8], [1.0e8 + 5.0, 2.0e8, 3.0e8]];
    let pcurves = BTreeMap::from([(
        2,
        B5Pcurve {
            object_id: 2,
            surface: 4,
            degree: 1,
            distinct_knots: crate::test_support::test_b5::finite_lane(&[0.0, 1.0]),
            multiplicities: vec![2, 2],
            control_points: vec![
                crate::test_support::test_b5::finite_vector([0.0, 0.0]),
                crate::test_support::test_b5::finite_vector([1.0, 0.0]),
            ],
            weights: None,
            parameter_range: None,
            parameterization: B5PcurveParameterization::Native,
            class_21_suffix_scalar: None,
            lifted_endpoints: Some(crate::test_support::test_b5::points(endpoints)),
        },
    )]);
    let opaque_pcurves = BTreeMap::new();
    let surfaces = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[2], &[3]),
        metadata: test_loop_metadata(),
        surface: 4,
    };

    let bound = bind_native_vertices(
        &BTreeMap::from([(1, loop_.clone())]),
        &geometry,
        &BTreeMap::from([(3, [10, 11])]),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[],
    );

    assert_eq!(
        bound.edges,
        BTreeMap::from([(3, [B5VertexRef::Logical(0), B5VertexRef::Logical(1)])])
    );
    assert_eq!(
        bound.vertices,
        vec![
            B5LogicalVertex {
                object_id: 10,
                point: crate::test_support::test_b5::point(endpoints[0]),
            },
            B5LogicalVertex {
                object_id: 11,
                point: crate::test_support::test_b5::point(endpoints[1]),
            },
        ]
    );
    assert!(bound.tolerances.is_empty());

    let mismatched = bind_native_vertices(
        &BTreeMap::from([(1, loop_)]),
        &geometry,
        &BTreeMap::from([(3, [10, 11])]),
        &BTreeMap::new(),
        &BTreeMap::from([(10, crate::test_support::test_b5::point(endpoints[1]))]),
        &[],
    );
    assert_eq!(
        mismatched.edges,
        BTreeMap::from([(3, [B5VertexRef::Logical(0), B5VertexRef::Logical(1)])])
    );
    assert_eq!(
        mismatched.vertices,
        vec![
            B5LogicalVertex {
                object_id: 10,
                point: crate::test_support::test_b5::point(endpoints[1]),
            },
            B5LogicalVertex {
                object_id: 11,
                point: crate::test_support::test_b5::point(endpoints[1]),
            },
        ]
    );
    assert_eq!(mismatched.tolerances.len(), 1);
    assert!((mismatched.tolerances[&0].get() - (5.0 + 1.0e-9)).abs() < f64::EPSILON);
}

mod incidence_loci;

#[test]
fn face_rejection_does_not_charge_unvisited_references() {
    let surfaces = BTreeMap::from([(
        1,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x27,
            payload: Vec::new(),
        },
    )]);
    let mut record = B5FaceRecord {
        object_id: 2,
        references: vec![1, 3],
        terminal_control: None,
    };
    // The first unsupported reference ends the scan under the same allowance
    // regardless of the number of trailing references.
    for suffix in [0, 10_000] {
        record.references.resize(2 + suffix, 3);
        assert!(crate::test_support::with_work_limit(100, |ctx| {
            super::super::parse_face(ctx, &record, &BTreeMap::new(), &surfaces, &BTreeMap::new())
        })
        .expect("only the visited prefix is charged")
        .is_none());
    }
}
