// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::{wire, EditableDecodeResult};

use cadmpeg_asm::dialect::DECLARED_SAVE_FORMAT_MAJOR;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

mod native_admission;
mod presentation_admission;
mod property_admission;

use super::{
    built_in_property_name, known_property_name, known_property_set_fmtid, preview_bytes,
    retained_hex, KnownPropertyName, MetadataProjection, PropertyName,
};
use crate::loss::InventorLossCode;
use crate::native::{DatabaseIssueRecord, DatabaseRecord, VersionTupleRecord};
use crate::property_set::PropertyValue;
use crate::test_support::test_fixtures::{
    acis_kernel_stream, acis_sphere_kernel_stream, fixture, primary_envelope_fixture,
    primary_envelope_fixture_with_kernel, EnvelopeDeclarations,
};
use crate::InventorCodec;

fn validation_findings(ir: &cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("validation context");
    crate::validate::validate_native(&ctx, ir).expect("validation fits service policy")
}

#[test]
fn decode_scopes_parsed_container_storage() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("container setup context");
    let container =
        crate::container::InventorContainer::open(&setup, root).expect("parsed container");
    let (output, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("output context");
    let expected = super::decode_container(&output, &container).expect("output projection");
    let cadmpeg_core::CodecError::ResourceLimit(output_limit) = output
        .charge_retained(u64::MAX, "measure decoded output storage")
        .expect_err("the measurement exceeds the retained allowance")
    else {
        panic!("retained output measurement must be a resource refusal");
    };
    assert_eq!(output_limit.dimension, ResourceDimension::RetainedBytes);
    assert!(output_limit.used > 0);

    let mut policy = DecodePolicy::service();
    // Let R be the output-only retained cost. The route must fit R, rather
    // than R plus parsed-container storage C. Temporary C is released on return.
    policy.limits.max_retained_bytes = output_limit.used;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("exact output context");
    let actual = super::decode(&ctx, root).expect("only output storage is retained");
    assert_eq!(actual, expected);
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse parsed container storage",
        )
        .expect("all temporary storage is released"),
    );
    assert!(
        matches!(ctx.charge_retained(1, "probe decoded output storage"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == output_limit.used
                && limit.additional == 1)
    );
}

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
fn property_names_classify_by_lowercased_alphanumeric_characters() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    for (name, expected) in [
        ("Part Number", Some(KnownPropertyName::PartNumber)),
        ("Preview-Image", Some(KnownPropertyName::PreviewImage)),
        ("DOCUMENT_TYPE", Some(KnownPropertyName::DocumentType)),
        ("Part Number 42", None),
        ("Étage #1", None),
        ("", None),
    ] {
        assert_eq!(
            known_property_name(&ctx, name).expect("classified name"),
            expected,
            "{name}"
        );
    }

    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        known_property_name(&ctx, "Name"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "normalize Inventor property name"
    ));
}

#[test]
fn property_name_classification_charges_only_the_characters_it_reads() {
    // "previewimage" fills the twelve-byte buffer; the thirteenth character
    // cannot fit, so classification stops after thirteen charged steps and
    // never reads the remaining characters.
    let name = format!("previewimage{}", "x".repeat(10_000));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 13;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert_eq!(
        known_property_name(&ctx, &name).expect("bounded walk"),
        None
    );

    policy.limits.max_work_units = 12;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        known_property_name(&ctx, &name),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "normalize Inventor property name"
    ));
}

