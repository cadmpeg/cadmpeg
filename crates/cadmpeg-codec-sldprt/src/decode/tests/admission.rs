// SPDX-License-Identifier: Apache-2.0
//! Container admission, strict-mode, and resource-limit decode tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
use cadmpeg_ir::LossTaxonomy;

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

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
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::Entities
                    && limit.context.operation == "admit SLDPRT container entities"
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
            cadmpeg_core::CodecError::ResourceLimit(limit)
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
    let container_entities = scan.blocks.len() + scan.compound_streams.len() + scan.directory.len();
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
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::Entities
                    && limit.context.operation == "admit SLDPRT entities"
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
    use cadmpeg_ir::report::{LossTaxonomy, StrictConsequence};

    let fixture = synthetic_sldprt();

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage decode keeps the partial result");
    assert!(!salvaged.report().geometry_transferred);
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

    let strict = SldprtCodec.decode(&mut Cursor::new(fixture), &strict_options());
    match strict {
        Err(cadmpeg_core::CodecError::Malformed(message)) => {
            assert!(
                message.contains("strict mode rejects sldprt/"),
                "unexpected message: {message}"
            );
        }
        other => panic!("strict decode must reject unrepresentable geometry, got {other:?}"),
    }
}

#[test]
fn strict_accepts_tolerable_gauge_substitution_geometry() {
    use cadmpeg_ir::report::{LossTaxonomy, StrictConsequence};

    let fixture = sldprt_with_body_and_history(&triangle_body());
    let strict = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect("strict decode accepts a tolerable-loss geometry result");
    assert!(strict.report().geometry_transferred);
    assert!(strict
        .report()
        .losses
        .iter()
        .all(|note| note.strict_consequence() == StrictConsequence::Tolerate));
}

/// Phase 5 freeze: export precondition (:50) rejects shared broken IR; empty accepts.
#[test]
fn phase5_freeze_export_precondition_admissibility_fixtures() {
    let accepted = cadmpeg_ir::validate::admissibility_freeze::accepted_empty();
    // Empty IR has no B-rep; writer refuses later for missing B-rep, but the
    // :50 precondition is full validate — empty passes validate.
    assert!(cadmpeg_ir::validate_neutral(&accepted, Vec::new()).is_ok());
    let rejected =
        cadmpeg_ir::validate::admissibility_freeze::rejected_missing_point("sldprt:test");
    assert!(!cadmpeg_ir::validate_neutral(&rejected, Vec::new()).is_ok());
}

#[test]
fn configuration_source_index_allocation_rejects_exhaustion() {
    let mut used = std::collections::HashSet::from([u32::MAX]);
    let mut next = u32::MAX;
    let error = crate::writer::reserve_configuration_index(&mut used, &mut next).unwrap_err();
    assert!(error.to_string().contains("index space is exhausted"));
}
