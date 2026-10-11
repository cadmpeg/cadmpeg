// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

#[test]
fn pcurve_diagnostics_keep_storage_after_the_identity_map_drops() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .unwrap();
    for release in [false, true] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "pcurve diagnostic lifetime checkpoint",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &data, &arena, &policy,
                )?;
                let mut backing = ctx.reserve_scoped(0, "pcurve fixture backing")?;
                let mut metadata = ctx.reserve_scoped(0, "pcurve fixture metadata")?;
                let super::super::DecodedPcurves { ids, values, warnings, _storage } =
                    super::super::decode_pcurves(
                        (&ctx, &mut backing),
                        &data,
                        ArchiveVersion::V5,
                        brep.raw(),
                        brep.resolved(),
                        "invalid key",
                        &std::collections::HashMap::new(),
                    ).map_err(|error| match error {
                        crate::curves::GeometryError::Codec(error) => error,
                        other => panic!("unexpected pcurve error: {other}"),
                    })?;
                assert!(ids.is_empty());
                assert!(values.is_empty());
                let mut destination = Diagnostics::new();
                metadata.with_storage(|| destination.append_scoped_admitted(
                    &ctx, warnings, "Rhino Brep pcurve diagnostic promotion",
                ))?;
                drop((ids, _storage));
                assert_eq!(destination.len(), 1);
                assert!(destination[0].message.starts_with("Brep pcurve identity key is invalid:"));
                if release {
                    drop(destination);
                    drop(metadata);
                }
                let held = ctx.reserve_scoped(1 << 20, "pcurve diagnostic lifetime checkpoint")?;
                drop(held);
                Ok(())
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("checkpoint refusal");
        };
        if release {
            assert_eq!(limit.used, 0, "consumed diagnostics release all scratch");
        } else {
            assert!(limit.used > 0, "live diagnostics retain their reservation");
        }
    }
}
