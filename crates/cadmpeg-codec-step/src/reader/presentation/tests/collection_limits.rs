// SPDX-License-Identifier: Apache-2.0
//! Presentation graph walks refuse at the session recursion depth.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;
use std::collections::HashSet;

fn style_target_refuses(operation: &str) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRIC_SET('',(#2));#2=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("style set exchange");
    let refused = (0..=8).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits style target policy");
        matches!(
            super::super::expand_style_targets(1, &exchange, &mut HashSet::new(), &mut BTreeSet::new(), &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.operation == operation
                    && refusal.dimension == ResourceDimension::RecursionDepth
        )
    });
    assert!(refused, "no refusal for {operation}");
}

#[test]
fn presentation_style_target_walk_refuses_depth_limit() {
    style_target_refuses("step_presentation_style_target_walk");
}

fn style_graph_refuses(
    operation: &str,
    run: impl Fn(&crate::parse::Exchange, &DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OVER_RIDING_STYLED_ITEM('',(),#2,#3);#2=CARTESIAN_POINT('',(0.,0.,0.));#3=STYLED_ITEM('',(),#2);#4=GEOMETRIC_SET('',(#2));#5=NULL_STYLE();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("style graph exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits graph policy");
    assert!(matches!(
        run(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
    ));
}

#[test]
fn presentation_style_depth_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_style_depth_walk", |exchange, ctx| {
        super::super::style_application_order(1, exchange, ctx).map(|_| ())
    });
}

#[test]
fn presentation_style_domain_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_style_domain_walk", |exchange, ctx| {
        super::super::style_domain(4, exchange, ctx).map(|_| ())
    });
}

#[test]
fn presentation_hidden_style_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_hidden_style_walk", |exchange, ctx| {
        super::super::style_is_hidden(1, &BTreeSet::new(), exchange, &mut BTreeSet::new(), ctx)
            .map(|_| ())
    });
}

#[test]
fn presentation_style_inheritance_walk_refuses_depth_limit() {
    style_graph_refuses(
        "step_presentation_style_inheritance_walk",
        |exchange, ctx| {
            super::super::style_inherits_from(1, 3, exchange, &mut BTreeSet::new(), ctx).map(|_| ())
        },
    );
}

#[test]
fn presentation_null_style_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_null_style_walk", |exchange, ctx| {
        super::super::contains_null_style(
            &crate::parse::Value::Reference(5),
            exchange,
            &mut BTreeSet::new(),
            ctx,
        )
        .map(|_| ())
    });
}

#[test]
fn presentation_predefined_color_matches_ascii_case_without_copy() {
    assert_eq!(
        super::super::predefined("ReD"),
        super::super::predefined("red")
    );
}

fn invisible_body_refuses(operation: &str) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("invisibility exchange");
    let setup_arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source, &setup_arena, &DecodePolicy::default())
            .expect("setup root");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir);
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &setup_ctx)
        .expect("setup topology");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
    assert!(matches!(
        super::super::invisible_body_ids(1, &exchange, &topology.value, &std::collections::BTreeMap::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
    ));
}

#[test]
fn presentation_invisible_body_walk_refuses_depth_limit() {
    invisible_body_refuses("step_presentation_invisible_body_walk");
}

#[test]
fn scalar_color_singletons_keep_one_color_per_target() {
    let mut candidates = std::collections::HashMap::new();
    let color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("color");
    for id in 0..1000 {
        let body = cadmpeg_ir::ids::BodyId::mint(format!("step:model:body#{id}")).expect("body ID");
        let target = cadmpeg_ir::appearance::AppearanceTarget::Body(body);
        super::super::push_scalar_candidate(&mut candidates, &target, id, color)
            .expect("target owns its first color inline");
    }
    assert_eq!(candidates.len(), 1000);
    assert!(candidates.values().all(|group| group.iter().count() == 1));
}

#[test]
fn presentation_color_walk_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('red',1.,0.,0.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("colour exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
    let result = super::super::find_color(
        1,
        &exchange,
        super::super::StyleDomain::Any,
        super::super::ColorSearchState {
            active: &mut BTreeSet::new(),
            cache: &mut super::super::ColorCache::default(),
            losses: &mut Vec::new(),
            invalid_surface_sides: &mut BTreeSet::new(),
        },
        &ctx,
    );
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_presentation_color_walk"
    ));
}
