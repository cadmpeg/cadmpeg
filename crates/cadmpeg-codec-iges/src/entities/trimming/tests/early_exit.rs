// SPDX-License-Identifier: Apache-2.0
use super::super::super::composite::CompositeCurveError;
use super::*;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;

const LARGE_TAIL: usize = 4096;
const LOSS_MESSAGE_FIRST_FRAGMENT: &str = "IGES entity type ";

struct ProjectionInput {
    directory: Vec<crate::directory::DirectoryEntry>,
    records: Vec<crate::parameter::ParameterRecord>,
    global: crate::global::ProjectedGlobal,
}

fn parse_projection_input(bytes: &[u8]) -> ProjectionInput {
    crate::test_support::with_service_context(bytes, |ctx| {
        let scan = crate::card::scan_with_context(bytes, ctx).unwrap();
        let (global, _, _global_storage) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) =
            crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        let parameters =
            crate::parameter::assemble_with_context(&scan, &directory, &quarantined, &global, ctx)
                .unwrap();
        ProjectionInput {
            directory,
            records: parameters.records,
            global: global.length_context().unwrap(),
        }
    })
}

fn projection_subset(
    input: &ProjectionInput,
    directory_sequences: &[u32],
    parameter_sequences: &[u32],
) -> ProjectionInput {
    ProjectionInput {
        directory: input
            .directory
            .iter()
            .filter(|entry| directory_sequences.contains(&entry.sequence))
            .copied()
            .collect(),
        records: input
            .records
            .iter()
            .filter(|record| parameter_sequences.contains(&record.directory_sequence))
            .cloned()
            .collect(),
        global: input.global,
    }
}

fn run_projection(
    input: &ProjectionInput,
    source_ir: &CadIr,
    work_limit: u64,
) -> Result<Vec<String>, CodecError> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work_limit;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let mut ir = source_ir.clone();
        let mut derivation_storage = ctx.reserve_scoped(0, "test trimming derivation storage")?;
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let (outcome, _) = super::super::project(
            &mut ir,
            &input.directory,
            &input.records,
            &input.global,
            (ctx, &mut derivation_storage),
            &mut sequences,
        )?;
        let messages = outcome
            .losses
            .into_iter()
            .map(|loss| loss.message)
            .collect();
        drop(outcome.decoded);
        drop(outcome.decoded_storage);
        drop(outcome.loss_slots_storage);
        Ok(messages)
    })
}

fn entity_loss_message(entity_type: i64, reason: &str) -> String {
    format!("IGES entity type {entity_type} form 0 was not projected: {reason}")
}

fn attributed_loss_format_work(message: &str, sequence: u32) -> u64 {
    let directory_tag = format!("directory_entry:D{sequence}");
    let message_bytes = u64::try_from(message.len()).unwrap();
    let provenance_bytes = u64::try_from("iges".len() + directory_tag.len()).unwrap();

    // format_retained charges the formatted byte count, then charges the same
    // bytes again as it appends the formatted output. Message and both
    // provenance fields use this path.
    2 * message_bytes + 2 * provenance_bytes
}

fn project_boundary(input: &ProjectionInput, source_ir: &CadIr, operation: &str) -> (u64, u64) {
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    match run_projection(input, source_ir, u64::MAX) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.operation, operation);
            (limit.used, limit.additional)
        }
        Err(error) => panic!("unexpected refusal before {operation}: {error:?}"),
        Ok(_) => panic!("missing work boundary for {operation}"),
    }
}

fn loss_message_boundary(input: &ProjectionInput, source_ir: &CadIr) -> (u64, u64) {
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "iges entity loss message",
        None,
    );
    match run_projection(input, source_ir, u64::MAX) {
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.operation, "iges entity loss message");
            (limit.used, limit.additional)
        }
        Err(error) => panic!("unexpected loss-message refusal: {error:?}"),
        Ok(_) => panic!("missing entity-loss message boundary"),
    }
}

