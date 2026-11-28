// SPDX-License-Identifier: Apache-2.0
//! High-level Inventor structural decode.

mod presentation_native_projection;
mod rse_native_projection;

use cadmpeg_ir::annotations::StreamHandle;
use std::collections::{BTreeMap, HashMap};

use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_core::decode::{ArenaClass, DecodeContext, DecodeDepth, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::design::projection::project_layer;
use cadmpeg_ir::document::{CadIr, Model, SourceMeta, Topology};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::decode::TransferLedger;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{
    dropped_with, Cause, DecodeParts, GivenUpPart, GivenUpRecord, Part,
};
use cadmpeg_ir::{AnnotationBuilder, SourceFidelity, UnknownRecord};

use crate::container::InventorContainer;
use crate::database::{RevisionPayload, VersionTuple};
use crate::dialect::{dialect_loss, kernel_dialect_loss, DialectRecovery};
use crate::external_reference::UfrxState;
use crate::kernel::ActiveCarrierState;
use crate::loss::InventorLossCode;
use crate::native::protein::{
    ProteinAssetRecord, ProteinAssetRecordWire, ProteinEntryRecord, ProteinRecord,
    ProteinRejectionRecord, ProteinRejectionRecordWire,
};
use crate::native::ufrx::{
    EmbeddedReferenceRecord, EmbeddedReferenceRecordWire, ExternalReferenceRecord,
    ExternalReferenceRecordWire, UfrxModelStateParameterRecord, UfrxModelStateRecord,
    UfrxModelStateRecordWire, UfrxOccurrenceRecord, UfrxOccurrenceRecordWire, UfrxParsedPrefix,
    UfrxRecord, UfrxRepresentationRecord, UfrxRepresentationRecordWire,
};
use crate::native::{
    ActiveCarrierRecord, AssemblyGroundingRecord, AssemblyOccurrenceRecord,
    AssemblyPlacementRecord, AssemblyPlacementRecordWire, DatabaseIssueRecord, DatabaseRecord,
    DesignViewRepresentationRecord, DesignViewVisibilityOffRecord, PropertyRecord,
    PropertySectionRecord, PropertySetIssueRecord, PropertySetRecord, PropertyValueKind,
    RevisionPayloadForm, RevisionRecord, SegmentRegistryRecord, StorageBandRecord,
    StructuralIssueRecord, VersionTupleRecord,
};
use crate::property_set::{PropertySection, PropertySetState, PropertyValue};
use crate::protein::ProteinState;
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{DatabaseState, DocumentKind, ParsedState};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    decode_container(ctx, root, &InventorContainer::open(ctx, root)?)
}

