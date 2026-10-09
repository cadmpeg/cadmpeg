// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, CurveId, FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Color, Face, FaceLoops, Sense};
use cadmpeg_ir::CadIr;

#[derive(Clone, Copy)]
enum Source {
    Curve,
    Surface,
    Body,
    Face,
}

fn global() -> crate::global::ProjectedGlobal {
    let bytes = owned_test_file(&[]);
    let scan = crate::test_support::scan(&bytes).unwrap();
    crate::test_support::parse_global(&scan).unwrap().0.length_context().unwrap()
}

fn model(source: Source, count: usize) -> CadIr {
    let mut ir = CadIr::empty();
    for index in 0..count {
        match source {
            Source::Curve => ir.model.curves.push(Curve {
                id: CurveId::mint(format!("test:model:curve#source-{index}")).unwrap(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0),
                ).unwrap())),
                source_object: None,
            }),
            Source::Surface => ir.model.surfaces.push(Surface {
                id: SurfaceId::mint(format!("test:model:surface#source-{index}")).unwrap(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                ).unwrap())),
                source_object: None,
            }),
            Source::Body => ir.model.bodies.push(Body {
                id: BodyId::mint(format!("test:model:body#source-{index}")).unwrap(),
                kind: BodyKind::Sheet, regions: Vec::new(), transform: None,
                name: Some("existing body".into()), color: Color::new(0.0, 0.0, 1.0, 1.0),
                visible: Some(false),
            }),
            Source::Face => ir.model.faces.push(Face {
                id: FaceId::mint(format!("test:model:face#source-{index}")).unwrap(),
                shell: ShellId::mint("test:model:shell#owner").unwrap(),
                surface: SurfaceId::mint("test:model:surface#carrier").unwrap(),
                sense: Sense::Forward, loops: FaceLoops::unspecified(Vec::new()),
                name: Some("existing face".into()), color: Color::new(0.0, 0.0, 1.0, 1.0),
                tolerance: None,
            }),
        }
    }
    ir
}

