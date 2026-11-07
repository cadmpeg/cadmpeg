use std::collections::HashSet;

use super::emit;
use crate::sketch::outer_sheet::{SheetLine, SheetPlan};
use crate::test_support::{with_entity_limit, with_service_context};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

const EPS_POSITION: f64 = 1e-12;

fn triangle(offset: u64, shift: f64, first_id: u32) -> Vec<SheetLine> {
    cycle(
        offset,
        first_id,
        [
            Point2::new(shift, 0.0),
            Point2::new(shift + 4.0, 0.0),
            Point2::new(shift, 3.0),
        ],
    )
}

fn cycle(offset: u64, first_id: u32, points: [Point2; 3]) -> Vec<SheetLine> {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .take(3)
        .enumerate()
        .map(|(i, (start, end))| {
            let du = end.u - start.u;
            let dv = end.v - start.v;
            let length = du.hypot(dv);
            let direction = Point2::new(du / length, dv / length);
            SheetLine {
                source_offset: offset + u64::try_from(i).expect("small source ordinal"),
                start_id: first_id + u32::try_from(i).expect("small endpoint ordinal"),
                end_id: first_id + u32::try_from((i + 1) % 3).expect("small endpoint ordinal"),
                start,
                end,
                origin: Point2::new(start.u - 2.0 * direction.u, start.v - 2.0 * direction.v),
                direction,
                interval: [2.0, 2.0 + length],
                forward: true,
            }
        })
        .collect()
}

fn plan(components: Vec<Vec<SheetLine>>) -> SheetPlan {
    SheetPlan {
        source_offset: 400,
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
        components,
    }
}

fn validate(ir: &mut CadIr, annotations: &AnnotationBuilder) {
    ir.finalize();
    let report = cadmpeg_ir::validate::validate_neutral_with_annotations(
        ir,
        annotations.annotations(),
        Vec::new(),
    )
    .expect("validation completes");
    assert!(report.is_ok(), "{:?}", report.findings);
}

#[test]
fn disconnected_cycles_become_separate_open_sheet_bodies() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let plans = [plan(vec![triangle(500, 0.0, 10), triangle(600, 10.0, 20)])];
    let count = with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &plans))
        .expect("emit two sheets");
    assert_eq!(count, 2);
    assert_eq!(ir.model.entity_count(), 42);
    assert_eq!(ir.model.points.len(), 6);
    assert_eq!(ir.model.vertices.len(), 6);
    assert_eq!(ir.model.curves.len(), 6);
    assert_eq!(ir.model.edges.len(), 6);
    assert_eq!(ir.model.coedges.len(), 6);
    assert_eq!(ir.model.surfaces.len(), 2);
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.loops.len(), 2);
    assert_eq!(ir.model.regions.len(), 2);
    assert_eq!(ir.model.shells.len(), 2);
    assert!(ir
        .model
        .bodies
        .iter()
        .all(|body| body.kind == BodyKind::Sheet && body.regions.len() == 1));
    assert!(ir.model.shells.iter().all(|shell| shell.faces().len() == 1));
    assert!(ir
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.radial_next == coedge.id));
    assert_eq!(annotations.annotations().provenance.len(), 42);
    assert!(annotations
        .annotations()
        .exactness()
        .values()
        .all(|note| note.entity() == Exactness::Derived));
    assert!(annotations
        .annotations()
        .provenance
        .values()
        .all(|source| source.stream() == "catia:outer-sketch"));
    validate(&mut ir, &annotations);
}

#[test]
fn reversed_use_preserves_native_line_and_endpoint_parameterization() {
    let mut lines = triangle(500, 0.0, 10);
    let line = &mut lines[0];
    std::mem::swap(&mut line.start_id, &mut line.end_id);
    std::mem::swap(&mut line.start, &mut line.end);
    line.direction = Point2::new(-1.0, 0.0);
    line.origin = Point2::new(6.0, 0.0);
    line.forward = false;
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &[plan(vec![lines])]))
        .expect("emit reversed use");
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = ir.model.curves[0].geometry else {
        panic!("native line carrier")
    };
    assert_eq!(line.origin().get(), Point3::new(6.0, 0.0, 0.0));
    assert_eq!(*line.direction().as_raw(), Vector3::new(-1.0, 0.0, 0.0));
    assert_eq!(
        ir.model.curves[0]
            .parameter_range
            .expect("native curve interval")
            .endpoints(),
        [2.0, 6.0]
    );
    let edge = &ir.model.edges[0];
    let point = |vertex: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|candidate| candidate.id == *vertex)
            .expect("edge vertex");
        ir.model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .expect("vertex point")
            .position()
            .get()
    };
    assert_eq!(point(&edge.start), Point3::new(4.0, 0.0, 0.0));
    assert_eq!(point(&edge.end), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(ir.model.coedges[0].sense, Sense::Reversed);
    assert_eq!(ir.model.faces[0].sense, Sense::Forward);
    validate(&mut ir, &annotations);
}

