// SPDX-License-Identifier: Apache-2.0
//! Retained mesh body admission through the STEP tessellation reader.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;

const BODY_LINKED_TRIANGLE: &str = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_SOLID('',(#2),#10);
#10=TESSELLATED_ITEM();";

fn decode_with_body(policy: DecodePolicy) -> Result<CadIr, CodecError> {
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;{BODY_LINKED_TRIANGLE}ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("test exchange parses");
    let mut ir = CadIr::empty();
    let geometry = super::super::super::geometry::decode(&exchange, &mut ir).value;
    let index = super::super::super::index::CarrierIndex::from_ir(&ir);
    let mut topology = super::super::super::topology::decode(&exchange, &mut ir, &index, None)
        .expect("test topology decodes")
        .value;
    topology.body_by_root.insert(
        10,
        vec![BodyId::try_from("step:data:body#10").expect("test body id")],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)?;
    super::super::decode(&exchange, &geometry, &topology, &mut ir, &ctx)?;
    Ok(ir)
}

#[test]
fn tessellation_mesh_body_copy_is_charged_before_clone() {
    let service = DecodePolicy::service();
    let ir = decode_with_body(service).expect("service admits mesh body");
    assert_eq!(ir.model.tessellations.len(), 1);
    let mut limited = service;
    let prior_bytes =
        8 + 72 + 24 + 12 + std::mem::size_of::<cadmpeg_ir::tessellation::Tessellation>();
    limited.limits.max_retained_bytes =
        u64::try_from(prior_bytes + "step:data:body#10".len() - 1).expect("test bytes fit u64");
    let error = decode_with_body(limited).expect_err("body copy exceeds retained byte limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_mesh_body")
    );
}
