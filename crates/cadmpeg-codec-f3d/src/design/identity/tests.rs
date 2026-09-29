// SPDX-License-Identifier: Apache-2.0
use super::neutral_configuration_id;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn configuration_identifier_refuses_retained_limit() {
    let entry = "asset/encoded:#% \u{2003}.dsgcfg";
    let name = "wide \u{a0} variant";
    let expected = crate::ids::neutral_configuration_id(entry, name);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.as_str().len()).unwrap() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(neutral_configuration_id(Some(&ctx), entry, name), Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == "f3d configuration identifier"));
}

#[test]
fn configuration_identifier_preserves_encoded_bytes() {
    for entry in ["plain.dsgcfg", "asset/a:#% b\u{2003}ç.dsgcfg"] {
        for name in ["Small", "v:#%\u{a0}ç"] {
            let expected = crate::ids::neutral_configuration_id(entry, name);
            let arena = DecodeArena::new();
            let policy = DecodePolicy::default();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert_eq!(neutral_configuration_id(Some(&ctx), entry, name).unwrap(), expected);
        }
    }
}

#[test]
fn projected_configuration_identifier_refuses_retained_limit() {
    let table = crate::records::configuration::DesignConfiguration::try_new(
        "asset/a:#% b\u{2003}ç.dsgcfg".to_owned(),
        crate::records::configuration::DesignConfigurationKind::Table,
        vec!["v:#%\u{a0}ç".to_owned()],
        serde_json::json!({"configurations":{"v:#%\u{a0}ç":{}}}).as_object().unwrap().clone(),
    ).unwrap();
    let name_bytes = u64::try_from(table.variants()[0].0.len()).unwrap();
    let id = crate::ids::neutral_configuration_id(table.entry_name(), &table.variants()[0].0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = name_bytes + u64::try_from(id.as_str().len()).unwrap() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(crate::design::configurations::project_configurations(Some(&ctx), &[table]), Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == "f3d configuration identifier"));
}
