// SPDX-License-Identifier: Apache-2.0
//! Shared characteristic representations contribute claims once.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiDimension};
use cadmpeg_ir::CadIr;

#[test]
fn shared_characteristic_representation_claims_are_discovered_once() {
    let mut records =
        String::from("#1=DIMENSIONAL_SIZE($,'width');#2=SHAPE_DIMENSION_REPRESENTATION('',(");
    for id in 10..74 {
        if id != 10 {
            records.push(',');
        }
        write!(records, "#{id}").expect("write reference");
    }
    records.push_str("),$);");
    for id in 10..74 {
        write!(records, "#{id}=MEASURE_REPRESENTATION_ITEM('',1.);").expect("write measure");
    }
    for id in 100..228 {
        write!(
            records,
            "#{id}=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#2);"
        )
        .expect("write characteristic");
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("characteristic exchange");
    let mut policy = DecodePolicy::service();
    // 128 rescans of 64 measures require 8192 record lookups, each costing
    // 33 * 8 work units at this record count, before classification.
    policy.limits.max_work_units = 300_000;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        // The read-only fixture arena and index use the queried session. Their
        // backing stays scoped through the claim pass; retained limit remains zero.
        let (_fixture_storage, fixture) = ctx.with_scoped_storage("test annotation claim input", || {
            let mut fixture_ir = CadIr::empty();
            let mut annotations =
                super::super::annotations::Annotations::new(ctx).expect("annotation index");
            annotations
                .push(
                    &mut fixture_ir,
                    1,
                    super::super::annotations::AnnotationDraft {
                        name: None,
                        targets: Vec::new(),
                        visible: None,
                        definition: PmiDefinition::Dimension(
                            PmiDimension::new(DimensionKind::Size, None, None).expect("dimension"),
                        ),
                    },
                )
                .expect("characteristic annotation");
            Ok::<_, cadmpeg_core::CodecError>((fixture_ir, annotations))
        }).map(|(value, storage)| (storage, value)).expect("scoped annotation fixture");
        let (_fixture_ir, annotations) = fixture;
        let mut claims = BTreeSet::new();
        let mut storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
        super::super::mark_characteristic_representations(
            &exchange,
            &annotations,
            (&mut claims, &mut storage),
            ctx,
        )
        .expect("shared claim scan fits work budget");
        let expected = std::iter::once(2)
            .chain(10..74)
            .chain(100..228)
            .collect::<BTreeSet<_>>();
        assert_eq!(claims, expected);
    });
}
