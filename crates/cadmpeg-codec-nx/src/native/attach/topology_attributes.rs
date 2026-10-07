// SPDX-License-Identifier: Apache-2.0
//! Projection of Parasolid topology attributes into IR attributes.

use crate::decode::ids::IdScope;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

pub(super) struct ParasolidStringAttributeSources<'a> {
    pub(super) string_uses: &'a [crate::native::parasolid::ParasolidEntity51StringUse],
    pub(super) strings: &'a [crate::native::parasolid::ParasolidEntity54StringRecord],
}

pub(super) fn attach_parasolid_topology_string_attributes(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &ParasolidStringAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_, '_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let (strings_by_id, _strings_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .strings
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid string record index",
    )?;
    let (mut uses_by_entity, _uses_by_entity_reservation) = ctx.collect_scoped_btree_groups(
        sources
            .string_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
        "NX Parasolid string use groups",
    )?;
    for (_, uses) in ctx.admit_iter(
        &mut uses_by_entity,
        "NX Parasolid string use group traversal",
    )? {
        ctx.stable_sort_by(
            uses,
            |value| &value.position,
            Ord::cmp,
            "NX Parasolid string attribute ordering",
        )?;
    }
    for context in ctx.admit_iter(&attribute_index.contexts, "NX string attribute contexts")? {
        let reference = context.reference;
        let entity = context.entity;
        if let Some(feature_property_records) = ctx.get_btree_map(
            &uses_by_entity,
            entity,
            "NX attach parasolid topology string attributes uses by entity lookup",
        )? {
            for string_use in
                ctx.admit_iter(feature_property_records, "NX string attribute uses")?
            {
                let Some(string) = ctx.get_btree_map(
                    &strings_by_id,
                    string_use.string_record.as_str(),
                    "NX attach parasolid topology string attributes strings by id lookup",
                )?
                else {
                    continue;
                };
                let id = topology_attribute_id(
                    ctx,
                    reference,
                    &cadmpeg_ir::identity_component!("topology-string-attribute"),
                    string_use.position.reference_ordinal(),
                    context.id_suffix.as_ref(),
                )?;
                let source_stream = StreamHandle::new(
                    ctx,
                    cadmpeg_ir::stream_name!("nx:s").with_suffix(
                        ctx,
                        reference.stream_ordinal,
                        "compose annotation stream name",
                    )?,
                    "allocate annotation stream handle",
                )?;
                annotations.note(
                    ctx,
                    id.as_str(),
                    &source_stream,
                    string.inflated_offset,
                    Some("ENTITY_54_STRING_ATTRIBUTE"),
                )?;
                annotations
                    .derived(ctx, id.as_str(), "target")
                    .map_err(cadmpeg_core::CodecError::from)?;
                annotations
                    .derived(ctx, id.as_str(), "name")
                    .map_err(cadmpeg_core::CodecError::from)?;
                let field_name = attribute_index.attribute_names.field_name(
                    ctx,
                    reference,
                    string_use.id.as_str(),
                )?;
                let name = topology_attribute_name(
                    ctx,
                    field_name,
                    ctx.get_btree_map(
                        &attribute_index.class_names,
                        reference.id.as_str(),"NX attach parasolid topology string attributes attribute index class names lookup",
                    )?
                    .and_then(Option::as_ref)
                    .copied(),
                    "84",
                    string_use.position.reference_ordinal(),
                )?;
                let values = single_string_attribute_values(ctx, string.value.as_str())?;
                push_topology_attribute(ctx, ir, context, id, name, values)?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut ir.model.attributes,
        |value| value.id.as_str(),
        Ord::cmp,
        "sort NX Parasolid string attributes",
    )?;
    Ok(())
}

pub(super) struct ParasolidNumericAttributeSources<'a> {
    pub(super) numeric_uses: &'a [crate::native::parasolid::ParasolidEntity51NumericUse],
    pub(super) integers: &'a [crate::native::parasolid::ParasolidEntity52IntegerRecord],
    pub(super) doubles: &'a [crate::native::parasolid::ParasolidEntity53DoubleRecord],
}

struct ParasolidAttributeNameIndex<'a> {
    classes_by_entity: BTreeMap<
        (&'a str, &'a str),
        Option<&'a crate::native::parasolid::ParasolidTopologyAttributeClassUse>,
    >,
    fields_by_value_use:
        BTreeMap<&'a str, Option<&'a crate::native::parasolid::ParasolidAttributeFieldUse>>,
    definitions_by_id:
        BTreeMap<&'a str, Option<&'a crate::native::parasolid::ParasolidAttributeDefinition>>,
    field_names_by_definition:
        BTreeMap<&'a str, Option<&'a crate::native::parasolid::ParasolidAttributeFieldNames>>,
}

