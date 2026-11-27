// SPDX-License-Identifier: Apache-2.0
//! The configuration table and rule documents of a generated Design stream.
//! Where the neutral configurations differ from the configurations that the
//! documents project, the neutral record wins (cadir-v11.md §3.4, W2; §5.14):
//! the write regenerates the rows of the table, their cells and order, and
//! the targets of the rules from the configurations (f3d.md §1.2). A
//! parameter, feature, part number or description with no column gets a
//! column, whose cell in each row states what the row's configuration
//! resolves, and an empty table document gets a table (cadir-v11.md §5.11).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{edit_dropped, record_residual_unavailable, value_unwritable};
use cadmpeg_ir::features::parameters::{DesignConfiguration as Configuration, ParameterValue};
use cadmpeg_ir::features::{ConfigurationId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::units::Tolerances;
use cadmpeg_ir::CadIr;
use serde_json::{Map, Value};

use super::configuration_root::{plan, Plan};
use super::presentation::deterministic_guid;
use crate::design::configurations::{table_at, ConfigurationTargets, Projection};
use crate::design::feature_project::DESIGN_LENGTH_TO_MILLIMETRE;
use crate::design_v11::configurations::{agree, restate, ActiveRows, RestatedRows};
use crate::ids::neutral_configuration_id;
use crate::native::F3dNative;
use crate::records::configuration::table::{
    new_document, new_row, property_component, push_column, push_property_column, push_row,
    remove_row, set_cell, set_row_name, set_row_order, write_test_rows, AdmittedTables, CellData,
    ColumnTarget, ConfigurationColumn, ConfigurationRow, ConfigurationTable, ConfiguredValue,
    RowGuids, TableNames, TestRowTarget,
};
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::writer::primitives::{
    bases, configuration_targets, decoded_configurations, differences, projection, projects,
    reconciled, same_row, Bases, Difference,
};

/// The configuration documents the write holds, and what they read back as.
pub(super) struct Written<'a> {
    /// The documents in source order.
    pub(super) documents: Cow<'a, [DesignConfiguration]>,
    /// The projection of `documents`.
    pub(super) projection: Projection,
    /// The configuration id of each row that the write holds under a row id
    /// the writer generates, keyed by the configuration id the row reads
    /// back as.
    pub(super) generated: BTreeMap<String, String>,
    /// The configuration of each written row, keyed by the configuration id
    /// the row reads back as.
    expected: BTreeMap<String, &'a Configuration>,
    /// The ids of the columns the write adds to the table of the rows.
    added: BTreeSet<String>,
    /// The configuration of the row the stored root record names active.
    stored_active: Option<Configuration>,
}

/// The `tableName` and the `tableType` of a table the write creates in an
/// empty table document (f3d-open-items.md DR-1069).
const NEW_TABLE_NAME: &str = "Configurations";
const NEW_TABLE_TYPE: &str = "Configurations";

/// Write `documents` with the rows, cells, row order and rule targets of
/// `configurations`, where they differ from `decoded`, the configurations
/// that `documents` project. Return the written documents, and the losses
/// of the configurations that no table holds a row for.
pub(super) fn regenerate<'a>(
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    configurations: &'a [Configuration],
    targets: &ConfigurationTargets<'_>,
) -> Result<(Written<'a>, Vec<LossNote>), CodecError> {
    let mut payloads = vec![None; documents.len()];
    let mut losses = Vec::new();
    let bases = bases(targets.neutral_features, targets.neutral_parameters);
    let stored = host(documents, decoded.table);
    let created = match stored {
        None if !configurations.is_empty() => created(documents),
        _ => None,
    };
    if let Some((index, payload, _)) = &created {
        payloads[*index] = Some(payload.clone());
        losses.extend(record_residual_unavailable(
            "F3D",
            &format!(
                "writes a table into this empty configuration table document with the table name {NEW_TABLE_NAME:?} and the table type {NEW_TABLE_TYPE:?}, and a table id and asset ids the writer generates: f3d-open-items.md DR-1069 leaves these members of a new table open"
            ),
            [documents[*index].id()],
        ));
    }
    let host = stored
        .map(|(index, table)| (index, documents[index].payload(), table))
        .or_else(|| {
            created
                .as_ref()
                .map(|(index, payload, table)| (*index, payload, table))
        });
    let (expected, added) = match host {
        Some((index, source, table)) => {
            let (expected, added, written) = Rows {
                entry: documents[index].entry_name(),
                source,
                payload: &mut payloads[index],
                table,
                columns: table.columns.clone(),
                added: BTreeSet::new(),
                targets,
                bases: &bases,
                unfilled: Vec::new(),
                components: Vec::new(),
            }
            .write(&decoded.configurations, configurations);
            losses.extend(written);
            (expected, added)
        }
        None => {
            losses.extend(edit_dropped(
                EntityKind::Configuration,
                "F3D",
                "writes no row for these configurations: the configuration table documents hold no table for their rows, or more than one",
                configurations.iter().map(|configuration| configuration.id.to_string()),
            ));
            (BTreeMap::new(), BTreeSet::new())
        }
    };
    rules(
        documents,
        &mut payloads,
        &decoded.configurations,
        configurations,
    );
    let documents = written(documents, payloads)?;
    let projection = projection(&documents, targets)?;
    let generated = expected
        .iter()
        .filter(|(written, configuration)| configuration.id.as_str() != written.as_str())
        .map(|(written, configuration)| (written.clone(), configuration.id.to_string()))
        .collect();
    let stored_active = decoded
        .configurations
        .iter()
        .find(|configuration| configuration.active)
        .cloned();
    Ok((
        Written {
            documents,
            projection,
            generated,
            expected,
            added,
            stored_active,
        },
        losses,
    ))
}

