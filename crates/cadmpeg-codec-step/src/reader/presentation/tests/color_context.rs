// SPDX-License-Identifier: Apache-2.0
//! Ordered colour walks keep contextual cycle results.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    ColorCache, ColorCompletions, ColorResolution, ColorSearchState, StyleColors, StyleDomain,
};

pub(super) fn shared<'source, 'ctx, 'arena>(
    exchange: &'source crate::parse::Exchange,
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'arena>,
) -> StyleColors<'source, 'ctx, 'arena> {
    StyleColors {
            exchange,
            prefixes: BTreeMap::new(),
            values: BTreeMap::new(),
            queries: Vec::new(),
            frames: Vec::new(),
            frame_storage: ctx
                .reserve_scoped(0, "context colour frames")
                .expect("scope"),
            completed: ColorCompletions::new(ctx).expect("completions"),
            ctx,
            storage: ctx
                .reserve_scoped(0, "context colour metadata")
                .expect("scope"),
    }
}

fn alpha(color: Option<ColorResolution>) -> f32 {
    let Some(ColorResolution::Candidate(candidate)) = color else {
        panic!("red candidate")
    };
    assert_eq!(
        (
            candidate.color.r(),
            candidate.color.g(),
            candidate.color.b()
        ),
        (1.0, 0.0, 0.0)
    );
    candidate.color.a()
}

#[test]
fn ordered_cycle_queries_keep_partial_local_color_results() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('red',1.,0.,0.);#2=ITEM((#3,#1));#3=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(#2,#4);#4=SURFACE_STYLE_TRANSPARENT(0.5);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("cycle exchange");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut shared = shared(&exchange, ctx);
        let reports = RefCell::new(
            ctx.reserve_scoped(0, "context colour reports")
                .expect("scope"),
        );
        for (references, expected) in [([2, 3], 1.0), ([3, 2], 0.5), ([2, 3], 1.0), ([3, 2], 0.5)] {
            let direct_storage =
                RefCell::new(ctx.reserve_scoped(0, "direct colours").expect("scope"));
            let mut direct_cache = ColorCache::default();
            let mut direct_losses = Vec::new();
            let mut active = BTreeSet::new();
            let mut invalid = BTreeSet::new();
            let direct = super::super::combine_color_resolutions(references.map(|id| {
                super::super::find_color(
                    id,
                    &exchange,
                    StyleDomain::Surface,
                    ColorSearchState {
                        storage: &direct_storage,
                        active: &mut active,
                        cache: &mut direct_cache,
                        completed: None,
                        losses: (&mut direct_losses, &reports),
                        invalid_surface_sides: &mut invalid,
                    },
                    0,
                    None,
                    ctx,
                )
            }))
            .expect("direct cycle colours");
            // Starting at #2 completes #3 with no colour at its active back
            // edge. Starting at #3 applies its transparency to #2's red.
            assert_eq!(alpha(direct), expected);
            let mut losses = Vec::new();
            let cached = shared
                .resolve(&references, StyleDomain::Surface, (&mut losses, &reports))
                .expect("ordered cycle colours")
                .1
                .color;
            assert_eq!(alpha(cached), expected);
            assert_eq!(losses, direct_losses);
            assert!(losses.is_empty());
        }
    });
}