impl<'a> ParasolidAttributeNameIndex<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
        definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
        field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
        field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    ) -> Result<Self, CodecError> {
        let mut classes_by_entity = BTreeMap::new();
        for class_use in ctx.admit_iter(class_uses, "NX attribute class uses")? {
            insert_sole(
                ctx,
                reservation,
                &mut classes_by_entity,
                (
                    class_use.topology_attribute_reference.as_str(),
                    class_use.entity_51_record.as_str(),
                ),
                class_use,
                "NX attribute class identity index",
            )?;
        }

        let mut fields_by_value_use = BTreeMap::new();
        for field_use in ctx.admit_iter(field_uses, "NX attribute field uses")? {
            insert_sole(
                ctx,
                reservation,
                &mut fields_by_value_use,
                field_use.value_use.as_str(),
                field_use,
                "NX attribute value-use identity index",
            )?;
        }

        let mut definitions_by_id = BTreeMap::new();
        for definition in ctx.admit_iter(definitions, "NX attribute definitions")? {
            insert_sole(
                ctx,
                reservation,
                &mut definitions_by_id,
                definition.id.as_str(),
                definition,
                "NX attribute definition identity index",
            )?;
        }

        let mut field_names_by_definition = BTreeMap::new();
        for names in ctx.admit_iter(field_names, "NX attribute field names")? {
            insert_sole(
                ctx,
                reservation,
                &mut field_names_by_definition,
                names.attribute_definition.as_str(),
                names,
                "NX attribute field-name definition index",
            )?;
        }

        Ok(Self {
            classes_by_entity,
            fields_by_value_use,
            definitions_by_id,
            field_names_by_definition,
        })
    }

    fn field_name(
        &self,
        ctx: &DecodeContext<'_>,
        topology_reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
        value_use: &str,
    ) -> Result<Option<String>, CodecError> {
        let Some(field_use) = ctx
            .get_btree_map(
                &self.fields_by_value_use,
                value_use,
                "NX field name self fields by value use lookup",
            )?
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let Some(class_use) = ctx
            .get_btree_map(
                &self.classes_by_entity,
                &(
                    topology_reference.id.as_str(),
                    field_use.entity_51_record.as_str(),
                ),
                "NX field name self classes by entity lookup",
            )?
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        if !ctx.equal_bytes(
            field_use.attribute_class_use.as_bytes(),
            class_use.attribute_class_use.as_bytes(),
            "NX attribute field class-use identity equality",
        )? || !ctx.equal_bytes(
            field_use.attribute_definition.as_bytes(),
            class_use.attribute_definition.as_bytes(),
            "NX attribute field definition identity equality",
        )? {
            return Ok(None);
        }
        let Some(definition) = ctx
            .get_btree_map(
                &self.definitions_by_id,
                class_use.attribute_definition.as_str(),
                "NX field name self definitions by id lookup",
            )?
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let mut field_reservation = ctx.reserve_scoped(0, "NX Parasolid field name component")?;
        let field_name = match (definition.name.as_str(), field_use.position.field_ordinal()) {
            ("SDL/TYSA_DENSITY", 0) => std::borrow::Cow::Borrowed("density"),
            ("SDL/TYSA_DENSITY", 1) => std::borrow::Cow::Borrowed("units"),
            _ => {
                let names = ctx
                    .get_btree_map(
                        &self.field_names_by_definition,
                        definition.id.as_str(),
                        "NX Parasolid field names lookup",
                    )?
                    .and_then(Option::as_ref);
                if let Some(names) = names {
                    let Some(name) = names
                        .fields
                        .get(cadmpeg_core::decode::index_from_u32(
                            field_use.position.field_ordinal(),
                        ))
                        .map(|field| field.name.as_str())
                    else {
                        return Ok(None);
                    };
                    std::borrow::Cow::Borrowed(name)
                } else {
                    std::borrow::Cow::Owned(ctx.format_scoped_text(
                        &mut field_reservation,
                        format_args!(
                            "field_{}.parasolid_type_{}",
                            field_use.position.field_ordinal(),
                            field_use.value_kind.field_code().code()
                        ),
                        "NX Parasolid field name component",
                    )?)
                }
            }
        };
        let name_len = definition
            .name
            .as_str()
            .len()
            .checked_add(1)
            .and_then(|bytes| bytes.checked_add(field_name.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX Parasolid attribute field name",
                    0,
                    cadmpeg_core::decode::u64_from_index(field_name.len()),
                )
            })?;
        let mut name = ctx.retained_string(name_len, "NX Parasolid attribute field name")?;
        ctx.append_retained(
            &mut name,
            definition.name.as_str(),
            "NX field name definition name append",
        )?;
        ctx.push_retained_char(&mut name, '.', "NX Parasolid attribute field separator")?;
        ctx.append_retained(&mut name, &field_name, "NX field name field name append")?;
        Ok(Some(name))
    }
}

