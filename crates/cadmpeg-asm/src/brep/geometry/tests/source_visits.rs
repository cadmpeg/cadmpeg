// SPDX-License-Identifier: Apache-2.0

use super::super::{classify_body_kinds, collect_carrier, reduce_homogeneous_bezier_to_quadratic};
use crate::brep::AsmBrep;
use crate::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn record(tokens: Vec<Token>) -> Record {
    Record { index: 1, name: "carrier".into(), tokens: tokens.into(), offset: 0, len: 0 }
}

#[test]
fn carrier_allocation_refuses_after_one_token_without_visiting_the_tail() {
    let input = record(vec![Token::Position([1.0, 2.0, 3.0]),
        Token::Vector3([0.0, 1.0, 0.0]), Token::Double(4.0)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match collect_carrier(&ctx, &input) {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected the first actual carrier allocation to refuse"),
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "ASM carrier positions");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [&input, &record(Vec::new())] {
        assert!(matches!(collect_carrier(&ctx, replay),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn carrier_token_source_refuses_one_visit_before_any_typed_allocation() {
    let input = record(vec![Token::Position([1.0, 2.0, 3.0]), Token::Double(4.0)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match collect_carrier(&ctx, &input) {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected first carrier token refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM analytic carrier tokens");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert!(matches!(collect_carrier(&ctx, &record(Vec::new())),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn carrier_ignored_tokens_admit_exact_source_length_without_an_end_probe() {
    let input = record(vec![Token::True, Token::False, Token::Long(7)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let carrier = collect_carrier(&ctx, &input).unwrap();
    assert!(carrier.positions.is_empty());
    assert!(carrier.vectors.is_empty());
    assert!(carrier.doubles.is_empty());
    drop(carrier);
    collect_carrier(&ctx, &record(Vec::new())).unwrap();
    ctx.finish_session().unwrap();
}

#[derive(Clone, Copy)]
enum ClassificationSource {
    Regions,
    Shells,
    Faces,
    Loops,
    Coedges,
    Bodies,
}

impl ClassificationSource {
    fn operation(self) -> &'static str {
        match self {
            Self::Regions => "ASM body classification regions",
            Self::Shells => "ASM body classification shells",
            Self::Faces => "ASM body classification faces",
            Self::Loops => "ASM body classification loops",
            Self::Coedges => "ASM body classification coedges",
            Self::Bodies => "ASM body classification bodies",
        }
    }

    fn input(self, count: usize) -> AsmBrep {
        use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId,
            ShellId, SurfaceId, VertexId};
        use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Face, FaceLoops, Loop,
            LoopBoundary, Region, Sense, Shell};
        let mut out = AsmBrep::default();
        for index in [1, 2, 3].into_iter().take(count) {
            let id = format!("f3d:brep:entity#{index}");
            match self {
                Self::Regions => out.regions.push(Region {
                    id: RegionId::mint(id).unwrap(),
                    body: BodyId::mint("f3d:brep:body#owner").unwrap(), shells: Vec::new(),
                }),
                Self::Shells => out.shells.push(Shell::with_face(
                    ShellId::mint(id).unwrap(),
                    RegionId::mint("f3d:brep:region#owner").unwrap(),
                    FaceId::mint("f3d:brep:face#member").unwrap(),
                )),
                Self::Faces => out.faces.push(Face {
                    id: FaceId::mint(id).unwrap(),
                    shell: ShellId::mint("f3d:brep:shell#owner").unwrap(),
                    surface: SurfaceId::mint("f3d:brep:surface#carrier").unwrap(),
                    sense: Sense::Forward, loops: FaceLoops::unspecified(Vec::new()),
                    name: None, color: None, tolerance: None,
                }),
                Self::Loops => out.loops.push(Loop {
                    id: LoopId::mint(id).unwrap(),
                    face: FaceId::mint("f3d:brep:face#owner").unwrap(),
                    boundary: LoopBoundary::Vertex {
                        vertex: VertexId::mint("f3d:brep:vertex#member").unwrap(),
                        pcurves: Vec::new(),
                    },
                }),
                Self::Coedges => out.coedges.push(Coedge {
                    id: CoedgeId::mint(id.clone()).unwrap(),
                    owner_loop: LoopId::mint("f3d:brep:loop#owner").unwrap(),
                    edge: EdgeId::mint("f3d:brep:edge#member").unwrap(),
                    radial_next: CoedgeId::mint(id).unwrap(), sense: Sense::Forward,
                    pcurves: Vec::new(), use_curve: None,
                }),
                Self::Bodies => out.bodies.push(Body {
                    id: BodyId::mint(id).unwrap(), kind: BodyKind::Solid,
                    regions: Vec::new(), transform: None, name: None, color: None, visible: None,
                }),
            }
        }
        out
    }
}

fn assert_classification_fields(out: &AsmBrep, expected: &AsmBrep) {
    assert_eq!(out.regions, expected.regions);
    assert_eq!(out.shells, expected.shells);
    assert_eq!(out.faces, expected.faces);
    assert_eq!(out.loops, expected.loops);
    assert_eq!(out.coedges, expected.coedges);
    assert_eq!(out.bodies, expected.bodies);
}

fn classification_source_refusal(source: ClassificationSource) {
    let mut out = source.input(3);
    let expected = source.input(3);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = classify_body_kinds(&ctx, &mut out) else {
        panic!("expected the first classification source visit to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, source.operation());
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert_classification_fields(&out, &expected);
    assert!(matches!(classify_body_kinds(&ctx, &mut out),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert_classification_fields(&out, &expected);
    assert!(matches!(classify_body_kinds(&ctx, &mut AsmBrep::default()),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

fn classification_source_acceptance(source: ClassificationSource) {
    for count in [3, 0] {
        let mut out = source.input(count);
        let mut expected = source.input(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if matches!(source, ClassificationSource::Regions) || count == 0 {
            policy.limits.max_work_units = u64::try_from(count).unwrap();
        }
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        classify_body_kinds(&ctx, &mut out).unwrap();
        if matches!(source, ClassificationSource::Bodies) {
            for body in &mut expected.bodies {
                body.kind = cadmpeg_ir::topology::BodyKind::Wire;
            }
        }
        assert_classification_fields(&out, &expected);
        ctx.finish_session().unwrap();
    }
}

macro_rules! classification_controls {
    ($refusal:ident, $acceptance:ident, $source:ident) => {
        #[test]
        fn $refusal() { classification_source_refusal(ClassificationSource::$source); }
        #[test]
        fn $acceptance() { classification_source_acceptance(ClassificationSource::$source); }
    };
}

classification_controls!(region_source_refuses_one_visit_before_shells,
    region_source_accepts_exact_visits_and_empty_input, Regions);
classification_controls!(shell_source_refuses_one_visit_before_owner_lookup,
    shell_source_preserves_unresolved_members_and_accepts_empty_input, Shells);
classification_controls!(face_source_refuses_one_visit_before_owner_lookup,
    face_source_preserves_unresolved_members_and_accepts_empty_input, Faces);
classification_controls!(loop_source_refuses_one_visit_before_owner_lookup,
    loop_source_preserves_unresolved_members_and_accepts_empty_input, Loops);
classification_controls!(coedge_source_refuses_one_visit_before_owner_lookup,
    coedge_source_preserves_unresolved_members_and_accepts_empty_input, Coedges);
classification_controls!(body_source_refuses_one_visit_before_classification,
    body_source_preserves_fields_and_classifies_empty_members_as_wire, Bodies);

fn region_with_shells() -> AsmBrep {
    let mut out = ClassificationSource::Regions.input(1);
    out.regions[0].shells = [1, 2, 3].map(|index|
        cadmpeg_ir::ids::ShellId::mint(format!("f3d:brep:shell#{index}")).unwrap()).into();
    out
}

#[test]
fn region_shell_source_refuses_one_visit_after_one_parent_without_admitting_the_tail() {
    let mut out = region_with_shells();
    let expected = region_with_shells();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = classify_body_kinds(&ctx, &mut out) else {
        panic!("expected the first nested shell source visit to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM region shells");
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert_classification_fields(&out, &expected);
    for mut replay in [region_with_shells(), AsmBrep::default()] {
        assert!(matches!(classify_body_kinds(&ctx, &mut replay),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn region_shell_source_preserves_owner_ids_and_shell_order() {
    let mut out = region_with_shells();
    let expected = region_with_shells();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    classify_body_kinds(&ctx, &mut out).unwrap();
    assert_classification_fields(&out, &expected);
    ctx.finish_session().unwrap();
}

#[test]
fn rational_reduction_source_refuses_one_visit_after_copy_and_row_entry() {
    let controls = [[0.0, 0.0, 0.0, 1.0]; 4];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Four copied source items and one reduction-row entry precede the poles.
    policy.limits.max_work_units = 4 + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Some(Err(CodecError::ResourceLimit(first))) =
        reduce_homogeneous_bezier_to_quadratic(&ctx, &controls) else {
        panic!("expected the first reduction pole source visit to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM rational reduction poles");
    assert_eq!((first.limit, first.used, first.additional), (5, 5, 1));
    for replay in [controls.as_slice(), &[]] {
        assert!(matches!(reduce_homogeneous_bezier_to_quadratic(&ctx, replay),
            Some(Err(CodecError::ResourceLimit(last))) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn rational_reduction_allocation_refuses_before_any_reduction_pole_visit() {
    let controls = [[0.0, 0.0, 0.0, 1.0]; 4];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4 + 1;
    let copied_bytes = u64::try_from(std::mem::size_of_val(&controls)).unwrap();
    policy.limits.max_materialized_bytes = copied_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Some(Err(CodecError::ResourceLimit(first))) =
        reduce_homogeneous_bezier_to_quadratic(&ctx, &controls) else {
        panic!("expected the reduced buffer allocation to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "ASM rational four-arc degree reduction");
    let reduced_bytes = u64::try_from(3 * std::mem::size_of::<[f64; 4]>()).unwrap();
    assert_eq!((first.limit, first.used, first.additional),
        (copied_bytes, copied_bytes, reduced_bytes));
    for replay in [controls.as_slice(), &[]] {
        assert!(matches!(reduce_homogeneous_bezier_to_quadratic(&ctx, replay),
            Some(Err(CodecError::ResourceLimit(last))) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn quadratic_controls_execute_no_reduction_visits_and_empty_input_has_no_copy() {
    let controls = [[0.0, 0.0, 0.0, 1.0]; 3];
    for source in [controls.as_slice(), &[]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(source.len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = reduce_homogeneous_bezier_to_quadratic(&ctx, source).transpose().unwrap();
        assert_eq!(result, (!source.is_empty()).then_some(controls));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn rational_reduction_preserves_the_degree_elevated_constant_curve() {
    let controls = [[0.0, 0.0, 0.0, 1.0]; 4];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(reduce_homogeneous_bezier_to_quadratic(&ctx, &controls).transpose().unwrap(),
        Some([[0.0, 0.0, 0.0, 1.0]; 3]));
    ctx.finish_session().unwrap();
}
