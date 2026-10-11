// SPDX-License-Identifier: Apache-2.0
//! Colour name storage grows with source names and emitted appearances.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;

use super::super::{ColorCompletions, ColorResolution, StyleColors, StyleDomain};

#[test]
fn distinct_color_roots_do_not_duplicate_long_names() {
    for (roots, name_len) in [(1, 8), (1, 8192), (128, 8), (128, 8192)] {
        let name = "R".repeat(name_len);
        let mut records = format!("#1=COLOUR_RGB('{name}',1.,0.,0.);");
        for id in 100..100 + roots {
            write!(records, "#{id}=PRESENTATION_STYLE_ASSIGNMENT((#1));").expect("root");
        }
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
        let (exchange, _) = crate::test_support::with_service_context(
            source.as_bytes(), crate::parse::parse_inner,
        ).expect("named colour exchange");
        let mut policy = DecodePolicy::service();
        // Cache metadata and one decoded name fit. 128 copies of an 8-KiB
        // name exceed this cap before counting completion and query payloads.
        policy.limits.max_materialized_bytes = 384 * 1024;
        policy.limits.max_retained_bytes = 0;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut shared = StyleColors {
                exchange: &exchange,
                prefixes: BTreeMap::new(),
                values: BTreeMap::new(),
                queries: Vec::new(),
                frames: Vec::new(),
                frame_storage: ctx.reserve_scoped(0, "named colour frames").expect("scope"),
                completed: ColorCompletions::new(ctx).expect("completions"),
                ctx,
                storage: ctx.reserve_scoped(0, "named colour metadata").expect("scope"),
            };
            let reports = RefCell::new(ctx.reserve_scoped(0, "named colour reports").expect("scope"));
            let mut losses = Vec::new();
            for id in 100..100 + roots {
                let cached = shared.resolve(&[id], StyleDomain::Surface, (&mut losses, &reports))
                    .expect("shared name fits").1;
                let Some(ColorResolution::Candidate(candidate)) = cached.color else {
                    panic!("red candidate")
                };
                assert_eq!(candidate.id, 1);
                assert_eq!(candidate.name, Some(1));
                assert_eq!((candidate.color.r(), candidate.color.g(), candidate.color.b()), (1.0, 0.0, 0.0));
            }
            assert_eq!(shared.completed.names.len(), 1);
            assert_eq!(shared.completed.names[&1], name);
            assert!(losses.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
            drop(shared);
            drop(losses);
            drop(reports);
            let reservation = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "released colour cache")
                .expect("cache releases all name and metadata storage");
            drop(reservation);
        });
    }
}

#[test]
fn named_color_is_copied_into_one_emitted_appearance() {
    let name = "R".repeat(8192);
    let mut records = format!("#1=COLOUR_RGB('{name}',1.,0.,0.);");
    for id in 100..228 {
        write!(records, "#{id}=PRESENTATION_STYLE_ASSIGNMENT((#1));#{}=STYLED_ITEM('',(#{id}),#{});#{}=SOURCE_ITEM();", id + 1000, id + 2000, id + 2000).expect("style");
    }
    let result = crate::test_support::exchange::decode_inline(&records);
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(result.ir().model.appearances[0].name.as_deref(), Some(name.as_str()));
    assert_eq!(result.ir().model.appearance_bindings.len(), 128);
}
