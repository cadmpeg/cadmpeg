// SPDX-License-Identifier: Apache-2.0
//! Neutral product structure from the admitted IGES instance expansion.

use super::{
    ExternalReferenceKind, NativeExternalReference, NativeNetworkDefinition,
    NativeProductOccurrence, NativeSelectedComponent, NativeSolidAssembly, NativeSolidInstance,
    NativeSubfigureDefinition,
};
use crate::directory::DirectoryEntry;
use crate::entities::geometry::{resolve_transform, SourceSequences};
use crate::global::RealPrecision;
use crate::ids::{self, Stem};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{BodyId, OccurrenceId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn source_sequence(source: &str) -> Result<u32, CodecError> {
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

/// Whether an instance's solid chain runs through Parameter Data the cut
/// removed. The orphan occurrence would re-root under a truncated path, so
/// it drops with its solid instead (#215 rule 2).
fn solid_chain_cut(
    instances: &BTreeMap<u32, &NativeSolidInstance>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    instance_sequence: u32,
) -> bool {
    let cut = |sequence: u32| {
        entries
            .get(&sequence)
            .is_some_and(|entry| entry.parameter_line_count > 0 && !records.contains_key(&sequence))
    };
    if cut(instance_sequence) {
        return true;
    }
    let mut visited = BTreeSet::from([instance_sequence]);
    let mut current = instance_sequence;
    loop {
        let target = instances
            .get(&current)
            .and_then(|instance| instance.solid.as_ref());
        let Some(sequence) = target.and_then(|target| source_sequence(target).ok()) else {
            return false;
        };
        if !visited.insert(sequence) {
            return false;
        }
        if cut(sequence) {
            return true;
        }
        if entries
            .get(&sequence)
            .is_none_or(|entry| entry.entity_type != 430)
        {
            return false;
        }
        current = sequence;
    }
}

pub(super) fn project(
    ir: &mut CadIr,
    occurrences: &[NativeProductOccurrence],
    subfigures: &[NativeSubfigureDefinition],
    networks: &[NativeNetworkDefinition],
    assemblies: &[NativeSolidAssembly],
    instances: &[NativeSolidInstance],
    externals: &[NativeExternalReference],
    selected: &[NativeSelectedComponent],
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: Option<f64>,
    precision: RealPrecision,
    sequences: &SourceSequences,
    ctx: &DecodeContext<'_>,
) -> Result<(Vec<LossNote>, Vec<String>), CodecError> {
    let mut losses = Vec::new();
    let mut dropped_occurrences = Vec::new();
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
                sketches: Vec::new(),
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
        for link in &record.native_links {
            let sequence = match source_sequence(link) {
                Ok(sequence) => sequence,
                Err(_) => {
                    losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                        "{} member {member}: product membership of carrier {link} has no directory sequence", record.id
                    )));
                    continue;
                }
            };
            // A carrier without a neutral body is a non-body member (a curve,
            // point, or surface): it stays native with its links, and the
            // definition lists only bodies. A missing solid body is already
            // reported by its own projection or give-up.
            for body in &ir.model.bodies {
                if sequences.body_neutral_form(&body.id) == Some(sequence) {
                    if !definition.bodies.contains(&body.id) {
                        definition.bodies.push(body.id.clone());
                    }
                }
            }
        }
    }
    let mut assemblies_by_sequence = BTreeMap::new();
    for assembly in assemblies {
        if let Ok(sequence) = source_sequence(&assembly.source_entity) {
            assemblies_by_sequence.insert(sequence, assembly);
        }
    }
    let mut instances_by_sequence = BTreeMap::new();
    for instance in instances {
        if let Ok(sequence) = source_sequence(&instance.source_entity) {
            instances_by_sequence.insert(sequence, instance);
        }
    }
    for assembly in assemblies_by_sequence.values() {
        let sequence = source_sequence(&assembly.source_entity)?;
        let label = entries
            .get(&sequence)
            .and_then(|entry| directory_label(entry));
        definitions.insert(
            assembly.source_entity.clone(),
            ProductDefinition {
                sketches: Vec::new(),
                id: ids::product(&Stem::directory(sequence)),
                kind: ProductDefinitionKind::Part,
                source_name: None,
                label,
                description: None,
                part_number: None,
                bom_properties: BTreeMap::new(),
                bodies: Vec::new(),
                native_ref: Some(assembly.id.clone()),
            },
        );
    }
    // Every solid an instance names gets a reusable definition, so instances
    // link through Local prototypes. Brep solids list their bodies; CSG
    // solids keep none, and their construction stays in their features.
    let mut solid_targets = BTreeSet::new();
    for sequence in instances_by_sequence.keys() {
        if let Some(target) = instance_solid_target(&instances_by_sequence, entries, *sequence) {
            solid_targets.insert(target);
        }
    }
    for assembly in assemblies_by_sequence.values() {
        for target in assembly_shell_sequences(
            assembly,
            &assemblies_by_sequence,
            &instances_by_sequence,
            entries,
        ) {
            solid_targets.insert(target);
        }
    }
    for target in solid_targets {
        ensure_solid_definition(&mut definitions, ir, entries, sequences, target);
    }
    for assembly in assemblies_by_sequence.values() {
        let Some(definition) = definitions.get_mut(&assembly.source_entity) else {
            continue;
        };
        let mut bodies = Vec::new();
        for target in assembly_shell_sequences(
            assembly,
            &assemblies_by_sequence,
            &instances_by_sequence,
            entries,
        ) {
            for body in neutral_bodies(ir, sequences, target) {
                if !bodies.contains(&body) {
                    bodies.push(body);
                }
            }
        }
        definition.bodies = bodies;
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(definitions.len()),
        "iges_product_definitions",
    )?;
    // Occurrence ordinals are dense in expansion order, starting at 0 (#209 §2.2).
    let mut ordinal = 0_u32;
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
            ordinal,
            transform: Some(transform),
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: Some(record.id.clone()),
        });
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("IGES product occurrence ordinal overflows"))?;
    }
    // Solid instances place their solid, and external references place a
    // document outside this one. Assembly members that are themselves
    // placeable carry their per-item placement. Ordinals stay dense in
    // source order after the expansion occurrences (#209 §2.2).
    let mut placed = Vec::new();
    for (sequence, instance) in &instances_by_sequence {
        let target = instance_solid_target(&instances_by_sequence, entries, *sequence);
        if target.is_none() && solid_chain_cut(&instances_by_sequence, entries, records, *sequence)
        {
            continue;
        }
        let prototype = match target {
            Some(target) => match definitions.get(&format!("iges:entity:directory#{target}")) {
                Some(definition) => PrototypeReference::Local {
                    definition: definition.id.clone(),
                },
                None => {
                    losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                        "{}: source definition iges:entity:directory#{target} has no admitted product",
                        instance.id
                    )));
                    continue;
                }
            },
            None => PrototypeReference::Unresolved {},
        };
        let path = match target {
            Some(target) => vec![
                format!("iges:entity:directory#{target}"),
                instance.source_entity.clone(),
            ],
            None => vec![instance.source_entity.clone()],
        };
        let transform = entry_placement(
            entries,
            records,
            length_factor,
            precision,
            *sequence,
            &instance.id,
            &mut losses,
            ctx,
        )?;
        placed.push((
            *sequence,
            occurrence_id(&path)?,
            prototype,
            transform,
            instance.id.clone(),
        ));
    }
    for assembly in assemblies_by_sequence.values() {
        let Ok(assembly_sequence) = source_sequence(&assembly.source_entity) else {
            continue;
        };
        for item in assembly.items.iter() {
            let Some(target) = item
                .item
                .as_ref()
                .and_then(|item| source_sequence(item).ok())
            else {
                continue;
            };
            let Some(entry) = entries.get(&target) else {
                continue;
            };
            let (prototype, native_ref) = match entry.entity_type {
                184 => match definitions.get(&format!("iges:entity:directory#{target}")) {
                    Some(definition) => (
                        PrototypeReference::Local {
                            definition: definition.id.clone(),
                        },
                        assemblies_by_sequence
                            .get(&target)
                            .map(|assembly| assembly.id.clone())
                            .unwrap_or_default(),
                    ),
                    None => continue,
                },
                430 => match instance_solid_target(&instances_by_sequence, entries, target) {
                    Some(resolved) => {
                        match definitions.get(&format!("iges:entity:directory#{resolved}")) {
                            Some(definition) => (
                                PrototypeReference::Local {
                                    definition: definition.id.clone(),
                                },
                                instances_by_sequence
                                    .get(&target)
                                    .map(|instance| instance.id.clone())
                                    .unwrap_or_default(),
                            ),
                            None => continue,
                        }
                    }
                    None => continue,
                },
                416 => match externals.iter().find(|record| {
                    record.source_entity == format!("iges:entity:directory#{target}")
                }) {
                    Some(record) => (external_prototype(record), record.id.clone()),
                    None => continue,
                },
                _ => continue,
            };
            let transform = item_placement(
                entries,
                records,
                length_factor,
                precision,
                item.transformation.as_deref(),
                &assembly.id,
                &mut losses,
                ctx,
            )?;
            let id = occurrence_id(
                &[
                    assembly.source_entity.clone(),
                    format!("iges:entity:directory#{target}"),
                ]
                .as_slice(),
            )?;
            placed.push((assembly_sequence, id, prototype, transform, native_ref));
        }
    }
    for external in externals {
        let sequence = source_sequence(&external.source_entity)?;
        let id = occurrence_id(std::slice::from_ref(&external.source_entity))?;
        if !records.contains_key(&sequence) && external.reference_kind.needs_document() {
            // The external reference's identifying Parameter Data is given
            // up, so no prototype document can be read. The occurrence is
            // not formed, and the tail loss names it (#215 rules 1-2).
            dropped_occurrences.push(id.as_str().to_owned());
            continue;
        }
        placed.push((
            sequence,
            id,
            external_prototype(external),
            None,
            external.id.clone(),
        ));
    }
    for component in selected {
        let sequence = source_sequence(&component.source_entity)?;
        let target = component
            .boolean_tree
            .as_ref()
            .and_then(|tree| source_sequence(tree).ok())
            .filter(|target| {
                entries
                    .get(target)
                    .is_some_and(|entry| entry.entity_type == 180)
            });
        let prototype = match target {
            Some(target) => {
                let key = format!("iges:entity:directory#{target}");
                if !definitions.contains_key(&key) {
                    definitions.insert(
                        key,
                        ProductDefinition {
                            sketches: Vec::new(),
                            id: ids::product(&Stem::directory(target)),
                            kind: ProductDefinitionKind::Part,
                            source_name: None,
                            label: entries
                                .get(&target)
                                .and_then(|entry| directory_label(entry)),
                            description: None,
                            part_number: None,
                            bom_properties: BTreeMap::new(),
                            bodies: Vec::new(),
                            native_ref: Some(format!("iges:solid:boolean-tree#D{target}")),
                        },
                    );
                }
                match definitions.get(&format!("iges:entity:directory#{target}")) {
                    Some(definition) => PrototypeReference::Local {
                        definition: definition.id.clone(),
                    },
                    None => continue,
                }
            }
            None => PrototypeReference::Unresolved {},
        };
        let path = match target {
            Some(target) => vec![
                format!("iges:entity:directory#{target}"),
                component.source_entity.clone(),
            ],
            None => vec![component.source_entity.clone()],
        };
        let transform = entry_placement(
            entries,
            records,
            length_factor,
            precision,
            sequence,
            &component.id,
            &mut losses,
            ctx,
        )?;
        placed.push((
            sequence,
            occurrence_id(&path)?,
            prototype,
            transform,
            component.id.clone(),
        ));
    }
    placed.sort_by(|left, right| (left.0, left.1.as_str()).cmp(&(right.0, right.1.as_str())));
    placed.dedup_by(|left, right| left.1 == right.1);
    for (_, id, prototype, transform, native_ref) in placed {
        ctx.charge_entities(1, "iges_product_occurrences")?;
        ir.model.occurrences.push(Occurrence {
            id,
            prototype,
            parent: OccurrenceParent::Root {},
            ordinal,
            transform,
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: Some(native_ref),
        });
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("IGES product occurrence ordinal overflows"))?;
    }
    ir.model
        .product_definitions
        .extend(definitions.into_values());
    Ok((losses, dropped_occurrences))
}