fn topology_attribute_name(
    ctx: &DecodeContext<'_>,
    field_name: Option<String>,
    class_name: Option<&str>,
    family: &str,
    reference_ordinal: u32,
) -> Result<String, CodecError> {
    if let Some(name) = field_name {
        return Ok(name);
    }
    let mut name = String::new();
    if let Some(class_name) = class_name {
        ctx.append_retained(&mut name, class_name, "NX Parasolid attribute class name")?;
        ctx.push_retained_char(&mut name, '.', "NX Parasolid attribute class separator")?;
    }
    ctx.append_formatted_retained(
        &mut name,
        format_args!("parasolid_type_{family}_reference_{reference_ordinal}"),
        "NX Parasolid attribute fallback name",
    )?;

    Ok(name)
}

/// Records `value` as the sole value for `key`, or `None` once the key repeats.
fn insert_sole<'a, K: Ord + cadmpeg_core::decode::cost::DecodeCost, V>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    values: &mut BTreeMap<K, Option<&'a V>>,
    key: K,
    value: &'a V,
    operation: &'static str,
) -> Result<(), CodecError> {
    match reservation.with_storage(|| ctx.entry_btree_map(values, key, operation))? {
        Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        Entry::Occupied(mut entry) => {
            entry.insert(None);
        }
    }
    Ok(())
}

fn parasolid_topology_attribute_class_names<'a>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
) -> Result<BTreeMap<&'a str, Option<&'a str>>, CodecError> {
    let mut classes_by_reference = BTreeMap::<&str, Option<&str>>::new();
    for class_use in ctx.admit_iter(class_uses, "NX topology attribute class uses")? {
        for definition in ctx.admit_iter(definitions, "NX Parasolid class name lookup")? {
            if !(ctx.equal_bytes(
                definition.id.as_bytes(),
                class_use.attribute_definition.as_bytes(),
                "NX attribute class definition identity equality",
            )?) {
                continue;
            }

            let key = class_use.topology_attribute_reference.as_str();
            let name = definition.name.as_str();
            match reservation.with_storage(|| {
                ctx.entry_btree_map(
                    &mut classes_by_reference,
                    key,
                    "NX parasolid topology attribute class names classes by reference entry",
                )
            })? {
                Entry::Vacant(entry) => {
                    entry.insert(Some(name));
                }
                Entry::Occupied(mut entry) => {
                    if match entry.get() {
                        Some(existing) => !ctx.equal_bytes(
                            existing.as_bytes(),
                            name.as_bytes(),
                            "NX attribute class name ambiguity equality",
                        )?,
                        None => false,
                    } {
                        entry.insert(None);
                    }
                }
            }
        }
    }
    Ok(classes_by_reference)
}

fn parasolid_topology_attribute_targets(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ir: &CadIr,
) -> Result<BTreeMap<String, AttributeTarget>, CodecError> {
    let mut targets = BTreeMap::new();
    for shell in ctx.admit_iter(&ir.model.shells, "NX topology attribute shells")? {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            shell.id.as_str(),
            || {
                shell
                    .id
                    .try_clone_for_decode(ctx, "NX attribute shell target identity")
                    .map(AttributeTarget::Shell)
            },
        )?;
    }
    for face in ctx.admit_iter(&ir.model.faces, "NX topology attribute faces")? {
        insert_parasolid_topology_target(ctx, reservation, &mut targets, face.id.as_str(), || {
            face.id
                .try_clone_for_decode(ctx, "NX attribute face target identity")
                .map(AttributeTarget::Face)
        })?;
    }
    for loop_ in ctx.admit_iter(&ir.model.loops, "NX topology attribute loops")? {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            loop_.id.as_str(),
            || {
                loop_
                    .id
                    .try_clone_for_decode(ctx, "NX attribute loop target identity")
                    .map(AttributeTarget::Loop)
            },
        )?;
    }
    for edge in ctx.admit_iter(&ir.model.edges, "NX topology attribute edges")? {
        insert_parasolid_topology_target(ctx, reservation, &mut targets, edge.id.as_str(), || {
            edge.id
                .try_clone_for_decode(ctx, "NX attribute edge target identity")
                .map(AttributeTarget::Edge)
        })?;
    }
    for coedge in ctx.admit_iter(&ir.model.coedges, "NX topology attribute coedges")? {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            coedge.id.as_str(),
            || {
                coedge
                    .id
                    .try_clone_for_decode(ctx, "NX attribute coedge target identity")
                    .map(AttributeTarget::Coedge)
            },
        )?;
    }
    for vertex in ctx.admit_iter(&ir.model.vertices, "NX topology attribute vertices")? {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            vertex.id.as_str(),
            || {
                vertex
                    .id
                    .try_clone_for_decode(ctx, "NX attribute vertex target identity")
                    .map(AttributeTarget::Vertex)
            },
        )?;
    }
    Ok(targets)
}

