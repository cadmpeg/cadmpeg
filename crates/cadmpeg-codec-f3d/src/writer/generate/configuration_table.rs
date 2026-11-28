// SPDX-License-Identifier: Apache-2.0
//! The configuration table and rule documents of a generated Design stream.
//! Where the neutral configurations differ from the configurations that the
//! documents project, the neutral record wins (cadir-v11.md §3.4, W2; §5.14):
//! the write regenerates the rows of the table, their cells and order, and
//! the targets of the rules from the configurations (f3d.md §1.2). A
//! parameter or feature with a writable Design target can acquire a column.
//! Part numbers and descriptions use property columns. Cells state the values
//! each row resolves, and an empty table document can host the rows.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{edit_dropped, record_residual_unavailable, value_unwritable};
use cadmpeg_ir::design::configuration::PartNumber;
use cadmpeg_ir::features::parameters::{DesignConfiguration as Configuration, ParameterValue};
use cadmpeg_ir::features::{ConfigurationId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::units::Tolerances;
use cadmpeg_ir::CadIr;
use serde_json::{Map, Value};

pub(in crate::writer) mod read_back;

use super::configuration_root::{plan, Plan};
use super::presentation::deterministic_guid;
use crate::design::configurations::{table_at, ConfigurationTargets, Projection};
use crate::design::feature_project::DESIGN_LENGTH_TO_MILLIMETRE;
use crate::design_v11::configurations::{agree, restate, ActiveRows, RestatedRows};
use crate::ids::neutral_configuration_id;
use crate::native::F3dNative;
use crate::records::configuration::table::{
    bind_column_target, new_document, new_row, property_component, push_column,
    push_property_column, push_row, remove_row, set_cell, set_row_name, set_row_order,
    write_test_rows, AdmittedTables, CellData, ColumnTarget, ConfigurationColumn, ConfigurationRow,
    ConfigurationTable, ConfiguredValue, RowGuids, TableNames, TestRowTarget,
};
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::writer::primitives::{
    bases, configuration_targets, decoded_configurations, differences, projection, projects,
    same_row, Bases, Difference,
};

/// The configuration documents the write holds, and what they read back as.
pub(super) struct Written<'a, 'f> {
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
    expected: BTreeMap<String, &'f Configuration>,
    /// The ids of the columns the write adds to the table of the rows.
    added: BTreeSet<String>,
    bindings: BTreeMap<String, ParameterId>,
    /// The configuration of the row the stored root record names active.
    stored_active: Option<Configuration>,
    test_rows: Option<(usize, BTreeMap<String, TestRowTarget>)>,
}

/// The `tableName` and the `tableType` of a table the write creates in an
/// empty table document (f3d-open-items.md DR-1069).
const NEW_TABLE_NAME: &str = "Configurations";
const NEW_TABLE_TYPE: &str = "Configurations";

/// Write `documents` with the rows, cells, row order and rule targets of
/// `configurations`, where they differ from `decoded`, the configurations
/// that `documents` project. Return the written documents, and the losses
/// of the configurations that no table holds a row for.
pub(super) fn regenerate<'a, 'f>(
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    configurations: &'f [Configuration],
    targets: &ConfigurationTargets<'_>,
) -> Result<(Written<'a, 'f>, Vec<LossNote>), CodecError> {
    regenerate_with_bindings(documents, decoded, configurations, targets, None)
}

fn regenerate_with_bindings<'a, 'f>(
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    configurations: &'f [Configuration],
    targets: &ConfigurationTargets<'_>,
    destinations: Option<&DestinationParameters<'_>>,
) -> Result<(Written<'a, 'f>, Vec<LossNote>), CodecError> {
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
    let (expected, added, bindings) = match host {
        Some((index, source, table)) => {
            let WrittenRows {
                expected,
                added,
                bindings,
                losses: written,
            } = Rows {
                entry: documents[index].entry_name(),
                source,
                payload: &mut payloads[index],
                table,
                columns: table.columns.clone(),
                bindings: table
                    .columns
                    .iter()
                    .filter_map(|column| {
                        Some((
                            column.id.clone(),
                            column_parameter(&column.target, targets)?,
                        ))
                    })
                    .collect(),
                destinations,
                added: BTreeSet::new(),
                targets,
                bases: &bases,
                components: Vec::new(),
            }
            .write(&decoded.configurations, configurations);
            losses.extend(written);
            (expected, added, bindings)
        }
        None => {
            losses.extend(edit_dropped(
                EntityKind::Configuration,
                "F3D",
                "writes no row for these configurations: the configuration table documents hold no table for their rows, or more than one",
                configurations.iter().map(|configuration| configuration.id.to_string()),
            ));
            (BTreeMap::new(), BTreeSet::new(), BTreeMap::new())
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
            bindings,
            stored_active,
            test_rows: None,
        },
        losses,
    ))
}

