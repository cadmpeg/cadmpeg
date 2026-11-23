// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use crate::{test_support, SolidEdgeCodec};

#[test]
fn directory_signature_detects_solid_edge() {
    let bytes = test_support::compound();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::High);
    ctx.finish_session().unwrap();
}

fn detect(bytes: &[u8]) -> Confidence {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let confidence = SolidEdgeCodec.detect(&ctx, root).unwrap();
    ctx.finish_session().unwrap();
    confidence
}

#[test]
fn family_directory_signature_detects_solid_edge() {
    let bytes = test_support::compound_with("FamilyInfo", "FOAPublishData", test_support::FAMILY);
    assert_eq!(detect(&bytes), Confidence::High);
}

#[test]
fn every_document_class_with_its_entries_is_detected() {
    for class in [
        test_support::PART,
        test_support::SHEET_METAL,
        test_support::ASSEMBLY,
    ] {
        let bytes = test_support::compound_with("PSMroots", "JSitesList", class);
        assert_eq!(detect(&bytes), Confidence::High, "{class:?}");
    }
}

#[test]
fn unlisted_root_class_is_rejected_with_solid_edge_entries() {
    for (first, second) in [("PSMroots", "JSitesList"), ("FamilyInfo", "FOAPublishData")] {
        let bytes = test_support::compound_with(first, second, test_support::OTHER);
        assert_eq!(detect(&bytes), Confidence::No, "{first}");
        assert!(SolidEdgeCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .is_err());
        assert!(SolidEdgeCodec
            .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
            .is_err());
    }
}

#[test]
fn root_class_of_the_other_form_is_rejected() {
    let document = test_support::compound_with("PSMroots", "JSitesList", test_support::FAMILY);
    let family = test_support::compound_with("FamilyInfo", "FOAPublishData", test_support::PART);
    assert_eq!(detect(&document), Confidence::No);
    assert_eq!(detect(&family), Confidence::No);
}

#[test]
fn entry_names_compare_without_regard_to_case_through_decode_and_inspect() {
    let bytes = test_support::compound_with("psmROOTS", "jsitesLIST", test_support::ASSEMBLY);
    assert_eq!(detect(&bytes), Confidence::High);
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
}

#[test]
fn family_documents_decode_and_inspect() {
    let bytes = test_support::compound_with("FAMILYinfo", "foaPUBLISHdata", test_support::FAMILY);
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
}

#[test]
fn incomplete_directory_signatures_are_rejected() {
    for pair in [
        ("JSitesList", "Payload"),
        ("PSMroots", "Payload"),
        ("JGlobal", "JSitesList"),
        ("PSMroots", "FamilyInfo"),
    ] {
        let bytes = test_support::compound_with(pair.0, pair.1, test_support::PART);
        assert_eq!(detect(&bytes), Confidence::No, "{pair:?}");
    }
}

#[test]
fn generic_compound_file_has_no_solid_edge_identity() {
    let bytes = cadmpeg_test_support::compound::compound_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::No);
    ctx.finish_session().unwrap();
    assert!(SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .is_err());
}

#[test]
fn inspection_lists_compound_streams() {
    let bytes = test_support::compound();
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
}

#[test]
fn full_decode_reports_pending_bodies() {
    let bytes = test_support::compound();
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
    assert!(!result.report().losses.is_empty());
}