fn insert_parasolid_topology_target(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    targets: &mut BTreeMap<String, AttributeTarget>,
    id: &str,
    target: impl FnOnce() -> Result<AttributeTarget, CodecError>,
) -> Result<(), CodecError> {
    let mut key = String::new();
    reservation.with_storage(|| {
        ctx.try_reserve_retained_text(
            &mut key,
            id.len(),
            "allocate NX Parasolid topology target key",
        )
    })?;
    ctx.append_retained(
        &mut key,
        id,
        "NX insert parasolid topology target id append",
    )?;
    let target = reservation.with_storage(target)?;
    reservation.with_storage(|| {
        ctx.insert_btree_map(targets, key, target, "NX Parasolid topology targets")
    })?;
    Ok(())
}

struct ParasolidTopologyAttributeContext<'a> {
    reference: &'a crate::native::parasolid::ParasolidTopologyAttributeListReference,
    entity: &'a str,
    id_suffix: Option<cadmpeg_ir::ids::IdentityKey>,
    target: AttributeTarget,
}

pub(super) struct ParasolidTopologyAttributeIndex<'a, 'ctx> {
    class_names: BTreeMap<&'a str, Option<&'a str>>,
    attribute_names: ParasolidAttributeNameIndex<'a>,
    contexts: Vec<ParasolidTopologyAttributeContext<'a>>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> ParasolidTopologyAttributeIndex<'a, 'ctx> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        ir: &CadIr,
        topology_references:
            &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
        class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
        definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
        field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
        field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    ) -> Result<Self, CodecError> {
        let mut reservation = ctx.reserve_scoped(0, "NX Parasolid attribute indexes")?;
        let attribute_names = ParasolidAttributeNameIndex::new(
            ctx,
            &mut reservation,
            class_uses,
            definitions,
            field_uses,
            field_names,
        )?;
        let class_names = parasolid_topology_attribute_class_names(
            ctx,
            &mut reservation,
            class_uses,
            definitions,
        )?;
        Ok(Self {
            class_names,
            attribute_names,
            contexts: parasolid_topology_attribute_contexts(
                ctx,
                &mut reservation,
                ir,
                topology_references,
                class_uses,
            )?,
            _reservation: reservation,
        })
    }
}

