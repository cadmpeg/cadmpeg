// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;
use crate::brep::{AsmBrep, Carriers, DecodePurpose, Reachable};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

fn with_owner(mut run: impl FnMut(&DecodeContext<'_>, &mut ScopedReservation<'_>, Option<ResourceLimit>)) {
    for dimension in [None, Some(ResourceDimension::WorkUnits), Some(ResourceDimension::CollectionItems),
        Some(ResourceDimension::MaterializedBytes), Some(ResourceDimension::RetainedBytes),
        Some(ResourceDimension::Entities), Some(ResourceDimension::RecursionDepth)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty caller input");
        let mut storage = ctx.reserve_scoped(0, "test actual carrier owner").expect("owner before refusal");
        let original = dimension.map(|dimension| {
            let refused = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "test original carrier refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original carrier refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original carrier refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original carrier refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "test original carrier refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("test original carrier refusal").map(|_| ()),
                _ => panic!("carrier refusal dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = refused else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 { run(&ctx, &mut storage, original); }
        drop(storage);
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().expect("fresh carrier owner finishes"),
        }
    }
}

fn surface(seed: bool) {
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)).expect("plane input fixture")));
    let record = crate::sab::Record { index: 7, name: "plane".into(), tokens: Vec::new().into(), offset: 0, len: 0 };
    with_owner(|ctx, storage, original| {
        if seed && original.is_none() { return; }
        let mut carriers = Carriers::default();
        if seed { carriers.surface_geo.insert(7, geometry.clone()); }
        let mut out = AsmBrep::default();
        let before = serde_json::to_value(&out).expect("context-free output snapshot");
        let result = super::super::emit_carrier_surface(ctx, &mut out, &record, 7,
            (&mut carriers, storage, DecodePurpose::Model), &Reachable::default(), crate::asm_format!("sat"));
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => result.expect("missing carrier is free"),
        }
        assert_eq!(serde_json::to_value(&out).expect("context-free output snapshot"), before);
        assert_eq!(carriers.surface_geo.len(), usize::from(seed));
        if seed { assert_eq!(carriers.surface_geo.get(&7), Some(&geometry)); }
    });
}

#[test]
fn asm_absent_surface_carrier_preserves_original_refusal() { surface(false); }

#[test]
fn asm_refused_surface_carrier_keeps_source_map() { surface(true); }

fn curve(seed: bool) {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0)).expect("line input fixture")));
    with_owner(|ctx, _, original| {
        if seed && original.is_none() { return; }
        let mut carriers = Carriers::default();
        if seed { carriers.curve_geo.insert(7, geometry.clone()); }
        let mut out = AsmBrep::default();
        let before = serde_json::to_value(&out).expect("context-free output snapshot");
        let result = super::super::emit_carrier_curve(ctx, &mut out, 7, &mut carriers,
            &std::collections::HashSet::new(), &std::collections::HashSet::new(), crate::asm_format!("sat"));
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => result.expect("missing carrier is free"),
        }
        assert_eq!(serde_json::to_value(&out).expect("context-free output snapshot"), before);
        assert_eq!(carriers.curve_geo.len(), usize::from(seed));
        if seed { assert_eq!(carriers.curve_geo.get(&7), Some(&geometry)); }
    });
}

#[test]
fn asm_absent_curve_carrier_preserves_original_refusal() { curve(false); }

#[test]
fn asm_refused_curve_carrier_keeps_source_map() { curve(true); }

#[test]
fn asm_absent_loft_path_preserves_original_refusal() {
    let id = cadmpeg_ir::ids::CurveId::mint("sat:brep:entity#7").expect("path input identity");
    with_owner(|ctx, _, original| {
        let mut out = AsmBrep::default();
        let before = serde_json::to_value(&out).expect("context-free output snapshot");
        let result = super::super::emit_loft_path_curve(ctx, &mut out,
            crate::nurbs::proc_surface::EmbeddedLoftPathLayout::Revision(None), id.clone());
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(result.expect("absent loft path is free").is_none()),
        }
        assert_eq!(serde_json::to_value(&out).expect("context-free output snapshot"), before);
    });
}

