// SPDX-License-Identifier: Apache-2.0
//! Order native definitions by their current typed references.

use super::{
    annotations, associations, attributes, coordinates, copious, count, directory_label, drawings,
    entity, fem, fonts, levels, macros, products, real, shape, solids, structure, text, BTreeSet,
    CadIr, DirectoryLineFont, Entity, EntityStatus, Fields, NativeExport, NativeRecord, Targets,
    Value, ARENAS,
};
use std::collections::BTreeMap;

fn encode(
    ir: &CadIr,
    arena: &str,
    record: &NativeRecord,
    targets: &Targets,
    claimed: &BTreeSet<u32>,
    lenient: bool,
) -> Result<(Entity, Vec<usize>, Vec<String>), String> {
    let (type_code, form) = shape(arena, record)?;
    let mut fields = Fields {
        values: record.fields().map_err(|error| error.to_string())?,
        targets,
        references: BTreeSet::new(),
        lenient,
        missing: Vec::new(),
    };
    let pattern_feature = if arena == "rectangular_arrays" {
        solids::apply_rectangular_pattern(ir, record, &mut fields)?
    } else {
        None
    };
    let boolean_feature = if arena == "boolean_trees" {
        solids::apply_combine(ir, record, &mut fields)?
    } else {
        None
    };
    let annotation = if arena == "annotations" {
        annotations::apply_text(ir, record, &mut fields)?
    } else {
        None
    };
    let definition = if matches!(arena, "subfigure_definitions" | "network_definitions") {
        let mut matches = ir
            .model
            .product_definitions
            .iter()
            .filter(|definition| definition.native_ref.as_deref() == Some(record.id()));
        match (matches.next(), matches.next()) {
            (Some(definition), None) => Some(definition),
            _ => None,
        }
    } else {
        None
    };
    if let Some(definition) = definition {
        let name = definition
            .label
            .as_ref()
            .filter(|label| !label.trim().is_empty())
            .ok_or("current product definition has no IGES name")?;
        fields.values.insert(
            "name".into(),
            serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(
                name.as_bytes().to_vec(),
            ))
            .map_err(|error| error.to_string())?,
        );
    }
    let occurrence = products::current_occurrence(ir, arena, record);
    let mut placement = None;
    if let Some(occurrence) = occurrence {
        let cadmpeg_ir::products::PrototypeReference::Local { definition } = &occurrence.prototype
        else {
            return Err("current occurrence has no local prototype".into());
        };
        let product = ir
            .model
            .product_definitions
            .iter()
            .find(|product| &product.id == definition)
            .ok_or("current occurrence prototype is missing")?;
        let native_definition = product
            .native_ref
            .as_ref()
            .ok_or("current occurrence prototype has no IGES definition")?;
        fields.values.insert(
            "definition".into(),
            Value::String(native_definition.clone()),
        );
        {
            let transform = occurrence
                .transform
                .unwrap_or(cadmpeg_ir::transform::Transform::identity());
            let native = occurrence.native_ref.as_deref().and_then(|id| {
                ir.native
                    .namespace("iges")?
                    .arenas()
                    .get("product_occurrences")?
                    .iter()
                    .find(|native| native.id() == id)?
                    .field("local_transform")
            });
            let same = native.and_then(|value| {
                serde_json::from_value::<cadmpeg_ir::transform::Transform>(value).ok()
            }) == Some(transform);
            if !same {
                fields
                    .values
                    .insert("translation".into(), serde_json::json!([0.0, 0.0, 0.0]));
                fields.values.insert(
                    "scale".into(),
                    if arena == "network_instances" {
                        serde_json::json!([1.0, 1.0, 1.0])
                    } else {
                        serde_json::json!(1.0)
                    },
                );
                let mut rows = [[cadmpeg_ir::scalar::FiniteReal::ONE; 4]; 3];
                for (row, values) in rows.iter_mut().zip(transform.affine_rows()) {
                    for (slot, value) in row.iter_mut().zip(values) {
                        *slot = cadmpeg_ir::scalar::FiniteReal::new(value)
                            .ok_or("occurrence placement is not finite")?;
                    }
                }
                placement = Some(super::super::Placement { rows });
            }
        }
    }
    let mut result = match arena {
        "text_fonts" | "line_fonts" | "text_templates" => {
            fonts::encode(arena, &mut fields, type_code, form)?
        }
        "groups" | "associativities" => structure::encode(&mut fields, type_code, form)?,
        "fem_entities" => fem::encode(&mut fields, type_code, form)?,
        "copious_data" => {
            if directory_sequence(record).is_some_and(|sequence| claimed.contains(&sequence)) {
                return Err("copious carrier is owned by model topology".into());
            }
            copious::encode(&mut fields, type_code, form)?
        }
        "annotations" => annotations::encode(arena, &mut fields, type_code, form)?,
        "macro_definitions" | "macro_instances" => {
            macros::encode(arena, &mut fields, type_code, form)?
        }
        "definition_levels" => levels::encode(&mut fields, type_code, form)?,
        "subfigure_definitions"
        | "subfigure_instances"
        | "network_definitions"
        | "network_instances"
        | "connect_points"
        | "solid_assemblies"
        | "solid_instances"
        | "external_references"
        | "transformations" => products::encode(arena, &mut fields, type_code, form)?,
        "views" | "view_visibility" | "segmented_visibility" | "drawings" => {
            drawings::encode(arena, &mut fields, type_code, form)?
        }
        "primitive_solids"
        | "procedural_solids"
        | "boolean_trees"
        | "selected_components"
        | "rectangular_arrays"
        | "circular_arrays" => solids::encode(arena, &mut fields, type_code, form)?,
        "flashes" => {
            let mut parameters = coordinates(fields.value("reference_point"), 2)?;
            parameters.extend([
                fields.real("dimension_1")?,
                fields.real("dimension_2")?,
                fields.real("rotation")?,
                fields.pointer("reference_entity")?,
            ]);
            entity(type_code, form, parameters)
        }
        "units_data" => {
            let units = fields.array("units")?;
            let mut parameters = vec![count(&fields.values, "declared_count", units.len())?];
            for unit in units {
                let unit = unit.as_object().ok_or("unit has no typed fields")?;
                parameters.extend([
                    text(unit.get("unit_type"))?,
                    text(unit.get("unit_value"))?,
                    real(unit.get("scale_factor"))?,
                ]);
            }
            entity(type_code, form, parameters)
        }
        _ => return Err("record has no native writer".into()),
    };
    super::numeric::retain(ir, record, &mut result);
    if let Some(definition) = definition {
        result.source_targets.push(definition.id.to_string());
    }
    if let Some(placement) = placement {
        result.transform = Some(placement);
    }
    if let Some(occurrence) = occurrence {
        result.source_targets.push(occurrence.id.to_string());
    }
    let mut owners = Vec::new();
    if let Some(value) = fields.value("owners") {
        for owner in value
            .as_array()
            .ok_or("owners have no typed identity list")?
        {
            let owner = owner.as_str().ok_or("owner has no typed identity")?;
            let [index] = targets.get(owner).map_or(&[][..], Vec::as_slice) else {
                return Err(format!("owner-wait:{owner}"));
            };
            owners.push(*index);
        }
    }
    if let Some(feature) = pattern_feature {
        result.source_targets.push(feature);
    }
    if let Some(feature) = boolean_feature {
        result.source_targets.push(feature);
    }
    if let Some(annotation) = annotation {
        result.source_targets.push(annotation);
    }
    result.source_targets.push(record.id().to_owned());
    if let Some(source) = fields.value("source_entity").and_then(Value::as_str) {
        result.source_targets.push(source.to_owned());
    }
    if let Some(number) = fields
        .value("fallback_line_font_number")
        .and_then(Value::as_i64)
    {
        if number < 0 {
            return Err("native definition has an unresolved fallback line font".into());
        }
        result.line_font = Some(DirectoryLineFont::Number(number));
    }
    result.status = match type_code {
        304 | 308 | 310 | 312 | 320 => EntityStatus::Definition,
        132 => EntityStatus::LogicalPositional,
        _ => result.status,
    };
    Ok((result, owners, fields.missing))
}

