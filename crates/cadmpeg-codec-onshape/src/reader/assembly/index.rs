// SPDX-License-Identifier: Apache-2.0
//! Source definition and placement indexes.
use super::{key, optional_rows, split_rows, text, Error, Report, Rows};
use crate::archive::Bundle;
use crate::loss::OnshapeLossCode;
use cadmpeg_core::decode::DecodeContext;
use serde_json::Value;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

pub(super) type ReferenceKey = [String; 5];
pub(super) type DefinitionKey = [String; 4];
type StudioKey = (String, String);

pub(super) struct Index<'a> {
    pub occurrences: BTreeMap<Vec<String>, &'a Value>,
    /// Each subassembly definition with its position in `subAssemblies`.
    pub subassemblies: BTreeMap<DefinitionKey, (usize, &'a Value)>,
    pub parts: BTreeMap<ReferenceKey, &'a Value>,
    pub names: BTreeMap<(String, String, String), usize>,
    pub studios: BTreeMap<StudioKey, usize>,
    pub native_features: BTreeMap<(String, String), &'a Value>,
    external: BTreeMap<StudioKey, Option<&'a str>>,
    pinned: String,
    document: String,
}

impl<'a> Index<'a> {
    pub(super) fn build(
        ctx: &DecodeContext<'_>,
        definition: &'a Value,
        definition_path: &str,
        manifest: &'a Value,
        bundle: &'a Bundle<'_>,
        pinned: &str,
        report: &mut Report,
    ) -> Result<Self, Error> {
        let mut index = Self {
            occurrences: BTreeMap::new(),
            subassemblies: BTreeMap::new(),
            parts: BTreeMap::new(),
            names: BTreeMap::new(),
            studios: BTreeMap::new(),
            native_features: BTreeMap::new(),
            external: BTreeMap::new(),
            pinned: pinned.into(),
            document: manifest
                .pointer("/document/id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
        };
        for row in source_rows(manifest, "external", None, report) {
            ctx.charge_entities(1, "Onshape external reference index")?;
            let studio = index.studio_key(row);
            let microversion = row
                .get("microversionId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            index
                .external
                .entry(studio)
                .and_modify(|known| {
                    if *known != microversion {
                        *known = None;
                    }
                })
                .or_insert(microversion);
        }
        for (position, row) in source_rows(
            definition.get("rootAssembly").unwrap_or(&Value::Null),
            "occurrences",
            Some(format!("{definition_path}#/rootAssembly").as_str()),
            report,
        )
        .iter()
        .enumerate()
        {
            ctx.charge_entities(1, "Onshape occurrence path index")?;
            let path = optional_rows(row, "path").and_then(|rows| {
                if rows.len() > super::ASSEMBLY_DEPTH {
                    return Err("occurrence path exceeds the supported depth".into());
                }
                rows.iter()
                    .map(|v| {
                        v.as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .ok_or_else(|| "occurrence path component is invalid".into())
                    })
                    .collect::<Result<Vec<_>, String>>()
            });
            match path {
                Ok(path) => match index.occurrences.entry(path) {
                    Entry::Occupied(entry) if *entry.get() == row => {
                        report.push_coded(
                            &key(entry.key()),
                            OnshapeLossCode::AssemblyRecordRepeated,
                            "occurrence path repeats with the same placement; retained the first placement",
                        );
                    }
                    Entry::Occupied(entry) => {
                        report.give_up_as(
                            &format!("{definition_path}#/rootAssembly/occurrences/{position}"),
                            &key(entry.key()),
                            OnshapeLossCode::AssemblyRecordRepeated,
                            "occurrence path repeats with a different placement; retained the first placement",
                        );
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(row);
                    }
                },
                Err(reason)
                    if row
                        .get("path")
                        .and_then(Value::as_array)
                        .is_some_and(|path| path.len() > super::ASSEMBLY_DEPTH) =>
                {
                    report.push_coded(
                        "occurrence",
                        OnshapeLossCode::AssemblyRecordUnsupported,
                        reason,
                    );
                }
                Err(reason) => report.give_up_as(
                    &format!("{definition_path}#/rootAssembly/occurrences/{position}"),
                    "occurrence",
                    OnshapeLossCode::AssemblyRecordUnsupported,
                    reason,
                ),
            }
        }
        for (position, sub) in source_rows(
            definition,
            "subAssemblies",
            Some(format!("{definition_path}#").as_str()),
            report,
        )
        .iter()
        .enumerate()
        {
            ctx.charge_entities(1, "Onshape subassembly index")?;
            if !sub
                .get("elementId")
                .and_then(Value::as_str)
                .is_some_and(super::element_id)
            {
                report.give_up_as(
                    &format!("{definition_path}#/subAssemblies/{position}"),
                    "subassembly",
                    OnshapeLossCode::AssemblyRecordUnsupported,
                    "subassembly element id has an invalid folder character",
                );
                continue;
            }
            if sub.get("instances").is_none() {
                report.push_coded(
                    "subassembly",
                    OnshapeLossCode::AssemblyInstancesMissing,
                    "subassembly record has no instances field; it decodes as an empty subassembly",
                );
            }
            let definition_key = index.definition_key(sub);
            match index.subassemblies.entry(definition_key) {
                Entry::Occupied(entry) => {
                    if !same_content(entry.get().1, sub) {
                        report.give_up_as(
                            &format!("{definition_path}#/subAssemblies/{position}"),
                            "subassembly",
                            OnshapeLossCode::AssemblyRecordRepeated,
                            format!(
                                "subassembly definition repeats with different content; retained the first record: {:?}",
                                entry.key()
                            ),
                        );
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert((position, sub));
                }
            }
        }
        for (position, part) in source_rows(
            definition,
            "parts",
            Some(format!("{definition_path}#").as_str()),
            report,
        )
        .iter()
        .enumerate()
        {
            ctx.charge_entities(1, "Onshape part identity index")?;
            let reference = index.reference(part);
            let studio = index.studio_key(part);
            match index.parts.entry(reference) {
                Entry::Occupied(entry) => {
                    if !same_content(entry.get(), part) {
                        report.give_up_as(
                            &format!("{definition_path}#/parts/{position}"),
                            "part",
                            OnshapeLossCode::AssemblyRecordRepeated,
                            format!(
                                "part identity repeats with different content; retained the first record: {:?}",
                                entry.key()
                            ),
                        );
                    }
                    continue;
                }
                Entry::Vacant(entry) => {
                    entry.insert(part);
                }
            }
            *index.studios.entry(studio.clone()).or_default() += 1;
            if let Some(name) = part.get("name").and_then(Value::as_str) {
                let (document, element) = studio;
                *index
                    .names
                    .entry((document, element, name.into()))
                    .or_default() += 1;
            }
        }
        for element in source_rows(manifest, "elements", None, report) {
            if element.get("type").and_then(Value::as_str) != Some("ASSEMBLY") {
                continue;
            }
            let Ok(eid) = text(element, "id") else {
                continue;
            };
            if !super::element_id(eid) {
                continue;
            }
            if let Ok(response) = bundle.json(&format!("assemblies/{eid}/features.json")) {
                for feature in source_rows(response, "features", None, report) {
                    ctx.charge_entities(1, "Onshape native mate parameter index")?;
                    if let Some(native) = feature
                        .get("featureId")
                        .or_else(|| feature.get("id"))
                        .and_then(Value::as_str)
                    {
                        index
                            .native_features
                            .entry((eid.into(), native.into()))
                            .or_insert(feature);
                    }
                }
            }
        }
        Ok(index)
    }

    /// The stated document microversion. An empty value is an omitted one;
    /// an omitted value is the external reference entry's.
    pub(super) fn microversion<'b>(&'b self, row: &'b Value) -> Option<&'b str> {
        nonempty(row, "documentMicroversion")
            .or_else(|| nonempty(row, "microversionId"))
            .or_else(|| self.external.get(&self.studio_key(row)).copied().flatten())
    }
    /// The Part Studio of a source row. An omitted or empty `documentId`
    /// names the bundle's own document.
    pub(super) fn studio_key(&self, row: &Value) -> StudioKey {
        (
            nonempty(row, "documentId")
                .unwrap_or(&self.document)
                .to_owned(),
            string(row, "elementId"),
        )
    }
    /// Whether a source row names the bundle's own document.
    pub(super) fn local_document(&self, row: &Value) -> bool {
        nonempty(row, "documentId").is_none_or(|document| document == self.document)
    }
    /// The identity key of a source row. The pinned document microversion,
    /// an omitted and an empty one share one key component, as do an
    /// omitted, empty and `default` configuration and an omitted and the
    /// bundle's own `documentId`.
    pub(super) fn reference(&self, row: &Value) -> ReferenceKey {
        let (document, element) = self.studio_key(row);
        let configuration = string(row, "configuration");
        [
            document,
            element,
            string(row, "partId"),
            self.microversion(row)
                .filter(|microversion| *microversion != self.pinned)
                .unwrap_or("")
                .into(),
            if configuration == "default" {
                String::new()
            } else {
                configuration
            },
        ]
    }
    pub(super) fn definition_key(&self, row: &Value) -> DefinitionKey {
        let [document, element, _, microversion, configuration] = self.reference(row);
        [document, element, microversion, configuration]
    }
}

/// The identity fields as the row states them.
pub(super) fn description(row: &Value) -> String {
    let stated = |field: &str| {
        row.get(field).map_or_else(
            || "omitted".to_owned(),
            |value| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), |text| format!("{text:?}"))
            },
        )
    };
    format!(
        "documentId={}, elementId={}, partId={}, documentMicroversion={}, configuration={}",
        stated("documentId"),
        stated("elementId"),
        stated("partId"),
        stated("documentMicroversion"),
        stated("configuration"),
    )
}

