// SPDX-License-Identifier: Apache-2.0
//! Presentation walks visit each shared record once.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::parse::Exchange;

const LEVELS: u64 = 40;
const TOP: u64 = LEVELS + 1;

/// `#1` is `bottom`; `#2` to `#{TOP}` are `set` records that each name the
/// one before twice, so a walk that revisits shared records takes
/// `2^LEVELS` steps.
fn doubling_source(set: &str, bottom: &str) -> String {
    let mut source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1={bottom};");
    for id in 2..=TOP {
        write!(source, "#{id}={set}('',(#{},#{}));", id - 1, id - 1).expect("write record fixture");
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    source
}

fn parse(source: &str) -> Exchange {
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("valid exchange")
        .0
}

#[test]
fn style_domain_resolves_shared_sets_once() {
    let exchange = parse(&doubling_source(
        "GEOMETRIC_SET",
        "CARTESIAN_POINT('',(0.,0.,0.))",
    ));
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut domains = BTreeMap::new();
        assert!(
            super::super::style_domain(TOP, &exchange, &mut domains, ctx).expect("the walk fits")
                == super::super::StyleDomain::Point
        );
        assert_eq!(domains.len(), usize::try_from(TOP).expect("fits"));
    });
}

#[test]
fn style_targets_of_shared_sets_appear_once() {
    let exchange = parse(&doubling_source(
        "GEOMETRIC_SET",
        "CARTESIAN_POINT('',(0.,0.,0.))",
    ));
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut typed = HashSet::new();
        assert_eq!(
            super::super::expand_style_targets(
                TOP,
                &exchange,
                &mut typed,
                &mut BTreeSet::new(),
                ctx
            )
            .expect("the walk fits"),
            [1]
        );
        assert_eq!(typed, (2..=TOP).collect());
    });
}

#[test]
fn invisible_body_walk_visits_shared_representations_once() {
    let source = doubling_source("SHAPE_REPRESENTATION", "ITEM()");
    let exchange = parse(&source);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::service())
            .expect("root fits policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir);
    let topology =
        crate::reader::topology::decode(&exchange, &mut ir, &index, &ctx).expect("topology");
    let (bodies, supported) =
        super::super::invisible_body_ids(TOP, &exchange, &topology.value, &BTreeMap::new(), &ctx)
            .expect("the walk fits");
    assert!(bodies.is_empty());
    assert!(!supported);
}
