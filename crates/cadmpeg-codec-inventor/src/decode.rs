// SPDX-License-Identifier: Apache-2.0
//! High-level Inventor structural decode.

mod presentation_native_projection;
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
use cadmpeg_ir::ids::{FaceId, ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::decode::TransferLedger;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::units::Tolerances;
use cadmpeg_ir::{AnnotationBuilder, SourceFidelity, UnknownRecord};

use crate::container::InventorContainer;
use crate::database::{RevisionPayload, VersionTuple};
use crate::dialect::{dialect_loss, DialectRecovery};
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
    ActiveCarrierRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecord,
    AssemblyPlacementRecordWire, DatabaseIssueRecord, DatabaseRecord, PropertyRecord,
    PropertySectionRecord, PropertySetIssueRecord, PropertySetRecord, PropertyValueKind,
    RevisionPayloadForm, RevisionRecord, SegmentRegistryRecord, StorageBandRecord,
    StructuralIssueRecord, VersionTupleRecord,
};
use crate::property_set::{PropertySection, PropertySetState, PropertyValue};
use crate::protein::ProteinState;
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{DatabaseState, DocumentKind, ParsedState};

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
    let mut recovery_storage = ctx.reserve_scoped(0, "collect Inventor dialect declarations")?;
    let recovery = recovery_storage.with_storage(|| DialectRecovery::of(ctx, container))?;
    let matched = recovery.classify(ctx)?;
    let (dialects, kernel_loss) =
        crate::dialect::layers(ctx, &matched, &container.rse.active_carrier)?;
    let mut assembly_inventory = crate::assembly::inventory(ctx, &container.rse)?;
    let mut presentation_inventory = crate::presentation::inventory(ctx, &container.rse)?;
    let design_inventory = crate::design::inventory(ctx, &container.rse)?;
    let sketch_inventory = crate::sketch::inventory(ctx, &container.rse)?;
    let feature_inventory = crate::feature::inventory(ctx, &container.rse)?;
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
    admitted_entities = cadmpeg_core::decode::u64_from_index(ir.model.entity_count());
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
    let mut property_set_descriptors = container.property_sets.iter();
    while let Some(descriptor) =
        ctx.next_charged(&mut property_set_descriptors, "visit Inventor decode items")?
    {
        match &descriptor.state {
            PropertySetState::Malformed(detail) => {
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                ctx.push_vec(
                    &mut property_set_issues,
                    project_property_set_issue(
                        ctx,
                        descriptor.stream.directory_id(),
                        &descriptor.path,
                        detail,
                    )?,
                    "retain Inventor native structural records",
                )?;
            }
            PropertySetState::Parsed(property_set) => {
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                let id = ctx.format_retained(
                    format_args!("inventor:property:set#{}", descriptor.stream.directory_id()),
                    "retain Inventor property-set id",
                )?;
                ctx.push_vec(
                    &mut property_sets,
                    PropertySetRecord {
                        id,
                        path: ctx.copy_retained_text(
                            &descriptor.path,
                            "retain Inventor property-set path",
                        )?,
                        directory_id: descriptor.stream.directory_id(),
                        version: property_set.version,
                        system_identifier: property_set.system_identifier,
                        clsid: retained_hex(
                            ctx,
                            &property_set.clsid,
                            "retain Inventor property-set CLSID",
                        )?,
                        section_count: cadmpeg_core::decode::u64_from_index(
                            property_set.sections.len(),
                        ),
                    },
                    "retain Inventor native structural records",
                )?;
                let mut sections = property_set.sections.iter().enumerate();
                while let Some((section_ordinal, section)) =
                    ctx.next_charged(&mut sections, "visit Inventor decode items")?
                {
                    let mut set_name_storage =
                        ctx.reserve_scoped(0, "read Inventor property set name")?;
                    let set_name =
                        set_name_storage.with_storage(|| property_set_name(ctx, section))?;
                    let identity_matches = set_name
                        .as_deref()
                        .and_then(known_property_set_fmtid)
                        .is_none_or(|expected| expected == section.fmtid);
                    if !identity_matches {
                        ctx.charge_entities(1, "admit Inventor native structural records")?;
                        let id = ctx.format_retained(
                            format_args!(
                                "inventor:property:set-identity#{}-{section_ordinal}",
                                descriptor.stream.directory_id()
                            ),
                            "retain Inventor property-set identity issue id",
                        )?;
                        ctx.charge_retained(
                            cadmpeg_core::decode::u64_from_index(
                                "embedded property-set name does not match its FMTID".len(),
                            ),
                            "retain Inventor property-set identity issue detail",
                        )?;
                        ctx.push_vec(
                            &mut property_set_issues,
                            PropertySetIssueRecord {
                                id,
                                path: ctx.copy_retained_text(
                                    &descriptor.path,
                                    "retain Inventor property-set identity issue path",
                                )?,
                                directory_id: descriptor.stream.directory_id(),
                                detail: "embedded property-set name does not match its FMTID"
                                    .into(),
                            },
                            "retain Inventor native structural records",
                        )?;
                    }
                    ctx.charge_entities(1, "admit Inventor native structural records")?;
                    let section_id = ctx.format_retained(
                        format_args!(
                            "inventor:property:section#{}-{section_ordinal}",
                            descriptor.stream.directory_id()
                        ),
                        "retain Inventor property section id",
                    )?;
                    ctx.push_vec(
                        &mut property_sections,
                        PropertySectionRecord {
                            id: section_id,
                            set_path: ctx.copy_retained_text(
                                &descriptor.path,
                                "retain Inventor property section path",
                            )?,
                            ordinal: record_ordinal(
                                ctx,
                                section_ordinal,
                                "Inventor property section ordinal",
                            )?,
                            fmtid: retained_hex(
                                ctx,
                                &section.fmtid,
                                "retain Inventor property section FMTID",
                            )?,
                            code_page: section.code_page,
                            offsets_ordered: section.offsets_ordered,
                            dictionary_entries: cadmpeg_core::decode::u64_from_index(
                                section.dictionary_entries,
                            ),
                            property_count: cadmpeg_core::decode::u64_from_index(
                                section.properties.len(),
                            ),
                        },
                        "retain Inventor native structural records",
                    )?;
                    let mut section_properties = section.properties.iter();
                    while let Some(property) =
                        ctx.next_charged(&mut section_properties, "visit Inventor decode items")?
                    {
                        ctx.charge_entities(1, "admit Inventor native structural records")?;
                        let property_name = property
                            .name
                            .as_deref()
                            .or_else(|| {
                                identity_matches
                                    .then_some(set_name.as_deref())
                                    .flatten()
                                    .and_then(|set_name| {
                                        built_in_property_name(set_name, property.id)
                                    })
                            })
                            .map(|name| {
                                ctx.copy_retained_text(name, "retain Inventor property name")
                            })
                            .transpose()?;
                        let native_id = ctx.format_retained(
                            format_args!(
                                "inventor:property:value#{}-{section_ordinal}-{}",
                                descriptor.stream.directory_id(),
                                property.id
                            ),
                            "retain Inventor property value id",
                        )?;
                        let scalar_value = property.value.scalar_text(ctx)?;
                        let classified_name = property_name
                            .as_deref()
                            .map(|name| PropertyName::classify(ctx, name))
                            .transpose()?;
                        metadata.consider(
                            ctx,
                            &section.fmtid,
                            property.id,
                            classified_name,
                            scalar_value.as_deref(),
                            &native_id,
                        )?;
                        if is_preview(&section.fmtid, property.id, classified_name) {
                            if let Some((bytes, media_type)) = preview_bytes(&property.value) {
                                let ordinal = ir.model.assets.len();
                                let asset = project_preview_asset(
                                    ctx,
                                    &mut admitted_entities,
                                    ordinal,
                                    &native_id,
                                    bytes,
                                    media_type,
                                )?;
                                ctx.push_vec(
                                    &mut ir.model.assets,
                                    asset,
                                    "collect Inventor preview asset",
                                )?;
                            }
                        }
                        ctx.push_vec(
                            &mut properties,
                            PropertyRecord {
                                id: native_id,
                                set_path: ctx.copy_retained_text(
                                    &descriptor.path,
                                    "retain Inventor property value path",
                                )?,
                                section_ordinal: record_ordinal(
                                    ctx,
                                    section_ordinal,
                                    "Inventor property section ordinal",
                                )?,
                                fmtid: retained_hex(
                                    ctx,
                                    &section.fmtid,
                                    "retain Inventor property section FMTID",
                                )?,
                                property_id: property.id,
                                name: property_name,
                                value_kind: property_value_kind(&property.value),
                                scalar_value,
                                raw_len: cadmpeg_core::decode::u64_from_index(
                                    property.raw.window().len(),
                                ),
                                raw_sha256:
                                    cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                                        ctx,
                                        property.raw.window(),
                                        "retain Inventor property raw digest",
                                    )?,
                            },
                            "retain Inventor native structural records",
                        )?;
                    }
                }
            }
        }
    }
    let protein = project_protein_state(ctx, &container.protein)?;
    let (protein_instances, protein_semantic_issue) = match &container.protein {
        ProteinState::Package(package) => {
            crate::protein::decode_instances_with_issue(ctx, package)?
        }
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
    ctx.extend_vec(
        &mut structural_issues,
        ufrx_issues,
        "retain Inventor native structural records",
    )?;
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
    ir.source = Some(SourceMeta::classified(
        dialects,
        cadmpeg_core::text::named_entries_for_decode(ctx, "the inventor document", attributes)?,
    ));
    if matches!(document_kind, DocumentKind::Part | DocumentKind::Assembly) {
        let entity_count = ir.model.entity_count();
        ctx.push_vec(
            &mut ir.model.product_definitions,
            project_root_product(
                ctx,
                &document_kind,
                &mut metadata,
                entity_count,
                &mut admitted_entities,
            )?,
            "retain Inventor native structural records",
        )?;
    }
    let storage_bands = ctx.try_collect_vec(
        container
            .rse
            .databases
            .iter()
            .map(|database| -> Result<_, CodecError> {
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                Ok(StorageBandRecord {
                    id: ctx.format_retained(
                        format_args!("inventor:rse:storage-band#v{}", database.band.value()),
                        "retain Inventor storage band id",
                    )?,
                    band: database.band.value(),
                    database_directory_id: database.stream.directory_id(),
                })
            }),
        "retain Inventor storage_bands records",
    )?;
    let databases = ctx.try_collect_vec(
        ctx.admit_iter(&container.rse.databases, "visit Inventor databases records")?
            .filter_map(|descriptor| {
                let DatabaseState::Parsed(database) = &descriptor.state else {
                    return None;
                };
                Some((descriptor, database))
            })
            .map(|(descriptor, database)| -> Result<_, CodecError> {
                ctx.charge_entities(1, "admit Inventor native structural records")?;
                Ok(DatabaseRecord {
                    id: ctx.format_retained(
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
                    note: ctx
                        .copy_retained_text(&database.note, "retain Inventor database note")?,
                })
            }),
        "retain Inventor databases records",
    )?;
    let mut database_issues = Vec::new();
    let mut database_descriptors = container.rse.databases.iter();
    while let Some(descriptor) =
        ctx.next_charged(&mut database_descriptors, "visit Inventor decode items")?
    {
        if let Some(detail) = descriptor.issue_detail(ctx)? {
            ctx.charge_entities(1, "admit Inventor native structural records")?;
            ctx.push_vec(
                &mut database_issues,
                DatabaseIssueRecord {
                    id: ctx.format_retained(
                        format_args!("inventor:rse:database-issue#v{}", descriptor.band.value()),
                        "retain Inventor database issue id",
                    )?,
                    band: descriptor.band.value(),
                    detail,
                },
                "retain Inventor native structural records",
            )?;
        }
    }
    let segment_registry =
        match &container.rse.registry {
            ParsedState::Parsed(registry) => ctx.try_collect_vec(
                registry.entries.iter().enumerate().map(
                    |(ordinal, entry)| -> Result<_, CodecError> {
                        ctx.charge_entities(1, "admit Inventor native structural records")?;
                        Ok(SegmentRegistryRecord {
                            id: ctx.format_retained(
                                format_args!("inventor:rse:registry-entry#{ordinal}"),
                                "retain Inventor registry entry id",
                            )?,
                            ordinal: record_ordinal(ctx, ordinal, "Inventor registry ordinal")?,
                            display_name: ctx.copy_retained_text(
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
                            type_name: ctx.copy_retained_text(
                                &entry.type_name,
                                "retain Inventor registry type name",
                            )?,
                            object_count: cadmpeg_core::decode::u64_from_index(entry.objects.len()),
                            node_count: cadmpeg_core::decode::u64_from_index(entry.nodes.len()),
                        })
                    },
                ),
                "retain Inventor segment_registry records",
            )?,
            ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
        };
    let revisions = match &container.rse.revisions {
        ParsedState::Parsed(table) => ctx.try_collect_vec(
            table
                .entries
                .iter()
                .enumerate()
                .map(|(ordinal, entry)| -> Result<_, CodecError> {
                    ctx.charge_entities(1, "admit Inventor native structural records")?;
                    Ok(RevisionRecord {
                        id: ctx.format_retained(
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
                }),
            "retain Inventor revisions records",
        )?,
        ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
    };
    if let ParsedState::Unavailable(detail) = &container.rse.registry {
        structural_issue(
            ctx,
            &mut structural_issues,
            format_args!("segment_registry"),
            detail,
        )?;
    }
    if let ParsedState::Unavailable(detail) = &container.rse.revisions {
        structural_issue(
            ctx,
            &mut structural_issues,
            format_args!("revision_table"),
            detail,
        )?;
    }
    let projection = rse_native_projection::project(ctx, container)?;
    ctx.extend_vec(
        &mut structural_issues,
        projection.identity_issues,
        "retain Inventor native structural records",
    )?;
    let segment_pairs = projection.segment_pairs;
    let segment_meta = projection.segment_meta;
    let meta_sections = projection.meta_sections;
    let meta_types = projection.meta_types;
    let segment_meta_issues = projection.segment_meta_issues;
    let rse_records = projection.rse_records;
    let segment_bulk = projection.segment_bulk;
    let segment_bulk_issues = projection.segment_bulk_issues;
    let unpaired_segments = projection.unpaired_segments;
    ctx.charge_entities(1, "admit Inventor native structural records")?;
    let active_carrier = ActiveCarrierRecord::from_state(ctx, &container.rse.active_carrier)?;
    let assembly_occurrences = ctx.try_collect_vec(
        assembly_inventory
            .occurrences
            .iter()
            .map(|occurrence| AssemblyOccurrenceRecord::from_occurrence(ctx, occurrence)),
        "retain Inventor assembly_occurrences records",
    )?;
    let mut assembly_placements = Vec::new();
    let mut placements = assembly_inventory.placements.iter();
    while let Some(placement) = ctx.next_charged(
        &mut placements,
        "visit Inventor assembly_placements records",
    )? {
        if let Some(record) = admit_assembly_placement(
            ctx,
            AssemblyPlacementRecordWire::from_placement(ctx, placement)?,
            &mut assembly_inventory.issues,
        )? {
            ctx.push_vec(
                &mut assembly_placements,
                record,
                "retain Inventor assembly_placements records",
            )?;
        }
    }
    let presentation_native =
        presentation_native_projection::project(ctx, &mut presentation_inventory)?;
    let pm_app_default_styles = presentation_native.default_styles;
    let pm_app_rendering_styles = presentation_native.rendering_styles;
    let pm_graphics_faces = presentation_native.graphics_faces;
    let pm_graphics_style_collections = presentation_native.graphics_style_collections;
    let pm_graphics_primary_color_styles = presentation_native.graphics_primary_color_styles;
    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
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
    protein.install(ctx, namespace)?;
    namespace.set_arena(ctx, "protein_assets", &protein_assets)?;
    namespace.set_arena(ctx, "protein_rejections", &protein_rejections)?;
    ufrx.install(ctx, namespace)?;
    namespace.set_arena(ctx, "assembly_occurrences", &assembly_occurrences)?;
    namespace.set_arena(ctx, "assembly_placements", &assembly_placements)?;
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
    namespace.set_arena(ctx, "pm_dc_feature_labels", &feature_inventory.labels)?;
    namespace.set_arena(
        ctx,
        "pm_dc_entity_style_links",
        &feature_inventory.entity_style_links,
    )?;
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

    // The failure detail only feeds the geometry loss message, so a formatted
    // error is held in scoped storage and other details are borrowed.
    let mut geometry_failure_storage = ctx.reserve_scoped(0, "Inventor geometry failure detail")?;
    let mut geometry_failure: Option<std::borrow::Cow<'_, str>> = None;
    let kernel_brep = match &container.rse.active_carrier {
        ActiveCarrierState::Selected(carrier) => match carrier.header.as_ref() {
            Ok(header) => match crate::kernel::decode_kernel_carrier(ctx, carrier, header) {
                Ok(brep) => {
                    apply_kernel_header(ctx, &mut ir, carrier.family, &header.metadata)?;
                    Some(brep)
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    geometry_failure = Some(std::borrow::Cow::Owned(
                        geometry_failure_storage.with_storage(|| {
                            ctx.format_retained(
                                format_args!("{error}"),
                                "format Inventor geometry failure",
                            )
                        })?,
                    ));
                    None
                }
            },
            Err(detail) => {
                geometry_failure = Some(std::borrow::Cow::Borrowed(detail.as_str()));
                None
            }
        },
        _ => None,
    };
    let kernel_brep = kernel_brep.unwrap_or_else(AsmBrep::default);
    let mut face_keys_storage = ctx.reserve_scoped(0, "index Inventor ASM face keys")?;
    let face_keys = face_keys_storage.with_storage(|| {
        index_asm_face_keys(ctx, &kernel_brep.face_native_keys, |record| {
            Ok(record.asm_face_key.map(|key| (&record.face, key)))
        })
    })?;
    let (
        _,
        AsmTransferRemainder {
            unknowns: kernel_unknowns,
            stats: kernel_stats,
            annotation_records: kernel_annotations,
        },
    ) = transfer_into_ir(ctx, &mut ir, "inventor", kernel_brep)?;
    ir.set_native_unknowns(ctx, "inventor", &[])?;
    let geometry_transferred =
        !(ir.model.surfaces.is_empty() && ir.model.points.is_empty() && ir.model.faces.is_empty());
    let body_ids = ctx.collect_indexed_vec(
        ir.model.bodies.len(),
        "collect Inventor projected body ids",
        |index| {
            ir.model.bodies[index]
                .id
                .try_clone_for_decode(ctx, "retain Inventor projected body id")
        },
    )?;
    if geometry_transferred {
        let mut product_definitions = ir.model.product_definitions.iter_mut();
        while let Some(product) =
            ctx.next_charged(&mut product_definitions, "visit Inventor product definitions")?
        {
            product.bodies = ctx.collect_indexed_vec(
                body_ids.len(),
                "collect Inventor product body ids",
                |index| {
                    body_ids[index].try_clone_for_decode(ctx, "retain Inventor product body id")
                },
            )?;
        }
    } else if matches!(
        &container.rse.active_carrier,
        ActiveCarrierState::Selected(_)
    ) && geometry_failure.is_none()
    {
        geometry_failure = Some(std::borrow::Cow::Borrowed(
            "the active kernel carrier decoded no surfaces, points, or faces",
        ));
    }
    let presentation_projection = crate::presentation::project_bindings(
        ctx,
        &presentation_inventory,
        &ir.model.appearances,
        &body_ids,
        &face_keys,
    )?;
    let face_color_appearance_count = presentation_projection.appearances.len();
    {
        // Both indexes borrow their keys from the projection, which moves into
        // the model only after the faces take their colors.
        let mut colors_storage = ctx.reserve_scoped(0, "index Inventor face colors")?;
        let projected_colors = colors_storage.with_storage(|| {
            index_colors(
                ctx,
                &presentation_projection.appearances,
                "index Inventor projected appearance colors",
                |appearance| Ok(appearance.base_color.map(|color| (&appearance.id, color))),
            )
        })?;
        let face_colors = colors_storage.with_storage(|| {
            index_colors(
                ctx,
                &presentation_projection.bindings,
                "index Inventor face colors",
                |binding| {
                    let cadmpeg_ir::appearance::AppearanceTarget::Face(face) = &binding.target
                    else {
                        return Ok(None);
                    };
                    Ok(ctx
                        .get_hash_map(
                            &projected_colors,
                            &binding.appearance,
                            "access Inventor decode records",
                        )?
                        .copied()
                        .map(|color| (face, color)))
                },
            )
        })?;
        let mut faces = ir.model.faces.iter_mut();
        while let Some(face) = ctx.next_charged(&mut faces, "visit Inventor faces")? {
            if face.color.is_none() {
                face.color = ctx
                    .get_hash_map(&face_colors, &face.id, "access Inventor decode records")?
                    .copied();
            }
        }
    }
    ctx.extend_vec(
        &mut ir.model.appearances,
        presentation_projection.appearances,
        "retain Inventor native structural records",
    )?;
    ir.model.appearance_bindings = presentation_projection.bindings;
    // Read before `geometry_failure` is consumed by the loss message below.
    let carrier_read_no_geometry = geometry_failure.is_some();
    let mut losses = Vec::new();
    if protein_admission_issue_count != 0 {
        admitted_loss(ctx, &mut losses, InventorLossCode::ProteinAssetRejected, format_args!(
            "Rejected {protein_admission_issue_count} Protein native record(s); retained the remaining records."
        ))?;
    }
    let loss = dialect_loss(ctx, &matched, &recovery)?;
    drop(recovery);
    drop(recovery_storage);
    if let Some(loss) = loss {
        ctx.push_vec(&mut losses, loss, "collect Inventor dialect loss")?;
    }
    if let Some(loss) = kernel_loss {
        ctx.push_vec(&mut losses, loss, "collect Inventor kernel dialect loss")?;
    }
    if !ctx.container_only()
        && !matches!(document_kind, DocumentKind::Assembly)
        && !geometry_transferred
    {
        let code = InventorLossCode::GeometryKernelCarrierNotTransferred;
        match (&geometry_failure, &container.rse.active_carrier) {
            (Some(detail), _) => {
                admitted_loss(ctx, &mut losses, code, format_args!("{detail}"))?;
            }
            (None, ActiveCarrierState::Selected(_)) => admitted_loss(
                ctx,
                &mut losses,
                code,
                format_args!("The typed active kernel carrier has not been transferred."),
            )?,
            (None, ActiveCarrierState::Unavailable(detail)) => admitted_loss(
                ctx,
                &mut losses,
                code,
                format_args!("The active Inventor kernel carrier is unavailable: {detail}"),
            )?,
            (None, ActiveCarrierState::NotApplicable) => admitted_loss(
                ctx,
                &mut losses,
                code,
                format_args!("Inventor geometry is not available for this document kind."),
            )?,
        }
    }
    if !ctx.container_only() {
        if kernel_stats.unknown_surface_faces() != 0 {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::GeometryProceduralSurfaceNotTransferred,
                format_args!(
                    "{} face(s) use procedural surfaces without a decoded carrier.",
                    kernel_stats.unknown_surface_faces()
                ),
            )?;
        }
        if !segment_pairs.is_empty() {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::RseSegmentPairUntyped,
                format_args!(
                    "Retained {} structurally paired RSe segment(s) without record semantics.",
                    segment_pairs.len()
                ),
            )?;
        }
        if !segment_meta_issues.is_empty() {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::RseMetadataStreamMalformed,
                format_args!(
                    "{} RSe metadata stream(s) are malformed or outside the implemented envelope.",
                    segment_meta_issues.len()
                ),
            )?;
        }
        if !segment_bulk_issues.is_empty() {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::RseBulkStreamMalformed,
                format_args!(
                    "{} RSe bulk stream(s) have invalid envelope or zlib framing.",
                    segment_bulk_issues.len()
                ),
            )?;
        }
        if !assembly_inventory.issues.is_empty() {
            admitted_loss(ctx, &mut losses, InventorLossCode::AssemblyRecordMalformed, format_args!(
                "{} typed Inventor assembly record(s) are malformed or outside the implemented branch.",
                assembly_inventory.issues.len()
            ))?;
        }
        if !presentation_inventory.issues.is_empty() {
            admitted_loss(ctx, &mut losses, InventorLossCode::PresentationRecordMalformed, format_args!(
                "{} typed Inventor presentation record(s) are malformed or outside the implemented branch.",
                presentation_inventory.issues.len()
            ))?;
        }
        if !design_inventory.issues.is_empty() {
            admitted_loss(ctx, &mut losses, InventorLossCode::DesignRecordMalformed, format_args!(
                "{} typed Inventor design record(s) are malformed or outside the implemented branch.",
                design_inventory.issues.len()
            ))?;
        }
        if !sketch_inventory.issues.is_empty() {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::SketchRecordMalformed,
                format_args!(
                    "{} typed Inventor sketch record(s) could not be parsed exactly.",
                    sketch_inventory.issues.len()
                ),
            )?;
        }
        if !feature_inventory.issues.is_empty() {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::FeatureRecordMalformed,
                format_args!(
                    "{} typed Inventor feature record(s) could not be parsed exactly.",
                    feature_inventory.issues.len()
                ),
            )?;
        }
        if unresolved_features != 0 {
            admitted_loss(ctx, &mut losses, InventorLossCode::FeatureOperationGraphOpen, format_args!(
                "Retained {unresolved_features} typed Inventor feature record(s) whose operation graph is not closed."
            ))?;
        }
        if unresolved_feature_states != 0 {
            admitted_loss(ctx, &mut losses, InventorLossCode::FeatureStateUnresolved, format_args!(
                "Transferred {unresolved_feature_states} Inventor operation(s) with native result-body identity and unresolved suppression and dependency state."
            ))?;
        }
        if unresolved_design_parameters != 0 {
            admitted_loss(ctx, &mut losses, InventorLossCode::ParameterGraphOpen, format_args!(
                "Retained {unresolved_design_parameters} Inventor parameter record(s) whose unit or expression graph is not closed."
            ))?;
        }
        if unresolved_sketches != 0
            || unresolved_sketch_entities != 0
            || unresolved_sketch_constraints != 0
        {
            admitted_loss(ctx, &mut losses, InventorLossCode::SketchGraphOpen, format_args!(
                "Retained {unresolved_sketches} Inventor sketch record(s), {unresolved_sketch_entities} sketch-entity record(s), and {unresolved_sketch_constraints} sketch-constraint record(s) whose neutral graph is not closed."
            ))?;
        }
        if !container.rse.unpaired_metadata.is_empty() || !container.rse.unpaired_bulk.is_empty() {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::RseStreamUnpaired,
                format_args!(
                    "RSe contains {} unpaired metadata stream(s) and {} unpaired bulk stream(s).",
                    container.rse.unpaired_metadata.len(),
                    container.rse.unpaired_bulk.len()
                ),
            )?;
        }
        if !property_set_issues.is_empty() {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::PropertySetStreamMalformed,
                format_args!(
                    "{} OLE property-set stream(s) are malformed.",
                    property_set_issues.len()
                ),
            )?;
        }
        if metadata.unmapped != 0 {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::MetadataPropertyUnmapped,
                format_args!(
                    "Retained {} property value(s) without neutral metadata mapping.",
                    metadata.unmapped
                ),
            )?;
        }
        match &container.protein {
            ProteinState::Package(_) => {
                if let Some(detail) = &protein_semantic_issue {
                    admitted_loss(
                        ctx,
                        &mut losses,
                        InventorLossCode::ProteinCatalogUndecodable,
                        format_args!("The Protein asset catalog could not be decoded: {detail}"),
                    )?;
                } else {
                    if !protein_rejections.is_empty() {
                        admitted_loss(ctx, &mut losses, InventorLossCode::ProteinAssetRejected, format_args!(
                            "Rejected {} malformed Protein asset record(s); later framed records remain decoded.",
                            protein_rejections.len()
                        ))?;
                    }
                    if ir.model.appearances.is_empty() {
                        admitted_loss(
                            ctx,
                            &mut losses,
                            InventorLossCode::ProteinAppearanceAbsent,
                            format_args!(
                                "The Protein package contains no decoded appearance assets.",
                            ),
                        )?;
                    } else if presentation_projection.unresolved_defaults != 0 {
                        admitted_loss(
                            ctx,
                            &mut losses,
                            InventorLossCode::AppearanceDefaultUnresolved,
                            format_args!(
                            "Could not resolve {} PmApp document-default appearance assignment(s).",
                            presentation_projection.unresolved_defaults
                        ),
                        )?;
                    }
                }
                if !material_catalog.duplicate_guids.is_empty() {
                    admitted_loss(ctx, &mut losses, InventorLossCode::ProteinGuidAmbiguous, format_args!(
                        "The Protein catalog contains {} duplicate asset GUID(s); ambiguous texture joins were refused.",
                        material_catalog.duplicate_guids.len()
                    ))?;
                }
                if material_catalog.untyped_distance_properties != 0 {
                    admitted_loss(ctx, &mut losses, InventorLossCode::MaterialDistanceUnitUntyped, format_args!(
                        "{} Protein texture Distance property value(s) retain an untyped unit tag; their typed texture carriers were omitted.",
                        material_catalog.untyped_distance_properties
                    ))?;
                }
            }
            ProteinState::Malformed { .. } => admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::ProteinStreamMalformed,
                format_args!("The Inventor Protein stream is malformed."),
            )?,
            ProteinState::Absent | ProteinState::Empty { .. } => {}
        }
        for (cause, count) in ctx.admit_iter(
            &presentation_projection.unresolved_face_overrides,
            "visit Inventor decode items",
        )? {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::AppearanceFaceOverrideUnresolved,
                format_args!(
                    "Could not resolve {count} PmGraphics face appearance override(s): {}.",
                    cause.description()
                ),
            )?;
        }
        if ufrx_issue_count != 0 {
            admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::UfrxTableMalformed,
                format_args!("Skipped {ufrx_issue_count} invalid UFRxDoc native record(s)."),
            )?;
        }
        match &container.ufrx {
            UfrxState::Malformed { .. } => admitted_loss(
                ctx,
                &mut losses,
                InventorLossCode::UfrxTableMalformed,
                format_args!("The UFRxDoc external-reference table is malformed."),
            )?,
            UfrxState::Unsupported { schema, .. } => {
                let code = if matches!(document_kind, DocumentKind::Assembly) {
                    InventorLossCode::UfrxSchemaUnsupportedAssembly
                } else {
                    InventorLossCode::UfrxSchemaUnsupported
                };
                admitted_loss(
                    ctx,
                    &mut losses,
                    code,
                    format_args!(
                    "Retained unsupported UFRxDoc schema {schema} semantic branch without transfer."
                ),
                )?;
            }
            UfrxState::Parsed(_) if matches!(document_kind, DocumentKind::Assembly) => {
                if !external_references.is_empty() {
                    admitted_loss(
                        ctx,
                        &mut losses,
                        InventorLossCode::AssemblyComponentExternal,
                        format_args!(
                            "Retained {} unresolved external component reference(s).",
                            external_references.len()
                        ),
                    )?;
                }
                for (cause, count) in ctx.admit_iter(
                    &assembly_projection.unresolved_placements,
                    "visit Inventor decode items",
                )? {
                    admitted_loss(
                        ctx,
                        &mut losses,
                        InventorLossCode::AssemblyPlacementNotTransferred,
                        format_args!(
                            "Could not transfer {count} assembly occurrence placement(s): {}.",
                            cause.description()
                        ),
                    )?;
                }
            }
            UfrxState::Absent | UfrxState::Parsed(_) => {}
        }
    }
    let preview_asset_count = ir.model.assets.len();
    let mut source_fidelity = SourceFidelity::default();
    let mut annotations = AnnotationBuilder::new();
    let mut kernel_annotation_records = kernel_annotations.iter();
    while let Some(record) =
        ctx.next_charged(&mut kernel_annotation_records, "visit Inventor kernel annotations")?
    {
        admit_kernel_annotation(ctx, &mut annotations, record)?;
    }
    source_fidelity.annotations = annotations.build();
    if let ActiveCarrierState::Selected(carrier) = &container.rse.active_carrier {
        // Retention is keyed on the outcome, not on a version band: a carrier
        // this decode read no geometry out of keeps its bytes verbatim, whatever
        // its save format declared.
        if carrier_read_no_geometry {
            admit_untransferred_carrier(ctx, carrier)?;
            let data = ctx.copy_retained(
                carrier.bytes.window(),
                "retain Inventor kernel carrier that read no geometry",
            )?;
            let mut stream_storage =
                ctx.reserve_scoped(0, "format Inventor carrier source stream")?;
            let stream = stream_storage.with_storage(|| {
                ctx.format_retained(
                    format_args!("RSeStorage/B{}:expanded", carrier.segment_token),
                    "retain Inventor carrier source stream",
                )
            })?;
            let mut key_storage =
                ctx.reserve_scoped(0, "Inventor temporary carrier identity key")?;
            let key = key_storage.with_storage(|| {
                ctx.format_retained(
                    format_args!("{}-{}", carrier.segment_token, carrier.record_ordinal),
                    "Inventor temporary carrier identity key",
                )
            })?;
            let id = ctx.format_retained(
                format_args!("inventor:kernel:carrier#{key}"),
                "retain Inventor unknown carrier id",
            )?;
            let validation_work = id.len().checked_add(key.len()).ok_or_else(|| {
                ctx.refuse_codec_limit("validate Inventor retained carrier id", u64::MAX, u64::MAX)
            })?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(validation_work),
                "validate Inventor retained carrier id",
            )?;
            let id = UnknownId::mint(id).map_err(CodecError::malformed)?;
            let mut links_storage = ctx.reserve_scoped(0, "collect Inventor carrier link")?;
            let mut links = Vec::new();
            links_storage.with_storage(|| {
                let link =
                    ctx.copy_retained_text(active_carrier.id(), "retain Inventor carrier link id")?;
                ctx.push_vec(&mut links, link, "collect Inventor carrier link")
            })?;
            source_fidelity.retain_unknown_records(
                stream,
                [UnknownRecord::retained(
                    id,
                    carrier.carrier_offset,
                    data,
                    links,
                )],
            )?;
        }
    }
    // The unknowns arena was emptied after transfer, and attachment either
    // stores every kernel unknown or refuses, so this is the arena's length.
    let kernel_unknown_record_count = kernel_unknowns.len();
    if !kernel_unknowns.is_empty() {
        source_fidelity
            .attach_native_unknown_records(&mut ir, "inventor", kernel_unknowns, ctx)
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "Inventor kernel unknown retention failed: {error}"
                ))
            })?;
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
    let coverage =
        cadmpeg_ir::report::decode::Coverage::from_iter_for_decode(ctx, coverage_entries)?;
    let body = DecodeBody {
        transfer: if ctx.container_only() {
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
        } else {
            cadmpeg_ir::report::decode::DecodeTransfer::full(geometry_transferred)
        },
        coverage,
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

fn decimal_digits(value: usize) -> Result<usize, CodecError> {
    let digits = value.checked_ilog10().map_or(0, |digits| digits);
    usize::try_from(digits)
        .map(|digits| digits + 1)
        .map_err(|_| {
            CodecError::Malformed("Inventor decimal digit count exceeds address space".into())
        })
}

fn record_ordinal(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    operation: &'static str,
) -> Result<u32, CodecError> {
    u32::try_from(ordinal)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::from(u32::MAX), u64::MAX))
}

