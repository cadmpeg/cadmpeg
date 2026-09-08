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

use super::*;
use super::{
    consolidated_vertex_identities, containing_finjpl_segment, definition_schema_selections,
    derive_reference_signature_cohorts, design_object_id, design_objects, entity_class_index,
    entity_suffix_schema_selection, entity_suffix_value, entity_value_schema_selections,
    external_reference_views, finjpl_family, preview_views, range_interval, reference_signature,
    repeated_reference_schema_selection, resolved_payload_references, resolved_storage_link,
    semantic_entity_indices, store_projection, terminal_null_entity_id, valid_legacy_identifier,
    value_schema_selections, zero_entity_endpoint_locus_candidates,
    zero_entity_endpoint_pair_candidates, zero_entity_record, zero_entity_vertex_owner,
    CatiaEntityReferenceIndex,
};
use crate::native::schema_configuration_chain::derive_schema_configuration_row_chains;

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