fn assert_first_invalid_stops_before_large_tail(
    input: &ProjectionInput,
    source_ir: &CadIr,
    operation: &str,
    entity_type: i64,
    sequence: u32,
    reason: &str,
    final_directory_scan: usize,
) {
    let expected_message = entity_loss_message(entity_type, reason);
    let (traversal_used, traversal_additional) = project_boundary(input, source_ir, operation);
    assert_eq!(traversal_additional, 1);

    let (message_used, message_first_fragment) = loss_message_boundary(input, source_ir);
    assert_eq!(
        message_first_fragment,
        u64::try_from(LOSS_MESSAGE_FIRST_FRAGMENT.len()).unwrap(),
        "the first formatted loss fragment is the fixed entity-type prefix"
    );
    assert!(message_used >= traversal_used);

    // message_used is before the first prefix fragment. Reaching the next
    // project phase requires both formatting passes for the message and both
    // provenance fields, then any later directory pass.
    let replay_limit = message_used
        + attributed_loss_format_work(&expected_message, sequence)
        + u64::try_from(final_directory_scan).unwrap();
    assert!(
        replay_limit - traversal_used < u64::try_from(LARGE_TAIL).unwrap(),
        "the post-boundary project work must stay below the unvisited tail"
    );

    let result = run_projection(input, source_ir, replay_limit).unwrap_or_else(|error| {
        panic!("the first invalid value did not stop {operation}: {error:?}")
    });
    assert!(
        result.iter().any(|loss| loss == &expected_message),
        "expected loss {expected_message:?}; got {result:#?}"
    );
}

fn repeated_fields(value: &str, count: usize) -> String {
    (0..count).map(|_| value).collect::<Vec<_>>().join(",")
}

fn plane_entity() -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 108,
        form: 0,
        label: "PLANE".into(),
        status: "00010000",
        parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
    }
}

fn support_plane_ir() -> CadIr {
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};

    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("iges:model:surface#D1").unwrap(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir
}

#[test]
fn type_141_stops_at_the_first_invalid_segment_pointer() {
    let segments = repeated_fields("0,1,0", LARGE_TAIL);
    let bytes = owned_test_file(&[
        plane_entity(),
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: format!("141,0,1,1,{LARGE_TAIL},{segments};"),
        },
    ]);
    let parsed = parse_projection_input(&bytes);
    let input = projection_subset(&parsed, &[3], &[3]);
    assert_first_invalid_stops_before_large_tail(
        &input,
        &support_plane_ir(),
        "iges Type141 segment traversal",
        141,
        3,
        "boundary model-curve pointer is invalid",
        input.directory.len(),
    );
}

#[test]
fn type_141_stops_at_the_first_invalid_pcurve_pointer() {
    let pcurve_tail = repeated_fields("0", LARGE_TAIL);
    let bytes = owned_test_file(&[
        plane_entity(),
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: format!("141,1,1,1,1,3,1,{LARGE_TAIL},{pcurve_tail};"),
        },
    ]);
    let parsed = parse_projection_input(&bytes);
    let input = projection_subset(&parsed, &[5], &[5]);
    assert_first_invalid_stops_before_large_tail(
        &input,
        &support_plane_ir(),
        "iges Type141 pcurve traversal",
        141,
        5,
        "boundary pcurve pointer is invalid",
        input.directory.len(),
    );
}

#[test]
fn type_143_stops_at_the_first_invalid_boundary_pointer() {
    let boundary_tail = repeated_fields("0", LARGE_TAIL);
    let bytes = owned_test_file(&[
        plane_entity(),
        OwnedTestEntity {
            entity_type: 143,
            form: 0,
            label: "BOUNDED".into(),
            status: "00000000",
            parameters: format!("143,0,1,{LARGE_TAIL},{boundary_tail};"),
        },
    ]);
    let parsed = parse_projection_input(&bytes);
    let input = projection_subset(&parsed, &[3], &[3]);
    assert_first_invalid_stops_before_large_tail(
        &input,
        &support_plane_ir(),
        "iges Type143 boundary traversal",
        143,
        3,
        "bounded-surface boundary pointer is invalid",
        0,
    );
}