fn retained_hex<const N: usize>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8; N],
    operation: &'static str,
) -> Result<String, CodecError> {
    let len = N.checked_mul(2).ok_or_else(|| {
        ctx.refuse_codec_limit("Inventor hexadecimal length", u64::MAX - 1, u64::MAX)
    })?;
    let mut output = ctx.retained_string(len, operation)?;
    for &byte in bytes {
        output.push(HEX_DIGITS[usize::from(byte >> 4)]);
        output.push(HEX_DIGITS[usize::from(byte & 0x0f)]);
    }
    Ok(output)
}

fn admitted_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    code: InventorLossCode,
    message: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let note = code.note(ctx, message, "retain Inventor decode loss message")?;
    ctx.push_vec(losses, note, "collect Inventor decode loss")?;
    Ok(())
}

fn project_root_product(
    ctx: &DecodeContext<'_>,
    document_kind: &DocumentKind,
    metadata: &mut MetadataProjection,
    entity_count: usize,
    admitted_entities: &mut u64,
) -> Result<ProductDefinition, CodecError> {
    let next_count = entity_count.checked_add(1).ok_or_else(|| {
        ctx.refuse_codec_limit("Inventor root product count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(next_count),
        admitted_entities,
        "admit Inventor root product",
    )?;
    // The product takes the projection's retained values; only the title,
    // which fills two fields, is copied.
    let source_name = metadata
        .title
        .as_deref()
        .map(|name| ctx.copy_retained_text(name, "retain Inventor root product source name"))
        .transpose()?;
    let label = metadata.title.take();
    let description = metadata.description.take();
    let part_number = metadata.part_number.take();
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
        bom_properties: cadmpeg_core::text::named_entries_for_decode(
            ctx,
            "inventor:document:product#root",
            std::mem::take(&mut metadata.bom_properties),
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
    let key = ctx.copy_retained_text(key, "retain Inventor source attribute key")?;
    let value = ctx.format_retained(value, "retain Inventor source attribute value")?;
    ctx.insert_btree_map(attributes, key, value, "collect Inventor source attribute")?;
    Ok(())
}

fn admit_kernel_annotation(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    record: &cadmpeg_asm::brep::annotations::AnnotationRecord,
) -> Result<(), CodecError> {
    let name = ctx.format_retained(
        format_args!("inventor:{}", record.stream),
        "retain Inventor annotation stream name",
    )?;
    let name = cadmpeg_ir::StreamName::try_from(name).map_err(CodecError::malformed)?;
    let stream = StreamHandle::new(ctx, name, "collect Inventor kernel provenance")?;
    annotations.note(
        ctx,
        &record.id,
        &stream,
        record.offset,
        Some(record.tag.as_str()),
    )?;
    let mut derived_fields = record.derived_fields.iter();
    while let Some(field) = ctx.next_charged(&mut derived_fields, "visit Inventor decode items")? {
        annotations
            .derived(ctx, &record.id, field)
            .map_err(CodecError::from)?;
    }
    Ok(())
}

fn admit_untransferred_carrier(
    ctx: &DecodeContext<'_>,
    carrier: &crate::kernel::ActiveCarrier<'_>,
) -> Result<(), CodecError> {
    let token = carrier.segment_token.as_str();
    ctx.charge_collection_items(1, "collect Inventor retained carrier")?;
    ctx.charge_formatted_retained(
        format_args!("RSeStorage/B{token}:expanded"),
        "retain Inventor carrier source stream copy",
    )?;
    // SourceOwner moves the incoming String and copies it once into the stored record.
    let copied_stream_work = "RSeStorage/B"
        .len()
        .checked_add(token.len())
        .and_then(|len| len.checked_add(":expanded".len()))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("copy Inventor retained carrier owner", u64::MAX, u64::MAX)
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(copied_stream_work),
        "copy Inventor retained carrier owner",
    )?;
    Ok(())
}

fn project_property_set_issue(
    ctx: &DecodeContext<'_>,
    directory_id: u32,
    path: &str,
    detail: &str,
) -> Result<PropertySetIssueRecord, CodecError> {
    let id = ctx.format_retained(
        format_args!("inventor:property:set-issue#{directory_id}"),
        "retain Inventor property-set issue id",
    )?;
    let path = ctx.copy_retained_text(path, "retain Inventor property-set issue path")?;
    let detail = ctx.copy_retained_text(detail, "retain Inventor property-set issue detail")?;
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
    let next_entities = admitted_entities.checked_add(1).ok_or_else(|| {
        ctx.refuse_codec_limit("Inventor preview entity count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.admit_entities(
        next_entities,
        admitted_entities,
        "admit Inventor preview asset entity",
    )?;
    let ordinal_digits = decimal_digits(ordinal)?;
    let key_len = "preview-"
        .len()
        .checked_add(ordinal_digits)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "format Inventor preview identity key",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    let id_len = "inventor:document:asset#"
        .len()
        .checked_add(key_len)
        .ok_or_else(|| {
            ctx.refuse_codec_limit("format Inventor preview asset id", u64::MAX - 1, u64::MAX)
        })?;
    let identity_work = ordinal_digits
        .checked_add(key_len)
        .and_then(|work| work.checked_add(id_len))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "format Inventor preview asset identity",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(identity_work),
        "format Inventor preview asset identity",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id_len),
        "retain Inventor preview asset id",
    )?;
    let id = {
        let _key_storage = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(key_len),
            "retain Inventor preview identity key",
        )?;
        AssetId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "document", "asset"),
            cadmpeg_ir::identity_key!("preview-").then(ordinal),
        )
    };
    if bytes.is_empty() {
        return Err(CodecError::Malformed("asset data must not be empty".into()));
    }
    let native_ref = ctx.copy_retained_text(native_id, "retain Inventor preview source id")?;
    let mut name =
        ctx.retained_string("document preview".len(), "retain Inventor preview name")?;
    name.push_str("document preview");
    let media_type = ctx.copy_retained_text(media_type, "retain Inventor preview media type")?;
    let data = ctx.copy_retained(bytes, "retain Inventor preview asset")?;
    let asset = Asset::try_new(
        ctx,
        id,
        Some(name),
        Some(media_type),
        AssetContent::Embedded {
            data: cadmpeg_ir::assets::AssetData::new(data)
                .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
        },
        Some(native_ref),
    )?;
    Ok(asset)
}

