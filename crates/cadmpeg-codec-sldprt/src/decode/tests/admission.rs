// SPDX-License-Identifier: Apache-2.0
//! Container admission, strict-mode, and resource-limit decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
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
    let error = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &options)
        .expect_err("display section copy must be admitted");
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
    let error = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &options)
        .expect_err("source-image retention must obey the session limit");
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
    let stream_entities = crate::decode::active_body_streams(&scan).len();
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
    assert!(cadmpeg_ir::validate_neutral(&accepted, Vec::new()).is_ok());
    let rejected = cadmpeg_test_support::admissibility::rejected_missing_point("sldprt:test")
        .expect("fixture identities are valid");
    assert!(!cadmpeg_ir::validate_neutral(&rejected, Vec::new()).is_ok());
}

#[test]
fn configuration_source_index_allocation_rejects_exhaustion() {
    let mut used = std::collections::HashSet::from([u32::MAX]);
    let mut next = u32::MAX;
    let error = crate::writer::reserve_configuration_index(&mut used, &mut next).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}
