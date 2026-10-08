// SPDX-License-Identifier: Apache-2.0
//! Storage and provenance admission at topology stage boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn topology_refusal(source: &[u8], dimension: ResourceDimension, operation: &'static str) {
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("valid topology source");
    let setup_ctx = cadmpeg_test_support::service_decode_context();
    let mut base = cadmpeg_ir::CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut base, &setup_ctx).expect("fixture geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&base, &setup_ctx)
        .expect("fixture point carriers");
    let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut ir = base.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            _ => panic!("unsupported test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits");
        let result = super::super::decode(&exchange, &mut ir, &carriers, &ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result.map(|_| ())
    });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn direct_topology_loss_slots_use_materialized_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ORIENTED_OPEN_SHELL('',#2,.T.);#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    topology_refusal(source, ResourceDimension::MaterializedBytes, "step_topology_losses");
}

#[test]
fn merged_topology_loss_slots_use_materialized_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHELL_BASED_SURFACE_MODEL('',(#2));#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    topology_refusal(source, ResourceDimension::MaterializedBytes, "step_topology_loss_merge");
}

#[test]
fn invalid_face_name_loss_slots_use_materialized_storage() {
    let source = std::str::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"))
        .expect("fixture is UTF-8")
        .replace("#29=ADVANCED_FACE('',(#26),#28,.T.);",
            r"#29=ADVANCED_FACE('\X2\ZZZZ\X0\',(#26),#28,.T.);");
    topology_refusal(source.as_bytes(), ResourceDimension::MaterializedBytes, "step_invalid_string_losses");
}

#[test]
fn topology_provenance_format_refuses_retained_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ORIENTED_OPEN_SHELL('',#2,.T.);#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    topology_refusal(source, ResourceDimension::RetainedBytes, "STEP topology provenance format");
}

#[test]
fn topology_provenance_tag_refuses_retained_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ORIENTED_OPEN_SHELL('',#2,.T.);#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    topology_refusal(source, ResourceDimension::RetainedBytes, "STEP topology provenance tag");
}
