// SPDX-License-Identifier: Apache-2.0

use super::ConfigurationParameterOverlay;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};
use std::collections::BTreeMap;

const FIRST_OVERRIDE: &str = "first override text";
const SECOND_OVERRIDE: &str = "second override text";

fn fixture() -> (Vec<DesignParameter>, BTreeMap<ParameterId, ParameterValue>) {
    let first = ParameterId::mint("synthetic:test:id#first-parameter").unwrap();
    let second = ParameterId::mint("synthetic:test:id#second-parameter").unwrap();
    let parameters = [first.clone(), second.clone()].into_iter().enumerate().map(|(ordinal, id)| {
        DesignParameter {
            id,
            owner: None,
            ordinal: u32::try_from(ordinal).unwrap(),
            name: format!("parameter {ordinal}"),
            expression: format!("original expression {ordinal}"),
            display: None,
            value: (ordinal == 0).then(|| ParameterValue::String("original value".into())),
            dependencies: DistinctMembers::default(),
            properties: BTreeMap::from([(cadmpeg_core::nonblank_literal!("property"), "original metadata".into())]),
            pmi: None,
            native_ref: Some(format!("synthetic-native-{ordinal}")),
        }
    }).collect();
    let values = BTreeMap::from([
        (first, ParameterValue::String(FIRST_OVERRIDE.into())),
        (second, ParameterValue::String(SECOND_OVERRIDE.into())),
    ]);
    (parameters, values)
}

fn overlay_peak() -> u64 {
    // Original reserve_vec first growth: four slots for this entry width.
    // Each admitted String copy reserves exactly its text length.
    let width = std::mem::size_of::<(usize, Option<ParameterValue>)>();
    let slots = match width { 0 => 0, 1 => 8, 2..=1024 => 4, _ => 1 };
    u64::try_from(slots * width + FIRST_OVERRIDE.len() + SECOND_OVERRIDE.len()).unwrap()
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = overlay_peak();
    policy
}

fn assert_overrides(overlay: &ConfigurationParameterOverlay<'_, '_>) {
    assert_eq!(overlay.as_slice()[0].value, Some(ParameterValue::String(FIRST_OVERRIDE.into())));
    assert_eq!(overlay.as_slice()[1].value, Some(ParameterValue::String(SECOND_OVERRIDE.into())));
}

#[test]
fn configuration_parameter_overlay_restores_before_scoped_storage_reuse() {
    let (mut parameters, values) = fixture();
    let expected = parameters.clone();
    let arena = DecodeArena::new();
    let policy = policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let overlay = ConfigurationParameterOverlay::new(&ctx, &mut parameters, &values).unwrap();
        assert_overrides(&overlay);
    }
    assert_eq!(parameters, expected);
    let _reuse = ctx.reserve_scoped(overlay_peak(), "reuse expired parameter overlay").unwrap();
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn configuration_parameter_overlay_restores_partial_construction_on_refusal() {
    let (mut parameters, values) = fixture();
    let expected = parameters.clone();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = overlay_peak() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let limit = match ConfigurationParameterOverlay::new(&ctx, &mut parameters, &values) {
        Err(CodecError::ResourceLimit(limit)) => limit,
        _ => panic!("expected original overlay storage refusal"),
    };
    assert_eq!(parameters, expected);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "overlay SLDPRT configuration parameter values");
    assert_eq!(limit.limit, overlay_peak() - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let replayed = match ConfigurationParameterOverlay::new(&ctx, &mut parameters, &values) {
        Err(CodecError::ResourceLimit(limit)) => limit,
        _ => panic!("expected original sticky overlay refusal"),
    };
    assert_eq!(replayed, limit);
    assert_eq!(parameters, expected);
}

#[test]
fn configuration_parameter_overlay_restores_on_unwind_before_storage_reuse() {
    let (mut parameters, values) = fixture();
    let expected = parameters.clone();
    let arena = DecodeArena::new();
    let policy = policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let overlay = ConfigurationParameterOverlay::new(&ctx, &mut parameters, &values).unwrap();
        assert_overrides(&overlay);
        panic!("synthetic projection unwind");
    }));
    assert!(panic.is_err());
    assert_eq!(parameters, expected);
    let _reuse = ctx.reserve_scoped(overlay_peak(), "reuse unwound parameter overlay").unwrap();
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn configuration_parameter_overlay_empty_overrides_have_no_storage_or_items() {
    let (mut parameters, _) = fixture();
    let expected = parameters.clone();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let overlay = ConfigurationParameterOverlay::new(&ctx, &mut parameters, &BTreeMap::new()).unwrap();
        assert_eq!(overlay.as_slice(), expected);
    }
    assert_eq!(parameters, expected);
    assert!(ctx.resource_refusal().is_none());
}
