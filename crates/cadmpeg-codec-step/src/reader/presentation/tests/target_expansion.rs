// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

#[test]
fn style_target_expansion_keeps_order_duplicates_cycles_and_depth_bound() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRIC_SET('',(#2,#3,#5));#2=GEOMETRIC_SET('',(#4));#3=GEOMETRIC_CURVE_SET('',(#4,#1));#4=CARTESIAN_POINT('',(0.,0.,0.));#5=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    crate::test_support::with_service_context(source, |_, ctx| {
        for (graph_limit, expected) in [(64, vec![4, 4, 5]), (2, vec![5])] {
            let mut active_storage = ctx.reserve_scoped(0, "test active targets").expect("scope");
            let mut claim_storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
            let mut typed = BTreeSet::new();
            let mut active = BTreeSet::new();
            let mut targets = Vec::new();
            super::super::expand_style_targets(
                1,
                &exchange,
                (&mut typed, &mut claim_storage),
                (&mut active, &mut active_storage),
                (0, graph_limit),
                &mut |id| ctx.push_vec(&mut targets, id, "target fixture"),
                ctx,
            )
            .expect("expanded targets");
            assert_eq!(targets, expected);
            assert_eq!(typed, BTreeSet::from([1, 2, 3]));
            assert!(active.is_empty());
        }
        let mut active_storage = ctx.reserve_scoped(0, "test active targets").expect("scope");
        let mut claim_storage = ctx
            .reserve_scoped(0, "missing claim fixture")
            .expect("scope");
        let mut typed = BTreeSet::new();
        let mut active = BTreeSet::new();
        let mut targets = Vec::new();
        super::super::expand_style_targets(
            99,
            &exchange,
            (&mut typed, &mut claim_storage),
            (&mut active, &mut active_storage),
            (0, 64),
            &mut |id| ctx.push_vec(&mut targets, id, "missing target fixture"),
            ctx,
        )
        .expect("missing record leaf");
        assert_eq!(targets, [99]);
        assert!(typed.is_empty());
        assert!(active.is_empty());
    });
}