fn decode_container<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    container: &InventorContainer<'a>,
) -> Result<Decoded, CodecError> {
    // One predicate, read once from the parsed declarations: it decides the
    // admission in `primary` and the dialect-unverified loss below, and neither
    // recomputes the other.
    let recovery = DialectRecovery::of(container);
    let matched = recovery.classify();
    let dialects = crate::dialect::layers(&matched, &container.rse.active_carrier)?;
    let mut assembly_inventory = crate::assembly::inventory(ctx, &container.rse)?;
    let mut presentation_inventory = crate::presentation::inventory(ctx, &container.rse)?;
    let design_inventory = crate::design::inventory(ctx, &container.rse)?;
    let sketch_inventory = crate::sketch::inventory(ctx, &container.rse)?;
    let feature_inventory = crate::feature::inventory(ctx, &container.rse)?;
    let mut ir = CadIr::empty();
    // Design projection derives only design-class records (features,
    // sketches, parameters) and the native records behind them, so a geometry
    // request stops before it.
    let delivered = if ctx.container_only() {
        DecodeDepth::Container
    } else {
        ctx.depth()
    };
    let mut admitted_entities = cadmpeg_core::decode::u64_from_index(ir.entity_count());
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "cfb_major_version".to_owned(),
        container.snapshot.major_version().to_string(),
    );
    let mut document_kind = container.rse.document_kind();
    let mut metadata = MetadataProjection::default();
    let mut property_sets = Vec::new();
    let mut property_sections = Vec::new();
    let mut properties = Vec::new();
    let mut property_set_issues = Vec::new();
    let mut malformed_property_set_count = 0;
    for descriptor in &container.property_sets {
        match &descriptor.state {
            PropertySetState::Unread => {
                let (_, detail) =
                    container
                        .streams
                        .failure(descriptor.stream)
                        .ok_or_else(|| {
                            CodecError::malformed("unread OLE property set has no stream outcome")
                        })?;
                admit_native_items(ctx, 1)?;
                let issue = project_property_set_issue(
                    descriptor.stream.directory_id(),
                    &descriptor.path,
                    detail,
                );
                malformed_property_set_count += 1;
                property_set_issues.push(issue);
            }
            PropertySetState::Parsed(property_set) => {
                admit_native_items(ctx, 1)?;
                let id = format!("inventor:property:set#{}", descriptor.stream.directory_id());
                property_sets.push(PropertySetRecord {
                    id,
                    path: descriptor.path.clone(),
                    directory_id: descriptor.stream.directory_id(),
                    version: property_set.version,
                    system_identifier: property_set.system_identifier,
                    clsid: hex(&property_set.clsid),
                    section_count: cadmpeg_core::decode::u64_from_index(
                        property_set.sections.iter().flatten().count(),
                    ),
                });
                for (section_ordinal, section) in property_set.sections.iter().enumerate() {
                    let Some(section) = section else { continue };
                    let set_name = property_set_name(section);
                    let identity_matches = set_name
                        .as_deref()
                        .and_then(known_property_set_fmtid)
                        .is_none_or(|expected| expected == section.fmtid);
                    if !identity_matches {
                        admit_native_items(ctx, 1)?;
                        let id = format!(
                            "inventor:property:set-identity#{}-{section_ordinal}",
                            descriptor.stream.directory_id()
                        );
                        property_set_issues.push(PropertySetIssueRecord {
                            id,
                            path: descriptor.path.clone(),
                            directory_id: descriptor.stream.directory_id(),
                            detail: "embedded property-set name does not match its FMTID".into(),
                        });
                    }
                    admit_native_items(ctx, 1)?;
                    let section_id = format!(
                        "inventor:property:section#{}-{section_ordinal}",
                        descriptor.stream.directory_id()
                    );
                    property_sections.push(PropertySectionRecord {
                        id: section_id,
                        set_path: descriptor.path.clone(),
                        ordinal: record_ordinal(
                            section_ordinal,
                            "Inventor property section ordinal",
                        )?,
                        fmtid: hex(&section.fmtid),
                        code_page: section.code_page,
                        offsets_ordered: section.offsets_ordered,
                        dictionary_entries: cadmpeg_core::decode::u64_from_index(
                            section.dictionary_entries,
                        ),
                        property_count: cadmpeg_core::decode::u64_from_index(
                            section.properties.len(),
                        ),
                    });
                    for property in &section.properties {
                        admit_native_items(ctx, 1)?;
                        let property_name = property.name.clone().or_else(|| {
                            identity_matches
                                .then_some(set_name.as_deref())
                                .flatten()
                                .and_then(|set_name| built_in_property_name(set_name, property.id))
                                .map(str::to_owned)
                        });
                        let native_id = format!(
                            "inventor:property:value#{}-{section_ordinal}-{}",
                            descriptor.stream.directory_id(),
                            property.id
                        );
                        let scalar_value = property.value.scalar_text();
                        metadata.consider(
                            &section.fmtid,
                            property.id,
                            property_name.as_deref(),
                            scalar_value.as_deref(),
                            &native_id,
                        );
                        if is_preview(&section.fmtid, property.id, property_name.as_deref()) {
                            if let Some((bytes, media_type)) = preview_bytes(&property.value) {
                                let ordinal = ir.model.assets.len();
                                ir.model.assets.push(project_preview_asset(
                                    ctx,
                                    &mut admitted_entities,
                                    ordinal,
                                    &native_id,
                                    bytes,
                                    media_type,
                                )?);
                            }
                        }
                        properties.push(PropertyRecord {
                            id: native_id,
                            set_path: descriptor.path.clone(),
                            section_ordinal: record_ordinal(
                                section_ordinal,
                                "Inventor property section ordinal",
                            )?,
                            fmtid: hex(&section.fmtid),
                            property_id: property.id,
                            name: property_name,
                            value_kind: property_value_kind(&property.value),
                            scalar_value,
                            raw_len: cadmpeg_core::decode::u64_from_index(
                                property.raw.window().len(),
                            ),
                            raw_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(
                                property.raw.window(),
                            ),
                        });
                    }
                }
            }
        }
    }
    let protein_state = project_protein_state(ctx, &container.protein)?;
    let protein = container.protein.read_instances(ctx)?;
    let protein_parts = protein.give_ups(ctx)?;
    let protein_members_unread = !protein_parts.is_empty();
    let protein_instances = protein.members;
    let material_catalog =
        crate::materials::project_catalog(ctx, &protein_instances, &mut admitted_entities)?;
    let ProteinNativeRecords {
        assets: protein_assets,
        rejections: protein_rejections,
        rejected: protein_rejected,
    } = project_protein_records(ctx, protein_instances)?;
    let mut ufrx_rejected = Rejections::default();
    ir.model.appearances = material_catalog.appearances;
    let protein_appearance_count = ir.model.appearances.len();
    let ufrx = project_ufrx_state(ctx, &container.ufrx, &mut ufrx_rejected)?;
    let protein_issue_records = protein_rejected.records;
    let ufrx_issue_records = ufrx_rejected.records;
    let ufrx_issue_count = ufrx_rejected.issues.len();
    let mut structural_issues = protein_rejected.issues;
    structural_issues.extend(ufrx_rejected.issues);
    let ufrx_model_states = ufrx.model_states();
    let external_references = ufrx.external_references();
    let embedded_references = ufrx.embedded_references();
    let ufrx_occurrences = ufrx.occurrences();
    if matches!(document_kind, DocumentKind::Unknown | DocumentKind::Mixed) {
        if let Some(property_kind) = metadata.document_kind.take() {
            document_kind = property_kind;
        }
    }
    attributes.insert(
        "document_kind".to_owned(),
        document_kind.label().to_string(),
    );
    metadata.apply_attributes(&mut attributes);
    ir.source = Some(SourceMeta::classified(
        dialects,
        cadmpeg_core::text::named_entries("the inventor document", attributes)?,
    ));
    if matches!(document_kind, DocumentKind::Part | DocumentKind::Assembly) {
        let entity_count = ir.entity_count();
        ir.model.product_definitions.push(project_root_product(
            ctx,
            &document_kind,
            &metadata,
            entity_count,
            &mut admitted_entities,
        )?);
    }
    let storage_bands = container
        .rse
        .databases
        .iter()
        .map(|database| -> Result<_, CodecError> {
            admit_native_items(ctx, 1)?;
            Ok(StorageBandRecord {
                id: format!("inventor:rse:storage-band#v{}", database.band.value()),
                band: database.band.value(),
                database_directory_id: database.stream.directory_id(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let databases = container
        .rse
        .databases
        .iter()
        .filter_map(|descriptor| {
            let DatabaseState::Parsed(database) = &descriptor.state else {
                return None;
            };
            Some((descriptor, database))
        })
        .map(|(descriptor, database)| -> Result<_, CodecError> {
            admit_native_items(ctx, 1)?;
            Ok(DatabaseRecord {
                id: format!("inventor:rse:database#v{}", descriptor.band.value()),
                band: descriptor.band.value(),
                database_id: hex(&database.id),
                schema: database.schema.value(),
                created_by: version_record(database.created_by),
                created_filetime: database.created_filetime,
                saved_by: version_record(database.saved_by),
                saved_filetime: database.saved_filetime,
                note: database.note.clone(),
            })
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    let mut database_issues = Vec::new();
    for descriptor in &container.rse.databases {
        if let Some(detail) = descriptor.issue_detail() {
            admit_native_items(ctx, 1)?;
            database_issues.push(DatabaseIssueRecord {
                id: format!("inventor:rse:database-issue#v{}", descriptor.band.value()),
                band: descriptor.band.value(),
                detail,
            });
        }
    }
    let segment_registry = match &container.rse.registry {
        ParsedState::Parsed(registry) => registry
            .entries
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| -> Result<_, CodecError> {
                admit_native_items(ctx, 1)?;
                Ok(SegmentRegistryRecord {
                    id: format!("inventor:rse:registry-entry#{ordinal}"),
                    ordinal: record_ordinal(ordinal, "Inventor registry ordinal")?,
                    display_name: entry.display_name.clone(),
                    segment_id: hex(&entry.segment_id),
                    revision_id: hex(&entry.revision_id),
                    type_name: entry.type_name.clone(),
                    object_count: cadmpeg_core::decode::u64_from_index(entry.objects.len()),
                    node_count: cadmpeg_core::decode::u64_from_index(entry.nodes.len()),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
    };
    let revisions = match &container.rse.revisions {
        ParsedState::Parsed(table) => table
            .entries
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| -> Result<_, CodecError> {
                admit_native_items(ctx, 1)?;
                Ok(RevisionRecord {
                    id: format!("inventor:rse:revision#{ordinal}"),
                    ordinal: record_ordinal(ordinal, "Inventor revision ordinal")?,
                    revision_id: hex(&entry.id),
                    flags: entry.flags,
                    kind: entry.kind,
                    payload_form: match entry.payload {
                        RevisionPayload::None => RevisionPayloadForm::None,
                        RevisionPayload::Short(..) => RevisionPayloadForm::Short,
                        RevisionPayload::Long(..) => RevisionPayloadForm::Long,
                    },
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
    };
    let mut given_up = container.give_ups(ctx, false)?;
    given_up.extend(protein_parts);
    let mut issue_records = std::collections::BTreeSet::new();
    let registry_failure = match &container.rse.registry {
        ParsedState::Unavailable(detail) => Some(detail),
        ParsedState::Absent | ParsedState::Parsed(_) => None,
    };
    let revisions_failure = match &container.rse.revisions {
        ParsedState::Unavailable(detail) => Some(detail),
        ParsedState::Absent | ParsedState::Parsed(_) => None,
    };
    for (scope, detail) in [
        ("segment_registry", registry_failure),
        ("revision_table", revisions_failure),
    ] {
        if let Some(detail) = detail {
            structural_issues.push(structural_issue(ctx, scope, detail)?);
        }
    }
    let projection = rse_native_projection::project(ctx, container)?;
    structural_issues.extend(projection.identity_issues);
    let segment_pairs = projection.segment_pairs;
    let segment_meta = projection.segment_meta;
    let meta_sections = projection.meta_sections;
    let meta_types = projection.meta_types;
    let segment_meta_issues = projection.segment_meta_issues;
    let rse_records = projection.rse_records;
    let mut drawing_inventory = crate::drawing::inventory(ctx, container, Some(&rse_records))?;
    given_up.extend(drawing_inventory.section_give_ups(ctx, false)?);
    given_up.append(&mut drawing_inventory.given_up);
    let segment_bulk = projection.segment_bulk;
    let segment_bulk_issues = projection.segment_bulk_issues;
    let unpaired_segments = projection.unpaired_segments;
    admit_native_items(ctx, 1)?;
    let active_carrier = ActiveCarrierRecord::from_state(&container.rse.active_carrier);
    let assembly_occurrences = assembly_inventory
        .occurrences
        .iter()
        .map(|occurrence| AssemblyOccurrenceRecord::from_occurrence(ctx, occurrence))
        .collect::<Result<Vec<_>, CodecError>>()?;
    let assembly_placements = assembly_inventory
        .placements
        .iter()
        .map(|placement| {
            admit_assembly_placement(
                ctx,
                AssemblyPlacementRecordWire::from_placement(placement),
                &mut assembly_inventory.issues,
            )
        })
        .collect::<Result<Vec<_>, CodecError>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let design_view_representations = assembly_inventory
        .representations
        .iter()
        .map(|representation| {
            DesignViewRepresentationRecord::from_representation(ctx, representation)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    let design_view_visibility = assembly_inventory
        .visibility_off
        .iter()
        .map(|state| DesignViewVisibilityOffRecord::from_state(ctx, state))
        .collect::<Result<Vec<_>, CodecError>>()?;
    let assembly_groundings = assembly_inventory
        .groundings
        .iter()
        .map(|grounding| AssemblyGroundingRecord::from_grounding(ctx, grounding))
        .collect::<Result<Vec<_>, CodecError>>()?;
    let presentation_native =
        presentation_native_projection::project(ctx, &mut presentation_inventory)?;
    let pm_app_default_styles = presentation_native.default_styles;
    let pm_app_rendering_styles = presentation_native.rendering_styles;
    let pm_graphics_faces = presentation_native.graphics_faces;
    let pm_graphics_style_collections = presentation_native.graphics_style_collections;
    let pm_graphics_primary_color_styles = presentation_native.graphics_primary_color_styles;
    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(ir.entity_count()),
        &mut admitted_entities,
        "admit Inventor pre-assembly entities",
    )?;
    let active_design_view = ufrx
        .representation()
        .map(|representation| representation.active_design_view.as_str());
    let assembly_projection = crate::assembly::project_occurrences(
        ctx,
        ufrx_occurrences,
        external_references,
        &assembly_occurrences,
        &assembly_placements,
        crate::assembly::OccurrenceStates {
            active_design_view,
            representations: &design_view_representations,
            visibility_off: &design_view_visibility,
            groundings: &assembly_groundings,
        },
    )?;
    if let Some(root) = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.is_root())
    {
        root.occurrences = assembly_projection
            .occurrences
            .iter()
            .map(|occurrence| occurrence.id.clone())
            .collect();
    }
    ir.model
        .product_definitions
        .extend(assembly_projection.definitions);
    ir.model.occurrences = assembly_projection.occurrences;
    ir.model.assembly_joints = assembly_projection.joints;
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("inventor"));
    namespace.set_arena(ctx, "storage_bands", &storage_bands)?;
    namespace.set_arena(ctx, "databases", &databases)?;
    namespace.set_arena(ctx, "database_issues", &database_issues)?;
    namespace.set_arena(ctx, "segment_registry", &segment_registry)?;
    namespace.set_arena(ctx, "revisions", &revisions)?;
    namespace.set_arena(ctx, "structural_issues", &structural_issues)?;
    namespace.set_arena(ctx, "property_sets", &property_sets)?;
    namespace.set_arena(ctx, "property_sections", &property_sections)?;
    namespace.set_arena(ctx, "properties", &properties)?;
    namespace.set_arena(ctx, "property_set_issues", &property_set_issues)?;
    protein_state.install(ctx, namespace)?;
    namespace.set_arena(ctx, "protein_assets", &protein_assets)?;
    namespace.set_arena(ctx, "protein_rejections", &protein_rejections)?;
    ufrx.install(ctx, namespace)?;
    namespace.set_arena(ctx, "assembly_occurrences", &assembly_occurrences)?;
    namespace.set_arena(ctx, "assembly_placements", &assembly_placements)?;
    namespace.set_arena(
        ctx,
        "design_view_representations",
        &design_view_representations,
    )?;
    namespace.set_arena(ctx, "design_view_visibility", &design_view_visibility)?;
    namespace.set_arena(ctx, "assembly_groundings", &assembly_groundings)?;
    namespace.set_arena(ctx, "assembly_record_issues", &assembly_inventory.issues)?;
    namespace.set_arena(ctx, "pm_app_default_styles", &pm_app_default_styles)?;
    namespace.set_arena(ctx, "pm_app_rendering_styles", &pm_app_rendering_styles)?;
    namespace.set_arena(ctx, "pm_graphics_faces", &pm_graphics_faces)?;
    namespace.set_arena(
        ctx,
        "pm_graphics_style_collections",
        &pm_graphics_style_collections,
    )?;
    namespace.set_arena(
        ctx,
        "pm_graphics_primary_color_styles",
        &pm_graphics_primary_color_styles,
    )?;
    namespace.set_arena(
        ctx,
        "presentation_record_issues",
        &presentation_inventory.issues,
    )?;
    namespace.set_arena(ctx, "pm_dc_parameters", &design_inventory.parameters)?;
    namespace.set_arena(ctx, "pm_dc_expressions", &design_inventory.expressions)?;
    namespace.set_arena(ctx, "pm_dc_units", &design_inventory.units)?;
    namespace.set_arena(
        ctx,
        "pm_dc_parameter_tables",
        &design_inventory.parameter_tables,
    )?;
    namespace.set_arena(ctx, "pm_dc_model_roots", &design_inventory.model_roots)?;
    namespace.set_arena(ctx, "design_record_issues", &design_inventory.issues)?;
    namespace.set_arena(ctx, "pm_dc_sketches", &sketch_inventory.sketches)?;
    namespace.set_arena(ctx, "pm_dc_sketch_entities", &sketch_inventory.entities)?;
    namespace.set_arena(ctx, "pm_dc_transforms", &sketch_inventory.transforms)?;
    namespace.set_arena(ctx, "pm_dc_directions", &sketch_inventory.directions)?;
    namespace.set_arena(
        ctx,
        "pm_dc_sketch_constraints",
        &sketch_inventory.constraints,
    )?;
    namespace.set_arena(ctx, "sketch_record_issues", &sketch_inventory.issues)?;
    let pattern_operands = feature_inventory.pattern_operands.native_records()?;
    if !pattern_operands.is_empty() {
        namespace.set_arena(ctx, "pm_dc_pattern_operands", &pattern_operands)?;
    }
    namespace.set_arena(ctx, "pm_dc_features", &feature_inventory.features)?;
    namespace.set_arena(
        ctx,
        "pm_dc_pattern_features",
        &feature_inventory.pattern_features,
    )?;
    namespace.set_arena(
        ctx,
        "pm_dc_feature_terminators",
        &feature_inventory.terminators,
    )?;
    namespace.set_arena(
        ctx,
        "pm_dc_feature_properties",
        &feature_inventory.properties,
    )?;
    namespace.set_arena(
        ctx,
        "pm_dc_feature_dimensions",
        &feature_inventory.dimensions,
    )?;
    namespace.set_arena(ctx, "pm_dc_feature_labels", &feature_inventory.labels)?;
    if !feature_inventory.thread_definitions.is_empty() {
        namespace.set_arena(
            ctx,
            "pm_dc_thread_definitions",
            &feature_inventory.thread_definitions,
        )?;
    }
    if !feature_inventory.sheet_metal_unfold_rules.is_empty() {
        namespace.set_arena(
            ctx,
            "pm_app_sheet_metal_unfold_rules",
            &feature_inventory.sheet_metal_unfold_rules,
        )?;
    }
    if !feature_inventory.sheet_metal_unfold_selections.is_empty() {
        namespace.set_arena(
            ctx,
            "pm_app_sheet_metal_unfold_selections",
            &feature_inventory.sheet_metal_unfold_selections,
        )?;
    }
    if !feature_inventory.sheet_metal_styles.is_empty() {
        namespace.set_arena(
            ctx,
            "pm_dc_sheet_metal_styles",
            &feature_inventory.sheet_metal_styles,
        )?;
    }
    if !feature_inventory.sheet_metal_flanges.is_empty() {
        namespace.set_arena(
            ctx,
            "pm_dc_sheet_metal_flanges",
            &feature_inventory.sheet_metal_flanges,
        )?;
    }
    namespace.set_arena(
        ctx,
        "pm_dc_entity_style_links",
        &feature_inventory.entity_style_links,
    )?;
    namespace.set_arena(
        ctx,
        "pm_dc_kernel_entity_links",
        &feature_inventory.kernel_entity_links,
    )?;
    namespace.set_arena(ctx, "pm_dc_work_planes", &feature_inventory.work_planes)?;
    namespace.set_arena(ctx, "feature_record_issues", &feature_inventory.issues)?;
    namespace.set_arena(ctx, "segment_pairs", &segment_pairs)?;
    namespace.set_arena(ctx, "segment_meta", &segment_meta)?;
    namespace.set_arena(ctx, "meta_sections", &meta_sections)?;
    namespace.set_arena(ctx, "meta_types", &meta_types)?;
    namespace.set_arena(ctx, "segment_meta_issues", &segment_meta_issues)?;
    namespace.set_arena(ctx, "segment_bulk", &segment_bulk)?;
    namespace.set_arena(ctx, "rse_records", &rse_records)?;
    namespace.set_arena(ctx, "segment_bulk_issues", &segment_bulk_issues)?;
    namespace.set_arena(ctx, "unpaired_segments", &unpaired_segments)?;
    namespace.set_arena(ctx, "active_carrier", std::slice::from_ref(&active_carrier))?;

    drawing_inventory.write(ctx, namespace)?;
    let unread_streams: Vec<_> = container.streams.damaged().collect();
    let mut losses = crate::container::deviation_losses(&container.snapshot, &unread_streams);
    losses.extend(drawing_inventory.losses());
    let mut geometry_failure = None;
    let mut given_up_carrier = None;
    let mut carrier_cause = Cause::Unreadable;
    let mut carrier_given_up = false;
    let mut history_chain = None;
    let mut decoded_states = std::collections::BTreeMap::new();
    let mut feature_history_states = Vec::new();
    let mut state_unknowns = Vec::new();
    let mut empty_kernel_stream = false;
    let mut kernel_withheld = Vec::new();
    let kernel_brep = match &container.rse.active_carrier {
        ActiveCarrierState::Selected(carrier) => match carrier.header.as_ref() {
            Ok(header) => {
                match ctx.run_part(|| {
                    crate::kernel::decode_kernel_carrier(ctx, carrier, header).map_err(|failure| {
                        carrier_cause = failure.cause;
                        failure.error
                    })
                })? {
                    Ok(decoded) => {
                        empty_kernel_stream = decoded.model_records == 0;
                        kernel_withheld = decoded.withheld;
                        apply_kernel_header(
                            &mut ir,
                            carrier.family,
                            &decoded.header.metadata,
                            &mut losses,
                        );
                        Some(decoded.brep)
                    }
                    Err(error) => {
                        carrier_cause = carrier_cause.of(&error);
                        geometry_failure = Some(format!("{error}"));
                        given_up_carrier = Some(carrier_part(carrier));
                        None
                    }
                }
            }
            Err(detail) => {
                geometry_failure = Some(detail.to_owned());
                given_up_carrier = Some(carrier_part(carrier));
                None
            }
        },
        ActiveCarrierState::Unreadable {
            segment_token,
            record_ordinal,
            payload_offset,
            payload,
            ..
        } => {
            given_up_carrier = Some((
                crate::rse::bulk_record_part(
                    "rse-record",
                    (segment_token, *record_ordinal),
                    *payload_offset,
                    payload.window(),
                    ArenaClass::Model,
                ),
                (segment_token.as_str().to_owned(), *record_ordinal),
            ));
            None
        }
        ActiveCarrierState::NotApplicable | ActiveCarrierState::Unavailable(_) => None,
    };
    let active_geometry = kernel_brep.is_some();
    let kernel_brep = kernel_brep.unwrap_or_else(AsmBrep::default);
    let body_keys = kernel_brep.body_native_keys.clone();
    let face_keys = kernel_brep
        .face_native_keys
        .iter()
        .filter_map(|record| record.asm_face_key.map(|key| (record.face.clone(), key)))
        .collect::<HashMap<_, _>>();
    let (
        _,
        AsmTransferRemainder {
            unknowns: kernel_unknowns,
            stats: kernel_stats,
            annotation_records: kernel_annotations,
            unbounded_faces: kernel_unbounded_faces,
            withheld: _,
        },
    ) = transfer_into_ir(ctx, &mut ir, cadmpeg_ir::codec_id!("inventor"), kernel_brep)?;
    let history_framing = match &container.rse.active_carrier {
        ActiveCarrierState::Selected(carrier) if delivered >= DecodeDepth::Design => {
            crate::feature::history_states::frame_carrier(ctx, carrier)?
        }
        _ => crate::feature::history_states::Framing::Absent,
    };
    let history_read = match &container.rse.active_carrier {
        ActiveCarrierState::Selected(carrier) => {
            crate::feature::history_states::read(ctx, carrier, &history_framing)?
        }
        _ => crate::feature::history_states::Read::Absent,
    };
    let state_views = match (&container.rse.active_carrier, &history_read) {
        (
            ActiveCarrierState::Selected(carrier),
            crate::feature::history_states::Read::Closed(closed),
        ) => Some(crate::feature::history_views::StateViews::new(
            carrier,
            closed,
            crate::feature::history_views::MAX_STATE_VIEW_WORK,
        )),
        _ => None,
    };
    let result_states: Option<&dyn crate::feature::history_views::ResultStates> = match &state_views
    {
        Some(views) => Some(views),
        None => None,
    };
    let mut unresolved = if delivered >= DecodeDepth::Design {
        project_design(
            ctx,
            &mut ir,
            &container.rse,
            &design_inventory,
            &sketch_inventory,
            &feature_inventory,
            result_states,
        )?
    } else {
        UnresolvedDesign::default()
    };
    let mut malformed_annotations = Vec::new();
    let mut needs_parameter = Vec::new();
    if delivered >= DecodeDepth::Geometry {
        let (annotation_losses, malformed, needs) = crate::annotation::project(
            ctx,
            &crate::annotation::Inputs {
                rse: &container.rse,
                labels: &feature_inventory.labels,
                parameters: &design_inventory.parameters,
                properties: &feature_inventory.properties,
            },
            &mut ir,
        )?;
        losses.extend(annotation_losses);
        malformed_annotations = malformed;
        needs_parameter = needs;
    }
    let mut stored_body_losses = Vec::new();
    if delivered >= DecodeDepth::Design && (!body_keys.is_empty() || result_states.is_some()) {
        let projection = crate::feature::base::project(
            ctx,
            &crate::feature::base::Inputs {
                document: &container.rse,
                inventory: &feature_inventory,
                design: &design_inventory,
                transforms: &sketch_inventory.transforms,
                keys: &body_keys,
                states: result_states,
            },
            &mut ir,
        )?;
        unresolved.features = unresolved
            .features
            .checked_sub(projection.transferred)
            .ok_or_else(|| {
                CodecError::malformed("Inventor Base projection exceeds unresolved feature count")
            })?;
        stored_body_losses = projection.losses;
    }
    let composite_links = if delivered >= DecodeDepth::Design {
        crate::feature::composite::links(
            ctx,
            &container.rse,
            &feature_inventory,
            &design_inventory,
        )?
    } else {
        Vec::new()
    };
    let composite = crate::feature::composite::project(
        ctx,
        &feature_inventory,
        &composite_links,
        &active_carrier,
        &crate::feature::body_selection::Scope {
            metadata: &segment_meta,
            roots: &design_inventory.model_roots,
        },
        if delivered >= DecodeDepth::Design {
            &body_keys
        } else {
            &[]
        },
        &mut ir,
    )?;
    unresolved.features = unresolved
        .features
        .checked_sub(composite.transferred)
        .ok_or_else(|| {
            CodecError::malformed("Inventor Composite projection exceeds unresolved feature count")
        })?;
    let composite_losses = composite.losses;
    if !composite_links.is_empty() {
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
            .set_arena(ctx, "composite_member_links", &composite_links)?;
    }
    if delivered >= DecodeDepth::Geometry {
        let links = crate::feature::body_selection::links(
            ctx,
            &container.rse,
            &feature_inventory,
            &design_inventory,
        )?;
        let mut joined = crate::feature::body_selection::body_map(
            &feature_inventory.properties,
            &feature_inventory.kernel_entity_links,
            &links,
            &active_carrier,
            &body_keys,
            &ir.geometry,
            &crate::feature::body_selection::Scope {
                metadata: &segment_meta,
                roots: &design_inventory.model_roots,
            },
        );
        joined.extend(composite.bodies);
        crate::feature::body_selection::project(&mut ir, &joined, &feature_inventory);
        if !links.is_empty() {
            ir.native
                .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
                .set_arena(ctx, "body_wrapper_links", &links)?;
        }
        if delivered >= DecodeDepth::Design {
            if let ActiveCarrierState::Selected(carrier) = &container.rse.active_carrier {
                let history = crate::feature::history_states::project(
                    ctx,
                    carrier,
                    &history_read,
                    &crate::feature::history_states::Inputs {
                        features: &feature_inventory.features,
                        model_features: &ir.model.features,
                        kernel_links: &feature_inventory.kernel_entity_links,
                        wrapper_links: &links,
                        properties: &feature_inventory.properties,
                    },
                )?;
                if !history.records.is_empty() {
                    ir.native
                        .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
                        .set_arena(ctx, "feature_history_states", &history.records)?;
                }
                losses.extend(crate::feature::history_states::losses(&history));
                if let Some(unread) = history.unread {
                    let why = crate::feature::history_states::unread_note(unread.reason);
                    // A carrier given up whole already holds the chain bytes;
                    // the note records the history the chain held.
                    if given_up_carrier.is_some() {
                        losses.push(why);
                    } else {
                        let cause = match unread.reason {
                            crate::feature::history_states::Unread::Unclosed => Cause::Unreadable,
                            crate::feature::history_states::Unread::Bounded => Cause::Undecoded,
                        };
                        let chain =
                            carrier.bytes.window().get(unread.start..).ok_or_else(|| {
                                CodecError::malformed(
                                    "Inventor history chain start is out of bounds",
                                )
                            })?;
                        let part = crate::rse::bulk_record_part(
                            "history-chain",
                            (&carrier.segment_token, carrier.record_ordinal),
                            carrier.stream_offset(unread.start)?,
                            chain,
                            ArenaClass::Design,
                        )
                        .costing("the features have no history states");
                        history_chain = Some((
                            part,
                            cause,
                            CodecError::malformed(format_args!("{}: {}", why.code, why.message)),
                        ));
                    }
                }
                feature_history_states = history.records;
                ir.model.history_bodies = history.bodies.bodies;
                decoded_states = history.bodies.states;
                crate::feature::body_selection::name_states(
                    &mut decoded_states,
                    &joined,
                    &feature_inventory,
                );
            }
            losses.extend(crate::feature::body_selection::native_losses(
                &ir.model, &joined,
            ));
        }
    }
    ir.set_native_unknowns(ctx, "inventor", &[])?;
    let geometry_transferred = !(ir.geometry.surfaces.is_empty()
        && ir.geometry.points.is_empty()
        && ir.geometry.faces.is_empty());
    let body_ids = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    if geometry_transferred {
        for product in &mut ir.model.product_definitions {
            product.bodies.clone_from(&body_ids);
        }
    } else if matches!(
        &container.rse.active_carrier,
        ActiveCarrierState::Selected(_)
    ) && geometry_failure.is_none()
    {
        geometry_failure = Some(if empty_kernel_stream {
            "the part has no bodies: its active kernel carrier holds no model records".to_owned()
        } else {
            "the active kernel carrier decoded no surfaces, points, or faces".to_owned()
        });
    }
    let presentation_projection = crate::presentation::project_bindings(
        ctx,
        &presentation_inventory,
        &ir.model.appearances,
        &body_ids,
        &face_keys,
    )?;
    let face_color_appearance_count = presentation_projection.appearances.len();
    let projected_colors = presentation_projection
        .appearances
        .iter()
        .filter_map(|appearance| Some((appearance.id.clone(), appearance.base_color?)))
        .collect::<HashMap<_, _>>();
    ir.model
        .appearances
        .extend(presentation_projection.appearances);
    ir.model.appearance_bindings = presentation_projection.bindings;
    let face_colors = ir
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            if let cadmpeg_ir::appearance::AppearanceTarget::Face(face) = &binding.target.entity {
                projected_colors
                    .get(&binding.appearance)
                    .copied()
                    .map(|color| (face.clone(), color))
            } else {
                None
            }
        })
        .collect::<HashMap<_, _>>();
    colour_faces(&mut ir.geometry, &mut decoded_states, &face_colors);
    // Read before `geometry_failure` is consumed by the loss message below.
    let carrier_read_no_geometry = geometry_failure.is_some();
    if let Some((part, cause, error)) = history_chain {
        // The decode retains the whole carrier it read no geometry out of.
        let part = if carrier_read_no_geometry {
            part.digest_only()
        } else {
            part
        };
        given_up.push(ctx.give_up_part(part, cause, error)?);
    }
    for (record, detail) in &protein_issue_records {
        losses.push(
            GivenUpRecord::new(
                cadmpeg_ir::ids::Identity::new(record)?,
                Cause::Undecoded,
                format_args!("a Protein native record does not admit: {detail}"),
            )
            .loss(),
        );
    }
    losses.extend(dialect_loss(&matched, &recovery));
    if let Some(kernel) = ir
        .source
        .as_ref()
        .and_then(SourceMeta::dialects)
        .and_then(|layers| {
            layers
                .iter()
                .find(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
        })
    {
        losses.extend(kernel_dialect_loss(kernel));
    }
    if !ctx.container_only()
        && !matches!(document_kind, DocumentKind::Assembly)
        && !geometry_transferred
    {
        let detail = match geometry_failure {
            Some(detail) => detail,
            None => match &container.rse.active_carrier {
                ActiveCarrierState::Selected(_) => {
                    "The typed active kernel carrier has not been transferred.".to_owned()
                }
                ActiveCarrierState::Unavailable(detail)
                | ActiveCarrierState::Unreadable { detail, .. } => {
                    format!("The active Inventor kernel carrier is unavailable: {detail}")
                }
                ActiveCarrierState::NotApplicable => {
                    "Inventor geometry is not available for this document kind.".to_owned()
                }
            },
        };
        // A carrier given up whole states the lost geometry in its give-up.
        match given_up_carrier {
            Some((part, record)) => {
                if issue_records.insert(record) {
                    given_up.push(ctx.give_up_part(
                        part,
                        carrier_cause,
                        CodecError::Malformed(detail),
                    )?);
                    carrier_given_up = true;
                }
            }
            None => losses.push(InventorLossCode::GeometryKernelCarrierNotTransferred.note(detail)),
        }
    }
    if !ctx.container_only() {
        if let (ActiveCarrierState::Selected(carrier), false) =
            (&container.rse.active_carrier, carrier_given_up)
        {
            for (offset, record) in &kernel_withheld {
                given_up.push(ctx.give_up_part(
                    withheld_part(carrier, *offset, record),
                    record.cause,
                    record.error(),
                )?);
            }
        }
        if kernel_stats.unknown_surface_faces() != 0 {
            losses.push(
                InventorLossCode::GeometryProceduralSurfaceNotTransferred.note(format!(
                    "{} face(s) use procedural surfaces without a decoded carrier.",
                    kernel_stats.unknown_surface_faces()
                )),
            );
        }
        for face in &kernel_unbounded_faces {
            losses.push(InventorLossCode::TopologyFaceBoundaryUnstated.note(format!(
                "ASM face record {} states no loop on a {}, which no face covers whole: face {} is stated partial.",
                face.record_index,
                face.surface_kind,
                face.face.as_str()
            )));
        }
        if !segment_pairs.is_empty() {
            losses.push(InventorLossCode::RseSegmentPairUntyped.note(format!(
                "Retained {} structurally paired RSe segment(s) without record semantics.",
                segment_pairs.len()
            )));
        }
        give_up_issue_records(
            ctx,
            container,
            &assembly_inventory.issues,
            ("assembly", ArenaClass::Model),
            (&mut issue_records, &mut given_up),
        )?;
        give_up_issue_records(
            ctx,
            container,
            &presentation_inventory.issues,
            ("presentation", ArenaClass::Model),
            (&mut issue_records, &mut given_up),
        )?;
        give_up_issue_records(
            ctx,
            container,
            &design_inventory.issues,
            ("design", ArenaClass::Design),
            (&mut issue_records, &mut given_up),
        )?;
        give_up_issue_records(
            ctx,
            container,
            &sketch_inventory.issues,
            ("sketch", ArenaClass::Design),
            (&mut issue_records, &mut given_up),
        )?;
        let unnamed_labels = feature_inventory
            .labels
            .iter()
            .filter(|label| label.name().is_none())
            .count();
        if unnamed_labels != 0 {
            losses.push(InventorLossCode::FeatureLabelUnusable.note(format!(
                "{unnamed_labels} optional feature label(s) have no display name; their exact native records are retained."
            )));
        }
        let replaced_names = feature_inventory
            .labels
            .iter()
            .filter(|label| label.name_replaced())
            .count();
        if replaced_names != 0 {
            losses.push(InventorLossCode::FeatureLabelNameReplaced.note(format!(
                "{replaced_names} feature label name(s) store unpaired UTF-16 surrogates; their neutral names replace each with U+FFFD, and their exact native code units are retained."
            )));
        }
        // A pattern whose suppression operand fails its check is kept.
        let (kept, refused): (Vec<_>, Vec<_>) =
            feature_inventory.issues.iter().partition(|issue| {
                feature_inventory.pattern_features.iter().any(|pattern| {
                    pattern.identity.segment_token == issue.segment_token
                        && pattern.identity.record_ordinal == issue.record_ordinal
                })
            });
        if !kept.is_empty() {
            losses.push(InventorLossCode::FeatureRecordMalformed.note(format!(
                "{} typed Inventor feature record(s) could not be parsed exactly.",
                kept.len()
            )));
        }
        give_up_issue_records(
            ctx,
            container,
            refused,
            ("feature", ArenaClass::Design),
            (&mut issue_records, &mut given_up),
        )?;
        give_up_annotation_records(
            ctx,
            container,
            (&malformed_annotations, &needs_parameter),
            (&mut issue_records, &mut given_up),
            &mut losses,
        )?;
        if unresolved.features != 0 {
            losses.push(InventorLossCode::FeatureOperationGraphOpen.note(format!(
                "{} typed Inventor feature record(s) have an operation graph that is not closed: each is retained as a native record, has no history placement, has a chamfer edge group that is unresolved, or is typed with a native profile or section selection.",
                unresolved.features
            )));
        }
        losses.extend(stored_body_losses);
        losses.extend(crate::feature::derived::native_source_loss(
            &feature_inventory.labels,
        ));
        losses.extend(crate::feature::tagging::native_losses(&feature_inventory));
        losses.extend(crate::feature::coil::native_losses(
            &feature_inventory,
            &ir.model.features,
        ));
        losses.extend(composite_losses);
        losses.extend(crate::feature::unrecognized_class_losses(
            &ir.model.features,
        ));
        losses.extend(crate::feature::stitch::losses(
            ir.model
                .features
                .iter()
                .filter(|feature| {
                    feature.source_tag.as_deref() == Some(crate::feature::stitch::TAG)
                })
                .count(),
        ));
        losses.extend(crate::feature::sheet_metal::native_kind_losses(
            &feature_inventory,
            &ir.model.features,
        ));
        losses.extend(crate::feature::edge_selection::native_losses(
            &feature_inventory,
            &ir.model.features,
        ));
        losses.extend(crate::feature::face_selection::native_losses(
            &ir.model.features,
        ));
        losses.extend(crate::feature::face_selection::assumption_losses(
            &ir.model.features,
            &feature_inventory,
        ));
        losses.extend(crate::feature::sheet_metal::retained_control_losses(
            ir.model.features.iter(),
        ));
        losses.extend(unresolved.extrusion_losses);
        losses.extend(unresolved.revolve_losses);
        losses.extend(crate::feature::delete_face::native_losses(
            &feature_inventory,
            &ir.model.features,
        ));
        losses.extend(unresolved.edge_treatment_losses);
        losses.extend(unresolved.loft_losses);
        losses.extend(unresolved.split_losses);
        losses.extend(unresolved.thicken_losses);
        losses.extend(unresolved.plate_losses);
        losses.extend(unresolved.mirror_losses);
        losses.extend(crate::feature::work_plane::native_losses(
            &feature_inventory.work_planes,
            &ir.model.features,
        ));
        if delivered >= DecodeDepth::Design {
            losses.extend(crate::feature::pattern::native_losses(
                feature_inventory.pattern_features.iter(),
                &ir.model.features,
            ));
        }
        losses.extend(crate::feature::loft_sweep::retained_control_losses(
            ir.model.features.iter().map(|feature| {
                (
                    feature.evaluation.definition(),
                    feature.native_ref.as_deref(),
                )
            }),
            &unresolved.refused_surface_lofts,
        ));
        let partial_threads = ir
            .model
            .features
            .iter()
            .filter(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::CosmeticThread { .. }
                    )
                )
            })
            .count();
        if let Some(loss) = crate::feature::thread::partial_loss(partial_threads) {
            losses.push(loss);
        }
        losses.extend(crate::feature::thicken_operand_losses(
            &ir.model.features,
            &crate::feature::thicken_source_ids(&feature_inventory),
        ));
        let partial_shells = ir
            .model
            .features
            .iter()
            .filter(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Shell { .. }
                    )
                )
            })
            .count();
        if partial_shells != 0 {
            losses.push(InventorLossCode::FeatureShellDefinitionPartial.note(format!(
                "Transferred {partial_shells} Inventor shell operation(s) with wall thickness and removed-face selections; construction controls remain native."
            )));
        }
        losses.extend(unresolved.profile_losses);
        if unresolved.profile_bodies != 0 {
            losses.push(InventorLossCode::FeatureProfileBodyUndecoded.note(format!(
                "{} PmDc profile body record(s) did not decode; extrusions whose profiles select them keep their native profile selection.",
                unresolved.profile_bodies
            )));
        }
        if unresolved.feature_states != 0 {
            losses.push(InventorLossCode::FeatureStateUnresolved.note(format!(
                "Transferred {} Inventor operation(s) with unresolved suppression and dependency state.",
                unresolved.feature_states
            )));
        }
        if unresolved.parameter_scopes != 0 {
            losses.push(InventorLossCode::ParameterScopeUnresolved.note(format!(
                "Retained {} Inventor parameter record(s) whose parameter table has no established owning model.",
                unresolved.parameter_scopes
            )));
        }
        if unresolved.parameters != 0 {
            losses.push(InventorLossCode::ParameterGraphOpen.note(format!(
                "Retained {} Inventor parameter record(s) whose unit or expression graph is not closed.",
                unresolved.parameters
            )));
        }
        if unresolved.value_only_parameters != 0 {
            losses.push(InventorLossCode::ParameterExpressionNative.note(format!(
                "Transferred {} Inventor parameter(s) with their exact value and no expression text: the unit record of the root literal has an undefined base-unit type and stays native.",
                unresolved.value_only_parameters
            )));
        }
        losses.extend(crate::design::layout_assumed_loss(
            &unresolved.layout_assumed_parameters,
        ));
        losses.extend(crate::design::bounded_expression_loss(
            unresolved.bounded_parameters,
        ));
        losses.extend(crate::sketch::graph_loss(
            unresolved.sketches,
            unresolved.sketch_entities,
            unresolved.sketch_constraints,
        ));
        if unresolved.sketch_construction != 0 {
            losses.push(InventorLossCode::SketchConstructionUnresolved.note(format!(
                "{} Inventor sketch curve(s) carry entity flags whose construction meaning is unresolved; they project as geometry and stay out of the sketch profile tables.",
                unresolved.sketch_construction
            )));
        }
        losses.extend(crate::sketch::axis_distance_loss(
            unresolved.sketch_axis_distances,
        ));
        losses.extend(crate::sketch::carrier_conflict_loss(
            unresolved.sketch_carrier_conflicts,
        ));
        losses.extend(crate::sketch::degenerate_line_loss(
            unresolved.sketch_degenerate_lines,
        ));
        losses.extend(crate::sketch::rounded_line_loss(
            unresolved.sketch_rounded_lines,
        ));
        losses.extend(crate::sketch::point_tail::paired_association_loss(
            &sketch_inventory,
        ));
        losses.extend(crate::sketch::curve::native_losses(
            &sketch_inventory.entities,
        ));
        losses.extend(crate::sketch::curve::relation_loss(&sketch_inventory));
        losses.extend(crate::sketch::curve::member_losses(
            &sketch_inventory,
            &container.rse,
        ));
        losses.extend(crate::sketch::polygon_control_vertex_loss(
            &sketch_inventory.constraints,
        ));
        losses.extend(crate::sketch::circle_parameter::loss(
            &sketch_inventory.constraints,
        ));
        losses.extend(crate::sketch::spline::point_relation_loss(
            &sketch_inventory.constraints,
        ));
        // A property set whose stream does not read is given up; an issue
        // of a set that reads keeps the set.
        let kept = property_set_issues.len() - malformed_property_set_count;
        if kept != 0 {
            losses.push(
                InventorLossCode::PropertySetStreamMalformed
                    .note(format!("{kept} OLE property-set stream(s) are malformed.")),
            );
        }
        if metadata.unmapped != 0 {
            losses.push(InventorLossCode::MetadataPropertyUnmapped.note(format!(
                "Retained {} property value(s) without neutral metadata mapping.",
                metadata.unmapped
            )));
        }
        match &container.protein {
            ProteinState::Package(_) => {
                for rejection in &protein_rejections {
                    losses.push(
                        GivenUpRecord::new(
                            cadmpeg_ir::ids::Identity::new(rejection.asset_id())?,
                            Cause::Undecoded,
                            format_args!(
                                "a Protein asset record does not decode: {}",
                                rejection.detail()
                            ),
                        )
                        .loss(),
                    );
                }
                if ir.model.appearances.is_empty() {
                    if !protein_members_unread {
                        losses
                            .push(InventorLossCode::ProteinAppearanceAbsent.note(
                                "The Protein package contains no decoded appearance assets.",
                            ));
                    }
                } else if presentation_projection.unresolved_defaults != 0 {
                    losses.push(InventorLossCode::AppearanceDefaultUnresolved.note(format!(
                        "Could not resolve {} PmApp document-default appearance assignment(s).",
                        presentation_projection.unresolved_defaults
                    )));
                }
                if !material_catalog.duplicate_guids.is_empty() {
                    losses.push(InventorLossCode::ProteinGuidAmbiguous.note(format!(
                        "The Protein catalog contains {} duplicate asset GUID(s); ambiguous texture joins were refused.",
                        material_catalog.duplicate_guids.len()
                    )));
                }
                if material_catalog.untyped_distance_properties != 0 {
                    losses.push(InventorLossCode::MaterialDistanceUnitUntyped.note(format!(
                        "{} Protein texture Distance property value(s) retain an untyped unit tag; their typed texture carriers were omitted.",
                        material_catalog.untyped_distance_properties
                    )));
                }
            }
            ProteinState::Absent | ProteinState::Empty { .. } | ProteinState::Malformed { .. } => {}
        }
        for (cause, count) in &presentation_projection.unresolved_face_overrides {
            losses.push(
                InventorLossCode::AppearanceFaceOverrideUnresolved.note(format!(
                    "Could not resolve {count} PmGraphics face appearance override(s): {}.",
                    cause.description()
                )),
            );
        }
        // A rejected part of a record the document keeps has no record id
        // to give up.
        let kept = ufrx_issue_count - ufrx_issue_records.len();
        if kept != 0 {
            losses.push(
                InventorLossCode::UfrxTableMalformed
                    .note(format!("Skipped {kept} invalid UFRxDoc native record(s).")),
            );
        }
        for (record, detail) in &ufrx_issue_records {
            losses.push(
                GivenUpRecord::new(
                    cadmpeg_ir::ids::Identity::new(record)?,
                    Cause::Undecoded,
                    format_args!("a UFRxDoc native record does not admit: {detail}"),
                )
                .loss(),
            );
        }
        match &container.ufrx {
            UfrxState::Parsed(_) if matches!(document_kind, DocumentKind::Assembly) => {
                if !external_references.is_empty() {
                    losses.push(InventorLossCode::AssemblyComponentExternal.note(format!(
                        "Retained {} unresolved external component reference(s).",
                        external_references.len()
                    )));
                }
                losses.extend(nested_visibility_loss(assembly_projection.nested_hidden));
                // An occurrence given up below has its one loss there.
                let mut kept = assembly_projection
                    .unresolved_placements
                    .iter()
                    .map(|(cause, count)| (*cause, count.get()))
                    .collect::<BTreeMap<_, _>>();
                for (_, cause) in &assembly_projection.unbuilt_placements {
                    if let Some(count) = kept.get_mut(&cause.unresolved()) {
                        if let Some(left) = count.checked_sub(1) {
                            *count = left;
                        }
                    }
                }
                for (cause, count) in kept.into_iter().filter(|(_, count)| *count != 0) {
                    losses.push(
                        InventorLossCode::AssemblyPlacementNotTransferred.note(format!(
                            "Could not transfer {count} assembly occurrence placement(s): {}.",
                            cause.description()
                        )),
                    );
                }
                let placed = assembly_placements
                    .iter()
                    .map(|placement| placement.occurrence_id)
                    .collect::<std::collections::HashSet<_>>();
                let mut rejected = Vec::new();
                for placement in &assembly_inventory.placements {
                    if placed.contains(&placement.occurrence_id) {
                        continue;
                    }
                    let Some(frame) = rse_record_frame(
                        container,
                        &placement.segment_token,
                        placement.record_ordinal,
                    ) else {
                        continue;
                    };
                    let part = rse_record_part(frame, ArenaClass::Model);
                    rejected.push((placement.occurrence_id, part.id()?.into()));
                }
                losses.extend(crate::assembly::unbuilt_losses(
                    &assembly_projection.unbuilt_placements,
                    &rejected,
                )?);
            }
            UfrxState::Absent
            | UfrxState::Malformed { .. }
            | UfrxState::Unsupported { .. }
            | UfrxState::Parsed(_) => {}
        }
    }
    let preview_asset_count = ir.model.assets.len();
    let mut source_fidelity = SourceFidelity::for_root(root);
    let mut annotations = AnnotationBuilder::new();
    for record in kernel_annotations {
        admit_kernel_annotation(&mut annotations, &record)?;
    }
    source_fidelity.annotations = annotations.build();
    if let ActiveCarrierState::Selected(carrier) = &container.rse.active_carrier {
        // Retention is keyed on the outcome, not on a version band: a carrier
        // this decode read no geometry out of keeps its bytes verbatim, whatever
        // its save format declared.
        if carrier_read_no_geometry && !carrier_given_up {
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(carrier.bytes.window().len()),
                "retain Inventor kernel carrier that read no geometry",
            )?;
            let data = carrier.bytes.window().to_vec();
            let id = carrier_unknown_id(carrier);
            source_fidelity.retain_unknown_records(
                format!("RSeStorage/B{}:expanded", carrier.segment_token),
                [UnknownRecord::retained(
                    id.clone(),
                    carrier.carrier_offset,
                    data,
                    vec![active_carrier.id().to_owned()],
                )],
            )?;
            if !empty_kernel_stream {
                source_fidelity
                    .mark_untyped(id.as_str(), [cadmpeg_core::decode::ArenaClass::Model])?;
            }
        }
    }
    retain_kernel_unknowns(ctx, &mut ir, &mut source_fidelity, kernel_unknowns)?;
    let kernel_unknown_record_count = ir.native_unknowns("inventor")?.len();
    for part in given_up {
        losses.push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
    }
    let appearance_binding_count = ir.model.appearance_bindings.len();
    let transferred_occurrence_count = ir.model.occurrences.len();
    let design_parameter_count = ir.model.parameters.len();
    let transferred_sketch_count = ir.model.sketches.len();
    let transferred_sketch_entity_count = ir.model.sketch_entities.len();
    let transferred_sketch_constraint_count = ir.model.sketch_constraints.len();
    let transferred_feature_count = ir.model.features.len();
    let transferred_feature_result_count = ir.model.feature_result_topologies.len();
    let coverage_entries = [
        (crate::coverage::RSE_STORAGE_BANDS, storage_bands.len()),
        (crate::coverage::RSE_DATABASES, databases.len()),
        (
            crate::coverage::RSE_REGISTRY_ENTRIES,
            segment_registry.len(),
        ),
        (crate::coverage::RSE_REVISIONS, revisions.len()),
        (crate::coverage::RSE_SEGMENT_PAIRS, segment_pairs.len()),
        (crate::coverage::RSE_SEGMENT_META, segment_meta.len()),
        (crate::coverage::RSE_META_TYPES, meta_types.len()),
        (
            crate::coverage::RSE_SEGMENT_META_ISSUES,
            segment_meta_issues.len(),
        ),
        (crate::coverage::RSE_SEGMENT_BULK, segment_bulk.len()),
        (crate::coverage::RSE_RECORDS, rse_records.len()),
        (
            crate::coverage::RSE_SEGMENT_BULK_ISSUES,
            segment_bulk_issues.len(),
        ),
        (crate::coverage::PROPERTY_SETS, property_sets.len()),
        (crate::coverage::PROPERTIES, properties.len()),
        (crate::coverage::PREVIEW_ASSETS, preview_asset_count),
        (
            crate::coverage::PROTEIN_ENTRIES,
            protein_state.entries().len(),
        ),
        (crate::coverage::PROTEIN_ASSETS, protein_assets.len()),
        (
            crate::coverage::PROTEIN_REJECTIONS,
            protein_rejections.len(),
        ),
        (
            crate::coverage::PROTEIN_APPEARANCES,
            protein_appearance_count,
        ),
        (
            crate::coverage::APPEARANCE_BINDINGS_TRANSFERRED,
            appearance_binding_count,
        ),
        (
            crate::coverage::PM_APP_DEFAULT_STYLES,
            pm_app_default_styles.len(),
        ),
        (
            crate::coverage::PM_APP_RENDERING_STYLES,
            pm_app_rendering_styles.len(),
        ),
        (crate::coverage::PM_GRAPHICS_FACES, pm_graphics_faces.len()),
        (
            crate::coverage::PM_GRAPHICS_STYLE_COLLECTIONS,
            pm_graphics_style_collections.len(),
        ),
        (
            crate::coverage::PM_GRAPHICS_PRIMARY_COLOR_STYLES,
            pm_graphics_primary_color_styles.len(),
        ),
        (
            crate::coverage::FACE_COLOR_APPEARANCES,
            face_color_appearance_count,
        ),
        (
            crate::coverage::PRESENTATION_RECORD_ISSUES,
            presentation_inventory.issues.len(),
        ),
        (
            crate::coverage::PM_DC_PARAMETERS,
            design_inventory.parameters.len(),
        ),
        (
            crate::coverage::PM_DC_EXPRESSIONS,
            design_inventory.expressions.len(),
        ),
        (crate::coverage::PM_DC_UNITS, design_inventory.units.len()),
        (
            crate::coverage::PM_DC_PARAMETER_TABLES,
            design_inventory.parameter_tables.len(),
        ),
        (
            crate::coverage::PM_DC_MODEL_ROOTS,
            design_inventory.model_roots.len(),
        ),
        (
            crate::coverage::DESIGN_PARAMETERS_TRANSFERRED,
            design_parameter_count,
        ),
        (
            crate::coverage::DESIGN_RECORD_ISSUES,
            design_inventory.issues.len(),
        ),
        (
            crate::coverage::PM_DC_SKETCHES,
            sketch_inventory.sketches.len(),
        ),
        (
            crate::coverage::PM_DC_SKETCH_ENTITIES,
            sketch_inventory.entities.len(),
        ),
        (
            crate::coverage::PM_DC_TRANSFORMS,
            sketch_inventory.transforms.len(),
        ),
        (
            crate::coverage::PM_DC_DIRECTIONS,
            sketch_inventory.directions.len(),
        ),
        (
            crate::coverage::PM_DC_SKETCH_CONSTRAINTS,
            sketch_inventory.constraints.len(),
        ),
        (
            crate::coverage::SKETCH_RECORD_ISSUES,
            sketch_inventory.issues.len(),
        ),
        (
            crate::coverage::PM_DC_FEATURES,
            feature_inventory.features.len(),
        ),
        (
            crate::coverage::PM_DC_PATTERN_FEATURES,
            feature_inventory.pattern_features.len(),
        ),
        (
            crate::coverage::PM_DC_FEATURE_TERMINATORS,
            feature_inventory.terminators.len(),
        ),
        (
            crate::coverage::PM_DC_FEATURE_PROPERTIES,
            feature_inventory.properties.len(),
        ),
        (
            crate::coverage::PM_DC_FEATURE_LABELS,
            feature_inventory.labels.len(),
        ),
        (
            crate::coverage::PM_DC_ENTITY_STYLE_LINKS,
            feature_inventory.entity_style_links.len(),
        ),
        (
            crate::coverage::FEATURE_RECORD_ISSUES,
            feature_inventory.issues.len(),
        ),
        (
            crate::coverage::FEATURES_TRANSFERRED,
            transferred_feature_count,
        ),
        (
            crate::coverage::FEATURE_RESULT_TOPOLOGIES_TRANSFERRED,
            transferred_feature_result_count,
        ),
        (
            crate::coverage::SKETCHES_TRANSFERRED,
            transferred_sketch_count,
        ),
        (
            crate::coverage::SKETCH_ENTITIES_TRANSFERRED,
            transferred_sketch_entity_count,
        ),
        (
            crate::coverage::SKETCH_CONSTRAINTS_TRANSFERRED,
            transferred_sketch_constraint_count,
        ),
        (
            crate::coverage::EXTERNAL_REFERENCES,
            external_references.len(),
        ),
        (
            crate::coverage::EMBEDDED_REFERENCES,
            embedded_references.len(),
        ),
        (crate::coverage::UFRX_MODEL_STATES, ufrx_model_states.len()),
        (crate::coverage::UFRX_OCCURRENCES, ufrx_occurrences.len()),
        (
            crate::coverage::ASSEMBLY_OCCURRENCES,
            assembly_occurrences.len(),
        ),
        (
            crate::coverage::ASSEMBLY_PLACEMENTS,
            assembly_placements.len(),
        ),
        (
            crate::coverage::ASSEMBLY_OCCURRENCES_TRANSFERRED,
            transferred_occurrence_count,
        ),
        (
            crate::coverage::ASSEMBLY_RECORD_ISSUES,
            assembly_inventory.issues.len(),
        ),
        (
            crate::coverage::ACTIVE_KERNEL_CARRIERS,
            usize::from(matches!(
                &container.rse.active_carrier,
                ActiveCarrierState::Selected(_)
            )),
        ),
        (
            crate::coverage::KERNEL_UNKNOWN_SURFACE_FACES,
            kernel_stats.unknown_surface_faces(),
        ),
    ];
    let sheet_metal_coverage = [
        (
            crate::coverage::PM_DC_SHEET_METAL_STYLES,
            feature_inventory.sheet_metal_styles.len(),
        ),
        (
            crate::coverage::PM_DC_SHEET_METAL_FLANGES,
            feature_inventory.sheet_metal_flanges.len(),
        ),
    ];
    let mut coverage: cadmpeg_ir::report::decode::Coverage = coverage_entries
        .into_iter()
        .chain(
            sheet_metal_coverage
                .into_iter()
                .filter(|(_, count)| *count != 0),
        )
        .collect();
    if ctx.container_only() {
        ir.geometry = Topology::default();
        ir.model = Model::default();
    }
    if let Some(root) =
        ir.state_root_definition(ctx, root_product_id(), ExternalDocument::default())?
    {
        if root.kind.is_none() {
            root.kind = match document_kind {
                DocumentKind::Part => Some(ProductDefinitionKind::Part),
                DocumentKind::Assembly => Some(ProductDefinitionKind::Assembly),
                _ => None,
            };
        }
    }
    if document_kind == DocumentKind::Part && ctx.depth() >= DecodeDepth::Design {
        project_layer(ctx, "inventor", &mut ir, &mut losses, |ir, losses| {
            crate::design_v11::project(
                ctx,
                ir,
                &feature_inventory,
                &sketch_inventory,
                crate::design_v11::ParameterRecords {
                    parameters: &design_inventory.parameters,
                    units: &design_inventory.units,
                    tables: &design_inventory.parameter_tables,
                },
                losses,
            )?;
            crate::design_v11::evaluations::project(
                ctx,
                ir,
                crate::design_v11::evaluations::Evidence {
                    active: if active_geometry {
                        history_read.active_state()
                    } else {
                        None
                    },
                    features: &feature_history_states,
                    states: &decoded_states,
                },
                &source_fidelity,
                &mut state_unknowns,
                losses,
            )
        })?;
    }
    let state_unknown_record_count = state_unknowns.len();
    retain_history_unknowns(
        ctx,
        &mut ir,
        &mut source_fidelity,
        &decoded_states,
        state_unknowns,
    )?;
    coverage.record(
        crate::coverage::KERNEL_UNKNOWN_RECORDS,
        kernel_unknown_record_count + state_unknown_record_count,
    );
    let mut body = DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::at(delivered, geometry_transferred),
        coverage,
        losses,
        notes: Vec::new(),
        transfer_ledger: TransferLedger::default(),
    };
    if document_kind == DocumentKind::Assembly {
        crate::design_v11::assembly::project(ctx, &mut ir, &mut body)?;
    }
    body.coverage.record(
        crate::coverage::DESIGN_UNPRODUCED_BODIES,
        crate::design_v11::unproduced_bodies(&ir),
    );
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

fn record_ordinal(ordinal: usize, what: &'static str) -> Result<u32, CodecError> {
    u32::try_from(ordinal)
        .map_err(|_| CodecError::malformed(format_args!("{what} {ordinal} exceeds u32")))
}

/// Design records whose neutral projection did not close, by family.
#[derive(Debug, Default)]
struct UnresolvedDesign {
    parameters: usize,
    parameter_scopes: usize,
    bounded_parameters: usize,
    value_only_parameters: usize,
    layout_assumed_parameters: std::collections::BTreeMap<[u8; 16], usize>,
    sketches: usize,
    sketch_entities: usize,
    sketch_constraints: usize,
    features: usize,
    feature_states: usize,
    profile_bodies: usize,
    profile_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    sketch_construction: usize,
    sketch_axis_distances: usize,
    sketch_carrier_conflicts: usize,
    sketch_degenerate_lines: usize,
    sketch_rounded_lines: usize,
    extrusion_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    revolve_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    edge_treatment_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    loft_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    refused_surface_lofts: std::collections::HashSet<String>,
    split_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    thicken_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    plate_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    mirror_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Projects the design inventories into the document's design arenas.
fn project_design(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    rse: &crate::rse::RseInventory<'_>,
    design: &crate::design::DesignInventory,
    sketch: &crate::sketch::SketchInventory,
    feature: &crate::feature::FeatureInventory,
    result_states: Option<&dyn crate::feature::history_views::ResultStates>,
) -> Result<UnresolvedDesign, CodecError> {
    let mut admitted_entities = 0_u64;
    let scopes = crate::design::parameter_scopes(design)?;
    let parameters =
        crate::design::project_parameters(ctx, design, &scopes, &mut admitted_entities)?;
    ir.model.parameters = parameters.parameters;
    let sketches = crate::sketch::project(ctx, sketch, &ir.model.parameters)?;
    ir.model.sketches = sketches.sketches;
    ir.model.sketch_entities = sketches.entities;
    ir.model.sketch_constraints = sketches.constraints;
    let profile_bodies = crate::feature::profile::decode_profile_bodies(ctx, rse)?;
    let features = crate::feature::project(
        ctx,
        feature,
        design,
        sketch,
        &ir.model.parameters,
        &crate::feature::ProjectedSketches {
            sketches: &ir.model.sketches,
            entities: &ir.model.sketch_entities,
            profile_bodies: &profile_bodies.bodies,
            result_model: Some(ir),
            result_states,
        },
    )?;
    let mut added_profiles = features.added_profiles;
    for sketch in &mut ir.model.sketches {
        if let Some(loops) = added_profiles.remove(sketch.id.as_str()) {
            sketch
                .profiles
                .edit(|profiles| profiles.extend(loops))
                .map_err(CodecError::malformed)?;
        }
    }
    ir.model.features = features.features;
    ir.model.feature_result_topologies = features.result_topologies;
    ir.model.feature_input_topologies = features.input_topologies;
    // Flat-pattern models follow every projected document feature.
    let first_model_ordinal = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(Some(0), |ordinal| ordinal.checked_add(1))
        .ok_or_else(|| CodecError::malformed("Inventor feature ordinal overflows"))?;
    let model_features = crate::design::scope::project_model_features(
        ctx,
        &design.model_roots,
        &design.brep_segments,
        first_model_ordinal,
    )?;
    ir.model.features.extend(model_features);
    Ok(UnresolvedDesign {
        parameters: parameters.unresolved,
        parameter_scopes: parameters.unresolved_scopes,
        bounded_parameters: parameters.bounded,
        value_only_parameters: parameters.value_only,
        layout_assumed_parameters: parameters.layout_assumed,
        sketches: sketches.unresolved_sketches,
        sketch_entities: sketches.unresolved_entities,
        sketch_constraints: sketches.unresolved_constraints,
        features: features.unresolved_features,
        feature_states: features.unresolved_states,
        profile_bodies: profile_bodies.undecoded,
        profile_losses: profile_bodies.bounded,
        sketch_construction: sketches.unresolved_construction,
        sketch_axis_distances: sketches.native_axis_distances,
        sketch_carrier_conflicts: sketches.native_carrier_conflicts,
        sketch_degenerate_lines: sketches.native_degenerate_lines,
        sketch_rounded_lines: sketches.native_rounded_lines,
        extrusion_losses: features.extrusion_losses,
        revolve_losses: features.revolve_losses,
        edge_treatment_losses: features.edge_treatment_losses,
        loft_losses: features.loft_losses,
        refused_surface_lofts: features.refused_surface_lofts,
        split_losses: features.split_losses,
        thicken_losses: features.thicken_losses,
        plate_losses: features.plate_losses,
        mirror_losses: features.mirror_losses,
    })
}

/// The root definition: the part or assembly the document holds at its top.
fn root_product_id() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "document", "product"),
        cadmpeg_ir::identity_key!("root"),
    )
}

fn project_root_product(
    ctx: &DecodeContext<'_>,
    document_kind: &DocumentKind,
    metadata: &MetadataProjection,
    entity_count: usize,
    admitted_entities: &mut u64,
) -> Result<ProductDefinition, CodecError> {
    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(entity_count + 1),
        admitted_entities,
        "admit Inventor root product",
    )?;
    let source_name = metadata.title.as_deref().map(str::to_owned);
    let label = metadata.title.as_deref().map(str::to_owned);
    let description = metadata.description.as_deref().map(str::to_owned);
    let part_number = metadata.part_number.as_deref().map(str::to_owned);
    let mut definition = ProductDefinition::root(root_product_id(), ExternalDocument::default());
    definition.kind = Some(if *document_kind == DocumentKind::Assembly {
        ProductDefinitionKind::Assembly
    } else {
        ProductDefinitionKind::Part
    });
    definition.source_name = source_name;
    definition.label = label;
    definition.description = description;
    definition.part_number = part_number;
    definition.bom_properties = cadmpeg_core::text::named_entries(
        "inventor:document:product#root",
        metadata.bom_properties.clone(),
    )?;
    Ok(definition)
}