pub(super) fn cutoff_history_exchange() -> crate::parse::Exchange {
    use std::fmt::Write as _;
    // The diamond has 256 terminal paths; the later chain reaches #2 at
    // depth 255. A complete earlier visit supplies its local cached result.
    let mut records = String::from("#1=COLOUR_RGB('red',1.,0.,0.);#2=ITEM(#1);#9=SURFACE_STYLE_TRANSPARENT(0.5);#10=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(#9,#100);");
    for id in 20..=35 {
        let next = ((id - 20) / 2 + 1) * 2 + 20;
        write!(records, "#{id}=ITEM((#{next},#{}));", next + 1).expect("diamond");
    }
    records.push_str("#36=ITEM(#2);#37=ITEM(#2);#500=COLOUR_RGB('other',0.,0.,1.);");
    for id in 100..354 {
        write!(records, "#{id}=ITEM(#{});", if id == 353 {2} else {id+1}).expect("chain");
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("history exchange").0
}

#[test]
fn shared_color_completions_preserve_both_cutoff_history_orders() {
    let exchange = cutoff_history_exchange();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut shared = shared(&exchange, ctx);
        let reports = RefCell::new(ctx.reserve_scoped(0, "history reports").expect("scope"));
        let mut losses = Vec::new();
        assert_eq!(alpha(shared.resolve(&[20], StyleDomain::Surface, (&mut losses, &reports)).expect("prewarm").1.color), 1.0);
        for references in [[20, 10], [10, 20], [20, 10], [10, 20]] {
            let direct_storage = RefCell::new(ctx.reserve_scoped(0, "direct history").expect("scope"));
            let mut cache = ColorCache::default();
            let mut active = BTreeSet::new();
            let mut invalid = BTreeSet::new();
            let mut direct_losses = Vec::new();
            let direct = super::super::combine_color_resolutions(references.map(|id| {
                super::super::find_color(id, &exchange, StyleDomain::Surface, ColorSearchState {
                    storage: &direct_storage, active: &mut active, cache: &mut cache,
                    completed: None, losses: (&mut direct_losses, &reports), invalid_surface_sides: &mut invalid,
                }, 0, None, ctx)
            })).expect("direct history");
            let cached = shared.resolve(&references, StyleDomain::Surface, (&mut losses, &reports)).expect("shared history").1.color;
            if references[0] == 20 {
                assert_eq!(alpha(direct), 0.5);
                assert_eq!(alpha(cached), 0.5);
            } else {
                assert!(direct.is_none());
                assert!(cached.is_none());
            }
            assert_eq!(losses, direct_losses);
            assert!(losses.is_empty());
        }
    });
}

#[test]
fn contextual_shared_colors_preserve_claims_after_an_ambiguous_query() {
    let exchange = cutoff_history_exchange();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut shared = shared(&exchange, ctx);
        let reports = RefCell::new(ctx.reserve_scoped(0, "claim history reports").expect("scope"));
        let mut losses = Vec::new();
        assert!(matches!(shared.resolve(&[20, 500], StyleDomain::Surface, (&mut losses, &reports)).expect("ambiguous prewarm").1.color, Some(ColorResolution::Ambiguous { .. })));
        let direct_storage = RefCell::new(ctx.reserve_scoped(0, "direct claim history").expect("scope"));
        let mut cache = ColorCache::default();
        let mut active = BTreeSet::new();
        let mut invalid = BTreeSet::new();
        let mut direct_losses = Vec::new();
        let direct = super::super::combine_color_resolutions([10, 20, 500].map(|id| {
            super::super::find_color(id, &exchange, StyleDomain::Surface, ColorSearchState {
                storage: &direct_storage, active: &mut active, cache: &mut cache,
                completed: None, losses: (&mut direct_losses, &reports), invalid_surface_sides: &mut invalid,
            }, 0, None, ctx)
        })).expect("direct claim history");
        let (query, cached) = shared.resolve(&[10, 20, 500], StyleDomain::Surface, (&mut losses, &reports)).expect("shared claim history");
        let (Some(ColorResolution::Candidate(direct)), Some(ColorResolution::Candidate(cached))) = (direct, cached.color) else { panic!("blue candidate") };
        assert_eq!(direct.id, 500);
        assert_eq!(cached.id, direct.id);
        assert_eq!(cached.color, direct.color);
        let expected: BTreeSet<_> = cache.values.keys().map(|key| key.0).collect();
        assert!(!expected.contains(&1));
        let mut claims = BTreeSet::new();
        let mut storage = ctx.reserve_scoped(0, "history claims").expect("scope");
        shared.claim(query, StyleDomain::Surface, (&mut claims, &mut storage)).expect("history claims");
        assert_eq!(claims, expected);
        assert_eq!(losses, direct_losses);
        assert!(losses.is_empty());
    });
}
