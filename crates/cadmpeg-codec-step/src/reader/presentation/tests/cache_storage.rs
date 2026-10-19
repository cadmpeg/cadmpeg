// SPDX-License-Identifier: Apache-2.0
//! Local color resolution memos for style graphs.

use std::collections::BTreeSet;

#[test]
fn style_search_memoizes_every_visited_style_record() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('',1.,0.,0.);#2=FILL_AREA_STYLE_COLOUR('',#1);#3=FILL_AREA_STYLE('',(#2));#4=SURFACE_STYLE_FILL_AREA(#3);#5=PRESENTATION_STYLE_ASSIGNMENT((#4));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("bounded style graph");
    crate::test_support::with_service_context(source, |_, ctx| {
        let mut cache = super::super::ColorCache::default();
        let result = super::super::find_color(
            5,
            &exchange,
            super::super::StyleDomain::Surface,
            super::super::ColorSearchState {
                active: &mut BTreeSet::new(),
                cache: &mut cache,
                losses: &mut Vec::new(),
                invalid_surface_sides: &mut BTreeSet::new(),
            },
            ctx,
        )
        .expect("style graph resolves")
        .expect("resolved red");
        let super::super::ColorResolution::Candidate(color) = result else {
            panic!("one scalar color");
        };
        assert_eq!(color.color.r(), 1.0);
        assert_eq!(color.color.g(), 0.0);
        assert_eq!(color.color.b(), 0.0);
        let mut sources = cache.keys().map(|(id, _)| *id).collect::<Vec<_>>();
        sources.sort_unstable();
        assert_eq!(sources, [1, 2, 3, 4, 5]);
    });
}

#[test]
fn terminal_style_domains_resolve_without_recursion() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=LINE('',#1,#1);#3=PLANE('',#1);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("domain fixtures");
    crate::test_support::with_service_context(source, |_, ctx| {
        for (id, expected) in [
            (1, super::super::StyleDomain::Point),
            (2, super::super::StyleDomain::Curve),
            (3, super::super::StyleDomain::Surface),
        ] {
            assert!(
                super::super::style_domain(
                    id,
                    &exchange,
                    &mut std::collections::BTreeMap::new(),
                    ctx
                )
                .expect("terminal domain has no recursion state")
                    == expected
            );
        }
    });
}
