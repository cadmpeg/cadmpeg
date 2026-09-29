// SPDX-License-Identifier: Apache-2.0
//! Container admission, strict-mode, and resource-limit decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::appearance::sldprt_with_body_and_material;
use crate::test_support::container::add_solidworks_version;
use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::container::synthetic_sldprt;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::ir::strict_options;
use crate::test_support::parasolid::parasolid_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::tessellation::display_list_payload;
use crate::test_support::tessellation::sldprt_with_body_and_display_list;
use crate::SldprtCodec;

fn retained_refusal_at(
    source: &[u8],
    options: &mut DecodeOptions,
    operation: &str,
) -> cadmpeg_ir::DecodeFailure {
    for _ in 0..4096 {
        let refused = SldprtCodec
            .decode(&mut Cursor::new(source), options)
            .expect_err("fixture must refuse retained bytes");
        let cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            &refused
        else {
            panic!("expected retained refusal: {refused:?}");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RetainedBytes
        );
        if limit.operation == operation {
            options.policy.limits.max_retained_bytes = limit.used + limit.additional - 1;
            return SldprtCodec
                .decode(&mut Cursor::new(source), options)
                .expect_err("one byte below the requested copy need refuses");
        }
        let next = limit.used + limit.additional;
        assert!(next > options.policy.limits.max_retained_bytes);
        options.policy.limits.max_retained_bytes = next;
    }
    panic!("target charge was not reached within fixture admissions");
}

fn collection_refusal_at(
    source: &[u8],
    operation: &str,
) -> cadmpeg_core::decode::ResourceLimit {
    collection_refusal_with_options(source, DecodeOptions::default(), operation)
}

fn collection_refusal_with_options(
    source: &[u8],
    mut options: DecodeOptions,
    operation: &str,
) -> cadmpeg_core::decode::ResourceLimit {
    use cadmpeg_core::decode::ResourceDimension;

    options.policy.limits.max_collection_items = 0;
    for _ in 0..1024 {
        let error = SldprtCodec
            .decode(&mut Cursor::new(source), &options)
            .expect_err("collection limit must refuse the decode");
        let cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = error
        else {
            panic!("expected a collection-item refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        if limit.operation == operation {
            options.policy.limits.max_collection_items = limit.used + limit.additional - 1;
            let repeated = SldprtCodec
                .decode(&mut Cursor::new(source), &options)
                .expect_err("one item below the target must refuse");
            assert!(matches!(
                repeated,
                cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == operation
            ));
            return limit;
        }
        let next = limit.used + limit.additional;
        assert!(next > options.policy.limits.max_collection_items);
        options.policy.limits.max_collection_items = next;
    }
    panic!("target collection charge was not reached");
}

fn work_refusal_with_options(
    source: &[u8],
    mut options: DecodeOptions,
    operation: &str,
) -> cadmpeg_core::decode::ResourceLimit {
    use cadmpeg_core::decode::ResourceDimension;

    options.policy.limits.max_work_units = 0;
    for _ in 0..4096 {
        let error = SldprtCodec
            .decode(&mut Cursor::new(source), &options)
            .expect_err("work limit must refuse the decode");
        let cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = error
        else {
            panic!("expected a work-unit refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        if limit.operation == operation {
            options.policy.limits.max_work_units = limit.used + limit.additional - 1;
            let repeated = SldprtCodec
                .decode(&mut Cursor::new(source), &options)
                .expect_err("one work unit below the target must refuse");
            assert!(matches!(
                repeated,
                cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::WorkUnits
                        && refusal.operation == operation
            ));
            return limit;
        }
        let next = limit.used + limit.additional;
        assert!(next > options.policy.limits.max_work_units);
        options.policy.limits.max_work_units = next;
    }
    panic!("target work charge was not reached");
}

fn custom_property_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords Name="Part"><CustomProperty Name="PartNumber">A-123</CustomProperty></Keywords>"#,
    ));
    source
}

fn semantic_note_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords Name="Part"><Note Name="Inspection" Type="Note">REMOVE ALL BURRS</Note></Keywords>"#,
    ));
    source
}

fn configuration_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords Name="Part"><Configuration Name="Inspect" SourceIndex="0" Material="Steel" Finish="Ground"/></Keywords>"#,
    ));
    source
}

