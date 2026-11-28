// SPDX-License-Identifier: Apache-2.0
//! Observe the staged archive with the production reader.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::value_unwritable;
use cadmpeg_ir::design::configuration::PartNumber;
use cadmpeg_ir::features::parameters::DesignConfiguration as Configuration;
use cadmpeg_ir::features::{ConfigurationId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{compare, DestinationParameters, Written};
use crate::design::configurations::{root_document, ConfigurationTargets, Projection};
use crate::ids::{neutral_configuration_id, neutral_parameter_id_parts};
use crate::loss::F3dLossCode;
use crate::records::configuration::table::{test_row_differences, TestRowTarget};
use crate::records::configuration::{DesignConfiguration, DesignConfigurationActiveRow};
use crate::writer::primitives::bases;

struct ParameterPlacement {
    id: ParameterId,
    name: String,
    expression: String,
}

struct NativeComparison {
    losses: Vec<LossNote>,
    matches_plan: bool,
}

/// Final placement and the configuration facts requested at that placement.
pub(in crate::writer) struct Destination<'a> {
    pub(in crate::writer) documents: Cow<'a, [DesignConfiguration]>,
    rows: BTreeMap<String, Configuration>,
    assigned_ids: BTreeSet<String>,
    stored_active: Option<Configuration>,
    test_rows: Option<(usize, BTreeMap<String, TestRowTarget>)>,
    parameters: BTreeMap<ParameterId, ParameterPlacement>,
    original_parameters: BTreeMap<ParameterId, ParameterId>,
    configured_parameters: BTreeSet<ParameterId>,
    part_numbers: BTreeMap<ConfigurationId, PartNumber>,
    source_root: Option<ProductDefinitionId>,
    generated: bool,
}

impl<'a> Destination<'a> {
    pub(super) fn retained(
        written: Written<'a, '_>,
        part_numbers: BTreeMap<ConfigurationId, PartNumber>,
    ) -> Self {
        Self::new(
            written,
            None,
            part_numbers,
            None,
            None,
            None,
            super::super::DESIGN_BULK,
        )
    }

    pub(in crate::writer::generate) fn synthesized(
        written: Written<'a, '_>,
        mut documents: Vec<DesignConfiguration>,
        root: &super::Plan,
        part_numbers: BTreeMap<ConfigurationId, PartNumber>,
        targets: &DestinationParameters<'_>,
        source_root: Option<ProductDefinitionId>,
        bulk_stream: &str,
    ) -> Result<Self, CodecError> {
        if let Some((table, row)) = root.active_ids() {
            if let Some(index) = root_document(&documents, table) {
                documents[index].set_active_row(DesignConfigurationActiveRow {
                    record_index: 0,
                    table_id: table.to_owned(),
                    row_id: row.to_owned(),
                })?;
            }
        }
        Ok(Self::new(
            written,
            Some(documents),
            part_numbers,
            Some(root),
            Some(targets),
            source_root,
            bulk_stream,
        ))
    }

