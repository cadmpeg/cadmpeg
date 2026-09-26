// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::{
    classify_layers, NxDialect, DECLARED_SPLMSSTR_VERSION, DECLARED_UGII_VERSION, FORMAT,
    PARASOLID_FORMAT,
};
use crate::container::Container;
use crate::container::MAGIC;
use crate::loss::NxLossCode;
use crate::test_support::extract_streams;
use crate::test_support::test_prt::single_part_prt;
use cadmpeg_core::dialect::Admission;
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::CodecError;
use std::sync::OnceLock;

#[test]
fn enum_and_registry_rows_are_closed_bidirectionally() -> Result<(), Box<dyn std::error::Error>> {
    cadmpeg_test_support::assert_dialect_rows_closed(&NxDialect::ALL.map(NxDialect::id), FORMAT)?;
    Ok(())
}

/// A container carrying nothing but the dispatch flag and the version byte.
///
/// Classification reads exactly these two fields, so an empty directory is a
/// complete input for it.
fn container(legacy_cfb: bool, version: u8) -> Container<'static> {
    Container {
        data: (&[] as &[u8]).into(),
        physical_size: 0,
        layout: if legacy_cfb {
            crate::container::ContainerLayout::LegacyCfb { version }
        } else {
            crate::container::test_modern_layout(version)
        },
        entries: Vec::new(),
        fastload_table: None,
        indexed_section_layouts: OnceLock::new(),
        om_section_cache: OnceLock::new(),
    }
}

fn classify(container: &Container<'_>) -> DialectMatch {
    NxDialect::of_container(container).matched(container.layout.version())
}

#[test]
fn extracted_parasolid_schema_emits_a_kernel_layer() {
    let bytes = single_part_prt();
    let scan = crate::decode::Scan {
        container: crate::test_support::with_decode_context(|ctx| {
            crate::container::scan_bytes(ctx, bytes.clone())
        })
        .unwrap(),
        streams: extract_streams(&bytes),
    };
    let (layers, losses) = classify_layers(&scan).into_report_parts();
    let kernel = layers
        .iter()
        .find(|matched| matched.format() == PARASOLID_FORMAT)
        .expect("the extracted Parasolid stream emits a layer");

    assert_eq!(kernel.dialect().as_str(), "parasolid:unknown");
    assert_eq!(kernel.declared()["schema"], "SCH_TEST_1_9999");
    assert_eq!(kernel.instance(), None);
    assert_eq!(kernel.admission(), &Admission::Residual);

    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, NxLossCode::KernelDialectUnverified.kind());
    assert_eq!(
        losses[0].strict_consequence(),
        cadmpeg_ir::report::loss::StrictConsequence::Reject
    );
    assert!(losses[0].message.contains("SCH_TEST_1_9999"));
}

#[test]
fn a_named_sldprt_parasolid_schema_remains_unverified_under_nx() {
    let bytes = single_part_prt();
    let mut streams = extract_streams(&bytes);
    let crate::parasolid::StreamBody::Parasolid { schema, .. } = &mut streams[0].body else {
        panic!("Parasolid fixture");
    };
    *schema = Some(
        cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_3501171_35102_13006")
            .expect("the fixture text is a schema token"),
    );
    let scan = crate::decode::Scan {
        container: crate::test_support::with_decode_context(|ctx| {
            crate::container::scan_bytes(ctx, bytes)
        })
        .unwrap(),
        streams,
    };
    let (layers, losses) = classify_layers(&scan).into_report_parts();
    let kernel = layers
        .iter()
        .find(|matched| matched.format() == PARASOLID_FORMAT)
        .expect("the extracted Parasolid stream emits a layer");

    assert_eq!(kernel.dialect().as_str(), "parasolid:format-13006");
    assert_eq!(kernel.admission(), &Admission::Residual);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, NxLossCode::KernelDialectUnverified.kind());
    assert!(losses[0].message.contains("host did not verify"));
}

#[test]
fn duplicate_kernel_identity_is_omitted_with_a_typed_loss() {
    let bytes = single_part_prt();
    let mut streams = extract_streams(&bytes);
    streams.push(streams[0].clone());
    let scan = crate::decode::Scan {
        container: crate::test_support::with_decode_context(|ctx| {
            crate::container::scan_bytes(ctx, bytes)
        })
        .unwrap(),
        streams,
    };

    let summary = crate::summarize(&scan);
    let (layers, losses) = classify_layers(&scan).into_report_parts();
    assert_eq!(
        layers
            .iter()
            .filter(|matched| matched.format() == PARASOLID_FORMAT)
            .count(),
        1
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == NxLossCode::DialectLayerCollision.kind()));
    assert!(summary.losses.iter().any(|loss| {
        loss.code == NxLossCode::DialectLayerCollision.kind()
            && loss.message.contains("duplicate parasolid dialect layer")
    }));
}