fn parasolid_topology_attribute_contexts<'a>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ir: &CadIr,
    topology_references: &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
) -> Result<Vec<ParasolidTopologyAttributeContext<'a>>, CodecError> {
    let mut entities_by_reference = BTreeMap::<&str, BTreeSet<&str>>::new();
    for class_use in ctx.admit_iter(class_uses, "NX attribute context class uses")? {
        let key = class_use.topology_attribute_reference.as_str();
        let entities = reservation
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut entities_by_reference,
                    key,
                    "NX parasolid topology attribute contexts entities by reference entry",
                )
            })?
            .or_default();
        reservation.with_storage(|| {
            ctx.insert_btree_set(
                entities,
                class_use.entity_51_record.as_str(),
                "NX Parasolid attribute group entity",
            )
        })?;
    }
    let mut references_by_target = BTreeMap::<String, Vec<_>>::new();
    for reference in ctx.admit_iter(topology_references, "NX attribute topology references")? {
        let kind = reference.topology_type.as_str();
        let key = ctx.format_scoped_text(
            reservation,
            format_args!(
                "nx:s{}:{kind}#{}",
                reference.stream_ordinal, reference.topology_xmt
            ),
            "NX Parasolid topology reference key",
        )?;
        let references = reservation
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut references_by_target,
                    key,
                    "NX parasolid topology attribute contexts references by target entry",
                )
            })?
            .or_default();
        reservation.with_storage(|| {
            ctx.push_vec(references, reference, "NX Parasolid topology reference")
        })?;
    }
    let emitted_targets = parasolid_topology_attribute_targets(ctx, reservation, ir)?;
    let mut contexts = Vec::new();
    for (target_key, references) in
        ctx.admit_iter(&references_by_target, "NX attribute reference targets")?
    {
        let Some(&reference) = references.first().filter(|_| references.len() == 1) else {
            continue;
        };
        let Some(target) = ctx.get_btree_map(
            &emitted_targets,
            target_key.as_str(),
            "NX parasolid topology attribute contexts emitted targets lookup",
        )?
        else {
            continue;
        };
        let mut entities = BTreeSet::new();
        if let Some(entity) = reference.attribute_list_record.as_deref() {
            reservation.with_storage(|| {
                ctx.insert_btree_set(&mut entities, entity, "NX Parasolid reference entity")
            })?;
        }
        if let Some(class_entities) = ctx.get_btree_map(
            &entities_by_reference,
            reference.id.as_str(),
            "NX parasolid topology attribute contexts entities by reference lookup",
        )? {
            for entity in ctx.admit_iter(class_entities, "NX Parasolid class entity members")? {
                reservation.with_storage(|| {
                    ctx.insert_btree_set(&mut entities, entity, "NX Parasolid reference entity")
                })?;
            }
        }
        let multiple_entities = entities.len() > 1;
        for entity in ctx.admit_iter(entities, "NX Parasolid context entities")? {
            let id_suffix = if multiple_entities {
                entity_suffix_key(ctx, reservation, entity)?
            } else {
                None
            };
            ctx.charge_collection_items(1, "NX Parasolid attribute contexts")?;
            reservation.with_storage(|| {
                ctx.reserve_capacity(&mut contexts, 1, "NX Parasolid attribute contexts")
            })?;
            contexts.push(ParasolidTopologyAttributeContext {
                reference,
                entity,
                id_suffix,
                target: reservation.with_storage(|| {
                    target.try_clone_for_decode(ctx, "NX Parasolid attribute context")
                })?,
            });
        }
    }
    Ok(contexts)
}

fn topology_attribute_id(
    ctx: &DecodeContext<'_>,
    reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
    family: &cadmpeg_ir::ids::IdentityComponent,
    reference_ordinal: u32,
    entity_suffix: Option<&cadmpeg_ir::ids::IdentityKey>,
) -> Result<AttributeId, CodecError> {
    let suffix_len = entity_suffix.map_or(0, |suffix| suffix.as_str().len());
    let id_len = family
        .as_str()
        .len()
        .checked_add(suffix_len)
        .and_then(|bytes| bytes.checked_add(64))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid attribute identity",
                0,
                cadmpeg_core::decode::u64_from_index(suffix_len),
            )
        })?;
    let bytes = id_len.checked_mul(4).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX Parasolid attribute identity",
            0,
            cadmpeg_core::decode::u64_from_index(id_len),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(id_len),
        "NX Parasolid attribute identity",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(bytes),
        "NX Parasolid attribute identity",
    )?;
    let mut key = cadmpeg_ir::ids::IdentityKey::from(reference.topology_type.code())
        .dash(reference.topology_xmt)
        .dash(reference_ordinal);
    if let Some(suffix) = entity_suffix {
        key = key.dash(suffix);
    }
    Ok(IdScope::stream(reference.stream_ordinal).id(family, key))
}

