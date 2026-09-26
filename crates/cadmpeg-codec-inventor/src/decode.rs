// SPDX-License-Identifier: Apache-2.0
//! High-level Inventor structural decode.

mod rse_native_projection;

use cadmpeg_ir::annotations::StreamHandle;
use std::collections::{BTreeMap, HashMap};

use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AppearanceId, BodyId, FaceId, ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::decode::TransferLedger;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::units::Tolerances;
use cadmpeg_ir::{AnnotationBuilder, NativeUnknownRecord, SourceFidelity, UnknownRecord};

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
    UfrxModelStateRecordWire, UfrxOccurrenceRecord, UfrxOccurrenceRecordWire, UfrxRecord,
    UfrxRepresentationRecord, UfrxRepresentationRecordWire,
};
use crate::native::{
    ActiveCarrierRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecord,
    AssemblyPlacementRecordWire, DatabaseIssueRecord, DatabaseRecord, PmAppDefaultStyleRecord,
    PmAppRenderingStyleRecord, PmAppRenderingStyleRecordWire, PmGraphicsFaceRecord,
    PmGraphicsPrimaryColorStyleRecord, PmGraphicsStyleCollectionRecord, PropertyRecord,
    PropertySectionRecord, PropertySetIssueRecord, PropertySetRecord, PropertyValueKind,
    RevisionPayloadForm, RevisionRecord, SegmentRegistryRecord, StorageBandRecord,
    StructuralIssueRecord, VersionTupleRecord,
};
use crate::property_set::{PropertySection, PropertySetState, PropertyValue};
use crate::protein::ProteinState;
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{
    DatabaseState, DocumentKind, ParsedState, RecordFrameState, SegmentBulkState, SegmentMetaState,
};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    decode_container(ctx, &InventorContainer::open(ctx, root)?)
}