fn line_fonts(ir: &CadIr, entities: &mut [Entity], targets: &Targets) {
    let Some(namespace) = ir.native.namespace("iges") else {
        return;
    };
    for record in namespace
        .arenas()
        .get("display_attributes")
        .into_iter()
        .flatten()
    {
        let Ok(fields) = record.fields() else {
            continue;
        };
        let Some(source) = fields.get("source_entity").and_then(Value::as_str) else {
            continue;
        };
        let font =
            if let Some(definition) = fields.get("line_font_definition").and_then(Value::as_str) {
                let Some([index]) = targets.get(definition).map(Vec::as_slice) else {
                    continue;
                };
                if entities
                    .get(*index)
                    .is_none_or(|entity| entity.type_code != 304)
                {
                    continue;
                }
                DirectoryLineFont::Definition(*index)
            } else {
                let Some(number) = record
                    .field("line_font_number")
                    .as_ref()
                    .and_then(Value::as_i64)
                    .filter(|number| *number >= 0)
                else {
                    continue;
                };
                DirectoryLineFont::Number(number)
            };
        if let Some(indices) = targets.get(source) {
            for index in indices {
                if let Some(entity) = entities.get_mut(*index) {
                    entity.line_font = Some(font);
                }
            }
        }
    }
}

fn source_directory_label(ir: &CadIr, record: &NativeRecord) -> Option<String> {
    let source = record.field("source_entity")?.as_str()?.to_owned();
    let entity = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("entities")?
        .iter()
        .find(|entity| entity.id() == source)?;
    let bytes: cadmpeg_ir::native::bytes::NativeBytes =
        serde_json::from_value(entity.field("label")?).ok()?;
    let label = std::str::from_utf8(bytes.as_ref()).ok()?;
    (label.len() <= 8
        && label
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' '))
    .then(|| label.to_owned())
}

