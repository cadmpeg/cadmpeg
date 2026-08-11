// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA configuration identities.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    ConfigurationActivation, ConfigurationBodies, ConfigurationId, ConfigurationName,
    DesignConfiguration,
};

use crate::design_feature::neutral_history_id;
use crate::native::CatiaNative;

/// Object records represented by transferred neutral configuration identities.
#[derive(Debug, Default)]
pub(crate) struct ConfigurationTransfer {
    pub(crate) consumed_object_records: HashSet<String>,
}

/// Transfer each unambiguous self-defining `Configuration` record in modeling scope.
///
/// The native production establishes an identity but does not establish a
/// display name, active state, source slot, row membership, or model state.
/// Those fields remain unresolved instead of receiving generated values.
pub(crate) fn transfer(
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: Option<&HashSet<String>>,
) -> ConfigurationTransfer {
    let mut source_id_counts = HashMap::<&str, usize>::new();
    for entity in native.entity_records.iter().filter(|entity| {
        entity.configuration_record.is_some()
            && graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
        *source_id_counts.entry(entity.id.as_str()).or_default() += 1;
    }

    let mut result = ConfigurationTransfer::default();
    for entity in native.entity_records.iter().filter(|entity| {
        entity.configuration_record.is_some()
            && graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
            && source_id_counts.get(entity.id.as_str()) == Some(&1)
    }) {
        let id = ConfigurationId(neutral_history_id(&entity.id, "configuration"));
        if ir.model.configurations.iter().any(|configuration| {
            configuration.id == id
                || configuration.native_ref.as_deref() == Some(entity.id.as_str())
        }) {
            continue;
        }
        let Ok(ordinal) = u32::try_from(ir.model.configurations.len()) else {
            break;
        };
        ir.model.configurations.push(DesignConfiguration {
            id,
            ordinal,
            active: ConfigurationActivation::Unresolved,
            source_index: None,
            name: ConfigurationName::Unresolved,
            material: None,
            properties: BTreeMap::default(),
            parameter_overrides: BTreeMap::default(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Unresolved,
            parameter_values: BTreeMap::default(),
            feature_states: BTreeMap::default(),
            native_ref: Some(entity.id.clone()),
        });
        result
            .consumed_object_records
            .insert(entity.object_record.clone());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    use cadmpeg_ir::units::Units;

    use crate::native::{CatiaConfigurationRecord, CatiaEntityRecord, CatiaPayloadEntityReference};

    fn configuration_entity(id: &str, graph: &str) -> CatiaEntityRecord {
        CatiaEntityRecord {
            id: id.to_string(),
            object_graph: graph.to_string(),
            object_record: format!("{id}:object"),
            ordinal: 0,
            byte_offset: 0,
            byte_len: 0,
            lead: 0,
            definition_len: 0,
            definition_prefix: Vec::new(),
            definition_schema_selections: Vec::new(),
            entity_id: 1,
            definition_suffix: Vec::new(),
            value_len: 0,
            value_payload: Vec::new(),
            value_fields: Vec::new(),
            value_schema_selections: Vec::new(),
            relation_expression: None,
            parameter_value: None,
            constraint_range: None,
            definition_value: None,
            definition_chain_value: None,
            relation_program_instance: None,
            configuration_record: Some(CatiaConfigurationRecord {
                schema_payload_offset: 0,
                schema_ordinal: 0,
                schema_entry: "entry".to_string(),
                schema_name: "schema".to_string(),
                entity_reference: CatiaPayloadEntityReference::default(),
            }),
            configuration_row_link: None,
            formula_relation: None,
            value_packets: Vec::new(),
            numeric_pair: None,
            reference_signature: None,
            record_suffix: Vec::new(),
            suffix_value: None,
            suffix_framing: None,
            suffix_schema_selection: None,
        }
    }

    #[test]
    fn transfers_only_unambiguous_in_scope_configuration_identities() {
        let retained = configuration_entity("catia:outer:entity-record#retained", "model");
        let duplicate = configuration_entity("catia:outer:entity-record#duplicate", "model");
        let out_of_scope = configuration_entity("catia:outer:entity-record#outside", "other");
        let native = CatiaNative {
            entity_records: vec![retained.clone(), duplicate.clone(), duplicate, out_of_scope],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());

        let transferred = transfer(
            &mut ir,
            &native,
            Some(&HashSet::from(["model".to_string()])),
        );

        let [configuration] = ir.model.configurations.as_slice() else {
            panic!("one exact configuration identity")
        };
        assert_eq!(configuration.id.0, "catia:outer:configuration#retained");
        assert_eq!(configuration.ordinal, 0);
        assert_eq!(configuration.name, ConfigurationName::Unresolved);
        assert_eq!(configuration.active, ConfigurationActivation::Unresolved);
        assert!(configuration.bodies.is_unresolved());
        assert_eq!(
            configuration.native_ref.as_deref(),
            Some(retained.id.as_str())
        );
        assert_eq!(
            transferred.consumed_object_records,
            HashSet::from([retained.object_record])
        );
    }
}