#[test]
fn decode_body_stream_selection_refuses_collection_limit() {
    let source = sldprt_with_body(&triangle_body());
    let limit = collection_refusal_at(&source, "collect SLDPRT body streams");
    assert_eq!(limit.additional, 1);
}

#[test]
fn decoded_brep_site_collection_refuses_limit() {
    let source = sldprt_with_body(&triangle_body());
    let limit = collection_refusal_at(&source, "collect SLDPRT B-rep sites");
    assert_eq!(limit.additional, 1);
}

#[test]
fn decoded_brep_header_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = sldprt_with_body(&triangle_body());
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &source,
        &mut options,
        "retain SLDPRT B-rep header description",
    );
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT B-rep header description"
    ));
}

#[test]
fn merged_brep_site_identity_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = triangle_body();
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("first partition", "SCH_SW_33103_11000", &body),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body("second partition", "SCH_SW_33103_11000", &body),
    ));
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &source,
        &mut options,
        "qualify SLDPRT identity",
    );
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "qualify SLDPRT identity"
    ));
}

#[test]
fn geometry_configuration_partitions_refuse_collection_limit() {
    let body = triangle_body();
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("first partition", "SCH_SW_33103_11000", &body),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body("second partition", "SCH_SW_33103_11000", &body),
    ));
    let limit = collection_refusal_at(&source, "index SLDPRT configuration partitions");
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_material_appearance_refuses_collection_limit() {
    let source = sldprt_with_body_and_material(&triangle_body(), "Steel", [80, 90, 100]);
    let limit = collection_refusal_at(&source, "admit SLDPRT material appearance");
    assert_eq!(limit.additional, 1);
}

#[test]
fn geometry_xml_metadata_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks swPath="part.sldprt"/>"#,
    ));
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT XML metadata");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT XML metadata"
    ));
}

#[test]
fn metadata_active_site_refuses_collection_limit() {
    let source = sldprt_with_body(&triangle_body());
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(&source, options, "retain SLDPRT metadata site");
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_document_attributes_refuse_collection_limit() {
    let mut source = outer_header();
    let mut payload = b"moPart_c".to_vec();
    payload.extend_from_slice(&7_u32.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    payload.extend_from_slice(&8_u32.to_le_bytes());
    source.extend(make_block(0x43, "SWObjects", &payload));
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &source,
        options,
        "collect SLDPRT document attributes",
    );
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_linear_unit_name_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = crate::test_support::container::sldprt_with_body_and_envelope(&triangle_body());
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT linear unit name");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT linear unit name"
    ));
}

#[test]
fn metadata_history_xml_refuses_scoped_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = br#"<Keywords Name="Part"><Configuration Name="Default"/></Keywords>"#;
    let mut source = outer_header();
    source.extend(make_block(0x43, "Contents/Keywords", payload));
    let scan = container::scan_bytes(&source);
    let classification = crate::dialect::classify_layers(
        &cadmpeg_test_support::service_decode_context(),
        &scan,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = (payload.len() - 1) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let mut admitted_entities = 0;
    let error = super::super::build_metadata_ir(
        &ctx,
        &scan,
        &classification,
        None,
        &mut admitted_entities,
    )
    .expect_err("history XML text exceeds the scoped materialization limit");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected a resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "materialize SLDPRT history XML");
}

#[test]
fn metadata_history_text_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords Name="Part"><Feature Name="Boss" Type="Custom">text</Feature></Keywords>"#,
    ));
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT feature name");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT feature name"
    ));
}

#[test]
fn metadata_custom_property_projection_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = custom_property_source();
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(
        &source,
        options,
        "project SLDPRT custom properties",
    );
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_custom_property_projection_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = custom_property_source();
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &source,
        &mut options,
        "project SLDPRT custom properties",
    );
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "project SLDPRT custom properties"
    ));
}

#[test]
fn metadata_custom_property_projection_refuses_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = custom_property_source();
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_options(&source, options, "project SLDPRT custom properties");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert!(limit.additional > 0);
}

#[test]
fn metadata_semantic_note_projection_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = semantic_note_source();
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(&source, options, "project SLDPRT semantic notes");
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_semantic_note_projection_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = semantic_note_source();
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "project SLDPRT semantic notes");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "project SLDPRT semantic notes"
    ));
}

