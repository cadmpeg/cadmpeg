// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

use super::{give_up_evaluation_cycles, CarrierConstructions};
use crate::report::check::Check;
use crate::test_support::evaluation_cycles::cyclic_model;
use crate::validate::{admit, validate_neutral};

fn give_up(
    ir: &mut crate::CadIr,
    losses: &mut Vec<crate::report::loss::LossNote>,
    fidelity: &mut crate::SourceFidelity,
) -> Result<(), CodecError> {
    give_up_evaluation_cycles(
        &cadmpeg_test_support::service_decode_context(),
        ir,
        losses,
        fidelity,
        "test",
    )
}

#[test]
fn model_admission_gives_up_a_malformed_curve_surface_reference_cycle() {
    let (mut ir, curve, surface) = cyclic_model();
    let expected =
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}");
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity && finding.message == expected
    }));
    assert!(!admit::admit(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        admit::DRAFT_CORE_CHECKS,
        Vec::new()
    )
    .expect("admit the model")
    .is_ok());
    let mut losses = Vec::new();
    let mut fidelity = crate::SourceFidelity::for_decode(
        &cadmpeg_test_support::service_decode_context(),
        crate::Annotations::default(),
    );
    give_up(&mut ir, &mut losses, &mut fidelity).expect("a cycle gives up its carrier");
    assert!(ir.model.curves.iter().all(|carrier| carrier.id != curve));
    assert!(ir
        .model
        .surfaces
        .iter()
        .all(|carrier| carrier.id != surface));
    assert!(ir.model.procedural_curves.is_empty());
    assert!(ir.model.procedural_surfaces.is_empty());
    assert_eq!(ir.model.surfaces.len(), 1);
    assert!(ir.model.curves.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, crate::report::Severity::Error);
    assert!(
        losses[0].message.contains(curve.as_str()),
        "{}",
        losses[0].message
    );
    assert!(
        losses[0].message.contains(&expected),
        "{}",
        losses[0].message
    );
    let (_, retained) = fidelity
        .retained_records()
        .iter()
        .next()
        .expect("one retained part");
    assert!(retained.data().expect("retained bytes").is_empty());
    let unknowns = ir
        .native
        .namespace("test")
        .expect("test namespace")
        .arenas()
        .get("unknowns")
        .expect("one unknowns arena");
    assert_eq!(unknowns.len(), 1);
    assert!(unknowns[0].id().starts_with("test:source:part#"));
    assert_eq!(
        unknowns[0].id(),
        fidelity.retained_records().keys().next().unwrap().as_str()
    );
}

#[test]
fn cycle_give_up_drops_required_dependents_and_prunes_lists() {
    use crate::ids::{FaceId, RegionId, ShellId};
    use crate::topology::{Face, FaceLoops, Sense, Shell};
    let (mut ir, _, surface) = cyclic_model();
    let survivor = ir
        .model
        .surfaces
        .iter()
        .find(|carrier| carrier.id != surface)
        .expect("one solved surface")
        .id
        .clone();
    let region = RegionId::mint("test:model:region#cycle").expect("region identity");
    let face = FaceId::mint("test:model:face#cycle").expect("face identity");
    let shell = ShellId::mint("test:model:shell#cycle").expect("shell identity");
    let face_kept = FaceId::mint("test:model:face#kept").expect("face identity");
    let shell_kept = ShellId::mint("test:model:shell#kept").expect("shell identity");
    for (face, shell, surface) in [
        (&face, &shell, &surface),
        (&face_kept, &shell_kept, &survivor),
    ] {
        ir.model.faces.push(Face {
            native_ref: None,
            id: face.clone(),
            shell: shell.clone(),
            surface: surface.clone(),
            sense: Sense::Forward,
            loops: FaceLoops::Unspecified { loops: Vec::new() },
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.shells.push(
            Shell::new(
                shell.clone(),
                region.clone(),
                vec![face.clone()],
                Vec::new(),
                Vec::new(),
            )
            .expect("one-face shell"),
        );
    }
    let mut losses = Vec::new();
    let mut fidelity = crate::SourceFidelity::for_decode(
        &cadmpeg_test_support::service_decode_context(),
        crate::Annotations::default(),
    );
    give_up(&mut ir, &mut losses, &mut fidelity).expect("a cycle gives up its carrier");
    assert!(ir.model.faces.iter().all(|record| record.id != face));
    assert!(ir.model.faces.iter().any(|record| record.id == face_kept));
    let pruned = ir
        .model
        .shells
        .iter()
        .find(|record| record.id == shell)
        .expect("a listing shell survives");
    assert!(pruned.faces().is_empty());
    let kept = ir
        .model
        .shells
        .iter()
        .find(|record| record.id == shell_kept)
        .expect("an unaffected shell survives");
    assert_eq!(kept.faces(), std::slice::from_ref(&face_kept));
    assert_eq!(losses.len(), 1);
    assert!(losses[0].dropped.contains(&face.as_str().to_owned()));
    assert!(!losses[0].dropped.contains(&shell.as_str().to_owned()));
}

#[test]
fn cycle_admission_preserves_zero_depth_session_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (mut ir, _, _) = cyclic_model();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut losses = Vec::new();
    let mut fidelity = crate::SourceFidelity::for_decode(&ctx, crate::Annotations::default());
    let Err(CodecError::ResourceLimit(limit)) =
        give_up_evaluation_cycles(&ctx, &mut ir, &mut losses, &mut fidelity, "test")
    else {
        panic!("traversal frame must be refused");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, "cycle traversal depth");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn cycle_check_reports_one_cycle_for_repeated_carrier_rows() {
    let (mut ir, curve, surface) = cyclic_model();
    // Repeated carrier rows still share the last graph entry and one active state.
    ir.model.curves.push(ir.model.curves[0].clone());
    let index = crate::index::ModelIndex::new_model_only(&ir);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_evaluation_cycles(&ctx, &ir, &index, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].check, Check::ReferentialIntegrity);
    assert_eq!(findings[0].entity.as_deref(), Some(curve.as_str()));
    assert_eq!(findings[0].related, vec![surface.as_str()]);
    assert_eq!(
        findings[0].message,
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}")
    );
}

