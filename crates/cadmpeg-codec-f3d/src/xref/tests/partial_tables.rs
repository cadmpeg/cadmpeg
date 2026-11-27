// SPDX-License-Identifier: Apache-2.0
//! An unreadable entry beside an external reference table gives up that
//! entry alone, and the table decodes.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::assembly_test::redirections_json;
use crate::test_support::native_test::f3d_native;
use crate::F3dCodec;

/// A malformed `ComponentReferenceData.json` gives up that entry alone, and
/// the Redirections table decodes.
#[test]
fn malformed_component_reference_data_gives_up_its_own_entry() {
    let part = [("part.f3d", "aaaabbbb-cccc-dddd-eeee-ffff00001111")];
    let redirections = redirections_json("root.f3d", &part);
    let document = crate::test_support::assembly_test::f3d_with_reference_entries(
        "assembly-design",
        b"[1, 2",
        redirections.as_bytes(),
    );
    let head = "part archive-entry \"ComponentReferenceData.json\" stream=\"\" offset=";
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let mut source = Cursor::new(&document);
        let decoded = F3dCodec.decode(&mut source, &options).unwrap();
        let losses = &decoded.report().losses;
        crate::test_support::charged_part(decoded.ir(), losses, head, "");
        let unreadable = cadmpeg_ir::unknown::part::record_unreadable_kind();
        assert!(
            !losses
                .iter()
                .any(|loss| loss.code == unreadable
                    && loss.message.contains("RedirectionsStream.dat"))
        );
        if depth == DecodeDepth::Full {
            let references = &f3d_native(decoded.ir()).xref_references;
            assert_eq!(references.len(), 1);
            assert_eq!(references[0].relative_path.as_str(), "part.f3d");
        }
    }
    crate::test_support::assert_pinned_refusal(document, head);
}

/// A Design stream whose metastream does not read binds no occurrence: the
/// table keeps its reference unbound, and the metastream entry alone is
/// unread.
#[test]
fn an_unreadable_design_metastream_keeps_the_table_and_names_its_own_entry() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let placement = super::occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    let document = super::document_with_streams(role, b"no metastream", &placement);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&document, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut unread = Vec::new();
    let table = super::super::decode(&ctx, &scan, &mut unread)
        .expect("the table decodes")
        .expect("the document stores a table");
    assert_eq!(table.references.len(), 1);
    assert!(table.references[0].placement_source.is_none());
    let sources = unread
        .iter()
        .map(|failure| failure.source.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        [super::super::XrefSource::Entry(
            "FusionAssetName[Active]/Design1/MetaStream.dat".to_owned()
        )]
    );
}

/// The occurrence binding and the Design decode admit an occurrence header by
/// its class tag and u32 record index. A nonzero byte after that index gives
/// up no entry.
#[test]
fn an_occurrence_header_admitted_by_the_design_decode_gives_up_no_entry() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let mut placement = super::occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    placement[4..7].copy_from_slice(b"256");
    placement[11] = 1;
    let meta = super::design_metastream_with_records(
        &[(
            super::super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    );
    let document = super::document_with_streams(role, &meta, &placement);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&document, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut unread = Vec::new();
    let table = super::super::decode(&ctx, &scan, &mut unread)
        .expect("the table decodes")
        .expect("the document stores a table");
    assert_eq!(table.references.len(), 1);
    assert!(unread.is_empty());
}