fn single_string_attribute_values(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<Vec<AttributeValue>, CodecError> {
    ctx.charge_collection_items(1, "NX Parasolid string attribute values")?;

    let mut owned = String::new();
    ctx.try_reserve_retained_text(
        &mut owned,
        text.len(),
        "NX Parasolid string attribute value",
    )?;
    ctx.append_retained(
        &mut owned,
        text,
        "NX single string attribute values text append",
    )?;
    let mut values = Vec::new();
    ctx.reserve_capacity(&mut values, 1, "NX Parasolid string attribute values")?;
    values.push(AttributeValue::String(owned));
    Ok(values)
}

fn mapped_attribute_values<T>(
    ctx: &DecodeContext<'_>,
    input: &[T],
    map: impl Fn(&T) -> AttributeValue,
) -> Result<Vec<AttributeValue>, CodecError> {
    let mut values = ctx.collection_vec(input.len(), "NX Parasolid numeric attribute values")?;
    values.extend(
        ctx.admit_iter(input, "NX numeric attribute value traversal")?
            .map(map),
    );
    Ok(values)
}

fn mapped_vector_attribute_values<T, const N: usize>(
    ctx: &DecodeContext<'_>,
    input: &[T],
    map: impl Fn(&T) -> [FiniteReal; N],
) -> Result<Vec<AttributeValue>, CodecError> {
    let items = input
        .len()
        .checked_mul(N.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid vector value items",
                0,
                cadmpeg_core::decode::u64_from_index(N),
            )
        })?)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid vector value items",
                0,
                cadmpeg_core::decode::u64_from_index(input.len()),
            )
        })?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(items),
        "NX Parasolid vector value items",
    )?;
    let mut values = Vec::new();
    ctx.reserve_capacity(&mut values, input.len(), "NX Parasolid vector values")?;
    for item in ctx.admit_iter(input, "NX vector attribute values")? {
        let mut components = Vec::new();
        ctx.reserve_capacity(&mut components, N, "NX Parasolid vector components")?;
        components.extend(map(item));
        values.push(AttributeValue::Vector(components));
    }
    Ok(values)
}

fn push_topology_attribute(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    context: &ParasolidTopologyAttributeContext<'_>,
    id: AttributeId,
    name: String,
    values: Vec<AttributeValue>,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "NX Parasolid attribute output")?;
    ctx.reserve_capacity(&mut ir.model.attributes, 1, "NX Parasolid attribute output")?;
    ir.model.attributes.push(SourceAttribute {
        id,
        target: context
            .target
            .try_clone_for_decode(ctx, "NX Parasolid attribute output")?,
        name,
        values,
    });
    Ok(())
}

/// The key half of an entity reference, which is what an id suffix names.
///
/// The reference reaches this as stored record text, so it is admitted here;
/// text that is not key text names no suffix.
fn entity_suffix_key(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    entity: &str,
) -> Result<Option<cadmpeg_ir::ids::IdentityKey>, CodecError> {
    let suffix = ctx
        .rsplit_once(entity, "#", "NX Parasolid entity suffix search")?
        .map_or(entity, |(_, key)| key);
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(suffix.len()),
        "NX Parasolid entity suffix key",
    )?;
    reservation.grow(cadmpeg_core::decode::u64_from_index(suffix.len()))?;
    let Ok(key) = cadmpeg_ir::ids::IdentityKey::try_new(suffix) else {
        return Ok(None);
    };
    Ok(Some(key))
}

