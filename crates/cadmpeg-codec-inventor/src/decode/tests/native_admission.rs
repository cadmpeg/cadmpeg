// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, Decoded};

use crate::container::InventorContainer;
use crate::decode::rse_native_projection;
use crate::decode::{
    admit_assembly_placement, admit_kernel_annotation, decode_container, project_preview_asset,
    project_property_set_issue, project_protein_records, project_protein_state,
    project_root_product, project_ufrx_embedded_reference, project_ufrx_external_reference,
    project_ufrx_model_state, project_ufrx_occurrence, project_ufrx_representation,
    project_ufrx_state, property_set_name, structural_issue,
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
use crate::native::{
    ActiveCarrierRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecordWire,
    StructuralIssueRecord,
};
use crate::property_set::{Property, PropertySection, PropertyValue};
use crate::protein::{ProteinInstanceRecords, ProteinState};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{DocumentKind, RecordFrameState, SegmentBulkState, SegmentKind, SegmentMetaState};
use crate::test_support::test_fixtures::{fixture_with_ufrx, primary_envelope_fixture};
use crate::test_support::test_fixtures::{push_u16, push_u32, push_utf16};
use crate::InventorCodec;

#[test]
fn property_set_name_is_absent_without_property_255() {
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
    assert!(property_set_name(&section).is_none());
}

fn fixture_container<'a>(arena: &'a DecodeArena, bytes: &'a [u8]) -> InventorContainer<'a> {
    let (setup_ctx, root) = DecodeContext::from_root_bytes(bytes, arena, &DecodePolicy::service())
        .expect("fixture context");
    InventorContainer::open(&setup_ctx, root).expect("fixture container")
}

#[test]
fn rse_native_projection_projects_the_fixture() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let container = fixture_container(&arena, &bytes);
    assert_eq!(container.rse.segments.len(), 1);
    assert!(container.rse.segments[0].identity_issues.is_empty());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    rse_native_projection::project(&ctx, &container).expect("admitted RSe projection");
}

#[test]
fn rse_native_projection_projects_malformed_unpaired_and_unframed_segments() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let mut container = fixture_container(&arena, &bytes);
    let SegmentBulkState::Framed(bulk) = &mut container.rse.segments[0].bulk else {
        panic!("fixture bulk must be framed");
    };
    bulk.records = RecordFrameState::Unavailable("frame issue".into());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    rse_native_projection::project(&ctx, &container).expect("unframed records project");

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
    rse_native_projection::project(&ctx, &container).expect("malformed segments project");
}

#[test]
fn active_carrier_native_record_names_the_root() {
    assert_eq!(
        ActiveCarrierRecord::from_state(&ActiveCarrierState::NotApplicable).id(),
        "inventor:kernel:active-carrier#root"
    );
}

#[test]
fn selected_active_carrier_projects_its_native_record() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let container = fixture_container(&arena, &bytes);
    assert!(matches!(
        container.rse.active_carrier,
        ActiveCarrierState::Selected(_)
    ));
    assert!(matches!(
        ActiveCarrierRecord::from_state(&container.rse.active_carrier),
        ActiveCarrierRecord::Selected { .. }
    ));
}

#[test]
fn assembly_occurrence_native_record_refuses_entity_limit_before_creation() {
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
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    AssemblyOccurrenceRecord::from_occurrence(&ctx, &inventory.occurrences[0])
        .expect("admitted occurrence");
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        AssemblyOccurrenceRecord::from_occurrence(&ctx, &inventory.occurrences[0]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor native assembly occurrence"
    ));
}

