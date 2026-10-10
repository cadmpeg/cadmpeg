// SPDX-License-Identifier: Apache-2.0
//! Caller-context limits for decoded STEP presentation text.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{find_color, ColorResolution, StyleDomain};

const LAYER_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();#2=PRESENTATION_LAYER_ASSIGNMENT('Layer','details',(#1));ENDSEC;END-ISO-10303-21;";

fn layer_result(retained_limit: u64) -> Result<(), CodecError> {
    let (exchange, _) =
        crate::test_support::with_service_context(LAYER_SOURCE, crate::parse::parse_inner)
            .expect("valid layer exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(LAYER_SOURCE, &arena, &policy)
        .expect("root fits retained policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx)?;
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &ctx)?;
    super::super::decode(&exchange, &topology.value, &mut ir, &BTreeMap::new(), &ctx)?;
    Ok(())
}

#[test]
fn presentation_layer_name_refuses_retained_limit() {
    assert!(matches!(
        layer_result(1),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn presentation_layer_description_refuses_retained_limit() {
    assert!(matches!(
        layer_result(u64::try_from("Layer".len()).expect("fixed name length fits u64")),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

fn color_result(
    source: &[u8],
    materialized_limit: u64,
) -> Result<Option<ColorResolution>, CodecError> {
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid colour exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits retained policy");
    let result = find_color(
        1,
        &exchange,
        StyleDomain::Any,
        super::super::ColorSearchState {
            storage: &std::cell::RefCell::new(
                ctx.reserve_scoped(0, "color search fixture")
                    .expect("scope"),
            ),
            active: &mut BTreeSet::new(),
            cache: &mut super::super::ColorCache::default(),
                    completed: None,
            losses: (
                &mut Vec::new(),
                &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope")),
            ),
            invalid_surface_sides: &mut BTreeSet::new(),
        },
        0,
                    None,
        &ctx,
    );
    result
}

#[test]
fn rgb_colour_name_refuses_materialized_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('named colour',1.,0.,0.);ENDSEC;END-ISO-10303-21;";
    assert!(matches!(
        Err::<(), CodecError>(cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::MaterializedBytes, "step_string_text", |cap| color_result(SOURCE, cap))),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::MaterializedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn predefined_colour_name_refuses_materialized_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DRAUGHTING_PRE_DEFINED_COLOUR('red');ENDSEC;END-ISO-10303-21;";
    assert!(matches!(
        Err::<(), CodecError>(cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::MaterializedBytes, "step_string_text", |cap| color_result(SOURCE, cap))),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::MaterializedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn repeated_colour_retains_output_text_and_appearance_identities() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('',1.,0.,0.);#2=PRESENTATION_STYLE_ASSIGNMENT((#1));#10=STYLED_ITEM('',(#2),#20);#11=STYLED_ITEM('',(#2),#21);#20=SOURCE_ITEM();#21=SOURCE_ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid repeated colour exchange");
    let identity = "step:presentation:appearance#1";
    let mut expected_retained = None;
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "appearance retained probe",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            let index = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx)?;
            let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &ctx)?;
            let outcome =
                super::super::decode(&exchange, &topology.value, &mut ir, &BTreeMap::new(), &ctx)?;
            assert!(outcome.losses.is_empty());
            assert_eq!(ir.model.appearances.len(), 1);
            assert_eq!(ir.model.appearance_bindings.len(), 2);
            assert_eq!(ir.model.appearances[0].id.as_str(), identity);
            for binding in &ir.model.appearance_bindings {
                assert_eq!(binding.appearance.as_str(), identity);
            }
            // Retain arena slots, three appearance identity copies, the schema,
            // two source target IDs and two source entity IDs. Minted binding
            // identities need an admission-backed IR composition operation.
            // The appearance lookup index and color-query text are scoped.
            let output_slots = ir.model.appearances.capacity()
                * std::mem::size_of::<cadmpeg_ir::appearance::Appearance>()
                + ir.model.appearance_bindings.capacity()
                    * std::mem::size_of::<cadmpeg_ir::appearance::AppearanceBinding>();
            expected_retained =
                Some(u64::try_from(output_slots + 3 * identity.len() + "step_surface_style".len() + "#20".len() + "#21".len() + "#10".len() + "#11".len()).expect("fixture size"));
            ctx.charge_retained(1, "appearance retained probe")
        },
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    assert_eq!(Some(refusal.used), expected_retained);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn presentation_warning_text_refuses_before_report_insertion() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=INVISIBILITY($);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner).expect("invisibility exchange");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("setup");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology").value;
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        assert!(matches!(super::super::decode(&exchange, &topology, &mut ir, &BTreeMap::new(), ctx), Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "step_presentation_loss_text"
                && limit.used == 0
                && ctx.resource_refusal() == Some(limit)));
        assert!(ir.model.appearances.is_empty());
        assert!(ir.model.appearance_bindings.is_empty());
    });
}