#[test]
fn each_container_parser_classifies_into_its_own_row() {
    assert_eq!(
        NxDialect::of_container(&container(false, 0x06)),
        NxDialect::Splmsstr
    );
    assert_eq!(
        NxDialect::of_container(&container(true, 0x0a)),
        NxDialect::LegacyCfb
    );
}

#[test]
fn the_container_kind_label_and_the_registry_id_come_from_one_enum() {
    assert_eq!(
        NxDialect::Splmsstr.container_kind(),
        cadmpeg_ir::ContainerKind::Splmsstr
    );
    assert_eq!(NxDialect::Splmsstr.id().as_str(), "nx:splmsstr");
    assert_eq!(
        NxDialect::LegacyCfb.container_kind(),
        cadmpeg_ir::ContainerKind::Cfb
    );
    assert_eq!(NxDialect::LegacyCfb.id().as_str(), "nx:legacy-cfb");
}

#[test]
fn both_container_rows_are_admitted_because_classification_is_structural() {
    // The dispatch at `decode::scan` picks the parser that then defines the
    // row, so a document is never read with a grammar its row does not
    // declare. No arm of this codec substitutes one row's strategy for
    // another's, so no unverified admission and no dialect-unverified loss
    // exist. Anything else here would be an invented path.
    for legacy_cfb in [false, true] {
        let container = container(legacy_cfb, 0x06);
        let matched = classify(&container);
        assert_eq!(matched.admission(), &Admission::Admitted);
    }
}

#[test]
fn the_modern_arm_declares_the_container_version_byte_as_canonical_decimal() {
    let container = container(false, 0x06);
    let matched = classify(&container);

    assert_eq!(matched.dialect().as_str(), "nx:splmsstr");
    assert_eq!(matched.format(), "nx");
    assert_eq!(matched.declared()[DECLARED_SPLMSSTR_VERSION], "6");
    assert!(!matched.declared().contains_key(DECLARED_UGII_VERSION));
    assert_eq!(matched.declared().len(), 1);
}

#[test]
fn the_legacy_arm_declares_the_ugii_payload_version_as_canonical_decimal() {
    let container = container(true, 0x0a);
    let matched = classify(&container);

    assert_eq!(matched.dialect().as_str(), "nx:legacy-cfb");
    assert_eq!(matched.declared()[DECLARED_UGII_VERSION], "10");
    assert!(!matched.declared().contains_key(DECLARED_SPLMSSTR_VERSION));
    assert_eq!(matched.declared().len(), 1);
}

#[test]
fn splmsstr_version_byte_is_checked_before_container_construction() {
    let valid = single_part_prt();
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, valid.clone())
    })
    .expect("version 0x06 scans");
    let matched = classify(&container);
    assert_eq!(matched.dialect().as_str(), "nx:splmsstr");
    assert_eq!(matched.declared()[DECLARED_SPLMSSTR_VERSION], "6");
    assert_eq!(matched.admission(), &Admission::Admitted);

    for version in [0_u8, 1, 255] {
        let mut invalid = valid.clone();
        invalid[crate::layout::splmsstr_header::VERSION_TAG] = version;
        assert!(matches!(
            crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, invalid)),
            Err(CodecError::Malformed(message)) if message.contains("version byte must be 0x06")
        ));
    }
}

#[test]
fn a_header_too_short_to_declare_a_version_never_scans() {
    // The HEADER marker sits above the version byte, so every image that scans
    // carries a declaration. This pins the bound rather than the argument -- move the
    // marker below offset 8 and this test, not a golden, is what fails.
    const {
        assert!(
            crate::layout::splmsstr_header::HEADER_MARKER
                > crate::layout::splmsstr_header::VERSION_TAG
        );
    }
    let file = single_part_prt();
    for truncated_len in MAGIC.len()..=crate::layout::splmsstr_header::HEADER_MARKER {
        assert!(
            crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(
                ctx,
                &file[..truncated_len]
            ))
            .is_err(),
            "an image of {truncated_len} bytes must not scan"
        );
    }
    crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
        .expect("the whole image scans");
}
