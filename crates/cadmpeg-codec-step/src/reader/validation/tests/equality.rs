// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::CadIr;

#[test]
fn validation_mesh_body_option_equality_preserves_refusal() {
    let mut ir = CadIr::empty();
    let id = BodyId::try_from("step:data:body#1").unwrap();
    ir.model.bodies.push(Body {
        id: id.clone(), kind: BodyKind::Sheet, regions: Vec::new(),
        transform: None, name: None, color: None, visible: None,
    });
    let mesh = TessellationMesh::from_list_lanes(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![[0, 1, 2]], None,
    ).unwrap();
    let mut tessellation = Tessellation::new(
        TessellationId::try_from("step:tessellation:mesh#2").unwrap(), mesh, Vec::new(),
    ).unwrap();
    tessellation.body = Some(id);
    ir.model.tessellations.push(tessellation);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One mesh visit fits; optional body-ID equality refuses before geometry aggregation.
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(refusal)) = super::super::mesh_properties(&ir, &ctx) else {
        panic!("body comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP validation mesh body equality");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
