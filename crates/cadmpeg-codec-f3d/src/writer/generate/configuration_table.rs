// SPDX-License-Identifier: Apache-2.0
//! The configuration table and rule documents of a generated Design stream.
//! Where the neutral configurations differ from the configurations that the
//! documents project, the neutral record wins (cadir-v11.md §3.4, W2; §5.14):
//! the write regenerates the rows of the table, their cells and order, and
//! the targets of the rules from the configurations (f3d.md §1.2).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{edit_dropped, value_unwritable};
use cadmpeg_ir::features::parameters::{DesignConfiguration as Configuration, ParameterValue};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::schema::EntityKind;
use serde_json::{Map, Value};

use super::presentation::deterministic_guid;
use crate::design::configurations::{
    project, reconcile_active_configurations, table_at, ConfigurationTargets, Projection,
};
use crate::design::feature_project::DESIGN_LENGTH_TO_MILLIMETRE;
use crate::ids::neutral_configuration_id;
use crate::records::configuration::table::{
    new_row, push_row, remove_row, set_cell, set_row_name, set_row_order, CellData, ColumnTarget,
    ConfigurationColumn, ConfigurationRow, ConfigurationTable, ConfiguredValue, RowGuids,
};
use crate::records::configuration::DesignConfiguration;
use crate::writer::primitives::{
    unheld, Cells, Difference, SourceConfigurations, StoredStates, INFERRED,
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
}

/// Write `documents` with the rows, cells, row order and rule targets of
/// `configurations`, where they differ from `decoded`, the configurations
/// that `documents` project. Return the written documents and the losses of
/// what they read back as otherwise than `configurations`; `source` is the
/// source of the write.
pub(super) fn regenerate<'a>(
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    configurations: &[Configuration],
    targets: &ConfigurationTargets<'_>,
    source: &SourceConfigurations<'_>,
) -> Result<(Written<'a>, Vec<LossNote>), CodecError> {
    let mut payloads = vec![None; documents.len()];
    let mut losses = Vec::new();
    let host = host(documents, decoded.table);
    let expected = match host {
        Some((index, table)) => Rows {
            entry: documents[index].entry_name(),
            source: documents[index].payload(),
            payload: &mut payloads[index],
            table,
            targets,
            stored: &StoredStates::of(documents, Some((index, table.position)), targets),
            baseline: source,
        }
        .write(&decoded.configurations, configurations),
        None => {
            losses.extend(edit_dropped(
                EntityKind::Configuration,
                "F3D",
                "writes no row for these configurations: the configuration table documents hold no table for their rows, or more than one",
                configurations.iter().map(|configuration| configuration.id.to_string()),
            ));
            BTreeMap::new()
        }
    };
    rules(
        documents,
        &mut payloads,
        &decoded.configurations,
        configurations,
    );
    let documents = written(documents, payloads)?;
    let projection = match project(&documents, targets) {
        Ok(projection) => projection,
        Err(error) if crate::design::decode::record::is_fatal(&error) => return Err(error),
        Err(_) => Projection::default(),
    };
    let read = read_back(&projection, &expected, targets)?;
    let stored = StoredStates::of(&documents, projection.table, targets);
    losses.extend(compare(&expected, &read, &stored, source));
    let generated = expected
        .iter()
        .filter(|(written, configuration)| configuration.id.as_str() != written.as_str())
        .map(|(written, configuration)| (written.clone(), configuration.id.to_string()))
        .collect();
    Ok((
        Written {
            documents,
            projection,
            generated,
        },
        losses,
    ))
}