/// Gives up, as an `rse-record` part of a typed record of `family`, the
/// framed record of each issue that no earlier issue in `named` names, with
/// the detail of its first issue. The record frames, and the decoder does not
/// tell a malformed payload from a form it does not read, so the give-up is
/// [`Cause::Undecoded`]. The part holds content of the class `holds`, its
/// bytes are the record's payload in the expanded bulk stream of its segment,
/// and its stub links to the `RSe` record that frames it.
fn give_up_issue_records<'a>(
    ctx: &DecodeContext<'_>,
    container: &InventorContainer<'_>,
    issues: impl IntoIterator<Item = &'a RecordIssue>,
    (family, holds): (&str, ArenaClass),
    (named, parts): (
        &mut std::collections::BTreeSet<(String, u32)>,
        &mut Vec<GivenUpPart>,
    ),
) -> Result<(), CodecError> {
    for issue in issues {
        let token = issue.segment_token.as_str();
        if !named.insert((token.to_owned(), issue.record_ordinal)) {
            continue;
        }
        let Some(frame) = rse_record_frame(container, token, issue.record_ordinal) else {
            continue;
        };
        let part = rse_record_part(frame, holds);
        parts.push(ctx.give_up_part(
            part,
            Cause::Undecoded,
            CodecError::malformed(format_args!(
                "a typed {family} record does not parse: {}",
                issue.detail
            )),
        )?);
    }
    Ok(())
}