pub(super) fn attach_parasolid_topology_numeric_attributes(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &ParasolidNumericAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_, '_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let (integers_by_id, _integers_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .integers
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid integer record index",
    )?;
    let (doubles_by_id, _doubles_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .doubles
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid double record index",
    )?;
    let (mut uses_by_entity, _uses_by_entity_reservation) = ctx.collect_scoped_btree_groups(
        sources
            .numeric_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
        "NX Parasolid numeric use groups",
    )?;
    for (_, uses) in ctx.admit_iter(
        &mut uses_by_entity,
        "NX Parasolid numeric use group traversal",
    )? {
        ctx.stable_sort_by(
            uses,
            |value| &value.position,
            Ord::cmp,
            "NX Parasolid numeric attribute ordering",
        )?;
    }
    for context in ctx.admit_iter(&attribute_index.contexts, "NX numeric attribute contexts")? {
        let reference = context.reference;
        let entity = context.entity;
        if let Some(feature_property_records) = ctx.get_btree_map(
            &uses_by_entity,
            entity,
            "NX attach parasolid topology numeric attributes uses by entity lookup",
        )? {
            for numeric_use in
                ctx.admit_iter(feature_property_records, "NX numeric attribute uses")?
            {
                let (values, source_offset, tag, lane) = match numeric_use.kind {
                    crate::native::parasolid::ParasolidEntity51NumericKind::UnsignedIntegers => {
                        let Some(record) = ctx.get_btree_map(
                            &integers_by_id,
                            numeric_use.value_record.as_str(),
                            "NX attach parasolid topology numeric attributes integers by id lookup",
                        )?
                        else {
                            continue;
                        };
                        (
                            mapped_attribute_values(ctx, record.values.as_slice(), |value| {
                                AttributeValue::Integer(i64::from(*value))
                            })?,
                            record.inflated_offset,
                            "ENTITY_52_INTEGER_ATTRIBUTE",
                            "integer",
                        )
                    }
                    crate::native::parasolid::ParasolidEntity51NumericKind::Doubles => {
                        let Some(record) = ctx.get_btree_map(
                            &doubles_by_id,
                            numeric_use.value_record.as_str(),
                            "NX attach parasolid topology numeric attributes doubles by id lookup",
                        )?
                        else {
                            continue;
                        };
                        (
                            mapped_attribute_values(ctx, record.values.as_slice(), |value| {
                                AttributeValue::Float(*value)
                            })?,
                            record.inflated_offset,
                            "ENTITY_53_DOUBLE_ATTRIBUTE",
                            "double",
                        )
                    }
                };
                let id = topology_attribute_id(
                    ctx,
                    reference,
                    &cadmpeg_ir::identity_component!("topology-numeric-attribute"),
                    numeric_use.position.reference_ordinal(),
                    context.id_suffix.as_ref(),
                )?;
                let source_stream = StreamHandle::new(
                    ctx,
                    cadmpeg_ir::stream_name!("nx:s").with_suffix(
                        ctx,
                        reference.stream_ordinal,
                        "compose annotation stream name",
                    )?,
                    "allocate annotation stream handle",
                )?;
                annotations.note(ctx, id.as_str(), &source_stream, source_offset, Some(tag))?;
                annotations
                    .derived(ctx, id.as_str(), "target")
                    .map_err(cadmpeg_core::CodecError::from)?;
                annotations
                    .derived(ctx, id.as_str(), "name")
                    .map_err(cadmpeg_core::CodecError::from)?;
                let field_name = attribute_index.attribute_names.field_name(
                    ctx,
                    reference,
                    numeric_use.id.as_str(),
                )?;
                let name = topology_attribute_name(
                    ctx,
                    field_name,
                    ctx.get_btree_map(
                        &attribute_index.class_names,
                        reference.id.as_str(),"NX attach parasolid topology numeric attributes attribute index class names lookup",
                    )?
                    .and_then(Option::as_ref)
                    .copied(),
                    lane,
                    numeric_use.position.reference_ordinal(),
                )?;
                push_topology_attribute(ctx, ir, context, id, name, values)?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut ir.model.attributes,
        |value| value.id.as_str(),
        Ord::cmp,
        "sort NX Parasolid numeric attributes",
    )?;
    Ok(())
}

pub(super) struct ParasolidStructuredAttributeSources<'a> {
    pub(super) structured_uses: &'a [crate::native::parasolid::ParasolidEntity51StructuredUse],
    pub(super) vectors: &'a [crate::native::parasolid::ParasolidEntityVectorRecord],
    pub(super) axes: &'a [crate::native::parasolid::ParasolidEntity57AxisRecord],
    pub(super) tags: &'a [crate::native::parasolid::ParasolidEntity58TagRecord],
    pub(super) unicode: &'a [crate::native::parasolid::ParasolidEntity62UnicodeRecord],
}