fn decode_container<'a>(
    ctx: &DecodeContext<'a>,
    container: &InventorContainer<'a>,
) -> Result<Decoded, CodecError> {
    // One predicate, read once from the parsed declarations: it decides the
    // admission in `primary` and the dialect-unverified loss below, and neither
    // recomputes the other.
    let recovery = DialectRecovery::of(ctx, container)?;
    let matched = recovery.classify(ctx)?;
    let dialects = crate::dialect::layers(ctx, &matched, &container.rse.active_carrier)?;
    let mut assembly_inventory = crate::assembly::inventory(ctx, &container.rse)?;
    let mut presentation_inventory = crate::presentation::inventory(ctx, &container.rse)?;
    let design_inventory = crate::design::inventory(ctx, &container.rse)?;
    let sketch_inventory = crate::sketch::inventory(ctx, &container.rse)?;
    let feature_inventory = crate::feature::inventory(ctx, &container.rse)?;
    admit_native_record_items(
        ctx,
        container,
        &assembly_inventory,
        &presentation_inventory,
        &design_inventory,
        &sketch_inventory,
        &feature_inventory,
    )?;
    let mut ir = CadIr::empty();
    let mut admitted_entities = 0_u64;
    let (design_parameters, unresolved_design_parameters) =
        crate::design::project_parameters(ctx, &design_inventory, &mut admitted_entities)?;
    ir.model.parameters = design_parameters;
    let sketch_projection = crate::sketch::project(ctx, &sketch_inventory, &ir.model.parameters)?;
    let unresolved_sketches = sketch_projection.unresolved_sketches;
    let unresolved_sketch_entities = sketch_projection.unresolved_entities;
    let unresolved_sketch_constraints = sketch_projection.unresolved_constraints;
    ir.model.sketches = sketch_projection.sketches;
    ir.model.sketch_entities = sketch_projection.entities;
    ir.model.sketch_constraints = sketch_projection.constraints;
    let feature_projection = crate::feature::project(
        ctx,
        &feature_inventory,
        &design_inventory,
        &sketch_inventory,
        &ir.model.parameters,
        &ir.model.sketches,
    )?;
    let unresolved_features = feature_projection.unresolved_features;
    let unresolved_feature_states = feature_projection.unresolved_states;
    ir.model.features = feature_projection.features;
    ir.model.feature_result_topologies = feature_projection.result_topologies;
    admitted_entities = wire_len(ctx, ir.model.entity_count(), "Inventor model entity count")?;
    let mut attributes = BTreeMap::new();
    insert_source_attribute(
        ctx,
        &mut attributes,
        "cfb_major_version",
        format_args!("{}", container.snapshot.major_version()),
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "cfb_sector_size",
        format_args!("{}", container.snapshot.sector_size()),
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "rse_segment_pairs",
        format_args!("{}", container.rse.segments.len()),
    )?;
    let mut document_kind = container.rse.document_kind();
    let mut metadata = MetadataProjection::default();
    let mut property_sets = Vec::new();
    let mut property_sections = Vec::new();
    let mut properties = Vec::new();
    let mut property_set_issues = Vec::new();
    for descriptor in &container.property_sets {
        match &descriptor.state {
            PropertySetState::Malformed(detail) => {
                property_set_issues.push(project_property_set_issue(
                    ctx,
                    descriptor.stream.directory_id(),
                    &descriptor.path,
                    detail,
                )?);
            }
            PropertySetState::Parsed(property_set) => {
                let id = retained_format(
                    ctx,
                    format_args!("inventor:property:set#{}", descriptor.stream.directory_id()),
                    "retain Inventor property-set id",
                )?;
                charge_retained_len(ctx, 32, "retain Inventor property-set CLSID")?;
                property_sets.push(PropertySetRecord {
                    id,
                    path: retained_clone(
                        ctx,
                        &descriptor.path,
                        "retain Inventor property-set path",
                    )?,
                    directory_id: descriptor.stream.directory_id(),
                    version: property_set.version,
                    system_identifier: property_set.system_identifier,
                    clsid: hex(&property_set.clsid),
                    section_count: wire_len(
                        ctx,
                        property_set.sections.len(),
                        "Inventor property section count",
                    )?,
                });
                for (section_ordinal, section) in property_set.sections.iter().enumerate() {
                    let set_name = property_set_name(ctx, section)?;
                    let identity_matches = set_name
                        .as_deref()
                        .and_then(known_property_set_fmtid)
                        .is_none_or(|expected| expected == section.fmtid);
                    if !identity_matches {
                        admit_native_items(ctx, 1)?;
                        let id = retained_format(
                            ctx,
                            format_args!(
                                "inventor:property:set-identity#{}-{section_ordinal}",
                                descriptor.stream.directory_id()
                            ),
                            "retain Inventor property-set identity issue id",
                        )?;
                        charge_retained_len(
                            ctx,
                            "embedded property-set name does not match its FMTID".len(),
                            "retain Inventor property-set identity issue detail",
                        )?;
                        property_set_issues.push(PropertySetIssueRecord {
                            id,
                            path: retained_clone(
                                ctx,
                                &descriptor.path,
                                "retain Inventor property-set identity issue path",
                            )?,
                            directory_id: descriptor.stream.directory_id(),
                            detail: "embedded property-set name does not match its FMTID".into(),
                        });
                    }
                    let section_id = retained_format(
                        ctx,
                        format_args!(
                            "inventor:property:section#{}-{section_ordinal}",
                            descriptor.stream.directory_id()
                        ),
                        "retain Inventor property section id",
                    )?;
                    charge_retained_len(ctx, 32, "retain Inventor property section FMTID")?;
                    property_sections.push(PropertySectionRecord {
                        id: section_id,
                        set_path: retained_clone(
                            ctx,
                            &descriptor.path,
                            "retain Inventor property section path",
                        )?,
                        ordinal: record_ordinal(
                            ctx,
                            section_ordinal,
                            "Inventor property section ordinal",
                        )?,
                        fmtid: hex(&section.fmtid),
                        code_page: section.code_page,
                        offsets_ordered: section.offsets_ordered,
                        dictionary_entries: wire_len(
                            ctx,
                            section.dictionary_entries,
                            "Inventor property dictionary count",
                        )?,
                        property_count: wire_len(
                            ctx,
                            section.properties.len(),
                            "Inventor property count",
                        )?,
                    });
                    for property in &section.properties {
                        if let Some(name) = &property.name {
                            charge_retained_len(ctx, name.len(), "retain Inventor property name")?;
                        } else if identity_matches {
                            if let Some(name) = set_name
                                .as_deref()
                                .and_then(|set_name| built_in_property_name(set_name, property.id))
                            {
                                charge_retained_len(
                                    ctx,
                                    name.len(),
                                    "retain Inventor built-in property name",
                                )?;
                            }
                        }
                        let property_name = property.name.clone().or_else(|| {
                            identity_matches
                                .then_some(set_name.as_deref())
                                .flatten()
                                .and_then(|set_name| built_in_property_name(set_name, property.id))
                                .map(str::to_owned)
                        });
                        let native_id = retained_format(
                            ctx,
                            format_args!(
                                "inventor:property:value#{}-{section_ordinal}-{}",
                                descriptor.stream.directory_id(),
                                property.id
                            ),
                            "retain Inventor property value id",
                        )?;
                        let scalar_value = property.value.scalar_text(ctx)?;
                        metadata.consider(
                            ctx,
                            &section.fmtid,
                            property.id,
                            property_name.as_deref(),
                            scalar_value.as_deref(),
                            &native_id,
                        )?;
                        if is_preview(ctx, &section.fmtid, property.id, property_name.as_deref())? {
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
                        charge_retained_len(ctx, 32, "retain Inventor property value FMTID")?;
                        charge_retained_len(ctx, 64, "retain Inventor property raw digest")?;
                        ctx.charge_work(
                            wire_len(
                                ctx,
                                property.raw.window().len(),
                                "Inventor property raw length",
                            )?,
                            "hash Inventor property raw bytes",
                        )?;
                        properties.push(PropertyRecord {
                            id: native_id,
                            set_path: retained_clone(
                                ctx,
                                &descriptor.path,
                                "retain Inventor property value path",
                            )?,
                            section_ordinal: record_ordinal(
                                ctx,
                                section_ordinal,
                                "Inventor property section ordinal",
                            )?,
                            fmtid: hex(&section.fmtid),
                            property_id: property.id,
                            name: property_name,
                            value_kind: property_value_kind(&property.value),
                            scalar_value,
                            raw_len: wire_len(
                                ctx,
                                property.raw.window().len(),
                                "Inventor property raw length",
                            )?,
                            raw_sha256: crate::native::digest::Sha256Hex::digest(
                                property.raw.window(),
                            ),
                        });
                    }
                }
            }
        }
    }
    let protein = project_protein_state(ctx, &container.protein)?;
    let (protein_instances, protein_semantic_issue) = match &container.protein {
        ProteinState::Package(package) => match crate::protein::decode_instances(ctx, package) {
            Ok(instances) => (instances, None),
            Err(error) => (Vec::new(), Some(crate::issue_detail(error)?)),
        },
        ProteinState::Absent | ProteinState::Empty { .. } | ProteinState::Malformed { .. } => {
            (Vec::new(), None)
        }
    };
    let material_catalog =
        crate::materials::project_catalog(ctx, &protein_instances, &mut admitted_entities)?;
    let ProteinNativeRecords {
        assets: protein_assets,
        rejections: protein_rejections,
        issues: protein_issues,
    } = project_protein_records(ctx, protein_instances)?;
    let mut ufrx_issues = Vec::new();
    ir.model.appearances = material_catalog.appearances;
    let protein_appearance_count = ir.model.appearances.len();
    let ufrx = project_ufrx_state(ctx, &container.ufrx, &mut ufrx_issues)?;
    let protein_admission_issue_count = protein_issues.len();
    let ufrx_issue_count = ufrx_issues.len();
    let mut structural_issues = protein_issues;
    structural_issues.extend(ufrx_issues);
    let ufrx_model_states = ufrx.model_states();
    let external_references = ufrx.external_references();
    let embedded_references = ufrx.embedded_references();
    let ufrx_occurrences = ufrx.occurrences();
    if matches!(document_kind, DocumentKind::Unknown | DocumentKind::Mixed) {
        if let Some(property_kind) = metadata.document_kind.take() {
            document_kind = property_kind;
        }
    }
    insert_source_attribute(
        ctx,
        &mut attributes,
        "document_kind",
        format_args!("{}", document_kind.label()),
    )?;
    metadata.apply_attributes(ctx, &mut attributes)?;
    charge_items(ctx, attributes.len(), "name Inventor source attributes")?;
    ir.source = Some(SourceMeta::classified(
        dialects,
        cadmpeg_core::text::named_entries("the inventor document", attributes)?,
    ));
    if matches!(document_kind, DocumentKind::Part | DocumentKind::Assembly) {
        let entity_count = ir.model.entity_count();
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
            Ok(StorageBandRecord {
                id: retained_format(
                    ctx,
                    format_args!("inventor:rse:storage-band#v{}", database.band.value()),
                    "retain Inventor storage band id",
                )?,
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
            Ok(DatabaseRecord {
                id: retained_format(
                    ctx,
                    format_args!("inventor:rse:database#v{}", descriptor.band.value()),
                    "retain Inventor database id",
                )?,
                band: descriptor.band.value(),
                database_id: retained_hex(ctx, &database.id, "retain Inventor database GUID")?,
                schema: database.schema.value(),
                created_by: version_record(ctx, database.created_by)?,
                created_filetime: database.created_filetime,
                saved_by: version_record(ctx, database.saved_by)?,
                saved_filetime: database.saved_filetime,
                note: retained_clone(ctx, &database.note, "retain Inventor database note")?,
            })
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    let mut database_issues = Vec::new();
    for descriptor in &container.rse.databases {
        if let Some(detail) = descriptor.issue_detail(ctx)? {
            database_issues.push(DatabaseIssueRecord {
                id: retained_format(
                    ctx,
                    format_args!("inventor:rse:database-issue#v{}", descriptor.band.value()),
                    "retain Inventor database issue id",
                )?,
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
                Ok(SegmentRegistryRecord {
                    id: retained_format(
                        ctx,
                        format_args!("inventor:rse:registry-entry#{ordinal}"),
                        "retain Inventor registry entry id",
                    )?,
                    ordinal: record_ordinal(ctx, ordinal, "Inventor registry ordinal")?,
                    display_name: retained_clone(
                        ctx,
                        &entry.display_name,
                        "retain Inventor registry display name",
                    )?,
                    segment_id: retained_hex(
                        ctx,
                        &entry.segment_id,
                        "retain Inventor registry segment GUID",
                    )?,
                    revision_id: retained_hex(
                        ctx,
                        &entry.revision_id,
                        "retain Inventor registry revision GUID",
                    )?,
                    type_name: retained_clone(
                        ctx,
                        &entry.type_name,
                        "retain Inventor registry type name",
                    )?,
                    object_count: wire_len(
                        ctx,
                        entry.objects.len(),
                        "Inventor registry object count",
                    )?,
                    node_count: wire_len(ctx, entry.nodes.len(), "Inventor registry node count")?,
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
                Ok(RevisionRecord {
                    id: retained_format(
                        ctx,
                        format_args!("inventor:rse:revision#{ordinal}"),
                        "retain Inventor revision id",
                    )?,
                    ordinal: record_ordinal(ctx, ordinal, "Inventor revision ordinal")?,
                    revision_id: retained_hex(ctx, &entry.id, "retain Inventor revision GUID")?,
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
    if let ParsedState::Unavailable(detail) = &container.rse.registry {
        structural_issues.push(structural_issue(ctx, "segment_registry", detail)?);
    }
    if let ParsedState::Unavailable(detail) = &container.rse.revisions {
        structural_issues.push(structural_issue(ctx, "revision_table", detail)?);
    }
    let projection = rse_native_projection::project(ctx, container)?;
    structural_issues.extend(projection.identity_issues);
    let segment_pairs = projection.segment_pairs;
    let segment_meta = projection.segment_meta;
    let meta_sections = projection.meta_sections;
    let meta_types = projection.meta_types;
    let segment_meta_issues = projection.segment_meta_issues;
    let rse_records = projection.rse_records;
    let segment_bulk = projection.segment_bulk;
    let segment_bulk_issues = projection.segment_bulk_issues;
    let unpaired_segments = projection.unpaired_segments;
    let active_carrier = ActiveCarrierRecord::from_state(ctx, &container.rse.active_carrier)?;
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
                AssemblyPlacementRecordWire::from_placement(ctx, placement)?,
                &mut assembly_inventory.issues,
            )
        })
        .collect::<Result<Vec<_>, CodecError>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    admit_presentation_native_projection(ctx, &presentation_inventory)?;
    let pm_app_default_styles = presentation_inventory
        .default_styles
        .iter()
        .map(|style| {
            let (suffix_len, suffix_sha256) = crate::presentation::suffix_fields(style.suffix);
            PmAppDefaultStyleRecord {
                id: format!(
                    "inventor:presentation:default-style#{}-{}",
                    style.identity.segment_token, style.identity.record_ordinal
                ),
                segment_token: style.identity.segment_token.as_str().to_owned(),
                record_ordinal: style.identity.record_ordinal,
                segment_version_major: style.segment_version_major,
                header_value: style.header_value,
                header_id: style.header_id,
                material_reference: style.material_reference,
                rendering_style_reference: style.rendering_style_reference,
                related_references: style.related_references,
                state: style.state,
                terminal_reference: style.terminal_reference,
                suffix_len,
                suffix_sha256,
            }
        })
        .collect::<Vec<_>>();
    let pm_app_rendering_styles = presentation_inventory
        .rendering_styles
        .iter()
        .filter_map(|style| {
            let (suffix_len, suffix_sha256) = crate::presentation::suffix_fields(style.suffix);
            let wire = PmAppRenderingStyleRecordWire {
                id: format!(
                    "inventor:presentation:rendering-style#{}-{}",
                    style.identity.segment_token, style.identity.record_ordinal
                ),
                segment_token: style.identity.segment_token.as_str().to_owned(),
                record_ordinal: style.identity.record_ordinal,
                segment_version_major: style.segment_version_major,
                header_value: style.header_value,
                header_id: style.header_id,
                state: style.state,
                flags: style.flags,
                values: style.values,
                default_state: style.default_state,
                value: style.value,
                name_reference: style.name_reference,
                name: style.name.clone(),
                comment: style.comment.clone(),
                long_name: style.long_name.clone(),
                style_state: style
                    .extension
                    .as_ref()
                    .map(|extension| extension.style_state),
                style_label: style
                    .extension
                    .as_ref()
                    .map(|extension| extension.style_label.clone()),
                asset_guid: style
                    .extension
                    .as_ref()
                    .map(|extension| extension.asset_guid.clone()),
                material_id: style
                    .extension
                    .as_ref()
                    .map(|extension| extension.material_id.clone()),
                asset_library_id: style
                    .extension
                    .as_ref()
                    .map(|extension| extension.asset_library_id.clone()),
                style_values: style
                    .extension
                    .as_ref()
                    .map(|extension| extension.style_values),
                guid: style
                    .extension
                    .as_ref()
                    .map(|extension| extension.guid.clone()),
                suffix_len,
                suffix_sha256: suffix_sha256.into(),
            };
            PmAppRenderingStyleRecord::try_from(wire)
                .inspect_err(|detail| {
                    presentation_inventory.issues.push(RecordIssue {
                        family: RecordIssueFamily::Presentation,
                        segment_token: style.identity.segment_token.as_str().to_owned(),
                        record_ordinal: style.identity.record_ordinal,
                        detail: detail.clone(),
                    });
                })
                .ok()
        })
        .collect::<Vec<_>>();
    let pm_graphics_faces = presentation_inventory
        .graphics_faces
        .iter()
        .map(|face| PmGraphicsFaceRecord {
            id: format!(
                "inventor:presentation:graphics-face#{}-{}",
                face.identity.segment_token, face.identity.record_ordinal
            ),
            segment_token: face.identity.segment_token.as_str().to_owned(),
            record_ordinal: face.identity.record_ordinal,
            segment_version_major: face.segment_version_major,
            header_value: face.header_value,
            header_id: face.header_id,
            flags: face.flags,
            styles: face.styles,
            surface: face.surface,
            parent: face.parent,
            state: face.state,
            edge_references: face.edge_references.clone(),
            visibility_state: face.visibility_state,
            bounds: face.bounds,
            key: face.key,
            values: face.values,
        })
        .collect::<Vec<_>>();
    let pm_graphics_style_collections = presentation_inventory
        .graphics_style_collections
        .iter()
        .map(|collection| PmGraphicsStyleCollectionRecord {
            id: format!(
                "inventor:presentation:graphics-style-collection#{}-{}",
                collection.identity.segment_token, collection.identity.record_ordinal
            ),
            segment_token: collection.identity.segment_token.as_str().to_owned(),
            record_ordinal: collection.identity.record_ordinal,
            segment_version_major: collection.segment_version_major,
            style_references: collection.style_references.clone(),
        })
        .collect::<Vec<_>>();
    let pm_graphics_primary_color_styles = presentation_inventory
        .graphics_primary_color_styles
        .iter()
        .map(|style| PmGraphicsPrimaryColorStyleRecord {
            id: format!(
                "inventor:presentation:graphics-primary-color#{}-{}",
                style.identity.segment_token, style.identity.record_ordinal
            ),
            segment_token: style.identity.segment_token.as_str().to_owned(),
            record_ordinal: style.identity.record_ordinal,
            segment_version_major: style.segment_version_major,
            header_value: style.header_value,
            controls: style.controls,
            color_header: style.color_header,
            colors: style.colors,
            color_tail: style.color_tail,
            state: style.state,
            values: style.values,
            terminal_state: style.terminal_state,
        })
        .collect::<Vec<_>>();
    ctx.admit_entities(
        wire_len(
            ctx,
            ir.model.entity_count(),
            "Inventor pre-assembly entity count",
        )?,
        &mut admitted_entities,
        "admit Inventor pre-assembly entities",
    )?;
    let assembly_projection = crate::assembly::project_occurrences(
        ctx,
        ufrx_occurrences,
        external_references,
        &assembly_occurrences,
        &assembly_placements,
    )?;
    ir.model.occurrences = assembly_projection.occurrences;
    let namespace = ir.native.namespace_mut("inventor");
    namespace.set_arena("storage_bands", &storage_bands)?;
    namespace.set_arena("databases", &databases)?;
    namespace.set_arena("database_issues", &database_issues)?;
    namespace.set_arena("segment_registry", &segment_registry)?;
    namespace.set_arena("revisions", &revisions)?;
    namespace.set_arena("structural_issues", &structural_issues)?;
    namespace.set_arena("property_sets", &property_sets)?;
    namespace.set_arena("property_sections", &property_sections)?;
    namespace.set_arena("properties", &properties)?;
    namespace.set_arena("property_set_issues", &property_set_issues)?;
    protein.install(namespace)?;
    namespace.set_arena("protein_assets", &protein_assets)?;
    namespace.set_arena("protein_rejections", &protein_rejections)?;
    ufrx.install(namespace)?;
    namespace.set_arena("assembly_occurrences", &assembly_occurrences)?;
    namespace.set_arena("assembly_placements", &assembly_placements)?;
    namespace.set_arena("assembly_record_issues", &assembly_inventory.issues)?;
    namespace.set_arena("pm_app_default_styles", &pm_app_default_styles)?;
    namespace.set_arena("pm_app_rendering_styles", &pm_app_rendering_styles)?;
    namespace.set_arena("pm_graphics_faces", &pm_graphics_faces)?;
    namespace.set_arena(
        "pm_graphics_style_collections",
        &pm_graphics_style_collections,
    )?;
    namespace.set_arena(
        "pm_graphics_primary_color_styles",
        &pm_graphics_primary_color_styles,
    )?;
    namespace.set_arena("presentation_record_issues", &presentation_inventory.issues)?;
    namespace.set_arena("pm_dc_parameters", &design_inventory.parameters)?;
    namespace.set_arena("pm_dc_expressions", &design_inventory.expressions)?;
    namespace.set_arena("pm_dc_units", &design_inventory.units)?;
    namespace.set_arena("design_record_issues", &design_inventory.issues)?;
    namespace.set_arena("pm_dc_sketches", &sketch_inventory.sketches)?;
    namespace.set_arena("pm_dc_sketch_entities", &sketch_inventory.entities)?;
    namespace.set_arena("pm_dc_transforms", &sketch_inventory.transforms)?;
    namespace.set_arena("pm_dc_directions", &sketch_inventory.directions)?;
    namespace.set_arena("pm_dc_sketch_constraints", &sketch_inventory.constraints)?;
    namespace.set_arena("sketch_record_issues", &sketch_inventory.issues)?;
    namespace.set_arena("pm_dc_features", &feature_inventory.features)?;
    namespace.set_arena(
        "pm_dc_pattern_features",
        &feature_inventory.pattern_features,
    )?;
    namespace.set_arena("pm_dc_feature_terminators", &feature_inventory.terminators)?;
    namespace.set_arena("pm_dc_feature_properties", &feature_inventory.properties)?;
    namespace.set_arena("pm_dc_feature_labels", &feature_inventory.labels)?;
    namespace.set_arena(
        "pm_dc_entity_style_links",
        &feature_inventory.entity_style_links,
    )?;
    namespace.set_arena("feature_record_issues", &feature_inventory.issues)?;
    namespace.set_arena("segment_pairs", &segment_pairs)?;
    namespace.set_arena("segment_meta", &segment_meta)?;
    namespace.set_arena("meta_sections", &meta_sections)?;
    namespace.set_arena("meta_types", &meta_types)?;
    namespace.set_arena("segment_meta_issues", &segment_meta_issues)?;
    namespace.set_arena("segment_bulk", &segment_bulk)?;
    namespace.set_arena("rse_records", &rse_records)?;
    namespace.set_arena("segment_bulk_issues", &segment_bulk_issues)?;
    namespace.set_arena("unpaired_segments", &unpaired_segments)?;
    namespace.set_arena("active_carrier", std::slice::from_ref(&active_carrier))?;

    let mut geometry_failure = None;
    let kernel_brep = match &container.rse.active_carrier {
        ActiveCarrierState::Selected(carrier) => match carrier.header.as_ref() {
            Ok(header) => match crate::kernel::decode_kernel_carrier(ctx, carrier, header) {
                Ok(decoded) => {
                    apply_kernel_header(ctx, &mut ir, carrier.family, &decoded.header.metadata)?;
                    Some(decoded.brep)
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    geometry_failure = Some(retained_format(
                        ctx,
                        format_args!("{error}"),
                        "retain Inventor geometry failure",
                    )?);
                    None
                }
            },
            Err(detail) => {
                geometry_failure = Some(retained_clone(
                    ctx,
                    detail,
                    "retain Inventor kernel header failure",
                )?);
                None
            }
        },
        _ => None,
    };
    let kernel_brep = kernel_brep.unwrap_or_else(AsmBrep::default);
    let face_keys = index_asm_face_keys(
        ctx,
        kernel_brep
            .face_native_keys
            .iter()
            .filter_map(|record| record.asm_face_key.map(|key| (&record.face, key))),
    )?;
    let (
        _,
        AsmTransferRemainder {
            unknowns: kernel_unknowns,
            stats: kernel_stats,
            annotation_records: kernel_annotations,
        },
    ) = transfer_into_ir(ctx, &mut ir, "inventor", kernel_brep)?;
    ir.set_native_unknowns("inventor", &[] as &[NativeUnknownRecord])?;
    let geometry_transferred =
        !(ir.model.surfaces.is_empty() && ir.model.points.is_empty() && ir.model.faces.is_empty());
    let body_ids = collect_body_ids(ctx, ir.model.bodies.iter().map(|body| &body.id))?;
    if geometry_transferred {
        for product in &mut ir.model.product_definitions {
            clone_product_body_ids(ctx, &body_ids, &mut product.bodies)?;
        }
    } else if matches!(
        &container.rse.active_carrier,
        ActiveCarrierState::Selected(_)
    ) && geometry_failure.is_none()
    {
        geometry_failure = Some(retained_clone(
            ctx,
            "the active kernel carrier decoded no surfaces, points, or faces",
            "retain Inventor empty carrier failure",
        )?);
    }
    let presentation_projection = crate::presentation::project_bindings(
        ctx,
        &presentation_inventory,
        &ir.model.appearances,
        &body_ids,
        &face_keys,
    )?;
    let face_color_appearance_count = presentation_projection.appearances.len();
    let projected_colors = index_projected_colors(
        ctx,
        presentation_projection
            .appearances
            .iter()
            .filter_map(|appearance| Some((&appearance.id, appearance.base_color?))),
    )?;
    ir.model
        .appearances
        .extend(presentation_projection.appearances);
    ir.model.appearance_bindings = presentation_projection.bindings;
    let face_colors = index_face_colors(
        ctx,
        ir.model.appearance_bindings.iter().filter_map(|binding| {
            if let cadmpeg_ir::appearance::AppearanceTarget::Face(face) = &binding.target {
                projected_colors
                    .get(&binding.appearance)
                    .copied()
                    .map(|color| (face, color))
            } else {
                None
            }
        }),
    )?;
    for face in &mut ir.model.faces {
        if face.color.is_none() {
            face.color = face_colors.get(&face.id).copied();
        }
    }
    // Read before `geometry_failure` is consumed by the loss message below.
    let carrier_read_no_geometry = geometry_failure.is_some();
    let mut losses = Vec::new();
    if protein_admission_issue_count != 0 {
        losses.push(admitted_loss(ctx, InventorLossCode::ProteinAssetRejected, format_args!(
            "Rejected {protein_admission_issue_count} Protein native record(s); retained the remaining records."
        ))?);
    }
    losses.extend(dialect_loss(ctx, &matched, &recovery)?);
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
        losses.extend(kernel_dialect_loss(ctx, kernel)?);
    }
    if !ctx.container_only()
        && !matches!(document_kind, DocumentKind::Assembly)
        && !geometry_transferred
    {
        let detail = match geometry_failure {
            Some(detail) => detail,
            None => match &container.rse.active_carrier {
                ActiveCarrierState::Selected(_) => retained_clone(
                    ctx,
                    "The typed active kernel carrier has not been transferred.",
                    "retain Inventor untransferred carrier detail",
                )?,
                ActiveCarrierState::Unavailable(detail) => retained_format(
                    ctx,
                    format_args!("The active Inventor kernel carrier is unavailable: {detail}"),
                    "retain Inventor unavailable carrier detail",
                )?,
                ActiveCarrierState::NotApplicable => retained_clone(
                    ctx,
                    "Inventor geometry is not available for this document kind.",
                    "retain Inventor missing geometry detail",
                )?,
            },
        };
        losses.push(admitted_loss(
            ctx,
            InventorLossCode::GeometryKernelCarrierNotTransferred,
            format_args!("{detail}"),
        )?);
    }
    if !ctx.container_only() {
        if kernel_stats.unknown_surface_faces() != 0 {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::GeometryProceduralSurfaceNotTransferred,
                format_args!(
                    "{} face(s) use procedural surfaces without a decoded carrier.",
                    kernel_stats.unknown_surface_faces()
                ),
            )?);
        }
        if !segment_pairs.is_empty() {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::RseSegmentPairUntyped,
                format_args!(
                    "Retained {} structurally paired RSe segment(s) without record semantics.",
                    segment_pairs.len()
                ),
            )?);
        }
        if !segment_meta_issues.is_empty() {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::RseMetadataStreamMalformed,
                format_args!(
                    "{} RSe metadata stream(s) are malformed or outside the implemented envelope.",
                    segment_meta_issues.len()
                ),
            )?);
        }
        if !segment_bulk_issues.is_empty() {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::RseBulkStreamMalformed,
                format_args!(
                    "{} RSe bulk stream(s) have invalid envelope or zlib framing.",
                    segment_bulk_issues.len()
                ),
            )?);
        }
        if !assembly_inventory.issues.is_empty() {
            losses.push(admitted_loss(ctx, InventorLossCode::AssemblyRecordMalformed, format_args!(
                "{} typed Inventor assembly record(s) are malformed or outside the implemented branch.",
                assembly_inventory.issues.len()
            ))?);
        }
        if !presentation_inventory.issues.is_empty() {
            losses.push(admitted_loss(ctx, InventorLossCode::PresentationRecordMalformed, format_args!(
                "{} typed Inventor presentation record(s) are malformed or outside the implemented branch.",
                presentation_inventory.issues.len()
            ))?);
        }
        if !design_inventory.issues.is_empty() {
            losses.push(admitted_loss(ctx, InventorLossCode::DesignRecordMalformed, format_args!(
                "{} typed Inventor design record(s) are malformed or outside the implemented branch.",
                design_inventory.issues.len()
            ))?);
        }
        if !sketch_inventory.issues.is_empty() {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::SketchRecordMalformed,
                format_args!(
                    "{} typed Inventor sketch record(s) could not be parsed exactly.",
                    sketch_inventory.issues.len()
                ),
            )?);
        }
        if !feature_inventory.issues.is_empty() {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::FeatureRecordMalformed,
                format_args!(
                    "{} typed Inventor feature record(s) could not be parsed exactly.",
                    feature_inventory.issues.len()
                ),
            )?);
        }
        if unresolved_features != 0 {
            losses.push(admitted_loss(ctx, InventorLossCode::FeatureOperationGraphOpen, format_args!(
                "Retained {unresolved_features} typed Inventor feature record(s) whose operation graph is not closed."
            ))?);
        }
        if unresolved_feature_states != 0 {
            losses.push(admitted_loss(ctx, InventorLossCode::FeatureStateUnresolved, format_args!(
                "Transferred {unresolved_feature_states} Inventor operation(s) with native result-body identity and unresolved suppression and dependency state."
            ))?);
        }
        if unresolved_design_parameters != 0 {
            losses.push(admitted_loss(ctx, InventorLossCode::ParameterGraphOpen, format_args!(
                "Retained {unresolved_design_parameters} Inventor parameter record(s) whose unit or expression graph is not closed."
            ))?);
        }
        if unresolved_sketches != 0
            || unresolved_sketch_entities != 0
            || unresolved_sketch_constraints != 0
        {
            losses.push(admitted_loss(ctx, InventorLossCode::SketchGraphOpen, format_args!(
                "Retained {unresolved_sketches} Inventor sketch record(s), {unresolved_sketch_entities} sketch-entity record(s), and {unresolved_sketch_constraints} sketch-constraint record(s) whose neutral graph is not closed."
            ))?);
        }
        if !container.rse.unpaired_metadata.is_empty() || !container.rse.unpaired_bulk.is_empty() {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::RseStreamUnpaired,
                format_args!(
                    "RSe contains {} unpaired metadata stream(s) and {} unpaired bulk stream(s).",
                    container.rse.unpaired_metadata.len(),
                    container.rse.unpaired_bulk.len()
                ),
            )?);
        }
        if !property_set_issues.is_empty() {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::PropertySetStreamMalformed,
                format_args!(
                    "{} OLE property-set stream(s) are malformed.",
                    property_set_issues.len()
                ),
            )?);
        }
        if metadata.unmapped != 0 {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::MetadataPropertyUnmapped,
                format_args!(
                    "Retained {} property value(s) without neutral metadata mapping.",
                    metadata.unmapped
                ),
            )?);
        }
        match &container.protein {
            ProteinState::Package(_) => {
                if let Some(detail) = &protein_semantic_issue {
                    losses.push(admitted_loss(
                        ctx,
                        InventorLossCode::ProteinCatalogUndecodable,
                        format_args!("The Protein asset catalog could not be decoded: {detail}"),
                    )?);
                } else {
                    if !protein_rejections.is_empty() {
                        losses.push(admitted_loss(ctx, InventorLossCode::ProteinAssetRejected, format_args!(
                            "Rejected {} malformed Protein asset record(s); later framed records remain decoded.",
                            protein_rejections.len()
                        ))?);
                    }
                    if ir.model.appearances.is_empty() {
                        losses.push(admitted_loss(
                            ctx,
                            InventorLossCode::ProteinAppearanceAbsent,
                            format_args!(
                                "The Protein package contains no decoded appearance assets.",
                            ),
                        )?);
                    } else if presentation_projection.unresolved_defaults != 0 {
                        losses.push(admitted_loss(
                            ctx,
                            InventorLossCode::AppearanceDefaultUnresolved,
                            format_args!(
                            "Could not resolve {} PmApp document-default appearance assignment(s).",
                            presentation_projection.unresolved_defaults
                        ),
                        )?);
                    }
                }
                if !material_catalog.duplicate_guids.is_empty() {
                    losses.push(admitted_loss(ctx, InventorLossCode::ProteinGuidAmbiguous, format_args!(
                        "The Protein catalog contains {} duplicate asset GUID(s); ambiguous texture joins were refused.",
                        material_catalog.duplicate_guids.len()
                    ))?);
                }
                if material_catalog.untyped_distance_properties != 0 {
                    losses.push(admitted_loss(ctx, InventorLossCode::MaterialDistanceUnitUntyped, format_args!(
                        "{} Protein texture Distance property value(s) retain an untyped unit tag; their typed texture carriers were omitted.",
                        material_catalog.untyped_distance_properties
                    ))?);
                }
            }
            ProteinState::Malformed { .. } => losses.push(admitted_loss(
                ctx,
                InventorLossCode::ProteinStreamMalformed,
                format_args!("The Inventor Protein stream is malformed."),
            )?),
            ProteinState::Absent | ProteinState::Empty { .. } => {}
        }
        for (cause, count) in &presentation_projection.unresolved_face_overrides {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::AppearanceFaceOverrideUnresolved,
                format_args!(
                    "Could not resolve {count} PmGraphics face appearance override(s): {}.",
                    cause.description()
                ),
            )?);
        }
        if ufrx_issue_count != 0 {
            losses.push(admitted_loss(
                ctx,
                InventorLossCode::UfrxTableMalformed,
                format_args!("Skipped {ufrx_issue_count} invalid UFRxDoc native record(s)."),
            )?);
        }
        match &container.ufrx {
            UfrxState::Malformed { .. } => losses.push(admitted_loss(
                ctx,
                InventorLossCode::UfrxTableMalformed,
                format_args!("The UFRxDoc external-reference table is malformed."),
            )?),
            UfrxState::Unsupported { schema, .. } => {
                let code = if matches!(document_kind, DocumentKind::Assembly) {
                    InventorLossCode::UfrxSchemaUnsupportedAssembly
                } else {
                    InventorLossCode::UfrxSchemaUnsupported
                };
                losses.push(admitted_loss(
                    ctx,
                    code,
                    format_args!(
                    "Retained unsupported UFRxDoc schema {schema} semantic branch without transfer."
                ),
                )?);
            }
            UfrxState::Parsed(_) if matches!(document_kind, DocumentKind::Assembly) => {
                if !external_references.is_empty() {
                    losses.push(admitted_loss(
                        ctx,
                        InventorLossCode::AssemblyComponentExternal,
                        format_args!(
                            "Retained {} unresolved external component reference(s).",
                            external_references.len()
                        ),
                    )?);
                }
                for (cause, count) in &assembly_projection.unresolved_placements {
                    losses.push(admitted_loss(
                        ctx,
                        InventorLossCode::AssemblyPlacementNotTransferred,
                        format_args!(
                            "Could not transfer {count} assembly occurrence placement(s): {}.",
                            cause.description()
                        ),
                    )?);
                }
            }
            UfrxState::Absent | UfrxState::Parsed(_) => {}
        }
    }
    let preview_asset_count = ir.model.assets.len();
    let mut source_fidelity = SourceFidelity::default();
    let mut annotations = AnnotationBuilder::new();
    for record in kernel_annotations {
        admit_kernel_annotation(ctx, &record)?;
        let stream =
            StreamHandle::new(cadmpeg_ir::stream_name!("inventor:").with_suffix(&record.stream));
        annotations
            .note(&record.id, &stream, record.offset)
            .tag(record.tag.as_str());
        for field in record.derived_fields {
            annotations
                .derived(&record.id, field)
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
    }
    source_fidelity.annotations = annotations.build();
    if let ActiveCarrierState::Selected(carrier) = &container.rse.active_carrier {
        // Retention is keyed on the outcome, not on a version band: a carrier
        // this decode read no geometry out of keeps its bytes verbatim, whatever
        // its save format declared.
        if carrier_read_no_geometry {
            admit_untransferred_carrier(ctx, carrier, active_carrier.id())?;
            let data = ctx.copy_retained(
                carrier.bytes.window(),
                "retain Inventor kernel carrier that read no geometry",
            )?;
            source_fidelity.retain_unknown_records(
                format!("RSeStorage/B{}:expanded", carrier.segment_token),
                [UnknownRecord::retained(
                    UnknownId::compose(
                        &cadmpeg_ir::identity_namespace!("inventor", "kernel", "carrier"),
                        carrier.segment_token.clone().dash(carrier.record_ordinal),
                    ),
                    carrier.carrier_offset,
                    data,
                    vec![active_carrier.id().to_owned()],
                )],
            )?;
        }
    }
    if !kernel_unknowns.is_empty() {
        admit_kernel_unknown_fidelity(ctx, &source_fidelity, &kernel_unknowns)?;
        source_fidelity
            .attach_native_unknown_records(&mut ir, "inventor", kernel_unknowns)
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "Inventor kernel unknown retention failed: {error}"
                ))
            })?;
    }
    let kernel_unknown_record_count = ir.native_unknowns("inventor")?.len();
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
        (crate::coverage::PROTEIN_ENTRIES, protein.entries().len()),
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
            crate::coverage::KERNEL_UNKNOWN_RECORDS,
            kernel_unknown_record_count,
        ),
        (
            crate::coverage::KERNEL_UNKNOWN_SURFACE_FACES,
            kernel_stats.unknown_surface_faces(),
        ),
    ];
    admit_coverage_entries(ctx, coverage_entries.iter().map(|(key, _)| *key))?;
    let body = DecodeBody {
        transfer: if ctx.container_only() {
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
        } else {
            cadmpeg_ir::report::decode::DecodeTransfer::full(geometry_transferred)
        },
        coverage: coverage_entries.into_iter().collect(),
        losses,
        notes: Vec::new(),
        transfer_ledger: TransferLedger::default(),
    };
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