#[test]
fn metadata_semantic_note_projection_refuses_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = semantic_note_source();
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_options(&source, options, "project SLDPRT semantic notes");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert!(limit.additional > 0);
}

#[test]
fn metadata_configuration_projection_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = configuration_source();
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = collection_refusal_with_options(&source, options, "project SLDPRT configurations");
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.additional, 1);
}

#[test]
fn metadata_configuration_projection_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = configuration_source();
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "project SLDPRT configurations");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "project SLDPRT configurations"
    ));
}

#[test]
fn metadata_configuration_projection_refuses_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = configuration_source();
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let limit = work_refusal_with_options(&source, options, "project SLDPRT configurations");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert!(limit.additional > 0);
}

#[test]
fn geometry_history_xml_refuses_scoped_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = br#"<Keywords Name="Part"><Configuration Name="Default"/></Keywords>"#;
    let mut source = outer_header();
    source.extend(make_block(0x43, "Contents/Keywords", payload));
    let mut scan = container::scan_bytes(&source);
    let classification = crate::dialect::classify_layers(
        &cadmpeg_test_support::service_decode_context(),
        &scan,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = (payload.len() - 1) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let decoded = super::super::DecodedBrep {
        metadata_header: None,
        brep: crate::brep::graph::Brep::default(),
        configuration_bodies: Vec::new(),
    };
    let mut admitted_entities = 0;
    let error = super::super::build_geometry_ir(
        &ctx,
        &mut scan,
        &classification,
        decoded,
        None,
        &mut admitted_entities,
    )
    .expect_err("history XML text exceeds the scoped materialization limit");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected a resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "materialize SLDPRT history XML");
}

#[test]
fn native_loss_validation_propagates_typed_load_retained_refusal() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = sldprt_with_body_and_history(&triangle_body());
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "load typed native record");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "load typed native record"
    ));
    options.policy = cadmpeg_core::decode::DecodePolicy::service();
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("service profile admits typed native loss validation");
}

#[test]
fn direct_parasolid_stream_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = triangle_body();
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &body);
    let source = sldprt_with_body(&body);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = stream.len() as u64 - 1;
    let error = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &options)
        .expect_err("direct Parasolid copy must be admitted");
    assert!(
        matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain direct Parasolid stream"),
        "{error:?}"
    );

    options.policy = cadmpeg_core::decode::DecodePolicy::service();
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("service profile admits the direct stream");
}

#[test]
fn active_site_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = triangle_body();
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &body);
    let source = sldprt_with_body(&body);
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = (stream.len() * 2) as u64 - 1;
    let error = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &options)
        .expect_err("active site copy must be admitted");
    assert!(
        matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT active site"),
        "{error:?}"
    );

    options.policy = cadmpeg_core::decode::DecodePolicy::service();
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("service profile admits the active site");
}

#[test]
fn display_section_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = triangle_body();
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &body);
    let display = display_list_payload();
    let source = sldprt_with_body_and_display_list(&body);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = (stream.len() + display.len()) as u64 - 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT display section");
    assert!(
        matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT display section"),
        "{error:?}"
    );

    options.policy = cadmpeg_core::decode::DecodePolicy::service();
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("service profile admits the display section");
}

#[test]
fn neutral_brep_constructor_refuses_entity_limit_before_insertion() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = sldprt_with_body(&triangle_body());
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 2;
    let error = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &options)
        .expect_err("neutral B-rep constructor must be admitted");
    assert!(
        matches!(error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit SLDPRT B-rep entity"),
        "{error:?}"
    );

    options.policy = cadmpeg_core::decode::DecodePolicy::service();
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("service profile admits neutral B-rep entities");
}

#[test]
fn whole_source_copy_refuses_retained_limit_before_unknown_record() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = outer_header();
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes = source.len() as u64 - 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT source image");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain SLDPRT source image"
        ),
        "{error:?}"
    );

    options.policy = cadmpeg_core::decode::DecodePolicy::service();
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("service profile admits a small source image");
}

#[test]
fn decode_refuses_when_max_entities_is_zero_before_ir_build() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 0;
    let error = SldprtCodec
        .decode(&mut Cursor::new(synthetic_sldprt()), &options)
        .expect_err("max_entities=0 must refuse at container admission");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit SLDPRT block"
        ),
        "{error:?}"
    );
}