/// Gives up each annotation record outside the established grammar that
/// `named` does not hold yet, and drops each annotation that a record takes
/// out of the document with the give-up of that record. An annotation of
/// `needs` is dropped with the parameter record it needs.
fn give_up_annotation_records(
    ctx: &DecodeContext<'_>,
    container: &InventorContainer<'_>,
    (malformed, needs): (
        &[crate::annotation::Malformed],
        &[crate::annotation::NeedsParameter],
    ),
    (named, parts): (
        &mut std::collections::BTreeSet<(String, u32)>,
        &mut Vec<GivenUpPart>,
    ),
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for record in malformed {
        let token = record.segment_token.as_str();
        let Some(frame) = rse_record_frame(container, token, record.record_ordinal) else {
            continue;
        };
        let part = rse_record_part(frame, ArenaClass::Model);
        if let Some(annotation) = &record.annotation {
            losses.push(dropped_with(annotation, part.id()?));
        }
        if named.insert((token.to_owned(), record.record_ordinal)) {
            parts.push(ctx.give_up_part(
                part,
                Cause::Undecoded,
                CodecError::malformed(format_args!(
                    "an annotation record is outside the established grammar: {}",
                    record.detail
                )),
            )?);
        }
    }
    for need in needs {
        let Some(frame) = rse_record_frame(container, need.segment_token.as_str(), need.parameter)
        else {
            continue;
        };
        let part = rse_record_part(frame, ArenaClass::Design);
        losses.push(dropped_with(&need.annotation, part.id()?));
    }
    Ok(())
}

