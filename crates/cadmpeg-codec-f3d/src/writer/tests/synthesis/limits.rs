// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn preserved_source_patch_uses_caller_input_limit() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode synthetic BREP");
    let mut input = EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity()));
    input.limits.max_input_bytes = 0;
    let error = F3dCodec
        .plan(input, TargetRequest::Inherit)
        .expect_err("the retained source exceeds the caller's input limit");
    let cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit) = error else {
        panic!("the input ceiling returns a resource limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::InputBytes
    );
    assert_eq!(limit.limit, 0);

    let mut input = EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity()));
    input.limits.max_entities = 0;
    let error = F3dCodec
        .plan(input, TargetRequest::Inherit)
        .expect_err("patch decoding exceeds the caller's entity limit");
    let cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit) = error else {
        panic!("the entity ceiling returns a resource limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(limit.limit, 0);
}

#[test]
fn source_less_empty_brep_charges_its_two_records() {
    let ir = cadmpeg_ir::CadIr::empty();
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = 1;
    let error = F3dCodec.plan(input, TargetRequest::Inherit).unwrap_err();
    assert!(matches!(error, cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
        && (limit.limit, limit.used, limit.additional) == (1, 1, 1)));
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = 2;
    let plan = F3dCodec.plan(input, TargetRequest::Inherit).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
}

#[test]
fn source_less_brep_budget_matches_framed_records() {
    use cadmpeg_asm::kernel_header::RefWidth;
    use std::io::Read;

    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut bytes = Vec::new();
    F3dCodec.plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let name = archive.file_names().map(|name| name.unwrap()).find(|name| name.ends_with(".smbh")).unwrap().into_owned();
    let mut smbh = Vec::new();
    archive.by_name(&name).unwrap().read_to_end(&mut smbh).unwrap();
    let start = cadmpeg_asm::asm_header::record_stream_start(&smbh).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let records = cadmpeg_asm::sab::frame(&ctx, &smbh, start, smbh.len(), RefWidth::Eight, None).unwrap();
    let count = u64::try_from(records.len()).unwrap() + 1;
    let mut input = EncodeInput::new(decoded.ir(), None);
    input.limits.max_entities = count - 1;
    assert!(matches!(F3dCodec.plan(input, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities));
    let mut input = EncodeInput::new(decoded.ir(), None);
    input.limits.max_entities = count;
    F3dCodec.plan(input, TargetRequest::Inherit).unwrap();
}

#[test]
fn source_less_parameter_charges_its_collection_and_record() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.parameters.push(parameter(
        "width", 0, ParameterValue::Length(cadmpeg_ir::scalar::Length::new(42.0).unwrap()),
    ));
    let mut limited = EncodeInput::new(&ir, None);
    limited.limits.max_entities = 3;
    assert!(matches!(F3dCodec.plan(limited, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities));
    let mut exact = EncodeInput::new(&ir, None);
    exact.limits.max_entities = 4;
    F3dCodec.plan(exact, TargetRequest::Inherit).unwrap();
}

#[test]
fn source_less_appearance_charges_both_protein_records() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.appearances.push(cadmpeg_ir::appearance::Appearance {
        id: "authored:model:appearance#1".try_into().unwrap(),
        native_ref: None, name: Some("Prism-001".into()),
        asset_guid: Some("11111111-2222-3333-4444-555555555555".into()),
        library_id: None, visual_guid: None, physical_token: None,
        schema: Some("GenericSchema".into()), category: None,
        base_color: Some(cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.6, 1.0).unwrap()),
        properties: Default::default(), textures: Vec::new(),
    });
    let mut limited = EncodeInput::new(&ir, None);
    limited.limits.max_entities = 3;
    assert!(matches!(F3dCodec.plan(limited, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities));
    let mut exact = EncodeInput::new(&ir, None);
    exact.limits.max_entities = 4;
    F3dCodec.plan(exact, TargetRequest::Inherit).unwrap();
}