#[test]
fn decode_refuses_when_max_entities_is_below_container_cardinality() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = SldprtCodec
        .decode(&mut Cursor::new(synthetic_sldprt()), &options)
        .expect_err("max_entities below container cardinality must refuse at admission");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn decode_keeps_container_stream_and_model_entity_admission_additive() {
    use cadmpeg_core::decode::ResourceDimension;

    let fixture = sldprt_with_body_and_history(&triangle_body());
    let scan = container::scan_bytes(&fixture);
    let container_entities = scan.blocks.len()
        + scan.compound_streams.len()
        + scan.directory.len()
        + scan.cache_cells.len();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &fixture,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let stream_entities = crate::decode::active_body_streams(&ctx, &scan).unwrap().len();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("decode triangle body");
    let model_entities = decoded.ir().model.entity_count();
    assert!(container_entities > 0);
    assert!(stream_entities > 0);
    assert!(model_entities > 0);

    let previous_undercount = (container_entities + stream_entities).max(model_entities) as u64;
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = previous_undercount;
    let error = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &options)
        .expect_err("container, stream, and model cardinalities must remain additive");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit SLDPRT entities"
        ),
        "{error:?}"
    );

    options.policy.limits.max_entities =
        (container_entities + stream_entities + model_entities) as u64;
    SldprtCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect("the exact additive entity limit must admit the fixture");
}

#[test]
fn strict_accepts_operator_requested_container_only() {
    let fixture = synthetic_sldprt();
    let mut options = strict_options();
    options.container_only = true;
    SldprtCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect("strict container-only decode is accepted");
}

#[test]
fn strict_rejects_unrepresentable_geometry_while_salvage_records_loss_codes() {
    use crate::loss::SldprtLossCode;
    use cadmpeg_ir::report::loss::{LossTaxonomy, StrictConsequence};

    let fixture = synthetic_sldprt();

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage decode keeps the partial result");
    assert!(!salvaged.report().geometry_transferred());
    assert!(salvaged
        .report()
        .losses
        .iter()
        .any(|note| note.code.taxonomy() == LossTaxonomy::GeometryNotTransferred));
    assert!(salvaged
        .report()
        .losses
        .iter()
        .any(|note| note.code.taxonomy() == LossTaxonomy::TopologyNotTransferred));
    assert!(salvaged
        .report()
        .losses
        .iter()
        .any(|note| note.strict_consequence() == StrictConsequence::Reject));

    // Name the code rather than the `sldprt/` prefix: this fixture also
    // declares no `swVersion`, so `source.dialect-unverified` rejects under
    // strict too, and a prefix test would pass on either. The invariant here
    // is that unrepresentable *geometry* is what refuses.
    let strict = SldprtCodec.decode(&mut Cursor::new(fixture), &strict_options());
    match strict {
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) => {
            assert_eq!(
                rejection.loss().code,
                SldprtLossCode::GeometryParasolidNotTransferred.kind(),
                "unexpected loss code: {}",
                rejection.loss().code
            );
        }
        other => panic!("strict decode must reject unrepresentable geometry, got {other:?}"),
    }
}

#[test]
fn strict_accepts_tolerable_gauge_substitution_geometry() {
    use cadmpeg_ir::report::loss::StrictConsequence;

    // The fixture declares a `swVersion` so this test keeps asserting what it
    // is about. A part that declares nothing classifies as `sldprt:unknown`
    // and charges `source.dialect-unverified`, whose strict floor rejects; the
    // invariant here is that a *gauge substitution* stays tolerable, which a
    // missing version declaration would mask.
    let mut fixture = sldprt_with_body_and_history(&triangle_body());
    add_solidworks_version(&mut fixture, 13100);
    let strict = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect("strict decode accepts a tolerable-loss geometry result");
    assert!(strict.report().geometry_transferred());
    assert!(strict
        .report()
        .losses
        .iter()
        .all(|note| note.strict_consequence() == StrictConsequence::Tolerate));
}