#[test]
fn fixed_width_hexadecimal_conversion_admits_storage_without_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert_eq!(
        retained_hex(&ctx, &[0xaf], "retain Inventor test hexadecimal output")
            .expect("fixed hexadecimal conversion needs no input work"),
        "af"
    );
    assert_eq!(ctx.resource_refusal(), None);
    assert!(ctx.finish_session().is_ok());

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let cadmpeg_core::CodecError::ResourceLimit(refusal) =
        retained_hex(&ctx, &[0xaf], "retain Inventor test hexadecimal output")
            .expect_err("one retained byte is below the two-byte output")
    else {
        panic!("hex output storage must refuse before writing");
    };
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.operation, "retain Inventor test hexadecimal output");
    assert_eq!(refusal.limit, 1);
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 2);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == refusal
    ));

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("exact context");
    assert_eq!(
        retained_hex(&ctx, &[0xaf], "retain Inventor test hexadecimal output")
            .expect("exact retained cap admits output without materialized storage"),
        "af"
    );
    assert_eq!(ctx.resource_refusal(), None);
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn metadata_projection_maps_stable_fields_without_overwriting_conflicts() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let mut projection = MetadataProjection::default();
    projection
        .consider(
            &ctx,
            &[0; 16],
            5,
            Some(PropertyName::classify(&ctx, "Part Number").expect("name")),
            Some("P-1"),
            "first",
        )
        .expect("first property");
    projection
        .consider(
            &ctx,
            &[0; 16],
            5,
            Some(PropertyName::classify(&ctx, "Part Number").expect("name")),
            Some("P-2"),
            "second",
        )
        .expect("second property");
    projection
        .consider(
            &ctx,
            &[0; 16],
            29,
            Some(PropertyName::classify(&ctx, "Description").expect("name")),
            Some("Bracket"),
            "desc",
        )
        .expect("description property");
    assert_eq!(projection.part_number.as_deref(), Some("P-1"));
    assert_eq!(projection.description.as_deref(), Some("Bracket"));
    assert_eq!(
        projection.bom_properties.get("second").map(String::as_str),
        Some("P-2")
    );
}

#[test]
fn metadata_projection_refuses_retained_limit_before_value_copy() {
    let arena = DecodeArena::new();
    // The name is classified without storage; the 3-byte P-1 value is retained.
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("P-1".len() - 1).expect("metadata value length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut projection = MetadataProjection::default();
    assert!(matches!(
        projection.consider(&ctx, &[0; 16], 5, Some(PropertyName::classify(&ctx, "Part Number").expect("name")), Some("P-1"), "first"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor metadata value"
                && limit.used == 0
    ));
    assert!(projection.part_number.is_none());
}

#[test]
fn metadata_bom_property_refuses_collection_limit_before_insert() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut projection = MetadataProjection::default();
    assert!(matches!(
        projection.consider(&ctx, &[0; 16], 99, Some(PropertyName::classify(&ctx, "Custom").expect("name")), Some("value"), "custom"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor BOM property"
    ));
    assert!(projection.bom_properties.is_empty());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    projection
        .consider(
            &ctx,
            &[0; 16],
            99,
            Some(PropertyName::classify(&ctx, "Custom").expect("name")),
            Some("value"),
            "custom",
        )
        .expect("admitted property");
    assert_eq!(
        projection.bom_properties.get("Custom").map(String::as_str),
        Some("value")
    );
}

#[test]
fn metadata_attribute_refuses_collection_limit_before_insert() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projection = MetadataProjection {
        title: Some("Drawing".into()),
        ..MetadataProjection::default()
    };
    let mut attributes = std::collections::BTreeMap::new();
    assert!(matches!(
        projection.apply_attributes(&ctx, &mut attributes),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor metadata attribute"
    ));
    assert!(attributes.is_empty());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    projection
        .apply_attributes(&ctx, &mut attributes)
        .expect("admitted attribute");
    assert_eq!(attributes.get("title").map(String::as_str), Some("Drawing"));
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
        type_code: 0x0047,
        format: u32::MAX,
        data: root,
    };
    assert_eq!(
        preview_bytes(&value).map(|(_, media)| media.as_str()),
        Some("image/png")
    );

    bytes[8..10].copy_from_slice(&6_u16.to_le_bytes());
    let arena = DecodeArena::new();
    let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("synthetic preview fits policy");
    let value = PropertyValue::Clipboard {
        type_code: 0x0047,
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
    assert_eq!(decoded.report().format(), "inventor");
    assert!(!decoded.report().container_only());
    assert!(decoded.report().losses.iter().any(|loss| loss.code
        == InventorLossCode::GeometryKernelCarrierNotTransferred
            .kind(&cadmpeg_test_support::service_decode_context())
            .expect("expected loss code")));
    let native_findings = validation_findings(decoded.ir());
    assert_eq!(native_findings.len(), 1, "{native_findings:#?}");
    // The structural fixture has no readable registry body. The schema-31
    // grammar is applied to it regardless of what the `RSeDb` streams declared,
    // so what is reported is where that attempt stopped, not a version verdict.
    assert!(
        native_findings[0].message.contains("segment count"),
        "{native_findings:#?}"
    );
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let container_only = EditableDecodeResult::from(
        InventorCodec
            .decode(&mut std::io::Cursor::new(source), &options)
            .expect("container-only Inventor decode succeeds"),
    );
    assert_eq!(
        container_only
            .report()
            .losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        // The structural fixture has no `RSeDb` stream and no segment, so it
        // declares neither version this codec gates on and is admitted
        // unverified.
        [InventorLossCode::SourceDialectUnverified
            .kind(&cadmpeg_test_support::service_decode_context())
            .expect("expected loss code")]
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
        matches!(
            record.records,
            crate::native::SegmentBulkFrame::Unavailable { .. }
        )
    }));
    assert!(container_only
        .source_fidelity()
        .retained_records()
        .is_empty());
}