pub(super) fn attach_parasolid_topology_structured_attributes(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &ParasolidStructuredAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_, '_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let (vectors_by_id, _vectors_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .vectors
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid vector record index",
    )?;
    let (axes_by_id, _axes_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .axes
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid axis record index",
    )?;
    let (tags_by_id, _tags_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .tags
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid tag record index",
    )?;
    let (unicode_by_id, _unicode_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .unicode
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid Unicode record index",
    )?;
    let (mut uses_by_entity, _uses_by_entity_reservation) = ctx.collect_scoped_btree_groups(
        sources
            .structured_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
        "NX Parasolid structured use groups",
    )?;
    for (_, uses) in ctx.admit_iter(
        &mut uses_by_entity,
        "NX Parasolid structured use group traversal",
    )? {
        ctx.stable_sort_by(
            uses,
            |value| &value.position,
            Ord::cmp,
            "NX Parasolid structured attribute ordering",
        )?;
    }
    for context in ctx.admit_iter(
        &attribute_index.contexts,
        "NX structured attribute contexts",
    )? {
        let reference = context.reference;
        let entity = context.entity;
        if let Some(feature_property_records) = ctx.get_btree_map(
            &uses_by_entity,
            entity,
            "NX attach parasolid topology structured attributes uses by entity lookup",
        )? {
            for structured_use in
                ctx.admit_iter(feature_property_records, "NX structured attribute uses")?
            {
                use crate::native::parasolid::structured_value_kind::StructuredValueKind as Kind;
                use crate::native::parasolid::ParasolidVectorValueKind;
                let (values, source_offset, tag, family) = match structured_use.kind {
                    Kind::Points | Kind::Vectors | Kind::Directions => {
                        let Some(record) = ctx.get_btree_map(
                            &vectors_by_id,
                            structured_use.value_record.as_str(),"NX attach parasolid topology structured attributes vectors by id lookup",
                        )?
                        else {
                            continue;
                        };
                        let family = match (structured_use.kind, record.kind) {
                            (Kind::Points, ParasolidVectorValueKind::Points) => "85_point",
                            (Kind::Vectors, ParasolidVectorValueKind::Vectors) => "86_vector",
                            (Kind::Directions, ParasolidVectorValueKind::Directions) => {
                                "89_direction"
                            }
                            _ => continue,
                        };
                        (
                            mapped_vector_attribute_values(
                                ctx,
                                record.values.as_slice(),
                                |value| value.finite_components(),
                            )?,
                            record.inflated_offset,
                            "PARASOLID_VECTOR_ATTRIBUTE",
                            family,
                        )
                    }
                    Kind::Axes => {
                        let Some(record) = ctx.get_btree_map(
                            &axes_by_id,
                            structured_use.value_record.as_str(),
                            "NX attach parasolid topology structured attributes axes by id lookup",
                        )?
                        else {
                            continue;
                        };
                        (
                            mapped_vector_attribute_values(
                                ctx,
                                record.values.as_slice(),
                                |axis| {
                                    let first = axis[0].finite_components();
                                    let second = axis[1].finite_components();
                                    [
                                        first[0], first[1], first[2], second[0], second[1],
                                        second[2],
                                    ]
                                },
                            )?,
                            record.inflated_offset,
                            "ENTITY_57_AXIS_ATTRIBUTE",
                            "87_axis",
                        )
                    }
                    Kind::Tags => {
                        let Some(record) = ctx.get_btree_map(
                            &tags_by_id,
                            structured_use.value_record.as_str(),
                            "NX attach parasolid topology structured attributes tags by id lookup",
                        )?
                        else {
                            continue;
                        };
                        (
                            mapped_attribute_values(ctx, record.values.as_slice(), |value| {
                                AttributeValue::Integer(i64::from(*value))
                            })?,
                            record.inflated_offset,
                            "ENTITY_58_TAG_ATTRIBUTE",
                            "88_tag",
                        )
                    }
                    Kind::Unicode => {
                        let Some(record) = ctx.get_btree_map(
                            &unicode_by_id,
                            structured_use.value_record.as_str(),"NX attach parasolid topology structured attributes unicode by id lookup",
                        )?
                        else {
                            continue;
                        };
                        (
                            single_string_attribute_values(ctx, record.value.as_str())?,
                            record.inflated_offset,
                            "ENTITY_62_UNICODE_ATTRIBUTE",
                            "98_unicode",
                        )
                    }
                };
                let id = topology_attribute_id(
                    ctx,
                    reference,
                    &cadmpeg_ir::identity_component!("topology-structured-attribute"),
                    structured_use.position.reference_ordinal(),
                    context.id_suffix.as_ref(),
                )?;
                let source_stream = StreamHandle::new(
                    ctx,
                    cadmpeg_ir::stream_name!("nx:s").with_suffix(
                        ctx,
                        reference.stream_ordinal,
                        "compose annotation stream name",
                    )?,
                    "allocate annotation stream handle",
                )?;
                annotations.note(ctx, id.as_str(), &source_stream, source_offset, Some(tag))?;
                annotations
                    .derived(ctx, id.as_str(), "target")
                    .map_err(cadmpeg_core::CodecError::from)?;
                annotations
                    .derived(ctx, id.as_str(), "name")
                    .map_err(cadmpeg_core::CodecError::from)?;
                let field_name = attribute_index.attribute_names.field_name(
                    ctx,
                    reference,
                    structured_use.id.as_str(),
                )?;
                let name = topology_attribute_name(
                    ctx,
                    field_name,
                    ctx.get_btree_map(
                        &attribute_index.class_names,
                        reference.id.as_str(),"NX attach parasolid topology structured attributes attribute index class names lookup",
                    )?
                    .and_then(Option::as_ref)
                    .copied(),
                    family,
                    structured_use.position.reference_ordinal(),
                )?;
                push_topology_attribute(ctx, ir, context, id, name, values)?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut ir.model.attributes,
        |value| value.id.as_str(),
        Ord::cmp,
        "sort NX Parasolid structured attributes",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
