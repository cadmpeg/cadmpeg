// SPDX-License-Identifier: Apache-2.0
//! Known-length searches stop at a match or at the actual source boundary.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::{ArchiveVersion, FramingError};
use crate::wire::Uuid;

fn assert_rdk_no_terminal_visit(opaque: bool, count: usize) {
    let mut descriptor = super::legacy_rdk_descriptor(0..0);
    let crate::objects::UserdataDescriptor::Known(value) = &mut descriptor else {
        panic!("known userdata fixture");
    };
    value.class_uuid = Uuid::nil();
    let input: Vec<_> = (0..count).map(|_| descriptor.clone()).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(count).unwrap();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    if opaque {
        assert!(!crate::presentation::rdk_material_userdata_requires_opaque(&ctx, &[], &input).unwrap());
    } else {
        assert_eq!(crate::presentation::legacy_rdk_material_instance_id(&ctx, &[], &input).unwrap(), None);
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_rdk_identity_search_has_no_terminal_visit() {
    assert_rdk_no_terminal_visit(false, 0);
}

#[test]
fn complete_rdk_identity_search_visits_only_the_source_items() {
    assert_rdk_no_terminal_visit(false, 1024);
}

#[test]
fn empty_rdk_opaque_search_has_no_terminal_visit() {
    assert_rdk_no_terminal_visit(true, 0);
}

#[test]
fn complete_rdk_opaque_search_visits_only_the_source_items() {
    assert_rdk_no_terminal_visit(true, 1024);
}

#[test]
fn empty_rdk_searches_keep_the_original_refusal() {
    for opaque in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "Rhino original RDK search refusal").unwrap_err()
            else { panic!("original work refusal"); };
        let error = if opaque {
            crate::presentation::rdk_material_userdata_requires_opaque(&ctx, &[], &[]).map(|_| ())
        } else {
            crate::presentation::legacy_rdk_material_instance_id(&ctx, &[], &[]).map(|_| ())
        }.unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(refusal) if refusal == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}

#[test]
fn absent_rdk_instance_attribute_preserves_the_structural_error_without_a_visit() {
    let xml = "<xml><render-content-manager-data><material/></render-content-manager-data></xml>";
    let bytes = super::legacy_rdk_payload(xml, false, &[]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "Rhino RDK instance attribute search", None);
    let error = crate::presentation::classify_rdk_material_payload(&ctx, &bytes, 0..bytes.len()).unwrap_err();
    drop(probe);
    assert!(matches!(error, FramingError::Structural { offset: 0, message }
        if message == "legacy RDK material has no instance-id attribute"));
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn light_without_attribute_userdata_executes_no_search_visit() {
    use crate::test_support::test_dump::{class_wrapper, short_chunk};
    let archive = ArchiveVersion::V5;
    let mut bytes = class_wrapper(archive, crate::presentation::LIGHT.to_wire(), &[]);
    bytes.extend(short_chunk(archive, crate::presentation::LIGHT_RECORD_END, 0));
    let record = crate::container::Record::long(0x2000_8060, 0..bytes.len(), 0..bytes.len());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "Rhino parse light record attributes traversal", None);
    let mut losses = Vec::new();
    let value = crate::presentation::parse_light_record_attributes(
        &ctx, &bytes, &record, archive, None, &mut losses,
    ).unwrap();
    drop(probe);
    assert!(value.is_none());
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn mapping_primitive_without_userdata_executes_no_cache_search_visit() {
    use crate::test_support::test_dump::{class_wrapper, MESH_CLASS};
    let mut payload = MESH_CLASS.to_vec();
    payload.extend(6_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    for _ in 0..2 {
        for index in 0..16 {
            payload.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend(class_wrapper(ArchiveVersion::V8, MESH_CLASS, &[]));
    let bytes = super::anonymous(0, &payload);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "Rhino parse texture mapping traversal", None);
    let mapping = crate::presentation::parse_texture_mapping(
        &ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8, 42,
    ).unwrap();
    drop(probe);
    assert_eq!(mapping.value.mapping_type, 6);
    assert_eq!(mapping.value.primitive_class_uuid, Some(Uuid::from_wire(MESH_CLASS).to_string()));
    assert!(!mapping.cache_requires_opaque);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

fn assert_empty_install_search(table_type: u32, record_type: u32, class_uuid: Uuid,
    operation: &'static str, application_name: Option<&str>, arena_name: &str) {
    use crate::test_support::test_dump::{class_wrapper, crc_chunk, minimal_document, set_test_units, table};
    let archive = ArchiveVersion::V5;
    let record = crc_chunk(archive, record_type, &class_wrapper(archive, class_uuid.to_wire(), &[]));
    let bytes = minimal_document("50", &[
        table(archive, 0x1000_0014, &[]), table(archive, 0x1000_0015, &[]),
        table(archive, table_type, &[record]), table(archive, 0x1000_0013, &[]),
    ]);
    let mut scan = crate::container::scan_owned(bytes).unwrap();
    assert_eq!(scan.tables[2].records.len(), 1);
    set_test_units(&mut scan, 1.0);
    scan.metadata.properties.application = application_name.map(|name| crate::settings::Application {
        source: crate::settings::SourceRange { range: 0..0 },
        name: name.to_owned(), url: String::new(), details: String::new(),
    });
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let installed = crate::presentation::install(&ctx, &scan, &mut ir).unwrap();
    drop(probe);
    // The empty class payload is malformed but still has its original source owner.
    assert_eq!(installed.opaque_records.len(), 1);
    assert!(!installed.losses.is_empty());
    assert!(ir.native.namespace("rhino").unwrap().arenas()[arena_name].is_empty());
    drop(installed);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn material_without_userdata_executes_no_pbr_search_visit() {
    assert_empty_install_search(0x1000_0010, 0x2000_8040, crate::presentation::MATERIAL,
        "Rhino PBR userdata search", None, "materials");
}

#[test]
fn dimension_style_without_userdata_executes_no_extra_search_visit() {
    assert_empty_install_search(0x1000_0020, 0x2000_8075, crate::presentation::V5_DIMSTYLE,
        "Rhino dimension style userdata search", None, "dimension_styles");
}

#[test]
fn short_application_name_executes_no_font_window_search_visit() {
    assert_empty_install_search(0x1000_0019, 0x2000_8074, crate::presentation::TEXT_STYLE,
        "Rhino font application search", Some("ab"), "text_styles");
}
