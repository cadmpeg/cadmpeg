// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn assert_route_refusal(container_only: bool, dimension: ResourceDimension) {
    let source = crate::test_support::history::sldprt_with_body_and_history(&crate::test_support::parasolid::triangle_body());
    let mut options = DecodeOptions { container_only, policy: DecodePolicy::service(), ..DecodeOptions::default() };
    let expected = crate::SldprtCodec.decode(&mut Cursor::new(&source), &options).unwrap().ir().clone();
    assert!(!expected.model.features.is_empty());
    assert!(!expected.model.parameters.is_empty());
    let set_limit = |options: &mut DecodeOptions, limit| match dimension {
        ResourceDimension::CollectionItems => options.policy.limits.max_collection_items = limit,
        ResourceDimension::MaterializedBytes => options.policy.limits.max_materialized_bytes = limit,
        ResourceDimension::WorkUnits => options.policy.limits.max_work_units = limit,
        _ => panic!("unexpected baseline hash limit"),
    };
    let run = |options: &DecodeOptions| match crate::SldprtCodec.decode(&mut Cursor::new(&source), options) {
        Ok(decoded) => { assert_eq!(decoded.ir(), &expected); true }
        Err(cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => { assert_eq!(limit.dimension, dimension); false }
        Err(error) => panic!("unexpected baseline route error: {error}"),
    };
    let mut lower = 0;
    let mut upper = 1_u64;
    loop {
        set_limit(&mut options, upper);
        if run(&options) { break; }
        upper = upper.checked_mul(2).unwrap();
    }
    while lower < upper {
        let midpoint = lower + (upper - lower) / 2;
        set_limit(&mut options, midpoint);
        if run(&options) { upper = midpoint; } else { lower = midpoint + 1; }
    }
    assert!(upper > 0);
    set_limit(&mut options, upper); assert!(run(&options));
    set_limit(&mut options, upper - 1); assert!(!run(&options));
}

#[test]
fn geometry_baseline_hashes_refuse_collection_limit() { assert_route_refusal(false, ResourceDimension::CollectionItems); }
#[test]
fn geometry_baseline_hashes_refuse_scoped_limit() { assert_route_refusal(false, ResourceDimension::MaterializedBytes); }
#[test]
fn geometry_baseline_hashes_refuse_work_limit() { assert_route_refusal(false, ResourceDimension::WorkUnits); }
#[test]
fn metadata_baseline_hashes_refuse_collection_limit() { assert_route_refusal(true, ResourceDimension::CollectionItems); }
#[test]
fn metadata_baseline_hashes_refuse_scoped_limit() { assert_route_refusal(true, ResourceDimension::MaterializedBytes); }
#[test]
fn metadata_baseline_hashes_refuse_work_limit() { assert_route_refusal(true, ResourceDimension::WorkUnits); }
