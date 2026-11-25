// SPDX-License-Identifier: Apache-2.0
//! Container admission, strict-mode, and resource-limit decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::ArenaClass;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

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
    cadmpeg_core::CodecError::ResourceLimit(cadmpeg_test_support::decode::resource_refusal_at(
        &SldprtCodec,
        source,
        options,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        operation,
        0,
    ))
    .into()
}

#[test]
fn native_loss_validation_admits_history_fixture() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let options = DecodeOptions {
        policy: cadmpeg_core::decode::DecodePolicy::service(),
        ..DecodeOptions::default()
    };
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("decode the fixture");
}

#[test]
fn direct_parasolid_stream_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = triangle_body();
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &body);
    let source = sldprt_with_body(&body);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(stream.len()) - 1;
    let error = retained_refusal_at(&source, &mut options, "retain direct Parasolid stream");
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
        .expect("decode the fixture");
}

#[test]
fn active_site_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = triangle_body();
    let source = sldprt_with_body(&body);
    let mut options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "retain SLDPRT active site",
        |limit| {
            let mut limited = options.clone();
            limited.policy.limits.max_retained_bytes = limit;
            SldprtCodec
                .decode(&mut Cursor::new(&source), &limited)
                .map_err(|error| match error {
                    cadmpeg_ir::DecodeFailure::Codec(error) => error,
                    other => panic!("unexpected decode failure: {other:?}"),
                })
        },
    );
    assert!(
        matches!(error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT active site"),
        "{error:?}"
    );

    options.policy = cadmpeg_core::decode::DecodePolicy::service();
    SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("decode the fixture");
}

#[test]
fn face_surface_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut body = triangle_body();
    body.extend(crate::test_support::parasolid::nurbs_surface_carrier(
        180, 181, 10,
    ));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let source = sldprt_with_body(&body);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "retain Parasolid surface geometry copy",
        |limit| {
            let mut limited = DecodeOptions::default();
            limited.policy.limits.max_retained_bytes = limit;
            SldprtCodec
                .decode(&mut Cursor::new(&source), &limited)
                .map_err(|error| match error {
                    cadmpeg_ir::DecodeFailure::Codec(error) => error,
                    other => panic!("unexpected decode failure: {other:?}"),
                })
        },
    );
    assert!(
        matches!(error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Parasolid surface geometry copy"
                && limit.additional > 0),
        "{error:?}"
    );
}

#[test]
fn display_section_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = triangle_body();
    let stream = parasolid_with_body("partition body", "SCH_SW_33103_11000", &body);
    let display = display_list_payload();
    let source = sldprt_with_body_and_display_list(&body);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(stream.len() + display.len()) - 1;
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
        .expect("decode the fixture");
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
        .expect("decode the fixture");
}

#[test]
fn whole_source_copy_refuses_retained_limit_before_unknown_record() {
    use cadmpeg_core::decode::ResourceDimension;

    let source = outer_header();
    let mut options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(source.len()) - 1;
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
        .expect("decode the fixture");
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
    let scan = crate::test_support::container::scan(&fixture);
    let container_entities = scan.blocks.len()
        + scan.compound_streams.len()
        + scan.directory.len()
        + scan.cache_cells.len();
    let stream_entities = crate::decode::active_body_streams(&scan).len();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("decode triangle body");
    let ir = decoded.ir();
    // Each design scope's root definition is a model record admitted with
    // the design records.
    let design_entities = ir.design.entity_count() + ir.design.scopes.len();
    let model_entities = ir.entity_count() - design_entities;
    assert!(container_entities > 0);
    assert!(stream_entities > 0);
    assert!(model_entities > 0);
    assert!(design_entities > 0);

    let refusal = |max_entities: usize| {
        let mut options = DecodeOptions::default();
        options.policy.limits.max_entities = cadmpeg_core::decode::u64_from_index(max_entities);
        SldprtCodec
            .decode(&mut Cursor::new(fixture.clone()), &options)
            .expect_err("container, stream, model, and design cardinalities must remain additive")
    };
    let refused_at = |error: &cadmpeg_ir::DecodeFailure, operation: &str| {
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == operation
        )
    };
    let previous_undercount = (container_entities + stream_entities).max(model_entities);
    let error = refusal(previous_undercount);
    assert!(refused_at(&error, "admit SLDPRT entities"), "{error:?}");
    let error =
        refusal(container_entities + stream_entities + model_entities + design_entities - 1);
    assert!(
        refused_at(&error, "admit SLDPRT design records"),
        "{error:?}"
    );

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = cadmpeg_core::decode::u64_from_index(
        container_entities + stream_entities + model_entities + design_entities,
    );
    SldprtCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect("the exact additive entity limit must admit the fixture");
}

#[test]
fn strict_accepts_operator_requested_container_only() {
    let fixture = synthetic_sldprt();
    let mut options = strict_options();
    options.depth = cadmpeg_core::decode::DecodeDepth::Container;
    SldprtCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect("strict container-only decode is accepted");
}