/// Whether two records with one identity key state the same content: every
/// field but the identity fields, whose spellings the key folds.
fn same_content(first: &Value, second: &Value) -> bool {
    const IDENTITY: [&str; 6] = [
        "documentId",
        "elementId",
        "partId",
        "documentMicroversion",
        "microversionId",
        "configuration",
    ];
    fn rest(row: &Value) -> Option<BTreeMap<&String, &Value>> {
        row.as_object().map(|fields| {
            fields
                .iter()
                .filter(|(name, _)| !IDENTITY.contains(&name.as_str()))
                .collect()
        })
    }
    rest(first) == rest(second)
}
fn nonempty<'a>(row: &'a Value, field: &str) -> Option<&'a str> {
    row.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn string(row: &Value, field: &str) -> String {
    row.get(field).and_then(Value::as_str).unwrap_or("").into()
}
/// The rows of `field` of `row`. Where `row` is a record of the definition
/// at the source address `source` and `field` holds no array, the decode
/// gives up the field's record. Where `field` holds the values of an index
/// alone, or exceeds the row limit, a note states the loss.
fn source_rows<'a>(
    row: &'a Value,
    field: &str,
    source: Option<&str>,
    report: &mut Report,
) -> &'a [Value] {
    match (split_rows(row, field), source) {
        (Rows::Read(rows), _) => rows,
        (Rows::Malformed(reason), Some(source)) => {
            report.give_up_as(
                &format!("{source}/{field}"),
                field,
                OnshapeLossCode::AssemblyRecordUnsupported,
                reason,
            );
            &[]
        }
        (Rows::Bounded(reason) | Rows::Malformed(reason), _) => {
            report.push_coded(field, OnshapeLossCode::AssemblyRecordUnsupported, reason);
            &[]
        }
    }
}