fn charge_items(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let count = u64::try_from(count).map_err(|_| {
        ctx.refuse_codec_limit(
            "Inventor native collection item count",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    ctx.charge_collection_items(count, operation)
}

fn wire_len(
    ctx: &DecodeContext<'_>,
    len: usize,
    operation: &'static str,
) -> Result<u64, CodecError> {
    u64::try_from(len).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

fn decimal_digits(value: usize) -> usize {
    value
        .checked_ilog10()
        .map_or(1, |digits| digits as usize + 1)
}

fn record_ordinal(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    operation: &'static str,
) -> Result<u32, CodecError> {
    u32::try_from(ordinal)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::from(u32::MAX), u64::MAX))
}

fn charge_retained_len(
    ctx: &DecodeContext<'_>,
    bytes: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let bytes = u64::try_from(bytes).map_err(|_| {
        ctx.refuse_codec_limit("Inventor retained byte count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.charge_retained(bytes, operation)
}

fn retained_clone(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    charge_retained_len(ctx, value.len(), operation)?;
    Ok(value.to_owned())
}

fn retained_format(
    ctx: &DecodeContext<'_>,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    crate::record_issue::admit_formatted(ctx, value, operation)?;
    Ok(value.to_string())
}

fn retained_hex(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, CodecError> {
    let len = bytes.len().checked_mul(2).ok_or_else(|| {
        ctx.refuse_codec_limit("Inventor hexadecimal length", u64::MAX - 1, u64::MAX)
    })?;
    charge_retained_len(ctx, len, operation)?;
    ctx.charge_work(
        u64::try_from(bytes.len()).map_err(|_| {
            ctx.refuse_codec_limit("Inventor hexadecimal work", u64::MAX - 1, u64::MAX)
        })?,
        "encode Inventor hexadecimal bytes",
    )?;
    Ok(hex(bytes))
}

fn retained_sha256(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, CodecError> {
    ctx.charge_retained(64, operation)?;
    ctx.charge_work(
        u64::try_from(bytes.len())
            .map_err(|_| ctx.refuse_codec_limit("Inventor digest work", u64::MAX - 1, u64::MAX))?,
        "hash Inventor native bytes",
    )?;
    Ok(sha256_hex(bytes))
}

fn retained_native_sha256(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<crate::native::digest::Sha256Hex, CodecError> {
    ctx.charge_retained(64, operation)?;
    ctx.charge_work(
        u64::try_from(bytes.len())
            .map_err(|_| ctx.refuse_codec_limit("Inventor digest work", u64::MAX - 1, u64::MAX))?,
        "hash Inventor native bytes",
    )?;
    Ok(crate::native::digest::Sha256Hex::digest(bytes))
}

fn admit_native_format(
    ctx: &DecodeContext<'_>,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    crate::record_issue::admit_formatted(ctx, value, operation)
}

fn admit_native_digest(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_retained(64, operation)?;
    ctx.charge_work(
        wire_len(ctx, bytes.len(), "Inventor digest work")?,
        "hash Inventor native bytes",
    )
}

fn admitted_loss(
    ctx: &DecodeContext<'_>,
    code: InventorLossCode,
    message: std::fmt::Arguments<'_>,
) -> Result<cadmpeg_ir::report::loss::LossNote, CodecError> {
    ctx.charge_collection_items(1, "collect Inventor decode loss")?;
    charge_retained_len(ctx, "inventor".len(), "retain Inventor loss namespace")?;
    charge_retained_len(ctx, code.code().len(), "retain Inventor loss code")?;
    let message = retained_format(ctx, message, "retain Inventor decode loss message")?;
    Ok(code.note(message))
}

fn project_root_product(
    ctx: &DecodeContext<'_>,
    document_kind: &DocumentKind,
    metadata: &MetadataProjection,
    entity_count: usize,
    admitted_entities: &mut u64,
) -> Result<ProductDefinition, CodecError> {
    ctx.charge_collection_items(1, "collect Inventor root product")?;
    let next_count = entity_count.checked_add(1).ok_or_else(|| {
        ctx.refuse_codec_limit("Inventor root product count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.admit_entities(
        wire_len(ctx, next_count, "Inventor root product count")?,
        admitted_entities,
        "admit Inventor root product",
    )?;
    charge_retained_len(
        ctx,
        "inventor:document:product#root".len(),
        "retain Inventor root product id",
    )?;
    let source_name = metadata
        .title
        .as_deref()
        .map(|name| retained_clone(ctx, name, "retain Inventor root product source name"))
        .transpose()?;
    let label = metadata
        .title
        .as_deref()
        .map(|name| retained_clone(ctx, name, "retain Inventor root product label"))
        .transpose()?;
    let description = metadata
        .description
        .as_deref()
        .map(|value| retained_clone(ctx, value, "retain Inventor root product description"))
        .transpose()?;
    let part_number = metadata
        .part_number
        .as_deref()
        .map(|value| retained_clone(ctx, value, "retain Inventor root part number"))
        .transpose()?;
    charge_items(
        ctx,
        metadata.bom_properties.len(),
        "copy Inventor root BOM entries",
    )?;
    for (key, value) in &metadata.bom_properties {
        charge_retained_len(ctx, key.len(), "retain Inventor root BOM key")?;
        charge_retained_len(ctx, value.len(), "retain Inventor root BOM value")?;
    }
    charge_items(
        ctx,
        metadata.bom_properties.len(),
        "name Inventor root BOM entries",
    )?;
    Ok(ProductDefinition {
        id: ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "document", "product"),
            cadmpeg_ir::identity_key!("root"),
        ),
        kind: if *document_kind == DocumentKind::Assembly {
            ProductDefinitionKind::LinkGroup
        } else {
            ProductDefinitionKind::Part
        },
        source_name,
        label,
        description,
        part_number,
        bom_properties: cadmpeg_core::text::named_entries(
            "inventor:document:product#root",
            metadata.bom_properties.clone(),
        )?,
        bodies: Vec::new(),
        native_ref: None,
    })
}

fn insert_source_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: &'static str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "collect Inventor source attribute")?;
    let key = retained_clone(ctx, key, "retain Inventor source attribute key")?;
    let value = retained_format(ctx, value, "retain Inventor source attribute value")?;
    attributes.insert(key, value);
    Ok(())
}

fn admit_coverage_entries(
    ctx: &DecodeContext<'_>,
    keys: impl IntoIterator<Item = cadmpeg_ir::report::decode::CoverageKey>,
) -> Result<(), CodecError> {
    for key in keys {
        ctx.charge_collection_items(1, "collect Inventor coverage measure")?;
        charge_retained_len(ctx, key.as_str().len(), "retain Inventor coverage key")?;
    }
    Ok(())
}

fn admit_kernel_annotation(
    ctx: &DecodeContext<'_>,
    record: &cadmpeg_asm::brep::annotations::AnnotationRecord,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "collect Inventor kernel provenance")?;
    let stream_len = "inventor:"
        .len()
        .checked_add(record.stream.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("Inventor annotation stream length", u64::MAX - 1, u64::MAX)
        })?;
    charge_retained_len(ctx, stream_len, "retain Inventor annotation stream name")?;
    charge_retained_len(ctx, record.id.len(), "retain Inventor annotation entity id")?;
    charge_retained_len(ctx, stream_len, "retain Inventor provenance stream copy")?;
    charge_retained_len(
        ctx,
        record.tag.as_str().len(),
        "retain Inventor annotation tag",
    )?;
    for field in &record.derived_fields {
        ctx.charge_collection_items(2, "collect Inventor derived field annotation")?;
        charge_retained_len(ctx, record.id.len(), "retain Inventor derived entity id")?;
        charge_retained_len(ctx, field.len(), "retain Inventor derived field path")?;
    }
    Ok(())
}

fn admit_untransferred_carrier(
    ctx: &DecodeContext<'_>,
    carrier: &crate::kernel::ActiveCarrier<'_>,
    active_id: &str,
) -> Result<(), CodecError> {
    let token = carrier.segment_token.as_str();
    let ordinal = carrier.record_ordinal;
    ctx.charge_collection_items(1, "collect Inventor retained carrier")?;
    admit_native_format(
        ctx,
        format_args!("RSeStorage/B{token}:expanded"),
        "retain Inventor carrier source stream",
    )?;
    admit_native_format(
        ctx,
        format_args!("RSeStorage/B{token}:expanded"),
        "retain Inventor carrier source stream copy",
    )?;
    charge_retained_len(ctx, token.len(), "retain Inventor carrier identity token")?;
    admit_native_format(
        ctx,
        format_args!("{token}-{ordinal}"),
        "retain Inventor carrier identity key",
    )?;
    admit_native_format(
        ctx,
        format_args!("inventor:kernel:carrier#{token}-{ordinal}"),
        "retain Inventor unknown carrier id",
    )?;
    ctx.charge_collection_items(1, "collect Inventor carrier link")?;
    charge_retained_len(ctx, active_id.len(), "retain Inventor carrier link id")?;
    Ok(())
}

fn admit_kernel_unknown_fidelity(
    ctx: &DecodeContext<'_>,
    fidelity: &SourceFidelity,
    records: &[UnknownRecord],
) -> Result<(), CodecError> {
    for record in records {
        ctx.charge_collection_items(2, "collect Inventor kernel unknown fidelity")?;
        charge_retained_len(
            ctx,
            record.id().as_str().len(),
            "retain Inventor native unknown id copy",
        )?;
        charge_items(
            ctx,
            record.links().len(),
            "copy Inventor native unknown links",
        )?;
        for link in record.links() {
            charge_retained_len(ctx, link.len(), "retain Inventor native unknown link")?;
        }
        if let Some(provenance) = fidelity.annotations.provenance.get(record.id().as_str()) {
            charge_retained_len(
                ctx,
                provenance.stream().len(),
                "retain Inventor unknown provenance stream",
            )?;
        }
    }
    Ok(())
}

fn admit_presentation_native_projection(
    ctx: &DecodeContext<'_>,
    inventory: &crate::presentation::PresentationInventory<'_>,
) -> Result<(), CodecError> {
    for style in &inventory.default_styles {
        let token = style.identity.segment_token.as_str();
        admit_native_format(
            ctx,
            format_args!(
                "inventor:presentation:default-style#{token}-{}",
                style.identity.record_ordinal
            ),
            "retain Inventor default style id",
        )?;
        charge_retained_len(ctx, token.len(), "retain Inventor default style token")?;
        admit_native_digest(
            ctx,
            style.suffix.window(),
            "retain Inventor default style suffix digest",
        )?;
    }
    for style in &inventory.rendering_styles {
        let token = style.identity.segment_token.as_str();
        admit_native_format(
            ctx,
            format_args!(
                "inventor:presentation:rendering-style#{token}-{}",
                style.identity.record_ordinal
            ),
            "retain Inventor rendering style id",
        )?;
        charge_retained_len(ctx, token.len(), "retain Inventor rendering style token")?;
        for value in [&style.name, &style.comment, &style.long_name] {
            charge_retained_len(ctx, value.len(), "retain Inventor rendering style text")?;
        }
        if let Some(extension) = &style.extension {
            for value in [
                &extension.style_label,
                &extension.asset_guid,
                &extension.material_id,
                &extension.asset_library_id,
                &extension.guid,
            ] {
                charge_retained_len(ctx, value.len(), "retain Inventor rendering extension text")?;
            }
        }
        admit_native_digest(
            ctx,
            style.suffix.window(),
            "retain Inventor rendering style suffix digest",
        )?;
        let issue_detail = if style.extension.is_some() != (style.segment_version_major >= 17) {
            Some("rendering style extension disagrees with segment_version_major")
        } else if style.segment_version_major >= 17 && !style.comment.is_empty() {
            Some("rendering style comment must be empty for segment_version_major >= 17")
        } else {
            None
        };
        if let Some(detail) = issue_detail {
            charge_retained_len(
                ctx,
                detail.len(),
                "retain Inventor rendering conversion issue",
            )?;
            ctx.charge_collection_items(1, "collect Inventor rendering conversion issue")?;
            ctx.charge_entities(1, "admit Inventor rendering conversion issue")?;
            charge_retained_len(ctx, token.len(), "retain Inventor rendering issue token")?;
            charge_retained_len(
                ctx,
                detail.len(),
                "retain Inventor rendering issue detail copy",
            )?;
        }
    }
    for face in &inventory.graphics_faces {
        let token = face.identity.segment_token.as_str();
        admit_native_format(
            ctx,
            format_args!(
                "inventor:presentation:graphics-face#{token}-{}",
                face.identity.record_ordinal
            ),
            "retain Inventor graphics face id",
        )?;
        charge_retained_len(ctx, token.len(), "retain Inventor graphics face token")?;
        charge_items(
            ctx,
            face.edge_references.references().len(),
            "copy Inventor graphics face edge references",
        )?;
    }
    for collection in &inventory.graphics_style_collections {
        let token = collection.identity.segment_token.as_str();
        admit_native_format(
            ctx,
            format_args!(
                "inventor:presentation:graphics-style-collection#{token}-{}",
                collection.identity.record_ordinal
            ),
            "retain Inventor graphics style collection id",
        )?;
        charge_retained_len(
            ctx,
            token.len(),
            "retain Inventor graphics style collection token",
        )?;
        charge_items(
            ctx,
            collection.style_references.references().len(),
            "copy Inventor graphics style references",
        )?;
    }
    for style in &inventory.graphics_primary_color_styles {
        let token = style.identity.segment_token.as_str();
        admit_native_format(
            ctx,
            format_args!(
                "inventor:presentation:graphics-primary-color#{token}-{}",
                style.identity.record_ordinal
            ),
            "retain Inventor primary color style id",
        )?;
        charge_retained_len(
            ctx,
            token.len(),
            "retain Inventor primary color style token",
        )?;
    }
    Ok(())
}

fn project_property_set_issue(
    ctx: &DecodeContext<'_>,
    directory_id: u32,
    path: &str,
    detail: &str,
) -> Result<PropertySetIssueRecord, CodecError> {
    let id = retained_format(
        ctx,
        format_args!("inventor:property:set-issue#{directory_id}"),
        "retain Inventor property-set issue id",
    )?;
    let path = retained_clone(ctx, path, "retain Inventor property-set issue path")?;
    let detail = retained_clone(ctx, detail, "retain Inventor property-set issue detail")?;
    Ok(PropertySetIssueRecord {
        id,
        path,
        directory_id,
        detail,
    })
}

fn project_preview_asset(
    ctx: &DecodeContext<'_>,
    admitted_entities: &mut u64,
    ordinal: usize,
    native_id: &str,
    bytes: &[u8],
    media_type: &str,
) -> Result<Asset, CodecError> {
    ctx.charge_collection_items(1, "collect Inventor preview asset")?;
    let next_entities = admitted_entities.checked_add(1).ok_or_else(|| {
        ctx.refuse_codec_limit("Inventor preview entity count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.admit_entities(
        next_entities,
        admitted_entities,
        "admit Inventor preview asset entity",
    )?;
    let key_len = 8_u64 + u64::from(ordinal.checked_ilog10().map_or(1, |digits| digits + 1));
    ctx.charge_retained(key_len, "retain Inventor preview identity key")?;
    ctx.charge_retained(24_u64 + key_len, "retain Inventor preview asset id")?;
    let _ordinal_reservation = ctx.reserve_scoped(
        wire_len(
            ctx,
            decimal_digits(ordinal),
            "Inventor preview ordinal length",
        )?,
        "format Inventor preview ordinal",
    )?;
    charge_retained_len(ctx, native_id.len(), "retain Inventor preview source id")?;
    charge_retained_len(
        ctx,
        "document preview".len(),
        "retain Inventor preview name",
    )?;
    charge_retained_len(ctx, media_type.len(), "retain Inventor preview media type")?;
    if bytes.is_empty() {
        charge_retained_len(
            ctx,
            "asset data must not be empty".len(),
            "retain Inventor empty preview issue",
        )?;
    }
    let data = ctx.copy_retained(bytes, "retain Inventor preview asset")?;
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
    let id = retained_clone(
        ctx,
        "inventor:protein:state#root",
        "retain Inventor Protein state id",
    )?;
    Ok(match state {
        ProteinState::Absent => ProteinRecord::Absent { id },
        ProteinState::Empty { stream } => ProteinRecord::Empty {
            id,
            directory_id: stream.directory_id(),
        },
        ProteinState::Malformed { stream, detail } => ProteinRecord::Malformed {
            id,
            directory_id: stream.directory_id(),
            detail: retained_clone(ctx, detail, "retain Inventor Protein state detail")?,
        },
        ProteinState::Package(package) => {
            let entries = package
                .archive
                .entries()
                .iter()
                .enumerate()
                .map(|(ordinal, entry)| -> Result<_, CodecError> {
                    Ok(ProteinEntryRecord {
                        id: retained_format(
                            ctx,
                            format_args!("inventor:protein:entry#{ordinal}"),
                            "retain Inventor Protein entry id",
                        )?,
                        ordinal: record_ordinal(ctx, ordinal, "Inventor Protein entry ordinal")?,
                        name: retained_clone(
                            ctx,
                            &entry.name,
                            "retain Inventor Protein entry name",
                        )?,
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
    issues: Vec<StructuralIssueRecord>,
}

fn project_protein_records(
    ctx: &DecodeContext<'_>,
    instances: Vec<crate::protein::ProteinInstanceRecords>,
) -> Result<ProteinNativeRecords, CodecError> {
    for instance in &instances {
        admit_native_items(ctx, instance.records.len())?;
        admit_native_items(ctx, instance.rejected.len())?;
    }
    let mut assets = Vec::new();
    let mut rejections = Vec::new();
    let mut issues = Vec::new();
    for instance in instances {
        if instance.records.is_empty() && instance.rejected.is_empty() {
            continue;
        }
        let entry_name_len = u64::try_from(instance.entry_name.len()).map_err(|_| {
            ctx.refuse_codec_limit("Inventor Protein entry name length", u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_work(entry_name_len, "hash Inventor Protein entry name")?;
        let _digest_reservation = ctx.reserve_scoped(64, "hash Inventor Protein entry name")?;
        let entry_digest = sha256_hex(instance.entry_name.as_bytes());
        for asset in instance.records {
            let id = retained_format(
                ctx,
                format_args!("inventor:protein:asset#{}-{}", entry_digest, asset.ordinal),
                "retain Inventor Protein asset id",
            )?;
            let entry_name = retained_clone(
                ctx,
                &instance.entry_name,
                "retain Inventor Protein asset entry name",
            )?;
            let wire = ProteinAssetRecordWire {
                id,
                entry_name,
                ordinal: asset.ordinal,
                asset,
            };
            if let Some(record) = admit_protein_asset(ctx, wire, &mut issues)? {
                assets.push(record);
            }
        }
        for rejected in instance.rejected {
            let id = retained_format(
                ctx,
                format_args!(
                    "inventor:protein:rejection#{}-{}",
                    entry_digest, rejected.ordinal
                ),
                "retain Inventor Protein rejection id",
            )?;
            let entry_name = retained_clone(
                ctx,
                &instance.entry_name,
                "retain Inventor Protein rejection entry name",
            )?;
            let wire = ProteinRejectionRecordWire {
                id,
                entry_name,
                ordinal: rejected.ordinal,
                detail: rejected.detail,
            };
            if let Some(record) = admit_protein_rejection(ctx, wire, &mut issues)? {
                rejections.push(record);
            }
        }
    }
    Ok(ProteinNativeRecords {
        assets,
        rejections,
        issues,
    })
}

fn project_ufrx_state(
    ctx: &DecodeContext<'_>,
    state: &UfrxState<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<UfrxRecord, CodecError> {
    Ok(match state {
        UfrxState::Absent => UfrxRecord::Absent {
            id: retained_clone(
                ctx,
                "inventor:ufrx:state#root",
                "retain Inventor UFRx state id",
            )?,
        },
        UfrxState::Malformed { stream, detail } => UfrxRecord::Malformed {
            id: retained_clone(
                ctx,
                "inventor:ufrx:state#root",
                "retain Inventor UFRx state id",
            )?,
            directory_id: stream.directory_id(),
            detail: retained_clone(ctx, detail, "retain Inventor UFRx state detail")?,
        },
        UfrxState::Unsupported {
            stream,
            schema,
            section_versions,
            source,
            detail,
        } => {
            charge_items(
                ctx,
                section_versions.len(),
                "copy Inventor UFRx section versions",
            )?;
            UfrxRecord::Unsupported {
                id: retained_clone(
                    ctx,
                    "inventor:ufrx:state#root",
                    "retain Inventor UFRx state id",
                )?,
                directory_id: stream.directory_id(),
                schema: *schema,
                section_versions: section_versions.clone(),
                tail_len: wire_len(
                    ctx,
                    source.window().len(),
                    "Inventor UFRx unsupported tail length",
                )?,
                tail_sha256: retained_native_sha256(
                    ctx,
                    source.window(),
                    "retain Inventor UFRx unsupported tail digest",
                )?,
                detail: retained_clone(ctx, detail, "retain Inventor UFRx state detail")?,
            }
        }
        UfrxState::Parsed(document) => {
            let mut model_states = Vec::new();
            for (ordinal, state) in document.model_states.iter().enumerate() {
                if let Some(record) = project_ufrx_model_state(ctx, ordinal, state, issues)? {
                    model_states.push(record);
                }
            }
            let mut references = Vec::new();
            for (ordinal, reference) in document.references.iter().enumerate() {
                if let Some(record) =
                    project_ufrx_external_reference(ctx, ordinal, reference, issues)?
                {
                    references.push(record);
                }
            }
            let mut embedded = Vec::new();
            for (ordinal, reference) in document.embedded_references.iter().enumerate() {
                if let Some(record) =
                    project_ufrx_embedded_reference(ctx, ordinal, reference, issues)?
                {
                    embedded.push(record);
                }
            }
            let mut occurrences = Vec::new();
            for (ordinal, occurrence) in document.occurrences.iter().enumerate() {
                if let Some(record) = project_ufrx_occurrence(ctx, ordinal, occurrence, issues)? {
                    occurrences.push(record);
                }
            }
            let representation = document
                .representation
                .as_ref()
                .map(|state| project_ufrx_representation(ctx, state, issues))
                .transpose()?
                .flatten();
            UfrxRecord::ParsedPrefix {
                id: retained_clone(
                    ctx,
                    "inventor:ufrx:state#root",
                    "retain Inventor UFRx state id",
                )?,
                directory_id: document.stream.directory_id(),
                schema: document.schema,
                section_versions: {
                    charge_items(
                        ctx,
                        document.section_versions.len(),
                        "copy Inventor UFRx section versions",
                    )?;
                    document.section_versions.clone()
                },
                original_file_name: retained_clone(
                    ctx,
                    &document.original_file_name,
                    "retain Inventor UFRx original file name",
                )?,
                caption: retained_clone(ctx, &document.caption, "retain Inventor UFRx caption")?,
                representation,
                model_states,
                external_references: references,
                embedded_references: embedded,
                occurrences,
                tail_len: wire_len(
                    ctx,
                    document.unparsed_tail.window().len(),
                    "Inventor UFRx tail length",
                )?,
                tail_sha256: retained_native_sha256(
                    ctx,
                    document.unparsed_tail.window(),
                    "retain Inventor UFRx tail digest",
                )?,
            }
        }
    })
}

fn project_ufrx_model_state(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    state: &crate::external_reference::UfrxModelState<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<UfrxModelStateRecord>, CodecError> {
    let issue_detail = if state.suffix.window().len() != 77 {
        Some("suffix_len must be 77")
    } else if state.name.chars().all(char::is_whitespace) {
        Some("name must not be empty")
    } else {
        None
    };
    if let Some(detail) = issue_detail {
        charge_retained_len(
            ctx,
            detail.len(),
            "retain Inventor UFRx model-state conversion issue",
        )?;
    }
    charge_items(
        ctx,
        state.parameters.len(),
        "copy Inventor UFRx state parameters",
    )?;
    let parameters = state
        .parameters
        .iter()
        .map(|parameter| -> Result<_, CodecError> {
            Ok(UfrxModelStateParameterRecord {
                name: retained_clone(ctx, &parameter.name, "retain Inventor UFRx parameter name")?,
                tag: parameter.tag,
                kind: parameter.kind,
                state: parameter.state,
                value: retained_clone(
                    ctx,
                    &parameter.value,
                    "retain Inventor UFRx parameter value",
                )?,
                trailer: parameter.trailer,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let admitted = UfrxModelStateRecord::try_from(UfrxModelStateRecordWire {
        id: retained_format(
            ctx,
            format_args!("inventor:ufrx:model-state#{ordinal}"),
            "retain Inventor UFRx model-state id",
        )?,
        ordinal: record_ordinal(ctx, ordinal, "Inventor UFRx model-state ordinal")?,
        prefix: state.prefix,
        name: retained_clone(ctx, &state.name, "retain Inventor UFRx model-state name")?,
        state: state.state,
        prefix_count: state.prefix_count,
        parameters,
        suffix_len: wire_len(
            ctx,
            state.suffix.window().len(),
            "Inventor UFRx model-state suffix length",
        )?,
        suffix_sha256: retained_sha256(
            ctx,
            state.suffix.window(),
            "retain Inventor UFRx model-state digest",
        )?,
    });
    let _scope_reservation = ctx.reserve_scoped(
        wire_len(
            ctx,
            "ufrx-model-state-".len() + decimal_digits(ordinal),
            "Inventor UFRx issue scope length",
        )?,
        "format Inventor UFRx model-state issue scope",
    )?;
    let scope = format!("ufrx-model-state-{ordinal}");
    admit_ufrx_record(ctx, admitted, &scope, issues)
}

fn project_ufrx_external_reference(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    reference: &crate::external_reference::InventorExternalReference,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<ExternalReferenceRecord>, CodecError> {
    if reference.path.chars().all(char::is_whitespace)
        && reference.document_id.iter().all(|byte| *byte == 0)
    {
        charge_retained_len(
            ctx,
            "path or a nonzero document_id is required".len(),
            "retain Inventor UFRx external conversion issue",
        )?;
    }
    charge_items(
        ctx,
        reference.state_groups.len(),
        "copy Inventor UFRx reference state groups",
    )?;
    let admitted = ExternalReferenceRecord::try_from(ExternalReferenceRecordWire {
        id: retained_format(
            ctx,
            format_args!("inventor:ufrx:external-reference#{ordinal}"),
            "retain Inventor UFRx external reference id",
        )?,
        ordinal: record_ordinal(ctx, ordinal, "Inventor UFRx external ordinal")?,
        path: retained_clone(ctx, &reference.path, "retain Inventor UFRx external path")?,
        library_id: reference.library_id,
        library_name: retained_clone(
            ctx,
            &reference.library_name,
            "retain Inventor UFRx external library name",
        )?,
        display_name: retained_clone(
            ctx,
            &reference.display_name,
            "retain Inventor UFRx external display name",
        )?,
        state_groups: reference.state_groups.clone(),
        state: reference.state,
        document_id: Some(retained_hex(
            ctx,
            &reference.document_id,
            "retain Inventor UFRx document id",
        )?),
        database_id: retained_hex(
            ctx,
            &reference.database_id,
            "retain Inventor UFRx database id",
        )?,
        reference_id: reference.reference_id,
        occurrence_count: reference.occurrence_count,
        version: reference.version,
        flags: reference.flags,
    });
    let _scope_reservation = ctx.reserve_scoped(
        wire_len(
            ctx,
            "ufrx-external-reference-".len() + decimal_digits(ordinal),
            "Inventor UFRx issue scope length",
        )?,
        "format Inventor UFRx external issue scope",
    )?;
    let scope = format!("ufrx-external-reference-{ordinal}");
    admit_ufrx_record(ctx, admitted, &scope, issues)
}

fn project_ufrx_embedded_reference(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    reference: &crate::external_reference::InventorEmbeddedReference<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<EmbeddedReferenceRecord>, CodecError> {
    if reference.source.window().is_empty() {
        charge_retained_len(
            ctx,
            "record_len must not be zero".len(),
            "retain Inventor UFRx embedded conversion issue",
        )?;
    }
    let admitted = EmbeddedReferenceRecord::try_from(EmbeddedReferenceRecordWire {
        id: retained_format(
            ctx,
            format_args!("inventor:ufrx:embedded-reference#{ordinal}"),
            "retain Inventor UFRx embedded reference id",
        )?,
        ordinal: record_ordinal(ctx, ordinal, "Inventor UFRx embedded ordinal")?,
        value_0: reference.value_0,
        filetime: reference.filetime,
        value_1: reference.value_1,
        extended_value: reference.extended_value,
        value_2: reference.value_2,
        path: retained_clone(ctx, &reference.path, "retain Inventor UFRx embedded path")?,
        library_id: reference.library_id,
        library_name: retained_clone(
            ctx,
            &reference.library_name,
            "retain Inventor UFRx embedded library name",
        )?,
        state: reference.state,
        display_name: retained_clone(
            ctx,
            &reference.display_name,
            "retain Inventor UFRx embedded display name",
        )?,
        state_values: reference.state_values,
        record_len: wire_len(
            ctx,
            reference.source.window().len(),
            "Inventor UFRx embedded length",
        )?,
        record_sha256: retained_sha256(
            ctx,
            reference.source.window(),
            "retain Inventor UFRx embedded digest",
        )?,
    });
    let _scope_reservation = ctx.reserve_scoped(
        wire_len(
            ctx,
            "ufrx-embedded-reference-".len() + decimal_digits(ordinal),
            "Inventor UFRx issue scope length",
        )?,
        "format Inventor UFRx embedded issue scope",
    )?;
    let scope = format!("ufrx-embedded-reference-{ordinal}");
    admit_ufrx_record(ctx, admitted, &scope, issues)
}

fn project_ufrx_occurrence(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    occurrence: &crate::external_reference::UfrxOccurrence<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<UfrxOccurrenceRecord>, CodecError> {
    let issue_detail = if occurrence.header_padding_words > 8 {
        Some("header_padding_words must not exceed 8")
    } else if occurrence.source.window().is_empty() {
        Some("record_len must not be zero")
    } else {
        None
    };
    if let Some(detail) = issue_detail {
        charge_retained_len(
            ctx,
            detail.len(),
            "retain Inventor UFRx occurrence conversion issue",
        )?;
    }
    let admitted = UfrxOccurrenceRecord::try_from(UfrxOccurrenceRecordWire {
        id: retained_format(
            ctx,
            format_args!("inventor:ufrx:occurrence#{ordinal}"),
            "retain Inventor UFRx occurrence id",
        )?,
        ordinal: record_ordinal(ctx, ordinal, "Inventor UFRx occurrence ordinal")?,
        end_string_flag: occurrence.end_string_flag,
        file_reference_id: occurrence.file_reference_id,
        occurrence_id: occurrence.occurrence_id,
        header_value: occurrence.header_value,
        title: occurrence
            .title
            .as_deref()
            .map(|title| retained_clone(ctx, title, "retain Inventor UFRx occurrence title"))
            .transpose()?,
        header_padding_words: occurrence.header_padding_words,
        record_len: wire_len(
            ctx,
            occurrence.source.window().len(),
            "Inventor UFRx occurrence length",
        )?,
        record_sha256: retained_sha256(
            ctx,
            occurrence.source.window(),
            "retain Inventor UFRx occurrence digest",
        )?,
    });
    let _scope_reservation = ctx.reserve_scoped(
        wire_len(
            ctx,
            "ufrx-occurrence-".len() + decimal_digits(ordinal),
            "Inventor UFRx issue scope length",
        )?,
        "format Inventor UFRx occurrence issue scope",
    )?;
    let scope = format!("ufrx-occurrence-{ordinal}");
    admit_ufrx_record(ctx, admitted, &scope, issues)
}

fn project_ufrx_representation(
    ctx: &DecodeContext<'_>,
    state: &crate::external_reference::UfrxRepresentationState,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<UfrxRepresentationRecord>, CodecError> {
    let issue_detail = if let Some((name, kind)) = &state.active_representation {
        if name.chars().all(char::is_whitespace) {
            Some("active_representation must not be empty")
        } else if kind.chars().all(char::is_whitespace) {
            Some("active_representation_kind must not be empty")
        } else {
            None
        }
    } else {
        None
    };
    let issue_detail = issue_detail.or_else(|| {
        state
            .active_model_state
            .chars()
            .all(char::is_whitespace)
            .then_some("active_model_state must not be empty")
    });
    if let Some(detail) = issue_detail {
        charge_retained_len(
            ctx,
            detail.len(),
            "retain Inventor UFRx representation conversion issue",
        )?;
    }
    let (active_representation, active_representation_kind) =
        match state.active_representation.as_ref() {
            Some((name, kind)) => (
                Some(retained_clone(
                    ctx,
                    name,
                    "retain Inventor UFRx representation name",
                )?),
                Some(retained_clone(
                    ctx,
                    kind,
                    "retain Inventor UFRx representation kind",
                )?),
            ),
            None => (None, None),
        };
    let wire = UfrxRepresentationRecordWire {
        prefix: state.prefix,
        active_representation,
        active_representation_kind,
        secondary_active_lod_state: state.secondary_active_lod_state,
        active_model_state: retained_clone(
            ctx,
            &state.active_model_state,
            "retain Inventor UFRx active model state",
        )?,
        active_model_state_state: state.active_model_state_state,
    };
    admit_ufrx_record(
        ctx,
        UfrxRepresentationRecord::try_from(wire),
        "ufrx-representation",
        issues,
    )
}

fn admit_native_items(ctx: &DecodeContext<'_>, count: usize) -> Result<(), CodecError> {
    let count = u64::try_from(count).map_err(|_| {
        ctx.refuse_codec_limit("Inventor native record count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.charge_collection_items(count, "retain Inventor native structural records")?;
    ctx.charge_entities(count, "admit Inventor native structural records")
}

fn admit_native_record_items(
    ctx: &DecodeContext<'_>,
    container: &InventorContainer<'_>,
    assembly: &crate::assembly::AssemblyInventory<'_>,
    presentation: &crate::presentation::PresentationInventory<'_>,
    design: &crate::design::DesignInventory,
    sketch: &crate::sketch::SketchInventory,
    feature: &crate::feature::FeatureInventory,
) -> Result<(), CodecError> {
    admit_native_items(ctx, 3)?;
    for descriptor in &container.property_sets {
        admit_native_items(ctx, 1)?;
        if let PropertySetState::Parsed(property_set) = &descriptor.state {
            for section in &property_set.sections {
                ctx.charge_work(1, "count Inventor native property sections")?;
                admit_native_items(ctx, 1)?;
                admit_native_items(ctx, section.properties.len())?;
            }
        }
    }
    if let ProteinState::Package(package) = &container.protein {
        admit_native_items(ctx, package.archive.entries().len())?;
    }
    if let UfrxState::Parsed(document) = &container.ufrx {
        for state in &document.model_states {
            ctx.charge_work(1, "count Inventor native UFRx states")?;
            admit_native_items(ctx, state.parameters.len())?;
        }
        for reference in &document.references {
            ctx.charge_work(1, "count Inventor native UFRx references")?;
            charge_items(
                ctx,
                reference.state_groups.len(),
                "retain Inventor native UFRx state groups",
            )?;
        }
        for count in [
            document.model_states.len(),
            document.references.len(),
            document.embedded_references.len(),
            document.occurrences.len(),
        ] {
            admit_native_items(ctx, count)?;
        }
    }
    if let UfrxState::Unsupported {
        section_versions, ..
    } = &container.ufrx
    {
        charge_items(
            ctx,
            section_versions.len(),
            "retain Inventor native UFRx section versions",
        )?;
    }
    admit_native_items(ctx, container.rse.databases.len())?;
    admit_native_items(ctx, container.rse.databases.len())?;
    match &container.rse.registry {
        ParsedState::Parsed(registry) => admit_native_items(ctx, registry.entries.len())?,
        ParsedState::Unavailable(_) => admit_native_items(ctx, 1)?,
        ParsedState::Absent => {}
    }
    match &container.rse.revisions {
        ParsedState::Parsed(table) => admit_native_items(ctx, table.entries.len())?,
        ParsedState::Unavailable(_) => admit_native_items(ctx, 1)?,
        ParsedState::Absent => {}
    }
    for segment in &container.rse.segments {
        ctx.charge_work(1, "count Inventor native segment records")?;
        admit_native_items(ctx, 3)?;
        admit_native_items(ctx, segment.identity_issues.len())?;
        if let SegmentMetaState::Parsed(meta) = &segment.meta {
            admit_native_items(ctx, meta.tables.sections.len())?;
            admit_native_items(ctx, meta.tables.types.len())?;
        }
        if let SegmentBulkState::Framed(bulk) = &segment.bulk {
            if let RecordFrameState::Framed(table) = &bulk.records {
                admit_native_items(ctx, table.records.len())?;
            }
        }
    }
    for count in [
        container.rse.unpaired_metadata.len(),
        container.rse.unpaired_bulk.len(),
        assembly.occurrences.len(),
        assembly.placements.len(),
        assembly.issues.len(),
        presentation.default_styles.len(),
        presentation.rendering_styles.len(),
        presentation.graphics_faces.len(),
        presentation.graphics_style_collections.len(),
        presentation.graphics_primary_color_styles.len(),
        presentation.issues.len(),
        design.parameters.len(),
        design.expressions.len(),
        design.units.len(),
        design.issues.len(),
        sketch.sketches.len(),
        sketch.entities.len(),
        sketch.transforms.len(),
        sketch.directions.len(),
        sketch.constraints.len(),
        sketch.issues.len(),
        feature.features.len(),
        feature.pattern_features.len(),
        feature.terminators.len(),
        feature.properties.len(),
        feature.labels.len(),
        feature.entity_style_links.len(),
        feature.issues.len(),
    ] {
        admit_native_items(ctx, count)?;
    }
    for occurrence in &assembly.occurrences {
        ctx.charge_work(1, "count Inventor native occurrence references")?;
        charge_items(
            ctx,
            occurrence.related_references.len(),
            "retain Inventor native occurrence references",
        )?;
    }
    Ok(())
}

fn collect_body_ids<'b>(
    ctx: &DecodeContext<'_>,
    ids: impl IntoIterator<Item = &'b BodyId>,
) -> Result<Vec<BodyId>, CodecError> {
    let mut output = Vec::new();
    for id in ids {
        ctx.charge_collection_items(1, "collect Inventor projected body ids")?;
        charge_retained_len(ctx, id.as_str().len(), "retain Inventor projected body id")?;
        output.push(id.clone());
    }
    Ok(output)
}

fn clone_product_body_ids(
    ctx: &DecodeContext<'_>,
    body_ids: &[BodyId],
    target: &mut Vec<BodyId>,
) -> Result<(), CodecError> {
    charge_items(ctx, body_ids.len(), "collect Inventor product body ids")?;
    for body_id in body_ids {
        charge_retained_len(
            ctx,
            body_id.as_str().len(),
            "retain Inventor product body id",
        )?;
    }
    target.clear();
    target.extend(body_ids.iter().cloned());
    Ok(())
}

fn index_projected_colors<'b>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = (&'b AppearanceId, Color)>,
) -> Result<HashMap<AppearanceId, Color>, CodecError> {
    let mut output = HashMap::new();
    for (id, color) in entries {
        ctx.charge_collection_items(1, "index Inventor projected appearance colors")?;
        charge_retained_len(
            ctx,
            id.as_str().len(),
            "retain Inventor projected appearance color id",
        )?;
        output.insert(id.clone(), color);
    }
    Ok(output)
}

fn index_face_colors<'b>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = (&'b FaceId, Color)>,
) -> Result<HashMap<FaceId, Color>, CodecError> {
    let mut output = HashMap::new();
    for (id, color) in entries {
        ctx.charge_collection_items(1, "index Inventor face colors")?;
        charge_retained_len(ctx, id.as_str().len(), "retain Inventor face color id")?;
        output.insert(id.clone(), color);
    }
    Ok(output)
}

fn index_asm_face_keys<'b>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = (&'b FaceId, u64)>,
) -> Result<HashMap<FaceId, u64>, CodecError> {
    let mut output = HashMap::new();
    for (id, key) in entries {
        ctx.charge_collection_items(1, "index Inventor ASM face keys")?;
        charge_retained_len(ctx, id.as_str().len(), "retain Inventor ASM face key id")?;
        output.insert(id.clone(), key);
    }
    Ok(output)
}

fn version_record(
    ctx: &DecodeContext<'_>,
    version: VersionTuple,
) -> Result<VersionTupleRecord, CodecError> {
    Ok(VersionTupleRecord {
        revision: version.revision,
        minor: version.minor,
        major: version.major,
        state: retained_hex(
            ctx,
            &version.state,
            "retain Inventor database version state",
        )?,
    })
}

fn apply_kernel_header(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    family: crate::kernel::KernelFamily,
    header: &cadmpeg_asm::kernel_header::KernelHeader,
) -> Result<(), CodecError> {
    let Some(source) = ir.source.as_mut() else {
        return Ok(());
    };
    if let Some(version) = header.save_format_version {
        let value = admitted_kernel_attribute(
            ctx,
            "kernel_save_format_version",
            format_args!("{version}"),
        )?;
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_save_format_version"),
            value,
        );
    }
    if let Some(count) = header.entity_count {
        let value = admitted_kernel_attribute(ctx, "kernel_entity_count", format_args!("{count}"))?;
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_entity_count"),
            value,
        );
    }
    if let Some(flags) = header.flags {
        let value = admitted_kernel_attribute(ctx, "kernel_flags", format_args!("{flags}"))?;
        source
            .attributes
            .insert(cadmpeg_core::nonblank_literal!("kernel_flags"), value);
    }
    if let Some(family) = &header.product_family {
        let value =
            admitted_kernel_attribute(ctx, "kernel_product_family", format_args!("{family}"))?;
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_product_family"),
            value,
        );
    }
    if let Some(version) = &header.product_version {
        let value =
            admitted_kernel_attribute(ctx, "kernel_product_version", format_args!("{version}"))?;
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("kernel_product_version"),
            value,
        );
    }
    if let (Some(linear), Some(angular)) = (header.linear, header.angular) {
        ir.tolerances = Tolerances::new(linear * 10.0, angular).map_err(CodecError::Malformed)?;
    }
    let value =
        admitted_kernel_attribute(ctx, "kernel_family", format_args!("{}", family.label()))?;
    source
        .attributes
        .insert(cadmpeg_core::nonblank_literal!("kernel_family"), value);
    Ok(())
}

fn admitted_kernel_attribute(
    ctx: &DecodeContext<'_>,
    key: &'static str,
    value: std::fmt::Arguments<'_>,
) -> Result<String, CodecError> {
    ctx.charge_collection_items(1, "collect Inventor kernel attribute")?;
    charge_retained_len(ctx, key.len(), "format Inventor kernel attribute key")?;
    charge_retained_len(ctx, key.len(), "retain Inventor kernel attribute key")?;
    retained_format(ctx, value, "retain Inventor kernel attribute value")
}

fn admit_ufrx_record<T>(
    ctx: &DecodeContext<'_>,
    admitted: Result<T, String>,
    scope: &str,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<T>, CodecError> {
    match admitted {
        Ok(record) => Ok(Some(record)),
        Err(detail) => {
            issues.push(structural_issue(ctx, scope, &detail)?);
            Ok(None)
        }
    }
}

fn admit_protein_asset(
    ctx: &DecodeContext<'_>,
    wire: ProteinAssetRecordWire,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<ProteinAssetRecord>, CodecError> {
    let issue_detail = if wire.ordinal != wire.asset.ordinal {
        Some("ordinal disagrees with asset.ordinal")
    } else if !wire.entry_name.ends_with("InstanceProperties.bin") {
        Some("entry_name must end with InstanceProperties.bin")
    } else {
        None
    };
    if let Some(detail) = issue_detail {
        charge_retained_len(
            ctx,
            detail.len(),
            "retain Inventor Protein asset conversion issue",
        )?;
    }
    let scope_len = u64::try_from(wire.id.len()).map_err(|_| {
        ctx.refuse_codec_limit(
            "Inventor Protein issue scope length",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let _scope_reservation =
        ctx.reserve_scoped(scope_len, "copy Inventor Protein asset issue scope")?;
    let scope = wire.id.clone();
    admit_ufrx_record(ctx, ProteinAssetRecord::try_from(wire), &scope, issues)
}

fn admit_protein_rejection(
    ctx: &DecodeContext<'_>,
    wire: ProteinRejectionRecordWire,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<ProteinRejectionRecord>, CodecError> {
    let issue_detail = if !wire.entry_name.ends_with("InstanceProperties.bin") {
        Some("entry_name must end with InstanceProperties.bin")
    } else if wire.detail.chars().all(char::is_whitespace) {
        Some("detail must not be empty")
    } else {
        None
    };
    if let Some(detail) = issue_detail {
        charge_retained_len(
            ctx,
            detail.len(),
            "retain Inventor Protein rejection conversion issue",
        )?;
    }
    let scope_len = u64::try_from(wire.id.len()).map_err(|_| {
        ctx.refuse_codec_limit(
            "Inventor Protein issue scope length",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let _scope_reservation =
        ctx.reserve_scoped(scope_len, "copy Inventor Protein rejection issue scope")?;
    let scope = wire.id.clone();
    admit_ufrx_record(ctx, ProteinRejectionRecord::try_from(wire), &scope, issues)
}

fn admit_assembly_placement(
    ctx: &DecodeContext<'_>,
    wire: AssemblyPlacementRecordWire,
    issues: &mut Vec<RecordIssue>,
) -> Result<Option<AssemblyPlacementRecord>, CodecError> {
    let failure_detail = if wire.suffix_len == 0 {
        Some("suffix_len must not be zero")
    } else if wire.suffix_sha256.len() != 64
        || !wire
            .suffix_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        Some("suffix_sha256: SHA-256 digest must contain 64 hexadecimal characters")
    } else {
        None
    };
    if let Some(detail) = failure_detail {
        charge_retained_len(
            ctx,
            detail.len(),
            "retain Inventor placement conversion issue",
        )?;
    }
    let _token_reservation = ctx.reserve_scoped(
        wire_len(
            ctx,
            wire.segment_token.len(),
            "Inventor placement issue token length",
        )?,
        "copy Inventor placement issue token",
    )?;
    let segment_token = wire.segment_token.clone();
    let record_ordinal = wire.record_ordinal;
    match AssemblyPlacementRecord::try_from(wire) {
        Ok(record) => Ok(Some(record)),
        Err(detail) => {
            ctx.charge_collection_items(1, "collect Inventor placement conversion issue")?;
            ctx.charge_entities(1, "admit Inventor placement conversion issue")?;
            charge_retained_len(
                ctx,
                segment_token.len(),
                "retain Inventor placement issue token",
            )?;
            issues.push(RecordIssue {
                family: RecordIssueFamily::Assembly,
                segment_token,
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
    ctx.charge_collection_items(1, "collect Inventor structural issue")?;
    ctx.charge_entities(1, "admit Inventor structural issue")?;
    Ok(StructuralIssueRecord {
        id: retained_format(
            ctx,
            format_args!("inventor:rse:structural-issue#{scope}"),
            "retain Inventor structural issue id",
        )?,
        scope: retained_clone(ctx, scope, "retain Inventor structural issue scope")?,
        detail: retained_clone(ctx, detail, "retain Inventor structural issue detail")?,
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
        ctx: &DecodeContext<'_>,
        fmtid: &[u8; 16],
        property_id: u32,
        name: Option<&str>,
        value: Option<&str>,
        native_id: &str,
    ) -> Result<(), CodecError> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(());
        };
        let normalized = name
            .map(|name| normalize_property_name(ctx, name))
            .transpose()?;
        if matches!(normalized.as_deref(), Some("documentkind" | "documenttype")) {
            self.document_kind = DocumentKind::parse_property(value);
            if self.document_kind.is_some() {
                return Ok(());
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
                charge_retained_len(ctx, value.len(), "retain Inventor metadata value")?;
                *target = Some(value.into());
            } else if target.as_deref() != Some(value) {
                charge_items(ctx, 1, "collect Inventor BOM property")?;
                charge_retained_len(ctx, native_id.len(), "retain Inventor BOM property key")?;
                charge_retained_len(ctx, value.len(), "retain Inventor BOM property value")?;
                self.bom_properties.insert(native_id.into(), value.into());
            }
            return Ok(());
        }
        if let Some(name) = name {
            charge_items(ctx, 1, "collect Inventor BOM property")?;
            charge_retained_len(ctx, name.len(), "retain Inventor BOM property key")?;
            charge_retained_len(ctx, value.len(), "retain Inventor BOM property value")?;
            self.bom_properties.insert(name.into(), value.into());
        } else {
            self.unmapped += 1;
        }
        Ok(())
    }

    fn apply_attributes(
        &self,
        ctx: &DecodeContext<'_>,
        attributes: &mut BTreeMap<String, String>,
    ) -> Result<(), CodecError> {
        for (name, value) in [
            ("title", &self.title),
            ("author", &self.author),
            ("description", &self.description),
            ("part_number", &self.part_number),
        ] {
            if let Some(value) = value {
                charge_items(ctx, 1, "collect Inventor metadata attribute")?;
                charge_retained_len(ctx, name.len(), "retain Inventor metadata attribute key")?;
                charge_retained_len(ctx, value.len(), "retain Inventor metadata attribute value")?;
                attributes.insert(name.into(), value.clone());
            }
        }
        Ok(())
    }
}

fn normalize_property_name(ctx: &DecodeContext<'_>, name: &str) -> Result<String, CodecError> {
    ctx.charge_work(
        u64::try_from(name.len()).map_err(|_| {
            ctx.refuse_codec_limit("Inventor property name length", u64::MAX - 1, u64::MAX)
        })?,
        "normalize Inventor property name",
    )?;
    let normalized_len = name
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .try_fold(0_usize, |len, character| {
            len.checked_add(character.len_utf8())
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "Inventor normalized property name length",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    charge_retained_len(
        ctx,
        normalized_len,
        "retain Inventor normalized property name",
    )?;
    Ok(name
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect())
}

fn property_set_name(
    ctx: &DecodeContext<'_>,
    section: &PropertySection<'_>,
) -> Result<Option<String>, CodecError> {
    for property in &section.properties {
        ctx.charge_work(1, "scan Inventor property set name")?;
        if property.id == 255 {
            return property.value.scalar_text(ctx);
        }
    }
    Ok(None)
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

fn is_preview(
    ctx: &DecodeContext<'_>,
    fmtid: &[u8; 16],
    property_id: u32,
    name: Option<&str>,
) -> Result<bool, CodecError> {
    if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 17 {
        return Ok(true);
    }
    match name {
        Some(name) => Ok(matches!(
            normalize_property_name(ctx, name)?.as_str(),
            "thumbnail" | "preview" | "previewimage"
        )),
        None => Ok(false),
    }
}

fn preview_bytes<'a>(value: &'a PropertyValue<'a>) -> Option<(&'a [u8], &'static str)> {
    let bytes = match value {
        PropertyValue::Binary { value: view, .. } => view.window(),
        PropertyValue::Clipboard { format, data, .. } if *format == u32::MAX => {
            let bytes = data.window();
            let mut header = View::over_retained(bytes);
            let image_kind = header.u32_le()?;
            let header_size = header.u16_le()?;
            let width = header.u16_le()? as u32;
            let height = header.u16_le()? as u32;
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