impl Written<'_, '_> {
    #[cfg(test)]
    pub(super) fn compare_in_memory(
        &self,
        active: Option<&str>,
        targets: &ConfigurationTargets<'_>,
        producers: &BTreeSet<cadmpeg_ir::features::FeatureId>,
    ) -> Result<Vec<LossNote>, CodecError> {
        let mut read = self.projection.clone();
        for row in &mut read.configurations {
            row.active = self
                .expected
                .get(row.id.as_str())
                .is_some_and(|expected| Some(expected.id.as_str()) == active);
        }
        let mut features = targets.neutral_features.to_vec();
        let undecided = crate::design::configurations::undecided_suppression(
            &features,
            targets.scopes,
            producers,
        );
        crate::design::configurations::reconcile_active_configurations(
            &mut features,
            &mut read.configurations,
            &undecided,
        )?;
        let bases = bases(targets.neutral_features, targets.neutral_parameters);
        let generated = self.generated.keys().map(String::as_str).collect();
        let expected = self
            .expected
            .iter()
            .map(|(id, row)| (id.as_str(), *row))
            .collect();
        let mut losses = compare(&expected, &read.configurations, &bases, &generated);
        losses.extend(Self::compare_projected_features(
            self.stored_active.as_ref(),
            &read,
            targets,
            &features,
        ));
        Ok(losses)
    }