#[test]
fn translated_signed_chart_places_points_and_plane_without_reflection() {
    let mut source = plan(vec![triangle(500, 0.0, 10)]);
    source.origin = Point3::new(10.0, -20.0, 30.0);
    source.u_axis = Vector3::new(0.0, 0.0, -1.0);
    source.v_axis = Vector3::new(-1.0, 0.0, 0.0);
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &[source]))
        .expect("emit placed sheet");
    for expected in [
        Point3::new(10.0, -20.0, 30.0),
        Point3::new(10.0, -20.0, 26.0),
        Point3::new(7.0, -20.0, 30.0),
    ] {
        assert!(ir
            .model
            .points
            .iter()
            .any(|point| point.position().get().distance(expected) <= EPS_POSITION));
    }
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) =
        ir.model.surfaces[0].geometry
    else {
        panic!("plane carrier")
    };
    assert_eq!(plane.origin().get(), Point3::new(10.0, -20.0, 30.0));
    assert_eq!(*plane.frame().axis().as_raw(), Vector3::new(0.0, 1.0, 0.0));
    validate(&mut ir, &annotations);
}

#[test]
fn entity_budget_refuses_the_complete_batch_before_model_or_annotation_mutation() {
    let plans = [plan(vec![triangle(500, 0.0, 10), triangle(600, 10.0, 20)])];
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let error = with_entity_limit(41, |ctx| emit(ctx, &mut ir, &mut annotations, &plans))
        .expect_err("42 entities exceed the budget");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities && limit.operation == "admit outer sketch sheet entities")
    );
    assert_eq!(ir.model.entity_count(), 0);
    assert!(annotations.annotations().provenance.is_empty());
    with_entity_limit(42, |ctx| {
        assert_eq!(
            emit(ctx, &mut ir, &mut annotations, &plans).expect("exact entity budget"),
            2
        );
        assert!(matches!(
            ctx.charge_entities(1, "one more entity"),
            Err(CodecError::ResourceLimit(_))
        ));
    });
}

#[test]
fn invalid_later_component_leaves_prior_model_and_annotations_intact() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let first = plan(vec![triangle(500, 0.0, 10)]);
    with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &[first]))
        .expect("existing sheet");
    let before_model = ir.model.clone();
    let before_annotations = annotations.annotations().clone();
    let mut second = plan(vec![triangle(600, 10.0, 20), triangle(700, 20.0, 30)]);
    second.source_offset = 800;
    second.components[1][2].end_id = 99;
    assert_eq!(
        with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &[second]))
            .expect("withhold the incomplete candidate"),
        0
    );
    assert_eq!(ir.model, before_model);
    assert_eq!(*annotations.annotations(), before_annotations);
}

#[test]
fn source_scopes_and_existing_sheets_keep_distinct_entity_identities() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let first = plan(vec![triangle(500, 0.0, 10)]);
    let mut second = plan(vec![triangle(500, 0.0, 10)]);
    second.source_offset = 900;
    second.origin = Point3::new(0.0, 0.0, 5.0);
    with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &[first]))
        .expect("first sheet");
    let original = ir.model.clone();
    with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &[second]))
        .expect("second sheet");
    assert_eq!(&ir.model.points[..3], original.points.as_slice());
    assert_eq!(&ir.model.curves[..3], original.curves.as_slice());
    assert_eq!(&ir.model.bodies[..1], original.bodies.as_slice());
    let ids = annotations
        .annotations()
        .provenance
        .keys()
        .collect::<HashSet<_>>();
    assert_eq!(ids.len(), ir.model.entity_count());
    validate(&mut ir, &annotations);
}

