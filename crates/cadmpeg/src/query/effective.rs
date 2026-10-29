// SPDX-License-Identifier: Apache-2.0
//! Values that the IR resolvers compute for the records of a CADIR document.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::document::ArenaName;
use cadmpeg_ir::{CadIr, Expression, PresentationItem};
use serde::Serialize;
use serde_json::Value;

use super::{print_json, read_input, QueryArgs};

#[derive(Serialize)]
struct RecordValues {
    arena: &'static str,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_color: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    layers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transform: Option<Value>,
}

#[derive(Serialize)]
struct ConfigurationParameter {
    expression: Option<String>,
    value: Option<Value>,
}

#[derive(Serialize)]
struct ConfigurationValues {
    parameters: BTreeMap<String, ConfigurationParameter>,
    suppressed: BTreeMap<String, bool>,
}

#[derive(Serialize)]
struct Payload {
    protocol: &'static str,
    records: Vec<RecordValues>,
    expressions: BTreeMap<String, Option<String>>,
    configurations: BTreeMap<String, ConfigurationValues>,
    definitions: BTreeMap<String, BTreeMap<String, Option<Value>>>,
}

pub(super) fn run(args: &QueryArgs) -> Result<()> {
    let bytes = read_input(&args.file)?;
    let text = std::str::from_utf8(&bytes).context("CADIR is not UTF-8")?;
    let ir = CadIr::from_json(text).context("admitting CADIR document")?;
    let payload = Payload {
        protocol: "cadmpeg-effective-values-v1",
        records: record_values(&ir)?,
        expressions: ir
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.id.to_string(), render(&ir, &parameter.expression)))
            .collect(),
        configurations: configuration_values(&ir)?,
        definitions: definitions(&ir),
    };
    if args.json {
        print_json("effective", payload)
    } else {
        println!("arena\tid\tdisplay_color\tvisible\tlayers");
        for row in payload.records {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                row.arena,
                row.id,
                row.display_color
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.visible
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.layers.join(","),
            );
        }
        Ok(())
    }
}

/// Render an expression with each referenced parameter named by its source name.
fn render(ir: &CadIr, expression: &Expression) -> Option<String> {
    expression.render_with(|id| {
        ir.model
            .parameters
            .iter()
            .find(|parameter| parameter.id == *id)
            .map(|parameter| parameter.name.clone())
    })
}

fn presentation_item(target: &AppearanceTarget) -> Option<PresentationItem> {
    Some(match target.clone() {
        AppearanceTarget::Body(body) => PresentationItem::Body { body },
        AppearanceTarget::Face(face) => PresentationItem::Face { face },
        AppearanceTarget::Edge(edge) => PresentationItem::Edge { edge },
        AppearanceTarget::Vertex(vertex) => PresentationItem::Vertex { vertex },
        AppearanceTarget::Surface(surface) => PresentationItem::Surface { surface },
        AppearanceTarget::Curve(curve) => PresentationItem::Curve { curve },
        AppearanceTarget::Point(point) => PresentationItem::Point { point },
        AppearanceTarget::Occurrence(occurrence) => PresentationItem::Occurrence { occurrence },
        AppearanceTarget::Tessellation(tessellation) => {
            PresentationItem::Tessellation { tessellation }
        }
        AppearanceTarget::Record { .. } | AppearanceTarget::Source { .. } => return None,
    })
}

fn record_values(ir: &CadIr) -> Result<Vec<RecordValues>> {
    let model = serde_json::to_value(&ir.model).context("serializing the model")?;
    let mut rows = Vec::new();
    for arena in ArenaName::ALL {
        let Some(records) = model.get(arena.as_str()).and_then(Value::as_array) else {
            continue;
        };
        for id in records
            .iter()
            .filter_map(|record| record.get("id")?.as_str())
        {
            let target = AppearanceTarget::Record {
                arena: *arena,
                id: id.to_owned(),
            };
            let typed = target.typed_record_target(&ir.model);
            let layers = typed
                .as_ref()
                .and_then(presentation_item)
                .map(|item| {
                    ir.model
                        .presentation_layers
                        .iter()
                        .filter(|layer| layer.items.contains(&item))
                        .map(|layer| layer.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            let transform = match typed {
                Some(AppearanceTarget::Occurrence(ref occurrence)) => ir
                    .model
                    .occurrences
                    .iter()
                    .find(|record| record.id == *occurrence)
                    .and_then(|record| record.effective_transform().ok().flatten())
                    .map(serde_json::to_value)
                    .transpose()?,
                _ => None,
            };
            let row = RecordValues {
                arena: arena.as_str(),
                id: id.to_owned(),
                display_color: ir
                    .effective_display_color(&target)
                    .map(serde_json::to_value)
                    .transpose()?,
                visible: ir.effective_visibility(&target),
                layers,
                transform,
            };
            if row.display_color.is_some()
                || row.visible.is_some()
                || !row.layers.is_empty()
                || row.transform.is_some()
            {
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

fn configuration_values(ir: &CadIr) -> Result<BTreeMap<String, ConfigurationValues>> {
    let mut result = BTreeMap::new();
    for configuration in &ir.model.configurations {
        let mut parameters = BTreeMap::new();
        for parameter in &ir.model.parameters {
            parameters.insert(
                parameter.id.to_string(),
                ConfigurationParameter {
                    expression: configuration
                        .parameter_expression(ir, &parameter.id)
                        .and_then(|expression| render(ir, expression)),
                    value: configuration
                        .parameter_value(ir, &parameter.id)
                        .map(serde_json::to_value)
                        .transpose()?,
                },
            );
        }
        let suppressed = ir
            .model
            .features
            .iter()
            .map(|feature| {
                let state = configuration.feature_states.get(&feature.id).map_or(
                    feature.suppressed.unwrap_or(false),
                    cadmpeg_ir::features::ConfigurationFeatureState::is_suppressed,
                );
                (feature.id.to_string(), state)
            })
            .collect();
        result.insert(
            configuration.id.to_string(),
            ConfigurationValues {
                parameters,
                suppressed,
            },
        );
    }
    Ok(result)
}

/// Definitions with every dimension resolved to its literal or parameter value.
fn definitions(ir: &CadIr) -> BTreeMap<String, BTreeMap<String, Option<Value>>> {
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| {
            (
                feature.id.to_string(),
                ir.resolved_feature_definition(feature.definition()).ok(),
            )
        })
        .collect();
    let constraints = ir
        .model
        .sketch_constraints
        .iter()
        .map(|constraint| {
            (
                constraint.id.to_string(),
                ir.resolved_sketch_constraint_definition(constraint.definition.kind())
                    .ok(),
            )
        })
        .collect();
    BTreeMap::from([
        ("features".to_owned(), features),
        ("sketch_constraints".to_owned(), constraints),
    ])
}
