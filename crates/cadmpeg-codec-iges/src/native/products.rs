// SPDX-License-Identifier: Apache-2.0
//! Neutral product structure from the admitted IGES instance expansion.

use super::{NativeNetworkDefinition, NativeProductOccurrence, NativeSubfigureDefinition};
use crate::ids::{self, Stem};
use crate::loss::IgesLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn source_sequence(source: &str) -> Result<u32, CodecError> {
    source
        .strip_prefix("iges:entity:directory#")
        .and_then(|sequence| sequence.parse().ok())
        .ok_or_else(|| {
            CodecError::malformed("IGES product source identity has no directory sequence")
        })
}

fn occurrence_id(path: &[String]) -> Result<OccurrenceId, CodecError> {
    let (first, rest) = path
        .split_first()
        .ok_or_else(|| CodecError::malformed("IGES instance expansion has an empty path"))?;
    let mut stem = Stem::directory(source_sequence(first)?);
    for source in rest {
        stem = stem.child(source_sequence(source)?);
    }
    Ok(ids::occurrence(&stem))
}

fn carrier_bodies(ir: &CadIr, link: &str) -> Vec<cadmpeg_ir::ids::BodyId> {
    let edges: std::collections::BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.curve().is_some_and(|curve| curve.as_str() == link))
        .map(|edge| &edge.id)
        .collect();
    let vertices: std::collections::BTreeSet<_> = ir
        .model
        .vertices
        .iter()
        .filter(|vertex| vertex.point.as_str() == link)
        .map(|vertex| &vertex.id)
        .collect();
    let faces: std::collections::BTreeSet<_> = ir
        .model
        .faces
        .iter()
        .filter(|face| face.surface.as_str() == link)
        .map(|face| &face.id)
        .collect();
    let regions: std::collections::BTreeSet<_> = ir
        .model
        .shells
        .iter()
        .filter(|shell| {
            shell.wire_edges().iter().any(|edge| edges.contains(edge))
                || shell
                    .free_vertices()
                    .iter()
                    .any(|vertex| vertices.contains(vertex))
                || shell.faces().iter().any(|face| faces.contains(face))
        })
        .map(|shell| &shell.region)
        .collect();
    ir.model
        .regions
        .iter()
        .filter(|region| regions.contains(&region.id))
        .map(|region| region.body.clone())
        .collect()
}

pub(super) fn project(
    ir: &mut CadIr,
    occurrences: &[NativeProductOccurrence],
    subfigures: &[NativeSubfigureDefinition],
    networks: &[NativeNetworkDefinition],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let mut definitions = BTreeMap::new();
    for (source, native_ref, name) in subfigures
        .iter()
        .map(|item| (&item.source_entity, &item.id, &item.name))
        .chain(
            networks
                .iter()
                .map(|item| (&item.source_entity, &item.id, &item.name)),
        )
    {
        let id = ids::product(&Stem::directory(source_sequence(source)?));
        let label = name
            .as_ref()
            .and_then(crate::text::decoded)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        definitions.insert(
            source.clone(),
            ProductDefinition {
                id,
                kind: ProductDefinitionKind::Part,
                source_name: None,
                label,
                description: None,
                part_number: None,
                bom_properties: BTreeMap::new(),
                bodies: Vec::new(),
                native_ref: Some(native_ref.clone()),
            },
        );
    }
    for record in occurrences {
        let Some(member) = record.role.member() else {
            continue;
        };
        let Some(definition) = definitions.get_mut(&record.definition) else {
            continue;
        };
        for link in &record.neutral_links {
            let bodies =
                if let Some(body) = ir.model.bodies.iter().find(|body| body.id.as_str() == link) {
                    vec![body.id.clone()]
                } else {
                    carrier_bodies(ir, link)
                };
            if bodies.is_empty() {
                losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                    "{} member {member}: product membership of carrier {link} has no neutral body owner", record.id
                )));
            }
            for body in bodies {
                if !definition.bodies.contains(&body) {
                    definition.bodies.push(body);
                }
            }
        }
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(definitions.len()),
        "iges_product_definitions",
    )?;
    for record in occurrences
        .iter()
        .filter(|record| record.role.member().is_none())
    {
        let Some(definition) = definitions.get(&record.definition) else {
            losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                "{}: source definition {} has no admitted product",
                record.id, record.definition
            )));
            continue;
        };
        let id = occurrence_id(&record.instance_path)?;
        let parent = if record.instance_path.len() == 1 {
            OccurrenceParent::Root {}
        } else {
            OccurrenceParent::Occurrence {
                occurrence: occurrence_id(&record.instance_path[..record.instance_path.len() - 1])?,
            }
        };
        let transform = Transform::affine(record.local_transform).ok_or_else(|| {
            CodecError::malformed("admitted IGES occurrence has a non-finite affine placement")
        })?;
        ctx.charge_entities(1, "iges_product_occurrences")?;
        ir.model.occurrences.push(Occurrence {
            id,
            prototype: PrototypeReference::Local {
                definition: definition.id.clone(),
            },
            parent,
            ordinal: source_sequence(&record.source_instance)?,
            transform: Some(transform),
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: Some(record.id.clone()),
        });
    }
    ir.model
        .product_definitions
        .extend(definitions.into_values());
    Ok(losses)
}

#[cfg(test)]
mod tests;