/// The framed `RSe` record `frame` of the segment whose token is `key`, as an
/// `rse-record` part that holds content of the class `holds`.
fn rse_record_part<'s>(
    (key, frame): (
        &'s cadmpeg_ir::ids::IdentityKey,
        &'s crate::records::RseRecordFrame<'_>,
    ),
    holds: ArenaClass,
) -> Part<'s> {
    crate::rse::bulk_record_part(
        "rse-record",
        (key, frame.ordinal),
        frame.payload_offset,
        frame.payload.window(),
        holds,
    )
}

/// The framed `RSe` record `ordinal` of the segment whose token is `token`,
/// and that token as an identity key. A record that names no framed record
/// has no bytes to give up.
fn rse_record_frame<'s, 'a>(
    container: &'s InventorContainer<'a>,
    token: &str,
    ordinal: u32,
) -> Option<(
    &'s cadmpeg_ir::ids::IdentityKey,
    &'s crate::records::RseRecordFrame<'a>,
)> {
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.pair.token.as_str() == token)?;
    let crate::rse::SegmentBulkState::Framed(bulk) = &segment.bulk else {
        return None;
    };
    let crate::rse::RecordFrameState::Framed(table) = &bulk.records else {
        return None;
    };
    let frame = table
        .records
        .iter()
        .find(|record| record.ordinal == ordinal)?;
    Some((segment.pair.token.key(), frame))
}

