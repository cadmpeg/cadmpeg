use super::test_consolidated::{
    validate_consolidated_circles, validate_consolidated_class5b5c_records,
    validate_consolidated_class61_records, validate_consolidated_cone_faces,
    validate_consolidated_cones, validate_consolidated_cylinders,
    validate_consolidated_embedded_cylinders, validate_consolidated_groups,
    validate_consolidated_line_profiles, validate_consolidated_parameter_points,
    validate_consolidated_pcurves, validate_consolidated_plane_carriers,
    validate_consolidated_reference_lists, validate_consolidated_revolutions,
    validate_consolidated_spheres, validate_consolidated_tori,
};
use super::test_legacy::{valid_entity_record_shape, validate_legacy_entity_runs};
use super::test_links::{
    validate_consolidated_edge_runs, validate_consolidated_owner_packets, validate_native_links,
    ConsolidatedSupportArenas,
};
use super::test_zero_entity::{
    validate_zero_entity_endpoint_locus_candidates, validate_zero_entity_endpoint_pair_candidates,
    validate_zero_entity_ownership_roots, validate_zero_entity_records,
    validate_zero_entity_support_runs, validate_zero_entity_topology_records,
};
use super::*;
use crate::native::class5b5c::CatiaConsolidatedClass5b5cRecord;
use crate::native::edge_node::{load_edge_nodes, CatiaConsolidatedEdgeNodeWire};

impl CatiaNative {
    /// Decode CATIA-native records directly from a synthesized record source.
    #[must_use]
    pub(crate) fn decode(bytes: &[u8]) -> Self {
        let consolidated_records = crate::wire::records::consolidated_records(bytes);
        Self::decode_with_records(bytes, &consolidated_records)
    }

