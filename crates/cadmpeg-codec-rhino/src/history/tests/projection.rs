// SPDX-License-Identifier: Apache-2.0

use super::Value;

#[test]
fn history_value_joins_preserve_the_scalar_and_fixed_lane_spelling() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (value, expected) in [
        (Value::Booleans(vec![true, false]), "true,false"),
        (Value::Colors(vec![[1, 2, 3, 4], [5, 6, 7, 8]]), "1,2,3,4;5,6,7,8"),
        (Value::Strings(vec!["".into(), "text".into(), "".into()]), "\u{1f}text\u{1f}"),
    ] {
        assert_eq!(crate::history::value_text(&ctx, &value).expect("joined value"), Some(expected.to_owned()));
    }
}

#[test]
fn history_empty_string_joins_admit_each_source_step() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let value = Value::Strings(vec![String::new(), String::new()]);
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino history value text", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::history::value_text(&ctx, &value);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(limit)); }
        result
    });
}