#[test]
fn strict_rejects_residual_parasolid_schema_while_salvage_reports_it() {
    use crate::loss::SldprtLossCode;

    let mut fixture = outer_header();
    fixture.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_TEST_1_9999", &triangle_body()),
    ));
    add_solidworks_version(&mut fixture, 13100);

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage decode admits the residual kernel layer");
    assert!(salvaged.report().geometry_transferred());
    assert!(salvaged.report().losses.iter().any(|note| {
        note.code == SldprtLossCode::KernelDialectUnverified.kind()
            && note.message.contains("SCH_TEST_1_9999")
    }));

    let strict = SldprtCodec.decode(&mut Cursor::new(fixture), &strict_options());
    match strict {
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) => assert_eq!(
            rejection.loss().code,
            SldprtLossCode::KernelDialectUnverified.kind()
        ),
        other => panic!("strict decode must reject the residual kernel layer, got {other:?}"),
    }
}

/// Phase 5 freeze: export precondition (:50) rejects shared broken IR; empty accepts.
#[test]
fn phase5_freeze_export_precondition_admissibility_fixtures() {
    let accepted = cadmpeg_test_support::admissibility::accepted_empty();
    // Empty IR has no B-rep; writer refuses later for missing B-rep, but the
    // :50 precondition is full validate — empty passes validate.
    assert!(cadmpeg_ir::validate_neutral(&accepted, Vec::new())
        .expect("resource allocation did not fail")
        .is_ok());
    let rejected = cadmpeg_test_support::admissibility::rejected_missing_point("sldprt:test")
        .expect("fixture identities are valid");
    assert!(!cadmpeg_ir::validate_neutral(&rejected, Vec::new())
        .expect("resource allocation did not fail")
        .is_ok());
}

#[test]
fn configuration_source_index_allocation_rejects_exhaustion() {
    let mut used = std::collections::HashSet::from([u32::MAX]);
    let mut next = u32::MAX;
    let error = crate::writer::reserve_configuration_index(&mut used, &mut next).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

fn regeneration_parent_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Outer" Type="Custom" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_regeneration_parent_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &regeneration_parent_source(), options, "install decoded feature regeneration parent",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_regeneration_parent_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &regeneration_parent_source(), &mut options, "install decoded feature regeneration parent",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "install decoded feature regeneration parent"
    ));
}

#[test]
fn metadata_regeneration_parent_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &regeneration_parent_source(), options, "install decoded feature regeneration parent",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 3);
}

fn composite_curve_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><CompositeCurve Name="Composite" id="10" Segments="curve-a;curve-b"/></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_curve_projection_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &composite_curve_source(), options, "project SLDPRT composite curve segments",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_curve_projection_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &composite_curve_source(), &mut options, "retain SLDPRT datum and curve reference",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT datum and curve reference"
    ));
}

#[test]
fn metadata_curve_projection_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &composite_curve_source(), options, "project SLDPRT composite curve segments",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

fn variable_fillet_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Variable" id="10" Edges="edge-a"><Dimension Name="Radius0">2mm</Dimension><Dimension Name="Position0">0</Dimension><Dimension Name="Radius1">3mm</Dimension><Dimension Name="Position1">1</Dimension></Fillet></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_edit_projection_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &variable_fillet_source(), options, "collect SLDPRT variable fillet radii",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_edit_projection_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &variable_fillet_source(), &mut options, "retain SLDPRT edit selection reference",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT edit selection reference"
    ));
}

#[test]
fn metadata_edit_projection_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &variable_fillet_source(), options, "scan SLDPRT variable fillet radii",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

fn native_definition_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Custom" Type="Custom" id="10"><Dimension Name="Length">1mm</Dimension></Feature></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_native_definition_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &native_definition_source(), options, "collect SLDPRT native definition parameters",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_native_definition_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(
        &native_definition_source(), &mut options, "retain SLDPRT native definition kind",
    );
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT native definition kind"
    ));
}

#[test]
fn metadata_native_definition_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &native_definition_source(), options, "collect SLDPRT native definition parameters",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_surface_projection_refuses_retained_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><TrimSurface Name="Trim" id="10"/></Keywords>"#,
    ));
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT trim surface tool");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT trim surface tool"
    ));
}

fn loft_reference_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Loft Name="Loft" id="10" Profiles="a,b" Guides="c"/></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_loft_projection_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &loft_reference_source(), options, "project SLDPRT loft references",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_loft_projection_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&loft_reference_source(), &mut options, "retain SLDPRT loft reference");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT loft reference"
    ));
}

