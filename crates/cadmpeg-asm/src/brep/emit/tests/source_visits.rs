// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::Cell;

fn formula(count: usize) -> EmbeddedLawFormula {
    EmbeddedLawFormula::Named {
        name: cadmpeg_core::nonblank_literal!("law"),
        variables: (0..count).map(|_| EmbeddedLawExpression::Null).collect(),
    }
}

fn formula_refusal(work: u64, additional: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let visits = Cell::new(0);
    let result = map_law_formula(&ctx, formula(3), |_, _| {
        visits.set(visits.get() + 1);
        ctx.copy_retained_text("abc", "test formula text")
            .map(|value| cadmpeg_ir::geometry::LawExpression::Text {
                value: cadmpeg_core::text::NonBlankString::from_ascii_leading(value).unwrap(),
            })
    });
    let Err(CodecError::ResourceLimit(first)) = result else {
        panic!("expected formula source or text-copy refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    assert_eq!(visits.get(), usize::from(work != 0));
    for count in [3, 0] {
        assert!(matches!(map_law_formula(&ctx, formula(count), |_, _| {
            panic!("a refused source must not invoke the mapper")
        }), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn formula_source_refuses_one_visit_before_mapping() {
    formula_refusal(0, 1, "ASM procedural members");
}

#[test]
fn formula_text_copy_refuses_after_one_visit_without_admitting_the_tail() {
    formula_refusal(1, 3, "test formula text");
}

#[test]
fn formula_mapper_error_does_not_admit_the_remaining_variables() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let visits = Cell::new(0);
    let result = map_law_formula(&ctx, formula(3), |index, _| {
        visits.set(visits.get() + 1);
        assert_eq!(index, 0);
        Err(CodecError::malformed("test mapper failure"))
    });
    assert!(matches!(result, Err(CodecError::Malformed(_))));
    assert_eq!(visits.get(), 1);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn formula_source_accepts_exact_visits_and_preserves_variable_indices() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let visits = Cell::new(0);
    let result = map_law_formula(&ctx, formula(3), |index, expression| {
        assert_eq!(index, visits.get());
        assert!(matches!(expression, EmbeddedLawExpression::Null));
        visits.set(index + 1);
        Ok(cadmpeg_ir::geometry::LawExpression::Null {})
    }).unwrap();
    let cadmpeg_ir::geometry::LawFormula::Named { name, variables } = result else {
        panic!("expected the original named formula");
    };
    assert_eq!(name.as_str(), "law");
    assert_eq!(variables.len(), 3);
    assert!(variables.iter().all(|value|
        matches!(value, cadmpeg_ir::geometry::LawExpression::Null {})));
    assert_eq!(visits.get(), 3);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_formula_source_executes_no_visits_or_allocations() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = map_law_formula(&ctx, formula(0), |_, _| {
        panic!("an empty source must not invoke the mapper")
    }).unwrap();
    assert!(matches!(result, cadmpeg_ir::geometry::LawFormula::Named {
        name, variables
    } if name.as_str() == "law" && variables.is_empty()));
    ctx.finish_session().unwrap();
}

#[derive(Clone, Copy)]
enum RecordPass {
    Carriers,
    Pcurves,
    Points,
    Vertices,
    Edges,
    Coedges,
    Loops,
    Faces,
    Containers,
    Unknowns,
}

impl RecordPass {
    fn preflight_visits(self, count: usize) -> u64 {
        // The face pass first completes the independent subshell-owner scan.
        match self {
            Self::Faces => u64::try_from(count).unwrap(),
            _ => 0,
        }
    }

    fn run(
        self,
        ctx: &DecodeContext<'_>,
        records: &[Record],
        table: &nurbs::toks::SubtypeTable,
        out: &mut AsmBrep,
    ) -> Result<(), CodecError> {
        let reach = Reachable::default();
        let by_index = HashMap::new();
        let senses = CurveSenseRefs {
            reversed_curve_refs: &HashSet::new(),
            forward_curve_refs: &HashSet::new(),
        };
        let mut carriers = Carriers::default();
        let format = crate::asm_format!("f3d");
        match self {
            Self::Carriers => emit_carrier_records(ctx, out, records,
                (&mut carriers, &mut ctx.reserve_scoped(0, "test carrier scratch")?,
                    crate::brep::DecodePurpose::Model), &reach, senses, format),
            Self::Pcurves => emit_pcurves(ctx, out, records, &mut carriers, &reach, format),
            Self::Points => emit_points(ctx, out, records, &reach, format),
            Self::Vertices => emit_vertices(ctx, out, records, &by_index, &reach, format),
            Self::Edges => emit_edges(ctx, out, records, &by_index, &reach, senses, format),
            Self::Coedges => emit_coedges(ctx, out, records, CoedgeDecodeInputs {
                token_table: table, save_format_major: None,
            }, &carriers, &reach, format),
            Self::Loops => emit_loops(ctx, out, records, &by_index, &reach, format),
            Self::Faces => emit_faces(ctx, out, records, &by_index, &reach,
                &HashSet::new(), format),
            Self::Containers => emit_containers(ctx, out, ContainerInputs {
                records, by_index: &by_index, reach: &reach,
                wire: &WireShellTopology::default(), stream: "", header_scale: 1.0, format,
            }),
            Self::Unknowns => emit_passthrough_unknowns(ctx, out, records, &[], &reach, format),
        }
    }
}

fn records() -> [Record; 3] {
    // A known carrier head with no reachable owner makes each main pass a
    // complete source scan. No output payload or index insertion is executed.
    std::array::from_fn(|index| Record {
        index, name: "straight".into(), tokens: Vec::<Token>::new().into(), offset: 0, len: 0,
    })
}

fn source_refusal(pass: RecordPass) {
    let records = records();
    let table = super::subtype_table(&[]);
    let arena = DecodeArena::new();
    let work = pass.preflight_visits(records.len());
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    let Err(CodecError::ResourceLimit(first)) = pass.run(&ctx, &records, &table, &mut out) else {
        panic!("expected the first main source visit to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM emitted record pass");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    for replay in [records.as_slice(), &[]] {
        assert!(matches!(pass.run(&ctx, replay, &table, &mut out),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(out.points.is_empty() && out.curves.is_empty() && out.surfaces.is_empty());
    assert!(out.vertices.is_empty() && out.edges.is_empty() && out.coedges.is_empty());
    assert!(out.loops.is_empty() && out.faces.is_empty() && out.shells.is_empty());
    assert!(out.regions.is_empty() && out.bodies.is_empty() && out.unknowns.is_empty());
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

fn source_acceptance(pass: RecordPass) {
    let records = records();
    let table = super::subtype_table(&[]);
    for source in [records.as_slice(), &[]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = pass.preflight_visits(source.len())
            + u64::try_from(source.len()).unwrap();
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut out = AsmBrep::default();
        pass.run(&ctx, source, &table, &mut out).unwrap();
        assert!(out.points.is_empty() && out.curves.is_empty() && out.surfaces.is_empty());
        assert!(out.vertices.is_empty() && out.edges.is_empty() && out.coedges.is_empty());
        assert!(out.loops.is_empty() && out.faces.is_empty() && out.shells.is_empty());
        assert!(out.regions.is_empty() && out.bodies.is_empty() && out.unknowns.is_empty());
        ctx.finish_session().unwrap();
    }
}

#[cfg(target_pointer_width = "64")]
fn first_index_refusal(pass: RecordPass) {
    let mut records = records();
    let limit = 9_223_372_036_854_775_807_u64;
    records[0].index = usize::try_from(limit + 1).unwrap();
    let table = super::subtype_table(&[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The face preflight completes first. Only the first main source visit
    // executes before that record's index conversion refuses.
    policy.limits.max_work_units = pass.preflight_visits(records.len()) + 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    let Err(CodecError::ResourceLimit(first)) = pass.run(&ctx, &records, &table, &mut out) else {
        panic!("expected the original record-index refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::Codec("ASM record index"));
    assert_eq!(first.operation, "ASM record index");
    assert_eq!((first.limit, first.used, first.additional), (limit, limit, 1));
    for replay in [records.as_slice(), &[]] {
        assert!(matches!(pass.run(&ctx, replay, &table, &mut out),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(out.points.is_empty() && out.curves.is_empty() && out.surfaces.is_empty());
    assert!(out.vertices.is_empty() && out.edges.is_empty() && out.coedges.is_empty());
    assert!(out.loops.is_empty() && out.faces.is_empty() && out.shells.is_empty());
    assert!(out.regions.is_empty() && out.bodies.is_empty() && out.unknowns.is_empty());
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

macro_rules! source_controls {
    ($refusal:ident, $acceptance:ident, $index:ident, $pass:ident) => {
        #[test]
        fn $refusal() { source_refusal(RecordPass::$pass); }
        #[test]
        fn $acceptance() { source_acceptance(RecordPass::$pass); }
        #[cfg(target_pointer_width = "64")]
        #[test]
        fn $index() { first_index_refusal(RecordPass::$pass); }
    };
}

source_controls!(carrier_record_source_refuses_one_visit,
    carrier_record_source_accepts_exact_visits_and_empty_input,
    carrier_record_index_refuses_after_one_visit_without_admitting_the_tail, Carriers);
source_controls!(pcurve_record_source_refuses_one_visit,
    pcurve_record_source_accepts_exact_visits_and_empty_input,
    pcurve_record_index_refuses_after_one_visit_without_admitting_the_tail, Pcurves);
source_controls!(point_record_source_refuses_one_visit,
    point_record_source_accepts_exact_visits_and_empty_input,
    point_record_index_refuses_after_one_visit_without_admitting_the_tail, Points);
source_controls!(vertex_record_source_refuses_one_visit,
    vertex_record_source_accepts_exact_visits_and_empty_input,
    vertex_record_index_refuses_after_one_visit_without_admitting_the_tail, Vertices);
source_controls!(edge_record_source_refuses_one_visit,
    edge_record_source_accepts_exact_visits_and_empty_input,
    edge_record_index_refuses_after_one_visit_without_admitting_the_tail, Edges);
source_controls!(coedge_record_source_refuses_one_visit,
    coedge_record_source_accepts_exact_visits_and_empty_input,
    coedge_record_index_refuses_after_one_visit_without_admitting_the_tail, Coedges);
source_controls!(loop_record_source_refuses_one_visit,
    loop_record_source_accepts_exact_visits_and_empty_input,
    loop_record_index_refuses_after_one_visit_without_admitting_the_tail, Loops);
source_controls!(face_record_source_refuses_one_visit_after_complete_preflight,
    face_record_source_accepts_exact_preflight_and_source_visits,
    face_record_index_refuses_after_one_visit_without_admitting_the_tail, Faces);
source_controls!(container_record_source_refuses_one_visit,
    container_record_source_accepts_exact_visits_and_empty_input,
    container_record_index_refuses_after_one_visit_without_admitting_the_tail, Containers);
source_controls!(unknown_record_source_refuses_one_visit,
    unknown_record_source_accepts_exact_visits_and_empty_input,
    unknown_record_index_refuses_after_one_visit_without_admitting_the_tail, Unknowns);

fn algebraic(count: usize) -> EmbeddedLawExpression {
    EmbeddedLawExpression::Algebraic {
        operator: "SUM".into(),
        operands: [7, 11, 19].into_iter().take(count)
            .map(EmbeddedLawExpression::Integer).collect(),
    }
}

fn algebraic_refusal(work: u64, additional: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scope = cadmpeg_ir::identity_key!("scope");
    let mut out = AsmBrep::default();
    let run = |out: &mut AsmBrep, count| map_law_expression(
        &ctx, out, crate::asm_format!("f3d"), LawExpressionScope::Surface(&scope),
        cadmpeg_ir::ids::IdentityKey::try_new("key").unwrap(), algebraic(count),
    );
    let Err(CodecError::ResourceLimit(first)) = run(&mut out, 3) else {
        panic!("expected algebraic source or key-copy refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    for count in [3, 0] {
        assert!(matches!(run(&mut out, count),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(out.curves.is_empty() && out.surfaces.is_empty());
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn algebraic_source_refuses_one_visit_before_key_copy() {
    algebraic_refusal(0, 1, "ASM procedural members");
}

#[test]
fn algebraic_key_copy_refuses_after_one_visit_without_admitting_the_tail() {
    algebraic_refusal(1, 3, "ASM temporary identity key");
}

#[test]
fn algebraic_source_accepts_exact_visits_and_owned_key_copies() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Each of three operands visits the source once and copies three key bytes.
    policy.limits.max_work_units = 3 * (1 + 3);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scope = cadmpeg_ir::identity_key!("scope");
    let mut out = AsmBrep::default();
    let result = map_law_expression(
        &ctx, &mut out, crate::asm_format!("f3d"), LawExpressionScope::Surface(&scope),
        cadmpeg_ir::ids::IdentityKey::try_new("key").unwrap(), algebraic(3),
    ).unwrap();
    let cadmpeg_ir::geometry::LawExpression::Algebraic { operator, operands } = result else {
        panic!("expected the original algebraic expression");
    };
    assert_eq!(operator, "SUM");
    assert_eq!(operands, [7, 11, 19].map(|value|
        cadmpeg_ir::geometry::LawExpression::Integer { value }));
    assert!(out.curves.is_empty() && out.surfaces.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn empty_algebraic_source_executes_no_visits_or_key_copies() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scope = cadmpeg_ir::identity_key!("scope");
    let result = map_law_expression(
        &ctx, &mut AsmBrep::default(), crate::asm_format!("f3d"),
        LawExpressionScope::Surface(&scope),
        cadmpeg_ir::ids::IdentityKey::try_new("key").unwrap(), algebraic(0),
    ).unwrap();
    assert!(matches!(result, cadmpeg_ir::geometry::LawExpression::Algebraic {
        operator, operands
    } if operator == "SUM" && operands.is_empty()));
    ctx.finish_session().unwrap();
}

fn vertex_blend(count: usize) -> EmbeddedVertexBlend {
    use crate::nurbs::proc_surface::{EmbeddedVertexBlendBoundary,
        EmbeddedVertexBlendBoundaryGeometry};
    EmbeddedVertexBlend {
        revision: None,
        boundaries: [0.0, 1.0, 2.0].into_iter().take(count).map(|x|
            EmbeddedVertexBlendBoundary {
                boundary_type: false,
                magic: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0),
                u_smoothing: false, v_smoothing: false, fullness: 0.0,
                geometry: EmbeddedVertexBlendBoundaryGeometry::Degenerate {
                    location: cadmpeg_ir::math::Point3::new(x, 0.0, 0.0),
                    normals: [cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0); 2],
                },
            }).collect(),
        grid_size: 1,
        fit_tolerance: cadmpeg_ir::geometry::FitTolerance::try_new(0.0).unwrap(),
    }
}

#[test]
fn vertex_blend_source_refuses_one_visit_before_boundary_emission() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    let Err(CodecError::ResourceLimit(first)) = emit_vertex_blend_surface(
        &ctx, &mut out, 1, vertex_blend(3), crate::asm_format!("f3d"),
    ) else {
        panic!("expected the first boundary source visit to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM procedural members");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for count in [3, 0] {
        assert!(matches!(emit_vertex_blend_surface(
            &ctx, &mut out, 1, vertex_blend(count), crate::asm_format!("f3d"),
        ), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(out.curves.is_empty() && out.surfaces.is_empty());
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn vertex_blend_source_accepts_exact_visits_and_empty_input() {
    for count in [3, 0] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // This isolates ASM source visits; IR constructor admission is separate.
        policy.limits.max_work_units = u64::try_from(count).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut out = AsmBrep::default();
        let result = emit_vertex_blend_surface(
            &ctx, &mut out, 1, vertex_blend(count), crate::asm_format!("f3d"),
        ).unwrap();
        let ProceduralSurfaceDefinition::VertexBlend(payload) = result else {
            panic!("expected the original vertex blend");
        };
        let construction = payload.construction();
        assert!(construction.revision.is_none());
        assert_eq!(construction.grid_size, 1);
        assert_eq!(construction.fit_tolerance,
            cadmpeg_ir::geometry::FitTolerance::try_new(0.0).unwrap());
        assert_eq!(construction.boundaries.len(), count);
        for (boundary, x) in construction.boundaries.iter().zip([0.0, 1.0, 2.0]) {
            let boundary = boundary.to_raw();
            assert!(!boundary.boundary_type && !boundary.u_smoothing && !boundary.v_smoothing);
            assert_eq!(boundary.magic, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0));
            assert_eq!(boundary.fullness, 0.0);
            assert!(matches!(boundary.geometry,
                VertexBlendBoundaryGeometry::Degenerate { location, normals }
                if location == cadmpeg_ir::math::Point3::new(x, 0.0, 0.0)
                    && normals == [cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0); 2]));
        }
        assert!(out.curves.is_empty() && out.surfaces.is_empty());
        ctx.finish_session().unwrap();
    }
}