/// A container decode of a part whose metadata and history a full decode
/// states as model records states none of them, so its container scope holds.
#[test]
fn a_container_decode_states_no_model_record() {
    for fixture in [
        crate::test_support::container::sldprt_with_body_and_envelope(&triangle_body()),
        sldprt_with_body_and_history(&triangle_body()),
    ] {
        let full = SldprtCodec
            .decode(&mut Cursor::new(&fixture), &DecodeOptions::default())
            .expect("full decode");
        assert!(full.ir().model.features.len() + full.ir().model.attributes.len() > 0);
        let container = SldprtCodec
            .decode(
                &mut Cursor::new(&fixture),
                &DecodeOptions {
                    depth: cadmpeg_core::decode::DecodeDepth::Container,
                    ..DecodeOptions::default()
                },
            )
            .expect("container decode");
        for class in ArenaClass::ALL {
            assert_eq!(container.ir().populated_arena(class), None, "{class}");
        }
        assert!(container.ir().source.is_some());
    }
}

#[test]
fn strict_reads_unrepresentable_geometry_and_reports_its_loss_codes() {
    use crate::loss::SldprtLossCode;
    use cadmpeg_ir::report::loss::LossTaxonomy;

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

    let strict = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect("the geometry codes pin no strict floor");
    assert!(strict
        .report()
        .losses
        .iter()
        .any(|note| note.code == SldprtLossCode::GeometryParasolidNotTransferred.kind()));
}

#[test]
fn strict_accepts_tolerable_gauge_substitution_geometry() {
    let fixture = sldprt_with_body_and_history(&triangle_body());
    let strict = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect("strict decode accepts a tolerable-loss geometry result");
    assert!(strict.report().geometry_transferred());
    assert!(strict
        .report()
        .losses
        .iter()
        .all(|note| !strict_options().strictness.refuses(note, false)));
}

#[test]
fn strict_reads_and_reports_a_residual_parasolid_schema() {
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

    let strict = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect("the kernel-dialect code pins no strict floor");
    assert!(strict
        .report()
        .losses
        .iter()
        .any(|note| note.code == SldprtLossCode::KernelDialectUnverified.kind()));
}

#[test]
fn configuration_source_index_allocation_rejects_exhaustion() {
    let mut used = std::collections::HashSet::from([u32::MAX]);
    let mut next = u32::MAX;
    let error = crate::writer::reserve_configuration_index(&mut used, &mut next).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

mod projections;

#[test]
fn decoded_curve_carrier_is_copied() {
    let source = sldprt_with_body(&crate::test_support::parasolid::nurbs_sketch_body(true));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.ir().geometry.curves.is_empty());
}

mod surface_solver;

#[test]
fn an_embedded_document_body_is_not_an_active_body_stream() {
    let body = crate::test_support::parasolid::triangle_body();
    let mut fixture = sldprt_with_body(&body);
    fixture.extend(make_block(
        0x20,
        "ImportedComp/Pin.SLDPRT",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &body),
    ));
    let scan = crate::test_support::container::scan(&fixture);
    let streams = crate::decode::active_body_streams(&scan);
    assert_eq!(streams.len(), 1);
    assert_eq!(
        streams[0].source_stream().as_str(),
        "Contents/Config-0-Partition"
    );
}

fn untyped_marks(
    source: Vec<u8>,
    depth: cadmpeg_core::decode::DecodeDepth,
) -> Vec<(String, Vec<ArenaClass>)> {
    let options = DecodeOptions {
        depth,
        ..DecodeOptions::default()
    };
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .expect("decode");
    let decoded = cadmpeg_test_support::EditableDecodeResult::from(result);
    decoded
        .source_fidelity()
        .retained_records()
        .iter()
        .filter(|(_, record)| !record.untyped().is_empty())
        .map(|(id, record)| {
            (
                id.as_str().to_owned(),
                record.untyped().iter().copied().collect(),
            )
        })
        .collect()
}

#[test]
fn a_body_stream_kept_without_decoding_is_untyped_model_content() {
    assert_eq!(
        untyped_marks(
            sldprt_with_body(&triangle_body()),
            cadmpeg_core::decode::DecodeDepth::Container
        ),
        [("sldprt:file:block#8".to_owned(), vec![ArenaClass::Model])]
    );
}

#[test]
fn only_a_record_of_unknown_content_family_is_untyped_after_a_full_decode() {
    let source = sldprt_with_body(&triangle_body());
    assert!(untyped_marks(source.clone(), cadmpeg_core::decode::DecodeDepth::default()).is_empty());
    let mut with_opaque = source;
    with_opaque.extend(make_block(
        0x41,
        "Contents/Opaque",
        b"\x01\x02 not a known payload",
    ));
    let marked = untyped_marks(with_opaque, cadmpeg_core::decode::DecodeDepth::default());
    assert_eq!(marked.len(), 1, "{marked:?}");
    assert_eq!(marked[0].1, [ArenaClass::Model, ArenaClass::Design]);
}
