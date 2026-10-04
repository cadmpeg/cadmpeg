// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::CodecError;
use crate::feature::definitions::{DefinitionIdentity, FeatureSegmentKind, ScalarLane, TrimEntityKind};
use std::num::NonZeroU32;

#[test]
fn scalar_lane_cost_counts_only_active_value() {
    crate::decode::with_test_decode_ctx(|ctx| {
        // Each value has one tag; a scalar payload contributes eight bytes.
        for (value, expected) in [(ScalarLane::Value(1.0), 9), (ScalarLane::DimensionDriven, 1), (ScalarLane::Undefined, 1)] {
            assert_eq!(value.decode_cost(ctx, "scalar lane cost")?, expected);
        }
        Ok::<(), CodecError>(())
    }).expect("scalar lane costs fit service work");
}

#[test]
fn segment_kind_cost_counts_active_identifiers() {
    crate::decode::with_test_decode_ctx(|ctx| {
        // One tag precedes one or two four-byte identifiers.
        for (value, expected) in [(FeatureSegmentKind::Line([7, 8]), 9), (FeatureSegmentKind::Arc([9, 10]), 9), (FeatureSegmentKind::Point(11), 5)] {
            assert_eq!(value.decode_cost(ctx, "segment kind cost")?, expected);
        }
        Ok::<(), CodecError>(())
    }).expect("segment kind costs fit service work");
}

#[test]
fn trim_kind_cost_counts_only_active_center() {
    crate::decode::with_test_decode_ctx(|ctx| {
        // An arc adds one four-byte center identifier to the variant tag.
        for (value, expected) in [(TrimEntityKind::Line, 1), (TrimEntityKind::Arc { center_vertex: 7 }, 5)] {
            assert_eq!(value.decode_cost(ctx, "trim kind cost")?, expected);
        }
        Ok::<(), CodecError>(())
    }).expect("trim kind costs fit service work");
}

#[test]
fn definition_identity_cost_counts_active_optional_identifiers() {
    crate::decode::with_test_decode_ctx(|ctx| {
        // Variant and option tags cost one byte; each present identifier costs four bytes.
        for (value, expected) in [
            (DefinitionIdentity::Parsed { schema_id: None, owner_feature_id: None }, 3),
            (DefinitionIdentity::Parsed { schema_id: NonZeroU32::new(7), owner_feature_id: None }, 7),
            (DefinitionIdentity::Parsed { schema_id: None, owner_feature_id: Some(9) }, 7),
            (DefinitionIdentity::Parsed { schema_id: NonZeroU32::new(7), owner_feature_id: Some(9) }, 11),
            (DefinitionIdentity::BoundOwner { schema_id: None, owner_feature_id: 9 }, 6),
            (DefinitionIdentity::BoundOwner { schema_id: NonZeroU32::new(7), owner_feature_id: 9 }, 10),
        ] {
            assert_eq!(value.decode_cost(ctx, "definition identity cost")?, expected);
        }
        Ok::<(), CodecError>(())
    }).expect("definition identity costs fit service work");
}

#[test]
fn variable_type_cost_counts_only_active_unknown_code() {
    crate::decode::with_test_decode_ctx(|ctx| {
        // A class has one tag; only Unknown adds a four-byte code.
        for code in [0, 1, 2, 3, 4, 5, 6, 7, 8, u32::MAX] {
            let expected = if code < 8 { 1 } else { 5 };
            assert_eq!(crate::feature::definitions::VariableType::from(code).decode_cost(ctx, "variable type cost")?, expected);
        }
        Ok::<(), CodecError>(())
    }).expect("variable type costs fit service work");
}