#[test]
fn database_record_projects_schema_and_creation_and_save_versions() {
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(primary_envelope_fixture()),
            &DecodeOptions::default(),
        )
        .expect("primary envelope decodes");
    let native = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("Inventor namespace");
    let records = native
        .arena_as::<DatabaseRecord>("databases")
        .expect("database arena");
    let [record] = records.as_slice() else {
        panic!("one DatabaseRecord")
    };
    assert_eq!(record.schema, 31);
    assert_eq!(
        record.created_by,
        VersionTupleRecord {
            revision: 1,
            minor: 2,
            major: 24,
            state: "0405060708".into(),
        }
    );
    assert_eq!(
        record.saved_by,
        VersionTupleRecord {
            revision: 1,
            minor: 2,
            major: 25,
            state: "0405060708".into(),
        }
    );
    assert_eq!(record.created_filetime, 17);
    assert_eq!(record.saved_filetime, 18);
    assert_eq!(record.note, "synthetic primary document");
    assert!(native
        .arena_as::<DatabaseIssueRecord>("database_issues")
        .expect("database native arena")
        .is_empty());
}

#[test]
fn database_issues_preserve_the_unframed_database_instead_of_a_database_record() {
    let source =
        crate::test_support::test_fixtures::primary_envelope_fixture_with_broken_database();
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
        .expect("broken database does not prevent envelope decode");
    let native = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("Inventor namespace");
    assert!(native
        .arena_as::<DatabaseRecord>("databases")
        .expect("database native arena")
        .is_empty());
    let issues = native
        .arena_as::<DatabaseIssueRecord>("database_issues")
        .expect("database native arena");
    let [issue] = issues.as_slice() else {
        panic!("one database issue")
    };
    assert_eq!(issue.id, "inventor:rse:database-issue#v1");
    assert_eq!(issue.band, 1);
    assert!(issue.detail.contains("RSe database schema 31"));
    assert!(issue.detail.contains("did not frame it"));
    assert!(issue.detail.contains("creation version"));
}

#[test]
fn decodes_the_synthetic_primary_rse_envelope_end_to_end() {
    let source = primary_envelope_fixture();
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&InventorCodec, &source),
        Confidence::High
    );
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
        .expect("synthetic primary Inventor envelope decodes");
    assert_eq!(decoded.report().format(), "inventor");
    assert_eq!(wire::coverage(decoded.report())["rse_storage_bands"], 1);
    assert_eq!(wire::coverage(decoded.report())["rse_databases"], 1);
    assert_eq!(wire::coverage(decoded.report())["rse_registry_entries"], 1);
    assert_eq!(wire::coverage(decoded.report())["rse_segment_pairs"], 1);
    assert_eq!(wire::coverage(decoded.report())["rse_segment_meta"], 1);
    assert_eq!(wire::coverage(decoded.report())["rse_records"], 1);
    assert_eq!(
        wire::coverage(decoded.report())["active_kernel_carriers"],
        1
    );
    assert!(decoded.report().losses.iter().any(|loss| loss.code
        == InventorLossCode::GeometryKernelCarrierNotTransferred
            .kind(&cadmpeg_test_support::service_decode_context())
            .expect("expected loss code")));

    let native = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("Inventor native namespace exists");
    let active = native
        .arena_as::<crate::native::ActiveCarrierRecord>("active_carrier")
        .expect("active carrier arena exists");
    assert_eq!(active.len(), 1);
    assert!(matches!(
        active[0],
        crate::native::ActiveCarrierRecord::Selected { .. }
    ));
    assert!(validation_findings(decoded.ir()).is_empty());
}