    /// Load the typed CATIA namespace from generic native arenas.
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let mut catalog_headers: Vec<CatiaCatalogWire> = namespace.arena_as("catalogs")?;
        let entries: Vec<CatiaCatalogEntry> = namespace.arena_as("catalog_entries")?;
        let catalog_ids = catalog_headers
            .iter()
            .map(|catalog| catalog.id.as_str())
            .collect::<HashSet<_>>();
        if catalog_ids.len() != catalog_headers.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA catalog identity".to_string(),
            ));
        }
        if let Some(entry) = entries
            .iter()
            .find(|entry| !catalog_ids.contains(entry.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog entry `{}` references missing catalog `{}`",
                entry.id, entry.parent
            )));
        }
        for catalog in &mut catalog_headers {
            catalog.entries = entries
                .iter()
                .filter(|entry| entry.parent == catalog.id)
                .cloned()
                .collect();
            catalog.entries.sort_by_key(|entry| entry.ordinal);
            if catalog
                .entries
                .iter()
                .enumerate()
                .any(|(ordinal, entry)| usize::try_from(entry.ordinal).ok() != Some(ordinal))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog `{}` has an invalid entry sequence",
                    catalog.id
                )));
            }
        }
        let catalogs = catalog_headers
            .into_iter()
            .map(|header| {
                CatiaCatalog::try_from(header).map_err(|message| {
                    cadmpeg_ir::NativeConvertError::InvalidOwner(message.to_owned())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut graphs: Vec<CatiaObjectGraph> = namespace.arena_as("object_graphs")?;
        let records: Vec<CatiaObjectRecord> = namespace.arena_as("object_graph_records")?;
        let entity_wires: Vec<crate::native::entity_record::CatiaEntityRecordWire> =
            namespace.arena_as("entity_records")?;
        let entity_records = entity_wires
            .into_iter()
            .map(|wire| {
                CatiaEntityRecord::try_from(wire)
                    .map_err(cadmpeg_ir::NativeConvertError::InvalidOwner)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let schema_configuration_row_chains: Vec<CatiaSchemaConfigurationRowChain> =
            namespace.arena_as("schema_configuration_row_chains")?;
        let reference_signature_cohorts: Vec<CatiaReferenceSignatureCohort> =
            namespace.arena_as("reference_signature_cohorts")?;
        let graph_ids = graphs
            .iter()
            .map(|graph| graph.id.as_str())
            .collect::<HashSet<_>>();
        if graph_ids.len() != graphs.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA object-graph identity".to_string(),
            ));
        }
        if let Some(record) = records
            .iter()
            .find(|record| !graph_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object record `{}` references missing graph `{}`",
                record.id, record.parent
            )));
        }
        let record_ids = records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<HashSet<_>>();
        let entity_record_ids = entity_records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<HashSet<_>>();
        if record_ids.len() != records.len() || entity_record_ids.len() != entity_records.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA object or entity-record identity".to_string(),
            ));
        }
        if let Some(entity) = entity_records.iter().find(|entity| {
            !graph_ids.contains(entity.object_graph.as_str())
                || !record_ids.contains(entity.object_record.as_str())
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "entity record `{}` has a missing graph or object-record link",
                entity.id
            )));
        }
        let entity_classes_by_graph_identity = entity_class_index(&records);
        let (
            relation_expressions,
            relation_expression_entities,
            entities_by_graph_identity,
            terminal_nulls_by_graph,
            parameter_bindings,
        ) = semantic_entity_indices(&entity_records, &entity_classes_by_graph_identity);
        let expected_reference_signature_cohorts =
            derive_reference_signature_cohorts(&entity_records);
        if reference_signature_cohorts != expected_reference_signature_cohorts {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "CATIA reference-signature cohorts are not canonical".to_string(),
            ));
        }
        let expected_schema_configuration_row_chains = derive_schema_configuration_row_chains(
            &entity_records,
            &entities_by_graph_identity,
            &entity_classes_by_graph_identity,
            &terminal_nulls_by_graph,
        );
        if schema_configuration_row_chains != expected_schema_configuration_row_chains {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "schema-configuration-row chains do not match their successor links".to_string(),
            ));
        }
        for graph in &mut graphs {
            graph.records = records
                .iter()
                .filter(|record| record.parent == graph.id)
                .cloned()
                .collect();
            graph.records.sort_by_key(|record| record.ordinal);
            let mut graph_entities = entity_records
                .iter()
                .filter(|entity| entity.object_graph == graph.id)
                .collect::<Vec<_>>();
            graph_entities.sort_by_key(|entity| entity.ordinal);
            let catalog = graph
                .catalog
                .as_ref()
                .and_then(|catalog_id| catalogs.iter().find(|catalog| catalog.id == *catalog_id));
            if !graph_entities.is_empty()
                && (graph_entities.len() != graph.records.len()
                    || graph_entities
                        .iter()
                        .enumerate()
                        .any(|(ordinal, entity)| entity.ordinal != ordinal as u64)
                    || graph_entities
                        .windows(2)
                        .any(|pair| pair[0].entity_id >= pair[1].entity_id)
                    || graph_entities
                        .iter()
                        .any(|entity| !valid_entity_record_shape(entity))
                    || graph_entities.iter().any(|entity| {
                        entity.reference_signature
                            != entity_table::parse_reference_signature(entity.value_payload()).map(
                                |production| {
                                    reference_signature(
                                        production,
                                        &graph.id,
                                        &CatiaEntityReferenceIndex {
                                            entities: &entities_by_graph_identity,
                                            classes: &entity_classes_by_graph_identity,
                                            terminal_nulls: &terminal_nulls_by_graph,
                                        },
                                    )
                                },
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_schema_selections
                            != definition_schema_selections(
                                &entity_table::parse_definition_schema_selectors(
                                    entity.definition_prefix(),
                                ),
                                catalog,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.value_schema_selections
                            != entity_value_schema_selections(
                                &entity.value_fields(),
                                catalog,
                                &entity.value_packets(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.value_production
                            != value_production(entity, &graph.records, &entity.value_fields())
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_value()
                            != entity_suffix_value(entity.record_suffix()).as_ref()
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_framing()
                            != entity_suffix_framing(entity.record_suffix()).as_ref()
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_schema_selection
                            != entity_suffix_schema_selection(entity.suffix_value(), catalog)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.range_interval
                            != range_interval(
                                entity.value_payload(),
                                &entity.value_schema_selections,
                                entity.suffix_value(),
                                &graph.records,
                                &graph.id,
                                entity.entity_id,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.object_production
                            != object.and_then(|object| {
                                object_production(
                                    entity,
                                    object,
                                    &CatiaEntityReferenceIndex {
                                        entities: &entities_by_graph_identity,
                                        classes: &entity_classes_by_graph_identity,
                                        terminal_nulls: &terminal_nulls_by_graph,
                                    },
                                    &relation_expressions,
                                    &relation_expression_entities,
                                    &parameter_bindings,
                                )
                            })
                    })
                    || graph_entities.windows(2).any(|pair| {
                        pair[0].byte_offset.checked_add(pair[0].byte_len())
                            != Some(pair[1].byte_offset)
                    })
                    || graph_entities.last().and_then(|entity| {
                        entity
                            .byte_offset
                            .checked_add(entity.byte_len())?
                            .checked_add(1)
                    }) != Some(graph.byte_offset))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object graph `{}` has an invalid entity-table sequence",
                    graph.id
                )));
            }
            let record_ids = graph
                .records
                .iter()
                .map(|record| record.id.clone())
                .collect::<Vec<_>>();
            let record_design_objects = graph
                .records
                .iter()
                .map(|record| record.design_object.clone())
                .collect::<Vec<_>>();
            let record_indices = graph
                .records
                .iter()
                .enumerate()
                .filter_map(|(index, record)| Some((record.entity_id()?, index)))
                .collect::<HashMap<_, _>>();
            let terminal_null_entity_id = terminal_null_entity_id(&record_indices);
            if record_indices.len()
                != graph
                    .records
                    .iter()
                    .filter(|record| record.entity_id().is_some())
                    .count()
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object graph `{}` has duplicate entity identities",
                    graph.id
                )));
            }
            for (ordinal, record) in graph.records.iter().enumerate() {
                let expected_head_roles = object_graph::head_roles(record.lead, &record.head);
                let expected_owner = expected_head_roles.owner.map(CatiaObjectOwner::from);
                let expected_design_object = record
                    .owner_entity_id()
                    .map(|owner| design_object_id(graph.byte_offset, owner));
                let paired_entity = graph_entities.get(ordinal).copied();
                let expected_storage = resolved_storage_link(
                    record.storage_ref(),
                    &record_ids,
                    &record_design_objects,
                    &record_indices,
                );
                if usize::try_from(record.ordinal).ok() != Some(ordinal)
                    || record.owner != expected_owner
                    || (record.class_ref(), record.storage_ref())
                        != (
                            expected_head_roles.class_ref,
                            expected_head_roles.storage_ref,
                        )
                    || record.design_object != expected_design_object
                    || record.entity_record() != paired_entity.map(|entity| entity.id.as_str())
                    || record.entity_id() != paired_entity.map(|entity| entity.entity_id)
                    || paired_entity.is_some_and(|entity| entity.object_record != record.id)
                    || (record.storage_record(), record.storage_design_object())
                        != (expected_storage.0.as_deref(), expected_storage.1.as_deref())
                    || record.inline_body.as_ref().is_some_and(|body| {
                        (graph_entities.is_empty() && !object_graph::is_inline_body(body))
                            || body.first() != Some(&record.lead)
                            || !record.head.is_empty()
                            || record.owner.is_some()
                            || record.class_ref().is_some()
                            || record.storage_ref().is_some()
                            || record.payload.size != 0
                            || !record.payload.fields.is_empty()
                    })
                    || record.inline_body.is_none() && record.head.is_empty()
                    || record.references
                        != resolved_payload_references(
                            &record.payload,
                            &record_ids,
                            &record_design_objects,
                            &record_indices,
                            terminal_null_entity_id,
                        )
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "object graph `{}` has an invalid record sequence",
                        graph.id
                    )));
                }
            }
        }
        let mut value_blocks: Vec<CatiaValueBlock> = namespace.arena_as("value_blocks")?;
        let value_schema_selections: Vec<CatiaValueSchemaSelection> =
            namespace.arena_as("value_schema_selections")?;
        let value_block_ids = value_blocks
            .iter()
            .map(|block| block.id.clone())
            .collect::<HashSet<_>>();
        if value_block_ids.len() != value_blocks.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA value-block identity".to_string(),
            ));
        }
        let mut selections_by_block = HashMap::<String, Vec<CatiaValueSchemaSelection>>::new();
        for selection in value_schema_selections {
            if !value_block_ids.contains(&selection.parent) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "value selection `{}` references missing block `{}`",
                    selection.id, selection.parent
                )));
            }
            selections_by_block
                .entry(selection.parent.clone())
                .or_default()
                .push(selection);
        }
        for block in &mut value_blocks {
            block.schema_selections = selections_by_block.remove(&block.id).unwrap_or_default();
            block
                .schema_selections
                .sort_by_key(|selection| selection.offset);
        }
        let design_objects = design_objects(&graphs, &entity_records);
        let stored: Vec<CatiaDesignObject> = namespace.arena_as("design_objects")?;
        let stored_by_id = stored
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<HashMap<_, _>>();
        if stored_by_id.len() != stored.len()
            || stored.len() != design_objects.len()
            || design_objects
                .iter()
                .any(|object| stored_by_id.get(object.id.as_str()).copied() != Some(object))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA design objects disagree with their object graph".to_string(),
            ));
        }
        let mut finjpl_segments: Vec<CatiaFinjplSegment> = namespace.arena_as("finjpl_segments")?;
        finjpl_segments.sort_by_key(|segment| segment.byte_offset);
        let mut external_references: Vec<CatiaExternalReference> =
            namespace.arena_as("external_references")?;
        external_references.sort_by_key(|reference| reference.byte_offset);
        let expected_external_references = external_reference_views(&finjpl_segments);
        if external_references != expected_external_references {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA external references disagree with their project-flags segments"
                    .to_string(),
            ));
        }
        let external_references = expected_external_references;
        let mut legacy_entity_runs: Vec<CatiaLegacyEntityRun> =
            namespace.arena_as("legacy_entity_runs")?;
        legacy_entity_runs.sort_by_key(|run| run.byte_offset);
        validate_legacy_entity_runs(&legacy_entity_runs)?;
        let mut preview_images: Vec<CatiaPreviewImage> = namespace.arena_as("preview_images")?;
        preview_images.sort_by_key(|preview| preview.byte_offset);
        let expected_preview_images = preview_views(&finjpl_segments);
        if preview_images != expected_preview_images {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA previews disagree with their summary segments".to_string(),
            ));
        }
        let preview_images = expected_preview_images;
        let alias_rows: Vec<CatiaAliasRow> = namespace.arena_as("alias_rows")?;
        let mut consolidated_circles: Vec<CatiaConsolidatedCircle> =
            namespace.arena_as("consolidated_circles")?;
        consolidated_circles.sort_by_key(|circle| circle.byte_offset);
        validate_consolidated_circles(&consolidated_circles)?;
        let mut consolidated_class61_records: Vec<CatiaConsolidatedClass61Record> =
            namespace.arena_as("consolidated_class61_records")?;
        consolidated_class61_records.sort_by_key(|record| record.byte_offset);
        validate_consolidated_class61_records(&consolidated_class61_records)?;
        let mut consolidated_class5b5c_records: Vec<CatiaConsolidatedClass5b5cRecord> =
            namespace.arena_as("consolidated_class5b5c_records")?;
        consolidated_class5b5c_records
            .sort_by_key(|record| (record.source_index, record.source_offset));
        validate_consolidated_class5b5c_records(&consolidated_class5b5c_records)?;
        let mut consolidated_cone_faces: Vec<CatiaConsolidatedConeFace> =
            namespace.arena_as("consolidated_cone_faces")?;
        consolidated_cone_faces.sort_by_key(|face| face.byte_offset);
        let mut consolidated_cones: Vec<CatiaConsolidatedCone> =
            namespace.arena_as("consolidated_cones")?;
        consolidated_cones.sort_by_key(|cone| cone.byte_offset);
        validate_consolidated_cones(&consolidated_cones)?;
        let mut consolidated_cylinders: Vec<CatiaConsolidatedCylinder> =
            namespace.arena_as("consolidated_cylinders")?;
        consolidated_cylinders.sort_by_key(|cylinder| cylinder.byte_offset);
        validate_consolidated_cylinders(&consolidated_cylinders)?;
        let mut consolidated_groups: Vec<CatiaConsolidatedGroup> =
            namespace.arena_as("consolidated_groups")?;
        consolidated_groups.sort_by_key(|group| group.byte_offset);
        validate_consolidated_groups(&consolidated_groups)?;
        let mut consolidated_embedded_cylinders: Vec<CatiaConsolidatedEmbeddedCylinder> =
            namespace.arena_as("consolidated_embedded_cylinders")?;
        consolidated_embedded_cylinders.sort_by_key(|cylinder| cylinder.byte_offset);
        validate_consolidated_embedded_cylinders(
            &consolidated_embedded_cylinders,
            &consolidated_groups,
        )?;
        let mut consolidated_line_profiles: Vec<CatiaConsolidatedLineProfile> =
            namespace.arena_as("consolidated_line_profiles")?;
        consolidated_line_profiles.sort_by_key(|line| line.byte_offset);
        validate_consolidated_line_profiles(&consolidated_line_profiles)?;
        let mut consolidated_owner_packets: Vec<CatiaConsolidatedOwnerPacket> =
            namespace.arena_as("consolidated_owner_packets")?;
        consolidated_owner_packets.sort_by_key(|packet| packet.byte_offset);
        validate_consolidated_owner_packets(&consolidated_owner_packets)?;
        let mut consolidated_parameter_points: Vec<CatiaConsolidatedParameterPoint> =
            namespace.arena_as("consolidated_parameter_points")?;
        consolidated_parameter_points.sort_by_key(|point| point.byte_offset);
        validate_consolidated_parameter_points(&consolidated_parameter_points)?;
        validate_consolidated_cone_faces(&consolidated_cone_faces, &consolidated_parameter_points)?;
        let mut consolidated_plane_carriers: Vec<CatiaConsolidatedPlaneCarrier> =
            namespace.arena_as("consolidated_plane_carriers")?;
        consolidated_plane_carriers.sort_by_key(|carrier| carrier.byte_offset);
        validate_consolidated_plane_carriers(&consolidated_plane_carriers)?;
        let mut consolidated_pcurves: Vec<CatiaConsolidatedPcurve> =
            namespace.arena_as("consolidated_pcurves")?;
        consolidated_pcurves.sort_by_key(|pcurve| pcurve.byte_offset);
        validate_consolidated_pcurves(&consolidated_pcurves)?;
        let mut consolidated_reference_lists: Vec<CatiaConsolidatedReferenceList> =
            namespace.arena_as("consolidated_reference_lists")?;
        consolidated_reference_lists.sort_by_key(|list| list.byte_offset);
        validate_consolidated_reference_lists(&consolidated_reference_lists)?;
        let mut consolidated_revolutions: Vec<CatiaConsolidatedRevolution> =
            namespace.arena_as("consolidated_revolutions")?;
        consolidated_revolutions.sort_by_key(|revolution| revolution.byte_offset);
        validate_consolidated_revolutions(&consolidated_revolutions, &consolidated_circles)?;
        let mut consolidated_spheres: Vec<CatiaConsolidatedSphere> =
            namespace.arena_as("consolidated_spheres")?;
        consolidated_spheres.sort_by_key(|sphere| sphere.byte_offset);
        validate_consolidated_spheres(&consolidated_spheres)?;
        let mut consolidated_tori: Vec<CatiaConsolidatedTorus> =
            namespace.arena_as("consolidated_tori")?;
        consolidated_tori.sort_by_key(|torus| torus.byte_offset);
        validate_consolidated_tori(&consolidated_tori)?;
        let mut consolidated_edge_runs: Vec<CatiaConsolidatedEdgeRun> =
            namespace.arena_as("consolidated_edge_runs")?;
        consolidated_edge_runs.sort_by_key(|run| run.byte_offset);
        let consolidated_edge_node_wires: Vec<CatiaConsolidatedEdgeNodeWire> =
            namespace.arena_as("consolidated_edge_nodes")?;
        let consolidated_vertex_identities: Vec<CatiaConsolidatedVertexIdentity> =
            namespace.arena_as("consolidated_vertex_identities")?;
        let mut consolidated_edge_nodes = load_edge_nodes(
            consolidated_edge_node_wires,
            &consolidated_vertex_identities,
        )
        .map_err(cadmpeg_ir::NativeConvertError::InvalidOwner)?;
        consolidated_edge_nodes.sort_by_key(|node| node.byte_offset);
        let mut zero_entity_edge_strides: Vec<CatiaZeroEntityEdgeStride> =
            namespace.arena_as("zero_entity_edge_strides")?;
        zero_entity_edge_strides.sort_by_key(|record| record.byte_offset);
        let mut zero_entity_oriented_use_pairs: Vec<CatiaZeroEntityOrientedUsePair> =
            namespace.arena_as("zero_entity_oriented_use_pairs")?;
        zero_entity_oriented_use_pairs.sort_by_key(|pair| pair.header_byte_offset);
        let zero_entity_ownership_roots: Vec<CatiaZeroEntityOwnershipRoot> =
            namespace.arena_as("zero_entity_ownership_roots")?;
        let zero_entity_endpoint_pair_candidates: Vec<CatiaZeroEntityEndpointPairCandidate> =
            namespace.arena_as("zero_entity_endpoint_pair_candidates")?;
        let mut zero_entity_records: Vec<CatiaZeroEntityRecord> =
            namespace.arena_as("zero_entity_records")?;
        zero_entity_records.sort_by_key(|record| record.record_ordinal);
        validate_zero_entity_records(&zero_entity_records)?;
        let mut zero_entity_support_runs: Vec<CatiaZeroEntitySupportRun> =
            namespace.arena_as("zero_entity_support_runs")?;
        zero_entity_support_runs.sort_by_key(|run| run.carrier_byte_offset);
        validate_zero_entity_support_runs(&zero_entity_support_runs, &zero_entity_records)?;
        validate_zero_entity_ownership_roots(
            &zero_entity_ownership_roots,
            &zero_entity_support_runs,
            &zero_entity_records,
        )?;
        let zero_entity_endpoint_locus_candidates: Vec<CatiaZeroEntityEndpointLocusCandidate> =
            namespace.arena_as("zero_entity_endpoint_locus_candidates")?;
        validate_zero_entity_endpoint_pair_candidates(
            &zero_entity_endpoint_pair_candidates,
            &zero_entity_support_runs,
        )?;
        validate_zero_entity_endpoint_locus_candidates(
            &zero_entity_endpoint_locus_candidates,
            &zero_entity_support_runs,
        )?;
        let mut zero_entity_vertex_incidences: Vec<CatiaZeroEntityVertexIncidence> =
            namespace.arena_as("zero_entity_vertex_incidences")?;
        zero_entity_vertex_incidences.sort_by_key(|record| record.byte_offset);
        validate_zero_entity_topology_records(
            &zero_entity_edge_strides,
            &zero_entity_oriented_use_pairs,
            &zero_entity_vertex_incidences,
            &zero_entity_records,
        )?;
        validate_consolidated_edge_runs(
            &consolidated_edge_runs,
            &consolidated_pcurves,
            &ConsolidatedSupportArenas {
                circles: &consolidated_circles,
                cones: &consolidated_cones,
                cylinders: &consolidated_cylinders,
                embedded_cylinders: &consolidated_embedded_cylinders,
                groups: &consolidated_groups,
                planes: &consolidated_plane_carriers,
                spheres: &consolidated_spheres,
                tori: &consolidated_tori,
            },
            &consolidated_edge_nodes,
            &consolidated_vertex_identities,
        )?;
        validate_native_links(
            &alias_rows,
            &catalogs,
            &graphs,
            &finjpl_segments,
            &value_blocks,
        )?;
        validate_alias_surface_tags(&alias_rows)?;
        validate_owner_chart_support_aliases(&consolidated_owner_packets, &alias_rows)?;
        Ok(Self {
            alias_rows,
            catalogs,
            consolidated_circles,
            consolidated_class61_records,
            consolidated_class5b5c_records,
            consolidated_cone_faces,
            consolidated_cones,
            consolidated_cylinders,
            consolidated_embedded_cylinders,
            consolidated_edge_nodes,
            consolidated_edge_runs,
            consolidated_groups,
            consolidated_line_profiles,
            consolidated_owner_packets,
            consolidated_parameter_points,
            consolidated_plane_carriers,
            consolidated_pcurves,
            consolidated_reference_lists,
            consolidated_revolutions,
            consolidated_spheres,
            consolidated_tori,
            consolidated_vertex_identities,
            design_objects,
            entity_records,
            external_references,
            finjpl_segments,
            legacy_entity_runs,
            object_graphs: graphs,
            preview_images,
            reference_signature_cohorts,
            schema_configuration_row_chains,
            value_blocks,
            zero_entity_edge_strides,
            zero_entity_oriented_use_pairs,
            zero_entity_ownership_roots,
            zero_entity_endpoint_pair_candidates,
            zero_entity_records,
            zero_entity_support_runs,
            zero_entity_endpoint_locus_candidates,
            zero_entity_vertex_incidences,
        })
    }

    /// Store the typed CATIA namespace into generic native arenas.
    pub fn store(
        &self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        store_projection(&CatiaArenaProjection::from(self), namespace)
    }
}
