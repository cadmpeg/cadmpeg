// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn assert_route_refusal(container_only: bool, dimension: ResourceDimension) {
    let source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    assert_source_route_refusal(&source, container_only, dimension);
}

fn assert_source_route_refusal(source: &[u8], container_only: bool, dimension: ResourceDimension) {
    let mut options = DecodeOptions {
        container_only,
        policy: DecodePolicy::service(),
    };
    let expected = crate::SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .unwrap()
        .ir()
        .clone();
    assert!(!expected.model.features.is_empty());
    assert!(!expected.model.parameters.is_empty());
    let set_limit = |options: &mut DecodeOptions, limit| match dimension {
        ResourceDimension::CollectionItems => options.policy.limits.max_collection_items = limit,
        ResourceDimension::MaterializedBytes => {
            options.policy.limits.max_materialized_bytes = limit;
        }
        ResourceDimension::WorkUnits => options.policy.limits.max_work_units = limit,
        _ => panic!("unexpected baseline hash limit"),
    };
    let run = |options: &DecodeOptions| match crate::SldprtCodec
        .decode(&mut Cursor::new(source), options)
    {
        Ok(decoded) => {
            assert_eq!(decoded.ir(), &expected);
            true
        }
        Err(cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
            assert_eq!(limit.dimension, dimension);
            false
        }
        Err(error) => panic!("unexpected baseline route error: {error}"),
    };
    // Key insertion order can change the prefix. Keep the boundary assertions
    // on the same pair of fresh runs used to select their allowance.
    let (upper, admitted, refused) = (0..64)
        .find_map(|_| {
            let mut lower = 0;
            let mut upper = 1_u64;
            loop {
                set_limit(&mut options, upper);
                if run(&options) {
                    break;
                }
                upper = upper.checked_mul(2).unwrap();
            }
            while lower < upper {
                let midpoint = lower + (upper - lower) / 2;
                set_limit(&mut options, midpoint);
                if run(&options) {
                    upper = midpoint;
                } else {
                    lower = midpoint + 1;
                }
            }
            set_limit(&mut options, upper);
            let admitted = run(&options);
            set_limit(&mut options, upper - 1);
            let refused = run(&options);
            (admitted && !refused).then_some((upper, admitted, refused))
        })
        .expect("a success and its one-unit-below refusal");
    assert!(upper > 0);
    assert!(admitted);
    assert!(!refused);
}

#[test]
fn geometry_baseline_hashes_refuse_collection_limit() {
    assert_route_refusal(false, ResourceDimension::CollectionItems);
}
#[test]
fn geometry_baseline_hashes_refuse_scoped_limit() {
    assert_route_refusal(false, ResourceDimension::MaterializedBytes);
}
#[test]
fn geometry_baseline_hashes_refuse_work_limit() {
    assert_route_refusal(false, ResourceDimension::WorkUnits);
}
#[test]
fn metadata_baseline_hashes_refuse_collection_limit() {
    assert_route_refusal(true, ResourceDimension::CollectionItems);
}
#[test]
fn metadata_baseline_hashes_refuse_scoped_limit() {
    assert_route_refusal(true, ResourceDimension::MaterializedBytes);
}
#[test]
fn metadata_baseline_hashes_refuse_work_limit() {
    assert_route_refusal(true, ResourceDimension::WorkUnits);
}

fn many_history_records() -> Vec<u8> {
    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    let mut xml = String::from(
        "<Keywords Name=\"Repeated features\"><Configuration Name=\"Default\" SourceIndex=\"0\"/>",
    );
    for index in (0..32).rev() {
        use std::fmt::Write;
        write!(xml, "<Extrusion Name=\"Boss{index}\" Type=\"BossExtrude\" id=\"{index}\"><Dimension Name=\"Depth\">12.5mm</Dimension></Extrusion>").unwrap();
    }
    xml.push_str("</Keywords>");
    source.extend(crate::test_support::container::make_block(
        0x42,
        "Contents/Keywords",
        xml.as_bytes(),
    ));
    let decoded = crate::SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.features.len() >= 32);
    assert!(decoded.ir().model.parameters.len() >= 32);
    source
}

#[test]
fn geometry_stable_sort_route_refuses_scoped_limit() {
    assert_source_route_refusal(
        &many_history_records(),
        false,
        ResourceDimension::MaterializedBytes,
    );
}
#[test]
fn geometry_stable_sort_route_refuses_work_limit() {
    assert_source_route_refusal(&many_history_records(), false, ResourceDimension::WorkUnits);
}
#[test]
fn metadata_stable_sort_route_refuses_scoped_limit() {
    assert_source_route_refusal(
        &many_history_records(),
        true,
        ResourceDimension::MaterializedBytes,
    );
}
#[test]
fn metadata_stable_sort_route_refuses_work_limit() {
    assert_source_route_refusal(&many_history_records(), true, ResourceDimension::WorkUnits);
}

fn parameter_baseline_document() -> cadmpeg_ir::document::CadIr {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("sldprt:test"),
        )),
        std::collections::BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("unchanged"),
            String::from("kept"),
        )]),
    ));
    ir
}

#[test]
fn parameter_baseline_insertion_keeps_the_canonical_digest_and_existing_metadata() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = parameter_baseline_document();
    assert!(ir.model.parameters.is_empty());
    crate::decode::stamp_parameter_baseline(&ctx, &mut ir).unwrap();
    let attributes = &ir.source.as_ref().unwrap().attributes;
    assert_eq!(attributes.len(), 2);
    assert_eq!(attributes.get("unchanged").unwrap(), "kept");
    // Canonical JSON for an empty parameter list is [], whose SHA-256 is fixed.
    assert_eq!(
        attributes
            .get("sldprt_neutral_parameter_local_sha256")
            .unwrap(),
        "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
    );
}

fn parameter_baseline_insertion_refusal(dimension: ResourceDimension) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        dimension,
        "insert SLDPRT parameter baseline digest",
        |cap| {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => panic!("unexpected insertion dimension"),
            }
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = parameter_baseline_document();
            let original = ir.source.as_ref().unwrap().attributes.clone();
            let result = crate::decode::stamp_parameter_baseline(&ctx, &mut ir);
            if let Err(CodecError::ResourceLimit(ref limit)) = result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                assert_eq!(ir.source.as_ref().unwrap().attributes, original);
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == "insert SLDPRT parameter baseline digest"));
}

#[test]
fn parameter_baseline_insertion_collection_refusal_reaches_the_caller() {
    parameter_baseline_insertion_refusal(ResourceDimension::CollectionItems);
}

#[test]
fn parameter_baseline_insertion_work_refusal_reaches_the_caller() {
    parameter_baseline_insertion_refusal(ResourceDimension::WorkUnits);
}