#[test]
fn type_144_stops_at_the_first_invalid_inner_boundary_pointer() {
    let inner_tail = repeated_fields("0", LARGE_TAIL);
    let bytes =
        parameter_domain_trimmed_surface_file(&format!("144,1,0,{LARGE_TAIL},,{inner_tail};"));
    let parsed = parse_projection_input(&bytes);
    let input = projection_subset(&parsed, &[3], &[3]);
    assert_first_invalid_stops_before_large_tail(
        &input,
        &support_plane_ir(),
        "iges Type144 inner traversal",
        144,
        3,
        "trimmed-surface inner-boundary pointer is invalid",
        0,
    );
}

#[test]
fn trimming_boundary_walk_stops_at_a_missing_definition_before_the_tail() {
    let boundary_tail = repeated_fields("3", LARGE_TAIL);
    let bytes = owned_test_file(&[
        plane_entity(),
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "INVALID".into(),
            status: "00010000",
            parameters: "142,0,1,0,3,4;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "TRIMMED".into(),
            status: "00000000",
            parameters: format!("144,1,0,{LARGE_TAIL},,{boundary_tail};"),
        },
    ]);
    let parsed = parse_projection_input(&bytes);
    let input = projection_subset(&parsed, &[3, 5], &[3, 5]);
    let expected_message =
        entity_loss_message(144, "trimmed-surface boundary definition is missing");
    let source_ir = support_plane_ir();
    let (traversal_used, traversal_additional) =
        project_boundary(&input, &source_ir, "iges trimming boundary traversal");
    assert_eq!(traversal_additional, 1);

    // Type142 D3 is intentionally invalid and emits its own loss before this
    // walk. The later loss is followed only by its retained message and tag.
    let sequence = 5;
    // The boundary step admits this first pointer. Its loss then formats the
    // message and each provenance field twice through format_retained.
    let post_boundary_work =
        traversal_additional + attributed_loss_format_work(&expected_message, sequence);
    assert!(post_boundary_work < u64::try_from(LARGE_TAIL).unwrap());
    let replay_limit = traversal_used + post_boundary_work;
    let result = run_projection(&input, &source_ir, replay_limit).unwrap_or_else(|error| {
        panic!("the missing first boundary did not stop the tail: {error:?}")
    });
    assert!(
        result.iter().any(|loss| loss == &expected_message),
        "expected loss {expected_message:?}; got {result:#?}"
    );
}

#[test]
fn trimming_segment_walk_stops_at_the_first_missing_carrier() {
    let segments = repeated_fields("5,1,0", LARGE_TAIL);
    let bytes = owned_test_file(&[
        plane_entity(),
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: format!("141,0,1,1,{LARGE_TAIL},{segments};"),
        },
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "NONCURVE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 143,
            form: 0,
            label: "BOUNDED".into(),
            status: "00000000",
            parameters: "143,0,1,1,3;".into(),
        },
    ]);
    let parsed = parse_projection_input(&bytes);
    let input = projection_subset(&parsed, &[3, 5, 7], &[3, 7]);
    assert_first_invalid_stops_before_large_tail(
        &input,
        &support_plane_ir(),
        "iges trimming segment traversal",
        143,
        7,
        "boundary model curve has no bounded edge",
        0,
    );
}

#[test]
fn linear_boundary_candidates_stop_at_the_first_unusable_candidate() {
    let candidates: Vec<_> = std::iter::repeat_with(|| None).take(LARGE_TAIL).collect();
    let limit = {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "iges linear boundary candidates",
            None,
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        match crate::test_support::with_policy_context(&[], &policy, |ctx| {
            linear_boundary_rings(
                candidates.iter().map(Option::as_ref),
                BoundarySpace::Parameter,
                ctx,
            )
            .map(|_| ())
        }) {
            Err(CodecError::ResourceLimit(limit)) => limit,
            Err(error) => panic!("unexpected ring refusal: {error:?}"),
            Ok(()) => panic!("missing ring-candidate work boundary"),
        }
    };
    assert_eq!(limit.additional, 1);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used + limit.additional;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert!(linear_boundary_rings(
            candidates.iter().map(Option::as_ref),
            BoundarySpace::Parameter,
            ctx
        )
        .unwrap()
        .is_none());
    });
}