#[test]
fn metadata_loft_projection_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &loft_reference_source(), options, "project SLDPRT loft references",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 3);
}

#[test]
fn metadata_hole_projection_refuses_retained_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Hole Name="Hole" id="10" Face="face-a"><Dimension Name="Diameter">4mm</Dimension><Dimension Name="Depth">9mm</Dimension></Hole></Keywords>"#,
    ));
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT hole face reference");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT hole face reference"
    ));
}

fn construction_reference_source() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch" id="1"/><Extrusion Name="Boss" id="2" Profile="1"><Dimension Name="Depth">1mm</Dimension></Extrusion></Keywords>"#,
    ));
    source
}

#[test]
fn metadata_construction_binding_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &construction_reference_source(), options, "index SLDPRT native construction features",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_construction_binding_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &construction_reference_source(), options, "index SLDPRT native construction sources",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_offset_plane_binding_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &construction_reference_source(), options, "index SLDPRT offset plane ordinals",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_offset_plane_binding_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &construction_reference_source(), options, "index SLDPRT offset plane ordinals",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_projection_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &native_definition_source(), options, "collect SLDPRT projected parameters",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_projection_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&native_definition_source(), &mut options, "retain SLDPRT parameter expression");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT parameter expression"
    ));
}

#[test]
fn metadata_parameter_projection_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &native_definition_source(), options, "classify SLDPRT parameter owners",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_ordering_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &native_definition_source(), options, "order SLDPRT parameter dependencies",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_ordering_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &native_definition_source(), options, "order SLDPRT parameter dependencies",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_aliases_refuse_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &native_definition_source(), options, "index SLDPRT parameter aliases",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_aliases_refuse_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&native_definition_source(), &mut options, "retain SLDPRT parameter alias");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT parameter alias"
    ));
}

#[test]
fn metadata_parameter_aliases_refuse_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(
        &native_definition_source(), options, "scan SLDPRT parameter aliases",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_value_states_refuse_collection_limit() {
    let options = DecodeOptions::default();
    let refusal = collection_refusal_with_options(
        &native_definition_source(), options, "collect SLDPRT parameter value states",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_value_states_refuse_work_limit() {
    let options = DecodeOptions::default();
    let refusal = work_refusal_with_options(
        &native_definition_source(), options, "collect SLDPRT parameter value state",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_value_states_refuse_retained_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Custom" Type="Custom" id="10"><Dimension Name="Note">plain text</Dimension></Feature></Keywords>"#,
    ));
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&source, &mut options, "retain SLDPRT parameter value text");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT parameter value text"
    ));
}

#[test]
fn metadata_parameter_dependencies_refuse_collection_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="10"><Dimension Name="A">1</Dimension><Dimension Name="B">A + 1</Dimension></Feature></Keywords>"#,
    ));
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &source, options, "collect SLDPRT parameter dependencies",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert_eq!(refusal.additional, 1);
}

#[test]
fn metadata_parameter_expression_refuses_nesting_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="10"><Dimension Name="A">+ + + + + + + + + + + + 1</Dimension></Feature></Keywords>"#,
    ));
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_recursion_depth = 8;
    let error = SldprtCodec.decode(&mut Cursor::new(&source), &options)
        .expect_err("recursive parameter expression exceeds the nesting limit");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                && limit.operation == "parse SLDPRT parameter unary"
    ));
}

fn class_binding_source() -> Vec<u8> {
    let mut source = crate::test_support::history::sldprt_with_body_and_resolved_features(
        &triangle_body(), &[0, 1],
    );
    source.extend(make_block(0x43, "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="Sketch" id="10"/></Keywords>"#));
    source
}

#[test]
fn metadata_class_binding_refuses_collection_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = collection_refusal_with_options(
        &class_binding_source(), options, "validate SLDPRT history class candidates",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn geometry_class_binding_refuses_collection_limit() {
    let refusal = collection_refusal_at(
        &class_binding_source(), "validate SLDPRT history class candidates",
    );
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn metadata_class_binding_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "bind SLDPRT history classes");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "bind SLDPRT history classes"));
}

#[test]
fn geometry_class_binding_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "bind SLDPRT history classes");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "bind SLDPRT history classes"));
}

