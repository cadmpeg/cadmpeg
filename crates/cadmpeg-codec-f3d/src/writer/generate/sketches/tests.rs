// SPDX-License-Identifier: Apache-2.0

#[test]
fn sketch_records_share_the_existing_output_allowance() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchPlacement};

    let mut document = cadmpeg_ir::CadIr::empty();
    document.model.sketches.push(Sketch {
        id: "authored:model:sketch#42".try_into().unwrap(),
        name: Some("Sketch_42".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let native = crate::native::F3dNative::default();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_entities = 5;
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    ctx.charge_entities(2, "earlier output records").unwrap();
    assert!(super::encode(&document, &native, &ctx).unwrap().is_some());

    let arena = cadmpeg_core::decode::DecodeArena::new();
    policy.limits.max_entities = 4;
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    ctx.charge_entities(2, "earlier output records").unwrap();
    let Err(error) = super::encode(&document, &native, &ctx) else {
        panic!("the third record must exceed the shared allowance");
    };
    assert!(
        matches!(error, crate::writer::WriteError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
            && (limit.limit, limit.used, limit.additional) == (4, 4, 1))
    );
}