#[test]
fn every_emitted_arena_refuses_an_existing_identity_before_mutation() {
    use cadmpeg_ir::document::Model;

    let plans = [plan(vec![triangle(500, 0.0, 10)])];
    let mut prototype = CadIr::empty();
    with_service_context(|ctx| emit(ctx, &mut prototype, &mut AnnotationBuilder::new(), &plans))
        .expect("authored sheet prototype");
    let model = prototype.model;
    let mut collisions: [Model; 11] = std::array::from_fn(|_| Model::default());
    collisions[0].points = model.points;
    collisions[1].vertices = model.vertices;
    collisions[2].curves = model.curves;
    collisions[3].edges = model.edges;
    collisions[4].coedges = model.coedges;
    collisions[5].surfaces = model.surfaces;
    collisions[6].faces = model.faces;
    collisions[7].loops = model.loops;
    collisions[8].shells = model.shells;
    collisions[9].regions = model.regions;
    collisions[10].bodies = model.bodies;
    for existing in collisions {
        let mut ir = CadIr::empty();
        ir.model = existing;
        let before = ir.model.clone();
        let mut annotations = AnnotationBuilder::new();
        let error = with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &plans))
            .expect_err("existing arena identity must be preserved");
        assert!(matches!(error, CodecError::Malformed(_)));
        assert_eq!(ir.model, before);
        assert!(annotations.annotations().provenance.is_empty());
    }
}

#[test]
fn overflowing_chart_withholds_its_plan_and_retains_another_valid_plan() {
    let huge = f64::MAX * 0.5;
    let mut overflow = plan(vec![cycle(
        500,
        10,
        [
            Point2::new(0.0, 0.0),
            Point2::new(huge, 0.0),
            Point2::new(0.0, 3.0),
        ],
    )]);
    overflow.origin = Point3::new(f64::MAX * 0.75, 0.0, 0.0);
    assert!(!overflow
        .origin
        .translated(overflow.u_axis, huge)
        .is_finite());
    let mut valid = plan(vec![triangle(600, 0.0, 20)]);
    valid.source_offset = 900;
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let bodies = with_entity_limit(21, |ctx| {
        emit(ctx, &mut ir, &mut annotations, &[overflow, valid])
    })
    .expect("geometric overflow withholds one candidate");
    assert_eq!(bodies, 1);
    assert_eq!(ir.model.entity_count(), 21);
    assert_eq!(annotations.annotations().provenance.len(), 21);
    validate(&mut ir, &annotations);
}

#[test]
fn tightened_model_tolerance_withholds_only_the_mismatched_plan() {
    let mut shifted_line = plan(vec![triangle(500, 0.0, 10)]);
    shifted_line.components[0][0].origin.v += 5e-7;
    let mut ordinary_ir = CadIr::empty();
    assert_eq!(
        with_service_context(|ctx| emit(
            ctx,
            &mut ordinary_ir,
            &mut AnnotationBuilder::new(),
            &[shifted_line.clone()]
        ))
        .expect("default tolerance contains the source residual"),
        1
    );
    let mut valid = plan(vec![triangle(600, 10.0, 20)]);
    valid.source_offset = 900;
    let mut ir = CadIr::empty();
    ir.tolerances = cadmpeg_ir::units::Tolerances::new(1e-8, ir.tolerances.angular.get())
        .expect("positive tightened tolerance");
    let mut annotations = AnnotationBuilder::new();
    let count = with_entity_limit(21, |ctx| {
        emit(ctx, &mut ir, &mut annotations, &[shifted_line, valid])
    })
    .expect("the mismatched candidate remains native");
    assert_eq!(count, 1);
    assert_eq!(ir.model.entity_count(), 21);
    validate(&mut ir, &annotations);
}

#[test]
fn repeated_plans_refuse_staged_identities_before_any_batch_commit() {
    let source = plan(vec![triangle(500, 0.0, 10)]);
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let error =
        with_service_context(|ctx| emit(ctx, &mut ir, &mut annotations, &[source.clone(), source]))
            .expect_err("a prior staged plan already owns these identities");
    assert!(matches!(error, CodecError::Malformed(_)));
    assert_eq!(ir.model.entity_count(), 0);
    assert!(annotations.annotations().provenance.is_empty());
}