    pub(in crate::writer::generate) fn into_owned(self) -> Destination<'static> {
        Destination {
            documents: Cow::Owned(self.documents.into_owned()),
            ..self
        }
    }

    fn new(
        written: Written<'a, '_>,
        placed: Option<Vec<DesignConfiguration>>,
        part_numbers: BTreeMap<ConfigurationId, PartNumber>,
        root: Option<&super::Plan>,
        destinations: Option<&DestinationParameters<'_>>,
        source_root: Option<ProductDefinitionId>,
        bulk_stream: &str,
    ) -> Self {
        let stream = crate::ids::native_scope(bulk_stream);
        let parameters: BTreeMap<_, _> = destinations
            .into_iter()
            .flatten()
            .map(|(source, place)| {
                (
                    source.clone(),
                    ParameterPlacement {
                        id: neutral_parameter_id_parts(&stream, place.index),
                        name: place.name.to_owned(),
                        expression: place.expression.to_owned(),
                    },
                )
            })
            .collect();
        let original_parameters = parameters
            .iter()
            .map(|(source, destination)| (destination.id.clone(), source.clone()))
            .collect();
        let documents = placed.as_deref().unwrap_or(&written.documents);
        let rules = written
            .documents
            .iter()
            .zip(documents.iter())
            .filter(|(source, _)| source.rule().is_some())
            .filter_map(|(source, placed)| {
                let key = |name| {
                    cadmpeg_core::text::NonBlankString::new(format!("activation_rule:{name}"))
                };
                Some((key(source.entry_name())?, key(placed.entry_name())?))
            })
            .collect::<BTreeMap<_, _>>();
        let mut rows = BTreeMap::new();
        let mut assigned_ids = BTreeSet::new();
        for (index, (before, after)) in written.documents.iter().zip(documents.iter()).enumerate() {
            for table in before.tables() {
                for row in &table.rows {
                    let source = neutral_configuration_id(before.entry_name(), &row.id);
                    let Some(requested) = written.expected.get(source.as_str()) else {
                        continue;
                    };
                    let row = root.map_or(row.id.as_str(), |root| {
                        root.placed_row(index, table.position, &row.id)
                    });
                    let id = neutral_configuration_id(after.entry_name(), row).to_string();
                    let mut requested = (*requested).clone();
                    requested.native_ref = Some(after.id());
                    requested.active = root.map_or(requested.active, |root| {
                        root.active() == Some(requested.id.as_str())
                    });
                    requested.properties = requested
                        .properties
                        .into_iter()
                        .map(|(key, value)| (rules.get(&key).cloned().unwrap_or(key), value))
                        .collect();
                    if written.generated.contains_key(source.as_str()) {
                        assigned_ids.insert(id.clone());
                    }
                    rows.insert(id, requested);
                }
            }
        }
        Self {
            documents: placed.map_or(written.documents, Cow::Owned),
            rows,
            assigned_ids,
            stored_active: written.stored_active,
            test_rows: written.test_rows,
            parameters,
            original_parameters,
            configured_parameters: written.bindings.into_values().collect(),
            part_numbers,
            source_root,
            generated: root.is_some(),
        }
    }

    pub(in crate::writer) fn read_back(
        &self,
        policy: &DecodePolicy,
        archive: &[u8],
        target: &CadIr,
        expected_parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
        scopes: &[crate::records::feature::scope::DesignParameterScope],
        losses: &mut Vec<LossNote>,
    ) {
        if self.documents.is_empty() {
            return;
        }
        #[cfg(test)]
        let corrupted = crate::integration_tests::configuration_wire::read_back::corrupted(archive);
        #[cfg(test)]
        let archive = corrupted.as_deref().unwrap_or(archive);
        let arena = DecodeArena::new();
        let back =
            DecodeContext::from_root_bytes(archive, &arena, policy).and_then(|(local, root)| {
                let losses = if self.rows.is_empty() {
                    let scan = crate::container::scan_configurations(&local, root)?;
                    let decoded = crate::design::configurations::decode_configurations(
                        &local,
                        &scan,
                        &mut Vec::new(),
                    )?;
                    self.compare_wire(&decoded.documents).losses
                } else {
                    let decoded = crate::decode::decode(&local, root)?;
                    let read = decoded.ir;
                    let documents = read
                        .native
                        .namespace("f3d")
                        .map(|namespace| {
                            namespace.arena_as_for_decode(&local, "design_configurations")
                        })
                        .transpose()?
                        .unwrap_or_default();
                    let NativeComparison {
                        mut losses,
                        matches_plan,
                    } = self.compare_wire(&documents);
                    if matches_plan {
                        self.compare_cadir(target, expected_parameters, scopes, read, &mut losses);
                    }
                    losses
                };
                local
                    .resource_refusal()
                    .map_or(Ok(losses), |limit| Err(limit.into()))
            });
        match back {
            Ok(notes) => losses.extend(notes),
            Err(error) => {
                let mut ids: Vec<_> = self.documents.iter().map(DesignConfiguration::id).collect();
                ids.sort_unstable();
                ids.dedup();
                losses.push(F3dLossCode::WriterConfigurationReadBackUnread.note(format!(
                    "the F3D write cannot read the written configuration records ({error}): {}",
                    ids.join(", ")
                )));
            }
        }
    }

    fn compare_cadir(
        &self,
        target: &CadIr,
        expected_parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
        scopes: &[crate::records::feature::scope::DesignParameterScope],
        read: CadIr,
        losses: &mut Vec<LossNote>,
    ) {
        self.compare_own(
            expected_parameters,
            &read.model.parameters,
            &target.tolerances,
            losses,
        );
        self.compare_v11(target, &read, losses);
        let mut projection = Projection {
            configurations: read.model.configurations,
            ..Projection::default()
        };
        let mut own = read.model.parameters;
        let original = |id: ParameterId| self.original_parameters.get(&id).cloned().unwrap_or(id);
        for row in &mut projection.configurations {
            row.parameter_overrides = std::mem::take(&mut row.parameter_overrides)
                .into_iter()
                .map(|(id, value)| (original(id), value))
                .collect();
            row.parameter_values = std::mem::take(&mut row.parameter_values)
                .into_iter()
                .map(|(id, value)| (original(id), value))
                .collect();
        }
        for parameter in &mut own {
            if let Some(source) = self.original_parameters.get(&parameter.id) {
                parameter.id = source.clone();
            }
        }
        let configured = read
            .model
            .features
            .iter()
            .filter(|feature| {
                projection
                    .configurations
                    .iter()
                    .any(|row| row.active && row.feature_states.contains_key(&feature.id))
            })
            .cloned()
            .collect::<Vec<_>>();
        let neutral = target
            .model
            .features
            .iter()
            .filter(|feature| {
                configured
                    .iter()
                    .any(|configured| feature.id == configured.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        losses.extend(Written::compare_projected_features(
            self.stored_active.as_ref(),
            &projection,
            &ConfigurationTargets {
                parameters: &[],
                owners: &[],
                scopes,
                neutral_parameters: &[],
                neutral_features: &neutral,
                bindings: None,
            },
            &configured,
        ));
        let expected = self
            .rows
            .iter()
            .map(|(id, row)| (id.as_str(), row))
            .collect();
        let bases = bases(&read.model.features, &own);
        let assigned_ids = self.assigned_ids.iter().map(String::as_str).collect();
        losses.extend(compare(
            &expected,
            &projection.configurations,
            &bases,
            &assigned_ids,
        ));
    }

    fn compare_own(
        &self,
        expected: &[cadmpeg_ir::features::parameters::DesignParameter],
        read: &[cadmpeg_ir::features::parameters::DesignParameter],
        tolerances: &cadmpeg_ir::units::Tolerances,
        losses: &mut Vec<LossNote>,
    ) {
        let read = read
            .iter()
            .map(|parameter| (&parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let mut named = self.configured_parameters.iter().collect::<BTreeSet<_>>();
        for row in self.rows.values() {
            named.extend(row.parameter_overrides.keys());
        }
        let expected = expected
            .iter()
            .map(|parameter| (&parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        losses.extend(value_unwritable(
            "F3D",
            "reads these configuration parameters from Design with a different name, own expression or value",
            named.into_iter().filter_map(|id| {
                let destination = self.parameters.get(id);
                let authored = expected.get(id).copied();
                let (name, expression) = destination.map(|p| (p.name.as_str(), p.expression.as_str()))
                    .or_else(|| authored.map(|p| (p.name.as_str(), p.expression.as_str())))?;
                let Some(actual) = read.get(destination.map_or(id, |p| &p.id)) else {
                    return Some(format!("{id}: expected Design parameter {name:?}"));
                };
                let agrees = authored.is_none_or(|authored| match (&authored.value, &actual.value) {
                    (Some(want), Some(got)) => crate::design_v11::configurations::agree(want, got, tolerances),
                    (want, got) => want == got,
                });
                (actual.name != name || actual.expression != expression || !agrees)
                    .then(|| format!("{id}: expected {name:?} {expression:?} {:?}, reads {:?} {:?} {:?}", authored.and_then(|p| p.value.as_ref()), actual.name, actual.expression, actual.value))
            }),
        ));
    }

    fn compare_wire(&self, read: &[DesignConfiguration]) -> NativeComparison {
        let read = read
            .iter()
            .map(|document| (document.entry_name(), document))
            .collect::<BTreeMap<_, _>>();
        let mut changes = read
            .keys()
            .filter(|name| {
                !self
                    .documents
                    .iter()
                    .any(|document| document.entry_name() == **name)
            })
            .map(|name| format!("{name}: unplanned configuration document"))
            .collect::<Vec<_>>();
        let mut matches_plan = changes.is_empty();
        for (index, expected) in self.documents.iter().enumerate() {
            let name = expected.entry_name();
            let Some(actual) = read.get(name) else {
                changes.push(format!("{name}: missing configuration document"));
                matches_plan = false;
                continue;
            };
            let active = |document: &DesignConfiguration| {
                document
                    .active_row()
                    .map(|row| (row.table_id.clone(), row.row_id.clone()))
            };
            let expected_active = active(expected);
            let actual_active = active(actual);
            matches_plan &=
                expected_active == actual_active && expected.payload() == actual.payload();
            if expected_active != actual_active {
                changes.push(format!(
                    "{name}: active table and row {expected_active:?}, reads {actual_active:?}"
                ));
            }
            if expected.rule() != actual.rule() {
                changes.push(format!(
                    "{name}: activation rule {:?}, reads {:?}",
                    expected.rule(),
                    actual.rule()
                ));
            }
            let test_plan = self
                .test_rows
                .as_ref()
                .filter(|(at, _)| *at == index)
                .map(|(_, rows)| rows);
            let test_rows = test_plan.map_or_else(BTreeSet::new, |target| {
                test_row_differences(actual.payload(), target)
            });
            if test_rows.is_empty()
                && expected.payload().get("testRowData") != actual.payload().get("testRowData")
            {
                changes.push(format!(
                    "{name}: testRowData {:?}, reads {:?}",
                    expected.payload().get("testRowData"),
                    actual.payload().get("testRowData")
                ));
            }
            for column in test_rows {
                let stated = test_plan
                    .and_then(|plan| plan.get(column))
                    .and_then(|target| target.members.as_ref());
                changes.push(format!(
                    "{name}: testRowData column {column} stated {stated:?}"
                ));
            }
            let table_changes = changes.len();
            for table in actual.tables() {
                if !expected
                    .tables()
                    .iter()
                    .any(|planned| planned.id == table.id)
                {
                    changes.push(format!("{name}: unplanned table {}", table.id));
                }
            }
            for table in expected.tables() {
                let Some(back) = actual.tables().iter().find(|back| back.id == table.id) else {
                    changes.push(format!("{name}: missing table {}", table.id));
                    continue;
                };
                if table.columns != back.columns {
                    changes.push(format!(
                        "{name}: table {} columns {:?}, reads {:?}",
                        table.id, table.columns, back.columns
                    ));
                }
                let expected_rows = table.rows.iter().map(|row| (&row.id, &row.name));
                let actual_rows = back.rows.iter().map(|row| (&row.id, &row.name));
                if expected_rows.clone().ne(actual_rows.clone()) {
                    changes.push(format!(
                        "{name}: table {} rows {:?}, reads {:?}",
                        table.id,
                        expected_rows.collect::<Vec<_>>(),
                        actual_rows.collect::<Vec<_>>()
                    ));
                }
                let rows = back
                    .rows
                    .iter()
                    .map(|row| (&row.id, row))
                    .collect::<BTreeMap<_, _>>();
                for row in &table.rows {
                    let Some(back) = rows.get(&row.id) else {
                        continue;
                    };
                    let back_cells = back
                        .cells
                        .iter()
                        .map(|cell| (&cell.column_id, &cell.value))
                        .collect::<BTreeMap<_, _>>();
                    let expected_cells = row
                        .cells
                        .iter()
                        .map(|cell| (&cell.column_id, &cell.value))
                        .collect::<BTreeMap<_, _>>();
                    for column in expected_cells
                        .keys()
                        .chain(back_cells.keys())
                        .copied()
                        .collect::<BTreeSet<_>>()
                    {
                        let (value, back) = (expected_cells.get(column), back_cells.get(column));
                        if value != back {
                            changes.push(format!(
                                "{name}: row {}, column {column} stated {value:?}, reads {back:?}",
                                row.id
                            ));
                        }
                    }
                }
            }
            if expected.rule().is_none() {
                let expected_payload = expected.payload();
                let actual_payload = actual.payload();
                let typed_tables_changed = changes.len() != table_changes;
                for key in expected_payload
                    .keys()
                    .chain(actual_payload.keys())
                    .collect::<BTreeSet<_>>()
                {
                    if key == "testRowData" || (key == "tables" && typed_tables_changed) {
                        continue;
                    }
                    if expected_payload.get(key) != actual_payload.get(key) {
                        changes.push(format!("{name}: configuration payload differs at {key}"));
                    }
                }
            }
        }
        NativeComparison {
            matches_plan,
            losses: value_unwritable(
                "F3D",
                "reads different configuration facts from the written archive",
                changes,
            )
            .into_iter()
            .collect(),
        }
    }

    fn compare_v11(&self, target: &CadIr, actual: &CadIr, losses: &mut Vec<LossNote>) {
        let authored = target
            .design
            .configurations
            .iter()
            .map(|row| (&row.id, row))
            .collect::<BTreeMap<_, _>>();
        let active = actual
            .design
            .scopes
            .iter()
            .filter_map(|scope| Some((&scope.definition, scope.active_configuration.as_ref()?)))
            .collect::<BTreeMap<_, _>>();
        let parameters = actual
            .design
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let read = actual
            .design
            .configurations
            .iter()
            .map(|row| (row.id.as_str(), row))
            .collect::<BTreeMap<_, _>>();
        let legacy = actual
            .model
            .configurations
            .iter()
            .map(|row| (row.id.as_str(), row))
            .collect::<BTreeMap<_, _>>();
        let bases = bases(&actual.model.features, &actual.model.parameters);
        let mut missing = Vec::new();
        for (id, expected) in &self.rows {
            let (Some(authored_row), Some(legacy)) =
                (authored.get(&expected.id), legacy.get(id.as_str()))
            else {
                continue;
            };
            let path = format!("design.configurations[{}]", authored_row.id);
            let Some(actual) = read.get(id.as_str()) else {
                missing.push(format!(
                    "{path}: configuration has no v11 row in the written archive"
                ));
                continue;
            };
            let scope = if self.generated && self.source_root.as_ref() == Some(&authored_row.scope)
            {
                crate::tree::file_root_id()
            } else {
                authored_row.scope.clone()
            };
            if actual.scope != scope || actual.native_ref != expected.native_ref {
                missing.push(format!(
                    "{path}: scope {scope}, native reference {:?}, reads {} {:?}",
                    expected.native_ref, actual.scope, actual.native_ref
                ));
            }
            if (active.get(&actual.scope) == Some(&&actual.id)) != expected.active {
                missing.push(format!("{path}: expected active state {}", expected.active));
            }
            let mut ancestor = Some(*authored_row);
            let mut seen = BTreeSet::new();
            let mut description = false;
            while let Some(row) =
                ancestor.filter(|row| row.scope == authored_row.scope && seen.insert(&row.id))
            {
                if let Some(fields) = row.overrides.definitions.get(&authored_row.scope) {
                    description |= fields.description.is_some();
                }
                ancestor = row.parent.as_ref().and_then(|id| authored.get(id).copied());
            }
            if !expected.active {
                let vocabulary = target
                    .design
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.scope == authored_row.scope)
                    .filter_map(|parameter| {
                        let (name, id) = match self.parameters.get(&parameter.id) {
                            Some(bound) => (bound.name.as_str(), &bound.id),
                            None if !self.generated => {
                                (parameter.name.as_ref()?.as_str(), &parameter.id)
                            }
                            None => return None,
                        };
                        Some((name, id))
                    })
                    .collect();
                for (parameter, text) in &expected.parameter_overrides {
                    let id = self
                        .parameters
                        .get(parameter)
                        .map_or(parameter, |bound| &bound.id);
                    let requested_value = expected.parameter_values.get(parameter);
                    if bases.expression(legacy, id) != Some(text)
                        || requested_value
                            .is_some_and(|value| bases.value(legacy, id) != Some(value))
                    {
                        continue;
                    }
                    let value = requested_value
                        .and_then(|value| crate::design_v11::parameters::cache(value).1);
                    let expression = parameters.get(id).map(|parameter| {
                        crate::design_v11::parameters::expression(
                            text,
                            &parameter.kind,
                            &vocabulary,
                        )
                    });
                    let back = actual.overrides.parameters.get(id);
                    if back.is_none_or(|back| back.value != value || back.expression != expression)
                    {
                        missing.push(format!("{path}.overrides.parameters[{parameter}]: expression {text:?}, cache {value:?}, reads {back:?}"));
                    }
                }
                for (feature, state) in &expected.feature_states {
                    let suppressed = state.evaluation.is_suppressed();
                    if bases
                        .feature(legacy, feature)
                        .map(|state| state.evaluation.is_suppressed())
                        != Some(suppressed)
                    {
                        continue;
                    }
                    let back = actual
                        .overrides
                        .features
                        .get(feature)
                        .and_then(|state| state.suppressed);
                    if back != Some(suppressed) {
                        missing.push(format!("{path}.overrides.features[{feature}].suppressed: {suppressed}, reads {back:?}"));
                    }
                }
            }
            let identity = actual.overrides.definitions.get(&actual.scope);
            let property_preserved =
                |key: &str| expected.properties.get(key) == legacy.properties.get(key);
            if let Some(part_number) = self
                .part_numbers
                .get(&expected.id)
                .filter(|_| property_preserved("property:Part Number"))
            {
                let back = identity.and_then(|identity| identity.part_number.as_ref());
                if back != Some(part_number) {
                    missing.push(format!(
                        "{path}.overrides.definitions[{}].part_number: {part_number:?}, reads {back:?}",
                        authored_row.scope
                    ));
                }
            }
            if let Some(text) = expected
                .properties
                .get("property:Description")
                .filter(|_| description && property_preserved("property:Description"))
            {
                let back = identity.and_then(|identity| identity.description.as_ref());
                if back != Some(text) {
                    missing.push(format!(
                        "{path}.overrides.definitions[{}].description: {text:?}, reads {back:?}",
                        authored_row.scope
                    ));
                }
            }
        }
        losses.extend(value_unwritable(
            "F3D",
            "reads these configuration values differently in CADIR v11",
            missing,
        ));
    }
}