#[test]
fn metadata_class_binding_refuses_work_limit() {
    let options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    let refusal = work_refusal_with_options(&class_binding_source(), options, "bind SLDPRT history classes");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn geometry_class_binding_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "bind SLDPRT history classes");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn metadata_scalar_binding_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "collect SLDPRT scalar binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn metadata_scalar_binding_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "scan SLDPRT scalar binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn metadata_scalar_binding_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT scalar binding identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT scalar binding identity"));
}

#[test]
fn geometry_scalar_binding_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "collect SLDPRT scalar binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn geometry_scalar_binding_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "scan SLDPRT scalar binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn geometry_scalar_binding_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT scalar binding identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT scalar binding identity"));
}

#[test]
fn metadata_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "collect SLDPRT adjacent profile objects");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn metadata_adjacent_profiles_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT adjacent profile identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT adjacent profile identity"));
}

#[test]
fn metadata_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "scan SLDPRT adjacent profile objects");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn geometry_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "collect SLDPRT adjacent profile objects");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn geometry_adjacent_profiles_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT adjacent profile identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT adjacent profile identity"));
}

#[test]
fn geometry_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "scan SLDPRT adjacent profile objects");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn metadata_dissected_sketches_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "index SLDPRT dissected profiles");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn metadata_dissected_sketches_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT dissected profile identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT dissected profile identity"));
}

#[test]
fn metadata_dissected_sketches_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "classify SLDPRT dissected profiles");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn geometry_dissected_sketches_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "index SLDPRT dissected profiles");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn geometry_dissected_sketches_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT dissected profile identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT dissected profile identity"));
}

#[test]
fn geometry_dissected_sketches_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "classify SLDPRT dissected profiles");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn metadata_sweep_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "collect SLDPRT feature binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn metadata_sweep_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "scan SLDPRT feature binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn geometry_sweep_adjacent_profiles_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "collect SLDPRT feature binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn geometry_sweep_adjacent_profiles_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "scan SLDPRT feature binding candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn geometry_mirror_surface_planes_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "index SLDPRT mirror surface planes");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn geometry_mirror_surface_planes_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "index SLDPRT mirror surface planes");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

fn sweep_binding_source() -> Vec<u8> {
    let mut source = crate::test_support::container::sldprt_with_body(&triangle_body());
    let mut payload = crate::test_support::history::resolved_feature_classes_with_ids(&[
        ("moSweep_c", "Sweep1", 20),
        ("moProfileFeature_c", "Sketch1", 10),
    ]);
    payload.extend(crate::test_support::parasolid::parasolid_with_body(
        "profile", "SCH_SW_33103_11000", &triangle_body(),
    ));
    source.extend(make_block(0x45, "Contents/Config-0-ResolvedFeatures", &payload));
    source.extend(make_block(0x43, "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sweep1" Type="Sweep" id="20"/><Sketch Name="Sketch1" Type="Sketch" id="10"/></Keywords>"#));
    source
}

#[test]
fn metadata_sweep_adjacent_profiles_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&sweep_binding_source(), &mut options, "retain SLDPRT feature binding identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT feature binding identity"));
}

#[test]
fn geometry_sweep_adjacent_profiles_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&sweep_binding_source(), &mut options, "retain SLDPRT feature binding identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT feature binding identity"));
}

#[test]
fn metadata_pattern_inputs_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "collect SLDPRT pattern input candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn metadata_pattern_inputs_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions { container_only: true, ..DecodeOptions::default() }, "scan SLDPRT pattern input candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn metadata_pattern_inputs_refuses_retained_limit() {
    let mut options = DecodeOptions { container_only: true, ..DecodeOptions::default() };
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT pattern native identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT pattern native identity"));
}

#[test]
fn geometry_pattern_inputs_refuses_collection_limit() {
    let refusal = collection_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "collect SLDPRT pattern input candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn geometry_pattern_inputs_refuses_work_limit() {
    let refusal = work_refusal_with_options(&class_binding_source(), DecodeOptions::default(), "scan SLDPRT pattern input candidates");
    assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn geometry_pattern_inputs_refuses_retained_limit() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;
    let error = retained_refusal_at(&class_binding_source(), &mut options, "retain SLDPRT pattern native identity");
    assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT pattern native identity"));
}
