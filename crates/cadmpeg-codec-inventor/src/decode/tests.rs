// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};

use super::*;
use crate::loss::InventorLossCode;
use crate::test_support::{fixture, primary_envelope_fixture};
use crate::InventorCodec;

#[test]
fn built_in_properties_are_selected_by_embedded_set_identity() {
    assert_eq!(
        built_in_property_name("Design Tracking Properties", 5),
        Some("Part Number")
    );
    assert_eq!(
        built_in_property_name("Inventor Summary Information", 17),
        Some("Thumbnail")
    );
    assert!(known_property_set_fmtid("Design Tracking Properties").is_some());
    assert!(built_in_property_name("Unknown Set", 5).is_none());
}

#[test]
fn metadata_projection_maps_stable_fields_without_overwriting_conflicts() {
    let mut projection = MetadataProjection::default();
    projection.consider(&[0; 16], 5, Some("Part Number"), Some("P-1"), "first");
    projection.consider(&[0; 16], 5, Some("Part Number"), Some("P-2"), "second");
    projection.consider(&[0; 16], 29, Some("Description"), Some("Bracket"), "desc");
    assert_eq!(projection.part_number.as_deref(), Some("P-1"));
    assert_eq!(projection.description.as_deref(), Some("Bracket"));
    assert_eq!(
        projection.bom_properties.get("second").map(String::as_str),
        Some("P-2")
    );
}

#[test]
fn inventor_clipboard_preview_requires_matching_png_dimensions() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&5_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    bytes.extend_from_slice(&4_u32.to_be_bytes());
    bytes.extend_from_slice(&5_u32.to_be_bytes());
    let arena = DecodeArena::new();
    let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("synthetic preview fits policy");
    let value = PropertyValue::Clipboard {
        format: u32::MAX,
        data: root,
    };
    assert_eq!(
        preview_bytes(&value).map(|(_, media)| media),
        Some("image/png")
    );

    bytes[8..10].copy_from_slice(&6_u16.to_le_bytes());
    let arena = DecodeArena::new();
    let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("synthetic preview fits policy");
    let value = PropertyValue::Clipboard {
        format: u32::MAX,
        data: root,
    };
    assert!(preview_bytes(&value).is_none());
}

#[test]
fn decode_distinguishes_container_only_from_untransferred_geometry() {
    let source = fixture(true);
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(&source),
            &DecodeOptions::default(),
        )
        .expect("synthetic Inventor container decodes structurally");
    assert_eq!(decoded.report().format, "inventor");
    assert!(!decoded.report().container_only);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == InventorLossCode::GeometryKernelCarrierNotTransferred.kind()));
    let native_findings = crate::validate_native(decoded.ir());
    assert_eq!(native_findings.len(), 1, "{native_findings:#?}");
    assert!(native_findings[0]
        .message
        .contains("do not select one registry grammar"));
    assert_eq!(
        decoded
            .ir()
            .native
            .namespace("inventor")
            .expect("Inventor native namespace exists")
            .version,
        crate::native::INVENTOR_NATIVE_VERSION
    );

    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let container_only = InventorCodec
        .decode(&mut std::io::Cursor::new(source), &options)
        .expect("container-only Inventor decode succeeds");
    assert_eq!(
        container_only
            .report()
            .losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        // The structural fixture has no `RSeDb` stream and no segment, so it
        // declares neither version this codec gates on and is admitted
        // unverified. That charge is independent of the transfer, so it stands
        // beside the container-only note.
        [
            InventorLossCode::SourceDialectUnverified.kind(),
            InventorLossCode::ContainerOnlyDecode.kind()
        ]
    );
    let namespace = container_only
        .ir()
        .native
        .namespace("inventor")
        .expect("Inventor native namespace exists");
    let bulk = namespace
        .arena_as::<crate::native::SegmentBulkRecord>("segment_bulk")
        .expect("container-only bulk records retain their outer envelopes");
    assert!(bulk.iter().all(|record| {
        record.record_state == "not_expanded"
            && record.expanded_len.is_none()
            && record.expanded_sha256.is_none()
    }));
    assert!(container_only.source_fidelity().retained_records.is_empty());
}

#[test]
fn decodes_the_synthetic_primary_rse_envelope_end_to_end() {
    let source = primary_envelope_fixture();
    assert_eq!(InventorCodec.detect(&source), Confidence::High);
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
        .expect("synthetic primary Inventor envelope decodes");
    assert_eq!(decoded.report().format, "inventor");
    assert_eq!(decoded.report().coverage["rse_storage_bands"], 1);
    assert_eq!(decoded.report().coverage["rse_databases"], 1);
    assert_eq!(decoded.report().coverage["rse_registry_entries"], 1);
    assert_eq!(decoded.report().coverage["rse_segment_pairs"], 1);
    assert_eq!(decoded.report().coverage["rse_segment_meta"], 1);
    assert_eq!(decoded.report().coverage["rse_records"], 1);
    assert_eq!(decoded.report().coverage["active_kernel_carriers"], 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == InventorLossCode::GeometryKernelCarrierNotTransferred.kind()));

    let native = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("Inventor native namespace exists");
    let active = native
        .arena_as::<crate::native::ActiveCarrierRecord>("active_carrier")
        .expect("active carrier arena exists");
    assert_eq!(active.len(), 1);
    assert_eq!(
        active[0].state,
        crate::native::ActiveCarrierRecordState::Selected
    );
    assert!(crate::validate_native(decoded.ir()).is_empty());
}
