// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, Decoded};
use cadmpeg_ir::ids::{AppearanceId, BodyId, FaceId};
use cadmpeg_ir::topology::Color;

use crate::container::InventorContainer;
use crate::decode::{
    admit_active_carrier_projection, admit_assembly_native_projection, admit_assembly_placement,
    admit_coverage_entries, admit_kernel_annotation, admit_kernel_unknown_fidelity,
    admit_native_record_items, admit_rse_segment_projection, admit_untransferred_carrier,
    admitted_kernel_attribute, admitted_loss, clone_product_body_ids, collect_body_ids,
    decode_container, index_asm_face_keys, index_face_colors, index_projected_colors,
    insert_source_attribute, project_preview_asset, project_property_set_issue,
    project_protein_records, project_protein_state, project_root_product,
    project_ufrx_embedded_reference, project_ufrx_external_reference, project_ufrx_model_state,
    project_ufrx_occurrence, project_ufrx_representation, project_ufrx_state, property_set_name,
    structural_issue,
};

use crate::assembly::{AssemblyInventory, AssemblyOccurrence, AssemblyPlacement};
use crate::compact_matrix::CompactMatrix;
use crate::external_reference::{
    InventorEmbeddedReference, InventorExternalReference, UfrxDocument, UfrxModelState,
    UfrxModelStateParameter, UfrxOccurrence, UfrxRepresentationState, UfrxState,
};
use crate::kernel::ActiveCarrierState;
use crate::loss::InventorLossCode;
use crate::native::ufrx::UfrxRecord;
use crate::native::{AssemblyPlacementRecordWire, StructuralIssueRecord};
use crate::property_set::{Property, PropertySection, PropertyValue};
use crate::protein::{ProteinInstanceRecords, ProteinState};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{DocumentKind, RecordFrameState, SegmentBulkState, SegmentKind, SegmentMetaState};
use crate::test_support::test_fixtures::{fixture_with_ufrx, primary_envelope_fixture};
use crate::test_support::test_fixtures::{push_u16, push_u32, push_utf16};
use crate::InventorCodec;

#[test]
fn property_set_name_scan_refuses_work_before_lookup() {
    let bytes = [];
    let section = PropertySection {
        fmtid: [0; 16],
        code_page: None,
        offsets_ordered: true,
        dictionary_entries: 0,
        properties: vec![Property {
            id: 1,
            name: None,
            value: PropertyValue::Empty { type_code: 0 },
            raw: View::over_retained(&bytes),
        }],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        property_set_name(&ctx, &section),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "scan Inventor property set name"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(property_set_name(&ctx, &section)
        .expect("name scan")
        .is_none());
}

#[test]
fn rse_segment_native_projection_refuses_before_pair_id_creation() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    assert_eq!(container.rse.segments.len(), 1);
    assert!(container.rse.segments[0].identity_issues.is_empty());
    let token = container.rse.segments[0].pair.token.as_str();
    let id_len = "inventor:rse:segment#".len() + token.len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id_len - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_rse_segment_projection(&ctx, &container),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor segment pair id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_rse_segment_projection(&ctx, &container).expect("admitted RSe projection");
}

#[test]
fn rse_segment_native_projection_refuses_each_parsed_copy_before_creation() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    let operations = rse_retained_refusal_operations(&arena, &container);
    for operation in [
        "retain Inventor segment pair id",
        "retain Inventor segment pair token",
        "retain Inventor segment metadata id",
        "retain Inventor segment metadata token",
        "retain Inventor segment kind",
        "retain Inventor segment display name",
        "retain Inventor segment GUID",
        "retain Inventor segment creation text",
        "retain Inventor segment modification text",
        "retain Inventor segment body digest",
        "retain Inventor segment terminal GUID",
        "retain Inventor metadata section id",
        "retain Inventor metadata section token",
        "retain Inventor metadata section digest",
        "retain Inventor metadata type id",
        "retain Inventor metadata type token",
        "retain Inventor metadata type GUID",
        "retain Inventor RSe record id",
        "retain Inventor RSe record token",
        "retain Inventor RSe record type GUID",
        "retain Inventor RSe payload digest",
        "retain Inventor RSe trailer digest",
        "retain Inventor RSe stream trailer digest",
        "retain Inventor segment bulk id",
        "retain Inventor segment bulk token",
        "retain Inventor segment bulk prefix",
        "retain Inventor compressed bulk digest",
        "retain Inventor expanded bulk digest",
    ] {
        assert!(
            operations.contains(&operation),
            "no refusal for {operation}"
        );
    }
}

#[test]
fn rse_segment_native_projection_refuses_malformed_and_unpaired_copies() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let mut container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    let token = container.rse.segments[0].pair.token.clone();
    container.rse.segments[0]
        .identity_issues
        .push("identity issue".into());
    container.rse.segments[0].meta = SegmentMetaState::Malformed {
        declared: None,
        detail: "metadata issue".into(),
    };
    container.rse.segments[0].bulk = SegmentBulkState::Malformed("bulk issue".into());
    container.rse.unpaired_metadata.push(token.clone());
    container.rse.unpaired_bulk.push(token);
    let operations = rse_retained_refusal_operations(&arena, &container);
    for operation in [
        "retain Inventor segment identity issue id",
        "retain Inventor segment identity issue scope",
        "retain Inventor segment identity issue detail",
        "retain Inventor metadata issue id",
        "retain Inventor metadata issue token",
        "retain Inventor metadata issue detail",
        "retain Inventor bulk issue id",
        "retain Inventor bulk issue token",
        "retain Inventor bulk issue detail",
        "retain Inventor unpaired metadata id",
        "retain Inventor unpaired metadata token",
        "retain Inventor unpaired bulk id",
        "retain Inventor unpaired bulk token",
    ] {
        assert!(
            operations.contains(&operation),
            "no refusal for {operation}"
        );
    }
}