/// The `acis:` kernel layer one decode reported, with the losses beside it.
fn kernel_layer_of(bytes: &[u8]) -> (cadmpeg_core::dialect::DialectMatch, Vec<String>) {
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(bytes.to_vec()),
            &DecodeOptions::default(),
        )
        .expect("the save-format band degrades rather than refuses");
    let report = decoded.report();
    let layer = report
        .dialects()
        .as_ref()
        .expect("the report is classified")
        .iter()
        .find(|matched| matched.format() == "acis")
        .unwrap_or_else(|| panic!("a kernel layer, got {:#?}", report.dialects()))
        .clone();
    let codes = report
        .losses
        .iter()
        .map(|loss| format!("{:?}", loss.code))
        .collect::<Vec<_>>();
    (layer, codes)
}

#[test]
fn a_verified_acis_carrier_reports_its_band_admitted() {
    let bytes = primary_envelope_fixture_with_kernel(
        EnvelopeDeclarations::default(),
        &acis_kernel_stream(21_800),
    );
    let (layer, codes) = kernel_layer_of(&bytes);

    assert_eq!(layer.dialect().as_str(), "acis:save-format-218");
    assert_eq!(
        layer.admission(),
        &cadmpeg_core::dialect::Admission::Admitted
    );
    assert!(
        !codes.iter().any(|code| code.contains("dialect-unverified")),
        "{codes:?}"
    );
}

#[test]
fn binary_kernel_resabs_is_converted_from_centimetres() {
    let bytes = primary_envelope_fixture_with_kernel(
        EnvelopeDeclarations::default(),
        &acis_sphere_kernel_stream(21_800),
    );
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("binary ACIS sphere decodes");
    let expected_resabs_mm = 0.000_01;
    assert!(
        (decoded.ir().tolerances.linear.get() - expected_resabs_mm).abs()
            <= f64::EPSILON * expected_resabs_mm
    );
}

#[test]
fn an_unverified_acis_carrier_is_read_and_marked() {
    // The band is not a gate: the carrier is framed and decoded, the kernel
    // layer says which grammar was substituted, and the recovery is charged.
    let bytes = primary_envelope_fixture_with_kernel(
        EnvelopeDeclarations::default(),
        &acis_kernel_stream(70_000),
    );
    let (layer, codes) = kernel_layer_of(&bytes);

    assert_eq!(layer.dialect().as_str(), "acis:save-format-binary-other");
    assert!(matches!(
        layer.admission(),
        cadmpeg_core::dialect::Admission::Unverified { .. }
    ));
    assert_eq!(
        layer
            .using(&cadmpeg_test_support::service_decode_context())
            .expect("service lookup"),
        Some(cadmpeg_core::dialect_id!("acis:save-format-218"))
    );
    assert_eq!(layer.declared()[DECLARED_SAVE_FORMAT_MAJOR], "700");
    assert!(
        codes
            .iter()
            .any(|code| { code.contains(InventorLossCode::KernelDialectUnverified.code()) }),
        "{codes:?}"
    );
}

#[test]
fn an_unverified_acis_carrier_recovers_the_same_solid_as_a_verified_one() {
    // The recovery is content, not a label: the same records under a band no
    // `acis:` row verifies decode into the same geometry the verified band
    // produces. An empty carrier would satisfy the admission assertions above
    // without reading anything, so this case carries real records.
    let decode = |save_format_version: u32| {
        let bytes = primary_envelope_fixture_with_kernel(
            EnvelopeDeclarations::default(),
            &acis_sphere_kernel_stream(save_format_version),
        );
        InventorCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .expect("the save-format band degrades rather than refuses")
    };

    let verified = decode(21_800);
    let unverified = decode(70_000);

    for (label, decoded) in [("verified", &verified), ("unverified", &unverified)] {
        assert!(decoded.report().geometry_transferred(), "{label}");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{label}");
        assert_eq!(decoded.ir().model.faces.len(), 1, "{label}");
        assert_eq!(decoded.ir().model.surfaces.len(), 1, "{label}");
    }
    assert_eq!(
        unverified.ir().model.surfaces,
        verified.ir().model.surfaces,
        "the substituted grammar read the same carriers"
    );

    // And the recovery is still declared: the unverified band charges, the
    // verified one does not.
    let charged = |decoded: &cadmpeg_ir::codec::DecodeResult| {
        decoded.report().losses.iter().any(|loss| {
            loss.code
                == InventorLossCode::KernelDialectUnverified
                    .kind(&cadmpeg_test_support::service_decode_context())
                    .expect("expected loss code")
        })
    };
    assert!(charged(&unverified));
    assert!(!charged(&verified));
}
