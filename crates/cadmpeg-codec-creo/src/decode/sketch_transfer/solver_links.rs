// SPDX-License-Identifier: Apache-2.0
//! Unique numeric joins between solver relations, equations, and incidences.

use super::super::feature_history::dimensions::feature_relation_table_complete;
use super::loci::section_skamp_active;
use crate::feature::definitions::{
    FeatureDefinition, FeatureRelation, FeatureRelationTriple, FeatureSkamp, SolverSubtable,
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

type UniqueRows<'rows, T> = HashMap<u32, Option<&'rows T>>;

/// A second row invalidates an ID even when both rows have equal values.
fn unique_rows<'rows, T>(
    ctx: &DecodeContext<'_>,
    rows: &'rows [T],
    key: impl Fn(&T) -> Option<u32>,
    storage: &mut ScopedReservation<'_>,
    operation: &'static str,
) -> Result<UniqueRows<'rows, T>, CodecError> {
    storage.with_storage(|| {
        let mut unique = HashMap::new();
        for row in ctx.admit_iter(rows, operation)? {
            let Some(id) = key(row) else { continue; };
            ctx.entry_hash_map(&mut unique, id, operation)?
                .and_modify(|value| *value = None)
                .or_insert(Some(row));
        }
        Ok(unique)
    })
}

struct IncidenceJoins<'definition, 'ctx> {
    triples: UniqueRows<'definition, FeatureRelationTriple>,
    incidences: UniqueRows<'definition, FeatureSkamp>,
    storage: ScopedReservation<'ctx>,
}

impl<'definition, 'ctx> IncidenceJoins<'definition, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        definition: &'definition FeatureDefinition,
        key: impl Fn(&FeatureRelationTriple) -> Option<u32>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo solver join index storage")?;
        let mut triples = HashMap::new();
        let mut incidences = HashMap::new();
        if let Some(relations) = &definition.relations {
            if relations.triples.as_ref().is_none_or(SolverSubtable::is_complete)
                && relations.skamps.as_ref().is_none_or(SolverSubtable::is_complete)
                && !relations.triples().is_empty() && !relations.skamps().is_empty() {
                triples = unique_rows(ctx, relations.triples(), key, &mut storage, operation)?;
                incidences = unique_rows(ctx, relations.skamps(), |row| Some(row.id),
                    &mut storage, "creo solver incidence identity rows")?;
            }
        }
        Ok(Self { triples, incidences, storage })
    }

    fn joined(&self, id: u32) -> Option<(&'definition FeatureRelationTriple, &'definition FeatureSkamp)> {
        let triple = self.triples.get(&id).copied().flatten()?;
        let incidence = self.incidences.get(&triple.skamp_id?).copied().flatten()?;
        Some((triple, incidence))
    }
}

pub(in super::super) struct RelationIncidences<'definition, 'ctx> {
    relations: UniqueRows<'definition, FeatureRelation>,
    joins: IncidenceJoins<'definition, 'ctx>,
}

impl<'definition, 'ctx> RelationIncidences<'definition, 'ctx> {
    pub(in super::super) fn new(ctx: &'ctx DecodeContext<'_>, definition: &'definition FeatureDefinition) -> Result<Self, CodecError> {
        let mut joins = IncidenceJoins::new(ctx, definition,
            |triple| triple.skamp_id.and(triple.relation_id), "creo solver relation join rows")?;
        let relations = match definition.relations.as_ref().filter(|table| feature_relation_table_complete(table)) {
            Some(table) => unique_rows(ctx, &table.rows, |row| Some(row.relation_id),
                &mut joins.storage, "creo solver relation identity rows")?,
            None => HashMap::new(),
        };
        Ok(Self { relations, joins })
    }

    pub(in super::super) fn is_unique(&self, id: u32) -> bool {
        self.relations.get(&id).is_some_and(Option::is_some)
    }

    pub(in super::super) fn joined(&self, id: u32) -> Option<(&'definition FeatureRelationTriple, &'definition FeatureSkamp)> {
        self.joins.joined(id)
    }
    pub(in super::super) fn is_disabled(&self, id: u32) -> bool {
        self.is_unique(id) && self.joined(id).is_some_and(|(_, incidence)| !section_skamp_active(incidence.status))
    }

}

pub(in super::super) struct EquationIncidences<'definition, 'ctx>(IncidenceJoins<'definition, 'ctx>);

impl<'definition, 'ctx> EquationIncidences<'definition, 'ctx> {
    pub(in super::super) fn new(ctx: &'ctx DecodeContext<'_>, definition: &'definition FeatureDefinition) -> Result<Self, CodecError> {
        IncidenceJoins::new(ctx, definition,
            |triple| triple.skamp_id.and(triple.equation_id), "creo solver equation join rows").map(Self)
    }

    pub(in super::super) fn is_disabled(&self, id: u32) -> bool {
        self.0.joined(id).is_some_and(|(_, incidence)| !section_skamp_active(incidence.status))
    }
}

pub(super) struct SkampEquations<'definition, 'ctx>(IncidenceJoins<'definition, 'ctx>);

impl<'definition, 'ctx> SkampEquations<'definition, 'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, definition: &'definition FeatureDefinition) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo solver join index storage")?;
        let mut triples = HashMap::new();
        let mut incidences = HashMap::new();
        if let Some(relations) = &definition.relations {
            if relations.skamps.as_ref().is_none_or(SolverSubtable::is_complete) {
                incidences = unique_rows(ctx, relations.skamps(), |row| Some(row.id),
                    &mut storage, "creo solver incidence identity rows")?;
                if !incidences.is_empty() && relations.triples.as_ref().is_none_or(SolverSubtable::is_complete) {
                    triples = unique_rows(ctx, relations.triples(),
                        |triple| triple.equation_id.and(triple.skamp_id),
                        &mut storage, "creo solver SKAMP equation join rows")?;
                }
            }
        }
        Ok(Self(IncidenceJoins { triples, incidences, storage }))
    }

    pub(super) fn is_unique(&self, skamp_id: u32) -> bool {
        self.0.incidences.get(&skamp_id).is_some_and(Option::is_some)
    }

    pub(super) fn equation_id(&self, skamp_id: u32) -> Option<u32> {
        self.0.joined(skamp_id)?.0.equation_id
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn duplicate_numeric_join_keys_are_ambiguous() {
        crate::test_support::assert_work_boundaries(&["creo test solver identity rows"], |ctx| {
            let mut storage = ctx.reserve_scoped(0, "creo test solver index storage")?;
            let rows = [(7, 1), (9, 2), (7, 1), (11, 3)];
            let index = super::unique_rows(ctx, &rows, |row| Some(row.0), &mut storage,
                "creo test solver identity rows")?;
            assert_eq!(index.get(&7), Some(&None));
            assert_eq!(index.get(&9).copied().flatten(), Some(&rows[1]));
            assert_eq!(index.get(&11).copied().flatten(), Some(&rows[3]));
            assert!(!index.contains_key(&12));
            Ok(())
        });
    }
}