fn rse_retained_refusal_operations<'a>(
    arena: &DecodeArena,
    container: &InventorContainer<'a>,
) -> Vec<&'static str> {
    let mut cap = 0;
    let mut operations = Vec::new();
    let mut admitted = false;
    for _ in 0..128 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
        match admit_rse_segment_projection(&ctx, &container) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert!(limit.used + limit.additional > cap);
                operations.push(limit.operation);
                cap = limit.used + limit.additional;
            }
            Ok(()) => {
                admitted = true;
                break;
            }
            Err(error) => panic!("unexpected projection error: {error}"),
        }
    }
    assert!(admitted, "RSe projection did not reach service success");
    operations
}

#[test]
fn active_carrier_native_record_refuses_before_id_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("inventor:kernel:active-carrier#root".len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_active_carrier_projection(&ctx, &ActiveCarrierState::NotApplicable),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor active carrier id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_active_carrier_projection(&ctx, &ActiveCarrierState::NotApplicable)
        .expect("admitted active carrier");
}

#[test]
fn selected_active_carrier_refuses_token_and_digest_before_native_copy() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    let ActiveCarrierState::Selected(carrier) = &container.rse.active_carrier else {
        panic!("fixture must select an active carrier");
    };
    let id_len = "inventor:kernel:active-carrier#root".len();
    let token_len = carrier.segment_token.as_str().len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(id_len + token_len - 1).expect("token budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_active_carrier_projection(&ctx, &container.rse.active_carrier),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor active carrier segment token"
    ));
    policy.limits.max_retained_bytes =
        u64::try_from(id_len + token_len + 63).expect("digest budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_active_carrier_projection(&ctx, &container.rse.active_carrier),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor active carrier digest"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_active_carrier_projection(&ctx, &container.rse.active_carrier)
        .expect("admitted selected carrier");
}

