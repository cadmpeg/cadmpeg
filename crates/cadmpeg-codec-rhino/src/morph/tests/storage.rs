// SPDX-License-Identifier: Apache-2.0
use crate::chunks::ArchiveVersion;
use crate::mesh::MeshExpand;
use crate::settings::MillimeterScale;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn raw_morph() -> Vec<u8> {
    let mut transform = Vec::new();
    for value in [
        1.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        transform.extend(value.to_le_bytes());
    }
    let mut body = 3_i32.to_le_bytes().to_vec();
    body.extend(super::anonymous(1, 0, &transform));
    body.extend(super::anonymous(1, 0, &super::cage()));
    let mut captives = 128_i32.to_le_bytes().to_vec();
    for _ in 0..128 {
        captives.extend([0; 16]);
    }
    body.extend(super::anonymous(1, 0, &captives));
    body.extend(super::anonymous(1, 0, &0_i32.to_le_bytes()));
    super::anonymous(2, 0, &body)
}

#[test]
fn morph_raw_controls_use_scoped_storage_and_release_after_projection() {
    let bytes = raw_morph();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    policy.limits.max_materialized_bytes = 16_384;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "Rhino morph captive IDs",
        None,
    );
    let mut storage = ctx.reserve_scoped(0, "Rhino morph raw controls").unwrap();
    let morph = super::super::decode(
        MeshExpand::new(&ctx, root),
        0..bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
        &mut storage,
    )
    .unwrap();
    drop(probe);
    assert_eq!(morph.captive_ids.len(), 128);
    let feature =
        super::super::project(&ctx, &morph, "fixture", None, "native".into(), |_| Ok(None))
            .unwrap();
    drop(morph);
    drop(storage);
    let reclaimed = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test reclaimed raw morph storage",
        )
        .unwrap();
    assert_eq!(feature.source_properties["end_dimension"], "3");
    drop(reclaimed);
    assert!(ctx.finish_session().is_ok());
}