#[test]
fn asm_null_law_formula_preserves_original_refusal() {
    with_owner(|ctx, _, original| {
        let result = super::super::map_law_formula(ctx,
            crate::nurbs::proc_surface::EmbeddedLawFormula::Null,
            |_, _| panic!("null formula must execute no member callback"));
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(cadmpeg_ir::geometry::LawFormula::Null {}))),
        }
    });
}

#[test]
fn asm_null_law_expression_preserves_original_refusal() {
    let prefix = cadmpeg_ir::identity_key!("7");
    with_owner(|ctx, _, original| {
        let mut out = AsmBrep::default();
        let result = super::super::map_law_expression(ctx, &mut out, crate::asm_format!("sat"),
            super::super::LawExpressionScope::Surface(&prefix), cadmpeg_ir::identity_key!("primary"),
            crate::nurbs::proc_surface::EmbeddedLawExpression::Null);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(cadmpeg_ir::geometry::LawExpression::Null {}))),
        }
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

fn absent_member(pair: bool) {
    use crate::nurbs::proc_surface::LoftProfileData;
    use cadmpeg_ir::geometry::{LoftMemberForm, LoftSubdata};
    let id = cadmpeg_ir::ids::SurfaceId::mint("sat:brep:entity#7").expect("support input identity");
    with_owner(|ctx, _, original| {
        let mut out = AsmBrep::default();
        let subdata = LoftSubdata::Type211 { dimensions: [0, 0], row: [0.0, 1.0] };
        let data = if pair {
            LoftProfileData::RevisionPcurvePair { endpoints: [None; 2], pcurve: None,
                secondary_pcurve: None, asm_extension: None, subdata, direction: None }
        } else {
            LoftProfileData::RevisionSupport { endpoints: [None; 2],
                type_code: std::num::NonZeroI64::new(1).expect("nonzero support discriminator"),
                surface: None, support_bounds: [None; 4], pcurve: None, first_flag: false,
                asm_extension: None, subdata, direction: None }
        };
        let result = super::super::emit_loft_member_form(ctx, &mut out, data, id.clone());
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None if pair => assert!(matches!(result, Ok(LoftMemberForm::PcurvePair {
                pcurve: None, secondary_pcurve: None, .. }))),
            None => assert!(matches!(result, Ok(LoftMemberForm::Support {
                type_code: 1, surface: None, pcurve: None, .. }))),
        }
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_absent_loft_member_support_preserves_original_refusal() { absent_member(false); }

#[test]
fn asm_absent_loft_member_pcurves_preserve_original_refusal() { absent_member(true); }

#[test]
fn asm_absent_rolling_ball_side_preserves_original_refusal() {
    use cadmpeg_ir::geometry::{RollingBallSide, VariableBlendSupportKind};
    with_owner(|ctx, _, original| {
        let mut out = AsmBrep::default();
        let side = RollingBallSide { support_kind: VariableBlendSupportKind::ZeroCurve,
            surface: None, curve: None, pcurve: None, location: Point3::new(0.0, 0.0, 0.0),
            secondary_pcurve: None, extension: None };
        let result = super::super::emit_rolling_ball_side(ctx, &mut out, crate::asm_format!("sat"),
            cadmpeg_ir::identity_key!("7"), side);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => {
                let side = result.expect("absent side is free");
                assert!(side.surface.is_none() && side.curve.is_none() && side.pcurve.is_none());
                assert_eq!(side.support_kind, VariableBlendSupportKind::ZeroCurve);
            }
        }
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_absent_blend_members_preserve_original_refusal() {
    with_owner(|ctx, _, original| {
        let mut out = AsmBrep::default();
        let parts = super::super::BlendSurfaceParts { supports: Box::new([None, None]), spine: None,
            radius_offsets: [1.0, 1.0], cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
            native: None };
        let result = super::super::emit_blend_surface(ctx, &mut out, 7, parts, crate::asm_format!("sat"));
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend(_)))),
        }
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_empty_compound_loft_preserves_original_refusal() {
    use crate::nurbs::proc_surface::{EmbeddedCompoundLoft, EmbeddedCompoundLoftDirection, EmbeddedCompoundLoftTail};
    with_owner(|ctx, _, original| {
        let Some(first) = original else { return; };
        let mut out = AsmBrep::default();
        let embedded = EmbeddedCompoundLoft { scales: Box::new([None, None, None, None]),
            fifth_scale: None, flags: [false; 2], tail: EmbeddedCompoundLoftTail::Zero {
                flags: [false; 2], direction: EmbeddedCompoundLoftDirection::Vector(Vector3::new(0.0, 0.0, 1.0)),
                trailing_flags: [false; 2] } };
        let result = super::super::emit_compound_loft_surface(ctx, &mut out, 7, embedded, crate::asm_format!("sat"));
        assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

fn absent_surface_curve_layout<F>() -> crate::nurbs::proc_curve::EmbeddedSurfaceCurveLayout<F> {
    use crate::nurbs::proc_curve::{EmbeddedIntersection, EmbeddedSurfaceCurveLayout, SupportSlot};
    EmbeddedSurfaceCurveLayout::ContextFirst(EmbeddedIntersection {
        surfaces: [SupportSlot::Absent, SupportSlot::Absent], pcurves: [None, None],
        parameter_range: [0.0, 1.0], discontinuities: [Vec::new(), Vec::new(), Vec::new()],
    })
}

fn original_carrier_error<T>(result: Result<T, super::super::CarrierCurveError>, first: ResourceLimit) {
    assert!(matches!(result, Err(super::super::CarrierCurveError::Resource(
        CodecError::ResourceLimit(last))) if last == first));
}

#[test]
fn asm_absent_surface_curve_layout_preserves_original_refusal() {
    with_owner(|ctx, _, original| {
        let Some(first) = original else { return; };
        let mut out = AsmBrep::default();
        original_carrier_error(super::super::emit_surface_curve_layout(ctx, &mut out, 7,
            crate::asm_format!("sat"), absent_surface_curve_layout::<bool>(), None), first);
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_absent_surface_curve_families_preserve_original_refusal() {
    use crate::nurbs::proc_curve::EmbeddedSurfaceCurve;
    with_owner(|ctx, _, original| {
        let Some(first) = original else { return; };
        let families = [EmbeddedSurfaceCurve::Blend(absent_surface_curve_layout()),
            EmbeddedSurfaceCurve::SurfaceConstrained(absent_surface_curve_layout()),
            EmbeddedSurfaceCurve::Parametric(absent_surface_curve_layout()),
            EmbeddedSurfaceCurve::Skin(absent_surface_curve_layout())];
        for family in families {
            let mut out = AsmBrep::default();
            original_carrier_error(super::super::emit_surface_curve_family(ctx, &mut out, 7,
                crate::asm_format!("sat"), family, None), first);
            assert!(out.surfaces.is_empty() && out.curves.is_empty());
        }
    });
}

#[test]
fn asm_spring_support_ranges_preserve_original_refusal() {
    let ranges = [[0.0, 1.0], [0.0, 1.0]];
    with_owner(|ctx, _, original| {
        let mut out = AsmBrep::default();
        let result = super::super::emit_spring_support(ctx, &mut out, 7, crate::asm_format!("sat"), 0,
            crate::nurbs::proc_curve::EmbeddedSpringSupport::Ranges(ranges));
        match original {
            Some(first) => original_carrier_error(result, first),
            None => assert!(matches!(result, Ok(cadmpeg_ir::geometry::SpringSupport::Ranges(value)) if value == ranges)),
        }
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_absent_spring_geometry_preserves_original_refusal() {
    use crate::nurbs::proc_curve::{EmbeddedSpring, EmbeddedSpringLayout, EmbeddedSpringPcurve, EmbeddedSpringSupport};
    with_owner(|ctx, _, original| {
        let Some(first) = original else { return; };
        let mut out = AsmBrep::default();
        let range = [0.0, 1.0];
        let embedded = EmbeddedSpring { layout: EmbeddedSpringLayout::ContextFirst {
            supports: Box::new([EmbeddedSpringSupport::Ranges([range; 2]), EmbeddedSpringSupport::Ranges([range; 2])]),
            first_pcurve: Box::new(EmbeddedSpringPcurve::Range(range)), second_pcurve: None,
            parameter_range: range, discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            discontinuity_flag: false }, direction: 0 };
        original_carrier_error(super::super::emit_spring_curve(ctx, &mut out, 7, embedded,
            crate::asm_format!("sat"), None), first);
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_missing_law_curve_domain_preserves_original_refusal() {
    use crate::nurbs::proc_curve::{EmbeddedLawCurve, EmbeddedLawCurveLayout, SupportSlot};
    with_owner(|ctx, _, original| {
        let Some(first) = original else { return; };
        let mut out = AsmBrep::default();
        let embedded = EmbeddedLawCurve { surfaces: [SupportSlot::Absent, SupportSlot::Absent],
            pcurves: [None, None], discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            layout: EmbeddedLawCurveLayout::Version { stamp: 23_100, post_enum: 0, parameter_range: [None; 2] },
            extension: 0, primary: crate::nurbs::proc_surface::EmbeddedLawFormula::Null, additional: Vec::new() };
        original_carrier_error(super::super::emit_law_curve(ctx, &mut out, 7, embedded,
            crate::asm_format!("sat"), None), first);
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_missing_surface_offset_domain_preserves_original_refusal() {
    use crate::nurbs::proc_curve::{EmbeddedSpringLayout, EmbeddedSurfaceOffset, EmbeddedSurfaceOffsetLayout, ProceduralCurveConstruction};
    use crate::sab::{Record, Token};
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("spring_int_cur".into()),
        Token::Long(23_100), Token::Enum(0), Token::Ident("nubs".into()), Token::Long(1),
        Token::Enum(0), Token::Long(2), Token::Double(2.0), Token::Long(1),
        Token::Double(5.0), Token::Long(1)];
    tokens.extend([0.0, 0.0, 0.0, 1.0, 0.0, 0.0].map(Token::Double));
    tokens.extend([Token::Double(0.0004), Token::Ident("null_surface".into()),
        Token::Ident("null_surface".into()), Token::Ident("nullbs".into()), Token::Ident("nullbs".into()),
        Token::False, Token::False, Token::Long(0), Token::Long(0), Token::Long(0),
        Token::Long(7), Token::Enum(4), Token::SubtypeClose]);
    let records = [Record { index: 7, name: "intcurve".into(), tokens: tokens.into(), offset: 0, len: 0 }];
    let table = super::subtype_table(&records);
    with_owner(|ctx, _, original| {
        let Some(first) = original else { return; };
        let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(
            &cadmpeg_test_support::service_decode_context(), &records[0].tokens, &table)
            .expect("synthetic spring input").expect("fixture decode admission");
        let ProceduralCurveConstruction::Spring(spring) = decoded.construction else { panic!("spring fixture"); };
        let EmbeddedSpringLayout::CacheFirst { context } = spring.layout else { panic!("cache-first fixture"); };
        let embedded = EmbeddedSurfaceOffset { layout: EmbeddedSurfaceOffsetLayout::CacheFirst {
            context, base_endpoints: [None; 2] }, base_u_range: [0.0, 1.0], base_v_range: [0.0, 1.0],
            base: decoded.curve, base_range: [2.0, 5.0], distance: 1.0, shift: 0.0, scale: 1.0 };
        let mut out = AsmBrep::default();
        original_carrier_error(super::super::emit_surface_offset_curve(ctx, &mut out, 7, embedded,
            crate::asm_format!("sat"), None), first);
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}