#[test]
fn solved_composite_control_walk_stops_at_the_first_active_child() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, Curve,
        CurveGeometry, SolvedCurveGeometry,
    };

    let parent = CurveId::mint("test:model:curve#parent").unwrap();
    let segments = (0..LARGE_TAIL)
        .map(|_| CompositeCurveSegment {
            curve: parent.clone(),
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        })
        .collect::<Vec<_>>();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: parent.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: CompositeCurveSegments::try_from(segments).unwrap(),
            self_intersect: None,
        }),
        source_object: None,
    });
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let run = |work_limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work_limit;
        crate::test_support::with_policy_context(&[], &policy, |ctx| {
            super::super::source_curve_control_intervals(
                &index,
                &parent,
                (&[], &[]),
                crate::global::RealPrecision {
                    single_significance: 7,
                    double_significance: 15,
                },
                1.0,
                &mut std::collections::BTreeSet::new(),
                ctx,
            )
            .map(|controls| {
                assert!(controls.is_none());
                None::<()>
            })
        })
    };
    let limit = {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "iges source solved composite traversal",
            None,
        );
        match run(u64::MAX) {
            Err(CodecError::ResourceLimit(limit)) => limit,
            Err(error) => panic!("unexpected composite refusal: {error:?}"),
            Ok(_) => panic!("missing composite-child work boundary"),
        }
    };
    assert_eq!(limit.additional, 1);

    // After the first segment, recursion finds the parent in the one-entry
    // active set and returns. The parent then removes that one B-tree node.
    let key_work = u64::try_from(parent.as_str().len()).unwrap();
    let alignment = std::mem::align_of::<CurveId>()
        .max(std::mem::align_of::<()>())
        .max(std::mem::align_of::<usize>());
    let removal_work = 11 * std::mem::size_of::<CurveId>()
        + 11 * std::mem::size_of::<()>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * alignment;
    let first_cycle_work =
        limit.additional + key_work + key_work + u64::try_from(removal_work).unwrap();
    assert!(first_cycle_work < u64::try_from(LARGE_TAIL).unwrap());
    assert!(run(limit.used + first_cycle_work).unwrap().is_none());
}

#[test]
fn pcurve_pole_mapping_stops_at_the_first_invalid_point() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsError, NurbsPoles3};
    use cadmpeg_ir::math::Point3;

    let mut points = vec![FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(); LARGE_TAIL];
    points[0] = FinitePoint3::new(Point3::new(f64::MAX, 0.0, 0.0)).unwrap();
    let mut knots = vec![0.0, 0.0];
    knots.extend(std::iter::repeat_n(1.0, LARGE_TAIL));
    let nurbs = crate::test_support::with_service_context(&[], |ctx| {
        NurbsCurve::new(ctx, 1, knots, NurbsPoles3::Polynomial { points }, false)
            .unwrap()
            .unwrap()
    });
    let run = |work_limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work_limit;
        crate::test_support::with_policy_context(&[], &policy, |ctx| {
            super::super::map_bounded_pcurve_nurbs(
                nurbs.clone(),
                Some((2.0, 0.0, 1.0, 0.0)),
                (1.0, 0.0, 1.0, 0.0),
                1.0,
                ctx,
            )
        })
    };
    let limit = {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "iges pcurve polynomial pole mapping",
            None,
        );
        match run(u64::MAX) {
            Err(CompositeCurveError::Budget(CodecError::ResourceLimit(limit))) => limit,
            Err(error) => panic!("unexpected pcurve mapping refusal: {error:?}"),
            Ok(_) => panic!("missing pcurve pole work boundary"),
        }
    };
    assert_eq!(limit.additional, 1);
    assert!(matches!(
        run(limit.used + limit.additional),
        Err(CompositeCurveError::Carrier(NurbsError::Structure(_)))
    ));
}