#[test]
fn cycle_admission_admits_solved_carriers_among_many_points() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut ir = crate::examples::unit_cube().expect("cube fixture");
    let point = ir.model.points[0].clone();
    ir.model.points.extend(std::iter::repeat_n(point, 1_024));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    let mut losses = Vec::new();
    let mut fidelity = crate::SourceFidelity::for_decode(&ctx, crate::Annotations::default());
    give_up_evaluation_cycles(&ctx, &mut ir, &mut losses, &mut fidelity, "test")
        .expect("solved carriers have no cycle index slots");
    assert!(losses.is_empty());
    ctx.finish_session().expect("unfused session");
}

#[test]
fn cycle_admission_gives_up_cycles_among_many_unrelated_points() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut ir, curve, surface) = cyclic_model();
    let cube = crate::examples::unit_cube().expect("cube fixture");
    ir.model
        .points
        .extend(std::iter::repeat_n(cube.model.points[0].clone(), 1_024));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    let mut losses = Vec::new();
    let mut fidelity = crate::SourceFidelity::for_decode(&ctx, crate::Annotations::default());
    give_up_evaluation_cycles(&ctx, &mut ir, &mut losses, &mut fidelity, "test")
        .expect("a cycle gives up its carrier");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(
        losses[0].message.contains(curve.as_str()),
        "{}",
        losses[0].message
    );
    assert!(
        losses[0].message.contains(surface.as_str()),
        "{}",
        losses[0].message
    );
    assert!(ir.model.curves.iter().all(|carrier| carrier.id != curve));
    assert!(ir
        .model
        .surfaces
        .iter()
        .all(|carrier| carrier.id != surface));
    assert_eq!(ir.model.points.len(), 1_024);
    ctx.finish_session()
        .expect("a give-up does not fuse resources");
}

#[test]
fn cycle_associations_keep_the_general_index_duplicate_selection_rules() {
    use crate::geometry::{
        CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
        ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};
    let (mut ir, curve, surface) = cyclic_model();
    // Duplicate construction IDs select the first construction row.
    ir.model.procedural_curves.push(ProceduralCurve::new(
        ir.model.procedural_curves[0].id.clone(),
        ProceduralCurveDefinition::Exact { cache: None },
    ));
    // Duplicate curve carriers select the first resolved construction.
    let other_curve = ProceduralCurveId::mint("test:cycle:construction#other-curve").unwrap();
    ir.model.procedural_curves.push(ProceduralCurve::new(
        other_curve.clone(),
        ProceduralCurveDefinition::Exact { cache: None },
    ));
    let mut carrier = ir.model.curves[0].clone();
    carrier.geometry = CurveGeometry::Procedural {
        construction: other_curve,
        cache: None,
    };
    ir.model.curves.push(carrier);
    // Duplicate surface carriers select the last resolved construction.
    let other_surface = ProceduralSurfaceId::mint("test:cycle:construction#other-surface").unwrap();
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        other_surface.clone(),
        ProceduralSurfaceDefinition::Replica {
            source: surface.clone(),
            transform: crate::transform::Transform::identity(),
        },
        None,
    ));
    let mut carrier = ir.model.surfaces[0].clone();
    carrier.geometry = SurfaceGeometry::Procedural {
        construction: other_surface,
        cache: None,
    };
    ir.model.surfaces.push(carrier);
    let index = super::CycleIndex::build(&ir);
    let general = crate::index::ModelIndex::new_model_only(&ir);
    assert_eq!(index.curve(curve.as_str()), general.curve(curve.as_str()));
    assert_eq!(
        index.surface(surface.as_str()),
        general.surface(surface.as_str())
    );
    assert_eq!(
        index.curve(curve.as_str()),
        Some(&ir.model.procedural_curves[0])
    );
    assert_eq!(
        index.surface(surface.as_str()),
        Some(&ir.model.procedural_surfaces[1])
    );
}
