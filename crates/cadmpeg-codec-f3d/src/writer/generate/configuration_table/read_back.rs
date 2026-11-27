// SPDX-License-Identifier: Apache-2.0
//! Verify configurations through the finished archive's table and Design records.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, ByteRange, DecodeArena, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::value_unwritable;
use cadmpeg_ir::design::configuration::PartNumber;
use cadmpeg_ir::features::parameters::DesignConfiguration as Configuration;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{compare, DestinationParameters, Written};
use crate::design::configurations::ConfigurationTargets;
use crate::design::decode::{meta, parameters, scopes::parameter_scope, sketch};
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::ids::{native_stream, neutral_configuration_id, neutral_parameter_id_parts};
use crate::records::configuration::table::{test_row_differences, TestRowTarget};
use crate::records::configuration::DesignConfiguration;
use crate::records::parameters::DesignParameterOwner;
use crate::writer::primitives::{bases, projection, reconciled};

/// Configuration facts expected at their final archive placements.
#[derive(Default)]
pub(in crate::writer) struct Witness {
    documents: Vec<DesignConfiguration>,
    root: Option<(String, String)>,
    rows: BTreeMap<String, Configuration>,
    assigned_ids: BTreeSet<String>,
    stored_active: Option<Configuration>,
    test_rows: BTreeMap<String, BTreeMap<String, TestRowTarget>>,
    parameters: BTreeMap<ParameterId, ParameterId>,
    parameter_names: BTreeMap<ParameterId, String>,
}

impl Witness {
    pub(in crate::writer::generate) fn new(destinations: &DestinationParameters<'_>) -> Self {
        let mut witness = Self::default();
        let stream = crate::ids::native_scope(super::super::DESIGN_BULK);
        for (source, (index, name)) in destinations {
            let destination = neutral_parameter_id_parts(&stream, *index);
            witness.parameters.insert(destination, source.clone());
            witness
                .parameter_names
                .insert(source.clone(), (*name).to_owned());
        }
        witness
    }

    /// Record the named, placed payload and its mapping back to the target rows.
    pub(in crate::writer::generate) fn add(
        &mut self,
        written: &Written<'_>,
        index: usize,
        name: &str,
        payload: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), CodecError> {
        let document = DesignConfiguration::try_new(
            name.to_owned(),
            written.documents[index].kind(),
            payload,
        )?;
        self.track(written, index, document);
        Ok(())
    }

    pub(in crate::writer::generate) fn expect_root(&mut self, root: Option<(&str, &str)>) {
        self.root = root.map(|(table, row)| (table.to_owned(), row.to_owned()));
    }

    pub(super) fn retained(written: &Written<'_>) -> Self {
        let mut witness = Self::default();
        for (index, document) in written.documents.iter().enumerate() {
            witness.track(written, index, document.clone());
        }
        witness
    }

    fn track(&mut self, written: &Written<'_>, index: usize, after: DesignConfiguration) {
        let before = &written.documents[index];
        for (source, placed) in before.tables().iter().zip(after.tables()) {
            for (row, actual) in source.rows.iter().zip(&placed.rows) {
                let original = neutral_configuration_id(before.entry_name(), &row.id);
                if let Some(configuration) = written.expected.get(original.as_str()) {
                    let id = neutral_configuration_id(after.entry_name(), &actual.id).to_string();
                    if written.generated.contains_key(original.as_str()) {
                        self.assigned_ids.insert(id.clone());
                    }
                    self.rows.insert(id, (*configuration).clone());
                }
            }
        }
        if let Some((_, target)) = written.test_rows.as_ref().filter(|(at, _)| *at == index) {
            self.test_rows
                .insert(after.entry_name().clone(), target.clone());
        }
        self.documents.push(after);
        if self.stored_active.is_none() {
            self.stored_active.clone_from(&written.stored_active);
        }
    }

    pub(in crate::writer) fn read_back(
        &self,
        ctx: &DecodeContext<'_>,
        archive: &[u8],
        target: &CadIr,
        expected_parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if self.rows.is_empty()
            && !self
                .documents
                .iter()
                .any(|document| !document.tables().is_empty())
        {
            return Ok(());
        }
        #[cfg(test)]
        let corrupted = crate::integration_tests::configuration_wire::read_back::corrupted(archive);
        #[cfg(test)]
        let archive = corrupted.as_deref().unwrap_or(archive);
        let arena = DecodeArena::new();
        let back = ctx.with_arena(&arena, |local| {
            let root = View::over_retained(archive);
            let root = local.register_slice(
                root,
                ByteRange {
                    start: 0,
                    end: u64_from_index(archive.len()),
                },
            )?;
            self.verify(local, root, target, expected_parameters)
        });
        match back {
            Ok(notes) => losses.extend(notes),
            Err(error) if crate::design::decode::record::is_fatal(&error) => return Err(error),
            Err(error) => losses.extend(value_unwritable(
                "F3D",
                &format!("cannot read the written configuration records: {error}"),
                self.documents.iter().map(DesignConfiguration::id),
            )),
        }
        Ok(())
    }