fn boundary_lookup_input(bounded: bool, missing: bool) -> ProjectionInput {
    let mut entities = vec![
        plane_entity(),
        OwnedTestEntity {
            entity_type: if bounded { 141 } else { 142 },
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: if bounded {
                "141,0,1,1,1,3,1,0;"
            } else {
                "142,0,1,0,3,2;"
            }
            .into(),
        },
    ];
    if missing {
        entities.push(OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "INVALID".into(),
            status: "00010000",
            parameters: "142,0,1,0,3,4;".into(),
        });
    }
    entities.push(OwnedTestEntity {
        entity_type: if bounded { 143 } else { 144 },
        form: 0,
        label: "TRIMMED".into(),
        status: "00000000",
        parameters: if bounded {
            "143,0,1,1,3;"
        } else if missing {
            "144,1,0,1,,5;"
        } else {
            "144,1,0,1,,3;"
        }
        .into(),
    });
    parse_projection_input(&owned_test_file(&entities))
}

fn assert_boundary_lookup_admission(
    bounded: bool,
    missing: bool,
    operation: &str,
    next_operation: &str,
    next_work: u64,
) {
    let input = boundary_lookup_input(bounded, missing);
    let source_ir = support_plane_ir();
    let refuse = |work_limit| {
        let mut ir = source_ir.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut derivation_storage = ctx.reserve_scoped(0, "test trimming derivations").unwrap();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let limit = match super::super::project(
            &mut ir,
            &input.directory,
            &input.records,
            &input.global,
            (&ctx, &mut derivation_storage),
            &mut sequences,
        ) {
            Err(CodecError::ResourceLimit(limit)) => limit,
            Err(error) => panic!("unexpected boundary refusal: {error:?}"),
            Ok(_) => panic!("boundary lookup did not refuse"),
        };
        assert_eq!(ir.model, source_ir.model);
        assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
        let sticky = match super::super::project(
            &mut ir,
            &[],
            &[],
            &input.global,
            (&ctx, &mut derivation_storage),
            &mut sequences,
        ) {
            Err(CodecError::ResourceLimit(limit)) => limit,
            Err(error) => panic!("unexpected sticky refusal: {error:?}"),
            Ok(_) => panic!("empty projection lost the original refusal"),
        };
        assert_eq!(sticky, limit);
        drop(sequences);
        drop(derivation_storage);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == limit)
        );
        limit
    };
    let first = {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        refuse(u64::MAX)
    };
    // The actual boundary map has one u32 key. Core admits at most one
    // comparison, with size_of::<u32>() key bytes, before the search.
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!(
        first.additional,
        u64::try_from(std::mem::size_of::<u32>()).unwrap()
    );
    assert_eq!(first.limit, first.used);
    let next = refuse(first.used + first.additional);
    assert_eq!(next.dimension, ResourceDimension::WorkUnits);
    assert_eq!(next.operation, next_operation);
    assert_eq!(next.used, first.used + first.additional);
    assert_eq!(next.limit, next.used);
    assert_eq!(next.additional, next_work);
    let losses = run_projection(&input, &source_ir, u64::MAX).unwrap();
    let entity_type = if bounded { 143 } else { 144 };
    let reason = if missing {
        "trimmed-surface boundary definition is missing"
    } else {
        "boundary model curve has no bounded edge"
    };
    assert!(losses.contains(&entity_loss_message(entity_type, reason)));
}

#[test]
fn bounded_boundary_lookup_admits_search_before_representation() {
    assert_boundary_lookup_admission(
        true,
        false,
        "iges bounded boundary definition lookup",
        "iges bounded representation proof",
        1,
    );
}

#[test]
fn trimming_boundary_lookup_admits_search_before_segments() {
    assert_boundary_lookup_admission(
        false,
        false,
        "iges trimming boundary definition lookup",
        "iges trimming segment traversal",
        1,
    );
}

#[test]
fn missing_trimming_boundary_lookup_admits_search_before_its_loss() {
    assert_boundary_lookup_admission(
        false,
        true,
        "iges trimming boundary definition lookup",
        "iges entity loss message",
        u64::try_from(LOSS_MESSAGE_FIRST_FRAGMENT.len()).unwrap(),
    );
}