fn assert_source_bounds(source: Source, operation: &'static str, second_body_pass: bool) {
    let global = global();
    for count in [1_usize, 64] {
        let expected = model(source, count);
        let count_work = u64::try_from(count).unwrap();
        let total = if matches!(source, Source::Body) { 2 * count_work } else { count_work };
        let start = if second_body_pass { count_work } else { 0 };
        // The tested sources have no metadata. Curve/surface bodies are free;
        // absent body/face sequence maps perform zero key comparisons. Each
        // actual source visit admits one unit, with no terminal empty probe.
        for (cap, accepts) in [(start, false), (start + count_work - 1, false), (total, true)] {
            let mut ir = expected.clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let sequences = super::super::super::geometry::SourceSequences::default();
            let result = super::super::project(
                &mut ir, &[], (&BTreeMap::new(), &BTreeMap::new()), &BTreeMap::new(),
                &global, &ctx, &sequences,
            ).map(|outcome| {
                assert!(outcome.decoded.is_empty());
                assert!(outcome.losses.is_empty());
                drop(outcome);
            });
            if accepts {
                result.unwrap();
                ctx.finish_session().unwrap();
            } else {
                let first = match result {
                    Err(CodecError::ResourceLimit(first)) => first,
                    Err(error) => panic!("unexpected presentation error: {error:?}"),
                    Ok(_) => panic!("expected actual presentation source refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, operation);
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                // Replay with both the original source and an empty model.
                for replay in [expected.clone(), CadIr::empty()] {
                    let mut replay = replay;
                    assert!(matches!(super::super::project(
                        &mut replay, &[], (&BTreeMap::new(), &BTreeMap::new()), &BTreeMap::new(),
                        &global, &ctx, &sequences,
                    ), Err(CodecError::ResourceLimit(last)) if last == first));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
            assert_eq!(ir.model, expected.model);
        }
    }
}

#[test]
fn curve_display_visits_refuse_first_and_last_and_accept_exact_source_length() {
    assert_source_bounds(Source::Curve, "iges curve display traversal", false);
}
#[test]
fn surface_display_visits_refuse_first_and_last_and_accept_exact_source_length() {
    assert_source_bounds(Source::Surface, "iges surface display traversal", false);
}
#[test]
fn body_display_visits_refuse_first_and_last_and_accept_both_exact_passes() {
    assert_source_bounds(Source::Body, "iges body display traversal", false);
}
#[test]
fn body_name_visits_refuse_first_and_last_and_accept_both_exact_passes() {
    assert_source_bounds(Source::Body, "iges body name traversal", true);
}
#[test]
fn face_display_visits_refuse_first_and_last_and_accept_exact_source_length() {
    assert_source_bounds(Source::Face, "iges face display traversal", false);
}

#[test]
fn empty_presentation_sources_execute_no_work_or_allocation() {
    let global = global();
    let mut ir = CadIr::empty();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let outcome = super::super::project(
        &mut ir, &[], (&BTreeMap::new(), &BTreeMap::new()), &BTreeMap::new(),
        &global, &ctx, &super::super::super::geometry::SourceSequences::default(),
    ).unwrap();
    assert!(outcome.decoded.is_empty());
    assert!(outcome.losses.is_empty());
    assert_eq!(ir.model, CadIr::empty().model);
    drop(outcome);
    ctx.finish_session().unwrap();
}

fn font_record(count: usize, motions: usize, invalid_last: Option<bool>) -> crate::parameter::ParameterRecord {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    let mut values = vec![TokenValue::Integer(310), TokenValue::Integer(101),
        TokenValue::String(b"FONT".to_vec()), TokenValue::Omitted,
        TokenValue::Integer(10), TokenValue::Integer(i64::try_from(count).unwrap())];
    for index in 0..count {
        let code = if invalid_last == Some(false) && index + 1 == count { 0 }
            else { i64::try_from(index).unwrap() };
        values.extend([TokenValue::Integer(code), TokenValue::Integer(8),
            TokenValue::Integer(0), TokenValue::Integer(i64::try_from(motions).unwrap())]);
        for motion in 0..motions {
            let pen = if invalid_last == Some(true) && index + 1 == count && motion + 1 == motions { 2 } else { 0 };
            values.extend([TokenValue::Integer(pen), TokenValue::Integer(0), TokenValue::Integer(0)]);
        }
    }
    ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
        values.into_iter().map(|value| Token { value, span: 0..0 }).collect(), Vec::new())
}

fn font_entry() -> crate::directory::DirectoryEntry {
    let mut entry = crate::test_support::directory_target(1, 310);
    entry.status = crate::directory::SourceStatus::from_codes([0, 0, 2, 0]);
    entry
}

fn font_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn assert_font_visit_bounds(motions: usize) {
    let entry = font_entry();
    let empty = font_record(0, 0, None);
    for count in [1_usize, 64] {
        let record = font_record(count, motions, None);
        let total = u64::try_from(count * (1 + motions)).unwrap();
        let first = u64::from(motions != 0);
        let operation = if motions == 0 { "iges text font character traversal" }
            else { "iges text font motion traversal" };
        for (work, accepts) in [(first, false), (total - 1, false), (total, true)] {
            let arena = DecodeArena::new();
            let policy = font_policy(work);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::super::text_font_definition(
                &entry, &record, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
            );
            if accepts {
                let font = result.unwrap().unwrap();
                assert_eq!(font.supersedes, None);
                ctx.finish_session().unwrap();
            } else {
                let first = match result {
                    Err(CodecError::ResourceLimit(first)) => first,
                    Err(error) => panic!("unexpected font error: {error:?}"),
                    Ok(_) => panic!("expected actual font visit refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, operation);
                assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
                for replay in [&record, &empty] {
                    assert!(matches!(super::super::text_font_definition(
                        &entry, replay, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
                    ), Err(CodecError::ResourceLimit(last)) if last == first));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

#[test]
fn font_characters_admit_only_actual_first_and_last_visits() {
    assert_font_visit_bounds(0);
}
#[test]
fn font_motions_admit_only_actual_first_and_last_visits() {
    assert_font_visit_bounds(2);
}

#[test]
fn invalid_final_font_character_stops_before_its_motion_records() {
    let entry = font_entry();
    let record = font_record(64, 2, Some(false));
    let arena = DecodeArena::new();
    // 63 complete characters with two motions, then the duplicate code.
    let policy = font_policy(63 * 3 + 1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::text_font_definition(
        &entry, &record, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
    ).unwrap().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_final_font_motion_stops_at_its_actual_visit() {
    let entry = font_entry();
    let record = font_record(64, 2, Some(true));
    let arena = DecodeArena::new();
    let policy = font_policy(64 * 3);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::text_font_definition(
        &entry, &record, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
    ).unwrap().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn empty_font_definition_is_free_and_preserves_original_entry_refusal() {
    let entry = font_entry();
    let record = font_record(0, 0, None);
    for fused in [false, true] {
        let arena = DecodeArena::new();
        let policy = font_policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if fused {
            let first = match ctx.charge_work(1, "test original empty font refusal").unwrap_err() {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected original resource refusal"),
            };
            assert!(matches!(super::super::text_font_definition(
                &entry, &record, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
            ), Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert!(super::super::text_font_definition(
                &entry, &record, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
            ).unwrap().is_none());
            ctx.finish_session().unwrap();
        }
    }
}