fn project_protein_state(
    ctx: &DecodeContext<'_>,
    state: &ProteinState<'_>,
) -> Result<ProteinRecord, CodecError> {
    ctx.charge_entities(1, "admit Inventor native structural records")?;
    let id = ctx.copy_retained_text(
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
            detail: ctx.copy_retained_text(detail, "retain Inventor Protein state detail")?,
        },
        ProteinState::Package(package) => {
            let entries = ctx.try_collect_vec(
                package.archive.entries().iter().enumerate().map(
                    |(ordinal, entry)| -> Result<_, CodecError> {
                        ctx.charge_entities(1, "admit Inventor native structural records")?;
                        Ok(ProteinEntryRecord {
                            id: ctx.format_retained(
                                format_args!("inventor:protein:entry#{ordinal}"),
                                "retain Inventor Protein entry id",
                            )?,
                            ordinal: record_ordinal(
                                ctx,
                                ordinal,
                                "Inventor Protein entry ordinal",
                            )?,
                            name: ctx.copy_retained_text(
                                &entry.name,
                                "retain Inventor Protein entry name",
                            )?,
                            compression: entry.compression,
                            crc32: entry.crc32,
                            compressed_size: entry.compressed_size,
                            uncompressed_size: entry.uncompressed_size,
                        })
                    },
                ),
                "retain Inventor entries records",
            )?;
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
    let mut assets = Vec::new();
    let mut rejections = Vec::new();
    let mut issues = Vec::new();
    let mut instances = instances.into_iter();
    while let Some(instance) =
        ctx.next_charged(&mut instances, "visit Inventor Protein instances")?
    {
        if instance.records.is_empty() && instance.rejected.is_empty() {
            continue;
        }
        let entry_name_len = cadmpeg_core::decode::u64_from_index(instance.entry_name.len());
        ctx.charge_work(entry_name_len, "hash Inventor Protein entry name")?;
        let _digest_reservation = ctx.reserve_scoped(64, "hash Inventor Protein entry name")?;
        let entry_digest = sha256_hex(instance.entry_name.as_bytes());
        let mut asset_records = instance.records.into_iter();
        while let Some(asset) =
            ctx.next_charged(&mut asset_records, "visit Inventor Protein assets")?
        {
            let ordinal = asset.ordinal;
            let id = ctx.format_retained(
                format_args!("inventor:protein:asset#{}-{}", entry_digest, asset.ordinal),
                "retain Inventor Protein asset id",
            )?;
            let entry_name = ctx.copy_retained_text(
                &instance.entry_name,
                "retain Inventor Protein asset entry name",
            )?;
            let wire = ProteinAssetRecordWire {
                id,
                entry_name,
                ordinal: asset.ordinal,
                asset,
            };
            if let Some(record) = admit_ufrx_record(
                ctx,
                wire.into_record(ctx),
                format_args!("inventor:protein:asset#{entry_digest}-{ordinal}"),
                &mut issues,
            )? {
                ctx.push_vec(
                    &mut assets,
                    record,
                    "retain Inventor native structural records",
                )?;
            }
        }
        let mut rejected_records = instance.rejected.into_iter();
        while let Some(rejected) =
            ctx.next_charged(&mut rejected_records, "visit Inventor Protein rejections")?
        {
            let ordinal = rejected.ordinal;
            let id = ctx.format_retained(
                format_args!(
                    "inventor:protein:rejection#{}-{}",
                    entry_digest, rejected.ordinal
                ),
                "retain Inventor Protein rejection id",
            )?;
            let entry_name = ctx.copy_retained_text(
                &instance.entry_name,
                "retain Inventor Protein rejection entry name",
            )?;
            let wire = ProteinRejectionRecordWire {
                id,
                entry_name,
                ordinal: rejected.ordinal,
                detail: rejected.detail,
            };
            if let Some(record) = admit_ufrx_record(
                ctx,
                wire.into_record(ctx),
                format_args!("inventor:protein:rejection#{entry_digest}-{ordinal}"),
                &mut issues,
            )? {
                ctx.push_vec(
                    &mut rejections,
                    record,
                    "retain Inventor native structural records",
                )?;
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
    ctx.charge_entities(1, "admit Inventor native structural records")?;
    Ok(match state {
        UfrxState::Absent => {
            let mut id = ctx.retained_string(
                "inventor:ufrx:state#root".len(),
                "retain Inventor UFRx state id",
            )?;
            id.push_str("inventor:ufrx:state#root");
            UfrxRecord::Absent { id }
        }
        UfrxState::Malformed { stream, detail } => UfrxRecord::Malformed {
            id: {
                let mut id = ctx.retained_string(
                    "inventor:ufrx:state#root".len(),
                    "retain Inventor UFRx state id",
                )?;
                id.push_str("inventor:ufrx:state#root");
                id
            },
            directory_id: stream.directory_id(),
            detail: ctx.copy_retained_text(detail, "retain Inventor UFRx state detail")?,
        },
        UfrxState::Unsupported {
            stream,
            schema,
            section_versions,
            source,
            detail,
        } => UfrxRecord::Unsupported {
            id: {
                let mut id = ctx.retained_string(
                    "inventor:ufrx:state#root".len(),
                    "retain Inventor UFRx state id",
                )?;
                id.push_str("inventor:ufrx:state#root");
                id
            },
            directory_id: stream.directory_id(),
            schema: *schema,
            section_versions: ctx
                .copy_slice(section_versions, "copy Inventor UFRx section versions")?,
            tail_len: cadmpeg_core::decode::u64_from_index(source.window().len()),
            tail_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                source.window(),
                "retain Inventor UFRx unsupported tail digest",
            )?,
            detail: ctx.copy_retained_text(detail, "retain Inventor UFRx state detail")?,
        },
        UfrxState::Parsed(document) => {
            let mut model_states = Vec::new();
            let mut model_states_source = document.model_states.iter().enumerate();
            while let Some((ordinal, state)) =
                ctx.next_charged(&mut model_states_source, "visit Inventor decode items")?
            {
                if let Some(record) = project_ufrx_model_state(ctx, ordinal, state, issues)? {
                    ctx.push_vec(
                        &mut model_states,
                        record,
                        "retain Inventor native structural records",
                    )?;
                }
            }
            let mut references = Vec::new();
            let mut references_source = document.references.iter().enumerate();
            while let Some((ordinal, reference)) =
                ctx.next_charged(&mut references_source, "visit Inventor decode items")?
            {
                if let Some(record) =
                    project_ufrx_external_reference(ctx, ordinal, reference, issues)?
                {
                    ctx.push_vec(
                        &mut references,
                        record,
                        "retain Inventor native structural records",
                    )?;
                }
            }
            let mut embedded = Vec::new();
            let mut embedded_references_source = document.embedded_references.iter().enumerate();
            while let Some((ordinal, reference)) = ctx
                .next_charged(&mut embedded_references_source, "visit Inventor decode items")?
            {
                if let Some(record) =
                    project_ufrx_embedded_reference(ctx, ordinal, reference, issues)?
                {
                    ctx.push_vec(
                        &mut embedded,
                        record,
                        "retain Inventor native structural records",
                    )?;
                }
            }
            let mut occurrences = Vec::new();
            let mut occurrences_source = document.occurrences.iter().enumerate();
            while let Some((ordinal, occurrence)) =
                ctx.next_charged(&mut occurrences_source, "visit Inventor decode items")?
            {
                if let Some(record) = project_ufrx_occurrence(ctx, ordinal, occurrence, issues)? {
                    ctx.push_vec(
                        &mut occurrences,
                        record,
                        "retain Inventor native structural records",
                    )?;
                }
            }
            let representation = document
                .representation
                .as_ref()
                .map(|state| project_ufrx_representation(ctx, state, issues))
                .transpose()?
                .flatten();
            UfrxRecord::ParsedPrefix(Box::new(UfrxParsedPrefix {
                id: {
                    let mut id = ctx.retained_string(
                        "inventor:ufrx:state#root".len(),
                        "retain Inventor UFRx state id",
                    )?;
                    id.push_str("inventor:ufrx:state#root");
                    id
                },
                directory_id: document.stream.directory_id(),
                schema: document.schema,
                section_versions: {
                    ctx.copy_slice(
                        &document.section_versions,
                        "copy Inventor UFRx section versions",
                    )?
                },
                original_file_name: ctx.copy_retained_text(
                    &document.original_file_name,
                    "retain Inventor UFRx original file name",
                )?,
                caption: ctx
                    .copy_retained_text(&document.caption, "retain Inventor UFRx caption")?,
                representation,
                model_states,
                external_references: references,
                embedded_references: embedded,
                occurrences,
                tail_len: cadmpeg_core::decode::u64_from_index(
                    document.unparsed_tail.window().len(),
                ),
                tail_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                    ctx,
                    document.unparsed_tail.window(),
                    "retain Inventor UFRx tail digest",
                )?,
            }))
        }
    })
}

fn project_ufrx_model_state(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    state: &crate::external_reference::UfrxModelState<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<UfrxModelStateRecord>, CodecError> {
    let parameters = ctx.try_collect_vec(
        state
            .parameters
            .iter()
            .map(|parameter| -> Result<_, CodecError> {
                ctx.charge_entities(1, "admit Inventor UFRx state parameter")?;
                Ok(UfrxModelStateParameterRecord {
                    name: ctx.copy_retained_text(
                        &parameter.name,
                        "retain Inventor UFRx parameter name",
                    )?,
                    tag: parameter.tag,
                    kind: parameter.kind,
                    state: parameter.state,
                    value: ctx.copy_retained_text(
                        &parameter.value,
                        "retain Inventor UFRx parameter value",
                    )?,
                    trailer: parameter.trailer,
                })
            }),
        "retain Inventor parameters records",
    )?;
    let admitted = UfrxModelStateRecordWire {
        id: ctx.format_retained(
            format_args!("inventor:ufrx:model-state#{ordinal}"),
            "retain Inventor UFRx model-state id",
        )?,
        ordinal: record_ordinal(ctx, ordinal, "Inventor UFRx model-state ordinal")?,
        prefix: state.prefix,
        name: ctx.copy_retained_text(&state.name, "retain Inventor UFRx model-state name")?,
        state: state.state,
        prefix_count: state.prefix_count,
        parameters,
        suffix_len: cadmpeg_core::decode::u64_from_index(state.suffix.window().len()),
        suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
            ctx,
            state.suffix.window(),
            "retain Inventor UFRx model-state digest",
        )
        .map(String::from)?,
    }
    .into_record(ctx);
    let admitted = match admitted {
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        result => result,
    };
    admit_ufrx_record(
        ctx,
        admitted,
        format_args!("ufrx-model-state-{ordinal}"),
        issues,
    )
}

fn project_ufrx_external_reference(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    reference: &crate::external_reference::InventorExternalReference,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<ExternalReferenceRecord>, CodecError> {
    let admitted = ExternalReferenceRecordWire {
        id: ctx.format_retained(
            format_args!("inventor:ufrx:external-reference#{ordinal}"),
            "retain Inventor UFRx external reference id",
        )?,
        ordinal: record_ordinal(ctx, ordinal, "Inventor UFRx external ordinal")?,
        path: ctx.copy_retained_text(&reference.path, "retain Inventor UFRx external path")?,
        library_id: reference.library_id,
        library_name: ctx.copy_retained_text(
            &reference.library_name,
            "retain Inventor UFRx external library name",
        )?,
        display_name: ctx.copy_retained_text(
            &reference.display_name,
            "retain Inventor UFRx external display name",
        )?,
        state_groups: ctx.copy_slice(
            &reference.state_groups,
            "copy Inventor UFRx reference state groups",
        )?,
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
    }
    .into_record(ctx);
    let admitted = match admitted {
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        result => result,
    };
    admit_ufrx_record(
        ctx,
        admitted,
        format_args!("ufrx-external-reference-{ordinal}"),
        issues,
    )
}

fn project_ufrx_embedded_reference(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    reference: &crate::external_reference::InventorEmbeddedReference<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<EmbeddedReferenceRecord>, CodecError> {
    let admitted = EmbeddedReferenceRecordWire {
        id: ctx.format_retained(
            format_args!("inventor:ufrx:embedded-reference#{ordinal}"),
            "retain Inventor UFRx embedded reference id",
        )?,
        ordinal: record_ordinal(ctx, ordinal, "Inventor UFRx embedded ordinal")?,
        value_0: reference.value_0,
        filetime: reference.filetime,
        value_1: reference.value_1,
        extended_value: reference.extended_value,
        value_2: reference.value_2,
        path: ctx.copy_retained_text(&reference.path, "retain Inventor UFRx embedded path")?,
        library_id: reference.library_id,
        library_name: ctx.copy_retained_text(
            &reference.library_name,
            "retain Inventor UFRx embedded library name",
        )?,
        state: reference.state,
        display_name: ctx.copy_retained_text(
            &reference.display_name,
            "retain Inventor UFRx embedded display name",
        )?,
        state_values: reference.state_values,
        record_len: cadmpeg_core::decode::u64_from_index(reference.source.window().len()),
        record_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
            ctx,
            reference.source.window(),
            "retain Inventor UFRx embedded digest",
        )
        .map(String::from)?,
    }
    .into_record(ctx);
    let admitted = match admitted {
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        result => result,
    };
    admit_ufrx_record(
        ctx,
        admitted,
        format_args!("ufrx-embedded-reference-{ordinal}"),
        issues,
    )
}

fn project_ufrx_occurrence(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    occurrence: &crate::external_reference::UfrxOccurrence<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<UfrxOccurrenceRecord>, CodecError> {
    let admitted = UfrxOccurrenceRecordWire {
        id: ctx.format_retained(
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
            .map(|title| ctx.copy_retained_text(title, "retain Inventor UFRx occurrence title"))
            .transpose()?,
        header_padding_words: occurrence.header_padding_words,
        record_len: cadmpeg_core::decode::u64_from_index(occurrence.source.window().len()),
        record_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
            ctx,
            occurrence.source.window(),
            "retain Inventor UFRx occurrence digest",
        )
        .map(String::from)?,
    }
    .into_record(ctx);
    let admitted = match admitted {
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        result => result,
    };
    admit_ufrx_record(
        ctx,
        admitted,
        format_args!("ufrx-occurrence-{ordinal}"),
        issues,
    )
}

fn project_ufrx_representation(
    ctx: &DecodeContext<'_>,
    state: &crate::external_reference::UfrxRepresentationState,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<UfrxRepresentationRecord>, CodecError> {
    let (active_representation, active_representation_kind) =
        match state.active_representation.as_ref() {
            Some((name, kind)) => (
                Some(ctx.copy_retained_text(name, "retain Inventor UFRx representation name")?),
                Some(ctx.copy_retained_text(kind, "retain Inventor UFRx representation kind")?),
            ),
            None => (None, None),
        };
    let wire = UfrxRepresentationRecordWire {
        prefix: state.prefix,
        active_representation,
        active_representation_kind,
        secondary_active_lod_state: state.secondary_active_lod_state,
        active_model_state: ctx.copy_retained_text(
            &state.active_model_state,
            "retain Inventor UFRx active model state",
        )?,
        active_model_state_state: state.active_model_state_state,
    };
    admit_ufrx_record(
        ctx,
        wire.into_record(ctx),
        format_args!("ufrx-representation"),
        issues,
    )
}

/// Indexes the color each entry projects under a key borrowed from it.
fn index_colors<'b, T, K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    entries: &'b [T],
    operation: &'static str,
    mut project: impl FnMut(&'b T) -> Result<Option<(&'b K, Color)>, CodecError>,
) -> Result<HashMap<&'b K, Color>, CodecError> {
    let mut output = HashMap::new();
    let mut source = entries.iter();
    while let Some(entry) = ctx.next_charged(&mut source, operation)? {
        if let Some((key, color)) = project(entry)? {
            ctx.insert_hash_map(&mut output, key, color, operation)?;
        }
    }
    Ok(output)
}

fn index_asm_face_keys<'b, T>(
    ctx: &DecodeContext<'_>,
    entries: &'b [T],
    mut project: impl FnMut(&'b T) -> Result<Option<(&'b FaceId, u64)>, CodecError>,
) -> Result<BTreeMap<FaceId, u64>, CodecError> {
    let mut output = BTreeMap::new();
    let mut source = entries.iter();
    while let Some(entry) = ctx.next_charged(&mut source, "visit Inventor ASM face keys")? {
        let Some((id, key)) = project(entry)? else {
            continue;
        };
        ctx.insert_btree_map(
            &mut output,
            id.try_clone_for_decode(ctx, "retain Inventor ASM face key id")?,
            key,
            "index Inventor ASM face keys",
        )?;
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
        admitted_kernel_attribute(
            ctx,
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("kernel_save_format_version"),
            format_args!("{version}"),
        )?;
    }
    if let Some(count) = header.entity_count {
        admitted_kernel_attribute(
            ctx,
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("kernel_entity_count"),
            format_args!("{count}"),
        )?;
    }
    if let Some(flags) = header.flags {
        admitted_kernel_attribute(
            ctx,
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("kernel_flags"),
            format_args!("{flags}"),
        )?;
    }
    if let Some(family) = &header.product_family {
        admitted_kernel_attribute(
            ctx,
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("kernel_product_family"),
            format_args!("{family}"),
        )?;
    }
    if let Some(version) = &header.product_version {
        admitted_kernel_attribute(
            ctx,
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("kernel_product_version"),
            format_args!("{version}"),
        )?;
    }
    if let (Some(linear), Some(angular)) = (header.linear, header.angular) {
        ir.tolerances = Tolerances::new(linear * 10.0, angular).map_err(CodecError::Malformed)?;
    }
    admitted_kernel_attribute(
        ctx,
        &mut source.attributes,
        cadmpeg_core::nonblank_literal!("kernel_family"),
        format_args!("{}", family.label()),
    )?;
    Ok(())
}

fn admitted_kernel_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: cadmpeg_core::text::NonBlankString,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let value = ctx.format_retained(value, "retain Inventor kernel attribute value")?;
    ctx.insert_btree_map(attributes, key, value, "collect Inventor kernel attribute")?;
    Ok(())
}

/// Admits a converted record, or records its malformed conversion as a
/// structural issue under `scope`, which is formatted only for an issue.
fn admit_ufrx_record<T>(
    ctx: &DecodeContext<'_>,
    admitted: Result<T, CodecError>,
    scope: std::fmt::Arguments<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
) -> Result<Option<T>, CodecError> {
    match admitted {
        Ok(record) => {
            ctx.charge_entities(1, "admit Inventor native structural records")?;
            Ok(Some(record))
        }
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(CodecError::Malformed(detail)) => {
            structural_issue(ctx, issues, scope, &detail)?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn admit_assembly_placement(
    ctx: &DecodeContext<'_>,
    wire: AssemblyPlacementRecordWire,
    issues: &mut Vec<RecordIssue>,
) -> Result<Option<AssemblyPlacementRecord>, CodecError> {
    let mut token_reservation = ctx.reserve_scoped(0, "retain Inventor placement issue token")?;
    let segment_token = token_reservation.with_storage(|| {
        ctx.copy_retained_text(&wire.segment_token, "copy Inventor placement issue token")
    })?;
    let record_ordinal = wire.record_ordinal;
    match wire.into_record() {
        Ok(record) => {
            ctx.charge_entities(1, "admit Inventor native assembly placement")?;
            Ok(Some(record))
        }
        Err(CodecError::Malformed(detail)) => {
            ctx.charge_entities(1, "admit Inventor placement conversion issue")?;
            ctx.reserve_capacity(issues, 1, "retain Inventor native structural records")?;
            token_reservation.commit()?;
            let key_work = segment_token.len().checked_mul(2).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "validate Inventor placement issue token",
                    u64::MAX,
                    u64::MAX,
                )
            })?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(key_work),
                "validate Inventor placement issue token",
            )?;
            ctx.push_vec(
                issues,
                RecordIssue {
                    family: RecordIssueFamily::Assembly,
                    segment_token: cadmpeg_ir::ids::IdentityKey::try_new(segment_token)
                        .map_err(CodecError::malformed)?,
                    record_ordinal,
                    detail: ctx
                        .copy_retained_text(&detail, "retain Inventor placement issue detail")?,
                },
                "retain Inventor native structural records",
            )?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn structural_issue(
    ctx: &DecodeContext<'_>,
    issues: &mut Vec<StructuralIssueRecord>,
    scope: std::fmt::Arguments<'_>,
    detail: impl cadmpeg_core::decode::text::TextSource,
) -> Result<(), CodecError> {
    ctx.charge_entities(1, "admit Inventor structural issue")?;
    ctx.reserve_capacity(issues, 1, "collect Inventor structural issue")?;
    let record = StructuralIssueRecord {
        id: ctx.format_retained(
            format_args!("inventor:rse:structural-issue#{scope}"),
            "retain Inventor structural issue id",
        )?,
        scope: ctx.format_retained(scope, "retain Inventor structural issue scope")?,
        detail: detail.into_retained_text(ctx, "retain Inventor structural issue detail")?,
    };
    ctx.push_vec(issues, record, "collect Inventor structural issue")?;
    Ok(())
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
        name: Option<PropertyName<'_>>,
        value: Option<&str>,
        native_id: &str,
    ) -> Result<(), CodecError> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(());
        };
        let known = name.and_then(|name| name.known);
        if matches!(
            known,
            Some(KnownPropertyName::DocumentKind | KnownPropertyName::DocumentType)
        ) {
            self.document_kind = DocumentKind::parse_property(ctx, value)?;
            if self.document_kind.is_some() {
                return Ok(());
            }
        }
        let target = if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 2
            || known == Some(KnownPropertyName::Title)
        {
            Some(&mut self.title)
        } else if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 4
            || matches!(
                known,
                Some(KnownPropertyName::Author | KnownPropertyName::Designer)
            )
        {
            Some(&mut self.author)
        } else if fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 6
            || matches!(
                known,
                Some(KnownPropertyName::Description | KnownPropertyName::Comments)
            )
        {
            Some(&mut self.description)
        } else if known == Some(KnownPropertyName::PartNumber) {
            Some(&mut self.part_number)
        } else {
            None
        };
        if let Some(target) = target {
            if target.is_none() {
                *target = Some(ctx.copy_retained_text(value, "retain Inventor metadata value")?);
            } else if !ctx.equal(
                &target.as_deref(),
                &Some(value),
                "match Inventor metadata value",
            )? {
                ctx.insert_btree_map(
                    &mut self.bom_properties,
                    ctx.copy_retained_text(native_id, "retain Inventor BOM property key")?,
                    ctx.copy_retained_text(value, "retain Inventor BOM property value")?,
                    "collect Inventor BOM property",
                )?;
            }
            return Ok(());
        }
        if let Some(name) = name {
            ctx.insert_btree_map(
                &mut self.bom_properties,
                ctx.copy_retained_text(name.text, "retain Inventor BOM property key")?,
                ctx.copy_retained_text(value, "retain Inventor BOM property value")?,
                "collect Inventor BOM property",
            )?;
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
                ctx.insert_btree_map(
                    attributes,
                    ctx.copy_retained_text(name, "retain Inventor metadata attribute key")?,
                    ctx.copy_retained_text(value, "retain Inventor metadata attribute value")?,
                    "collect Inventor metadata attribute",
                )?;
            }
        }
        Ok(())
    }
}