/// Directory labels from the neutral records a native record projects to.
/// Features, product definitions and occurrences carry their Directory label
/// neutrally; re-emission restores it so an edited label reaches the output.
fn neutral_labels(ir: &CadIr) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for feature in &ir.model.features {
        if let (Some(native_ref), Some(name)) = (feature.native_ref.as_ref(), feature.name.as_ref())
        {
            labels
                .entry(native_ref.clone())
                .or_insert_with(|| name.clone());
        }
    }
    for definition in &ir.model.product_definitions {
        if let (Some(native_ref), Some(label)) =
            (definition.native_ref.as_ref(), definition.label.as_ref())
        {
            labels
                .entry(native_ref.clone())
                .or_insert_with(|| label.clone());
        }
    }
    for occurrence in &ir.model.occurrences {
        if let (Some(native_ref), Some(name)) =
            (occurrence.native_ref.as_ref(), occurrence.name.as_ref())
        {
            labels
                .entry(native_ref.clone())
                .or_insert_with(|| name.clone());
        }
    }
    labels
}

/// The Directory sequence of a native record's own entity, when it names one.
fn directory_sequence(record: &NativeRecord) -> Option<u32> {
    let source = record.field("source_entity");
    source
        .as_ref()
        .and_then(|value| value.as_str())
        .and_then(|text| text.strip_prefix("iges:entity:directory#"))
        .and_then(|sequence| sequence.parse::<u32>().ok())
}

