// SPDX-License-Identifier: Apache-2.0
//! Test-only CATIA native decode/load/store helpers.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;

use cadmpeg_ir::geometry::{knots_nondecreasing, knots_strictly_increasing};

use crate::container;
use crate::entity_table;
use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;
use crate::legacy_entity;
use crate::object_graph;
use crate::value_block;

use super::*;
use super::{
    consolidated_vertex_identities, containing_finjpl_segment, definition_chain_value,
    definition_schema_selections, definition_value, derive_reference_signature_cohorts,
    derive_schema_configuration_row_chains, design_object_id, design_objects, entity_class_index,
    entity_incidences, entity_suffix_schema_selection, entity_suffix_value,
    entity_value_schema_selections, external_reference_views, finjpl_family, formula_relation,
    parameter_value, preview_views, range_interval, reference_signature, relation_expression,
    relation_program_instance, repeated_reference_schema_selection, resolved_constraint_range,
    resolved_payload_references, resolved_storage_link, schema_configuration_record,
    schema_configuration_row_link, semantic_entity_indices, store_projection,
    terminal_null_entity_id, valid_legacy_identifier, value_schema_selections,
    zero_entity_endpoint_locus_candidates, zero_entity_endpoint_pair_candidates,
    zero_entity_record, zero_entity_vertex_owner, CatiaEntityReferenceIndex,
    CATIA_CONFIGURATION_INCIDENCE_VERSION, CATIA_DEFINITION_CHAIN_OWNERSHIP_VERSION,
    CATIA_FORMULA_DEPENDENCY_CANDIDATE_VERSION, CATIA_LEGACY_EVALUATED_VALUE_NAME_VERSION,
    CATIA_LEGACY_IDENTITY_LEAD_VERSION, CATIA_LEGACY_ROLE_FIELD_CODE_VERSION,
    CATIA_LEGACY_ROLE_SELECTOR_VERSION, CATIA_LEGACY_SCHEMA_BOUNDARY_VERSION,
    CATIA_LEGACY_SCHEMA_IDENTIFIER_VERSION, CATIA_OBJECT_GRAPH_SEGMENT_VERSION,
    CATIA_SUFFIX_FRAMING_VERSION, CATIA_TERMINAL_NULL_REFERENCE_VERSION,
    CATIA_TYPED_OWNER_SLOT_VERSION,
};

mod test_consolidated;
mod test_legacy;
mod test_links;
mod test_load;
mod test_zero_entity;

impl CatiaOwnerPacketPayload {
    fn final_reference(&self) -> Option<u32> {
        match self {
            Self::FixedNine { references, .. } => references.last().copied(),
            Self::Counted { references, .. } => references.last().copied(),
        }
    }
}
