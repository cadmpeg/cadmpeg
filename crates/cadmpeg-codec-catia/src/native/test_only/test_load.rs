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
use super::test_legacy::{
    legacy_schema_identifiers, legacy_value_name, valid_entity_record_shape,
    validate_legacy_entity_runs,
};
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
        let mut catalogs: Vec<CatiaCatalog> = namespace.arena_as("catalogs")?;
        let entries: Vec<CatiaCatalogEntry> = namespace.arena_as("catalog_entries")?;
        let catalog_ids = catalogs
            .iter()
            .map(|catalog| catalog.id.as_str())
            .collect::<HashSet<_>>();
        if catalog_ids.len() != catalogs.len() {
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
        for catalog in &mut catalogs {
            catalog.entries = entries
                .iter()
                .filter(|entry| entry.parent == catalog.id)
                .cloned()
                .collect();
            catalog.entries.sort_by_key(|entry| entry.ordinal);
            if u32::try_from(catalog.entries.len())
                .ok()
                .and_then(|count| count.checked_add(1))
                != Some(catalog.declared_count)
                || catalog
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
        let mut graphs: Vec<CatiaObjectGraph> = namespace.arena_as("object_graphs")?;
        let mut records: Vec<CatiaObjectRecord> = namespace.arena_as("object_graph_records")?;
        if namespace.version() < CATIA_TYPED_OWNER_SLOT_VERSION {
            for record in &mut records {
                let roles = object_graph::head_roles(record.lead, &record.head);
                record.owner = roles
                    .owner_ref
                    .map(CatiaObjectOwner::Entity)
                    .or_else(|| roles.owner_literal.map(CatiaObjectOwner::UnassignedLiteral));
            }
        }
        let mut entity_records: Vec<CatiaEntityRecord> = namespace.arena_as("entity_records")?;
        if namespace.version() < CATIA_NUMERIC_PAIR_VERSION {
            for entity in &mut entity_records {
                entity.numeric_pair = entity_table::parse_numeric_pair(&entity.value_payload);
            }
        }
        let row_chain_arena = if namespace
            .arenas
            .contains_key("schema_configuration_row_chains")
        {
            "schema_configuration_row_chains"
        } else {
            "configuration_row_chains"
        };
        let mut schema_configuration_row_chains: Vec<CatiaSchemaConfigurationRowChain> =
            namespace.arena_as(row_chain_arena)?;
        let mut reference_signature_cohorts: Vec<CatiaReferenceSignatureCohort> =
            namespace.arena_as("reference_signature_cohorts")?;
        if namespace.version() < CATIA_REFERENCE_SIGNATURE_INCIDENCE_VERSION {
            for entity in &mut entity_records {
                entity.reference_signature = entity_table::parse_reference_signature(
                    &entity.value_payload,
                )
                .map(|production| CatiaReferenceSignature {
                    production,
                    first_entity: CatiaEntityReference::Unresolved { entity_id: 0 },
                    second_entity: CatiaEntityReference::Unresolved { entity_id: 0 },
                });
            }
        }
        if namespace.version() < CATIA_SUFFIX_FRAMING_VERSION {
            for entity in &mut entity_records {
                entity.suffix_framing = entity_suffix_framing(&entity.record_suffix);
            }
        }
        if namespace.version() < CATIA_ENTITY_SCHEMA_VALUE_INCIDENCE_VERSION
            || namespace.version() < CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION
        {
            for entity in &mut entity_records {
                entity.relation_expression = relation_expression(
                    &entity.definition_schema_selections,
                    &entity.value_schema_selections,
                );
                entity.parameter_value = parameter_value(
                    entity.lead,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                );
                entity.constraint_range = resolved_constraint_range(
                    entity.lead,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                    &records,
                    &entity.object_graph,
                    entity.entity_id,
                );
                entity.definition_value = definition_value(
                    entity.lead,
                    &entity.definition_schema_selections,
                    &entity.value_fields,
                    entity.suffix_value.as_ref(),
                    entity.suffix_schema_selection.as_ref(),
                );
                entity.definition_chain_value = definition_chain_value(
                    entity.lead,
                    &entity.definition_schema_selections,
                    &entity.value_fields,
                    entity.suffix_value.as_ref(),
                    entity.suffix_schema_selection.as_ref(),
                );
            }
        }
        if namespace.version() < CATIA_SUFFIX_EVALUATION_OFFSET_VERSION
            || namespace.version() < CATIA_SUFFIX_TRAILER_8193_VERSION
        {
            for graph in &graphs {
                let catalog = graph.catalog.as_deref().and_then(|catalog_id| {
                    catalogs.iter().find(|catalog| catalog.id == catalog_id)
                });
                for entity in entity_records
                    .iter_mut()
                    .filter(|entity| entity.object_graph == graph.id)
                {
                    entity.suffix_value = entity_suffix_value(&entity.record_suffix);
                    entity.suffix_schema_selection =
                        entity_suffix_schema_selection(entity.suffix_value.as_ref(), catalog);
                    entity.parameter_value = parameter_value(
                        entity.lead,
                        &entity.value_schema_selections,
                        entity.suffix_value.as_ref(),
                    );
                    entity.constraint_range = resolved_constraint_range(
                        entity.lead,
                        &entity.value_schema_selections,
                        entity.suffix_value.as_ref(),
                        &records,
                        &entity.object_graph,
                        entity.entity_id,
                    );
                    entity.definition_value = definition_value(
                        entity.lead,
                        &entity.definition_schema_selections,
                        &entity.value_fields,
                        entity.suffix_value.as_ref(),
                        entity.suffix_schema_selection.as_ref(),
                    );
                    entity.definition_chain_value = definition_chain_value(
                        entity.lead,
                        &entity.definition_schema_selections,
                        &entity.value_fields,
                        entity.suffix_value.as_ref(),
                        entity.suffix_schema_selection.as_ref(),
                    );
                }
            }
        }
        if namespace.version() < CATIA_RANGE_NOMINAL_VERSION {
            for entity in &mut entity_records {
                entity.range_interval = range_interval(
                    &entity.value_payload,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                    &records,
                    &entity.object_graph,
                    entity.entity_id,
                );
            }
        }
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
        if namespace.version() < CATIA_REFERENCE_SIGNATURE_ENTITY_VERSION {
            let entity_references = CatiaEntityReferenceIndex {
                entities: &entities_by_graph_identity,
                classes: &entity_classes_by_graph_identity,
                terminal_nulls: &terminal_nulls_by_graph,
            };
            for entity in &mut entity_records {
                if let Some(signature) = entity.reference_signature.take() {
                    entity.reference_signature = Some(reference_signature(
                        signature.production,
                        &entity.object_graph,
                        &entity_references,
                    ));
                }
            }
        }
        if namespace.version() < CATIA_REFERENCE_SIGNATURE_FRAME_VERSION {
            for entity in &mut entity_records {
                let Some(signature) = &mut entity.reference_signature else {
                    continue;
                };
                let Some(production) =
                    entity_table::parse_reference_signature(&entity.value_payload)
                else {
                    continue;
                };
                signature.production = production;
            }
        }
        if namespace.version() < CATIA_REFERENCE_SIGNATURE_PAIR_VERSION {
            let entity_references = CatiaEntityReferenceIndex {
                entities: &entities_by_graph_identity,
                classes: &entity_classes_by_graph_identity,
                terminal_nulls: &terminal_nulls_by_graph,
            };
            for entity in &mut entity_records {
                entity.reference_signature = entity_table::parse_reference_signature(
                    &entity.value_payload,
                )
                .map(|production| {
                    reference_signature(production, &entity.object_graph, &entity_references)
                });
            }
        }
        let expected_reference_signature_cohorts =
            derive_reference_signature_cohorts(&entity_records);
        if namespace.version() < CATIA_DERIVED_NATIVE_ID_VERSION {
            reference_signature_cohorts = expected_reference_signature_cohorts;
        } else if reference_signature_cohorts != expected_reference_signature_cohorts {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "CATIA reference-signature cohorts are not canonical".to_string(),
            ));
        }
        if namespace.version() < CATIA_TERMINAL_NULL_REFERENCE_VERSION {
            for graph in &graphs {
                let terminal_null = entity_records
                    .iter()
                    .filter(|entity| entity.object_graph == graph.id)
                    .map(|entity| entity.entity_id)
                    .max()
                    .and_then(|entity_id| entity_id.checked_add(1));
                for record in records
                    .iter_mut()
                    .filter(|record| record.parent == graph.id)
                {
                    for reference in &mut record.references {
                        *reference = reference
                            .clone()
                            .with_null_from_terminal(Some(reference.entity_id()) == terminal_null);
                    }
                }
            }
        }
        if namespace.version() < CATIA_FORMULA_DEPENDENCY_CANDIDATE_VERSION
            || namespace.version() < CATIA_TERMINAL_NULL_REFERENCE_VERSION
            || namespace.version() < CATIA_FORMULA_OUTPUT_REFERENCE_VERSION
            || namespace.version() < CATIA_FORMULA_EXPRESSION_REFERENCE_VERSION
            || namespace.version() < CATIA_FORMULA_DEPENDENCY_REFERENCE_VERSION
            || namespace.version() < CATIA_TYPED_INCIDENCE_NULL_VERSION
            || namespace.version() < CATIA_RELATION_DEPENDENCY_OFFSET_VERSION
            || namespace.version() < CATIA_RELATION_STRING_LITERAL_DEPENDENCY_VERSION
            || namespace.version() < CATIA_FORMULA_REFERENCE_OFFSET_VERSION
            || namespace.version() < CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.formula_relation = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        formula_relation(
                            &entity.definition_schema_selections,
                            entity.entity_id,
                            object,
                            &relation_expressions,
                            &CatiaEntityReferenceIndex {
                                entities: &entities_by_graph_identity,
                                classes: &entity_classes_by_graph_identity,
                                terminal_nulls: &terminal_nulls_by_graph,
                            },
                            &parameter_bindings,
                        )
                    });
            }
        }
        if namespace.version() < CATIA_RELATION_PROGRAM_INSTANCE_VERSION
            || namespace.version() < CATIA_RELATION_PROGRAM_CONTEXT_VERSION
            || namespace.version() < CATIA_TYPED_INCIDENCE_CLASS_VERSION
            || namespace.version() < CATIA_RELATION_TYPED_REFERENCE_VERSION
            || namespace.version() < CATIA_TYPED_INCIDENCE_NULL_VERSION
            || namespace.version() < CATIA_RELATION_PROGRAM_REFERENCE_INCIDENCE_VERSION
            || namespace.version() < CATIA_RELATION_PROGRAM_DEPENDENCY_VERSION
            || namespace.version() < CATIA_RELATION_PROGRAM_INPUT_VERSION
            || namespace.version() < CATIA_RELATION_PROGRAM_OUTPUT_VERSION
            || namespace.version() < CATIA_RELATION_DEPENDENCY_OFFSET_VERSION
            || namespace.version() < CATIA_RELATION_REFERENCE_OFFSET_VERSION
            || namespace.version() < CATIA_RELATION_STRING_LITERAL_DEPENDENCY_VERSION
            || namespace.version() < CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.relation_program_instance = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        relation_program_instance(
                            entity.entity_id,
                            object,
                            &CatiaEntityReferenceIndex {
                                entities: &entities_by_graph_identity,
                                classes: &entity_classes_by_graph_identity,
                                terminal_nulls: &terminal_nulls_by_graph,
                            },
                            &relation_expression_entities,
                            &parameter_bindings,
                        )
                    });
            }
        }
        if namespace.version() < CATIA_CONSTRAINT_RANGE_INCIDENCE_VERSION
            || namespace.version() < CATIA_CONSTRAINT_RANGE_SOURCE_ENTITY_VERSION
            || namespace.version() < CATIA_CONSTRAINT_RANGE_STORAGE_INCIDENCE_VERSION
        {
            for entity in &mut entity_records {
                if let Some(range) = &mut entity.constraint_range {
                    (range.incoming_references, range.incoming_storage_references) =
                        entity_incidences(&records, &entity.object_graph, entity.entity_id);
                }
            }
        }
        if namespace.version() < CATIA_CONFIGURATION_INCIDENCE_VERSION
            || namespace.version() < CATIA_SCHEMA_CONFIGURATION_REFERENCE_VERSION
            || namespace.version() < CATIA_TYPED_INCIDENCE_CLASS_VERSION
            || namespace.version() < CATIA_TYPED_INCIDENCE_NULL_VERSION
            || namespace.version() < CATIA_CONFIGURATION_PAYLOAD_OFFSET_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.schema_configuration_record = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        schema_configuration_record(
                            entity.entity_id,
                            object,
                            &entity.value_schema_selections,
                            &entities_by_graph_identity,
                            &entity_classes_by_graph_identity,
                            &terminal_nulls_by_graph,
                        )
                    });
                entity.schema_configuration_row_link = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        schema_configuration_row_link(
                            entity.entity_id,
                            object,
                            &entities_by_graph_identity,
                            &entity_classes_by_graph_identity,
                            &terminal_nulls_by_graph,
                        )
                    });
            }
        }
        let expected_schema_configuration_row_chains = derive_schema_configuration_row_chains(
            &entity_records,
            &entities_by_graph_identity,
            &entity_classes_by_graph_identity,
            &terminal_nulls_by_graph,
        );
        if namespace.version() < CATIA_DERIVED_NATIVE_ID_VERSION
            || namespace.version() < CATIA_SCHEMA_CONFIGURATION_NAMING_VERSION
        {
            schema_configuration_row_chains = expected_schema_configuration_row_chains;
        } else if schema_configuration_row_chains != expected_schema_configuration_row_chains {
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
                            != entity_table::parse_reference_signature(&entity.value_payload).map(
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
                                    &entity.definition_prefix,
                                ),
                                catalog,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.value_schema_selections
                            != entity_value_schema_selections(
                                &entity.value_fields,
                                catalog,
                                &entity.value_packets,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.relation_expression
                            != relation_expression(
                                &entity.definition_schema_selections,
                                &entity.value_schema_selections,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_value != entity_suffix_value(&entity.record_suffix)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_framing != entity_suffix_framing(&entity.record_suffix)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_schema_selection
                            != entity_suffix_schema_selection(entity.suffix_value.as_ref(), catalog)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.parameter_value
                            != parameter_value(
                                entity.lead,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.range_interval
                            != range_interval(
                                &entity.value_payload,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                                &graph.records,
                                &graph.id,
                                entity.entity_id,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.constraint_range
                            != resolved_constraint_range(
                                entity.lead,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                                &graph.records,
                                &graph.id,
                                entity.entity_id,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_value
                            != definition_value(
                                entity.lead,
                                &entity.definition_schema_selections,
                                &entity.value_fields,
                                entity.suffix_value.as_ref(),
                                entity.suffix_schema_selection.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_chain_value
                            != definition_chain_value(
                                entity.lead,
                                &entity.definition_schema_selections,
                                &entity.value_fields,
                                entity.suffix_value.as_ref(),
                                entity.suffix_schema_selection.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.relation_program_instance
                            != object.and_then(|object| {
                                relation_program_instance(
                                    entity.entity_id,
                                    object,
                                    &CatiaEntityReferenceIndex {
                                        entities: &entities_by_graph_identity,
                                        classes: &entity_classes_by_graph_identity,
                                        terminal_nulls: &terminal_nulls_by_graph,
                                    },
                                    &relation_expression_entities,
                                    &parameter_bindings,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.schema_configuration_record
                            != object.and_then(|object| {
                                schema_configuration_record(
                                    entity.entity_id,
                                    object,
                                    &entity.value_schema_selections,
                                    &entities_by_graph_identity,
                                    &entity_classes_by_graph_identity,
                                    &terminal_nulls_by_graph,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.schema_configuration_row_link
                            != object.and_then(|object| {
                                schema_configuration_row_link(
                                    entity.entity_id,
                                    object,
                                    &entities_by_graph_identity,
                                    &entity_classes_by_graph_identity,
                                    &terminal_nulls_by_graph,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.formula_relation
                            != object.and_then(|object| {
                                formula_relation(
                                    &entity.definition_schema_selections,
                                    entity.entity_id,
                                    object,
                                    &relation_expressions,
                                    &CatiaEntityReferenceIndex {
                                        entities: &entities_by_graph_identity,
                                        classes: &entity_classes_by_graph_identity,
                                        terminal_nulls: &terminal_nulls_by_graph,
                                    },
                                    &parameter_bindings,
                                )
                            })
                    })
                    || graph_entities.windows(2).any(|pair| {
                        pair[0].byte_offset.checked_add(pair[0].byte_len)
                            != Some(pair[1].byte_offset)
                    })
                    || graph_entities.last().and_then(|entity| {
                        entity
                            .byte_offset
                            .checked_add(entity.byte_len)?
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
                .filter_map(|(index, record)| Some((record.entity_id?, index)))
                .collect::<HashMap<_, _>>();
            let terminal_null_entity_id = terminal_null_entity_id(&record_indices);
            if record_indices.len()
                != graph
                    .records
                    .iter()
                    .filter(|record| record.entity_id.is_some())
                    .count()
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object graph `{}` has duplicate entity identities",
                    graph.id
                )));
            }
            for (ordinal, record) in graph.records.iter().enumerate() {
                let expected_head_roles = object_graph::head_roles(record.lead, &record.head);
                let expected_owner = expected_head_roles
                    .owner_ref
                    .map(CatiaObjectOwner::Entity)
                    .or_else(|| {
                        expected_head_roles
                            .owner_literal
                            .map(CatiaObjectOwner::UnassignedLiteral)
                    });
                let expected_design_object = record
                    .owner_entity_id()
                    .map(|owner| design_object_id(graph.byte_offset, owner));
                let paired_entity = graph_entities.get(ordinal).copied();
                let expected_storage = resolved_storage_link(
                    record.storage_ref,
                    &record_ids,
                    &record_design_objects,
                    &record_indices,
                );
                if usize::try_from(record.ordinal).ok() != Some(ordinal)
                    || record.owner != expected_owner
                    || (record.class_ref, record.storage_ref)
                        != (
                            expected_head_roles.class_ref,
                            expected_head_roles.storage_ref,
                        )
                    || record.design_object != expected_design_object
                    || record.entity_record != paired_entity.map(|entity| entity.id.clone())
                    || record.entity_id != paired_entity.map(|entity| entity.entity_id)
                    || paired_entity.is_some_and(|entity| entity.object_record != record.id)
                    || (
                        record.storage_record.as_ref(),
                        record.storage_design_object.as_ref(),
                    ) != (expected_storage.0.as_ref(), expected_storage.1.as_ref())
                    || record.repeated_reference_suffix
                        != object_graph::repeated_reference_suffix(&record.payload)
                    || record.inline_body.as_ref().is_some_and(|body| {
                        (graph_entities.is_empty() && !object_graph::is_inline_body(body))
                            || body.first() != Some(&record.lead)
                            || !record.head.is_empty()
                            || record.owner.is_some()
                            || record.class_ref.is_some()
                            || record.storage_ref.is_some()
                            || record.payload.size != 0
                            || !record.payload.fields.is_empty()
                            || record.subtype != PayloadSubtype::Empty
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
        if namespace.arenas.contains_key("design_objects") {
            let mut stored: Vec<CatiaDesignObject> = namespace.arena_as("design_objects")?;
            if namespace.version() < CATIA_DEFINITION_CHAIN_OWNERSHIP_VERSION {
                let derived_by_id = design_objects
                    .iter()
                    .map(|object| (object.id.as_str(), object))
                    .collect::<HashMap<_, _>>();
                for object in &mut stored {
                    if let Some(derived) = derived_by_id.get(object.id.as_str()) {
                        object
                            .definition_chain_values
                            .clone_from(&derived.definition_chain_values);
                    }
                }
            }
            if namespace.version() < CATIA_PARALLEL_REFERENCE_COLUMN_INCIDENCE_VERSION {
                let derived_by_id = design_objects
                    .iter()
                    .map(|object| (object.id.as_str(), object))
                    .collect::<HashMap<_, _>>();
                for object in &mut stored {
                    if let Some(derived) = derived_by_id.get(object.id.as_str()) {
                        object
                            .parallel_reference_table
                            .clone_from(&derived.parallel_reference_table);
                    }
                }
            }
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
        }
        let mut finjpl_segments: Vec<CatiaFinjplSegment> =
            if namespace.arenas.contains_key("finjpl_segments") {
                namespace.arena_as("finjpl_segments")?
            } else {
                Vec::new()
            };
        finjpl_segments.sort_by_key(|segment| segment.byte_offset);
        if namespace.version() < CATIA_OBJECT_GRAPH_SEGMENT_VERSION {
            for graph in &mut graphs {
                graph.finjpl_segment =
                    containing_finjpl_segment(graph.byte_offset, graph.byte_len, &finjpl_segments)
                        .map(str::to_owned);
            }
        }
        let mut external_references: Vec<CatiaExternalReference> =
            if namespace.arenas.contains_key("external_references") {
                namespace.arena_as("external_references")?
            } else {
                Vec::new()
            };
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
            if namespace.arenas.contains_key("legacy_entity_runs") {
                namespace.arena_as("legacy_entity_runs")?
            } else {
                Vec::new()
            };
        if namespace.version() < CATIA_LEGACY_IDENTITY_LEAD_VERSION {
            for identity in legacy_entity_runs
                .iter_mut()
                .flat_map(|run| &mut run.identities)
            {
                identity.lead = 0x81;
            }
        }
        if namespace.version() < CATIA_LEGACY_ROLE_SELECTOR_VERSION {
            for run in &mut legacy_entity_runs {
                for field in &mut run.text_fields {
                    if let Some(role) = &mut field.role {
                        role.entity_id = field.entity_id;
                        run.role_selectors.push(role.clone());
                    }
                }
                run.role_selectors.sort_by_key(|role| role.byte_offset);
                run.role_selectors.dedup_by_key(|role| role.byte_offset);
            }
        }
        if namespace.version() < CATIA_LEGACY_SCHEMA_IDENTIFIER_VERSION {
            for program in legacy_entity_runs
                .iter_mut()
                .filter_map(|run| run.schema_program.as_mut())
            {
                program.identifiers = legacy_schema_identifiers(program).ok_or_else(|| {
                    cadmpeg_ir::NativeConvertError::InvalidOwner(
                        "legacy schema-program offset exceeds the platform index range".to_string(),
                    )
                })?;
            }
        }
        if namespace.version() < CATIA_LEGACY_SCHEMA_BOUNDARY_VERSION {
            for program in legacy_entity_runs
                .iter_mut()
                .filter_map(|run| run.schema_program.as_mut())
            {
                program.boundary = CatiaLegacySchemaProgramBoundary::VendorFooter;
            }
        }
        if namespace.version() < CATIA_LEGACY_EVALUATED_VALUE_NAME_VERSION {
            for run in &mut legacy_entity_runs {
                for index in 0..run.scalar_values.len() {
                    let entity_id = run.scalar_values[index].entity_id;
                    let value_offset = run.scalar_values[index].byte_offset;
                    let name = (run
                        .scalar_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.scalar_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.scalar_values[index].name = name.map(|(_, name)| name);
                }
                for index in 0..run.string_values.len() {
                    let entity_id = run.string_values[index].entity_id;
                    let value_offset = run.string_values[index].byte_offset;
                    let name = (run
                        .string_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.string_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.string_values[index].name = name.map(|(_, name)| name);
                }
                for index in 0..run.integer_values.len() {
                    let entity_id = run.integer_values[index].entity_id;
                    let value_offset = run.integer_values[index].byte_offset;
                    let name = (run
                        .integer_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.integer_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.integer_values[index].name = name.map(|(_, name)| name);
                }
            }
        }
        legacy_entity_runs.sort_by_key(|run| run.byte_offset);
        validate_legacy_entity_runs(
            &legacy_entity_runs,
            namespace.version() >= CATIA_LEGACY_ROLE_FIELD_CODE_VERSION,
        )?;
        let mut preview_images: Vec<CatiaPreviewImage> =
            if namespace.arenas.contains_key("preview_images") {
                namespace.arena_as("preview_images")?
            } else {
                Vec::new()
            };
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
        let mut consolidated_class5b5c_records: Vec<CatiaConsolidatedClass5b5cRecord> = if namespace
            .arenas
            .contains_key("consolidated_class5b5c_records")
        {
            namespace.arena_as("consolidated_class5b5c_records")?
        } else {
            Vec::new()
        };
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
        let mut consolidated_edge_nodes: Vec<CatiaConsolidatedEdgeNode> =
            namespace.arena_as("consolidated_edge_nodes")?;
        consolidated_edge_nodes.sort_by_key(|node| node.byte_offset);
        let consolidated_vertex_identities: Vec<CatiaConsolidatedVertexIdentity> =
            namespace.arena_as("consolidated_vertex_identities")?;
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
            &zero_entity_endpoint_pair_candidates,
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
        validate_alias_links(
            &alias_rows,
            &consolidated_owner_packets,
            namespace.version(),
        )?;
        Ok(Self {
            version: namespace.version(),
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