pub(in crate::writer) fn append(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    version: crate::IgesVersion,
) -> NativeExport {
    let targets = attributes::targets(ir, entities);
    let linked_primitives = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("primitive_solids"))
        .into_iter()
        .flatten()
        .filter(|record| {
            ir.model
                .features
                .iter()
                .any(|feature| feature.native_ref.as_deref() == Some(record.id()))
        })
        .map(|record| record.id().to_owned())
        .collect::<BTreeSet<_>>();
    let already_generated = linked_primitives
        .iter()
        .filter(|id| targets.contains_key(*id))
        .cloned()
        .collect();
    let mut emitter = Emitter {
        ir,
        version,
        labels: neutral_labels(ir),
        claimed: super::topology_claimed(ir),
        entities,
        targets,
        result: NativeExport {
            regenerated: already_generated,
            losses: Vec::new(),
        },
    };
    let linked_primitives = &linked_primitives;
    let mut pending = ir
        .native
        .namespace("iges")
        .into_iter()
        .flat_map(|namespace| {
            namespace
                .arenas()
                .iter()
                .filter(|(arena, _)| ARENAS.contains(&arena.as_str()))
                .flat_map(|(arena, records)| {
                    records
                        .iter()
                        .filter(move |record| !linked_primitives.contains(record.id()))
                        .map(move |record| (arena.as_str(), record))
                })
        })
        .collect::<Vec<_>>();
    // Re-emit in Directory order: the re-decode numbers feature and
    // occurrence ordinals in emission order, so a reordered re-emission
    // permutes them. Records without a source sequence sort last.
    pending.sort_by_key(|(_, record)| {
        (
            directory_sequence(record).is_none(),
            directory_sequence(record),
        )
    });
    // Boolean trees that project to features follow their current feature
    // ordinals, while the other native records keep their Directory order.
    let feature_ordinals = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|native| (native, feature.ordinal))
        })
        .collect::<BTreeMap<_, _>>();
    let mut booleans = pending
        .iter()
        .copied()
        .filter(|(arena, _)| *arena == "boolean_trees")
        .collect::<Vec<_>>();
    booleans.sort_by_key(|(_, record)| {
        let ordinal = feature_ordinals.get(record.id()).copied();
        (ordinal.is_none(), ordinal, directory_sequence(record))
    });
    let mut ordered = booleans.into_iter();
    for slot in &mut pending {
        if slot.0 == "boolean_trees" {
            *slot = ordered.next().expect("each Boolean slot was counted");
        }
    }
    // Records emitted before every reference they name resolves carry null
    // pointers until a re-encode fills them: each entry names the entity
    // index holding the optimistic encoding with its arena and record.
    let mut optimistic: Vec<(usize, &str, &NativeRecord)> = Vec::new();
    loop {
        let before = pending.len();
        let mut deferred = Vec::new();
        for (arena, record) in pending {
            if let Push::Deferred = emitter.try_push(arena, record) {
                deferred.push((arena, record));
            }
        }
        pending = deferred;
        if emitter.reencode(&optimistic) || pending.len() != before {
            if pending.is_empty() {
                break;
            }
            continue;
        }
        // No strict pass emits: classify each pending record with a lenient
        // encoding that nulls unresolvable references.
        let ids: BTreeSet<String> = pending
            .iter()
            .map(|(_, record)| record.id().to_owned())
            .collect();
        let mut stalled = Vec::new();
        let mut ready = Vec::new();
        let mut dropped_any = false;
        for (arena, record) in pending {
            match emitter.classify(arena, record, &ids) {
                Class::Drop => dropped_any = true,
                Class::Wait => stalled.push((arena, record)),
                Class::Ready(entity, owners) => {
                    ready.push((arena, record, *entity, owners));
                }
            }
        }
        pending = stalled;
        if ready.is_empty() {
            // Every remaining record waits on a transformation target that
            // never lands, or nothing remains.
            break;
        }
        if dropped_any {
            pending.extend(
                ready
                    .into_iter()
                    .map(|(arena, record, _, _)| (arena, record)),
            );
            continue;
        }
        // Every remaining record is valid data blocked only on emission
        // order: back-pointer cycles across native records. Emit the first
        // in Directory order with nulls for its still-missing references;
        // later passes re-encode it once they land.
        let (arena, record, entity, owners) = ready.remove(0);
        for (arena, record, _, _) in ready {
            pending.push((arena, record));
        }
        emitter.push_entity(arena, record, entity, owners);
        optimistic.push((emitter.entities.len() - 1, arena, record));
    }
    let Emitter {
        entities,
        targets,
        result,
        ..
    } = emitter;
    line_fonts(ir, entities, &targets);
    associations::link(ir, entities, &targets);
    result
}
/// Emission state for one native re-emission pass: the read-only document
/// context plus the entities, reference targets and dispositions the passes
/// extend.
struct Emitter<'ir, 'out> {
    ir: &'ir CadIr,
    version: crate::IgesVersion,
    labels: BTreeMap<String, String>,
    claimed: BTreeSet<u32>,
    entities: &'out mut Vec<Entity>,
    targets: Targets,
    result: NativeExport,
}

/// The outcome of one strict emission attempt.
enum Push {
    Emitted,
    Deferred,
}