fn directory_label(entry: &DirectoryEntry) -> Option<String> {
    std::str::from_utf8(&entry.label)
        .ok()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn solid_native_id(entity_type: i64, sequence: u32) -> Option<String> {
    match entity_type {
        150 | 152 | 154 | 156 | 158 | 160 | 168 => {
            Some(format!("iges:solid:primitive#D{sequence}"))
        }
        162 | 164 => Some(format!("iges:solid:procedural#D{sequence}")),
        180 => Some(format!("iges:solid:boolean-tree#D{sequence}")),
        182 => Some(format!("iges:solid:selected-component#D{sequence}")),
        184 => Some(format!("iges:product:solid-assembly#D{sequence}")),
        186 => Some(format!("iges:solid:manifold-brep#D{sequence}")),
        _ => None,
    }
}

fn instance_solid_target(
    instances: &BTreeMap<u32, &NativeSolidInstance>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    instance_sequence: u32,
) -> Option<u32> {
    let mut visited = BTreeSet::from([instance_sequence]);
    let mut current = instance_sequence;
    loop {
        let target = instances.get(&current)?.solid.as_ref()?;
        let sequence = source_sequence(target).ok()?;
        if !visited.insert(sequence) {
            return None;
        }
        let entry = entries.get(&sequence)?;
        if entry.entity_type == 430 {
            current = sequence;
            continue;
        }
        return solid_native_id(entry.entity_type, sequence).map(|_| sequence);
    }
}

fn neutral_bodies(ir: &CadIr, sequences: &SourceSequences, sequence: u32) -> Vec<BodyId> {
    ir.model
        .bodies
        .iter()
        .filter(|body| sequences.body_neutral_form(&body.id) == Some(sequence))
        .map(|body| body.id.clone())
        .collect()
}

fn ensure_solid_definition(
    definitions: &mut BTreeMap<String, ProductDefinition>,
    ir: &CadIr,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    sequences: &SourceSequences,
    sequence: u32,
) {
    let key = format!("iges:entity:directory#{sequence}");
    if definitions.contains_key(&key) {
        return;
    }
    let Some(entry) = entries.get(&sequence) else {
        return;
    };
    let Some(native_ref) = solid_native_id(entry.entity_type, sequence) else {
        return;
    };
    let bodies = if entry.entity_type == 186 {
        neutral_bodies(ir, sequences, sequence)
    } else {
        Vec::new()
    };
    definitions.insert(
        key,
        ProductDefinition {
            sketches: Vec::new(),
            id: ids::product(&Stem::directory(sequence)),
            kind: ProductDefinitionKind::Part,
            source_name: None,
            label: directory_label(entry),
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies,
            native_ref: Some(native_ref),
        },
    );
}

fn assembly_shell_sequences(
    assembly: &NativeSolidAssembly,
    assemblies: &BTreeMap<u32, &NativeSolidAssembly>,
    instances: &BTreeMap<u32, &NativeSolidInstance>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> Vec<u32> {
    let mut shells = Vec::new();
    let mut visited = BTreeSet::new();
    let mut stack: Vec<u32> = assembly
        .items
        .iter()
        .filter_map(|item| item.item.as_ref())
        .filter_map(|item| source_sequence(item).ok())
        .collect();
    while let Some(sequence) = stack.pop() {
        if !visited.insert(sequence) {
            continue;
        }
        let Some(entry) = entries.get(&sequence) else {
            continue;
        };
        match entry.entity_type {
            186 => shells.push(sequence),
            184 => {
                if let Some(nested) = assemblies.get(&sequence) {
                    stack.extend(
                        nested
                            .items
                            .iter()
                            .filter_map(|item| item.item.as_ref())
                            .filter_map(|item| source_sequence(item).ok()),
                    );
                }
            }
            430 => {
                if let Some(target) = instance_solid_target(instances, entries, sequence) {
                    stack.push(target);
                }
            }
            _ => {}
        }
    }
    shells.sort();
    shells.dedup();
    shells
}

fn external_prototype(record: &NativeExternalReference) -> PrototypeReference {
    let text = |value: Option<&cadmpeg_ir::native::bytes::NativeBytes<Vec<u8>>>| {
        value
            .and_then(crate::text::decoded)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
    };
    let file = text(record.file_identifier.as_ref());
    let symbolic = text(record.symbolic_name.as_ref());
    let library = text(record.library_name.as_ref());
    let object = symbolic;
    let document = match record.reference_kind {
        ExternalReferenceKind::ExternalDefinition
        | ExternalReferenceKind::ExternalFileDefinition
        | ExternalReferenceKind::ExternalLogical => match file {
            Some(path) => ExternalDocument::path(path),
            None => ExternalDocument::Missing {},
        },
        ExternalReferenceKind::NativeDefinition => ExternalDocument::Missing {},
        ExternalReferenceKind::NativeLibraryDefinition => match library {
            Some(name) => ExternalDocument::document_id(name),
            None => ExternalDocument::Missing {},
        },
    };
    PrototypeReference::External { document, object }
}

fn native_transformation_sequence(value: &str) -> Option<u32> {
    value
        .strip_prefix("iges:native:transformation#D")?
        .parse()
        .ok()
}

fn entry_placement(
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: Option<f64>,
    precision: RealPrecision,
    sequence: u32,
    native_id: &str,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Transform>, CodecError> {
    let Some(entry) = entries.get(&sequence) else {
        return Ok(None);
    };
    if entry.transform == 0 {
        return Ok(None);
    }
    let Some(length_factor) = length_factor else {
        return Ok(None);
    };
    match resolve_transform(
        entry.transform,
        entries,
        records,
        length_factor,
        precision,
        &mut BTreeSet::new(),
        ctx,
    ) {
        Ok(transform) => Ok(Some(transform)),
        Err(_) => {
            losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                "{native_id}: solid placement pointer is not a resolvable transformation"
            )));
            Ok(None)
        }
    }
}

fn item_placement(
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: Option<f64>,
    precision: RealPrecision,
    transformation: Option<&str>,
    native_id: &str,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Transform>, CodecError> {
    let Some(sequence) = transformation.and_then(native_transformation_sequence) else {
        return Ok(None);
    };
    let Some(length_factor) = length_factor else {
        return Ok(None);
    };
    match resolve_transform(
        i64::from(sequence),
        entries,
        records,
        length_factor,
        precision,
        &mut BTreeSet::new(),
        ctx,
    ) {
        Ok(transform) => Ok(Some(transform)),
        Err(_) => {
            losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                "{native_id}: assembly member placement pointer is not a resolvable transformation"
            )));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