#[test]
fn assembly_placement_native_record_refuses_entity_limit_before_creation() {
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
            transform: CompactMatrix::try_new(0, 0, |_| Ok(cadmpeg_ir::scalar::FiniteReal::ZERO))
                .expect("finite matrix"),
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (limited, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let wire = AssemblyPlacementRecordWire::from_placement(&inventory.placements[0]);
    assert!(matches!(
        admit_assembly_placement(&limited, wire, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor native assembly placement"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let wire = AssemblyPlacementRecordWire::from_placement(&inventory.placements[0]);
    assert!(admit_assembly_placement(&ctx, wire, &mut Vec::new())
        .expect("service placement admission")
        .is_some());
}

#[test]
fn root_product_has_the_root_identity() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let mut admitted = 0;
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
fn kernel_annotation_is_admitted() {
    let record = cadmpeg_asm::brep::annotations::AnnotationRecord {
        id: "inventor:body#one".into(),
        stream: "carrier".into(),
        offset: 7,
        tag: cadmpeg_asm::brep::annotations::AnnotationTag::ProceduralSurface,
        derived_fields: vec!["definition.surface"],
    };
    admit_kernel_annotation(
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &record,
    )
    .expect("admitted annotation");
}

#[test]
fn property_set_issue_record_keeps_path_and_detail() {
    let record = project_property_set_issue(7, "PropertySet", "malformed");
    assert_eq!(record.id, "inventor:property:set-issue#7");
    assert_eq!(record.path, "PropertySet");
    assert_eq!(record.detail, "malformed");
}

#[test]
fn preview_asset_refuses_entity_limit_before_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_preview_asset(&ctx, &mut 0_u64, 0, "inventor:property:value#1-0-17", b"image", "image/png"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor preview asset entity"
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
fn preview_asset_charges_its_retained_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        project_preview_asset(&ctx, &mut 0_u64, 0, "inventor:property:value#1-0-17", b"image", "image/png"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor preview asset"
    ));
}

#[test]
fn absent_protein_state_names_the_root() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(matches!(
        project_protein_state(&ctx, &ProteinState::Absent).expect("admitted state"),
        crate::native::protein::ProteinRecord::Absent { id } if id == "inventor:protein:state#root"
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
fn protein_asset_record_is_projected() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let records = project_protein_records(&ctx, protein_asset_instance()).expect("admitted asset");
    assert_eq!(records.assets.len(), 1);
    assert!(records.rejections.is_empty());
    assert!(records.issues.is_empty());
}

#[test]
fn protein_rejection_record_is_projected() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let records =
        project_protein_records(&ctx, protein_rejection_instance()).expect("admitted rejection");
    assert!(records.assets.is_empty());
    assert_eq!(records.rejections.len(), 1);
    assert!(records.issues.is_empty());
}

#[test]
fn absent_ufrx_state_names_the_root() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(matches!(
        project_ufrx_state(&ctx, &UfrxState::Absent, &mut Vec::new()).expect("admitted state"),
        UfrxRecord::Absent { id } if id == "inventor:ufrx:state#root"
    ));
}

#[test]
fn ufrx_model_state_with_a_parameter_is_admitted() {
    let bytes = [0_u8; 77];
    let arena = DecodeArena::new();
    let (setup_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("source context");
    let state = UfrxModelState {
        prefix: 0,
        name: "Primary".into(),
        state: [0; 2],
        prefix_count: 0,
        parameters: vec![UfrxModelStateParameter {
            name: "Length".into(),
            tag: 0,
            kind: 0,
            state: 0,
            value: "1".into(),
            trailer: 0,
        }],
        suffix: source,
    };
    let mut issues = Vec::new();
    assert!(project_ufrx_model_state(&setup_ctx, 0, &state, &mut issues)
        .expect("admitted model state")
        .is_some());
    assert!(issues.is_empty());
}

#[test]
fn structural_issue_keeps_scope_and_detail() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let issue = structural_issue(&ctx, "scope", "bad").expect("admitted issue");
    assert_eq!(issue.scope, "scope");
    assert_eq!(issue.detail, "bad");
}

#[test]
fn protein_conversion_failure_becomes_an_issue() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let wire = serde_json::from_value(serde_json::json!({
        "id": "asset", "entry_name": "bad.bin", "ordinal": 3,
        "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
            "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
    }))
    .expect("Protein asset wire fixture");
    let mut issues = Vec::new();
    assert!(crate::decode::admit_protein_asset(&ctx, wire, &mut issues)
        .expect("service admission")
        .is_none());
    assert_eq!(issues.len(), 1);
}

#[test]
fn ufrx_model_state_conversion_failure_becomes_an_issue() {
    let bytes = [0_u8; 76];
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("source context");
    let state = UfrxModelState {
        prefix: 0,
        name: "Primary".into(),
        state: [0; 2],
        prefix_count: 0,
        parameters: Vec::new(),
        suffix: source,
    };
    let mut issues = Vec::new();
    assert!(project_ufrx_model_state(&ctx, 0, &state, &mut issues)
        .expect("service admission")
        .is_none());
    assert_eq!(issues.len(), 1);
}

fn external_reference(path: &str) -> InventorExternalReference {
    InventorExternalReference {
        path: path.into(),
        library_id: 0,
        library_name: String::new(),
        display_name: String::new(),
        state_groups: vec![[0; 3]],
        state: [0; 2],
        document_id: [0; 16],
        database_id: [0; 16],
        reference_id: 7,
        occurrence_count: 1,
        version: 0,
        flags: 0,
    }
}

#[test]
fn ufrx_external_reference_with_a_state_group_is_admitted() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(project_ufrx_external_reference(
        &ctx,
        0,
        &external_reference("part.ipt"),
        &mut Vec::new()
    )
    .expect("admitted reference")
    .is_some());
}

#[test]
fn ufrx_external_conversion_failure_becomes_an_issue() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let mut issues = Vec::new();
    assert!(
        project_ufrx_external_reference(&ctx, 0, &external_reference(""), &mut issues)
            .expect("service admission")
            .is_none()
    );
    assert_eq!(issues.len(), 1);
}

#[test]
fn ufrx_embedded_reference_is_admitted() {
    let bytes = [0_u8; 1];
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
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
    assert!(
        project_ufrx_embedded_reference(&ctx, 0, &reference, &mut Vec::new())
            .expect("admitted embedded reference")
            .is_some()
    );
}

