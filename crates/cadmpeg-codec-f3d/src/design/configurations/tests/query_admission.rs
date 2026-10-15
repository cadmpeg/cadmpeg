// SPDX-License-Identifier: Apache-2.0

use crate::design::configurations::project_configurations;
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn configuration_nonempty_scan_stops_before_unused_tail() {
    let tables = ["first.dsgcfg", "second.dsgcfg", "tail.dsgcfg"].map(|entry| {
        crate::test_support::with_decode_context(|ctx| {
            DesignConfiguration::try_new_charged(
                ctx,
                entry.into(),
                DesignConfigurationKind::Table,
                vec!["variant".into()],
                serde_json::json!({"configurations": {"variant": {}}})
                    .as_object()
                    .unwrap()
                    .clone(),
            )
        })
        .unwrap()
    });
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "unused configuration tail",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for _ in ctx.admit_iter(&tables[..2], "f3d configuration nonempty tables")? {}
            let mut tail = tables[2..].iter();
            ctx.next_charged(&mut tail, "unused configuration tail")
                .map(|_| ())
        },
    );
    let CodecError::ResourceLimit(refusal) = error else {
        unreachable!("the refusal oracle returns a resource limit");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = refusal.used;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(project_configurations(&ctx, &tables),
        Err(CodecError::NotImplemented(message))
            if message == "independent nonempty F3D configuration tables have no shared authored order"));
}
