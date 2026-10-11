// SPDX-License-Identifier: Apache-2.0
//! Ordered colour walks keep contextual cycle results.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    ColorCache, ColorCompletions, ColorResolution, ColorSearchState, StyleColors, StyleDomain,
};

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
        let mut shared = StyleColors {
            exchange: &exchange,
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
        };
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