impl Emitter<'_, '_> {
    /// Try a strict emission: the record lands only when every reference,
    /// owner and transformation target it names is already emitted.
    fn try_push(&mut self, arena: &str, record: &NativeRecord) -> Push {
        let Ok((mut entity, owners, _)) =
            encode(self.ir, arena, record, &self.targets, &self.claimed, false)
        else {
            return Push::Deferred;
        };
        if !self.version.admits(&entity) {
            return Push::Deferred;
        }
        if entity.transform.is_none() {
            if let Some(id) = record
                .field("transformation")
                .and_then(|value| value.as_str().map(str::to_owned))
            {
                match self.targets.get(&id).map(Vec::as_slice) {
                    Some([index]) => entity.transform_link = Some(*index),
                    _ => return Push::Deferred,
                }
            }
        }
        self.push_entity(arena, record, entity, owners);
        Push::Emitted
    }

    fn push_entity(
        &mut self,
        arena: &str,
        record: &NativeRecord,
        mut entity: Entity,
        owners: Vec<usize>,
    ) {
        // A re-emitted boundary carrier keeps its source dependence: an
        // independent copy mints standalone topology on re-decode.
        if matches!(
            arena,
            "annotations" | "drawings" | "views" | "view_visibility"
        ) {
            entity.status =
                super::source_status(self.ir, record).unwrap_or(EntityStatus::Annotation);
        } else if arena == "copious_data" {
            if let Some(status) = super::source_status(self.ir, record) {
                entity.status = status;
            }
        }
        if matches!(
            arena,
            "subfigure_definitions"
                | "network_definitions"
                | "subfigure_instances"
                | "network_instances"
                | "connect_points"
        ) {
            if let Some(label) = source_directory_label(self.ir, record) {
                entity.label = label;
            }
        } else if let Some(label) = self
            .labels
            .get(record.id())
            .and_then(|name| directory_label(name))
        {
            entity.label = label;
        }
        let index = self.entities.len();
        for id in &entity.source_targets {
            self.targets.entry(id.clone()).or_default().push(index);
        }
        for owner in owners {
            self.entities[owner].property_links.push(index);
        }
        self.entities.push(entity);
        self.result.regenerated.insert(record.id().to_owned());
    }

    /// Classify a record no strict pass emits. A lenient encoding nulls
    /// unresolvable references; what remains missing decides the class.
    fn classify(&self, arena: &str, record: &NativeRecord, pending: &BTreeSet<String>) -> Class {
        let (entity, owners, missing) =
            match encode(self.ir, arena, record, &self.targets, &self.claimed, true) {
                Ok(ok) => ok,
                Err(error) => {
                    // An owner that still waits for emission keeps the record
                    // waiting; any other encoding error drops it.
                    if let Some(owner) = error.strip_prefix("owner-wait:") {
                        if pending.contains(owner) {
                            return Class::Wait;
                        }
                    }
                    return Class::Drop;
                }
            };
        if !self.version.admits(&entity) {
            return Class::Drop;
        }
        if entity.transform.is_none() {
            if let Some(id) = record
                .field("transformation")
                .and_then(|value| value.as_str().map(str::to_owned))
            {
                if !matches!(self.targets.get(&id).map(Vec::as_slice), Some([_])) {
                    return Class::Wait;
                }
            }
        }
        for id in &missing {
            // A pending identity with no emitted target yet lands on a later
            // pass. Anything else never resolves to one target: an absent
            // identity never lands, and two targets never narrow to one.
            if pending.contains(id)
                && !matches!(self.targets.get(id).map(Vec::as_slice), Some([_, _, ..]))
            {
                continue;
            }
            return Class::Drop;
        }
        Class::Ready(Box::new(entity), owners)
    }

    /// Re-encode optimistically emitted records now that more targets landed.
    /// Only the parameter body changes: identities, labels, links and the
    /// transformation attachment stay as pushed.
    fn reencode(&mut self, entries: &[(usize, &str, &NativeRecord)]) -> bool {
        let mut progressed = false;
        for (index, arena, record) in entries {
            let Ok((fresh, _, _)) =
                encode(self.ir, arena, record, &self.targets, &self.claimed, false)
            else {
                continue;
            };
            if self.entities[*index].parameter_body != fresh.parameter_body {
                self.entities[*index].parameter_body = fresh.parameter_body;
                progressed = true;
            }
        }
        progressed
    }
}

/// A stalled record: either invalid data, still waiting on a
/// transformation target, or valid data blocked only on emission order.
enum Class {
    Drop,
    Wait,
    Ready(Box<Entity>, Vec<usize>),
}