    fn verify<'a>(
        &self,
        ctx: &DecodeContext<'a>,
        root: View<'a>,
        target: &CadIr,
        expected_parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
    ) -> Result<Vec<LossNote>, CodecError> {
        let scan = crate::container::scan_configurations(ctx, root)?;
        let mut read_losses = Vec::new();
        let configs = crate::design::configurations::decode_configurations(
            ctx,
            &scan,
            &mut read_losses,
            &mut Vec::new(),
        )?;
        let mut losses = self.compare_wire(&configs.documents);
        if self.rows.is_empty() {
            return Ok(losses);
        }

        let wants_v11 = !target.design.configurations.is_empty();
        let bindings = configuration_records(ctx, &scan, &configs.documents, wants_v11)?;
        let targets = ConfigurationTargets {
            parameters: &bindings.parameters,
            owners: &bindings.owners,
            scopes: &bindings.scopes,
            neutral_parameters: &bindings.projection.parameters,
            neutral_features: &bindings.projection.features,
        };
        let mut features = bindings.projection.features.clone();
        let projected = reconciled(projection(&configs.documents, &targets)?, &mut features)?;
        let expected_features = ConfigurationTargets {
            neutral_features: &target.model.features,
            ..targets
        };
        losses.extend(Written::compare_projected_features(
            self.stored_active.as_ref(),
            &projected,
            &expected_features,
            &features,
        ));
        let mut legacy = projected.configurations;
        let mut own = bindings.projection.parameters;
        self.compare_own(expected_parameters, &own, &target.tolerances, &mut losses);
        if wants_v11 {
            self.compare_v11(
                target,
                &legacy,
                &own,
                &bindings.projection.features,
                bindings.has_timeline,
                &mut losses,
            );
        }
        for row in &mut legacy {
            row.parameter_overrides = std::mem::take(&mut row.parameter_overrides)
                .into_iter()
                .map(|(id, value)| (self.original(&id).clone(), value))
                .collect();
            row.parameter_values = std::mem::take(&mut row.parameter_values)
                .into_iter()
                .map(|(id, value)| (self.original(&id).clone(), value))
                .collect();
        }
        for parameter in &mut own {
            parameter.id = self.original(&parameter.id).clone();
        }
        let expected = self
            .rows
            .iter()
            .map(|(id, row)| (id.clone(), row))
            .collect();
        let bases = bases(&features, &own);
        let assigned_ids = self.assigned_ids.iter().map(String::as_str).collect();
        losses.extend(compare(&expected, &legacy, &bases, &assigned_ids));
        Ok(losses)
    }

    fn original<'a>(&'a self, id: &'a ParameterId) -> &'a ParameterId {
        self.parameters.get(id).unwrap_or(id)
    }

    fn compare_own(
        &self,
        expected: &[cadmpeg_ir::features::parameters::DesignParameter],
        read: &[cadmpeg_ir::features::parameters::DesignParameter],
        tolerances: &cadmpeg_ir::units::Tolerances,
        losses: &mut Vec<LossNote>,
    ) {
        let expected = expected
            .iter()
            .map(|parameter| (&parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let read = read
            .iter()
            .map(|parameter| (self.original(&parameter.id), parameter))
            .collect::<BTreeMap<_, _>>();
        let mut named = BTreeSet::new();
        for row in self.rows.values() {
            named.extend(row.parameter_overrides.keys());
        }
        named.extend(self.parameter_names.keys());
        losses.extend(value_unwritable(
            "F3D",
            "reads these configuration parameters from Design with a different name, own expression or value",
            named.into_iter().filter_map(|id| {
                let authored = expected.get(id)?;
                let actual = read.get(id)?;
                let name = self.parameter_names.get(id).unwrap_or(&authored.name);
                let agrees = match (&authored.value, &actual.value) {
                    (Some(expected), Some(actual)) => crate::design_v11::configurations::agree(expected, actual, tolerances),
                    (expected, actual) => expected == actual,
                };
                (actual.name != *name || authored.expression != actual.expression || !agrees)
                    .then(|| format!("{id}: expected {name:?} {:?} {:?}, reads {:?} {:?} {:?}", authored.expression, authored.value, actual.name, actual.expression, actual.value))
            }),
        ));
    }

    fn compare_wire(&self, read: &[DesignConfiguration]) -> Vec<LossNote> {
        let read = read
            .iter()
            .map(|document| (document.entry_name(), document))
            .collect::<BTreeMap<_, _>>();
        let mut changes = Vec::new();
        for expected in &self.documents {
            let name = expected.entry_name();
            let Some(actual) = read.get(name) else {
                changes.push(format!("{name}: missing configuration document"));
                continue;
            };
            let active = |document: &DesignConfiguration| {
                document
                    .active_row()
                    .map(|row| (row.table_id.clone(), row.row_id.clone()))
            };
            let expected_active = if let Some((table, row)) = &self.root {
                expected
                    .tables()
                    .iter()
                    .any(|candidate| candidate.id == *table)
                    .then(|| (table.clone(), row.clone()))
            } else {
                active(expected)
            };
            if expected_active != active(actual) {
                changes.push(format!(
                    "{name}: active table and row {expected_active:?}, reads {:?}",
                    active(actual)
                ));
            }
            if expected.rule() != actual.rule() {
                changes.push(format!(
                    "{name}: activation rule {:?}, reads {:?}",
                    expected.rule(),
                    actual.rule()
                ));
            }
            let test_rows = self
                .test_rows
                .get(name)
                .map_or_else(BTreeSet::new, |target| {
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
                let stated = &self.test_rows[name][column].members;
                changes.push(format!(
                    "{name}: testRowData column {column} stated {stated:?}"
                ));
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
        }
        value_unwritable(
            "F3D",
            "reads different configuration facts from the written archive",
            changes,
        )
        .into_iter()
        .collect()
    }

    fn compare_v11(
        &self,
        target: &CadIr,
        legacy: &[Configuration],
        parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
        features: &[cadmpeg_ir::features::Feature],
        has_timeline: bool,
        losses: &mut Vec<LossNote>,
    ) {
        let Some(definition) =
            super::super::design::root_scope(target).map(|scope| &scope.definition)
        else {
            return;
        };
        let scopes = if has_timeline {
            BTreeMap::from([(String::new(), definition.clone())])
        } else {
            BTreeMap::new()
        };
        let features = features.iter().map(|feature| feature.id.as_str()).collect();
        let mut read_losses = Vec::new();
        let parameters = crate::design_v11::parameters::project(
            parameters,
            &std::collections::HashMap::new(),
            &scopes,
            &BTreeSet::new(),
            &features,
            &mut read_losses,
        );
        let read = crate::design_v11::configurations::project(
            legacy,
            &scopes,
            &BTreeSet::new(),
            &features,
            &parameters,
            &mut read_losses,
        );
        let authored = target
            .design
            .configurations
            .iter()
            .map(|row| (&row.id, row))
            .collect::<BTreeMap<_, _>>();
        let active = &read.active;
        let read = read
            .records
            .iter()
            .map(|row| (row.id.as_str(), row))
            .collect::<BTreeMap<_, _>>();
        let mut missing = Vec::new();
        for (id, expected) in &self.rows {
            let Some(authored_row) = authored.get(&expected.id) else {
                continue;
            };
            let path = format!("design.configurations[{}]", authored_row.id);
            let Some(actual) = read.get(id.as_str()) else {
                missing.push(format!(
                    "{path}: configuration has no v11 row in the written archive"
                ));
                continue;
            };
            if actual.name != authored_row.name {
                missing.push(format!(
                    "{path}.name: {:?}, reads {:?}",
                    authored_row.name, actual.name
                ));
            }
            let mut ancestor = Some(*authored_row);
            let mut seen = BTreeSet::new();
            let mut part = false;
            let mut description = false;
            while let Some(row) =
                ancestor.filter(|row| row.scope == authored_row.scope && seen.insert(&row.id))
            {
                if let Some(fields) = row.overrides.definitions.get(&authored_row.scope) {
                    part |= fields.part_number.is_some();
                    description |= fields.description.is_some();
                }
                ancestor = row.parent.as_ref().and_then(|id| authored.get(id).copied());
            }
            if active.get(&actual.scope) != Some(&actual.id) {
                for parameter in expected.parameter_overrides.keys() {
                    let written = parameters
                        .iter()
                        .find(|written| self.original(&written.id) == parameter);
                    let value = expected
                        .parameter_values
                        .get(parameter)
                        .and_then(|value| crate::design_v11::parameters::cache(value).1);
                    let back =
                        written.and_then(|written| actual.overrides.parameters.get(&written.id));
                    if back.is_none_or(|back| back.value != value) {
                        missing.push(format!("{path}.overrides.parameters[{parameter}]: cache {value:?}, reads {back:?}"));
                    }
                }
                for (feature, state) in &expected.feature_states {
                    let suppressed = state.evaluation.is_suppressed();
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
            let mut check = |stated: bool, key: &str, field: &str, actual: Option<&str>| {
                if let Some(text) = expected.properties.get(key).filter(|_| stated) {
                    if actual != Some(text.as_str()) {
                        missing.push(format!(
                            "{path}.overrides.definitions[{}].{field}: {text:?}, reads {actual:?}",
                            authored_row.scope
                        ));
                    }
                }
            };
            check(
                part,
                "property:Part Number",
                "part_number",
                identity.and_then(|identity| match identity.part_number.as_ref()? {
                    PartNumber::Text(text) => Some(text.as_str()),
                    PartNumber::ConfigurationName => Some(""),
                    _ => None,
                }),
            );
            check(
                description,
                "property:Description",
                "description",
                identity.and_then(|identity| identity.description.as_deref()),
            );
        }
        losses.extend(value_unwritable(
            "F3D",
            "reads these configuration values differently in CADIR v11",
            missing,
        ));
    }
}

struct ConfigurationRecords {
    parameters: Vec<crate::records::parameters::DesignParameter>,
    owners: Vec<DesignParameterOwner>,
    scopes: Vec<crate::records::feature::scope::DesignParameterScope>,
    projection: crate::design::feature_project::ProjectedParameterDesign,
    has_timeline: bool,
}

/// Read the Design records that resolve typed configuration objects.
fn configuration_records(
    ctx: &DecodeContext<'_>,
    scan: &crate::container::ContainerScan<'_>,
    documents: &[DesignConfiguration],
    wants_v11: bool,
) -> Result<ConfigurationRecords, CodecError> {
    use crate::records::configuration::table::ColumnTarget;

    let mut direct = BTreeSet::new();
    let mut owned = BTreeSet::new();
    let mut features = BTreeSet::new();
    for column in documents
        .iter()
        .flat_map(DesignConfiguration::tables)
        .flat_map(|table| &table.columns)
    {
        match column.target {
            ColumnTarget::Parameter(index) => {
                direct.insert(index);
            }
            ColumnTarget::OwnedParameter(index) => {
                owned.insert(index);
            }
            ColumnTarget::Feature(index) => {
                features.insert(index);
            }
            ColumnTarget::Property(_) => {}
        }
    }
    let mut parameters = if direct.is_empty() && owned.is_empty() {
        Vec::new()
    } else {
        parameters::decode_parameters(scan)?.parameters
    };
    parameters.retain(|parameter| {
        direct.contains(&parameter.record_index)
            || parameter
                .owner_record_index()
                .is_some_and(|owner| owned.contains(&owner))
    });
    let owners = if owned.is_empty() {
        Vec::new()
    } else {
        let indices = parameters
            .iter()
            .filter_map(|parameter| {
                Some((
                    native_stream(&parameter.id)?.to_owned(),
                    parameter.owner_record_index()?,
                ))
            })
            .collect::<Vec<_>>();
        let headers = sketch::decode_related_record_headers(scan, &indices)?;
        parameters::decode_parameter_owners(scan, &parameters, &headers)?.owners
    };
    features.extend(owners.iter().map(DesignParameterOwner::scope_record_index));
    let timelines = if wants_v11 || !features.is_empty() {
        meta::decode_feature_timelines(scan, &mut Vec::new())?
    } else {
        Vec::new()
    };
    let has_timeline = timelines
        .iter()
        .any(|timeline| !timeline.frame().items().is_empty());
    let types = if features.is_empty() {
        Vec::new()
    } else {
        meta::decode_types(scan)?
    };
    let scopes = if features.is_empty() {
        Vec::new()
    } else {
        let entities = sketch::decode_entity_headers(scan)?;
        let recipes = parameters::decode_recipes(scan)?;
        let mut decoded = parameter_scope::decode_parameter_scopes_for(
            ctx,
            scan,
            &parameter_scope::ParameterScopeSources {
                entities: &entities,
                types: &types,
                parameters: &parameters,
                parameter_owners: &owners,
                recipes: &recipes,
                ..Default::default()
            },
            Some(&features),
        )?
        .scopes;
        decoded.retain(|scope| features.contains(&scope.record_index));
        decoded
    };
    let projection = project_parameter_design_with_edge_identities(
        ctx,
        &ProjectInputs {
            native: &parameters,
            source_parameters: &parameters,
            owners: &owners,
            scopes: &scopes,
            timelines: &timelines,
            design_types: &types,
            ..Default::default()
        },
    )?;
    Ok(ConfigurationRecords {
        parameters,
        owners,
        scopes,
        projection,
        has_timeline,
    })
}