/// The active kernel carrier as a `kernel-carrier` part of its `RSe` record, and
/// the segment token and ordinal of that record.
fn carrier_part<'a>(carrier: &crate::kernel::ActiveCarrier<'a>) -> (Part<'a>, (String, u32)) {
    let part = crate::rse::bulk_record_part(
        "kernel-carrier",
        (&carrier.segment_token, carrier.record_ordinal),
        carrier.carrier_offset,
        carrier.bytes.window(),
        ArenaClass::Model,
    );
    (
        part,
        (
            carrier.segment_token.as_str().to_owned(),
            carrier.record_ordinal,
        ),
    )
}

/// The topology record `record` that the kernel decode of `carrier` read and
/// left out, as a part over its bytes at `offset` in the bulk stream.
fn withheld_part<'r>(
    carrier: &crate::kernel::ActiveCarrier<'_>,
    offset: u64,
    record: &'r cadmpeg_asm::brep::given_up::WithheldRecord,
) -> Part<'r> {
    crate::rse::bulk_record_part(
        record.kind,
        (&carrier.segment_token, carrier.record_ordinal),
        offset,
        &record.bytes,
        ArenaClass::Model,
    )
    .dropping(record.dropped.iter().cloned())
    .marking(record.marked.iter().cloned())
}

fn carrier_unknown_id(carrier: &crate::kernel::ActiveCarrier<'_>) -> UnknownId {
    UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "kernel", "carrier"),
        carrier.segment_token.clone().dash(carrier.record_ordinal),
    )
}

/// Give each face of the current geometry and of the decoded history states
/// that holds no colour the colour bound to its id.
fn colour_faces(
    geometry: &mut Topology,
    states: &mut BTreeMap<u32, crate::feature::history_bodies::StateRecords>,
    colours: &HashMap<cadmpeg_ir::ids::FaceId, cadmpeg_ir::topology::Color>,
) {
    let state_faces = states
        .values_mut()
        .flat_map(|state| state.geometry.faces.iter_mut());
    for face in geometry.faces.iter_mut().chain(state_faces) {
        if face.color.is_none() {
            face.color = colours.get(&face.id).copied();
        }
    }
}

/// Retain the undecoded kernel records `records` in the document's unknown
/// arena.
fn retain_kernel_unknowns(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    source_fidelity: &mut SourceFidelity,
    records: Vec<UnknownRecord>,
) -> Result<(), CodecError> {
    if records.is_empty() {
        return Ok(());
    }
    // Topology-only vertex and edge records carry nothing the arenas lack.
    let untyped = records
        .iter()
        .filter(|record| {
            let id = record.id().as_str();
            !id.contains(":brep:tvertex#") && !id.contains(":brep:tedge#")
        })
        .map(|record| record.id().clone())
        .collect::<Vec<_>>();
    source_fidelity
        .attach_native_unknown_records(ir, "inventor", records, ctx)
        .map_err(|error| {
            CodecError::malformed(format_args!(
                "Inventor kernel unknown retention failed: {error}"
            ))
        })?;
    for id in untyped {
        source_fidelity.mark_untyped(id.as_str(), [cadmpeg_core::decode::ArenaClass::Model])?;
    }
    Ok(())
}

pub(crate) fn retain_history_unknowns(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    source_fidelity: &mut SourceFidelity,
    decoded_states: &BTreeMap<u32, crate::feature::history_bodies::StateRecords>,
    state_unknowns: Vec<(u32, UnknownRecord)>,
) -> Result<(), CodecError> {
    if state_unknowns.is_empty() {
        return Ok(());
    }
    let mut provenance = AnnotationBuilder::new();
    for (ordinal, record) in &state_unknowns {
        let located = decoded_states
            .get(ordinal)
            .into_iter()
            .flat_map(|state| &state.provenance)
            .filter(|located| located.id.as_str() == record.id().as_str());
        for located in located {
            admit_kernel_annotation(&mut provenance, located)?;
        }
    }
    source_fidelity
        .annotations
        .provenance
        .extend(provenance.build().provenance);
    let records = state_unknowns
        .into_iter()
        .map(|(_, record)| record)
        .collect();
    retain_kernel_unknowns(ctx, ir, source_fidelity, records)?;
    Ok(())
}

fn admit_kernel_annotation(
    annotations: &mut AnnotationBuilder,
    record: &cadmpeg_asm::brep::annotations::AnnotationRecord,
) -> Result<(), CodecError> {
    let name = format!("inventor:{}", record.stream);
    let name = cadmpeg_ir::StreamName::try_from(name).map_err(CodecError::malformed)?;
    let stream = StreamHandle::new(name);
    annotations.note(
        &record.id,
        &stream,
        record.offset,
        Some(record.tag.as_str()),
    );
    for field in &record.derived_fields {
        annotations
            .derived(&record.id, field)
            .map_err(CodecError::from)?;
    }
    Ok(())
}

fn project_property_set_issue(
    directory_id: u32,
    path: &str,
    detail: &str,
) -> PropertySetIssueRecord {
    PropertySetIssueRecord {
        id: format!("inventor:property:set-issue#{directory_id}"),
        path: path.to_owned(),
        directory_id,
        detail: detail.to_owned(),
    }
}

fn project_preview_asset(
    ctx: &DecodeContext<'_>,
    admitted_entities: &mut u64,
    ordinal: usize,
    native_id: &str,
    bytes: &[u8],
    media_type: &str,
) -> Result<Asset, CodecError> {
    ctx.admit_entities(
        *admitted_entities + 1,
        admitted_entities,
        "admit Inventor preview asset entity",
    )?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "retain Inventor preview asset",
    )?;
    let data = bytes.to_vec();
    Asset::try_new(
        AssetId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "document", "asset"),
            cadmpeg_ir::identity_key!("preview-").then(ordinal),
        ),
        Some("document preview".into()),
        Some(media_type.into()),
        AssetContent::Embedded {
            data: cadmpeg_ir::assets::AssetData::new(data)
                .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
        },
        Some(native_id.to_owned()),
    )
    .map_err(CodecError::Malformed)
}

