// SPDX-License-Identifier: Apache-2.0
//! High-level Inventor structural decode.

use std::collections::BTreeMap;

use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::debug_assert_primary_layer;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::{DecodeReport, TransferLedger};
use cadmpeg_ir::units::{Tolerances, Units};
use cadmpeg_ir::{AnnotationBuilder, NativeUnknownRecord, SourceFidelity, UnknownRecord};

use crate::container::{ContainerPurpose, InventorContainer};
use crate::database::{RevisionPayload, VersionTuple};
use crate::dialect::DialectRecovery;
use crate::external_reference::UfrxState;
use crate::kernel::ActiveCarrierState;
use crate::loss::InventorLossCode;
use crate::native::{
    ActiveCarrierRecord, ActiveCarrierRecordState, AssemblyOccurrenceRecord,
    AssemblyPlacementRecord, AssemblyRecordIssueRecord, DatabaseIssueRecord, DatabaseRecord,
    EmbeddedReferenceRecord, ExternalReferenceRecord, MetaSectionRecord, MetaTypeRecord,
    PmAppDefaultStyleRecord, PmAppRenderingStyleRecord, PmGraphicsFaceRecord,
    PmGraphicsPrimaryColorStyleRecord, PmGraphicsStyleCollectionRecord,
    PresentationRecordIssueRecord, PropertyRecord, PropertySectionRecord, PropertySetIssueRecord,
    PropertySetRecord, ProteinAssetRecord, ProteinEntryRecord, ProteinRecord, ProteinRecordState,
    ProteinRejectionRecord, RevisionRecord, RseRecordRecord, SegmentBulkIssueRecord,
    SegmentBulkRecord, SegmentMetaIssueRecord, SegmentMetaRecord, SegmentPairRecord,
    SegmentRegistryRecord, StorageBandRecord, StructuralIssueRecord, UfrxModelStateParameterRecord,
    UfrxModelStateRecord, UfrxOccurrenceRecord, UfrxRecord, UfrxRecordState,
    UfrxRepresentationRecord, UnpairedSegmentRecord, VersionTupleRecord, INVENTOR_NATIVE_VERSION,
};
use crate::property_set::{PropertySection, PropertySetState, PropertyValue};
use crate::protein::ProteinState;
use crate::rse::{DocumentKind, ParsedState, RecordFrameState, SegmentBulkState, SegmentMetaState};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let purpose = if ctx.container_only() {
        ContainerPurpose::Inspect
    } else {
        ContainerPurpose::Decode
    };
    let container = InventorContainer::open(ctx, root, purpose)?;
    // One predicate, read once from the parsed declarations: it decides the
    // admission in `primary` and the dialect-unverified loss below, and neither
    // recomputes the other.
    let recovery = DialectRecovery::of(&container);
    let primary = recovery.dialect_match();
    let assembly_inventory = crate::assembly::inventory(ctx, &container.rse)?;
    let presentation_inventory = crate::presentation::inventory(ctx, &container.rse)?;
    let design_inventory = crate::design::inventory(ctx, &container.rse)?;
    let sketch_inventory = crate::sketch::inventory(ctx, &container.rse)?;
    let feature_inventory = crate::feature::inventory(ctx, &container.rse)?;
    let mut ir = CadIr::empty(Units::default());
    let (design_parameters, unresolved_design_parameters) =
        crate::design::project_parameters(&design_inventory);
    ir.model.parameters = design_parameters;
    let sketch_projection = crate::sketch::project(&sketch_inventory, &ir.model.parameters);
    let unresolved_sketches = sketch_projection.unresolved_sketches;
    let unresolved_sketch_entities = sketch_projection.unresolved_entities;
    let unresolved_sketch_constraints = sketch_projection.unresolved_constraints;
    ir.model.sketches = sketch_projection.sketches;
    ir.model.sketch_entities = sketch_projection.entities;
    ir.model.sketch_constraints = sketch_projection.constraints;
    let feature_projection = crate::feature::project(
        &feature_inventory,
        &design_inventory,
        &sketch_inventory,
        &ir.model.parameters,
        &ir.model.sketches,
    );
    let unresolved_features = feature_projection.unresolved_features;
    let unresolved_feature_states = feature_projection.unresolved_states;
    ir.model.features = feature_projection.features;
    ir.model.feature_result_topologies = feature_projection.result_topologies;
    // Charge semantic IR before native-arena materialization and kernel BREP
    // transfer so max_entities refuses that work.
    let mut admitted_entities = 0_u64;
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        &mut admitted_entities,
        "admit Inventor semantic entities",
    )?;
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "cfb_major_version".into(),
        container.snapshot.major_version().to_string(),
    );
    attributes.insert(
        "cfb_sector_size".into(),
        container.snapshot.sector_size().to_string(),
    );
    attributes.insert(
        "rse_segment_pairs".into(),
        container.rse.segments.len().to_string(),
    );
    let mut document_kind = container.rse.document_kind();
    let mut metadata = MetadataProjection::default();
    let mut property_sets = Vec::new();
    let mut property_sections = Vec::new();
    let mut properties = Vec::new();
    let mut property_set_issues = Vec::new();
    for descriptor in &container.property_sets {
        match &descriptor.state {
            PropertySetState::Malformed(detail) => {
                property_set_issues.push(PropertySetIssueRecord {
                    id: format!(
                        "inventor:property:set-issue#{}",
                        descriptor.stream.directory_id()
                    ),
                    path: descriptor.path.clone(),
                    directory_id: descriptor.stream.directory_id(),
                    detail: detail.clone(),
                });
            }
            PropertySetState::Parsed(property_set) => {
                property_sets.push(PropertySetRecord {
                    id: format!("inventor:property:set#{}", descriptor.stream.directory_id()),
                    path: descriptor.path.clone(),
                    directory_id: descriptor.stream.directory_id(),
                    version: property_set.version,
                    system_identifier: property_set.system_identifier,
                    clsid: hex(&property_set.clsid),
                    section_count: property_set.sections.len() as u64,
                });
                for (section_ordinal, section) in property_set.sections.iter().enumerate() {
                    let set_name = property_set_name(section);
                    let identity_matches = set_name
                        .as_deref()
                        .and_then(known_property_set_fmtid)
                        .is_none_or(|expected| expected == section.fmtid);
                    if !identity_matches {
                        property_set_issues.push(PropertySetIssueRecord {
                            id: format!(
                                "inventor:property:set-identity#{}-{section_ordinal}",
                                descriptor.stream.directory_id()
                            ),
                            path: descriptor.path.clone(),
                            directory_id: descriptor.stream.directory_id(),
                            detail: "embedded property-set name does not match its FMTID".into(),
                        });
                    }
                    property_sections.push(PropertySectionRecord {
                        id: format!(
                            "inventor:property:section#{}-{section_ordinal}",
                            descriptor.stream.directory_id()
                        ),
                        set_path: descriptor.path.clone(),
                        ordinal: section_ordinal as u32,
                        fmtid: hex(&section.fmtid),
                        code_page: section.code_page,
                        offsets_ordered: section.offsets_ordered,
                        dictionary_entries: section.names.len() as u64,
                        property_count: section.properties.len() as u64,
                    });
                    for property in &section.properties {
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
                                let data = ctx.copy_retained(
                                    bytes,
                                    "retain Inventor preview asset",
                                    Some(property.raw.location()),
                                )?;
                                ir.model.assets.push(Asset {
                                    id: AssetId(format!(
                                        "inventor:document:asset#preview-{}",
                                        ir.model.assets.len()
                                    )),
                                    name: Some("document preview".into()),
                                    media_type: Some(media_type.into()),
                                    content: AssetContent::Embedded { data },
                                    native_ref: Some(native_id.clone()),
                                });
                            }
                        }
                        properties.push(PropertyRecord {
                            id: native_id,
                            set_path: descriptor.path.clone(),
                            section_ordinal: section_ordinal as u32,
                            fmtid: hex(&section.fmtid),
                            property_id: property.id,
                            name: property_name,
                            type_code: property.type_code,
                            value_kind: property_value_kind(&property.value),
                            scalar_value,
                            raw_len: property.raw.window().len() as u64,
                            raw_sha256: sha256_hex(property.raw.window()),
                        });
                    }
                }
            }
        }
    }
    let (protein, protein_entries) = match &container.protein {
        ProteinState::Absent => (
            ProteinRecord {
                id: "inventor:protein:state#root".into(),
                state: ProteinRecordState::Absent,
                directory_id: None,
                declared_len: None,
                entry_count: 0,
                detail: None,
            },
            Vec::new(),
        ),
        ProteinState::Empty { stream } => (
            ProteinRecord {
                id: "inventor:protein:state#root".into(),
                state: ProteinRecordState::Empty,
                directory_id: Some(stream.directory_id()),
                declared_len: Some(0),
                entry_count: 0,
                detail: None,
            },
            Vec::new(),
        ),
        ProteinState::Malformed { stream, detail } => (
            ProteinRecord {
                id: "inventor:protein:state#root".into(),
                state: ProteinRecordState::Malformed,
                directory_id: Some(stream.directory_id()),
                declared_len: None,
                entry_count: 0,
                detail: Some(detail.clone()),
            },
            Vec::new(),
        ),
        ProteinState::Package(package) => {
            let entries = package
                .archive
                .entries()
                .iter()
                .enumerate()
                .map(|(ordinal, entry)| ProteinEntryRecord {
                    id: format!("inventor:protein:entry#{ordinal}"),
                    ordinal: ordinal as u32,
                    name: entry.name.clone(),
                    compression: entry.compression.label().into(),
                    crc32: entry.crc32,
                    compressed_size: entry.compressed_size,
                    uncompressed_size: entry.uncompressed_size,
                })
                .collect::<Vec<_>>();
            (
                ProteinRecord {
                    id: "inventor:protein:state#root".into(),
                    state: ProteinRecordState::Package,
                    directory_id: Some(package.stream.directory_id()),
                    declared_len: Some(package.declared_len),
                    entry_count: entries.len() as u64,
                    detail: None,
                },
                entries,
            )
        }
    };
    let (protein_instances, protein_semantic_issue) = match &container.protein {
        ProteinState::Package(package) => match crate::protein::decode_instances(ctx, package) {
            Ok(instances) => (instances, None),
            Err(error) => (Vec::new(), Some(crate::issue_detail(error)?)),
        },
        ProteinState::Absent | ProteinState::Empty { .. } | ProteinState::Malformed { .. } => {
            (Vec::new(), None)
        }
    };
    let material_catalog = crate::materials::project_catalog(&protein_instances);
    let protein_assets = protein_instances
        .iter()
        .flat_map(|instance| {
            instance.records.iter().map(|asset| ProteinAssetRecord {
                id: format!(
                    "inventor:protein:asset#{}-{}",
                    sha256_hex(instance.entry_name.as_bytes()),
                    asset.ordinal
                ),
                entry_name: instance.entry_name.clone(),
                ordinal: asset.ordinal,
                asset: asset.clone(),
            })
        })
        .collect::<Vec<_>>();
    let protein_rejections = protein_instances
        .iter()
        .flat_map(|instance| {
            instance
                .rejected
                .iter()
                .map(|rejected| ProteinRejectionRecord {
                    id: format!(
                        "inventor:protein:rejection#{}-{}",
                        sha256_hex(instance.entry_name.as_bytes()),
                        rejected.ordinal
                    ),
                    entry_name: instance.entry_name.clone(),
                    ordinal: rejected.ordinal,
                    detail: rejected.detail.clone(),
                })
        })
        .collect::<Vec<_>>();
    ir.model.appearances = material_catalog.appearances;
    let protein_appearance_count = ir.model.appearances.len();
    let ufrx_projection = match &container.ufrx {
        UfrxState::Absent => (
            UfrxRecord {
                id: "inventor:ufrx:state#root".into(),
                state: UfrxRecordState::Absent,
                directory_id: None,
                schema: None,
                section_versions: Vec::new(),
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len: 0,
                tail_sha256: None,
                detail: None,
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        UfrxState::Malformed { stream, detail } => (
            UfrxRecord {
                id: "inventor:ufrx:state#root".into(),
                state: UfrxRecordState::Malformed,
                directory_id: Some(stream.directory_id()),
                schema: None,
                section_versions: Vec::new(),
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len: 0,
                tail_sha256: None,
                detail: Some(detail.clone()),
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        UfrxState::Unsupported {
            stream,
            schema,
            section_versions,
            source,
            detail,
        } => (
            UfrxRecord {
                id: "inventor:ufrx:state#root".into(),
                state: UfrxRecordState::Unsupported,
                directory_id: Some(stream.directory_id()),
                schema: Some(*schema),
                section_versions: section_versions.clone(),
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len: source.window().len() as u64,
                tail_sha256: Some(sha256_hex(source.window())),
                detail: Some(detail.clone()),
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        UfrxState::Parsed(document) => {
            let model_states = document
                .model_states
                .iter()
                .enumerate()
                .map(|(ordinal, state)| UfrxModelStateRecord {
                    id: format!("inventor:ufrx:model-state#{ordinal}"),
                    ordinal: ordinal as u32,
                    prefix: state.prefix,
                    name: state.name.clone(),
                    state: state.state,
                    prefix_count: state.prefix_count,
                    parameters: state
                        .parameters
                        .iter()
                        .map(|parameter| UfrxModelStateParameterRecord {
                            name: parameter.name.clone(),
                            tag: parameter.tag,
                            kind: parameter.kind,
                            state: parameter.state,
                            value: parameter.value.clone(),
                            trailer: parameter.trailer,
                        })
                        .collect(),
                    suffix_len: state.suffix.window().len() as u64,
                    suffix_sha256: sha256_hex(state.suffix.window()),
                })
                .collect::<Vec<_>>();
            let references = document
                .references
                .iter()
                .enumerate()
                .map(|(ordinal, reference)| ExternalReferenceRecord {
                    id: format!("inventor:ufrx:external-reference#{ordinal}"),
                    ordinal: ordinal as u32,
                    path: reference.path.clone(),
                    library_id: reference.library_id,
                    library_name: reference.library_name.clone(),
                    display_name: reference.display_name.clone(),
                    state_groups: reference.state_groups.clone(),
                    state: reference.state,
                    document_id: hex(&reference.document_id),
                    database_id: hex(&reference.database_id),
                    reference_id: reference.reference_id,
                    occurrence_count: reference.occurrence_count,
                    version: reference.version,
                    flags: reference.flags,
                })
                .collect::<Vec<_>>();
            let embedded = document
                .embedded_references
                .iter()
                .enumerate()
                .map(|(ordinal, reference)| EmbeddedReferenceRecord {
                    id: format!("inventor:ufrx:embedded-reference#{ordinal}"),
                    ordinal: ordinal as u32,
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
                    state_values: reference.state_values,
                    record_len: reference.source.window().len() as u64,
                    record_sha256: sha256_hex(reference.source.window()),
                })
                .collect::<Vec<_>>();
            let occurrences = document
                .occurrences
                .iter()
                .enumerate()
                .map(|(ordinal, occurrence)| UfrxOccurrenceRecord {
                    id: format!("inventor:ufrx:occurrence#{ordinal}"),
                    ordinal: ordinal as u32,
                    end_string_flag: occurrence.end_string_flag,
                    file_reference_id: occurrence.file_reference_id,
                    occurrence_id: occurrence.occurrence_id,
                    header_value: occurrence.header_value,
                    title: occurrence.title.clone(),
                    header_padding_words: occurrence.header_padding_words,
                    record_len: occurrence.source.window().len() as u64,
                    record_sha256: sha256_hex(occurrence.source.window()),
                })
                .collect::<Vec<_>>();
            (
                UfrxRecord {
                    id: "inventor:ufrx:state#root".into(),
                    state: UfrxRecordState::ParsedPrefix,
                    directory_id: Some(document.stream.directory_id()),
                    schema: Some(document.schema),
                    section_versions: document.section_versions.clone(),
                    original_file_name: Some(document.original_file_name.clone()),
                    caption: Some(document.caption.clone()),
                    representation: document.representation.as_ref().map(|state| {
                        UfrxRepresentationRecord {
                            prefix: state.prefix,
                            active_representation: state.active_representation.clone(),
                            active_representation_kind: state.active_representation_kind.clone(),
                            secondary_active_lod_state: state.secondary_active_lod_state,
                            active_model_state: state.active_model_state.clone(),
                            active_model_state_state: state.active_model_state_state,
                        }
                    }),
                    model_state_count: model_states.len() as u64,
                    reference_count: references.len() as u64,
                    embedded_reference_count: embedded.len() as u64,
                    occurrence_count: occurrences.len() as u64,
                    tail_len: document.unparsed_tail.window().len() as u64,
                    tail_sha256: Some(sha256_hex(document.unparsed_tail.window())),
                    detail: None,
                },
                model_states,
                embedded,
                references,
                occurrences,
            )
        }
    };
    let (ufrx, ufrx_model_states, embedded_references, external_references, ufrx_occurrences) =
        ufrx_projection;
    if let DocumentKind::Unknown(_) = document_kind {
        if let Some(property_kind) = metadata.document_kind.take() {
            document_kind = property_kind;
        }
    }
    attributes.insert("document_kind".into(), document_kind.label().into());
    metadata.apply_attributes(&mut attributes);
    ir.source = Some(SourceMeta {
        declared: primary.declared.clone(),
        dialect: primary.dialect.clone(),
        format: crate::dialect::FORMAT.into(),
        attributes,
    });
    if matches!(document_kind, DocumentKind::Part | DocumentKind::Assembly) {
        ir.model.product_definitions.push(ProductDefinition {
            id: ProductDefinitionId("inventor:document:product#root".into()),
            kind: if document_kind == DocumentKind::Assembly {
                ProductDefinitionKind::LinkGroup
            } else {
                ProductDefinitionKind::Part
            },
            source_name: metadata.title.clone(),
            label: metadata.title.clone(),
            description: metadata.description.clone(),
            part_number: metadata.part_number.clone(),
            bom_properties: metadata.bom_properties.clone(),
            bodies: Vec::new(),
            native_ref: None,
        });
    }
    let storage_bands = container
        .rse
        .databases
        .iter()
        .map(|database| StorageBandRecord {
            id: format!("inventor:rse:storage-band#v{}", database.band.value()),
            band: database.band.value(),
            database_directory_id: database.stream.directory_id(),
        })
        .collect::<Vec<_>>();
    let databases = container
        .rse
        .databases
        .iter()
        .filter_map(|descriptor| {
            let ParsedState::Parsed(database) = &descriptor.state else {
                return None;
            };
            Some(DatabaseRecord {
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
        .collect::<Vec<_>>();
    let database_issues = container
        .rse
        .databases
        .iter()
        .filter_map(|descriptor| {
            let ParsedState::Unavailable(detail) = &descriptor.state else {
                return None;
            };
            Some(DatabaseIssueRecord {
                id: format!("inventor:rse:database-issue#v{}", descriptor.band.value()),
                band: descriptor.band.value(),
                detail: detail.clone(),
            })
        })
        .collect::<Vec<_>>();
    let segment_registry = match &container.rse.registry {
        ParsedState::Parsed(registry) => registry
            .entries
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| SegmentRegistryRecord {
                id: format!("inventor:rse:registry-entry#{ordinal}"),
                ordinal: ordinal as u32,
                display_name: entry.display_name.clone(),
                segment_id: hex(&entry.segment_id),
                revision_id: hex(&entry.revision_id),
                type_name: entry.type_name.clone(),
                object_count: entry.objects.len() as u64,
                node_count: entry.nodes.len() as u64,
            })
            .collect(),
        ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
    };
    let revisions = match &container.rse.revisions {
        ParsedState::Parsed(table) => table
            .entries
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| RevisionRecord {
                id: format!("inventor:rse:revision#{ordinal}"),
                ordinal: ordinal as u32,
                revision_id: hex(&entry.id),
                flags: entry.flags,
                kind: entry.kind,
                payload_form: match entry.payload {
                    RevisionPayload::None => "none",
                    RevisionPayload::Short { .. } => "short",
                    RevisionPayload::Long { .. } => "long",
                }
                .into(),
            })
            .collect(),
        ParsedState::Absent | ParsedState::Unavailable(_) => Vec::new(),
    };
    let mut structural_issues = Vec::new();
    if let ParsedState::Unavailable(detail) = &container.rse.registry {
        structural_issues.push(structural_issue("segment_registry", detail));
    }
    if let ParsedState::Unavailable(detail) = &container.rse.revisions {
        structural_issues.push(structural_issue("revision_table", detail));
    }
    structural_issues.extend(container.rse.segments.iter().flat_map(|segment| {
        segment
            .identity_issues
            .iter()
            .enumerate()
            .map(move |(ordinal, detail)| StructuralIssueRecord {
                id: format!(
                    "inventor:rse:structural-issue#segment-{}-{ordinal}",
                    segment.pair.token.as_str()
                ),
                scope: format!("segment:{}", segment.pair.token.as_str()),
                detail: detail.clone(),
            })
    }));
    let segment_pairs = container
        .rse
        .segments
        .iter()
        .map(|segment| SegmentPairRecord {
            id: format!("inventor:rse:segment#{}", segment.pair.token.as_str()),
            token: segment.pair.token.as_str().into(),
            metadata_directory_id: segment.pair.metadata.directory_id(),
            bulk_directory_id: segment.pair.bulk.directory_id(),
        })
        .collect::<Vec<_>>();
    let segment_meta = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let SegmentMetaState::Parsed(meta) = &segment.meta else {
                return None;
            };
            Some(SegmentMetaRecord {
                id: format!("inventor:rse:segment-meta#{}", segment.pair.token.as_str()),
                token: segment.pair.token.as_str().into(),
                version: meta.version.value(),
                kind: segment.kind.label().into(),
                display_name: meta.display_name.clone(),
                segment_id: hex(&meta.segment_id),
                header_values: meta.header_values,
                state_words: meta.state_words,
                created: meta.created.clone(),
                modified: meta.modified.clone(),
                body_form: meta.body_form,
                expanded_body_len: meta.body.window().len() as u64,
                expanded_body_sha256: sha256_hex(meta.body.window()),
                table_prefix: meta.tables.prefix,
                block_count: meta.tables.blocks.len() as u64,
                type_count: meta.tables.types.len() as u64,
                terminal_id: hex(&meta.tables.terminal_id),
            })
        })
        .collect::<Vec<_>>();
    let meta_sections = container
        .rse
        .segments
        .iter()
        .flat_map(|segment| {
            let SegmentMetaState::Parsed(meta) = &segment.meta else {
                return Vec::new();
            };
            meta.tables
                .sections
                .iter()
                .map(|section| MetaSectionRecord {
                    id: format!(
                        "inventor:rse:meta-section#{}-{}",
                        segment.pair.token.as_str(),
                        section.number
                    ),
                    token: segment.pair.token.as_str().into(),
                    number: section.number,
                    discriminator: section.discriminator,
                    payload_len: section.payload.window().len() as u64,
                    payload_sha256: sha256_hex(section.payload.window()),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let meta_types = container
        .rse
        .segments
        .iter()
        .flat_map(|segment| {
            let SegmentMetaState::Parsed(meta) = &segment.meta else {
                return Vec::new();
            };
            meta.tables
                .types
                .iter()
                .map(|descriptor| MetaTypeRecord {
                    id: format!(
                        "inventor:rse:meta-type#{}-{}",
                        segment.pair.token.as_str(),
                        descriptor.index
                    ),
                    token: segment.pair.token.as_str().into(),
                    index: descriptor.index,
                    type_id: hex(&descriptor.id),
                    fields: descriptor.fields,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let segment_meta_issues = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let (status, detail) = match &segment.meta {
                SegmentMetaState::Parsed(_) => return None,
                SegmentMetaState::Unsupported(declared) => (
                    "unsupported",
                    format!("marker {:?}, version {}", declared.marker, declared.version),
                ),
                SegmentMetaState::Malformed { detail, .. } => ("malformed", detail.clone()),
            };
            Some(SegmentMetaIssueRecord {
                id: format!(
                    "inventor:rse:segment-meta-issue#{}",
                    segment.pair.token.as_str()
                ),
                token: segment.pair.token.as_str().into(),
                status: status.into(),
                detail,
            })
        })
        .collect::<Vec<_>>();
    let rse_records = container
        .rse
        .segments
        .iter()
        .flat_map(|segment| {
            let SegmentBulkState::Framed(bulk) = &segment.bulk else {
                return Vec::new();
            };
            let RecordFrameState::Framed(table) = &bulk.records else {
                return Vec::new();
            };
            table
                .records
                .iter()
                .map(|record| RseRecordRecord {
                    id: format!(
                        "inventor:rse:record#{}-{}",
                        segment.pair.token.as_str(),
                        record.ordinal
                    ),
                    token: segment.pair.token.as_str().into(),
                    ordinal: record.ordinal,
                    selector: record.selector,
                    type_index: record.type_index,
                    type_id: hex(&record.type_id),
                    payload_offset: record.payload_offset,
                    payload_len: record.declared_payload_len as u64,
                    payload_sha256: sha256_hex(record.payload.window()),
                    trailing_payload_len: record.trailing_payload_len,
                    trailer_len: record.trailer.window().len() as u64,
                    trailer_sha256: sha256_hex(record.trailer.window()),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let segment_bulk = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let SegmentBulkState::Framed(bulk) = &segment.bulk else {
                return None;
            };
            let (
                record_state,
                record_count,
                stream_trailer_len,
                stream_trailer_sha256,
                record_detail,
            ) = match &bulk.records {
                RecordFrameState::NotExpanded => ("not_expanded", 0, None, None, None),
                RecordFrameState::Framed(table) => (
                    "framed",
                    table.records.len() as u64,
                    Some(table.stream_trailer.window().len() as u64),
                    Some(sha256_hex(table.stream_trailer.window())),
                    None,
                ),
                RecordFrameState::Unavailable(detail) => {
                    ("unavailable", 0, None, None, Some(detail.clone()))
                }
            };
            Some(SegmentBulkRecord {
                id: format!("inventor:rse:segment-bulk#{}", segment.pair.token.as_str()),
                token: segment.pair.token.as_str().into(),
                prefix: hex(&bulk.prefix),
                form: bulk.form.value(),
                compressed_len: bulk.compressed.window().len() as u64,
                compressed_sha256: sha256_hex(bulk.compressed.window()),
                expanded_len: bulk.expanded.map(|view| view.window().len() as u64),
                expanded_sha256: bulk.expanded.map(|view| sha256_hex(view.window())),
                record_state: record_state.into(),
                record_count,
                stream_trailer_len,
                stream_trailer_sha256,
                record_detail,
            })
        })
        .collect::<Vec<_>>();
    let segment_bulk_issues = container
        .rse
        .segments
        .iter()
        .filter_map(|segment| {
            let SegmentBulkState::Malformed(detail) = &segment.bulk else {
                return None;
            };
            Some(SegmentBulkIssueRecord {
                id: format!(
                    "inventor:rse:segment-bulk-issue#{}",
                    segment.pair.token.as_str()
                ),
                token: segment.pair.token.as_str().into(),
                detail: detail.clone(),
            })
        })
        .collect::<Vec<_>>();
    let unpaired_segments = container
        .rse
        .unpaired_metadata
        .iter()
        .map(|token| UnpairedSegmentRecord {
            id: format!("inventor:rse:unpaired-metadata#{}", token.as_str()),
            token: token.as_str().into(),
            missing_member: "bulk".into(),
        })
        .chain(
            container
                .rse
                .unpaired_bulk
                .iter()
                .map(|token| UnpairedSegmentRecord {
                    id: format!("inventor:rse:unpaired-bulk#{}", token.as_str()),
                    token: token.as_str().into(),
                    missing_member: "metadata".into(),
                }),
        )
        .collect::<Vec<_>>();
    let active_carrier = match &container.rse.active_carrier {
        ActiveCarrierState::NotApplicable => ActiveCarrierRecord {
            id: "inventor:kernel:active-carrier#root".into(),
            state: ActiveCarrierRecordState::NotApplicable,
            segment_token: None,
            record_ordinal: None,
            segment_version_major: None,
            family: None,
            header_state: None,
            header_kind: None,
            header_value: None,
            schema: None,
            carrier_len: None,
            carrier_offset: None,
            carrier_sha256: None,
            selected_key: None,
            enabled: None,
            delta_state: None,
            history_reference: None,
            detail: None,
        },
        ActiveCarrierState::NotExpanded => ActiveCarrierRecord {
            id: "inventor:kernel:active-carrier#root".into(),
            state: ActiveCarrierRecordState::NotExpanded,
            segment_token: None,
            record_ordinal: None,
            segment_version_major: None,
            family: None,
            header_state: None,
            header_kind: None,
            header_value: None,
            schema: None,
            carrier_len: None,
            carrier_offset: None,
            carrier_sha256: None,
            selected_key: None,
            enabled: None,
            delta_state: None,
            history_reference: None,
            detail: None,
        },
        ActiveCarrierState::Unavailable(detail) => ActiveCarrierRecord {
            id: "inventor:kernel:active-carrier#root".into(),
            state: ActiveCarrierRecordState::Unavailable,
            segment_token: None,
            record_ordinal: None,
            segment_version_major: None,
            family: None,
            header_state: None,
            header_kind: None,
            header_value: None,
            schema: None,
            carrier_len: None,
            carrier_offset: None,
            carrier_sha256: None,
            selected_key: None,
            enabled: None,
            delta_state: None,
            history_reference: None,
            detail: Some(detail.clone()),
        },
        ActiveCarrierState::Selected(carrier) => ActiveCarrierRecord {
            id: "inventor:kernel:active-carrier#root".into(),
            state: ActiveCarrierRecordState::Selected,
            segment_token: Some(carrier.segment_token.clone()),
            record_ordinal: Some(carrier.record_ordinal),
            segment_version_major: Some(carrier.segment_version_major),
            family: Some(carrier.family.label().into()),
            header_state: Some(carrier.header_state),
            header_kind: Some(carrier.header_kind),
            header_value: Some(carrier.header_value),
            schema: Some(carrier.schema),
            carrier_len: Some(carrier.bytes.window().len() as u64),
            carrier_offset: Some(carrier.carrier_offset),
            carrier_sha256: Some(sha256_hex(carrier.bytes.window())),
            selected_key: Some(carrier.selected_key),
            enabled: Some(carrier.enabled),
            delta_state: Some(carrier.delta_state),
            history_reference: Some(carrier.history_reference),
            detail: None,
        },
    };
    let assembly_occurrences = assembly_inventory
        .occurrences
        .iter()
        .map(|occurrence| AssemblyOccurrenceRecord {
            id: format!(
                "inventor:assembly:occurrence#{}-{}",
                occurrence.segment_token, occurrence.record_ordinal
            ),
            segment_token: occurrence.segment_token.clone(),
            record_ordinal: occurrence.record_ordinal,
            header_value: occurrence.header_value,
            header_id: occurrence.header_id,
            next_reference: occurrence.next_reference,
            flags: occurrence.flags,
            owner_reference: occurrence.owner_reference,
            node_index: occurrence.node_index,
            state: occurrence.state,
            ordinal_key: occurrence.ordinal_key,
            related_references: occurrence.related_references.clone(),
            child_reference: occurrence.child_reference,
            occurrence_id: occurrence.occurrence_id,
        })
        .collect::<Vec<_>>();
    let assembly_placements = assembly_inventory
        .placements
        .iter()
        .map(|placement| AssemblyPlacementRecord {
            id: format!(
                "inventor:assembly:placement#{}-{}",
                placement.segment_token, placement.record_ordinal
            ),
            segment_token: placement.segment_token.clone(),
            record_ordinal: placement.record_ordinal,
            header_id: placement.header_id,
            owner_reference: placement.owner_reference,
            attribute_reference: placement.attribute_reference,
            state: placement.state,
            transform_prefix: placement.transform_prefix,
            transform_encoding: placement.transform_encoding,
            transform: placement.transform,
            branch: placement.branch,
            graphics_state: placement.graphics_state,
            occurrence_id: placement.occurrence_id,
            graphics_index: placement.graphics_index,
            object_reference: placement.object_reference,
            suffix_len: placement.suffix.window().len() as u64,
            suffix_sha256: sha256_hex(placement.suffix.window()),
        })
        .collect::<Vec<_>>();
    let assembly_record_issues = assembly_inventory
        .issues
        .iter()
        .map(|issue| AssemblyRecordIssueRecord {
            id: format!(
                "inventor:assembly:record-issue#{}-{}",
                issue.segment_token, issue.record_ordinal
            ),
            segment_token: issue.segment_token.clone(),
            record_ordinal: issue.record_ordinal,
            detail: issue.detail.clone(),
        })
        .collect::<Vec<_>>();
    let pm_app_default_styles = presentation_inventory
        .default_styles
        .iter()
        .map(|style| {
            let (suffix_len, suffix_sha256) = crate::presentation::suffix_fields(style.suffix);
            PmAppDefaultStyleRecord {
                id: format!(
                    "inventor:presentation:default-style#{}-{}",
                    style.segment_token, style.record_ordinal
                ),
                segment_token: style.segment_token.clone(),
                record_ordinal: style.record_ordinal,
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
        .map(|style| {
            let (suffix_len, suffix_sha256) = crate::presentation::suffix_fields(style.suffix);
            PmAppRenderingStyleRecord {
                id: format!(
                    "inventor:presentation:rendering-style#{}-{}",
                    style.segment_token, style.record_ordinal
                ),
                segment_token: style.segment_token.clone(),
                record_ordinal: style.record_ordinal,
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
                style_state: style.style_state,
                style_label: style.style_label.clone(),
                asset_guid: style.asset_guid.clone(),
                material_id: style.material_id.clone(),
                asset_library_id: style.asset_library_id.clone(),
                style_values: style.style_values,
                guid: style.guid.clone(),
                suffix_len,
                suffix_sha256,
            }
        })
        .collect::<Vec<_>>();
    let pm_graphics_faces = presentation_inventory
        .graphics_faces
        .iter()
        .map(|face| PmGraphicsFaceRecord {
            id: format!(
                "inventor:presentation:graphics-face#{}-{}",
                face.segment_token, face.record_ordinal
            ),
            segment_token: face.segment_token.clone(),
            record_ordinal: face.record_ordinal,
            segment_version_major: face.segment_version_major,
            header_value: face.header_value,
            header_id: face.header_id,
            flags: face.flags,
            styles_reference: face.styles_reference,
            styles_reference_qualified: face.styles_reference_qualified,
            surface_reference: face.surface_reference,
            surface_reference_qualified: face.surface_reference_qualified,
            parent_reference: face.parent_reference,
            parent_reference_qualified: face.parent_reference_qualified,
            state: face.state,
            edge_references: face.edge_references.clone(),
            edge_reference_qualifiers: face.edge_reference_qualifiers.clone(),
            edge_list_metadata: face.edge_list_metadata,
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
                collection.segment_token, collection.record_ordinal
            ),
            segment_token: collection.segment_token.clone(),
            record_ordinal: collection.record_ordinal,
            segment_version_major: collection.segment_version_major,
            style_references: collection.style_references.clone(),
            style_reference_qualifiers: collection.style_reference_qualifiers.clone(),
            list_metadata: collection.list_metadata,
        })
        .collect::<Vec<_>>();
    let pm_graphics_primary_color_styles = presentation_inventory
        .graphics_primary_color_styles
        .iter()
        .map(|style| PmGraphicsPrimaryColorStyleRecord {
            id: format!(
                "inventor:presentation:graphics-primary-color#{}-{}",
                style.segment_token, style.record_ordinal
            ),
            segment_token: style.segment_token.clone(),
            record_ordinal: style.record_ordinal,
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
    let presentation_record_issues = presentation_inventory
        .issues
        .iter()
        .map(|issue| PresentationRecordIssueRecord {
            id: format!(
                "inventor:presentation:record-issue#{}-{}",
                issue.segment_token, issue.record_ordinal
            ),
            segment_token: issue.segment_token.clone(),
            record_ordinal: issue.record_ordinal,
            detail: issue.detail.clone(),
        })
        .collect::<Vec<_>>();
    let assembly_projection = crate::assembly::project_occurrences(
        &ufrx_occurrences,
        &external_references,
        &assembly_occurrences,
        &assembly_placements,
    );
    ir.model.occurrences = assembly_projection.occurrences;
    ctx.charge_collection_items(
        storage_bands
            .len()
            .saturating_add(segment_pairs.len())
            .saturating_add(databases.len())
            .saturating_add(database_issues.len())
            .saturating_add(segment_registry.len())
            .saturating_add(revisions.len())
            .saturating_add(structural_issues.len())
            .saturating_add(segment_meta.len())
            .saturating_add(meta_sections.len())
            .saturating_add(meta_types.len())
            .saturating_add(segment_meta_issues.len())
            .saturating_add(segment_bulk.len())
            .saturating_add(rse_records.len())
            .saturating_add(segment_bulk_issues.len())
            .saturating_add(property_sets.len())
            .saturating_add(property_sections.len())
            .saturating_add(properties.len())
            .saturating_add(property_set_issues.len())
            .saturating_add(1)
            .saturating_add(protein_entries.len())
            .saturating_add(protein_assets.len())
            .saturating_add(protein_rejections.len())
            .saturating_add(1)
            .saturating_add(ufrx_model_states.len())
            .saturating_add(embedded_references.len())
            .saturating_add(ufrx_occurrences.len())
            .saturating_add(external_references.len())
            .saturating_add(assembly_occurrences.len())
            .saturating_add(assembly_placements.len())
            .saturating_add(assembly_record_issues.len())
            .saturating_add(pm_app_default_styles.len())
            .saturating_add(pm_app_rendering_styles.len())
            .saturating_add(pm_graphics_faces.len())
            .saturating_add(pm_graphics_style_collections.len())
            .saturating_add(pm_graphics_primary_color_styles.len())
            .saturating_add(presentation_record_issues.len())
            .saturating_add(design_inventory.parameters.len())
            .saturating_add(design_inventory.expressions.len())
            .saturating_add(design_inventory.units.len())
            .saturating_add(design_inventory.issues.len())
            .saturating_add(sketch_inventory.sketches.len())
            .saturating_add(sketch_inventory.entities.len())
            .saturating_add(sketch_inventory.transforms.len())
            .saturating_add(sketch_inventory.directions.len())
            .saturating_add(sketch_inventory.constraints.len())
            .saturating_add(sketch_inventory.issues.len())
            .saturating_add(feature_inventory.features.len())
            .saturating_add(feature_inventory.pattern_features.len())
            .saturating_add(feature_inventory.terminators.len())
            .saturating_add(feature_inventory.issues.len())
            .saturating_add(unpaired_segments.len())
            .saturating_add(1) as u64,
        "retain Inventor native structural records",
    )?;
    let namespace = ir.native.namespace_mut("inventor");
    namespace.version = INVENTOR_NATIVE_VERSION;
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
    namespace.set_arena("protein", std::slice::from_ref(&protein))?;
    namespace.set_arena("protein_entries", &protein_entries)?;
    namespace.set_arena("protein_assets", &protein_assets)?;
    namespace.set_arena("protein_rejections", &protein_rejections)?;
    namespace.set_arena("ufrx", std::slice::from_ref(&ufrx))?;
    namespace.set_arena("ufrx_model_states", &ufrx_model_states)?;
    namespace.set_arena("embedded_references", &embedded_references)?;
    namespace.set_arena("ufrx_occurrences", &ufrx_occurrences)?;
    namespace.set_arena("external_references", &external_references)?;
    namespace.set_arena("assembly_occurrences", &assembly_occurrences)?;
    namespace.set_arena("assembly_placements", &assembly_placements)?;
    namespace.set_arena("assembly_record_issues", &assembly_record_issues)?;
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
    namespace.set_arena("presentation_record_issues", &presentation_record_issues)?;
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
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        &mut admitted_entities,
        "admit Inventor entities",
    )?;

    let mut geometry_failure = None;
    let kernel_brep = match &container.rse.active_carrier {
        ActiveCarrierState::Selected(carrier) => {
            match crate::kernel::decode_kernel_carrier(ctx, carrier) {
                Ok(decoded) => {
                    apply_kernel_header(&mut ir, carrier.family, &decoded.header);
                    Some(decoded.brep)
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    geometry_failure = Some(error.to_string());
                    None
                }
            }
        }
        _ => None,
    };
    let AsmTransferRemainder {
        body_keys: _,
        face_keys,
        unknowns: kernel_unknowns,
        stats: kernel_stats,
        annotation_records: kernel_annotations,
    } = transfer_into_ir(
        ctx,
        &mut ir,
        "inventor",
        INVENTOR_NATIVE_VERSION,
        kernel_brep.unwrap_or_else(AsmBrep::default),
    )?;
    ir.set_native_unknowns("inventor", &[] as &[NativeUnknownRecord])?;
    let geometry_transferred =
        !(ir.model.surfaces.is_empty() && ir.model.points.is_empty() && ir.model.faces.is_empty());
    if geometry_transferred {
        let body_ids = ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>();
        for product in &mut ir.model.product_definitions {
            product.bodies.clone_from(&body_ids);
        }
    } else if matches!(
        &container.rse.active_carrier,
        ActiveCarrierState::Selected(_)
    ) && geometry_failure.is_none()
    {
        geometry_failure =
            Some("the active kernel carrier decoded no surfaces, points, or faces".into());
    }
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let presentation_projection = crate::presentation::project_bindings(
        &presentation_inventory,
        &ir.model.appearances,
        &body_ids,
        &face_keys,
    );
    ir.model
        .appearances
        .extend(presentation_projection.appearances.clone());
    ir.model.appearance_bindings = presentation_projection.bindings;
    let projected_colors = presentation_projection
        .appearances
        .iter()
        .filter_map(|appearance| Some((appearance.id.clone(), appearance.base_color?)))
        .collect::<std::collections::HashMap<_, _>>();
    let face_colors = ir
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| match &binding.target {
            cadmpeg_ir::appearance::AppearanceTarget::Face(face) => projected_colors
                .get(&binding.appearance)
                .copied()
                .map(|color| (face.clone(), color)),
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    for face in &mut ir.model.faces {
        if face.color.is_none() {
            face.color = face_colors.get(&face.id).copied();
        }
    }
    let mut losses = Vec::new();
    losses.extend(recovery.dialect_loss());
    if ctx.container_only() {
        losses.push(
            InventorLossCode::ContainerOnlyDecode.note("Container-only decode was requested."),
        );
    } else if !matches!(document_kind, DocumentKind::Assembly) && !geometry_transferred {
        let detail = geometry_failure.unwrap_or_else(|| match &container.rse.active_carrier {
            ActiveCarrierState::Selected(_) => {
                "The typed active kernel carrier has not been transferred.".into()
            }
            ActiveCarrierState::Unavailable(detail) => {
                format!("The active Inventor kernel carrier is unavailable: {detail}")
            }
            ActiveCarrierState::NotApplicable => {
                "Inventor geometry is not available for this document kind.".into()
            }
            ActiveCarrierState::NotExpanded => {
                unreachable!("non-container decode expands RSe bulk streams")
            }
        });
        losses.push(InventorLossCode::GeometryKernelCarrierNotTransferred.note(detail));
    }
    if !ctx.container_only() {
        if kernel_stats.unknown_surface_faces != 0 {
            losses.push(
                InventorLossCode::GeometryProceduralSurfaceNotTransferred.note(format!(
                    "{} face(s) use procedural surfaces without a decoded carrier.",
                    kernel_stats.unknown_surface_faces
                )),
            );
        }
        if !segment_pairs.is_empty() {
            losses.push(InventorLossCode::RseSegmentPairUntyped.note(format!(
                "Retained {} structurally paired RSe segment(s) without record semantics.",
                segment_pairs.len()
            )));
        }
        if !segment_meta_issues.is_empty() {
            losses.push(InventorLossCode::RseMetadataStreamMalformed.note(format!(
                "{} RSe metadata stream(s) are malformed or outside the implemented envelope.",
                segment_meta_issues.len()
            )));
        }
        if !segment_bulk_issues.is_empty() {
            losses.push(InventorLossCode::RseBulkStreamMalformed.note(format!(
                "{} RSe bulk stream(s) have invalid envelope or zlib framing.",
                segment_bulk_issues.len()
            )));
        }
        if !assembly_record_issues.is_empty() {
            losses.push(InventorLossCode::AssemblyRecordMalformed.note(format!(
                "{} typed Inventor assembly record(s) are malformed or outside the implemented branch.",
                assembly_record_issues.len()
            )));
        }
        if !presentation_record_issues.is_empty() {
            losses.push(InventorLossCode::PresentationRecordMalformed.note(format!(
                "{} typed Inventor presentation record(s) are malformed or outside the implemented branch.",
                presentation_record_issues.len()
            )));
        }
        if !design_inventory.issues.is_empty() {
            losses.push(InventorLossCode::DesignRecordMalformed.note(format!(
                "{} typed Inventor design record(s) are malformed or outside the implemented branch.",
                design_inventory.issues.len()
            )));
        }
        if !sketch_inventory.issues.is_empty() {
            losses.push(InventorLossCode::SketchRecordMalformed.note(format!(
                "{} typed Inventor sketch record(s) could not be parsed exactly.",
                sketch_inventory.issues.len()
            )));
        }
        if !feature_inventory.issues.is_empty() {
            losses.push(InventorLossCode::FeatureRecordMalformed.note(format!(
                "{} typed Inventor feature record(s) could not be parsed exactly.",
                feature_inventory.issues.len()
            )));
        }
        if unresolved_features != 0 {
            losses.push(InventorLossCode::FeatureOperationGraphOpen.note(format!(
                "Retained {unresolved_features} typed Inventor feature record(s) whose operation graph is not closed."
            )));
        }
        if unresolved_feature_states != 0 {
            losses.push(InventorLossCode::FeatureStateUnresolved.note(format!(
                "Transferred {unresolved_feature_states} Inventor operation(s) with native result-body identity and unresolved suppression and dependency state."
            )));
        }
        if unresolved_design_parameters != 0 {
            losses.push(InventorLossCode::ParameterGraphOpen.note(format!(
                "Retained {unresolved_design_parameters} Inventor parameter record(s) whose unit or expression graph is not closed."
            )));
        }
        if unresolved_sketches != 0
            || unresolved_sketch_entities != 0
            || unresolved_sketch_constraints != 0
        {
            losses.push(InventorLossCode::SketchGraphOpen.note(format!(
                "Retained {unresolved_sketches} Inventor sketch record(s), {unresolved_sketch_entities} sketch-entity record(s), and {unresolved_sketch_constraints} sketch-constraint record(s) whose neutral graph is not closed."
            )));
        }
        if !container.rse.unpaired_metadata.is_empty() || !container.rse.unpaired_bulk.is_empty() {
            losses.push(InventorLossCode::RseStreamUnpaired.note(format!(
                "RSe contains {} unpaired metadata stream(s) and {} unpaired bulk stream(s).",
                container.rse.unpaired_metadata.len(),
                container.rse.unpaired_bulk.len()
            )));
        }
        if !property_set_issues.is_empty() {
            losses.push(InventorLossCode::PropertySetStreamMalformed.note(format!(
                "{} OLE property-set stream(s) are malformed.",
                property_set_issues.len()
            )));
        }
        if metadata.unmapped != 0 {
            losses.push(InventorLossCode::MetadataPropertyUnmapped.note(format!(
                "Retained {} property value(s) without neutral metadata mapping.",
                metadata.unmapped
            )));
        }
        match &container.protein {
            ProteinState::Package(_) => {
                if let Some(detail) = &protein_semantic_issue {
                    losses.push(InventorLossCode::ProteinCatalogUndecodable.note(format!(
                        "The Protein asset catalog could not be decoded: {detail}"
                    )));
                } else {
                    if !protein_rejections.is_empty() {
                        losses.push(InventorLossCode::ProteinAssetRejected.note(format!(
                            "Rejected {} malformed Protein asset record(s); later framed records remain decoded.",
                            protein_rejections.len()
                        )));
                    }
                    if ir.model.appearances.is_empty() {
                        losses
                            .push(InventorLossCode::ProteinAppearanceAbsent.note(
                                "The Protein package contains no decoded appearance assets.",
                            ));
                    } else if presentation_projection.unresolved_defaults != 0 {
                        losses.push(InventorLossCode::AppearanceDefaultUnresolved.note(format!(
                            "Could not resolve {} PmApp document-default appearance assignment(s).",
                            presentation_projection.unresolved_defaults
                        )));
                    }
                }
                if !material_catalog.duplicate_guids.is_empty() {
                    losses.push(InventorLossCode::ProteinGuidAmbiguous.note(format!(
                        "The Protein catalog contains {} duplicate asset GUID(s); ambiguous texture joins were refused.",
                        material_catalog.duplicate_guids.len()
                    )));
                }
            }
            ProteinState::Malformed { .. } => losses.push(
                InventorLossCode::ProteinStreamMalformed
                    .note("The Inventor Protein stream is malformed."),
            ),
            ProteinState::Absent | ProteinState::Empty { .. } => {}
        }
        if presentation_projection.unresolved_face_overrides != 0 {
            losses.push(
                InventorLossCode::AppearanceFaceOverrideUnresolved.note(format!(
                    "Could not resolve {} PmGraphics face appearance override(s).",
                    presentation_projection.unresolved_face_overrides
                )),
            );
        }
        match &container.ufrx {
            UfrxState::Malformed { .. } => losses.push(
                InventorLossCode::UfrxTableMalformed
                    .note("The UFRxDoc external-reference table is malformed."),
            ),
            UfrxState::Unsupported { schema, .. } => {
                let code = if matches!(document_kind, DocumentKind::Assembly) {
                    InventorLossCode::UfrxSchemaUnsupportedAssembly
                } else {
                    InventorLossCode::UfrxSchemaUnsupported
                };
                losses.push(code.note(format!(
                    "Retained unsupported UFRxDoc schema {schema} semantic branch without transfer."
                )));
            }
            UfrxState::Parsed(_) if matches!(document_kind, DocumentKind::Assembly) => {
                if !external_references.is_empty() {
                    losses.push(InventorLossCode::AssemblyComponentExternal.note(format!(
                        "Retained {} unresolved external component reference(s).",
                        external_references.len()
                    )));
                }
                if assembly_projection.unresolved_placements != 0 {
                    losses.push(
                        InventorLossCode::AssemblyPlacementNotTransferred.note(format!(
                            "Could not transfer {} assembly occurrence placement(s).",
                            assembly_projection.unresolved_placements
                        )),
                    );
                }
            }
            UfrxState::Absent | UfrxState::Parsed(_) => {}
        }
    }
    let preview_asset_count = ir.model.assets.len();
    let face_color_appearance_count = presentation_projection.appearances.len();
    let mut source_fidelity = SourceFidelity::default();
    let mut annotations = AnnotationBuilder::new();
    for record in kernel_annotations {
        let stream = annotations.stream(format!("inventor:{}", record.stream));
        annotations
            .note(&record.id, stream, record.offset)
            .tag(record.tag);
        for field in record.derived_fields {
            annotations.derived(&record.id, field);
        }
    }
    source_fidelity.annotations = annotations.build();
    if let ActiveCarrierState::Selected(carrier) = &container.rse.active_carrier {
        let unsupported_acis = carrier.family == crate::kernel::KernelFamily::Acis
            && !matches!(
                cadmpeg_asm::acis_header::parse(carrier.bytes.window())
                    .and_then(|header| header.save_format_major()),
                Some(217 | 218)
            );
        if unsupported_acis {
            let data = ctx.copy_retained(
                carrier.bytes.window(),
                "retain unsupported Inventor ACIS carrier",
                Some(carrier.bytes.location()),
            )?;
            source_fidelity.retain_unknown_records(
                &format!("RSeStorage/B{}:expanded", carrier.segment_token),
                [UnknownRecord {
                    id: UnknownId(format!(
                        "inventor:kernel:carrier#{}-{}",
                        carrier.segment_token, carrier.record_ordinal
                    )),
                    offset: carrier.carrier_offset,
                    byte_len: carrier.bytes.window().len() as u64,
                    sha256: sha256_hex(carrier.bytes.window()),
                    data: Some(data),
                    links: vec![active_carrier.id.clone()],
                }],
            );
        }
    }
    if !kernel_unknowns.is_empty() {
        source_fidelity
            .attach_native_unknown_records(&mut ir, "inventor", kernel_unknowns)
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "Inventor kernel unknown retention failed: {error}"
                ))
            })?;
    }
    source_fidelity.finalize();
    let kernel_unknown_record_count = ir.native_unknowns("inventor")?.len();
    let appearance_binding_count = ir.model.appearance_bindings.len();
    let transferred_occurrence_count = ir.model.occurrences.len();
    let design_parameter_count = ir.model.parameters.len();
    let transferred_sketch_count = ir.model.sketches.len();
    let transferred_sketch_entity_count = ir.model.sketch_entities.len();
    let transferred_sketch_constraint_count = ir.model.sketch_constraints.len();
    let transferred_feature_count = ir.model.features.len();
    let transferred_feature_result_count = ir.model.feature_result_topologies.len();
    let dialects = vec![primary];
    debug_assert_primary_layer(&dialects, crate::dialect::FORMAT);
    Ok(DecodeResult::new(
        ir,
        DecodeReport {
            dialects,
            format: crate::dialect::FORMAT.into(),
            container_only: ctx.container_only(),
            geometry_transferred,
            coverage: BTreeMap::from([
                ("rse_storage_bands".into(), storage_bands.len()),
                ("rse_databases".into(), databases.len()),
                ("rse_registry_entries".into(), segment_registry.len()),
                ("rse_revisions".into(), revisions.len()),
                ("rse_segment_pairs".into(), segment_pairs.len()),
                ("rse_segment_meta".into(), segment_meta.len()),
                ("rse_meta_types".into(), meta_types.len()),
                ("rse_segment_meta_issues".into(), segment_meta_issues.len()),
                ("rse_segment_bulk".into(), segment_bulk.len()),
                ("rse_records".into(), rse_records.len()),
                ("rse_segment_bulk_issues".into(), segment_bulk_issues.len()),
                ("property_sets".into(), property_sets.len()),
                ("properties".into(), properties.len()),
                ("preview_assets".into(), preview_asset_count),
                ("protein_entries".into(), protein_entries.len()),
                ("protein_assets".into(), protein_assets.len()),
                ("protein_rejections".into(), protein_rejections.len()),
                ("protein_appearances".into(), protein_appearance_count),
                (
                    "appearance_bindings_transferred".into(),
                    appearance_binding_count,
                ),
                ("pm_app_default_styles".into(), pm_app_default_styles.len()),
                (
                    "pm_app_rendering_styles".into(),
                    pm_app_rendering_styles.len(),
                ),
                ("pm_graphics_faces".into(), pm_graphics_faces.len()),
                (
                    "pm_graphics_style_collections".into(),
                    pm_graphics_style_collections.len(),
                ),
                (
                    "pm_graphics_primary_color_styles".into(),
                    pm_graphics_primary_color_styles.len(),
                ),
                ("face_color_appearances".into(), face_color_appearance_count),
                (
                    "presentation_record_issues".into(),
                    presentation_record_issues.len(),
                ),
                ("pm_dc_parameters".into(), design_inventory.parameters.len()),
                (
                    "pm_dc_expressions".into(),
                    design_inventory.expressions.len(),
                ),
                ("pm_dc_units".into(), design_inventory.units.len()),
                (
                    "design_parameters_transferred".into(),
                    design_parameter_count,
                ),
                ("design_record_issues".into(), design_inventory.issues.len()),
                ("pm_dc_sketches".into(), sketch_inventory.sketches.len()),
                (
                    "pm_dc_sketch_entities".into(),
                    sketch_inventory.entities.len(),
                ),
                ("pm_dc_transforms".into(), sketch_inventory.transforms.len()),
                ("pm_dc_directions".into(), sketch_inventory.directions.len()),
                (
                    "pm_dc_sketch_constraints".into(),
                    sketch_inventory.constraints.len(),
                ),
                ("sketch_record_issues".into(), sketch_inventory.issues.len()),
                ("pm_dc_features".into(), feature_inventory.features.len()),
                (
                    "pm_dc_pattern_features".into(),
                    feature_inventory.pattern_features.len(),
                ),
                (
                    "pm_dc_feature_terminators".into(),
                    feature_inventory.terminators.len(),
                ),
                (
                    "pm_dc_feature_properties".into(),
                    feature_inventory.properties.len(),
                ),
                (
                    "pm_dc_feature_labels".into(),
                    feature_inventory.labels.len(),
                ),
                (
                    "pm_dc_entity_style_links".into(),
                    feature_inventory.entity_style_links.len(),
                ),
                (
                    "feature_record_issues".into(),
                    feature_inventory.issues.len(),
                ),
                ("features_transferred".into(), transferred_feature_count),
                (
                    "feature_result_topologies_transferred".into(),
                    transferred_feature_result_count,
                ),
                ("sketches_transferred".into(), transferred_sketch_count),
                (
                    "sketch_entities_transferred".into(),
                    transferred_sketch_entity_count,
                ),
                (
                    "sketch_constraints_transferred".into(),
                    transferred_sketch_constraint_count,
                ),
                ("external_references".into(), external_references.len()),
                ("embedded_references".into(), embedded_references.len()),
                ("ufrx_model_states".into(), ufrx_model_states.len()),
                ("ufrx_occurrences".into(), ufrx_occurrences.len()),
                ("assembly_occurrences".into(), assembly_occurrences.len()),
                ("assembly_placements".into(), assembly_placements.len()),
                (
                    "assembly_occurrences_transferred".into(),
                    transferred_occurrence_count,
                ),
                (
                    "assembly_record_issues".into(),
                    assembly_record_issues.len(),
                ),
                (
                    "active_kernel_carriers".into(),
                    usize::from(matches!(
                        &container.rse.active_carrier,
                        ActiveCarrierState::Selected(_)
                    )),
                ),
                ("kernel_unknown_records".into(), kernel_unknown_record_count),
                (
                    "kernel_unknown_surface_faces".into(),
                    kernel_stats.unknown_surface_faces,
                ),
            ]),
            losses,
            notes: Vec::new(),
            transfer_ledger: TransferLedger::default(),
        },
        source_fidelity,
    ))
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
) {
    let source = ir
        .source
        .as_mut()
        .expect("Inventor source metadata is established before ASM transfer");
    if let Some(version) = header.save_format_version {
        source
            .attributes
            .insert("kernel_save_format_version".into(), version.to_string());
    }
    if let Some(count) = header.entity_count {
        source
            .attributes
            .insert("kernel_entity_count".into(), count.to_string());
    }
    if let Some(flags) = header.flags {
        source
            .attributes
            .insert("kernel_flags".into(), flags.to_string());
    }
    if let Some(family) = &header.product_family {
        source
            .attributes
            .insert("kernel_product_family".into(), family.clone());
    }
    if let Some(version) = &header.product_version {
        source
            .attributes
            .insert("kernel_product_version".into(), version.clone());
    }
    if let (Some(linear), Some(angular)) = (header.linear, header.angular) {
        ir.tolerances = Tolerances { linear, angular };
    }
    source
        .attributes
        .insert("kernel_family".into(), family.label().into());
}

fn structural_issue(scope: &str, detail: &str) -> StructuralIssueRecord {
    StructuralIssueRecord {
        id: format!("inventor:rse:structural-issue#{scope}"),
        scope: scope.into(),
        detail: detail.into(),
    }
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

fn property_value_kind(value: &PropertyValue<'_>) -> String {
    match value {
        PropertyValue::Empty => "empty".into(),
        PropertyValue::Signed(_) => "signed".into(),
        PropertyValue::Unsigned(_) => "unsigned".into(),
        PropertyValue::Float(_) => "float".into(),
        PropertyValue::Bool(_) => "bool".into(),
        PropertyValue::Filetime(_) => "filetime".into(),
        PropertyValue::String(_) => "string".into(),
        PropertyValue::Guid(_) => "guid".into(),
        PropertyValue::Binary(data) => format!("binary:{}", data.window().len()),
        PropertyValue::Clipboard { format, data } => {
            format!("clipboard:{format}:{}", data.window().len())
        }
        PropertyValue::Vector(values) => format!("vector:{}", values.len()),
        PropertyValue::Dictionary => "dictionary".into(),
        PropertyValue::Unknown => "unknown".into(),
    }
}

fn is_preview(fmtid: &[u8; 16], property_id: u32, name: Option<&str>) -> bool {
    fmtid == &FMTID_SUMMARY_INFORMATION && property_id == 17
        || name.is_some_and(|name| {
            matches!(
                normalize_property_name(name).as_str(),
                "thumbnail" | "preview" | "previewimage"
            )
        })
}

fn preview_bytes<'a>(value: &'a PropertyValue<'a>) -> Option<(&'a [u8], &'static str)> {
    let bytes = match value {
        PropertyValue::Binary(view) => view.window(),
        PropertyValue::Clipboard { format, data } if *format == u32::MAX => {
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

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests;
