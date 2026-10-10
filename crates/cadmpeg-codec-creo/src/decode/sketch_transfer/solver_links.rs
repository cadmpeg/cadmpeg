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
            if relations
                .triples
                .as_ref()
                .is_none_or(SolverSubtable::is_complete)
                && relations
                    .skamps
                    .as_ref()
                    .is_none_or(SolverSubtable::is_complete)
                && !relations.triples().is_empty()
                && !relations.skamps().is_empty()
            {
                let (index, mut index_storage) = ctx.unique_index(
                    ctx.admit_iter(relations.triples(), operation)?
                        .filter_map(|row| key(row).map(|id| (id, row))),
                    operation,
                )?;
                storage.absorb(&mut index_storage)?;
                triples = index;
                let has_join = !triples.is_empty() && ctx.any_by(
                    relations.triples(),
                    |row| Ok(key(row).is_some_and(|id| triples.get(&id).is_some_and(Option::is_some))),
                    "creo solver usable join rows",
                )?;
                if has_join {
                    let (index, mut index_storage) = ctx.unique_index(
                        relations.skamps().iter().map(|row| (row.id, row)),
                        "creo solver incidence identity rows",
                    )?;
                    storage.absorb(&mut index_storage)?;
                    incidences = index;
                }
            }
        }
        Ok(Self {
            triples,
            incidences,
            storage,
        })
    }

    fn joined(
        &self,
        id: u32,
    ) -> Option<(
        &'definition FeatureRelationTriple,
        &'definition FeatureSkamp,
    )> {
        let triple = self.triples.get(&id).copied().flatten()?;
        let incidence = self.incidences.get(&triple.skamp_id?).copied().flatten()?;
        Some((triple, incidence))
    }
}

pub(in super::super) struct RelationIncidences<'definition, 'ctx> {
    pub(super) definition: &'definition FeatureDefinition,
    relations: UniqueRows<'definition, FeatureRelation>,
    joins: IncidenceJoins<'definition, 'ctx>,
}

impl<'definition, 'ctx> RelationIncidences<'definition, 'ctx> {
    /// Build dimension joins only for unique relation rows.
    pub(super) fn for_dimension_rows(
        ctx: &'ctx DecodeContext<'_>,
        definition: &'definition FeatureDefinition,
    ) -> Result<Option<Self>, CodecError> {
        let Some(table) = definition.relations.as_ref().filter(|table| !table.rows.is_empty()) else {
            return Ok(None);
        };
        let mut storage = ctx.reserve_scoped(0, "creo dimension relation index storage")?;
        let relations = if feature_relation_table_complete(table) {
            let (index, mut index_storage) = ctx.unique_index(
                table.rows.iter().map(|row| (row.relation_id, row)),
                "creo solver relation identity rows",
            )?;
            storage.absorb(&mut index_storage)?;
            index
        } else {
            HashMap::new()
        };
        let has_unique_relation = !relations.is_empty() && ctx.any_by(
            &table.rows,
            |row| Ok(relations.get(&row.relation_id).is_some_and(Option::is_some)),
            "creo dimension relation join consumers",
        )?;
        let joins = if has_unique_relation {
            let mut joins = IncidenceJoins::new(
                ctx, definition, |triple| triple.skamp_id.and(triple.relation_id),
                "creo solver relation join rows",
            )?;
            joins.storage.absorb(&mut storage)?;
            joins
        } else {
            IncidenceJoins { triples: HashMap::new(), incidences: HashMap::new(), storage }
        };
        Ok(Some(Self { definition, relations, joins }))
    }

    pub(in super::super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        definition: &'definition FeatureDefinition,
    ) -> Result<Self, CodecError> {
        let mut joins = IncidenceJoins::new(
            ctx,
            definition,
            |triple| triple.skamp_id.and(triple.relation_id),
            "creo solver relation join rows",
        )?;
        let relations = match definition
            .relations
            .as_ref()
            .filter(|table| feature_relation_table_complete(table))
        {
            Some(table) if !table.rows.is_empty() => {
                let (index, mut storage) = ctx.unique_index(
                    table.rows.iter().map(|row| (row.relation_id, row)),
                    "creo solver relation identity rows",
                )?;
                joins.storage.absorb(&mut storage)?;
                index
            },
            _ => HashMap::new(),
        };
        Ok(Self {
            definition,
            relations,
            joins,
        })
    }

    pub(in super::super) fn is_unique(&self, id: u32) -> bool {
        self.relations.get(&id).is_some_and(Option::is_some)
    }

    pub(in super::super) fn joined(
        &self,
        id: u32,
    ) -> Option<(
        &'definition FeatureRelationTriple,
        &'definition FeatureSkamp,
    )> {
        self.joins.joined(id)
    }
    pub(in super::super) fn is_disabled(&self, id: u32) -> bool {
        self.is_unique(id)
            && self
                .joined(id)
                .is_some_and(|(_, incidence)| !section_skamp_active(incidence.status))
    }
}

pub(in super::super) struct EquationIncidences<'definition, 'ctx>(
    IncidenceJoins<'definition, 'ctx>,
);

impl<'definition, 'ctx> EquationIncidences<'definition, 'ctx> {
    pub(in super::super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        definition: &'definition FeatureDefinition,
    ) -> Result<Self, CodecError> {
        IncidenceJoins::new(
            ctx,
            definition,
            |triple| triple.skamp_id.and(triple.equation_id),
            "creo solver equation join rows",
        )
        .map(Self)
    }

    pub(in super::super) fn is_disabled(&self, id: u32) -> bool {
        self.0
            .joined(id)
            .is_some_and(|(_, incidence)| !section_skamp_active(incidence.status))
    }
}

pub(super) struct SkampEquations<'definition, 'ctx>(IncidenceJoins<'definition, 'ctx>);

impl<'definition, 'ctx> SkampEquations<'definition, 'ctx> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        definition: &'definition FeatureDefinition,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo solver join index storage")?;
        let mut triples = HashMap::new();
        let mut incidences = HashMap::new();
        if let Some(relations) = &definition.relations {
            if relations
                .skamps
                .as_ref()
                .is_none_or(SolverSubtable::is_complete)
            {
                let (index, mut index_storage) = ctx.unique_index(
                    relations.skamps().iter().map(|row| (row.id, row)),
                    "creo solver incidence identity rows",
                )?;
                storage.absorb(&mut index_storage)?;
                incidences = index;
                if !incidences.is_empty()
                    && relations
                        .triples
                        .as_ref()
                        .is_none_or(SolverSubtable::is_complete)
                {
                    let (index, mut index_storage) = ctx.unique_index(
                        ctx.admit_iter(relations.triples(), "creo solver SKAMP equation join rows")?
                            .filter_map(|triple| triple.equation_id.and(triple.skamp_id).map(|id| (id, triple))),
                        "creo solver SKAMP equation join rows",
                    )?;
                    storage.absorb(&mut index_storage)?;
                    triples = index;
                }
            }
        }
        Ok(Self(IncidenceJoins {
            triples,
            incidences,
            storage,
        }))
    }

    pub(super) fn is_unique(&self, skamp_id: u32) -> bool {
        self.0
            .incidences
            .get(&skamp_id)
            .is_some_and(Option::is_some)
    }

    pub(super) fn equation_id(&self, skamp_id: u32) -> Option<u32> {
        self.0.joined(skamp_id)?.0.equation_id
    }
}

#[cfg(test)]
mod tests;