fn project_protein_state(
    ctx: &DecodeContext<'_>,
    state: &ProteinState<'_>,
) -> Result<ProteinRecord, CodecError> {
    admit_native_items(ctx, 1)?;
    let id = "inventor:protein:state#root".to_owned();
    Ok(match state {
        ProteinState::Absent => ProteinRecord::Absent { id },
        ProteinState::Empty { stream } => ProteinRecord::Empty {
            id,
            directory_id: stream.directory_id(),
        },
        ProteinState::Malformed { stream, detail } => ProteinRecord::Malformed {
            id,
            directory_id: stream.directory_id(),
            detail: detail.to_owned(),
        },
        ProteinState::Package(package) => {
            let entries = package
                .archive
                .entries()
                .iter()
                .enumerate()
                .map(|(ordinal, entry)| -> Result<_, CodecError> {
                    admit_native_items(ctx, 1)?;
                    Ok(ProteinEntryRecord {
                        id: crate::protein::entry_id(ordinal),
                        ordinal: record_ordinal(ordinal, "Inventor Protein entry ordinal")?,
                        name: entry.name.clone(),
                        compression: entry.compression,
                        crc32: entry.crc32,
                        compressed_size: entry.compressed_size,
                        uncompressed_size: entry.uncompressed_size,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            ProteinRecord::Package {
                id,
                directory_id: package.stream.directory_id(),
                declared_len: package.declared_len,
                entries,
            }
        }
    })
}

struct ProteinNativeRecords {
    assets: Vec<ProteinAssetRecord>,
    rejections: Vec<ProteinRejectionRecord>,
    rejected: Rejections,
}

fn project_protein_records(
    ctx: &DecodeContext<'_>,
    instances: Vec<crate::protein::ProteinInstanceRecords>,
) -> Result<ProteinNativeRecords, CodecError> {
    let mut assets = Vec::new();
    let mut rejections = Vec::new();
    let mut rejected = Rejections::default();
    for instance in instances {
        if instance.records.is_empty() && instance.rejected.is_empty() {
            continue;
        }
        let entry_digest = sha256_hex(instance.entry_name.as_bytes());
        for asset in instance.records {
            let id = crate::native::protein::asset_id(&instance.entry_name, asset.ordinal);
            let entry_name = instance.entry_name.clone();
            let wire = ProteinAssetRecordWire {
                id,
                entry_name,
                ordinal: asset.ordinal,
                asset,
            };
            if let Some(record) = admit_protein_asset(ctx, wire, &mut rejected)? {
                assets.push(record);
            }
        }
        for rejection in instance.rejected {
            let id = format!(
                "inventor:protein:rejection#{}-{}",
                entry_digest, rejection.ordinal
            );
            let entry_name = instance.entry_name.clone();
            let wire = ProteinRejectionRecordWire {
                id,
                entry_name,
                ordinal: rejection.ordinal,
                detail: rejection.detail,
            };
            if let Some(record) = admit_protein_rejection(ctx, wire, &mut rejected)? {
                rejections.push(record);
            }
        }
    }
    Ok(ProteinNativeRecords {
        assets,
        rejections,
        rejected,
    })
}

/// The loss for occurrences hidden below a root occurrence, which only the
/// native records keep.
pub(crate) fn nested_visibility_loss(nested_hidden: usize) -> Option<LossNote> {
    (nested_hidden != 0).then(|| {
        InventorLossCode::AssemblyVisibilityNestedNative.note(format!(
            "Retained {nested_hidden} hidden occurrence(s) below a root occurrence in native records."
        ))
    })
}

fn project_ufrx_state(
    ctx: &DecodeContext<'_>,
    state: &UfrxState<'_>,
    rejected: &mut Rejections,
) -> Result<UfrxRecord, CodecError> {
    admit_native_items(ctx, 1)?;
    Ok(match state {
        UfrxState::Absent => UfrxRecord::Absent {
            id: "inventor:ufrx:state#root".to_owned(),
        },
        UfrxState::Malformed { stream, detail } => UfrxRecord::Malformed {
            id: "inventor:ufrx:state#root".to_owned(),
            directory_id: stream.directory_id(),
            detail: detail.to_owned(),
        },
        UfrxState::Unsupported {
            stream,
            schema,
            section_versions,
            source,
            detail,
        } => UfrxRecord::Unsupported {
            id: "inventor:ufrx:state#root".to_owned(),
            directory_id: stream.directory_id(),
            schema: *schema,
            section_versions: section_versions.clone(),
            tail_len: cadmpeg_core::decode::u64_from_index(source.window().len()),
            tail_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(source.window()),
            detail: detail.to_owned(),
        },
        UfrxState::Parsed(document) => {
            let mut model_states = Vec::new();
            for (ordinal, state) in document.model_states.iter().enumerate() {
                if let Some(record) = project_ufrx_model_state(ctx, ordinal, state, rejected)? {
                    model_states.push(record);
                }
            }
            let mut references = Vec::new();
            for (ordinal, reference) in document.references.iter().enumerate() {
                if let Some(record) =
                    project_ufrx_external_reference(ctx, ordinal, reference, rejected)?
                {
                    references.push(record);
                }
            }
            let mut embedded = Vec::new();
            for (ordinal, reference) in document.embedded_references.iter().enumerate() {
                if let Some(record) =
                    project_ufrx_embedded_reference(ctx, ordinal, reference, rejected)?
                {
                    embedded.push(record);
                }
            }
            let mut occurrences = Vec::new();
            for (ordinal, occurrence) in document.occurrences.iter().enumerate() {
                if let Some(record) = project_ufrx_occurrence(ctx, ordinal, occurrence, rejected)? {
                    occurrences.push(record);
                }
            }
            let representation = document
                .representation
                .as_ref()
                .map(|state| project_ufrx_representation(ctx, state, rejected))
                .transpose()?
                .flatten();
            UfrxRecord::ParsedPrefix(Box::new(UfrxParsedPrefix {
                id: "inventor:ufrx:state#root".to_owned(),
                directory_id: document.stream.directory_id(),
                schema: document.schema,
                section_versions: { document.section_versions.clone() },
                original_file_name: document.original_file_name.clone(),
                caption: document.caption.clone(),
                representation,
                model_states,
                levels_of_detail: project_ufrx_levels_of_detail(&document.levels_of_detail),
                external_references: references,
                embedded_references: embedded,
                occurrences,
                tail_len: cadmpeg_core::decode::u64_from_index(
                    document.unparsed_tail.window().len(),
                ),
                tail_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(
                    document.unparsed_tail.window(),
                ),
            }))
        }
    })
}

fn project_ufrx_levels_of_detail(
    levels: &[crate::external_reference::UfrxLevelOfDetail],
) -> Vec<crate::native::ufrx::UfrxLevelOfDetailRecord> {
    use crate::external_reference::UfrxLevelOfDetailMemberKind;
    use crate::native::ufrx::{UfrxLevelOfDetailMemberRecord, UfrxLevelOfDetailRecord};
    levels
        .iter()
        .map(|level| UfrxLevelOfDetailRecord {
            prefix: level.prefix,
            name: level.name.clone(),
            state: level.state,
            members: level
                .members
                .iter()
                .map(|member| {
                    let (kind, nested_state, nested_level_of_detail) = match &member.kind {
                        UfrxLevelOfDetailMemberKind::Listed => (1, None, None),
                        UfrxLevelOfDetailMemberKind::Nested {
                            state,
                            level_of_detail,
                        } => (2, Some(*state), Some(level_of_detail.clone())),
                    };
                    UfrxLevelOfDetailMemberRecord {
                        occurrence: member.occurrence.clone(),
                        kind,
                        nested_state,
                        nested_level_of_detail,
                    }
                })
                .collect(),
        })
        .collect()
}

fn project_ufrx_model_state(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    state: &crate::external_reference::UfrxModelState<'_>,
    rejected: &mut Rejections,
) -> Result<Option<UfrxModelStateRecord>, CodecError> {
    let parameters = state
        .parameters
        .iter()
        .map(|parameter| -> Result<_, CodecError> {
            ctx.charge_entities(1, "admit Inventor UFRx state parameter")?;
            Ok(UfrxModelStateParameterRecord {
                name: parameter.name.clone(),
                tag: parameter.tag,
                kind: parameter.kind,
                state: parameter.state,
                value: parameter.value.clone(),
                trailer: parameter.trailer,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let admitted = UfrxModelStateRecord::try_from(UfrxModelStateRecordWire {
        id: format!("inventor:ufrx:model-state#{ordinal}"),
        ordinal: record_ordinal(ordinal, "Inventor UFRx model-state ordinal")?,
        prefix: state.prefix,
        name: state.name.clone(),
        state: state.state,
        prefix_count: state.prefix_count,
        parameters,
        suffix_len: cadmpeg_core::decode::u64_from_index(state.suffix.window().len()),
        suffix_sha256: String::from(cadmpeg_ir::hash::digest::Sha256Digest::digest(
            state.suffix.window(),
        )),
    });
    let scope = format!("ufrx-model-state-{ordinal}");
    let record = format!("inventor:ufrx:model-state#{ordinal}");
    admit_ufrx_record(ctx, admitted, (&scope, Some(&record)), rejected)
}

fn project_ufrx_external_reference(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    reference: &crate::external_reference::InventorExternalReference,
    rejected: &mut Rejections,
) -> Result<Option<ExternalReferenceRecord>, CodecError> {
    let admitted = ExternalReferenceRecord::try_from(ExternalReferenceRecordWire {
        id: format!("inventor:ufrx:external-reference#{ordinal}"),
        ordinal: record_ordinal(ordinal, "Inventor UFRx external ordinal")?,
        path: reference.path.clone(),
        library_id: reference.library_id,
        library_name: reference.library_name.clone(),
        display_name: reference.display_name.clone(),
        state_groups: reference.state_groups.clone(),
        state: reference.state,
        document_id: Some(hex(&reference.document_id)),
        database_id: hex(&reference.database_id),
        reference_id: reference.reference_id,
        occurrence_count: reference.occurrence_count,
        version: reference.version,
        flags: reference.flags,
    });
    let scope = format!("ufrx-external-reference-{ordinal}");
    let record = format!("inventor:ufrx:external-reference#{ordinal}");
    admit_ufrx_record(ctx, admitted, (&scope, Some(&record)), rejected)
}

fn project_ufrx_embedded_reference(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    reference: &crate::external_reference::InventorEmbeddedReference<'_>,
    rejected: &mut Rejections,
) -> Result<Option<EmbeddedReferenceRecord>, CodecError> {
    let admitted = EmbeddedReferenceRecord::try_from(EmbeddedReferenceRecordWire {
        id: format!("inventor:ufrx:embedded-reference#{ordinal}"),
        ordinal: record_ordinal(ordinal, "Inventor UFRx embedded ordinal")?,
        value_0: reference.value_0,
        filetime: reference.filetime,
        value_1: reference.value_1,
        extended_value: reference.extended_value,
        value_2: reference.value_2,
        path: reference.path.clone(),
        library_id: reference.library_id,
        library_name: reference.library_name.clone(),
        state: reference.state,
        display_name: reference.display_name.clone(),
        state_values: reference.state_values.into(),
        record_len: cadmpeg_core::decode::u64_from_index(reference.source.window().len()),
        record_sha256: String::from(cadmpeg_ir::hash::digest::Sha256Digest::digest(
            reference.source.window(),
        )),
    });
    let scope = format!("ufrx-embedded-reference-{ordinal}");
    let record = format!("inventor:ufrx:embedded-reference#{ordinal}");
    admit_ufrx_record(ctx, admitted, (&scope, Some(&record)), rejected)
}

fn project_ufrx_occurrence(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    occurrence: &crate::external_reference::UfrxOccurrence<'_>,
    rejected: &mut Rejections,
) -> Result<Option<UfrxOccurrenceRecord>, CodecError> {
    let admitted = UfrxOccurrenceRecord::try_from(UfrxOccurrenceRecordWire {
        id: format!("inventor:ufrx:occurrence#{ordinal}"),
        ordinal: record_ordinal(ordinal, "Inventor UFRx occurrence ordinal")?,
        reference_id: occurrence.reference_id,
        occurrence_id: occurrence.occurrence_id,
        occurrence_key: occurrence.occurrence_key,
        header_value: occurrence.header_value,
        title: occurrence.title.as_deref().map(str::to_owned),
        header_padding_words: occurrence.header_padding_words,
        prototype_document_id: occurrence
            .prototype_document_id
            .map(cadmpeg_ir::native::bytes::NativeBytes::from),
        component_name: occurrence.component_name.clone(),
        record_len: cadmpeg_core::decode::u64_from_index(occurrence.source.window().len()),
        record_sha256: String::from(cadmpeg_ir::hash::digest::Sha256Digest::digest(
            occurrence.source.window(),
        )),
    });
    let scope = format!("ufrx-occurrence-{ordinal}");
    let record = format!("inventor:ufrx:occurrence#{ordinal}");
    admit_ufrx_record(ctx, admitted, (&scope, Some(&record)), rejected)
}

fn project_ufrx_representation(
    ctx: &DecodeContext<'_>,
    state: &crate::external_reference::UfrxRepresentationState,
    rejected: &mut Rejections,
) -> Result<Option<UfrxRepresentationRecord>, CodecError> {
    let design_views = state.design_views.as_ref().map(|views| {
        views
            .iter()
            .map(|(name, kind)| crate::native::ufrx::UfrxDesignView {
                name: name.to_owned(),
                kind: kind.to_owned(),
            })
            .collect()
    });
    let wire = UfrxRepresentationRecordWire {
        prefix: state.prefix,
        design_views,
        secondary_active_lod_state: state.secondary_active_lod_state,
        active_design_view: state.active_design_view.clone(),
        active_design_view_state: state.active_design_view_state,
    };
    admit_ufrx_record(
        ctx,
        UfrxRepresentationRecord::try_from(wire),
        ("ufrx-representation", None),
        rejected,
    )
}

fn admit_native_items(ctx: &DecodeContext<'_>, count: usize) -> Result<(), CodecError> {
    let count = cadmpeg_core::decode::u64_from_index(count);
    ctx.charge_entities(count, "admit Inventor native structural records")
}

fn version_record(version: VersionTuple) -> VersionTupleRecord {
    VersionTupleRecord {
        revision: version.revision,
        minor: version.minor,
        major: version.major,
        state: hex(&version.state),
    }
}

fn apply_kernel_header(
    ir: &mut CadIr,
    family: crate::kernel::KernelFamily,
    header: &cadmpeg_asm::kernel_header::KernelHeader,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    for field in header.unreadable_product_fields() {
        losses.push(
            InventorLossCode::KernelHeaderMetadataUnresolved.note(format!(
                "kernel header {field} is unreadable; independent geometry retained"
            )),
        );
    }
    if let Some(value) = header.linear {
        if let Some(linear) = cadmpeg_ir::scalar::PositiveLength::new(value * 10.0) {
            ir.tolerances.linear = linear;
        } else {
            losses.push(InventorLossCode::KernelHeaderToleranceUnresolved.note(format!(
                "kernel linear tolerance {value} cannot supply a positive finite millimetre tolerance; keeping the default"
            )));
        }
    }
    if let Some(value) = header.angular {
        if let Some(angular) = cadmpeg_ir::scalar::PositiveAngle::new(value) {
            ir.tolerances.angular = angular;
        } else {
            losses.push(InventorLossCode::KernelHeaderToleranceUnresolved.note(format!(
                "kernel angular tolerance {value} cannot supply a positive finite tolerance; keeping the default"
            )));
        }
    }
    let Some(source) = ir.source.as_mut() else {
        return;
    };
    if let Some(version) = header.save_format_version {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_save_format_version"),
            format!("{version}"),
        );
    }
    if let Some(flags) = header.flags {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_flags"),
            format!("{flags}"),
        );
    }
    if let Some(family) = &header.product_family {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_product_family"),
            family.clone(),
        );
    }
    if let Some(version) = &header.product_version {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_product_version"),
            version.clone(),
        );
    }
    source.attributes.insert(
        cadmpeg_core::nonblank_literal!("kernel_family"),
        family.label().to_string(),
    );
}

fn admit_ufrx_record<T>(
    ctx: &DecodeContext<'_>,
    admitted: Result<T, String>,
    (scope, record): (&str, Option<&str>),
    rejected: &mut Rejections,
) -> Result<Option<T>, CodecError> {
    match admitted {
        Ok(record) => {
            admit_native_items(ctx, 1)?;
            Ok(Some(record))
        }
        Err(detail) => {
            if let Some(record) = record {
                rejected.records.push((record.to_owned(), detail.clone()));
            }
            rejected.issues.push(structural_issue(ctx, scope, &detail)?);
            Ok(None)
        }
    }
}

/// The structural issues of the records a projection rejects, and the id
/// and detail of each rejected record that the document would hold under
/// its own id. A rejected part of a record the document keeps has an issue
/// and no id.
#[derive(Default)]
struct Rejections {
    issues: Vec<StructuralIssueRecord>,
    records: Vec<(String, String)>,
}

fn admit_protein_asset(
    ctx: &DecodeContext<'_>,
    wire: ProteinAssetRecordWire,
    rejected: &mut Rejections,
) -> Result<Option<ProteinAssetRecord>, CodecError> {
    let scope = wire.id.clone();
    admit_ufrx_record(
        ctx,
        ProteinAssetRecord::try_from(wire),
        (&scope, Some(&scope)),
        rejected,
    )
}

fn admit_protein_rejection(
    ctx: &DecodeContext<'_>,
    wire: ProteinRejectionRecordWire,
    rejected: &mut Rejections,
) -> Result<Option<ProteinRejectionRecord>, CodecError> {
    let scope = wire.id.clone();
    admit_ufrx_record(
        ctx,
        ProteinRejectionRecord::try_from(wire),
        (&scope, Some(&scope)),
        rejected,
    )
}

fn admit_assembly_placement(
    ctx: &DecodeContext<'_>,
    wire: AssemblyPlacementRecordWire,
    issues: &mut Vec<RecordIssue>,
) -> Result<Option<AssemblyPlacementRecord>, CodecError> {
    let segment_token = wire.segment_token.clone();
    let record_ordinal = wire.record_ordinal;
    match AssemblyPlacementRecord::try_from(wire) {
        Ok(record) => {
            ctx.charge_entities(1, "admit Inventor native assembly placement")?;
            Ok(Some(record))
        }
        Err(detail) => {
            ctx.charge_entities(1, "admit Inventor placement conversion issue")?;
            issues.push(RecordIssue {
                family: RecordIssueFamily::Assembly,
                segment_token: cadmpeg_ir::ids::IdentityKey::try_new(segment_token)
                    .map_err(CodecError::malformed)?,
                record_ordinal,
                detail,
            });
            Ok(None)
        }
    }
}

fn structural_issue(
    ctx: &DecodeContext<'_>,
    scope: &str,
    detail: &str,
) -> Result<StructuralIssueRecord, CodecError> {
    ctx.charge_entities(1, "admit Inventor structural issue")?;
    Ok(StructuralIssueRecord {
        id: format!("inventor:rse:structural-issue#{scope}"),
        scope: scope.to_owned(),
        detail: detail.to_owned(),
    })
}

const FMTID_SUMMARY_INFORMATION: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];

#[derive(Default)]
struct MetadataProjection {
    title: Option<String>,
    author: Option<String>,
    description: Option<String>,
    part_number: Option<String>,
    document_kind: Option<DocumentKind>,
    bom_properties: BTreeMap<String, String>,
    unmapped: usize,
}

impl MetadataProjection {
    fn consider(
        &mut self,
        fmtid: &[u8; 16],
        property_id: u32,
        name: Option<&str>,
        value: Option<&str>,
        native_id: &str,
    ) {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return;
        };
        let normalized = name.map(normalize_property_name);
        if matches!(normalized.as_deref(), Some("documentkind" | "documenttype")) {
            self.document_kind = DocumentKind::parse_property(value);
            if self.document_kind.is_some() {
                return;
            }
        }
        let target = if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 2
            || normalized.as_deref() == Some("title")
        {
            Some(&mut self.title)
        } else if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 4
            || matches!(normalized.as_deref(), Some("author" | "designer"))
        {
            Some(&mut self.author)
        } else if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 6
            || matches!(normalized.as_deref(), Some("description" | "comments"))
        {
            Some(&mut self.description)
        } else if normalized.as_deref() == Some("partnumber") {
            Some(&mut self.part_number)
        } else {
            None
        };
        if let Some(target) = target {
            if target.is_none() {
                *target = Some(value.into());
            } else if target.as_deref() != Some(value) {
                self.bom_properties.insert(native_id.into(), value.into());
            }
            return;
        }
        if let Some(name) = name {
            self.bom_properties.insert(name.into(), value.into());
        } else {
            self.unmapped += 1;
        }
    }

    fn apply_attributes(&self, attributes: &mut BTreeMap<String, String>) {
        for (name, value) in [
            ("title", &self.title),
            ("author", &self.author),
            ("description", &self.description),
            ("part_number", &self.part_number),
        ] {
            if let Some(value) = value {
                attributes.insert(name.into(), value.clone());
            }
        }
    }
}

fn normalize_property_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn property_set_name(section: &PropertySection<'_>) -> Option<String> {
    section
        .properties
        .iter()
        .find(|property| property.id == 255)
        .and_then(|property| property.value.scalar_text())
}

fn known_property_set_fmtid(set_name: &str) -> Option<[u8; 16]> {
    match set_name {
        "Design Tracking Control" => Some([
            0x30, 0xfb, 0x61, 0xd8, 0x36, 0x31, 0xd1, 0x11, 0x9e, 0x92, 0x00, 0x60, 0xb0, 0x3c,
            0x1c, 0xa6,
        ]),
        "Inventor User Defined Properties" => Some([
            0xb8, 0xad, 0x29, 0x99, 0x07, 0x64, 0x3e, 0x41, 0xb3, 0xdc, 0xcb, 0x9a, 0xd2, 0xf5,
            0x64, 0xb7,
        ]),
        "Inventor Summary Information" => Some([
            0x39, 0xde, 0x38, 0x3d, 0x88, 0x05, 0x14, 0x4c, 0xbb, 0x37, 0x18, 0xf4, 0xd5, 0xdd,
            0x31, 0xc7,
        ]),
        "Inventor Document Summary Information" => Some([
            0x00, 0x80, 0xf5, 0x8c, 0x66, 0xda, 0xe6, 0x4a, 0x8f, 0xf0, 0x7b, 0x58, 0x40, 0x6f,
            0xb0, 0x49,
        ]),
        "Design Tracking Properties" => Some([
            0x0f, 0x3f, 0x85, 0x32, 0x44, 0x34, 0xd1, 0x11, 0x9e, 0x93, 0x00, 0x60, 0xb0, 0x3c,
            0x1c, 0xa6,
        ]),
        "_Private Model Information" => Some([
            0x90, 0x69, 0x58, 0xbb, 0x3e, 0xaf, 0xd3, 0x11, 0x95, 0xa9, 0x00, 0xa0, 0xc9, 0xb6,
            0xe3, 0x7a,
        ]),
        _ => None,
    }
}

fn built_in_property_name(set_name: &str, id: u32) -> Option<&'static str> {
    match (set_name, id) {
        (_, 1) => Some("Code Page"),
        (_, 255) => Some("Property Set Name"),
        ("Inventor Summary Information", 2) => Some("Title"),
        ("Inventor Summary Information", 3) => Some("Subject"),
        ("Inventor Summary Information", 4) => Some("Author"),
        ("Inventor Summary Information", 5) => Some("Keywords"),
        ("Inventor Summary Information", 6) => Some("Comments"),
        ("Inventor Summary Information", 8) => Some("Last Saved By"),
        ("Inventor Summary Information", 9) => Some("Revision"),
        ("Inventor Summary Information", 12) => Some("Creation Time"),
        ("Inventor Summary Information", 17) => Some("Thumbnail"),
        ("Inventor Document Summary Information", 2) => Some("Category"),
        ("Inventor Document Summary Information", 14) => Some("Manager"),
        ("Inventor Document Summary Information", 15) => Some("Company"),
        ("Design Tracking Control", 5) => Some("Checked Out By"),
        ("Design Tracking Control", 6) => Some("Checked Out Date"),
        ("Design Tracking Control", 7) => Some("Checked In By"),
        ("Design Tracking Control", 8) => Some("Checked In Date"),
        ("Design Tracking Control", 9) => Some("Check Out Workgroup"),
        ("Design Tracking Control", 11) => Some("Check Out Workspace"),
        ("Design Tracking Control", 12) => Some("Check Out Version"),
        ("Design Tracking Control", 13) => Some("Next Version"),
        ("Design Tracking Control", 14) => Some("Current Version"),
        ("Design Tracking Control", 15) => Some("Previous Version"),
        ("Design Tracking Control", 16) => Some("Last Saved By"),
        ("Design Tracking Control", 17) => Some("Last Saved Date"),
        ("Design Tracking Control", 19) => Some("Drawing Defer Update"),
        ("Design Tracking Control", 22) => Some("Build Version"),
        ("Design Tracking Properties", 4) => Some("Creation Date"),
        ("Design Tracking Properties", 5) => Some("Part Number"),
        ("Design Tracking Properties", 7) => Some("Project"),
        ("Design Tracking Properties", 9) => Some("Cost Center"),
        ("Design Tracking Properties", 10) => Some("Checked By"),
        ("Design Tracking Properties", 11) => Some("Date Checked"),
        ("Design Tracking Properties", 12) => Some("Engineering Approved By"),
        ("Design Tracking Properties", 13) => Some("Date Engineering Approved"),
        ("Design Tracking Properties", 17) => Some("User Status"),
        ("Design Tracking Properties", 20) => Some("Material"),
        ("Design Tracking Properties", 21) => Some("Part Property Revision Id"),
        ("Design Tracking Properties", 23) => Some("Catalog Web Link"),
        ("Design Tracking Properties", 28) => Some("Part Icon"),
        ("Design Tracking Properties", 29) => Some("Description"),
        ("Design Tracking Properties", 30) => Some("Vendor"),
        ("Design Tracking Properties", 31) => Some("Document Subtype"),
        ("Design Tracking Properties", 32) => Some("Document Type"),
        ("Design Tracking Properties", 33) => Some("Proxy Refresh Date"),
        ("Design Tracking Properties", 34) => Some("Manufacturing Approved By"),
        ("Design Tracking Properties", 35) => Some("Date Manufacturing Approved"),
        ("Design Tracking Properties", 36) => Some("Cost"),
        ("Design Tracking Properties", 37) => Some("Standard"),
        ("Design Tracking Properties", 40) => Some("Design Status"),
        ("Design Tracking Properties", 41) => Some("Designer"),
        ("Design Tracking Properties", 42) => Some("Engineer"),
        ("Design Tracking Properties", 43) => Some("Authority"),
        ("Design Tracking Properties", 44) => Some("Parameterized Template"),
        ("Design Tracking Properties", 45) => Some("Template Row"),
        ("Design Tracking Properties", 46) => Some("External Property Revision Id"),
        ("Design Tracking Properties", 47) => Some("Standard Revision"),
        ("Design Tracking Properties", 48) => Some("Manufacturer"),
        ("Design Tracking Properties", 49) => Some("Standards Organization"),
        ("Design Tracking Properties", 50) => Some("Language"),
        ("Design Tracking Properties", 51) => Some("Drawing Defer Update"),
        ("Design Tracking Properties", 52) => Some("Designation Size"),
        ("Design Tracking Properties", 55) => Some("Stock Number"),
        ("Design Tracking Properties", 56) => Some("Categories"),
        ("Design Tracking Properties", 57) => Some("Weld Material"),
        ("Design Tracking Properties", 58) => Some("Mass"),
        ("Design Tracking Properties", 59) => Some("Surface Area"),
        ("Design Tracking Properties", 60) => Some("Volume"),
        ("Design Tracking Properties", 61) => Some("Density"),
        ("Design Tracking Properties", 62) => Some("Valid Mass Properties"),
        ("Design Tracking Properties", 63) => Some("Flat Pattern Extents Width"),
        ("Design Tracking Properties", 64) => Some("Flat Pattern Extents Length"),
        ("Design Tracking Properties", 65) => Some("Flat Pattern Extents Area"),
        ("Design Tracking Properties", 66) => Some("Sheet Metal Rule"),
        ("Design Tracking Properties", 67) => Some("Last Updated With"),
        ("Design Tracking Properties", 71) => Some("Material Identifier"),
        ("Design Tracking Properties", 72) => Some("Appearance"),
        ("Design Tracking Properties", 73) => Some("Flat Pattern Defer Update"),
        ("_Private Model Information", 8) => Some("Length Units"),
        ("_Private Model Information", 9) => Some("Angle Units"),
        ("_Private Model Information", 10) => Some("Time Units"),
        ("_Private Model Information", 11) => Some("Mass Units"),
        ("_Private Model Information", 12) => Some("Length Display Precision"),
        ("_Private Model Information", 13) => Some("Angle Display Precision"),
        ("_Private Model Information", 14) => Some("Compacted"),
        ("_Private Model Information", 15) => Some("Assembly Available PVS"),
        ("_Private Model Information", 16) => Some("Part Active Color Style"),
        _ => None,
    }
}

fn property_value_kind(value: &PropertyValue<'_>) -> PropertyValueKind {
    match value {
        PropertyValue::Empty { type_code, .. } => PropertyValueKind::Empty {
            type_code: *type_code,
        },
        PropertyValue::Signed { type_code, .. } => PropertyValueKind::Signed {
            type_code: *type_code,
        },
        PropertyValue::Unsigned { type_code, .. } => PropertyValueKind::Unsigned {
            type_code: *type_code,
        },
        PropertyValue::Float { type_code, .. } => PropertyValueKind::Float {
            type_code: *type_code,
        },
        PropertyValue::Bool { type_code, .. } => PropertyValueKind::Bool {
            type_code: *type_code,
        },
        PropertyValue::Filetime { type_code, .. } => PropertyValueKind::Filetime {
            type_code: *type_code,
        },
        PropertyValue::String { type_code, .. } => PropertyValueKind::String {
            type_code: *type_code,
        },
        PropertyValue::Guid { type_code, .. } => PropertyValueKind::Guid {
            type_code: *type_code,
        },
        PropertyValue::Binary { type_code, value } => PropertyValueKind::Binary {
            type_code: *type_code,
            len: value.window().len(),
        },
        PropertyValue::Clipboard {
            type_code,
            format,
            data,
        } => PropertyValueKind::Clipboard {
            type_code: *type_code,
            format: *format,
            len: data.window().len(),
        },
        PropertyValue::Vector { type_code, values } => PropertyValueKind::Vector {
            type_code: *type_code,
            len: values.len(),
        },
        PropertyValue::Dictionary => PropertyValueKind::Dictionary,
        PropertyValue::Unknown { type_code } => PropertyValueKind::Unknown {
            type_code: *type_code,
        },
    }
}

fn is_preview(fmtid: &[u8; 16], property_id: u32, name: Option<&str>) -> bool {
    if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 17 {
        return true;
    }
    name.is_some_and(|name| {
        matches!(
            normalize_property_name(name).as_str(),
            "thumbnail" | "preview" | "previewimage"
        )
    })
}

fn preview_bytes<'a>(value: &'a PropertyValue<'a>) -> Option<(&'a [u8], &'static str)> {
    let bytes = match value {
        PropertyValue::Binary { value: view, .. } => view.window(),
        PropertyValue::Clipboard { format, data, .. } if *format == u32::MAX => {
            let bytes = data.window();
            let mut header = View::over_retained(bytes);
            let image_kind = header.u32_le()?;
            let header_size = header.u16_le()?;
            let width = u32::from(header.u16_le()?);
            let height = u32::from(header.u16_le()?);
            let reserved = header.u16_le()?;
            let png = bytes.get(12..)?;
            let png_header = png.get(..24)?;
            if image_kind != 3
                || header_size != 8
                || width == 0
                || height == 0
                || reserved != 0
                || !png_header.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
                || View::u32_be_at(png, 16)? != width
                || View::u32_be_at(png, 20)? != height
            {
                return None;
            }
            return Some((png, "image/png"));
        }
        PropertyValue::Clipboard { .. } => return None,
        _ => return None,
    };
    let media_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        "image/tiff"
    } else {
        return None;
    };
    Some((bytes, media_type))
}

/// Lowercase hexadecimal digits, indexed by nibble.
const HEX_DIGITS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Appends `byte` as two lowercase hexadecimal digits.
pub(crate) fn push_hex(out: &mut String, byte: u8) {
    out.push(HEX_DIGITS[usize::from(byte >> 4)]);
    out.push(HEX_DIGITS[usize::from(byte & 0x0f)]);
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        push_hex(&mut output, *byte);
    }
    output
}

#[cfg(test)]
mod tests;