    fn compare_projected_features(
        stored_active: Option<&Configuration>,
        read: &Projection,
        targets: &ConfigurationTargets<'_>,
        features: &[cadmpeg_ir::features::Feature],
    ) -> Vec<LossNote> {
        let mut losses = Vec::new();
        losses.extend(value_unwritable(
            "F3D",
            "writes these features in the current state that the active row of the written configuration table gives them: the Design stream stores the state of its active row",
            targets
                .neutral_features
                .iter()
                .filter_map(|stated| {
                    let read = features.iter().find(|read| read.id == stated.id)?;
                    let (stated_state, read_state) = (
                        stated.configuration_state().evaluation,
                        read.configuration_state().evaluation,
                    );
                    (stated_state.is_suppressed() != read_state.is_suppressed()).then(|| {
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
                && stored_active
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
        losses
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
    pub(super) part_numbers: BTreeMap<ConfigurationId, PartNumber>,
    expressions: BTreeMap<(ConfigurationId, ParameterId), cadmpeg_ir::design::expr::Expression>,
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
        mut active,
        stated,
        mut kept,
    } = restate(
        rows,
        decoded,
        target,
        targets.neutral_features,
        targets.neutral_parameters,
        which,
    )
    .into_rows(losses);
    match which {
        ActiveRows::Stored => {
            if held {
                let legacy = rows
                    .iter()
                    .filter(|row| row.active)
                    .map(|row| &row.id)
                    .collect::<BTreeSet<_>>();
                if legacy.is_empty() && !active.is_empty() {
                    kept.insert(None);
                }
                kept.extend(
                    legacy
                        .difference(&active.iter().collect())
                        .map(|id| Some((*id).clone())),
                );
            }
        }
        ActiveRows::Design => {
            let chosen = restated
                .iter()
                .find(|row| active.contains(&row.id))
                .or_else(|| restated.iter().find(|row| row.active))
                .map(|row| row.id.clone());
            active = chosen.into_iter().collect();
        }
    }
    let rewritten = match which {
        ActiveRows::Design => rewritten(targets, &target.tolerances),
        ActiveRows::Stored => rebased.clone(),
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
                .contains(&(row.id.clone(), parameter.clone()))
                || (matches!(which, ActiveRows::Design)
                    && !stays
                    && !target
                        .design
                        .configurations
                        .iter()
                        .any(|record| record.id == row.id));
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
    if matches!(which, ActiveRows::Stored) {
        for row in &mut restated {
            row.active = active.contains(&row.id);
        }
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
        kept: kept
            .into_iter()
            .map(|requested| {
                let current = active.iter().map(ToString::to_string).collect::<Vec<_>>();
                let current = if current.is_empty() {
                    "no configuration".to_owned()
                } else {
                    current.join(", ")
                };
                let requested =
                    requested.map_or("no configuration".to_owned(), |id| id.to_string());
                format!("{current} active in place of {requested}")
            })
            .collect(),
        part_numbers: stated.part_numbers,
        expressions: stated.expressions,
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
    fn by_id(rows: &[Configuration]) -> BTreeMap<&ConfigurationId, &Configuration> {
        rows.iter().map(|row| (&row.id, row)).collect()
    }
    let bases = bases(targets.neutral_features, targets.neutral_parameters);
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

/// Allocated record index and stored name, keyed by the authored parameter.
pub(super) type DestinationParameters<'a> = BTreeMap<ParameterId, DestinationParameter<'a>>;

pub(super) struct DestinationParameter<'a> {
    pub(super) index: u32,
    pub(super) owner: Option<u32>,
    pub(super) name: &'a str,
    pub(super) expression: &'a str,
    pub(super) evaluated: f64,
}

impl Stated {
    pub(super) fn place_expressions(
        &mut self,
        target: &CadIr,
        parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
        destinations: &DestinationParameters<'_>,
        losses: &mut Vec<LossNote>,
    ) {
        use cadmpeg_ir::design::expr::ExpressionToken;

        let mut source = std::collections::HashMap::<&str, Option<&ParameterId>>::new();
        let mut names = |name, id| {
            source
                .entry(name)
                .and_modify(|previous| {
                    if *previous != Some(id) {
                        *previous = None;
                    }
                })
                .or_insert(Some(id));
        };
        for parameter in &target.design.parameters {
            if let Some(name) = &parameter.name {
                names(name.as_str(), &parameter.id);
            }
        }
        for parameter in parameters {
            names(parameter.name.as_str(), &parameter.id);
        }
        let vocabulary = source
            .into_iter()
            .filter_map(|(name, id)| Some((name, id?)))
            .collect();
        let names = destinations
            .iter()
            .map(|(id, destination)| (id, destination.name))
            .collect();
        let name = |id: &ParameterId| destinations.get(id).map(|destination| destination.name);
        let mut kinds = parameters
            .iter()
            .filter_map(|parameter| {
                Some((
                    &parameter.id,
                    crate::design_v11::parameters::cache(parameter.value.as_ref()?).0,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        kinds.extend(
            target
                .design
                .parameters
                .iter()
                .map(|parameter| (&parameter.id, parameter.kind.clone())),
        );
        let mut unavailable = Vec::new();
        for row in &mut self.rows {
            for (id, expression) in &mut row.parameter_overrides {
                let placed = match self.expressions.get(&(row.id.clone(), id.clone())) {
                    Some(typed) => crate::design_v11::configurations::spelled(typed, &names),
                    None => {
                        if kinds.get(id).is_some_and(|kind| {
                            crate::design_v11::parameters::literal(expression, kind).is_some()
                        }) {
                            continue;
                        }
                        let tokens = crate::design_v11::parameters::tokens(expression, &vocabulary);
                        if !tokens
                            .iter()
                            .any(|token| matches!(token, ExpressionToken::Reference(_)))
                        {
                            continue;
                        }
                        super::design::expression::print_tokens(&tokens, &name)
                    }
                };
                if let Some(placed) = placed {
                    *expression = placed;
                } else {
                    unavailable.push(format!("{} parameter {id}: {expression:?}", row.id));
                }
            }
        }
        losses.extend(value_unwritable(
            "F3D",
            "cannot bind references in these configuration expressions to written Design parameters",
            unavailable,
        ));
    }
}

fn pin_losses(pinned: &[Pinned]) -> Option<LossNote> {
    value_unwritable(
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
    )
}

/// Edits of the retained active row which need a change to the Design base.
pub(in crate::writer) fn active_parameter_edits(
    target: &CadIr,
    native: &F3dNative,
) -> Result<BTreeMap<ParameterId, (String, Option<ParameterValue>)>, CodecError> {
    let targets = configuration_targets(target, native);
    let decoded = decoded_configurations(&native.design_configurations, &targets)?;
    let stated = stated_rows(
        target,
        &decoded.configurations,
        &targets,
        &BTreeSet::new(),
        ActiveRows::Stored,
        &mut Vec::new(),
    );
    Ok(stated
        .pinned
        .into_iter()
        .map(|pin| (pin.parameter, pin.stated))
        .collect())
}

/// The configuration documents a patch of the retained archive carries, and
/// the losses of what the configurations state otherwise than the rows of
/// the documents.
pub(in crate::writer) struct Patched<'a> {
    pub(in crate::writer) destination: read_back::Destination<'a>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

/// Rewrite the configuration documents against the patched Design parameters.
/// The retained root names the active row.
pub(in crate::writer) fn patched<'a>(
    target: &CadIr,
    native: &'a F3dNative,
    own: &[cadmpeg_ir::features::parameters::DesignParameter],
    written_parameters: &BTreeSet<ParameterId>,
) -> Result<Patched<'a>, CodecError> {
    let mut targets = configuration_targets(target, native);
    let documents = &native.design_configurations;
    let decoded = decoded_configurations(documents, &targets)?;
    targets.neutral_parameters = own;
    let mut losses = Vec::new();
    let stated = stated_rows(
        target,
        &decoded.configurations,
        &targets,
        written_parameters,
        ActiveRows::Stored,
        &mut losses,
    );
    losses.extend(value_unwritable(
        "F3D",
        "keeps the archive's active configuration state in place of the requested state: the DesignConfigurationRoot names the row whose state the Design stream stores (f3d.md §1.2), and the patch keeps that selection",
        stated.kept.iter().cloned(),
    ));
    let unchanged = stated.pinned.is_empty()
        && stated.rewritten.is_empty()
        && projects(&stated.rows, documents, &targets)?;
    let (mut written, regenerated) = regenerate(documents, &decoded, &stated.rows, &targets)?;
    losses.extend(regenerated);
    let active = decoded
        .configurations
        .iter()
        .find(|configuration| configuration.active)
        .map(|configuration| configuration.id.as_str());
    if !unchanged {
        written.finish(active, &targets, &stated, None, &mut losses)?;
    }
    Ok(Patched {
        destination: read_back::Destination::retained(written, stated.part_numbers),
        losses,
    })
}

/// The configuration documents of a generated Design stream: `documents`,
/// whose rows are `decoded`, written to state `stated`, with the root record
/// `target` names, and the `testRowData` entries of its active row. Adds to
/// `losses` what the documents read back as otherwise.
pub(super) fn generated<'a, 'f>(
    target: &CadIr,
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    stated: &'f Stated,
    targets: &ConfigurationTargets<'_>,
    destinations: Option<&DestinationParameters<'_>>,
    losses: &mut Vec<LossNote>,
) -> Result<(Written<'a, 'f>, Plan), CodecError> {
    let (mut written, regenerated) =
        regenerate_with_bindings(documents, decoded, &stated.rows, targets, destinations)?;
    losses.extend(regenerated);
    let plan = plan(target, documents, decoded.table, &written, &stated.rows);
    written.finish(plan.active(), targets, stated, destinations, losses)?;
    Ok((written, plan))
}

impl Written<'_, '_> {
    pub(super) fn bind_features(
        &mut self,
        targets: &ConfigurationTargets<'_>,
        destinations: &BTreeMap<cadmpeg_ir::features::FeatureId, u32>,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        let occupied = destinations.values().copied().collect::<BTreeSet<_>>();
        let mut missing = Vec::new();
        for document in self.documents.to_mut() {
            let mut payload = None;
            for table in document.tables() {
                for column in &table.columns {
                    let ColumnTarget::Feature(record) = column.target else {
                        continue;
                    };
                    let id = targets.feature(record).map(|feature| &feature.id);
                    let replacement = match (id, id.and_then(|id| destinations.get(id))) {
                        (_, Some(&destination)) => destination,
                        (Some(id), None) => {
                            missing.push(format!(
                                "{} column {}: {id}",
                                document.entry_name(),
                                column.id,
                            ));
                            u32::MAX
                        }
                        (None, None) if occupied.contains(&record) => u32::MAX,
                        (None, None) => continue,
                    };
                    if record != replacement
                        && !bind_column_target(
                            edit(&mut payload, document.payload()),
                            table.position,
                            &column.id,
                            &ColumnTarget::Feature(replacement),
                        )
                    {
                        missing.push(format!(
                            "{} column {}: destination scope {replacement}",
                            document.entry_name(),
                            column.id
                        ));
                    }
                }
            }
            if let Some(payload) = payload {
                *document = document.with_payload(payload)?;
            }
        }
        losses.extend(value_unwritable("F3D", "cannot bind these configuration columns to their Design feature scopes in the destination", missing));
        Ok(())
    }

    /// Finish the table's test entries and destination Design bindings.
    fn finish(
        &mut self,
        active: Option<&str>,
        targets: &ConfigurationTargets<'_>,
        stated: &Stated,
        destinations: Option<&DestinationParameters<'_>>,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if let Some(test_rows) =
            self.test_rows(active, targets, destinations, &stated.rewritten, losses)
        {
            if let Some(payload) = test_rows.payload {
                let document = self.documents[test_rows.index].with_payload(payload)?;
                self.documents.to_mut()[test_rows.index] = document;
            }
            self.test_rows = Some((test_rows.index, test_rows.target));
        }
        losses.extend(pin_losses(&stated.pinned));
        if let Some(destinations) = destinations {
            self.bind_columns(targets, destinations, losses)?;
        }
        Ok(())
    }

    fn bind_columns(
        &mut self,
        targets: &ConfigurationTargets<'_>,
        destinations: &DestinationParameters<'_>,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        let timeline = targets.bindings.is_some();
        let projected = host(&self.documents, self.projection.table)
            .map(|(index, table)| (index, table.position));
        let mut occupied = destinations
            .values()
            .map(|destination| destination.index)
            .collect::<BTreeSet<_>>();
        let mut missing = Vec::new();
        let mut updated = Vec::new();
        for (index, document) in self.documents.iter().enumerate() {
            let mut payload = None;
            for table in document.tables() {
                let host = projected == Some((index, table.position));
                if !host && !timeline {
                    continue;
                }
                for column in &table.columns {
                    let (ColumnTarget::Parameter(record) | ColumnTarget::OwnedParameter(record)) =
                        column.target
                    else {
                        continue;
                    };
                    let source = if host {
                        self.bindings.get(&column.id).cloned()
                    } else {
                        column_parameter(&column.target, targets)
                    };
                    let destination = source.as_ref().and_then(|id| destinations.get(id));
                    let replacement = if let Some(destination) = destination {
                        destination.owner.map_or(
                            ColumnTarget::Parameter(destination.index),
                            ColumnTarget::OwnedParameter,
                        )
                    } else {
                        if let Some(id) = &source {
                            missing.push(format!(
                                "{} column {}: {id}",
                                document.entry_name(),
                                column.id
                            ));
                        }
                        let unbound = if timeline && source.is_some() {
                            u32::MAX
                        } else {
                            if !matches!(column.target, ColumnTarget::Parameter(_))
                                || !occupied.contains(&record)
                            {
                                continue;
                            }
                            let Some(unbound) =
                                (0..=u32::MAX).find(|record| !occupied.contains(record))
                            else {
                                continue;
                            };
                            occupied.insert(unbound);
                            unbound
                        };
                        ColumnTarget::Parameter(unbound)
                    };
                    if column.target != replacement
                        && !bind_column_target(
                            edit(&mut payload, document.payload()),
                            table.position,
                            &column.id,
                            &replacement,
                        )
                    {
                        missing.push(format!(
                            "{} column {}: destination record {replacement:?}",
                            document.entry_name(),
                            column.id
                        ));
                    }
                }
            }
            if let Some(payload) = payload {
                updated.push((index, document.with_payload(payload)?));
            }
        }
        for (index, document) in updated {
            self.documents.to_mut()[index] = document;
        }
        losses.extend(value_unwritable(
            "F3D",
            "cannot bind these configuration columns to their Design records in the destination",
            missing,
        ));
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
        destinations: Option<&DestinationParameters<'_>>,
        rewritten: &BTreeSet<ParameterId>,
        losses: &mut Vec<LossNote>,
    ) -> Option<PlannedTestRows> {
        let (index, table) = host(&self.documents, self.projection.table)?;
        let bases = bases(targets.neutral_features, targets.neutral_parameters);
        let placed = active.and_then(|id| {
            self.expected
                .iter()
                .find(|(_, configuration)| configuration.id.as_str() == id)
        });
        let configuration = placed.map(|(_, configuration)| *configuration);
        let stored = self.stored_active.as_ref();
        let moved = active != stored.map(|stored| stored.id.as_str());
        let entry_name = self.documents[index].entry_name();
        let stored_names = targets
            .parameters
            .iter()
            .map(|record| (crate::ids::neutral_parameter_id(record), record.name()))
            .collect::<BTreeMap<_, _>>();
        let selected = placed.and_then(|(placed, _)| {
            table
                .rows
                .iter()
                .find(|row| neutral_configuration_id(entry_name, &row.id).as_str() == placed)
        });
        let expected = table
            .columns
            .iter()
            .filter_map(|column| {
                let parameter = self.bindings.get(&column.id);
                let destination = parameter.and_then(|parameter| destinations?.get(parameter));
                let selected_cell = selected
                    .and_then(|row| row.cells.iter().find(|cell| cell.column_id == column.id));
                let pair = selected_cell.and_then(|cell| match &cell.value {
                    ConfiguredValue::Parameter {
                        expression, value, ..
                    }
                    | ConfiguredValue::OwnedParameter {
                        expression, value, ..
                    } => Some((expression.as_str(), value.get())),
                    _ => None,
                });
                let existing_only = parameter.is_none() && pair.is_some();
                let members = if existing_only {
                    pair.map(|(expression, value)| {
                        Map::from_iter([
                            ("DataType".to_owned(), Value::from("ParameterDoubleValue")),
                            ("Expression".to_owned(), Value::from(expression)),
                            ("Value".to_owned(), Value::from(value)),
                        ])
                    })
                } else {
                    test_row(
                        column,
                        parameter,
                        configuration,
                        targets,
                        &bases,
                        destination,
                        pair,
                    )
                };
                let added = self.added.contains(&column.id);
                let rewritten = parameter.is_some_and(|parameter| {
                    rewritten.contains(parameter)
                        || destination
                            .zip(stored_names.get(parameter))
                            .is_some_and(|(destination, stored)| destination.name != *stored)
                });
                let restated = moved
                    || added
                    || rewritten
                    || (!existing_only
                        && members
                            != test_row(column, parameter, stored, targets, &bases, None, None));
                restated.then_some((
                    column.id.clone(),
                    TestRowTarget {
                        members,
                        required: !existing_only && (added || rewritten || stored.is_some()),
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
        Some(PlannedTestRows {
            index,
            payload: (result.added || result.rewritten).then_some(payload),
            target: expected,
        })
    }
}

struct PlannedTestRows {
    index: usize,
    payload: Option<Map<String, Value>>,
    target: BTreeMap<String, TestRowTarget>,
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
    bindings: BTreeMap<String, ParameterId>,
    destinations: Option<&'s DestinationParameters<'s>>,
    /// The ids of the columns the write adds.
    added: BTreeSet<String>,
    targets: &'s ConfigurationTargets<'s>,
    /// What a configuration that states no override resolves to.
    bases: &'s Bases<'s>,
    /// Each property column the write adds under a component id it
    /// generates.
    components: Vec<String>,
}

struct WrittenRows<'a> {
    expected: BTreeMap<String, &'a Configuration>,
    added: BTreeSet<String>,
    bindings: BTreeMap<String, ParameterId>,
    losses: Vec<LossNote>,
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
    ) -> WrittenRows<'c> {
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
        let losses = record_residual_unavailable(
            "F3D",
            "writes these property columns the write adds with a component id the writer generates: no property column of the table names the component, and f3d-open-items.md DR-1069 leaves the id of an added property column open",
            std::mem::take(&mut self.components),
        ).into_iter().collect();
        let expected = hosted
            .into_iter()
            .map(|(configuration, row)| {
                (
                    neutral_configuration_id(entry, &row).to_string(),
                    configuration,
                )
            })
            .collect();
        WrittenRows {
            expected,
            added: self.added,
            bindings: self.bindings,
            losses,
        }
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
            let data = match cell(
                column,
                self.bindings.get(&column.id),
                configuration,
                value,
                self.targets,
                self.bases,
                self.bindings
                    .get(&column.id)
                    .and_then(|id| self.destinations?.get(id)),
            ) {
                Cell::Keep | Cell::Unwritable => continue,
                Cell::Write(data) => Some(data),
                Cell::Absent if value.is_some() => None,
                Cell::Absent => continue,
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
        let parameters = self.bindings.values().collect::<BTreeSet<_>>();
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
            let physical = self
                .destinations
                .and_then(|destinations| destinations.get(id));
            let target = match (self.destinations, physical) {
                (Some(_), Some(parameter)) => Some(ColumnTarget::Parameter(parameter.index)),
                (Some(_), None) => None,
                (None, _) => targets.parameter_column(id),
            };
            if let Some(target) = target {
                let title = physical.map_or_else(
                    || {
                        self.bases
                            .parameters
                            .get(id)
                            .map_or(id.as_str(), |parameter| parameter.name.as_str())
                    },
                    |parameter| parameter.name,
                );
                added.push((id.to_string(), title.to_owned(), target, Some(id.clone())));
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
                    added.push((id.to_string(), title.to_owned(), target, None));
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
                    None,
                ));
            }
        }
        for (key, title, target, parameter) in added {
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
                if let Some(parameter) = parameter {
                    self.bindings.insert(id.clone(), parameter);
                }
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
    parameter: Option<&ParameterId>,
    active: Option<&Configuration>,
    targets: &ConfigurationTargets<'_>,
    bases: &Bases<'_>,
    destination: Option<&DestinationParameter<'_>>,
    pair: Option<(&str, f64)>,
) -> Option<Map<String, Value>> {
    let mut entry = Map::new();
    let mut set = |member: &str, value: Value| {
        entry.insert(member.to_owned(), value);
    };
    match &column.target {
        ColumnTarget::Parameter(_) | ColumnTarget::OwnedParameter(_) => {
            let parameter = parameter?;
            let own = bases.parameters.get(parameter);
            let name = destination
                .map(|destination| destination.name)
                .or_else(|| own.map(|base| base.name.as_str()))?;
            let (expression, value) = match (pair, active) {
                (Some(pair), _) => pair,
                (None, Some(row)) if row.parameter_overrides.contains_key(parameter) => (
                    row.parameter_overrides.get(parameter)?.as_str(),
                    evaluated(parameter, row.parameter_values.get(parameter)?, targets)?.get(),
                ),
                (None, _) => match destination {
                    Some(destination) => (destination.expression, destination.evaluated),
                    None => {
                        let base = own?;
                        (
                            base.expression.as_str(),
                            evaluated(parameter, base.value.as_ref()?, targets)?.get(),
                        )
                    }
                },
            };
            set("DataType", Value::from("ParameterDoubleValue"));
            set("ColumnUniqueID", Value::from(column.id.as_str()));
            set("UserName", Value::from(name));
            set("Expression", Value::from(expression));
            set("Value", Value::from(value));
        }
        ColumnTarget::Feature(record) => {
            let computed = match active {
                Some(active) => computed(column, *record, active, targets, bases)?,
                None => !bases
                    .features
                    .get(&targets.feature(*record)?.id)?
                    .evaluation
                    .is_suppressed(),
            };
            set("DataType", Value::from("BoolValue"));
            set("ColumnUniqueID", Value::from(column.id.as_str()));
            set("Value", Value::from(computed));
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
    Absent,
    Unwritable,
    Write(CellData),
}

/// The cell of `column` that states what `configuration` resolves for the
/// column's target, where the row stores `stored`. A configuration that
/// states no override of a feature or a parameter takes its base in
/// `bases`. A cell the write cannot state is kept, and the read-back
/// charges it.
fn cell(
    column: &ConfigurationColumn,
    parameter: Option<&ParameterId>,
    configuration: &Configuration,
    stored: Option<&ConfiguredValue>,
    targets: &ConfigurationTargets<'_>,
    bases: &Bases<'_>,
    destination: Option<&DestinationParameter<'_>>,
) -> Cell {
    match &column.target {
        ColumnTarget::Property(key) => {
            match configuration
                .properties
                .get(format!("property:{key}").as_str())
            {
                None => Cell::Absent,
                Some(text) if matches!(stored, Some(ConfiguredValue::Property { text: stored, .. }) if stored == text) => {
                    Cell::Keep
                }
                Some(text) => Cell::Write(CellData::Text(text.clone())),
            }
        }
        ColumnTarget::Feature(record) => {
            let computed = computed(column, *record, configuration, targets, bases);
            if computed.is_none()
                && configuration
                    .properties
                    .contains_key(format!("suppressed:{}", column.id).as_str())
            {
                return Cell::Unwritable;
            }
            match computed {
                None => Cell::Absent,
                Some(computed) if matches!(stored, Some(ConfiguredValue::Feature { computed: stored, .. }) if *stored == computed) => {
                    Cell::Keep
                }
                Some(computed) => Cell::Write(CellData::Computed(computed)),
            }
        }
        ColumnTarget::Parameter(_) | ColumnTarget::OwnedParameter(_) => expression_cell(
            column,
            parameter,
            configuration,
            stored,
            targets,
            bases,
            destination,
        ),
    }
}

/// Whether the configured feature is computed, including a column-local value.
fn computed(
    column: &ConfigurationColumn,
    record: u32,
    row: &Configuration,
    targets: &ConfigurationTargets<'_>,
    bases: &Bases<'_>,
) -> Option<bool> {
    if let Some(feature) = targets.feature(record) {
        return bases
            .feature(row, &feature.id)
            .map(|state| !state.evaluation.is_suppressed());
    }
    match row
        .properties
        .get(format!("suppressed:{}", column.id).as_str())?
        .as_str()
    {
        "true" => Some(false),
        "false" => Some(true),
        _ => None,
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
    destination: Option<&DestinationParameter<'_>>,
) -> Cell {
    if let Some(destination) = destination
        .filter(|_| parameter.is_some_and(|id| !configuration.parameter_overrides.contains_key(id)))
    {
        let Some(value) = FiniteReal::new(destination.evaluated) else {
            return Cell::Unwritable;
        };
        return match stored {
            Some(
                ConfiguredValue::Parameter {
                    expression,
                    value: cache,
                    ..
                }
                | ConfiguredValue::OwnedParameter {
                    expression,
                    value: cache,
                    ..
                },
            ) if expression == destination.expression && *cache == value => Cell::Keep,
            _ => Cell::Write(CellData::Expression {
                expression: destination.expression.to_owned(),
                value,
            }),
        };
    }
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
        return Cell::Absent;
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
            None => Cell::Unwritable,
        },
        (Some(_), None) => Cell::Unwritable,
        (None, _) => Cell::Unwritable,
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
    expected: &BTreeMap<&str, &Configuration>,
    read: &[Configuration],
    bases: &Bases<'_>,
    assigned_ids: &BTreeSet<&str>,
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
    let mut unresolved = Vec::new();
    for (written, configuration) in expected {
        let id = configuration.id.as_str();
        let Some(back) = read.get(written) else {
            dropped.push(format!("{id}: row"));
            continue;
        };
        if assigned_ids.contains(written) {
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
            .filter_map(|difference| {
                if evaluated.contains(&difference.field) {
                    return None;
                }
                if difference.field.starts_with("properties.parameter:")
                    && difference.stated.is_some()
                {
                    unresolved.push(format!("{id}: {difference}"));
                    return None;
                }
                Some(difference.to_string())
            })
            .collect::<Vec<_>>();
        if !fields.is_empty() {
            dropped.push(format!("{id}: {}", fields.join("; ")));
        }
    }
    let mut losses = Vec::new();
    losses.extend(value_unwritable(
        "F3D",
        "cannot resolve the Design bindings of these configuration cells",
        unresolved,
    ));
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