#[test]
fn assembly_occurrence_native_record_refuses_before_id_creation() {
    let inventory = AssemblyInventory {
        occurrences: vec![AssemblyOccurrence {
            segment_token: "segment".into(),
            record_ordinal: 1,
            header_value: 0,
            header_id: 0,
            next_reference: 0,
            flags: 0,
            owner_reference: 0,
            node_index: 0,
            state: [0; 2],
            ordinal_key: 0,
            related_references: Vec::new(),
            child_reference: 0,
            occurrence_id: 0,
        }],
        placements: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("inventor:assembly:occurrence#segment-1".len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_assembly_native_projection(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor assembly occurrence id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_assembly_native_projection(&ctx, &inventory).expect("admitted occurrence");
}

#[test]
fn assembly_placement_native_record_refuses_id_and_digest_before_creation() {
    let suffix = [1_u8];
    let inventory = AssemblyInventory {
        occurrences: Vec::new(),
        placements: vec![AssemblyPlacement {
            segment_token: "segment".into(),
            record_ordinal: 1,
            header_id: 0,
            owner_reference: 0,
            attribute_reference: 0,
            state: 0,
            transform_prefix: false,
            transform: CompactMatrix::try_new(0, 0, |_| Ok(0.0)).expect("finite matrix"),
            branch: 0,
            graphics_state: 0,
            occurrence_id: 0,
            graphics_index: 0,
            object_reference: 0,
            suffix: View::over_retained(&suffix),
        }],
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let id_len = "inventor:assembly:placement#segment-1".len();
    let token_len = "segment".len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id_len - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_assembly_native_projection(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor assembly placement id"
    ));
    policy.limits.max_retained_bytes =
        u64::try_from(id_len + token_len + 63).expect("digest budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_assembly_native_projection(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor assembly placement suffix digest"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_assembly_native_projection(&ctx, &inventory).expect("admitted placement");
}

#[test]
fn decode_loss_refuses_before_message_and_code_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admitted_loss(
            &ctx,
            InventorLossCode::RseSegmentPairUntyped,
            format_args!("Retained {} segment", 1),
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor decode loss"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(
        "inventor".len()
            + InventorLossCode::RseSegmentPairUntyped.code().len()
            + "Retained 1 segment".len()
            - 1,
    )
    .expect("loss length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admitted_loss(
            &ctx,
            InventorLossCode::RseSegmentPairUntyped,
            format_args!("Retained {} segment", 1),
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor decode loss message"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let note = admitted_loss(
        &ctx,
        InventorLossCode::RseSegmentPairUntyped,
        format_args!("Retained {} segment", 1),
    )
    .expect("admitted loss");
    assert_eq!(note.message, "Retained 1 segment");
}

#[test]
fn coverage_map_refuses_before_key_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_coverage_entries(&ctx, [crate::coverage::RSE_DATABASES]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor coverage measure"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(crate::coverage::RSE_DATABASES.as_str().len() - 1).expect("key fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_coverage_entries(&ctx, [crate::coverage::RSE_DATABASES]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor coverage key"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_coverage_entries(&ctx, [crate::coverage::RSE_DATABASES]).expect("admitted coverage key");
}

#[test]
fn root_product_refuses_before_id_and_metadata_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("inventor:document:product#root".len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut admitted = 0;
    assert!(matches!(
        project_root_product(
            &ctx,
            &DocumentKind::Part,
            &crate::decode::MetadataProjection::default(),
            0,
            &mut admitted,
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor root product id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admitted = 0;
    let product = project_root_product(
        &ctx,
        &DocumentKind::Part,
        &crate::decode::MetadataProjection::default(),
        0,
        &mut admitted,
    )
    .expect("admitted root product");
    assert_eq!(product.id.as_str(), "inventor:document:product#root");
}

#[test]
fn initial_source_attribute_refuses_before_key_and_value_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut attributes = std::collections::BTreeMap::new();
    assert!(matches!(
        insert_source_attribute(&ctx, &mut attributes, "kind", format_args!("{}", 42)),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor source attribute"
    ));
    assert!(attributes.is_empty());
    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        insert_source_attribute(&ctx, &mut attributes, "kind", format_args!("{}", 42)),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor source attribute key"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    insert_source_attribute(&ctx, &mut attributes, "kind", format_args!("{}", 42))
        .expect("admitted attribute");
    assert_eq!(attributes["kind"], "42");
}

#[test]
fn kernel_annotation_refuses_before_provenance_creation() {
    let record = cadmpeg_asm::brep::annotations::AnnotationRecord {
        id: "inventor:body#one".into(),
        stream: "carrier".into(),
        offset: 7,
        tag: cadmpeg_asm::brep::annotations::AnnotationTag::ProceduralSurface,
        derived_fields: vec!["definition.surface"],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_kernel_annotation(&ctx, &record),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor kernel provenance"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_kernel_annotation(&ctx, &record).expect("admitted annotation");
}

#[test]
fn retained_carrier_fidelity_refuses_before_unknown_record_creation() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    let ActiveCarrierState::Selected(carrier) = &container.rse.active_carrier else {
        panic!("fixture selects a carrier");
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_untransferred_carrier(&ctx, carrier, "inventor:kernel:active-carrier#root"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor retained carrier"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_untransferred_carrier(&ctx, carrier, "inventor:kernel:active-carrier#root")
        .expect("admitted retained carrier");
}

#[test]
fn kernel_unknown_fidelity_refuses_before_native_record_copy() {
    let unknown = cadmpeg_ir::UnknownRecord::retained(
        cadmpeg_ir::ids::UnknownId::mint("inventor:kernel:unknown#one").expect("valid id"),
        0,
        vec![1],
        vec!["inventor:body#one".into()],
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admit_kernel_unknown_fidelity(
            &ctx,
            &cadmpeg_ir::SourceFidelity::default(),
            std::slice::from_ref(&unknown),
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor kernel unknown fidelity"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_kernel_unknown_fidelity(&ctx, &cadmpeg_ir::SourceFidelity::default(), &[unknown])
        .expect("admitted unknown fidelity");
}

#[test]
fn kernel_header_attribute_refuses_before_key_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        admitted_kernel_attribute(&ctx, "kernel_flags", format_args!("{}", 7)),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor kernel attribute"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert_eq!(
        admitted_kernel_attribute(&ctx, "kernel_flags", format_args!("{}", 7))
            .expect("admitted kernel attribute"),
        "7"
    );
}

#[test]
fn property_set_issue_refuses_retained_limit_before_record_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let id = "inventor:property:set-issue#7";
    policy.limits.max_retained_bytes = u64::try_from(id.len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_property_set_issue(&ctx, 7, "PropertySet", "malformed"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor property-set issue id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let record =
        project_property_set_issue(&ctx, 7, "PropertySet", "malformed").expect("admitted issue");
    assert_eq!(record.id, id);
    assert_eq!(record.path, "PropertySet");
    assert_eq!(record.detail, "malformed");
}

#[test]
fn preview_asset_refuses_collection_and_retained_limits_before_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_preview_asset(&ctx, &mut 0_u64, 0, "inventor:property:value#1-0-17", b"image", "image/png"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor preview asset"
    ));

    policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_preview_asset(&ctx, &mut 0_u64, 0, "inventor:property:value#1-0-17", b"image", "image/png"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor preview asset entity"
    ));

    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_preview_asset(&ctx, &mut 0_u64, 0, "inventor:property:value#1-0-17", b"image", "image/png"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor preview identity key"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_preview_asset(&ctx, &mut 0_u64, 0, "inventor:property:value#1-0-17", b"image", "image/png"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "format Inventor preview ordinal"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let asset = project_preview_asset(
        &ctx,
        &mut 0_u64,
        0,
        "inventor:property:value#1-0-17",
        b"image",
        "image/png",
    )
    .expect("admitted preview");
    assert_eq!(asset.id.as_str(), "inventor:document:asset#preview-0");
    assert_eq!(
        asset.native_ref.as_deref(),
        Some("inventor:property:value#1-0-17")
    );
}

#[test]
fn protein_state_refuses_retained_limit_before_id_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let id = "inventor:protein:state#root";
    policy.limits.max_retained_bytes = u64::try_from(id.len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_protein_state(&ctx, &ProteinState::Absent),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor Protein state id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(matches!(
        project_protein_state(&ctx, &ProteinState::Absent).expect("admitted state"),
        crate::native::protein::ProteinRecord::Absent { id: actual } if actual == id
    ));
}

fn protein_asset_instance() -> Vec<ProteinInstanceRecords> {
    vec![ProteinInstanceRecords {
        entry_name: "AssetData/InstanceProperties.bin".into(),
        records: vec![cadmpeg_protein::DecodedRecord {
            ordinal: 0,
            logical_offset: 0,
            schema: "schema".into(),
            guid: "guid".into(),
            base: "base".into(),
            asset_lib_id: "library".into(),
            properties: std::collections::BTreeMap::new(),
        }],
        rejected: Vec::new(),
    }]
}

fn protein_rejection_instance() -> Vec<ProteinInstanceRecords> {
    vec![ProteinInstanceRecords {
        entry_name: "AssetData/InstanceProperties.bin".into(),
        records: Vec::new(),
        rejected: vec![cadmpeg_protein::RejectedRecord {
            ordinal: 1,
            detail: "bad record".into(),
        }],
    }]
}

#[test]
fn protein_asset_id_refuses_retained_limit_before_record_creation() {
    let arena = DecodeArena::new();
    let entry = "AssetData/InstanceProperties.bin";
    let id_len = "inventor:protein:asset#".len() + 64 + "-0".len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id_len - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_protein_records(&ctx, protein_asset_instance()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor Protein asset id"
    ));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(entry.len() - 1).expect("entry length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_protein_records(&ctx, protein_asset_instance()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "hash Inventor Protein entry name"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let records = project_protein_records(&ctx, protein_asset_instance()).expect("admitted asset");
    assert_eq!(records.assets.len(), 1);
    assert!(records.rejections.is_empty());
    assert!(records.issues.is_empty());
}

#[test]
fn protein_rejection_id_refuses_retained_limit_before_record_creation() {
    let arena = DecodeArena::new();
    let id_len = "inventor:protein:rejection#".len() + 64 + "-1".len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id_len - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_protein_records(&ctx, protein_rejection_instance()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor Protein rejection id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let records =
        project_protein_records(&ctx, protein_rejection_instance()).expect("admitted rejection");
    assert!(records.assets.is_empty());
    assert_eq!(records.rejections.len(), 1);
    assert!(records.issues.is_empty());
}

#[test]
fn ufrx_state_id_refuses_retained_limit_before_record_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let id = "inventor:ufrx:state#root";
    policy.limits.max_retained_bytes = u64::try_from(id.len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_state(&ctx, &UfrxState::Absent, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx state id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(matches!(
        project_ufrx_state(&ctx, &UfrxState::Absent, &mut Vec::new()).expect("admitted state"),
        UfrxRecord::Absent { id: actual } if actual == id
    ));
}

#[test]
fn ufrx_model_state_refuses_id_and_parameter_limits_before_creation() {
    let bytes = [0_u8; 77];
    let arena = DecodeArena::new();
    let (setup_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("source context");
    let mut state = UfrxModelState {
        prefix: 0,
        name: "Primary".into(),
        state: [0; 2],
        prefix_count: 0,
        parameters: Vec::new(),
        suffix: source,
    };
    let id_len = "inventor:ufrx:model-state#0".len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id_len - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_model_state(&ctx, 0, &state, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx model-state id"
    ));
    state.parameters.push(UfrxModelStateParameter {
        name: "Length".into(),
        tag: 0,
        kind: 0,
        state: 0,
        value: "1".into(),
        trailer: 0,
    });
    policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_model_state(&ctx, 0, &state, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "copy Inventor UFRx state parameters"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from("ufrx-model-state-0".len() - 1).expect("scope length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_model_state(&ctx, 0, &state, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "format Inventor UFRx model-state issue scope"
    ));
    let mut issues = Vec::new();
    assert!(project_ufrx_model_state(&setup_ctx, 0, &state, &mut issues)
        .expect("admitted model state")
        .is_some());
    assert!(issues.is_empty());
}

#[test]
fn structural_issue_refuses_collection_limit_before_record_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        structural_issue(&ctx, "scope", "bad"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor structural issue"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let issue = structural_issue(&ctx, "scope", "bad").expect("admitted issue");
    assert_eq!(issue.scope, "scope");
    assert_eq!(issue.detail, "bad");
}

#[test]
fn protein_conversion_issue_refuses_before_failure_text_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("entry_name must end with InstanceProperties.bin".len() - 1)
            .expect("detail length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let wire = serde_json::from_value(serde_json::json!({
        "id": "asset", "entry_name": "bad.bin", "ordinal": 3,
        "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
            "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
    }))
    .expect("Protein asset wire fixture");
    let mut issues = Vec::new();
    assert!(matches!(
        crate::decode::admit_protein_asset(&ctx, wire, &mut issues),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor Protein asset conversion issue"
    ));
    assert!(issues.is_empty());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let wire = serde_json::from_value(serde_json::json!({
        "id": "asset", "entry_name": "bad.bin", "ordinal": 3,
        "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
            "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
    }))
    .expect("Protein asset wire fixture");
    assert!(crate::decode::admit_protein_asset(&ctx, wire, &mut issues)
        .expect("service admission")
        .is_none());
    assert_eq!(issues.len(), 1);
}

#[test]
fn ufrx_model_state_conversion_issue_refuses_before_failure_text_creation() {
    let bytes = [0_u8; 76];
    let arena = DecodeArena::new();
    let (_, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("source context");
    let state = UfrxModelState {
        prefix: 0,
        name: "Primary".into(),
        state: [0; 2],
        prefix_count: 0,
        parameters: Vec::new(),
        suffix: source,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("suffix_len must be 77".len() - 1).expect("detail length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut issues = Vec::new();
    assert!(matches!(
        project_ufrx_model_state(&ctx, 0, &state, &mut issues),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx model-state conversion issue"
    ));
    assert!(issues.is_empty());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(project_ufrx_model_state(&ctx, 0, &state, &mut issues)
        .expect("service admission")
        .is_none());
    assert_eq!(issues.len(), 1);
}

#[test]
fn ufrx_external_reference_refuses_id_and_state_group_limits_before_creation() {
    let arena = DecodeArena::new();
    let mut reference = InventorExternalReference {
        path: "part.ipt".into(),
        library_id: 0,
        library_name: String::new(),
        display_name: String::new(),
        state_groups: Vec::new(),
        state: [0; 2],
        document_id: [0; 16],
        database_id: [0; 16],
        reference_id: 7,
        occurrence_count: 1,
        version: 0,
        flags: 0,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("inventor:ufrx:external-reference#0".len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_external_reference(&ctx, 0, &reference, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx external reference id"
    ));
    reference.state_groups.push([0; 3]);
    policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_external_reference(&ctx, 0, &reference, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "copy Inventor UFRx reference state groups"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(
        project_ufrx_external_reference(&ctx, 0, &reference, &mut Vec::new())
            .expect("admitted reference")
            .is_some()
    );
}

#[test]
fn ufrx_external_conversion_issue_refuses_before_failure_text_creation() {
    let arena = DecodeArena::new();
    let reference = InventorExternalReference {
        path: String::new(),
        library_id: 0,
        library_name: String::new(),
        display_name: String::new(),
        state_groups: Vec::new(),
        state: [0; 2],
        document_id: [0; 16],
        database_id: [0; 16],
        reference_id: 7,
        occurrence_count: 1,
        version: 0,
        flags: 0,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("path or a nonzero document_id is required".len() - 1)
            .expect("detail length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut issues = Vec::new();
    assert!(matches!(
        project_ufrx_external_reference(&ctx, 0, &reference, &mut issues),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx external conversion issue"
    ));
    assert!(issues.is_empty());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(
        project_ufrx_external_reference(&ctx, 0, &reference, &mut issues)
            .expect("service admission")
            .is_none()
    );
    assert_eq!(issues.len(), 1);
}

#[test]
fn ufrx_embedded_reference_refuses_id_limit_before_creation() {
    let bytes = [0_u8; 1];
    let arena = DecodeArena::new();
    let (_, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("source context");
    let reference = InventorEmbeddedReference {
        value_0: 0,
        filetime: 0,
        value_1: 0,
        extended_value: None,
        value_2: 0,
        path: String::new(),
        library_id: 0,
        library_name: String::new(),
        state: 0,
        display_name: String::new(),
        state_values: [0; 8],
        source,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("inventor:ufrx:embedded-reference#0".len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_embedded_reference(&ctx, 0, &reference, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx embedded reference id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(
        project_ufrx_embedded_reference(&ctx, 0, &reference, &mut Vec::new())
            .expect("admitted embedded reference")
            .is_some()
    );
}

#[test]
fn ufrx_occurrence_refuses_id_limit_before_creation() {
    let bytes = [0_u8; 1];
    let arena = DecodeArena::new();
    let (_, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("source context");
    let occurrence = UfrxOccurrence {
        end_string_flag: 0,
        file_reference_id: 0,
        occurrence_id: 0,
        header_value: 0,
        title: None,
        header_padding_words: 0,
        source,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("inventor:ufrx:occurrence#0".len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_occurrence(&ctx, 0, &occurrence, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx occurrence id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(
        project_ufrx_occurrence(&ctx, 0, &occurrence, &mut Vec::new())
            .expect("admitted occurrence")
            .is_some()
    );
}

#[test]
fn ufrx_representation_refuses_retained_limit_before_creation() {
    let arena = DecodeArena::new();
    let representation = UfrxRepresentationState {
        prefix: 0,
        active_representation: None,
        secondary_active_lod_state: [0; 2],
        active_model_state: "Primary".into(),
        active_model_state_state: [0; 2],
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("Primary".len() - 1).expect("name length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_ufrx_representation(&ctx, &representation, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor UFRx active model state"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(
        project_ufrx_representation(&ctx, &representation, &mut Vec::new())
            .expect("admitted representation")
            .is_some()
    );
}

#[test]
fn projected_body_ids_refuse_collection_and_retained_limits_before_copy() {
    let id = BodyId::mint("inventor:test:body#one").expect("valid body id");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        collect_body_ids(&ctx, [&id]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor projected body ids"
    ));

    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = (id.as_str().len() - 1) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        collect_body_ids(&ctx, [&id]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor projected body id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert_eq!(
        collect_body_ids(&ctx, [&id]).expect("admitted body id"),
        vec![id]
    );
}

#[test]
fn product_body_id_copy_refuses_limits_before_target_changes() {
    let id = BodyId::mint("inventor:test:body#one").expect("valid body id");
    let old = BodyId::mint("inventor:test:body#old").expect("valid old body id");
    let body_ids = vec![id.clone()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut target = vec![old.clone()];
    assert!(matches!(
        clone_product_body_ids(&ctx, &body_ids, &mut target),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor product body ids"
    ));
    assert_eq!(target, vec![old.clone()]);

    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(id.as_str().len() - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        clone_product_body_ids(&ctx, &body_ids, &mut target),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor product body id"
    ));
    assert_eq!(target, vec![old]);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    clone_product_body_ids(&ctx, &body_ids, &mut target).expect("admitted copy");
    assert_eq!(target, body_ids);
}

#[test]
fn projected_appearance_color_index_refuses_limits_before_id_copy() {
    let id = AppearanceId::mint("inventor:test:appearance#one").expect("valid appearance id");
    let color = Color::new(0.2, 0.3, 0.4, 1.0).expect("valid color");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        index_projected_colors(&ctx, [(&id, color)]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "index Inventor projected appearance colors"
    ));

    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = (id.as_str().len() - 1) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        index_projected_colors(&ctx, [(&id, color)]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor projected appearance color id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert_eq!(
        index_projected_colors(&ctx, [(&id, color)]).expect("admitted color")[&id],
        color
    );
}

#[test]
fn face_color_index_refuses_limits_before_face_id_copy() {
    let id = FaceId::mint("inventor:test:face#one").expect("valid face id");
    let color = Color::new(0.2, 0.3, 0.4, 1.0).expect("valid color");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        index_face_colors(&ctx, [(&id, color)]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "index Inventor face colors"
    ));

    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = (id.as_str().len() - 1) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        index_face_colors(&ctx, [(&id, color)]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor face color id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert_eq!(
        index_face_colors(&ctx, [(&id, color)]).expect("admitted color")[&id],
        color
    );
}

#[test]
fn asm_face_key_index_refuses_before_face_id_copy() {
    let id = FaceId::mint("inventor:test:face#one").expect("valid face id");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        index_asm_face_keys(&ctx, [(&id, 7)]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "index Inventor ASM face keys"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id.as_str().len() - 1).expect("id fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        index_asm_face_keys(&ctx, [(&id, 7)]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor ASM face key id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert_eq!(
        index_asm_face_keys(&ctx, [(&id, 7)]).expect("admitted key")[&id],
        7
    );
}

#[test]
fn native_structural_records_refuse_aggregate_limit_before_materialization() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture fits service policy");
    let container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    assert_eq!(container.rse.databases.len(), 1);
    let assembly =
        crate::assembly::inventory(&setup_ctx, &container.rse).expect("assembly inventory");
    let presentation =
        crate::presentation::inventory(&setup_ctx, &container.rse).expect("presentation inventory");
    let design = crate::design::inventory(&setup_ctx, &container.rse).expect("design inventory");
    let sketch = crate::sketch::inventory(&setup_ctx, &container.rse).expect("sketch inventory");
    let feature = crate::feature::inventory(&setup_ctx, &container.rse).expect("feature inventory");

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("decode context");
    assert!(matches!(
        admit_native_record_items(&limited_ctx, &container, &assembly, &presentation, &design, &sketch, &feature),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "retain Inventor native structural records"
    ));
    assert!(admit_native_record_items(
        &setup_ctx,
        &container,
        &assembly,
        &presentation,
        &design,
        &sketch,
        &feature
    )
    .is_ok());
    assert!(decode_container(&setup_ctx, &container).is_ok());
}

#[test]
fn native_structural_entities_refuse_before_model_projection() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture fits service policy");
    let container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    let assembly =
        crate::assembly::inventory(&setup_ctx, &container.rse).expect("assembly inventory");
    let presentation =
        crate::presentation::inventory(&setup_ctx, &container.rse).expect("presentation inventory");
    let design = crate::design::inventory(&setup_ctx, &container.rse).expect("design inventory");
    let sketch = crate::sketch::inventory(&setup_ctx, &container.rse).expect("sketch inventory");
    let feature = crate::feature::inventory(&setup_ctx, &container.rse).expect("feature inventory");
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("decode context");
    assert!(matches!(
        admit_native_record_items(&limited_ctx, &container, &assembly, &presentation, &design, &sketch, &feature),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor native structural records"
    ));
    assert!(admit_native_record_items(
        &setup_ctx,
        &container,
        &assembly,
        &presentation,
        &design,
        &sketch,
        &feature
    )
    .is_ok());
    assert!(decode_container(&setup_ctx, &container).is_ok());
}

#[test]
fn native_validation_propagates_collection_refusal_from_assembly_projection() {
    let bytes = fixture_with_ufrx(&external_references_stream());
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("Inventor fixture decodes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("validation context");
    assert!(matches!(
        InventorCodec.validate_native(&ctx, decoded.ir()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "index Inventor external references"
    ));

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service validation context");
    assert!(InventorCodec.validate_native(&ctx, decoded.ir()).is_ok());
}

#[test]
fn empty_external_identity_does_not_fail_file_decode() {
    let bytes = fixture_with_ufrx(&external_references_stream());
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("invalid external identity must not fail file decode");
    assert_ufrx_issue(decoded.ir(), "ufrx-external-reference-0", "document_id");
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::InventorLossCode::UfrxTableMalformed.kind()));
    let namespace = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert_eq!(ufrx.external_references().len(), 1);
    assert_eq!(ufrx.external_references()[0].ordinal, 1);
    assert_eq!(ufrx.external_references()[0].reference_id, 8);
}

#[test]
fn rejected_model_state_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| document.model_states[0].name.clear());
    assert_ufrx_issue(&decoded.ir, "ufrx-model-state-0", "name");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert!(ufrx.model_states().is_empty());
    assert_eq!(ufrx.external_references().len(), 1);
}

#[test]
fn rejected_representation_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| {
        document
            .representation
            .as_mut()
            .expect("representation fixture")
            .active_model_state
            .clear();
    });
    assert_ufrx_issue(&decoded.ir, "ufrx-representation", "active_model_state");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    assert!(matches!(
        UfrxRecord::read(namespace).expect("admitted UFRx arenas agree"),
        UfrxRecord::ParsedPrefix {
            representation: None,
            ..
        }
    ));
}

#[test]
fn rejected_embedded_reference_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| {
        document.embedded_references[0].source = document.unparsed_tail;
    });
    assert_ufrx_issue(&decoded.ir, "ufrx-embedded-reference-0", "record_len");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert!(ufrx.embedded_references().is_empty());
    assert_eq!(ufrx.external_references().len(), 1);
}

#[test]
fn rejected_occurrence_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| document.occurrences[0].header_padding_words = 9);
    assert_ufrx_issue(&decoded.ir, "ufrx-occurrence-0", "header_padding_words");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert!(ufrx.occurrences().is_empty());
    assert_eq!(ufrx.external_references().len(), 1);
}

#[test]
fn nonfinite_assembly_placement_transform_is_rejected_at_parse() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("container fixture");
    let mut container = InventorContainer::open(&ctx, root).expect("container fixture");
    let mut payload = vec![0; 15];
    payload.extend_from_slice(&0_u16.to_le_bytes());
    payload.extend_from_slice(&0_u16.to_le_bytes());
    payload.extend_from_slice(&f64::INFINITY.to_le_bytes());
    let segment = &mut container.rse.segments[0];
    segment.kind = SegmentKind::AmGraphics;
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("framed fixture");
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("record fixture");
    };
    table.records[0].type_id = [
        0xa2, 0x63, 0x71, 0xca, 0xd0, 0x11, 0xb2, 0xd3, 0x00, 0x08, 0xbf, 0xbb, 0x21, 0xed, 0xdc,
        0x09,
    ];
    table.records[0].payload = View::over_retained(&payload);
    let decoded =
        decode_container(&ctx, &container).expect("invalid placement must not fail decode");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let issues = namespace
        .arena_as::<RecordIssue>("assembly_record_issues")
        .expect("assembly issues");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert!(issues[0].detail.contains("finite"), "{:?}", issues[0]);
    assert!(namespace
        .arena_as::<serde_json::Value>("assembly_placements")
        .expect("placements")
        .is_empty());
    assert!(super::validation_findings(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains(&issues[0].detail)));
}

#[test]
fn rejected_placement_digest_records_its_source_and_keeps_later_placements() {
    let wire = serde_json::json!({
        "id": "inventor:assembly:placement#segment-1", "segment_token": "segment", "record_ordinal": 1,
        "header_id": 0, "owner_reference": 0, "attribute_reference": 0, "state": 0,
        "transform_prefix": false, "transform_encoding": [0, 0],
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "branch": 0, "graphics_state": 0, "occurrence_id": 1, "graphics_index": 0,
        "object_reference": 0, "suffix_len": 48, "suffix_sha256": "invalid"
    });
    let mut issues = Vec::new();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let bad: AssemblyPlacementRecordWire =
        serde_json::from_value(wire.clone()).expect("wire fixture");
    assert!(admit_assembly_placement(&ctx, bad, &mut issues)
        .expect("service admission")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].segment_token, "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert!(issues[0].detail.contains("suffix_sha256"));
    let mut wire = wire;
    wire["suffix_sha256"] = serde_json::json!("0".repeat(64));
    let good = serde_json::from_value(wire).expect("wire fixture");
    assert!(admit_assembly_placement(&ctx, good, &mut issues)
        .expect("service admission")
        .is_some());
    assert_eq!(issues.len(), 1);
}

#[test]
fn placement_conversion_issue_refuses_before_failure_text_creation() {
    let wire: AssemblyPlacementRecordWire = serde_json::from_value(serde_json::json!({
        "id": "inventor:assembly:placement#segment-1", "segment_token": "segment", "record_ordinal": 1,
        "header_id": 0, "owner_reference": 0, "attribute_reference": 0, "state": 0,
        "transform_prefix": false, "transform_encoding": [0, 0],
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "branch": 0, "graphics_state": 0, "occurrence_id": 1, "graphics_index": 0,
        "object_reference": 0, "suffix_len": 0, "suffix_sha256": "0".repeat(64)
    }))
    .expect("placement wire");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("suffix_len must not be zero".len() - 1).expect("detail length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut issues = Vec::new();
    assert!(matches!(
        admit_assembly_placement(&ctx, wire, &mut issues),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor placement conversion issue"
    ));
    assert!(issues.is_empty());
}

fn assert_ufrx_issue(ir: &cadmpeg_ir::document::CadIr, scope: &str, field: &str) {
    let namespace = ir.native.namespace("inventor").expect("native namespace");
    let issues = namespace
        .arena_as::<StructuralIssueRecord>("structural_issues")
        .expect("structural issues");
    let issue = issues
        .iter()
        .find(|issue| issue.scope == scope)
        .expect("rejected record has an issue");
    assert!(issue.detail.contains(field), "{}", issue.detail);
    assert!(super::validation_findings(ir)
        .iter()
        .any(|finding| finding.message.contains(scope) && finding.message.contains(field)));
}

fn decode_ufrx(edit: impl FnOnce(&mut UfrxDocument<'_>)) -> Decoded {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("container fixture");
    let mut container = InventorContainer::open(&ctx, root).expect("container fixture");
    let source = root.child(0, 77).expect("source bytes");
    let mut document = UfrxDocument {
        stream: container
            .snapshot
            .stream("RSeStorage/RSeSegInfo")
            .expect("validated fixture stream")
            .id(),
        schema: 15,
        section_versions: vec![1],
        original_file_name: "part.ipt".into(),
        caption: "part".into(),
        representation: Some(UfrxRepresentationState {
            prefix: 0,
            active_representation: None,
            secondary_active_lod_state: [0; 2],
            active_model_state: "Primary".into(),
            active_model_state_state: [0; 2],
        }),
        model_states: vec![UfrxModelState {
            prefix: 0,
            name: "Primary".into(),
            state: [0; 2],
            prefix_count: 0,
            parameters: vec![],
            suffix: source,
        }],
        references: vec![InventorExternalReference {
            path: "part.ipt".into(),
            library_id: 0,
            library_name: String::new(),
            display_name: String::new(),
            state_groups: vec![],
            state: [0; 2],
            document_id: [0; 16],
            database_id: [0; 16],
            reference_id: 7,
            occurrence_count: 1,
            version: 0,
            flags: 0,
        }],
        embedded_references: vec![InventorEmbeddedReference {
            value_0: 0,
            filetime: 0,
            value_1: 0,
            extended_value: None,
            value_2: 0,
            path: String::new(),
            library_id: 0,
            library_name: String::new(),
            state: 0,
            display_name: String::new(),
            state_values: [0; 8],
            source,
        }],
        occurrences: vec![UfrxOccurrence {
            end_string_flag: 0,
            file_reference_id: 7,
            occurrence_id: 42,
            header_value: 0,
            title: None,
            header_padding_words: 0,
            source,
        }],
        unparsed_tail: root.child(0, 0).expect("empty tail"),
    };
    edit(&mut document);
    container.ufrx = UfrxState::Parsed(Box::new(document));
    let decoded =
        decode_container(&ctx, &container).expect("rejected native record must not fail decode");
    assert!(decoded
        .body
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::InventorLossCode::UfrxTableMalformed.kind()));
    decoded
}

fn external_references_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [
        11, 23, 31, 19, 12, 18, 1, 2, 4, 2, 1, 3, 1, 2, 6, 2, 2, 5, 0, 1, 2, 0, 0, 0, 0,
    ] {
        push_u16(&mut bytes, value);
    }
    bytes.extend_from_slice(&[0; 32]);
    push_utf16(&mut bytes, "");
    bytes.extend_from_slice(&[0; 48]);
    push_u32(&mut bytes, 0);
    bytes.extend_from_slice(&[0; 16]);
    push_utf16(&mut bytes, "assembly.iam");
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes.extend_from_slice(&[0; 4]);
    push_utf16(&mut bytes, "Default");
    bytes.extend_from_slice(&[0; 4]);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 3);
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, 1);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 2);
    push_utf16(&mut bytes, "References");
    push_u32(&mut bytes, 0);
    for (path, id) in [("", 7), ("part.ipt", 8)] {
        push_utf16(&mut bytes, path);
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        push_utf16(&mut bytes, "");
        push_u16(&mut bytes, 0);
        push_utf16(&mut bytes, "");
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        bytes.extend_from_slice(&[0; 32]);
        for value in [id, 0, 12, 4] {
            push_u32(&mut bytes, value);
        }
    }
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes
}

#[test]
fn protein_admission_keeps_later_assets_and_rejections() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let mut issues = Vec::new();
    let assets = ["bad.bin", "assets/InstanceProperties.bin"]
        .into_iter()
        .filter_map(|entry_name| {
            let wire = serde_json::from_value(serde_json::json!({
                "id": "asset", "entry_name": entry_name, "ordinal": 3,
                "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
                    "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
            }))
            .expect("Protein asset wire fixture");
            crate::decode::admit_protein_asset(&ctx, wire, &mut issues).expect("service admission")
        })
        .collect::<Vec<_>>();
    assert_eq!(assets.len(), 1);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].scope, "asset");
    assert!(issues[0].detail.contains("entry_name"));
    let rejections = ["bad.bin", "assets/InstanceProperties.bin"]
        .into_iter()
        .filter_map(|entry_name| {
            crate::decode::admit_protein_rejection(
                &ctx,
                crate::native::protein::ProteinRejectionRecordWire {
                    id: "rejection".into(),
                    entry_name: entry_name.into(),
                    ordinal: 4,
                    detail: "invalid record".into(),
                },
                &mut issues,
            )
            .expect("service admission")
        })
        .collect::<Vec<_>>();
    assert_eq!(rejections.len(), 1);
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[1].scope, "rejection");
    assert!(issues[1].detail.contains("entry_name"));
}