impl Written<'_> {
    /// The losses of what the written documents read back as otherwise than
    /// the configurations and the features of `targets`, when the root
    /// record names the row of the configuration `active`. The decoder
    /// reconciles the features with that row (f3d.md §1.2: the design
    /// stores the state of the active row). A feature whose suppression that
    /// row states otherwise than the reconciled feature is written in a state
    /// the row contradicts, unless the write keeps both the state its scope
    /// stores and the cell of the source's active row: the source holds the
    /// same contradiction. Each `pinned` cell is charged with the value the
    /// write states in its place.
    pub(super) fn read_back(
        &self,
        active: Option<&str>,
        targets: &ConfigurationTargets<'_>,
        pinned: &[Pinned],
    ) -> Result<Vec<LossNote>, CodecError> {
        let mut read = self.projection.clone();
        for configuration in &mut read.configurations {
            configuration.active = self
                .expected
                .get(configuration.id.as_str())
                .is_some_and(|expected| Some(expected.id.as_str()) == active);
        }
        let mut features = targets.neutral_features.to_vec();
        let read = reconciled(read, &mut features)?;
        let bases = bases(targets.neutral_features, targets.neutral_parameters);
        let mut losses = compare(&self.expected, &read.configurations, &bases);
        losses.extend(value_unwritable(
            "F3D",
            "writes these parameters of an active configuration at the expression and value of the Design stream, which stores the state of the active row (f3d.md §1.2) and which the writer cannot set to what the configuration resolves",
            pinned.iter().map(|pinned| {
                format!(
                    "{}: {}",
                    pinned.configuration,
                    Difference::of(
                        pinned.parameter.to_string(),
                        Some(&pinned.stated),
                        Some(&pinned.written),
                    )
                )
            }),
        ));
        losses.extend(value_unwritable(
            "F3D",
            "writes these features in the current state that the active row of the written configuration table gives them: the Design stream stores the state of its active row",
            targets
                .neutral_features
                .iter()
                .zip(&features)
                .filter_map(|(stated, read)| {
                    let (stated_state, read_state) = (
                        stated.configuration_state().evaluation,
                        read.configuration_state().evaluation,
                    );
                    (stated_state != read_state).then(|| {
                        Difference::of(
                            stated.id.to_string(),
                            Some(&stated_state),
                            Some(&read_state),
                        )
                        .to_string()
                    })
                }),
        ));
        let active_states = read
            .configurations
            .iter()
            .filter(|configuration| configuration.active)
            .flat_map(|configuration| &configuration.feature_states)
            .collect::<BTreeMap<_, _>>();
        let stored = targets
            .scopes
            .iter()
            .filter_map(|scope| {
                Some((
                    crate::ids::neutral_feature_id(scope),
                    scope.stored_suppression()?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let kept = |feature: &cadmpeg_ir::features::Feature, row: bool| {
            feature
                .suppressed
                .is_some_and(|state| stored.get(&feature.id) == Some(&state))
                && self
                    .stored_active
                    .as_ref()
                    .and_then(|stored| stored.feature_states.get(&feature.id))
                    .map(|state| state.evaluation.is_suppressed())
                    == Some(row)
        };
        losses.extend(value_unwritable(
            "F3D",
            "writes an active row of the configuration table that states these features otherwise than the document: the Design stream stores the state of its active row, so a feature whose scope stores no suppression and to which the body history binds no output body reads back in the row's state",
            features.iter().filter_map(|feature| {
                let row = &active_states.get(&feature.id)?.evaluation;
                let current = feature.configuration_state().evaluation;
                (row.is_suppressed() != current.is_suppressed()
                    && !kept(feature, row.is_suppressed()))
                .then(|| {
                    let shown = |state: &cadmpeg_ir::features::parameters::ConfigurationEvaluation| {
                        serde_json::to_string(state).unwrap_or_default()
                    };
                    format!(
                        "{} stated {}, the active row states {}",
                        feature.id,
                        shown(&current),
                        shown(row)
                    )
                })
            }),
        ));
        Ok(losses)
    }
}

/// The table that holds the rows of the configurations: the table whose rows
/// the decoder projects, else the one table of `documents` when it holds no
/// row.
fn host(
    documents: &[DesignConfiguration],
    projected: Option<(usize, usize)>,
) -> Option<(usize, &ConfigurationTable)> {
    if projected.is_some() {
        return table_at(documents, projected);
    }
    let mut tables = documents
        .iter()
        .enumerate()
        .flat_map(|(index, document)| document.tables().iter().map(move |table| (index, table)));
    let first = tables.next()?;
    (tables.next().is_none() && first.1.rows.is_empty()).then_some(first)
}

/// A parameter cell of the row of an active configuration that the write
/// states at the Design stream's expression and value, in place of the cell
/// the configuration resolves: the expression and value it resolves
/// (`stated`), and the ones the write states (`written`).
pub(super) struct Pinned {
    configuration: ConfigurationId,
    parameter: ParameterId,
    stated: (String, Option<ParameterValue>),
    written: (String, Option<ParameterValue>),
}

/// The target of the configuration tables that [`stated_rows`] derives.
pub(super) struct Stated {
    /// The rows, with the cells of each active row that the Design stream
    /// states removed: the cell writer fills them with the parameter's own
    /// expression and value.
    rows: Vec<Configuration>,
    pinned: Vec<Pinned>,
    /// The parameters whose own expression or value the write states
    /// otherwise than the stored Design stream.
    rewritten: BTreeSet<ParameterId>,
    /// One `<stored> active in place of <document>` per scope whose stored
    /// active row the rows keep ([`ActiveRows::Stored`]).
    kept: Vec<String>,
}

/// The target of the configuration tables of `target`: the v10 rows that
/// state its configurations (its rows where it holds them, else `decoded`,
/// the rows its configuration tables project, with what the configurations
/// of its `design` state otherwise than `decoded` written in: cadir-v11.md
/// §3.4, W2), with the active rows `which` selects. The features and
/// parameters of `targets` are the history the rows set, and the parameters'
/// own expressions and values are the Design stream the write states.
/// Adds to `losses` what no row states, and each value of a held row that
/// the configurations state otherwise.
///
/// The Design stream stores the state of the active row (f3d.md §1.2). A
/// cell of an active row resolves the Design stream value where its
/// parameter is in `rebased`, the parameters whose own expression and value
/// the write takes from the active configuration, or where neither the
/// design nor an edit of a held row states it; the write states the Design
/// stream value there, except that the stored active row keeps an unedited
/// cell of a parameter the write does not rewrite. Each other cell of an
/// active row that differs from the Design stream value is pinned to it. A
/// row made active drops the feature cells that neither the design nor an
/// edit states: its configuration resolves the feature's own state.
pub(super) fn stated_rows(
    target: &CadIr,
    decoded: &[Configuration],
    targets: &ConfigurationTargets<'_>,
    rebased: &BTreeSet<ParameterId>,
    which: ActiveRows,
    losses: &mut Vec<LossNote>,
) -> Stated {
    let held = target.model.omitted_arenas_held();
    let rows = if held {
        target.model.configurations.as_slice()
    } else {
        decoded
    };
    let RestatedRows {
        rows: mut restated,
        active,
        stated,
        kept,
    } = restate(
        rows,
        decoded,
        target,
        targets.neutral_features,
        targets.neutral_parameters,
        which,
    )
    .into_rows(losses);
    let rewritten = match which {
        ActiveRows::Design => rewritten(targets, &target.tolerances),
        ActiveRows::Stored => BTreeSet::new(),
    };
    let stored = decoded
        .iter()
        .map(|row| (&row.id, row))
        .collect::<BTreeMap<_, _>>();
    let own = targets
        .neutral_parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<BTreeMap<_, _>>();
    let mut pinned = Vec::new();
    for row in restated.iter_mut().filter(|row| active.contains(&row.id)) {
        let source = stored.get(&row.id).copied();
        let stays = source.is_some_and(|source| source.active);
        let cells = row.parameter_overrides.keys().cloned().collect::<Vec<_>>();
        for parameter in cells {
            let Some(own) = own.get(&parameter) else {
                continue;
            };
            let design = stated
                .parameters
                .contains(&(row.id.clone(), parameter.clone()));
            let edited = !source.is_some_and(|source| {
                source.parameter_overrides.get(&parameter)
                    == row.parameter_overrides.get(&parameter)
                    && source.parameter_values.get(&parameter)
                        == row.parameter_values.get(&parameter)
            });
            if rebased.contains(&parameter) || !(design || edited) {
                if !(stays && !edited && !rewritten.contains(&parameter)) {
                    row.parameter_overrides.remove(&parameter);
                    row.parameter_values.remove(&parameter);
                }
                continue;
            }
            let expression = &row.parameter_overrides[&parameter];
            let value = row.parameter_values.get(&parameter);
            let agrees = *expression == own.expression
                && match (value, &own.value) {
                    (Some(cell), Some(own)) => agree(cell, own, &target.tolerances),
                    (cell, own) => cell == own.as_ref(),
                };
            if !agrees {
                pinned.push(Pinned {
                    configuration: row.id.clone(),
                    parameter,
                    stated: (expression.clone(), value.cloned()),
                    written: (own.expression.clone(), own.value.clone()),
                });
            }
        }
        if !stays {
            row.feature_states.retain(|feature, state| {
                stated.features.contains(&(row.id.clone(), feature.clone()))
                    || !source
                        .is_some_and(|source| source.feature_states.get(feature) == Some(state))
            });
        }
    }
    if held {
        losses.extend(edit_dropped(
            EntityKind::Configuration,
            "F3D",
            "drops these edits of the v10 configuration rows, which the design configurations state otherwise: the neutral record wins (cadir-v11.md §3.4, W2)",
            overridden(rows, decoded, &restated, targets),
        ));
    }
    for pin in &pinned {
        if let Some(row) = restated.iter_mut().find(|row| row.id == pin.configuration) {
            row.parameter_overrides.remove(&pin.parameter);
            row.parameter_values.remove(&pin.parameter);
        }
    }
    Stated {
        rows: restated,
        pinned,
        rewritten,
        kept,
    }
}

/// Each edit of a held v10 row of `rows`, against `decoded`, the rows the
/// stored tables project, that `restated` states otherwise.
fn overridden(
    rows: &[Configuration],
    decoded: &[Configuration],
    restated: &[Configuration],
    targets: &ConfigurationTargets<'_>,
) -> Vec<String> {
    let bases = bases(targets.neutral_features, targets.neutral_parameters);
    let by_id = |rows: &[Configuration]| {
        rows.iter()
            .map(|row| (row.id.clone(), row.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    let (decoded, restated) = (by_id(decoded), by_id(restated));
    let mut out = Vec::new();
    for row in rows {
        let (Some(stored), Some(written)) = (decoded.get(&row.id), restated.get(&row.id)) else {
            continue;
        };
        let edited = differences(stored, row, &bases)
            .into_iter()
            .map(|difference| difference.field)
            .collect::<BTreeSet<_>>();
        out.extend(
            differences(row, written, &bases)
                .into_iter()
                .filter(|difference| edited.contains(&difference.field))
                .map(|difference| format!("{}: {difference}", row.id)),
        );
        if row.name != stored.name && written.name != row.name {
            out.push(format!("{}: name", row.id));
        }
        if row.active != stored.active && written.active != row.active {
            out.push(format!("{}: active", row.id));
        }
    }
    out
}

/// The parameters of `targets` whose own expression or value differs from
/// the Design stream record of the parameter, or that have none.
fn rewritten(targets: &ConfigurationTargets<'_>, tolerances: &Tolerances) -> BTreeSet<ParameterId> {
    let records = targets
        .parameters
        .iter()
        .map(|record| (crate::ids::neutral_parameter_id(record), record))
        .collect::<BTreeMap<_, _>>();
    targets
        .neutral_parameters
        .iter()
        .filter(|own| {
            records.get(&own.id).is_none_or(|record| {
                record.expression() != own.expression
                    || match (
                        targets.parameter_value(&own.id, record.evaluated_value()),
                        &own.value,
                    ) {
                        (Some(stored), Some(own)) => !agree(&stored, own, tolerances),
                        (stored, own) => stored.as_ref() != own.as_ref(),
                    }
            })
        })
        .map(|own| own.id.clone())
        .collect()
}

/// The configuration documents a patch of the retained archive carries, and
/// the losses of what the configurations state otherwise than the rows of
/// the documents.
pub(in crate::writer) struct Patched {
    /// The documents, where they state the configurations otherwise than the
    /// stored documents.
    pub(in crate::writer) documents: Option<Vec<DesignConfiguration>>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

/// The configuration documents of `target`, whose native records are
/// `native`, rewritten to state its configurations ([`stated_rows`]). The
/// patch writes no Design stream parameter, so the row the stored
/// `DesignConfigurationRoot` record names stays active, and each scope whose
/// active configuration differs is charged.
pub(in crate::writer) fn patched(
    target: &CadIr,
    native: &F3dNative,
) -> Result<Patched, CodecError> {
    let targets = configuration_targets(target, native);
    let documents = &native.design_configurations;
    let decoded = decoded_configurations(documents, &targets)?;
    let mut losses = Vec::new();
    let stated = stated_rows(
        target,
        &decoded.configurations,
        &targets,
        &BTreeSet::new(),
        ActiveRows::Stored,
        &mut losses,
    );
    losses.extend(value_unwritable(
        "F3D",
        "keeps the archive's active configuration state in place of the requested state: the DesignConfigurationRoot names the row whose state the Design stream stores (f3d.md §1.2), and the patch preserves its parameter values",
        stated.kept.iter().cloned(),
    ));
    if stated.pinned.is_empty() && projects(&stated.rows, documents, &targets)? {
        return Ok(Patched {
            documents: None,
            losses,
        });
    }
    let (mut written, regenerated) = regenerate(documents, &decoded, &stated.rows, &targets)?;
    losses.extend(regenerated);
    let active = decoded
        .configurations
        .iter()
        .find(|configuration| configuration.active)
        .map(|configuration| configuration.id.as_str());
    written.finish(active, &targets, &stated, &mut losses)?;
    Ok(Patched {
        documents: Some(written.documents.into_owned()),
        losses,
    })
}

/// The configuration documents of a generated Design stream: `documents`,
/// whose rows are `decoded`, written to state `stated`, with the root record
/// `target` names, and the `testRowData` entries of its active row. Adds to
/// `losses` what the documents read back as otherwise.
pub(super) fn generated<'a>(
    target: &CadIr,
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    stated: &'a Stated,
    targets: &ConfigurationTargets<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(Written<'a>, Plan), CodecError> {
    let (mut written, regenerated) = regenerate(documents, decoded, &stated.rows, targets)?;
    losses.extend(regenerated);
    let plan = plan(target, documents, decoded.table, &written, &stated.rows);
    written.finish(plan.active(), targets, stated, losses)?;
    Ok((written, plan))
}

impl Written<'_> {
    /// Write the `testRowData` entries of the row of the configuration
    /// `active` ([`Self::test_rows`]), and add to `losses` what the
    /// documents read back as otherwise than `stated` ([`Self::read_back`]).
    fn finish(
        &mut self,
        active: Option<&str>,
        targets: &ConfigurationTargets<'_>,
        stated: &Stated,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if let Some((index, payload)) = self.test_rows(active, targets, &stated.rewritten, losses) {
            let document = self.documents[index].with_payload(payload)?;
            self.documents.to_mut()[index] = document;
        }
        losses.extend(self.read_back(active, targets, &stated.pinned)?);
        Ok(())
    }

    /// The payload of the table document of the rows with the `testRowData`
    /// entries that state the values of the row of the configuration
    /// `active` (f3d.md §1.2): an entry of each added column, and the stored
    /// entry of each other column where the active row moves, where the
    /// active row states the column otherwise than the stored active row, or
    /// where the column's parameter is `rewritten`. Return the document's
    /// position and payload where they change, and charge to `losses` the
    /// entries the write adds, rewrites or keeps.
    fn test_rows(
        &self,
        active: Option<&str>,
        targets: &ConfigurationTargets<'_>,
        rewritten: &BTreeSet<ParameterId>,
        losses: &mut Vec<LossNote>,
    ) -> Option<(usize, Map<String, Value>)> {
        let (index, table) = host(&self.documents, self.projection.table)?;
        let bases = bases(targets.neutral_features, targets.neutral_parameters);
        let configuration = active.and_then(|active| {
            self.expected
                .values()
                .copied()
                .find(|configuration| configuration.id.as_str() == active)
        });
        let stored = self.stored_active.as_ref();
        let moved = active != stored.map(|stored| stored.id.as_str());
        let entry_name = self.documents[index].entry_name();
        let expected = table
            .columns
            .iter()
            .filter_map(|column| {
                let members = test_row(column, configuration, targets, &bases);
                let added = self.added.contains(&column.id);
                let rewritten = column_parameter(&column.target, targets)
                    .is_some_and(|parameter| rewritten.contains(&parameter));
                let restated = moved
                    || added
                    || rewritten
                    || members != test_row(column, stored, targets, &bases);
                restated.then_some((
                    column.id.as_str(),
                    TestRowTarget {
                        members,
                        required: added || rewritten || stored.is_some(),
                    },
                ))
            })
            .collect::<BTreeMap<_, _>>();
        if expected.is_empty() {
            return None;
        }
        let mut payload = self.documents[index].payload().clone();
        let result = write_test_rows(&mut payload, &expected);
        if result.added {
            losses.extend(record_residual_unavailable(
                "F3D",
                "adds testRowData entries to this configuration table document with the values of the active row in the members f3d.md §1.2 names: f3d-open-items.md DR-1069 leaves the member order and the unit of the value of a new entry open",
                [entry_name],
            ));
        }
        if result.rewritten {
            losses.extend(record_residual_unavailable(
                "F3D",
                "writes testRowData entries of this configuration table document with the values of the active row, each with its members in key order: f3d-open-items.md DR-1069 leaves the member order and the unit of the value of a written entry open",
                [entry_name],
            ));
        }
        losses.extend(value_unwritable(
            "F3D",
            "writes testRowData that reads back otherwise than the target value of these columns in the active row",
            result.unwritten.into_iter().map(|column| format!("{entry_name}: column {column}")),
        ));
        (result.added || result.rewritten).then_some((index, payload))
    }
}

/// The only table document of `documents`, where it is the empty object, as
/// a document that holds one table with no column and no row: its position,
/// payload and table.
fn created(
    documents: &[DesignConfiguration],
) -> Option<(usize, Map<String, Value>, ConfigurationTable)> {
    let mut tables = documents
        .iter()
        .enumerate()
        .filter(|(_, document)| document.kind() == DesignConfigurationKind::Table);
    let (index, document) = tables.next()?;
    if tables.next().is_some() || !document.payload().is_empty() {
        return None;
    }
    let entry = document.entry_name();
    let id = deterministic_guid("configuration table", entry);
    let payload = new_document(
        &TableNames {
            id: &id,
            name: NEW_TABLE_NAME,
            kind: NEW_TABLE_TYPE,
        },
        |domain| deterministic_guid(domain, entry),
    );
    let table = AdmittedTables::admit(&payload).tables.into_iter().next()?;
    Some((index, payload, table))
}

/// The payload of a document the write changes, from a copy of `source`.
fn edit<'p>(
    payload: &'p mut Option<Map<String, Value>>,
    source: &Map<String, Value>,
) -> &'p mut Map<String, Value> {
    payload.get_or_insert_with(|| source.clone())
}

/// The rows of `table`, the table at `table.position` of the document
/// `entry`, which `source` states and `payload` writes.
struct Rows<'s, 'p> {
    entry: &'s str,
    source: &'s Map<String, Value>,
    payload: &'p mut Option<Map<String, Value>>,
    table: &'s ConfigurationTable,
    /// The columns of `table` and the columns the write adds.
    columns: Vec<ConfigurationColumn>,
    /// The ids of the columns the write adds.
    added: BTreeSet<String>,
    targets: &'s ConfigurationTargets<'s>,
    /// What a configuration that states no override resolves to.
    bases: &'s Bases<'s>,
    /// Each row the write leaves with no cell of a property column it adds.
    unfilled: Vec<String>,
    /// Each property column the write adds under a component id it
    /// generates.
    components: Vec<String>,
}

impl Rows<'_, '_> {
    /// Write a row of each of `configurations`: the row of its id, else a
    /// new row. Add a column for each target a configuration states that no
    /// column names. Regenerate the cells of each row whose configuration
    /// states another row than `decoded` ([`same_row`]), and write a cell of
    /// each added column into each other row; remove the other rows; write
    /// the rows in the order of the configurations' ordinals. Return the
    /// configuration of each written row, keyed by the configuration id it
    /// reads back as, the ids of the added columns, and the losses of what
    /// the rows cannot state.
    fn write<'c>(
        mut self,
        decoded: &[Configuration],
        configurations: &'c [Configuration],
    ) -> (
        BTreeMap<String, &'c Configuration>,
        BTreeSet<String>,
        Vec<LossNote>,
    ) {
        let (entry, table) = (self.entry, self.table);
        for configuration in configurations {
            self.grow(configuration);
        }
        let decoded = decoded
            .iter()
            .map(|configuration| (configuration.id.as_str(), configuration))
            .collect::<BTreeMap<_, _>>();
        let stored = table
            .rows
            .iter()
            .map(|row| (neutral_configuration_id(entry, &row.id).to_string(), row))
            .collect::<BTreeMap<_, _>>();
        let mut taken = table
            .rows
            .iter()
            .map(|row| row.id.clone())
            .collect::<BTreeSet<_>>();
        let mut hosted = Vec::new();
        for configuration in configurations {
            let row_id = match stored.get(configuration.id.as_str()) {
                Some(row) => {
                    let unchanged = decoded
                        .get(configuration.id.as_str())
                        .is_some_and(|decoded| same_row(configuration, decoded, self.bases));
                    self.cells(&row.id, Some(row), configuration, unchanged);
                    row.id.clone()
                }
                None => {
                    let row_id = row_id(entry, configuration.id.as_str())
                        .filter(|row| !row.is_empty() && !taken.contains(row))
                        .unwrap_or_else(|| {
                            deterministic_guid("configuration row", configuration.id.as_str())
                        });
                    taken.insert(row_id.clone());
                    let guid = |domain| deterministic_guid(domain, &row_id);
                    let row = new_row(
                        &row_id,
                        configuration.name.as_deref().unwrap_or_default(),
                        RowGuids {
                            asset: guid("configuration row asset"),
                            revision: guid("configuration row revision"),
                            configuration_revision: guid(
                                "configuration row configuration revision",
                            ),
                        },
                    );
                    push_row(edit(self.payload, self.source), table.position, row);
                    self.cells(&row_id, None, configuration, false);
                    row_id
                }
            };
            hosted.push((configuration, row_id));
        }
        let kept = hosted
            .iter()
            .map(|(_, row)| row.as_str())
            .collect::<BTreeSet<_>>();
        for row in &table.rows {
            if !kept.contains(row.id.as_str()) {
                remove_row(edit(self.payload, self.source), table.position, &row.id);
            }
        }
        hosted.sort_by(|(left, _), (right, _)| {
            (left.ordinal, &left.id).cmp(&(right.ordinal, &right.id))
        });
        let order = hosted
            .iter()
            .map(|(_, row)| row.as_str())
            .collect::<Vec<_>>();
        if !order
            .iter()
            .copied()
            .eq(table.rows.iter().map(|row| row.id.as_str()))
        {
            set_row_order(edit(self.payload, self.source), table.position, &order);
        }
        let mut losses = value_unwritable(
            "F3D",
            "writes no cell of the property column the write adds in these rows, whose configurations state no value of the property",
            std::mem::take(&mut self.unfilled),
        )
        .into_iter()
        .collect::<Vec<_>>();
        losses.extend(record_residual_unavailable(
            "F3D",
            "writes these property columns the write adds with a component id the writer generates: no property column of the table names the component, and f3d-open-items.md DR-1069 leaves the id of an added property column open",
            std::mem::take(&mut self.components),
        ));
        let expected = hosted
            .into_iter()
            .map(|(configuration, row)| {
                (
                    neutral_configuration_id(entry, &row).to_string(),
                    configuration,
                )
            })
            .collect();
        (expected, self.added, losses)
    }

    /// Write the name and the cells of `configuration` into the row
    /// `row_id`, which `stored` states; where the row is `unchanged`, only
    /// its cells of the added columns.
    fn cells(
        &mut self,
        row_id: &str,
        stored: Option<&ConfigurationRow>,
        configuration: &Configuration,
        unchanged: bool,
    ) {
        let name = configuration.name.as_deref().unwrap_or_default();
        if !unchanged && stored.is_some_and(|row| row.name != name) {
            set_row_name(
                edit(self.payload, self.source),
                self.table.position,
                row_id,
                name,
            );
        }
        for column in &self.columns {
            let added = self.added.contains(&column.id);
            if unchanged && !added {
                continue;
            }
            let value = stored
                .and_then(|row| row.cells.iter().find(|cell| cell.column_id == column.id))
                .map(|cell| &cell.value);
            let data = match cell(column, configuration, value, self.targets, self.bases) {
                Cell::Keep => {
                    if let (true, ColumnTarget::Property(key), None) =
                        (added, &column.target, value)
                    {
                        self.unfilled.push(format!("{key} in {}", configuration.id));
                    }
                    continue;
                }
                Cell::Write(data) => Some(data),
                Cell::Remove => None,
            };
            set_cell(
                edit(self.payload, self.source),
                self.table.position,
                row_id,
                &column.id,
                data.as_ref(),
            );
        }
    }

    /// Add a column for each parameter `configuration` overrides, for each
    /// feature whose state it sets otherwise than the feature's own where it
    /// is not the active configuration, and for its part number and its
    /// description, that no column names.
    fn grow(&mut self, configuration: &Configuration) {
        let targets = self.targets;
        let parameters = self
            .columns
            .iter()
            .filter_map(|column| column_parameter(&column.target, targets))
            .collect::<BTreeSet<_>>();
        let features = self
            .columns
            .iter()
            .filter_map(|column| match column.target {
                ColumnTarget::Feature(record) => Some(&targets.feature(record)?.id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let properties = self
            .columns
            .iter()
            .filter_map(|column| match &column.target {
                ColumnTarget::Property(key) => Some(key.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut added = Vec::new();
        for id in configuration
            .parameter_overrides
            .keys()
            .filter(|id| !parameters.contains(*id))
        {
            if let Some(target) = targets.parameter_column(id) {
                let title = self
                    .bases
                    .parameters
                    .get(id)
                    .map_or(id.as_str(), |parameter| parameter.name.as_str());
                added.push((id.to_string(), title.to_owned(), target));
            }
        }
        if !configuration.active {
            for (id, state) in &configuration.feature_states {
                let own = self
                    .bases
                    .features
                    .get(id)
                    .map(|base| base.evaluation.is_suppressed());
                if features.contains(id) || own == Some(state.evaluation.is_suppressed()) {
                    continue;
                }
                if let Some(target) = targets.feature_column(id) {
                    let title = targets
                        .neutral_features
                        .iter()
                        .find(|feature| feature.id == *id)
                        .and_then(|feature| feature.name.as_deref())
                        .unwrap_or(id.as_str());
                    added.push((id.to_string(), title.to_owned(), target));
                }
            }
        }
        for key in [PART_NUMBER, DESCRIPTION] {
            if !properties.contains(key)
                && configuration
                    .properties
                    .contains_key(format!("property:{key}").as_str())
            {
                added.push((
                    format!("property:{key}"),
                    key.to_owned(),
                    ColumnTarget::Property(key.to_owned()),
                ));
            }
        }
        for (key, title, target) in added {
            let id = deterministic_guid("configuration column", &key);
            if self.columns.iter().any(|column| column.id == id) {
                continue;
            }
            let pushed = match &target {
                ColumnTarget::Property(key) => {
                    let component = property_component(
                        self.payload.as_ref().unwrap_or(self.source),
                        self.table.position,
                    );
                    let component = component.unwrap_or_else(|| {
                        self.components.push(key.clone());
                        deterministic_guid("configuration property component", self.entry)
                    });
                    push_property_column(
                        edit(self.payload, self.source),
                        self.table.position,
                        &id,
                        key,
                        &component,
                    )
                }
                _ => push_column(
                    edit(self.payload, self.source),
                    self.table.position,
                    &id,
                    &title,
                    &target,
                ),
            };
            if pushed {
                self.added.insert(id.clone());
                self.columns.push(ConfigurationColumn { id, target });
            }
        }
    }
}

/// The key of the property column of the part number.
const PART_NUMBER: &str = "Part Number";
/// The key of the property column of the description.
const DESCRIPTION: &str = "Description";

/// The `testRowData` entry of `column` that states its value in `active`,
/// or in a configuration that states no override where there is no active
/// configuration (f3d.md §1.2): a parameter's name, expression and value in
/// database units, whether a feature is computed, or a property's text. A
/// part number states `Option` 0 with the row's name where its cell is
/// empty, else `Option` 1 with the text.
fn test_row(
    column: &ConfigurationColumn,
    active: Option<&Configuration>,
    targets: &ConfigurationTargets<'_>,
    bases: &Bases<'_>,
) -> Option<Map<String, Value>> {
    let mut entry = Map::new();
    let mut set = |member: &str, value: Value| {
        entry.insert(member.to_owned(), value);
    };
    match &column.target {
        ColumnTarget::Parameter(_) | ColumnTarget::OwnedParameter(_) => {
            let parameter = column_parameter(&column.target, targets)?;
            let base = bases.parameters.get(&parameter)?;
            let (expression, value) = match active {
                Some(active) => (
                    bases.expression(active, &parameter)?,
                    bases.value(active, &parameter)?,
                ),
                None => (&base.expression, base.value.as_ref()?),
            };
            let value = evaluated(&parameter, value, targets)?;
            set("DataType", Value::from("ParameterDoubleValue"));
            set("ColumnUniqueID", Value::from(column.id.as_str()));
            set("UserName", Value::from(base.name.as_str()));
            set("Expression", Value::from(expression.as_str()));
            set("Value", Value::from(value.get()));
        }
        ColumnTarget::Feature(record) => {
            let feature = &targets.feature(*record)?.id;
            let state = match active {
                Some(active) => bases.feature(active, feature)?,
                None => bases.features.get(feature)?,
            };
            set("DataType", Value::from("BoolValue"));
            set("ColumnUniqueID", Value::from(column.id.as_str()));
            set("Value", Value::from(!state.evaluation.is_suppressed()));
        }
        ColumnTarget::Property(key) => {
            let active = active?;
            let text = active.properties.get(format!("property:{key}").as_str())?;
            if key == PART_NUMBER {
                let (text, option) = match text.as_str() {
                    "" => (active.name.as_deref().unwrap_or_default(), 0),
                    text => (text, 1),
                };
                set("DataType", Value::from("StringOptionValue"));
                set("ColumnUniqueID", Value::from(column.id.as_str()));
                set("Value", Value::from(text));
                set("Option", Value::from(option));
            } else {
                set("DataType", Value::from("StringValue"));
                set("ColumnUniqueID", Value::from(column.id.as_str()));
                set("Value", Value::from(text.as_str()));
            }
        }
    }
    Some(entry)
}

/// The neutral parameter that a column of `target` names.
fn column_parameter(
    target: &ColumnTarget,
    targets: &ConfigurationTargets<'_>,
) -> Option<ParameterId> {
    match target {
        ColumnTarget::Parameter(record) => targets.parameter(*record, None),
        ColumnTarget::OwnedParameter(record) => targets.owned_parameter(*record),
        ColumnTarget::Property(_) | ColumnTarget::Feature(_) => None,
    }
}

/// What the write does with one cell of a row.
enum Cell {
    Keep,
    Write(CellData),
    Remove,
}

/// The cell of `column` that states what `configuration` resolves for the
/// column's target, where the row stores `stored`. A configuration that
/// states no override of a feature or a parameter takes its base in
/// `bases`. A cell the write cannot state is kept, and the read-back
/// charges it.
fn cell(
    column: &ConfigurationColumn,
    configuration: &Configuration,
    stored: Option<&ConfiguredValue>,
    targets: &ConfigurationTargets<'_>,
    bases: &Bases<'_>,
) -> Cell {
    let absent = if stored.is_some() {
        Cell::Remove
    } else {
        Cell::Keep
    };
    match &column.target {
        ColumnTarget::Property(key) => {
            match configuration
                .properties
                .get(format!("property:{key}").as_str())
            {
                None => absent,
                Some(text) if matches!(stored, Some(ConfiguredValue::Property { text: stored, .. }) if stored == text) => {
                    Cell::Keep
                }
                Some(text) => Cell::Write(CellData::Text(text.clone())),
            }
        }
        ColumnTarget::Feature(record) => {
            let computed = match targets.feature(*record) {
                Some(feature) => bases
                    .feature(configuration, &feature.id)
                    .map(|state| !state.evaluation.is_suppressed()),
                None => match configuration
                    .properties
                    .get(format!("suppressed:{}", column.id).as_str())
                    .map(String::as_str)
                {
                    None => None,
                    Some("true") => Some(false),
                    Some("false") => Some(true),
                    Some(_) => return Cell::Keep,
                },
            };
            match computed {
                None => absent,
                Some(computed) if matches!(stored, Some(ConfiguredValue::Feature { computed: stored, .. }) if *stored == computed) => {
                    Cell::Keep
                }
                Some(computed) => Cell::Write(CellData::Computed(computed)),
            }
        }
        ColumnTarget::Parameter(record) => expression_cell(
            column,
            targets.parameter(*record, None).as_ref(),
            configuration,
            stored,
            targets,
            bases,
        ),
        ColumnTarget::OwnedParameter(record) => expression_cell(
            column,
            targets.owned_parameter(*record).as_ref(),
            configuration,
            stored,
            targets,
            bases,
        ),
    }
}

/// The cell of a parameter column whose target is the neutral `parameter`,
/// or a column-local property when the target has no neutral parameter.
fn expression_cell(
    column: &ConfigurationColumn,
    parameter: Option<&ParameterId>,
    configuration: &Configuration,
    stored: Option<&ConfiguredValue>,
    targets: &ConfigurationTargets<'_>,
    bases: &Bases<'_>,
) -> Cell {
    let (expression, value) = match parameter {
        Some(parameter) => (
            bases.expression(configuration, parameter),
            bases.value(configuration, parameter),
        ),
        None => (
            configuration
                .properties
                .get(format!("parameter:{}", column.id).as_str()),
            None,
        ),
    };
    let Some(expression) = expression else {
        return if stored.is_some() {
            Cell::Remove
        } else {
            Cell::Keep
        };
    };
    if let Some(
        ConfiguredValue::Parameter {
            expression: stored,
            value: evaluated,
            ..
        }
        | ConfiguredValue::OwnedParameter {
            expression: stored,
            value: evaluated,
            ..
        },
    ) = stored
    {
        let projected =
            parameter.and_then(|parameter| targets.parameter_value(parameter, *evaluated));
        if stored == expression && projected.as_ref() == value {
            return Cell::Keep;
        }
    }
    match (parameter, value) {
        (Some(parameter), Some(value)) => match evaluated(parameter, value, targets) {
            Some(value) => Cell::Write(CellData::Expression {
                expression: expression.clone(),
                value,
            }),
            None => Cell::Keep,
        },
        _ => Cell::Keep,
    }
}

/// The evaluated value in database units that reads back as `value` of
/// `parameter`, else the nearest one: a length in centimetres, an angle in
/// radians, a real as it is. A value of another kind has none.
fn evaluated(
    parameter: &ParameterId,
    value: &ParameterValue,
    targets: &ConfigurationTargets<'_>,
) -> Option<FiniteReal> {
    let nearest = match value {
        ParameterValue::Length(length) => length.get() / DESIGN_LENGTH_TO_MILLIMETRE,
        ParameterValue::Angle(angle) => angle.get(),
        ParameterValue::Real(real) => real.get(),
        ParameterValue::Integer(_) | ParameterValue::Boolean(_) | ParameterValue::String(_) => {
            return None
        }
    };
    [nearest, nearest.next_up(), nearest.next_down()]
        .into_iter()
        .filter_map(FiniteReal::new)
        .find(|candidate| targets.parameter_value(parameter, *candidate).as_ref() == Some(value))
        .or_else(|| FiniteReal::new(nearest))
}

/// The row id whose configuration id under the table document `entry` is
/// `id`.
fn row_id(entry: &str, id: &str) -> Option<String> {
    let prefix = neutral_configuration_id(entry, "").to_string();
    let rest = id.strip_prefix(prefix.strip_suffix("0:")?)?;
    let (length, encoded) = rest.split_once(':')?;
    if length.parse::<usize>().ok()? != encoded.len() {
        return None;
    }
    let row = percent_decoded(encoded)?;
    (neutral_configuration_id(entry, &row).as_str() == id).then_some(row)
}

/// `text` with each `%XX` escape replaced by the byte it names.
fn percent_decoded(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

/// Point each rule document at the configuration that states its
/// condition: the first of `configurations` with the property
/// `activation_rule:` followed by the rule's entry name. A rule whose
/// condition a configuration of `decoded` states, and none of
/// `configurations`, activates nothing.
fn rules(
    documents: &[DesignConfiguration],
    payloads: &mut [Option<Map<String, Value>>],
    decoded: &[Configuration],
    configurations: &[Configuration],
) {
    for (document, payload) in documents.iter().zip(payloads) {
        let Some(rule) = document.rule() else {
            continue;
        };
        let key = format!("activation_rule:{}", document.entry_name());
        let holder = configurations.iter().find_map(|configuration| {
            Some((configuration, configuration.properties.get(key.as_str())?))
        });
        match holder {
            Some((configuration, condition)) => {
                let Some(name) = configuration.name.as_deref() else {
                    continue;
                };
                let stated = |member: &str| rule.get(member).and_then(Value::as_str);
                if stated("when") != Some(condition) || stated("activate") != Some(name) {
                    let payload = edit(payload, rule);
                    payload.insert("when".to_owned(), Value::from(condition.as_str()));
                    payload.insert("activate".to_owned(), Value::from(name));
                }
            }
            None if decoded
                .iter()
                .any(|configuration| configuration.properties.contains_key(key.as_str())) =>
            {
                let payload = edit(payload, rule);
                payload.remove("when");
                payload.remove("activate");
            }
            None => {}
        }
    }
}

/// `documents` with each payload of `payloads` in place of its own.
fn written(
    documents: &[DesignConfiguration],
    payloads: Vec<Option<Map<String, Value>>>,
) -> Result<Cow<'_, [DesignConfiguration]>, CodecError> {
    if payloads.iter().all(Option::is_none) {
        return Ok(Cow::Borrowed(documents));
    }
    documents
        .iter()
        .zip(payloads)
        .map(|(document, payload)| match payload {
            Some(payload) => document.with_payload(payload),
            None => Ok(document.clone()),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Cow::Owned)
}

/// The losses of the configurations that the written rows, `read`, state
/// otherwise than `expected`, the configuration of each row keyed by the
/// configuration id it reads back as. `bases` holds what a configuration
/// that states no override resolves to.
fn compare(
    expected: &BTreeMap<String, &Configuration>,
    read: &[Configuration],
    bases: &Bases<'_>,
) -> Vec<LossNote> {
    let read = read
        .iter()
        .map(|configuration| (configuration.id.as_str(), configuration))
        .collect::<BTreeMap<_, _>>();
    let mut generated = Vec::new();
    let mut ordinals = Vec::new();
    let mut names = Vec::new();
    let mut values = Vec::new();
    let mut dropped = Vec::new();
    for (written, configuration) in expected {
        let id = configuration.id.as_str();
        let Some(back) = read.get(written.as_str()) else {
            dropped.push(format!("{id}: row"));
            continue;
        };
        if written != id {
            generated.push(format!("{id} as {written}"));
        }
        if back.ordinal != configuration.ordinal {
            ordinals.push(format!("{id} at {}", back.ordinal));
        }
        if back.name != configuration.name {
            names.push(id.to_owned());
        }
        let evaluated = configuration
            .parameter_overrides
            .keys()
            .chain(back.parameter_overrides.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|parameter| {
                bases.expression(configuration, parameter) == bases.expression(back, parameter)
                    && bases.value(back, parameter) != bases.value(configuration, parameter)
            })
            .collect::<Vec<_>>();
        values.extend(
            evaluated
                .iter()
                .map(|parameter| format!("{id}: {parameter}")),
        );
        let evaluated = evaluated
            .iter()
            .map(|parameter| format!("parameter_values.{parameter}"))
            .collect::<BTreeSet<_>>();
        let fields = differences(configuration, back, bases)
            .into_iter()
            .filter(|difference| !evaluated.contains(&difference.field))
            .map(|difference| difference.to_string())
            .collect::<Vec<_>>();
        if !fields.is_empty() {
            dropped.push(format!("{id}: {}", fields.join("; ")));
        }
    }
    let mut losses = Vec::new();
    losses.extend(edit_dropped(
        EntityKind::Configuration,
        "F3D",
        "writes these configurations, whose written rows read back as configurations that differ in these fields: the configuration table holds no cell or row for the field, or its cell reads back otherwise",
        dropped,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes the rows of these configurations under a row id the writer generates: the configuration id is not the id of a new row of the configuration table document",
        generated,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these configurations at the position of their row in the table's rowOrder, which numbers the rows from zero",
        ordinals,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these configurations without a name under the empty row name: a row stores its name as text",
        names,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these configuration parameter values as the evaluated value of their cell, which reads back as another value",
        values,
    ));
    losses
}

#[cfg(test)]
mod tests;