#[test]
fn ufrx_occurrence_is_admitted() {
    let bytes = [0_u8; 1];
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
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
    assert!(
        project_ufrx_occurrence(&ctx, 0, &occurrence, &mut Vec::new())
            .expect("admitted occurrence")
            .is_some()
    );
}

#[test]
fn ufrx_representation_is_admitted() {
    let arena = DecodeArena::new();
    let representation = UfrxRepresentationState {
        prefix: 0,
        active_representation: None,
        secondary_active_lod_state: [0; 2],
        active_model_state: "Primary".into(),
        active_model_state_state: [0; 2],
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(
        project_ufrx_representation(&ctx, &representation, &mut Vec::new())
            .expect("admitted representation")
            .is_some()
    );
}

#[test]
fn native_rse_projection_refuses_entity_limit_at_record_creation() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture fits service policy");
    let container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    assert_eq!(container.rse.databases.len(), 1);
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("decode context");
    assert!(matches!(
        rse_native_projection::project(&limited_ctx, &container),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor native structural records"
    ));
    assert!(rse_native_projection::project(&setup_ctx, &container).is_ok());
    assert!(decode_container(&setup_ctx, &container).is_ok());
}

#[test]
fn native_validation_accepts_decoded_external_references() {
    let bytes = fixture_with_ufrx(&external_references_stream());
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("Inventor fixture decodes");
    let arena = DecodeArena::new();
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
    let ufrx = UfrxRecord::read(&crate::native::test_ctx(), namespace)
        .expect("admitted UFRx arenas agree");
    assert_eq!(ufrx.external_references().len(), 1);
    assert_eq!(ufrx.external_references()[0].ordinal(), 1);
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
    let ufrx = UfrxRecord::read(&crate::native::test_ctx(), namespace)
        .expect("admitted UFRx arenas agree");
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
        UfrxRecord::read(&crate::native::test_ctx(), namespace).expect("admitted UFRx arenas agree"),
        UfrxRecord::ParsedPrefix(payload) if payload.representation.is_none()
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
    let ufrx = UfrxRecord::read(&crate::native::test_ctx(), namespace)
        .expect("admitted UFRx arenas agree");
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
    let ufrx = UfrxRecord::read(&crate::native::test_ctx(), namespace)
        .expect("admitted UFRx arenas agree");
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
    assert_eq!(issues[0].segment_token.as_str(), "segment");
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

fn placement_wire(suffix_len: u64, suffix_sha256: &str) -> AssemblyPlacementRecordWire {
    serde_json::from_value(serde_json::json!({
        "id": "inventor:assembly:placement#segment-1", "segment_token": "segment", "record_ordinal": 1,
        "header_id": 0, "owner_reference": 0, "attribute_reference": 0, "state": 0,
        "transform_prefix": false, "transform_encoding": [0, 0],
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "branch": 0, "graphics_state": 0, "occurrence_id": 1, "graphics_index": 0,
        "object_reference": 0, "suffix_len": suffix_len, "suffix_sha256": suffix_sha256
    }))
    .expect("placement wire")
}

#[test]
fn empty_placement_suffix_becomes_an_issue() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let mut issues = Vec::new();
    assert!(
        admit_assembly_placement(&ctx, placement_wire(0, &"0".repeat(64)), &mut issues)
            .expect("service admission")
            .is_none()
    );
    assert_eq!(issues.len(), 1);
    assert!(issues[0].detail.contains("suffix_len"));
}

#[test]
fn uppercase_placement_digest_becomes_an_issue() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let mut issues = Vec::new();
    assert!(
        admit_assembly_placement(&ctx, placement_wire(48, &"A".repeat(64)), &mut issues)
            .expect("service admission")
            .is_none()
    );
    assert_eq!(issues.len(), 1);
    assert!(issues[0].detail.contains("suffix_sha256"));
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

#[test]
fn malformed_optional_ufrx_is_a_warning_without_weakening_required_links() {
    let bytes = fixture_with_ufrx(b"unreadable optional table");
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("bounded optional table");
    let ctx = crate::native::test_ctx();
    let findings = InventorCodec
        .validate_native(&ctx, decoded.ir())
        .expect("validation");
    let finding = findings
        .iter()
        .find(|finding| finding.message.contains("UFRxDoc stream is malformed"))
        .expect("table diagnostic");
    assert_eq!(finding.severity, cadmpeg_ir::report::Severity::Warning);
    let mut broken = decoded.ir().clone();
    broken
        .native
        .namespace_mut("inventor")
        .arenas_mut()
        .remove("segment_pairs");
    let findings = InventorCodec
        .validate_native(&ctx, &broken)
        .expect("required arena validation");
    assert!(findings.iter().any(
        |finding| finding.severity == cadmpeg_ir::report::Severity::Error
            && finding.message.contains("missing arenas")
    ));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == InventorLossCode::UfrxTableMalformed.kind()));
}
