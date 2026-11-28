// SPDX-License-Identifier: Apache-2.0
//! The nested occurrence that a joint operand's residual native keys name.

use std::collections::BTreeMap;

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::Occurrence;

use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::xref::XrefReference;

/// The native occurrence key of each record that places an occurrence, by
/// the record's id: a component occurrence carrier, an external reference's
/// placement source, and a component insert's construction.
pub(crate) struct OperandKeys<'a>(BTreeMap<&'a str, &'a str>);

impl<'a> OperandKeys<'a> {
    pub(crate) fn new(
        carriers: &'a [DesignComponentOccurrence],
        references: &'a [XrefReference],
        scopes: &'a [DesignParameterScope],
    ) -> Self {
        Self(
            carriers
                .iter()
                .map(|carrier| (carrier.id.as_str(), carrier.occurrence_guid.as_str()))
                .chain(references.iter().filter_map(|reference| {
                    Some((
                        reference.id.as_str(),
                        reference
                            .placement_source
                            .as_ref()?
                            .occurrence_guid
                            .as_ref()?
                            .as_str(),
                    ))
                }))
                .chain(scopes.iter().filter_map(|scope| {
                    let key = scope
                        .component_insert_construction()?
                        .occurrence_key
                        .as_ref()?;
                    Some((scope.id.as_str(), key.value.as_str()))
                }))
                .collect(),
        )
    }

    /// Move `occurrence` to the child occurrence that each leading key of
    /// `path` names, while exactly one child matches, and remove the keys
    /// it consumes from `path`. `parents` gives each occurrence's parent.
    pub(crate) fn descend(
        &self,
        occurrences: &[Occurrence],
        parents: &BTreeMap<OccurrenceId, OccurrenceId>,
        occurrence: &mut OccurrenceId,
        path: &mut Vec<String>,
    ) {
        let mut consumed = 0;
        for key in path.iter() {
            let mut children = occurrences.iter().filter(|child| {
                parents.get(&child.id) == Some(&*occurrence)
                    && child
                        .native_ref
                        .as_deref()
                        .and_then(|id| self.0.get(id))
                        .is_some_and(|native_key| native_key.eq_ignore_ascii_case(key))
            });
            match (children.next(), children.next()) {
                (Some(child), None) => {
                    *occurrence = child.id.clone();
                    consumed += 1;
                }
                _ => break,
            }
        }
        path.drain(..consumed);
    }
}