/// A property's name as read, with the recognized name it normalizes to.
#[derive(Clone, Copy, Debug)]
struct PropertyName<'a> {
    text: &'a str,
    known: Option<KnownPropertyName>,
}

impl<'a> PropertyName<'a> {
    fn classify(ctx: &DecodeContext<'_>, text: &'a str) -> Result<Self, CodecError> {
        Ok(Self {
            text,
            known: known_property_name(ctx, text)?,
        })
    }
}

/// A property name the metadata projection recognizes once its alphanumeric
/// characters are kept and lowercased.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownPropertyName {
    DocumentKind,
    DocumentType,
    Title,
    Author,
    Designer,
    Description,
    Comments,
    PartNumber,
    Thumbnail,
    Preview,
    PreviewImage,
}

/// The byte length of the longest recognized normalized name.
const KNOWN_PROPERTY_NAME_MAX: usize = "previewimage".len();

/// Classifies `name` by its alphanumeric characters, lowercased.
///
/// The normalized form is built in a fixed buffer: once it outgrows the
/// longest recognized name nothing can match, so the walk stops there. Each
/// visited character is charged as it is read.
fn known_property_name(
    ctx: &DecodeContext<'_>,
    name: &str,
) -> Result<Option<KnownPropertyName>, CodecError> {
    // Four spare bytes let each character's whole UTF-8 buffer be copied.
    let mut normalized = [0_u8; KNOWN_PROPERTY_NAME_MAX + 4];
    let mut len = 0_usize;
    let mut characters = name.chars();
    while let Some(character) =
        ctx.next_charged(&mut characters, "normalize Inventor property name")?
    {
        if !character.is_alphanumeric() {
            continue;
        }
        for lower in character.to_lowercase() {
            let mut buffer = [0_u8; 4];
            let width = lower.encode_utf8(&mut buffer).len();
            let end = len + width;
            if end > KNOWN_PROPERTY_NAME_MAX {
                return Ok(None);
            }
            let Some(slot) = normalized.get_mut(len..len + 4) else {
                return Ok(None);
            };
            slot.copy_from_slice(&buffer);
            len = end;
        }
    }
    Ok(match &normalized[..len] {
        b"documentkind" => Some(KnownPropertyName::DocumentKind),
        b"documenttype" => Some(KnownPropertyName::DocumentType),
        b"title" => Some(KnownPropertyName::Title),
        b"author" => Some(KnownPropertyName::Author),
        b"designer" => Some(KnownPropertyName::Designer),
        b"description" => Some(KnownPropertyName::Description),
        b"comments" => Some(KnownPropertyName::Comments),
        b"partnumber" => Some(KnownPropertyName::PartNumber),
        b"thumbnail" => Some(KnownPropertyName::Thumbnail),
        b"preview" => Some(KnownPropertyName::Preview),
        b"previewimage" => Some(KnownPropertyName::PreviewImage),
        _ => None,
    })
}

fn property_set_name(
    ctx: &DecodeContext<'_>,
    section: &PropertySection<'_>,
) -> Result<Option<String>, CodecError> {
    match ctx.find_by(
        &section.properties,
        |property| Ok(property.id == 255),
        "find Inventor property set name",
    )? {
        Some(property) => property.value.scalar_text(ctx),
        None => Ok(None),
    }
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

fn is_preview(fmtid: &[u8; 16], property_id: u32, name: Option<PropertyName<'_>>) -> bool {
    fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 17
        || matches!(
            name.and_then(|name| name.known),
            Some(
                KnownPropertyName::Thumbnail
                    | KnownPropertyName::Preview
                    | KnownPropertyName::PreviewImage
            )
        )
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

#[cfg(test)]
mod tests;