/// The configurations that the decoder states for the written documents,
/// `projection`: each row active where its configuration in `expected` is,
/// since the root record names the active configuration, and the active
/// configuration reconciled with the features. A reconciliation that fails
/// leaves the rows as they are.
fn read_back(
    projection: &Projection,
    expected: &BTreeMap<String, &Configuration>,
    targets: &ConfigurationTargets<'_>,
) -> Result<Vec<Configuration>, CodecError> {
    let mut read = projection.configurations.clone();
    for configuration in &mut read {
        if let Some(expected) = expected.get(configuration.id.as_str()) {
            configuration.active = expected.active;
        }
    }
    let mut reconciled = read.clone();
    match reconcile_active_configurations(&mut targets.neutral_features.to_vec(), &mut reconciled) {
        Ok(()) => Ok(reconciled),
        Err(error) if crate::design::decode::record::is_fatal(&error) => Err(error),
        Err(_) => Ok(read),
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

/// The payload of a document the write changes, from a copy of `source`.
fn edit<'p>(
    payload: &'p mut Option<Map<String, Value>>,
    source: &Map<String, Value>,
) -> &'p mut Map<String, Value> {
    payload.get_or_insert_with(|| source.clone())
}

/// The rows of `table`, the table at `table.position` of the document
/// `entry`, which `source` states and `payload` writes.
struct Rows<'s, 'p, 'b> {
    entry: &'s str,
    source: &'s Map<String, Value>,
    payload: &'p mut Option<Map<String, Value>>,
    table: &'s ConfigurationTable,
    targets: &'s ConfigurationTargets<'s>,
    /// The feature states that each row of `table` stores in a cell.
    stored: &'s StoredStates,
    /// The source of the write.
    baseline: &'s SourceConfigurations<'b>,
}

