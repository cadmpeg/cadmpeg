// SPDX-License-Identifier: Apache-2.0
//! Output object identities and typed native selection references.

use crate::native::{ObjectRecord, PropertyRecord};
use cadmpeg_ir::document::CadIr;
use std::collections::{HashMap, HashSet};

fn reserve(proposed: &str, used: &mut HashSet<String>) -> String {
    let mut candidate = proposed.to_owned();
    // With n occupied names, one of these n + 1 distinct candidates is free.
    for suffix in 1..=used.len() {
        if used.insert(candidate.clone()) {
            return candidate;
        }
        candidate = format!("{proposed}_{suffix}");
    }
    used.insert(candidate.clone());
    candidate
}

pub(super) fn prepare(
    ir: &CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let mut used = HashSet::new();
    for object in objects {
        names.insert(object.id().as_str().to_owned(), object.name().to_owned());
        used.insert(object.name().to_owned());
    }
    // Product source names own their emitted object declarations. Resolve this
    // before features and sketches that may share the native object identity.
    for definition in &ir.model.product_definitions {
        if let Some(proposed) = &definition.source_name {
            let existing = definition
                .native_ref
                .as_ref()
                .and_then(|id| names.get(id))
                .cloned();
            if existing.as_ref() != Some(proposed) {
                let emitted = reserve(proposed, &mut used);
                if let Some(native) = &definition.native_ref {
                    names.insert(native.clone(), emitted.clone());
                }
                names.insert(definition.id.as_str().to_owned(), emitted);
            }
        }
    }
    for object in objects {
        if let Some(name) = names.get(object.id().as_str()).cloned() {
            names.insert(format!("name:{}", object.name()), name);
        }
    }
    for (index, feature) in ir.model.features.iter().enumerate() {
        let existing = feature
            .native_ref
            .as_ref()
            .and_then(|native| names.get(native))
            .cloned();
        let name = existing.unwrap_or_else(|| reserve(&format!("Feature{index}"), &mut used));
        names.insert(feature.id.as_str().to_owned(), name);
    }
    for (index, sketch) in ir.model.sketches.iter().enumerate() {
        let existing = sketch
            .native_ref
            .as_ref()
            .and_then(|native| names.get(native))
            .cloned()
            .or_else(|| {
                ir.model.features.iter().find_map(|feature| {
                    match feature.evaluation.definition().operation() {
                        cadmpeg_ir::features::FeatureOperation::Sketch { sketch: binding }
                            if binding.id() == Some(&sketch.id) =>
                        {
                            names.get(feature.id.as_str()).cloned()
                        }
                        _ => None,
                    }
                })
            });
        let name = existing.unwrap_or_else(|| reserve(&format!("Sketch{index}"), &mut used));
        names.insert(sketch.id.as_str().to_owned(), name);
    }
    for (index, definition) in ir.model.product_definitions.iter().enumerate() {
        let name = names
            .get(definition.id.as_str())
            .cloned()
            .or_else(|| {
                definition
                    .native_ref
                    .as_ref()
                    .and_then(|native| names.get(native))
                    .cloned()
            })
            .unwrap_or_else(|| {
                reserve(
                    &definition
                        .source_name
                        .clone()
                        .unwrap_or_else(|| format!("Product{index}")),
                    &mut used,
                )
            });
        names.insert(definition.id.as_str().to_owned(), name);
    }
    for (index, occurrence) in ir.model.occurrences.iter().enumerate() {
        let name = occurrence
            .native_ref
            .as_ref()
            .and_then(|native| names.get(native))
            .cloned()
            .unwrap_or_else(|| reserve(&format!("Instance{index}"), &mut used));
        names.insert(occurrence.id.as_str().to_owned(), name);
    }
    for property in properties {
        if let Some(owner) = names.get(&property.owner).cloned() {
            names.insert(format!("{}:owner", property.id), owner);
        }
        let mut local = Vec::new();
        for (index, link) in property.links().iter().enumerate() {
            let Some(link) = link else {
                continue;
            };
            if link.document().is_some() {
                continue;
            }
            let Some(target) = link.object().and_then(|target| names.get(target)).cloned() else {
                continue;
            };
            names.insert(format!("{}:link:{index}", property.id), target.clone());
            local.push(target);
            for (subindex, selection) in link.subelements().iter().enumerate() {
                names.insert(
                    format!("{}:link:{index}:subelement:{subindex}", property.id),
                    selection.clone(),
                );
            }
        }
        if let [target] = local.as_slice() {
            names.insert(property.id.clone(), target.clone());
        }
    }
    let mut topology_names =
        HashMap::<String, (HashSet<String>, HashSet<String>, HashSet<String>)>::new();
    for (index, body) in ir.model.bodies.iter().enumerate() {
        let owner = ir
            .model
            .features
            .iter()
            .rev()
            .find(|feature| feature.evaluation.outputs().contains(&body.id))
            .and_then(|feature| names.get(feature.id.as_str()))
            .cloned()
            .or_else(|| {
                body.regions
                    .iter()
                    .filter_map(|id| ir.model.regions.iter().find(|region| region.id == *id))
                    .flat_map(|region| &region.shells)
                    .filter_map(|id| ir.model.shells.iter().find(|shell| shell.id == *id))
                    .flat_map(cadmpeg_ir::topology::Shell::faces)
                    .filter_map(|id| ir.model.faces.iter().find(|face| face.id == *id))
                    .find_map(|face| {
                        ir.model
                            .surfaces
                            .iter()
                            .find(|surface| surface.id == face.surface)
                            .and_then(|surface| surface.source_object.as_ref())
                            .and_then(|source| names.get(source.object_id.as_str()))
                            .cloned()
                    })
            })
            .unwrap_or_else(|| reserve(&format!("Shape{index}"), &mut used));
        names.insert(body.id.as_str().to_owned(), owner.clone());
        let mut faces = Vec::new();
        let mut edges = Vec::new();
        for region in &body.regions {
            for shell in ir
                .model
                .regions
                .iter()
                .find(|candidate| candidate.id == *region)
                .into_iter()
                .flat_map(|region| &region.shells)
                .filter_map(|shell| {
                    ir.model
                        .shells
                        .iter()
                        .find(|candidate| candidate.id == *shell)
                })
            {
                for face in shell.faces() {
                    if !faces.contains(face) {
                        faces.push(face.clone());
                    }
                    for wire in ir
                        .model
                        .faces
                        .iter()
                        .find(|candidate| candidate.id == *face)
                        .into_iter()
                        .flat_map(|face| face.loops.iter())
                        .filter_map(|wire| {
                            ir.model
                                .loops
                                .iter()
                                .find(|candidate| candidate.id == *wire)
                        })
                    {
                        for edge in wire
                            .coedges()
                            .iter()
                            .filter_map(|coedge| {
                                ir.model
                                    .coedges
                                    .iter()
                                    .find(|candidate| candidate.id == *coedge)
                            })
                            .map(|coedge| &coedge.edge)
                        {
                            if !edges.contains(edge) {
                                edges.push(edge.clone());
                            }
                        }
                    }
                }
                for edge in shell.wire_edges() {
                    if !edges.contains(edge) {
                        edges.push(edge.clone());
                    }
                }
            }
        }
        let mut vertices = Vec::new();
        for id in &edges {
            if let Some(edge) = ir.model.edges.iter().find(|edge| edge.id == *id) {
                for vertex in [&edge.start, &edge.end] {
                    if !vertices.contains(vertex) {
                        vertices.push(vertex.clone());
                    }
                }
            }
        }
        for region in body
            .regions
            .iter()
            .filter_map(|id| ir.model.regions.iter().find(|region| region.id == *id))
        {
            for shell in region
                .shells
                .iter()
                .filter_map(|id| ir.model.shells.iter().find(|shell| shell.id == *id))
            {
                for vertex in shell.free_vertices() {
                    if !vertices.contains(vertex) {
                        vertices.push(vertex.clone());
                    }
                }
            }
        }
        let (seen_faces, seen_edges, seen_vertices) =
            topology_names.entry(owner.clone()).or_default();
        for (ids, seen, prefix) in [
            (
                faces
                    .iter()
                    .map(cadmpeg_ir::ids::FaceId::as_str)
                    .collect::<Vec<_>>(),
                seen_faces,
                "Face",
            ),
            (
                edges
                    .iter()
                    .map(cadmpeg_ir::ids::EdgeId::as_str)
                    .collect::<Vec<_>>(),
                seen_edges,
                "Edge",
            ),
            (
                vertices
                    .iter()
                    .map(cadmpeg_ir::ids::VertexId::as_str)
                    .collect::<Vec<_>>(),
                seen_vertices,
                "Vertex",
            ),
        ] {
            for id in ids {
                if seen.insert(id.to_owned()) {
                    names.insert(id.to_owned(), owner.clone());
                    names.insert(
                        format!("{id}:subelement"),
                        format!("{prefix}{}", seen.len()),
                    );
                }
            }
        }
    }
    names
}
