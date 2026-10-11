// SPDX-License-Identifier: Apache-2.0
//! Native configuration lookup ownership and assignment order.

use crate::decode::assign_native_configuration_indices;
use crate::native::SldprtNative;
use crate::records::{Configuration, FeatureHistory};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn fixture() -> (CadIr, SldprtNative) {
    let native_record = |id: &str, source_index| Configuration {
        id: id.into(),
        parent: "sldprt:history#first".into(),
        ordinal: 0,
        source_index,
        name: "Configuration".into(),
        material: None,
        properties: BTreeMap::new(),
    };
    let history = |id: &str, configurations| FeatureHistory {
        id: id.into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations,
        features: Vec::new(),
    };
    let mut duplicate = native_record("sldprt:configuration#shared", Some(91));
    duplicate.parent = "sldprt:history#second".into();
    let native = SldprtNative {
        feature_histories: vec![
            history(
                "sldprt:history#first",
                vec![
                    native_record("sldprt:configuration#shared", Some(81)),
                    native_record("sldprt:configuration#unselected", Some(82)),
                ],
            ),
            history("sldprt:history#second", vec![duplicate]),
        ],
        feature_input_lanes: Vec::new(),
        pmi_dimensions: Vec::new(),
    };
    let mut ir = CadIr::empty();
    for (ordinal, source_index, native_ref) in [
        (0, Some(7), Some("sldprt:configuration#shared")),
        (1, None, Some("sldprt:configuration#shared")),
        (2, Some(9), Some("sldprt:configuration#missing")),
        (3, Some(10), None),
    ] {
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint(format!("sldprt:model:configuration#{ordinal}"))
                .expect("configuration identity"),
            ordinal,
            active: false,
            source_index,
            name: None,
            material: None,
            properties: BTreeMap::new(),
            bodies: None,
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: native_ref.map(str::to_owned),
        });
    }
    (ir, native)
}

#[cfg(target_pointer_width = "64")]
#[test]
fn native_configuration_lookup_releases_scratch_and_preserves_order() {
    // Two distinct borrowed keys fit one admitted B-tree node:
    // 11 * (16-byte &str + 16-byte position) + 16 * 8-byte metadata
    // + 2 * 8-byte alignment = 496. Two assignments use the first four
    // 24-byte Vec slots = 96. The simultaneous admitted peak is 592.
    const SCRATCH_BYTES: u64 = 592;
    let (ir, mut native) = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    for _ in 0..64 {
        assign_native_configuration_indices(&ctx, &ir, &mut native)
            .expect("exact scratch peak fits");
        // Both model records target the first native record. The second
        // assignment clears its source index; a later native duplicate loses.
        assert_eq!(
            native.feature_histories[0].configurations[0].source_index,
            None
        );
        assert_eq!(
            native.feature_histories[0].configurations[1].source_index,
            Some(82)
        );
        assert_eq!(
            native.feature_histories[1].configurations[0].source_index,
            Some(91)
        );
        let probe = ctx
            .reserve_scoped(SCRATCH_BYTES, "reuse native configuration scratch")
            .expect("all scratch reusable while native records remain alive");
        drop(probe);
    }
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session()
        .expect("zero retained storage and released scratch");
}

#[cfg(target_pointer_width = "64")]
#[test]
fn native_configuration_lookup_refuses_before_assigning_and_keeps_first_refusal() {
    for (dimension, cap, used, additional) in [
        (ResourceDimension::MaterializedBytes, 591, 496, 96),
        (ResourceDimension::CollectionItems, 1, 1, 1),
    ] {
        let (ir, mut native) = fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            _ => unreachable!("fixed test dimensions"),
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
        let CodecError::ResourceLimit(limit) =
            assign_native_configuration_indices(&ctx, &ir, &mut native)
                .expect_err("lookup and assignments must be admitted before mutation")
        else {
            panic!("expected original resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(
            limit.operation,
            "assign SLDPRT native configuration indices"
        );
        assert_eq!(limit.limit, cap);
        assert_eq!(limit.used, used);
        assert_eq!(limit.additional, additional);
        assert_eq!(
            native.feature_histories[0].configurations[0].source_index,
            Some(81)
        );
        assert_eq!(
            native.feature_histories[0].configurations[1].source_index,
            Some(82)
        );
        assert_eq!(
            native.feature_histories[1].configurations[0].source_index,
            Some(91)
        );
        assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(first)) if first == limit
        ));
    }
}

#[test]
fn native_configuration_index_skips_histories_without_native_references() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    for empty in [false, true] {
        let (mut ir, mut native) = fixture();
        if empty {
            ir.model.configurations.clear();
        } else {
            for configuration in &mut ir.model.configurations {
                configuration.native_ref = None;
            }
        }
        let before = native.feature_histories.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(
            ResourceDimension::MaterializedBytes,
            "assign SLDPRT native configuration indices",
            None,
        );
        assign_native_configuration_indices(&ctx, &ir, &mut native).unwrap();
        assert_eq!(native.feature_histories, before);
        ctx.finish_session().unwrap();
    }
}