impl Rows<'_, '_, '_> {
    /// Write a row of each of `configurations`: the row of its id, else a
    /// new row. Regenerate the cells of each row whose configuration states
    /// cells other than `decoded`, or a part no cell stores that neither
    /// `decoded` nor the source states ([`unheld`]); remove the other rows;
    /// write the rows in the order of the configurations' ordinals. Return
    /// the configuration of each written row, keyed by the configuration id
    /// it reads back as.
    fn write<'c>(
        mut self,
        decoded: &[Configuration],
        configurations: &'c [Configuration],
    ) -> BTreeMap<String, &'c Configuration> {
        let (entry, table) = (self.entry, self.table);
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
                    let stored = self.stored.row(configuration.id.as_str());
                    let unchanged = decoded
                        .get(configuration.id.as_str())
                        .is_some_and(|decoded| {
                            Cells::of(decoded, stored) == Cells::of(configuration, stored)
                                && unheld(configuration, decoded, stored, self.baseline).is_empty()
                        });
                    if !unchanged {
                        self.cells(&row.id, Some(row), configuration);
                    }
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
                    self.cells(&row_id, None, configuration);
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
        hosted
            .into_iter()
            .map(|(configuration, row)| {
                (
                    neutral_configuration_id(entry, &row).to_string(),
                    configuration,
                )
            })
            .collect()
    }

    /// Write the name and the cells of `configuration` into the row
    /// `row_id`, which `stored` states.
    fn cells(
        &mut self,
        row_id: &str,
        stored: Option<&ConfigurationRow>,
        configuration: &Configuration,
    ) {
        let name = configuration.name.as_deref().unwrap_or_default();
        if stored.is_some_and(|row| row.name != name) {
            set_row_name(
                edit(self.payload, self.source),
                self.table.position,
                row_id,
                name,
            );
        }
        for column in &self.table.columns {
            let value = stored
                .and_then(|row| row.cells.iter().find(|cell| cell.column_id == column.id))
                .map(|cell| &cell.value);
            let data = match cell(column, configuration, value, self.targets) {
                Cell::Keep => continue,
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
}

/// What the write does with one cell of a row.
enum Cell {
    Keep,
    Write(CellData),
    Remove,
}

/// The cell of `column` that states what `configuration` states of the
/// column's target, where the row stores `stored`. A cell the write cannot
/// state is kept, and the read-back charges it.
fn cell(
    column: &ConfigurationColumn,
    configuration: &Configuration,
    stored: Option<&ConfiguredValue>,
    targets: &ConfigurationTargets<'_>,
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
                Some(feature) => configuration
                    .feature_states
                    .get(&feature.id)
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
        ),
        ColumnTarget::OwnedParameter(record) => expression_cell(
            column,
            targets.owned_parameter(*record).as_ref(),
            configuration,
            stored,
            targets,
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
) -> Cell {
    let (expression, value) = match parameter {
        Some(parameter) => (
            configuration.parameter_overrides.get(parameter),
            configuration.parameter_values.get(parameter),
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

/// The keys whose values `left` and `right` state otherwise.
fn differing<'k, K: Ord, V: PartialEq>(
    left: &'k BTreeMap<K, V>,
    right: &'k BTreeMap<K, V>,
) -> impl Iterator<Item = &'k K> {
    left.keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| left.get(*key) != right.get(*key))
}

/// The losses of the configurations that the written rows, `read`, state
/// otherwise than `expected`, the configuration of each row keyed by the
/// configuration id it reads back as. `stored` names the feature states
/// that each written row stores in a cell; a part that no cell stores is
/// charged where `source` does not state it as `expected` does
/// ([`unheld`]).
fn compare(
    expected: &BTreeMap<String, &Configuration>,
    read: &[Configuration],
    stored: &StoredStates,
    source: &SourceConfigurations<'_>,
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
        let mut fields = Vec::new();
        if back.material != configuration.material {
            fields.push(Difference::of(
                "material".to_owned(),
                configuration.material.as_ref(),
                back.material.as_ref(),
            ));
        }
        if back.bodies != configuration.bodies {
            fields.push(Difference::of(
                "bodies".to_owned(),
                configuration.bodies.as_ref(),
                back.bodies.as_ref(),
            ));
        }
        fields.extend(
            differing(&configuration.properties, &back.properties)
                .filter(|key| !key.as_str().starts_with(INFERRED))
                .map(|key| {
                    Difference::of(
                        format!("properties.{key}"),
                        configuration.properties.get(key),
                        back.properties.get(key),
                    )
                }),
        );
        fields.extend(
            differing(
                &configuration.parameter_overrides,
                &back.parameter_overrides,
            )
            .map(|parameter| {
                Difference::of(
                    format!("parameter_overrides.{parameter}"),
                    configuration.parameter_overrides.get(parameter),
                    back.parameter_overrides.get(parameter),
                )
            }),
        );
        for parameter in differing(&configuration.parameter_values, &back.parameter_values) {
            let expression = configuration.parameter_overrides.get(parameter);
            if expression.is_some() && expression == back.parameter_overrides.get(parameter) {
                values.push(format!("{id}: {parameter}"));
            } else {
                fields.push(Difference::of(
                    format!("parameter_values.{parameter}"),
                    configuration.parameter_values.get(parameter),
                    back.parameter_values.get(parameter),
                ));
            }
        }
        let states = |configuration: &'_ Configuration| {
            configuration
                .feature_states
                .iter()
                .map(|(feature, state)| (feature.clone(), state.evaluation.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let row = stored.row(written);
        let (stated, held) = (states(configuration), states(back));
        fields.extend(
            differing(&stated, &held)
                .filter(|feature| row.contains(*feature))
                .map(|feature| {
                    Difference::of(
                        format!("feature_states.{feature}"),
                        stated.get(feature),
                        held.get(feature),
                    )
                }),
        );
        fields.extend(unheld(configuration, back, row, source));
        if !fields.is_empty() {
            let fields = fields.iter().map(ToString::to_string).collect::<Vec<_>>();
            dropped.push(format!("{id}: {}", fields.join("; ")));
        }
    }
    let removed = read
        .keys()
        .filter(|id| !expected.contains_key(**id))
        .map(|id| (*id).to_owned());
    let mut losses = Vec::new();
    losses.extend(edit_dropped(
        EntityKind::Configuration,
        "F3D",
        "writes these configurations, whose written rows read back as configurations that differ in these fields: the configuration table holds no cell or row for the field, or its cell reads back otherwise",
        dropped,
    ));
    losses.extend(edit_dropped(
        EntityKind::Configuration,
        "F3D",
        "writes the rows of these configurations, which the document removes",
        removed,
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
