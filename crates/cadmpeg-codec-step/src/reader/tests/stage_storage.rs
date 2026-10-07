// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn dependency_buffers_remain_scoped_until_the_outcome_drops() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DOCUMENT('id','name','',$);#2=DOCUMENT_REFERENCE(#1,'source',$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let expected = "external document id (name) from source";
    let mut held_usage = None;
    let mut released_usage = None;
    for (hold, usage) in [(true, &mut held_usage), (false, &mut released_usage)] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "stage lifetime probe",
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = limit;
                // Only the final note text is retained; claim nodes and report slots are scratch.
                policy.limits.max_retained_bytes =
                    u64::try_from(expected.len()).expect("fixture length");
                let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
                let outcome = super::super::dependencies::decode(&exchange, &ctx)?;
                assert_eq!(outcome.notes, [expected]);
                assert_eq!(outcome.claims, std::collections::BTreeSet::from([1, 2]));
                let outcome = hold.then_some(outcome);
                // This probe exceeds the fixture's previous scratch peak in either state.
                let result = ctx.reserve_scoped(4096, "stage lifetime probe").map(|_| ());
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                drop(outcome);
                result
            },
        );
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("resource refusal");
        };
        *usage = Some(refusal.used);
    }
    assert!(held_usage.expect("held usage") > released_usage.expect("released usage"));
}

#[test]
fn product_claim_and_report_buffers_release_before_its_indices() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PRODUCT('id','name','\\X\\GG',$);#2=PRODUCT_DEFINITION_FORMATION('','',#1);#3=PRODUCT_DEFINITION('','',#2,$);#4=PRODUCT_DEFINITION_SHAPE('','',#3);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let (geometry, topology) = crate::test_support::with_service_context(source, |_, ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let geometry = crate::reader::geometry::decode(&exchange, &mut ir, ctx).expect("geometry");
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("carrier index");
        let topology =
            crate::reader::topology::decode(&exchange, &mut ir, &index, ctx).expect("topology");
        (geometry.value, topology.value)
    });
    let mut held_usage = None;
    let mut released_usage = None;
    for (hold, usage) in [(true, &mut held_usage), (false, &mut released_usage)] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "product lifetime probe",
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
                let mut session = super::super::StepDecodeSession::new(
                    &exchange,
                    &[],
                    &ctx,
                    super::super::DecodeMode::Decode(super::super::Packaging::Bare),
                )?;
                let mut outcome = crate::reader::product::decode(
                    &exchange,
                    &geometry,
                    &topology,
                    &mut session.ir,
                    &ctx,
                    &mut session.admitted_ir_entities,
                )?;
                assert!(!outcome.claims.is_empty());
                assert!(!outcome.losses.is_empty());
                session.absorb(&mut outcome)?;
                let (data, claims, reports) = outcome.value;
                assert_eq!(data.product_definition_ids_by_source.len(), 1);
                assert_eq!(data.product_definition_ids_by_shape.len(), 1);
                let guards = hold.then_some((claims, reports));
                // Exceed the earlier scratch peak while the result indices stay live.
                let result = ctx
                    .reserve_scoped(1024 * 1024, "product lifetime probe")
                    .map(|_| ());
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                drop(guards);
                drop(data);
                result
            },
        );
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("resource refusal");
        };
        *usage = Some(refusal.used);
    }
    assert!(held_usage.expect("held usage") > released_usage.expect("released usage"));
}
