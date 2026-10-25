// SPDX-License-Identifier: Apache-2.0
//! Application declarations, dependency lists, object data and extensions.

use super::{document_properties, ObjectRecord, Vocabulary};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

mod namespace;
use namespace::UnavailableDeclarationScope;

pub(crate) struct ObjectSource<'source> {
    owner: cadmpeg_core::decode::RecordOwner<'source>,
    source: Rc<crate::xml_framing::XmlSource<'source>>,
    declaration: usize,
    dependencies: Vec<usize>,
}

impl<'source> ObjectSource<'source> {
    pub(crate) fn object_id(&self, ctx: &DecodeContext<'source>) -> Result<String, CodecError> {
        let name = self
            .source
            .attribute(ctx, self.declaration, "name")?
            .ok_or_else(|| CodecError::malformed("derived object owner has no source name"))?;
        Ok(crate::native::native_id("object", name))
    }

    pub(crate) fn domain(
        &self,
        ctx: &DecodeContext<'source>,
    ) -> Result<Option<String>, CodecError> {
        let type_name = self
            .source
            .attribute(ctx, self.declaration, "type")?
            .ok_or_else(|| {
                CodecError::malformed("admitted object declaration has no source type")
            })?;
        Ok(type_name
            .split_once("::")
            .map(|(domain, _)| domain.to_owned()))
    }

    pub(crate) fn dependencies(
        &self,
        ctx: &DecodeContext<'source>,
        retained: &[String],
    ) -> Result<Vec<String>, CodecError> {
        let mut expected = retained.iter().peekable();
        let mut dependencies = Vec::new();
        for &index in &self.dependencies {
            let name = self
                .source
                .peek_attribute(ctx, index, "Name")?
                .ok_or_else(|| {
                    CodecError::malformed("admitted dependency has no original source name")
                })?;
            let candidate = crate::native::native_id("object", name);
            if expected.peek().is_none_or(|next| **next != candidate) {
                continue;
            }
            let name = self.source.attribute(ctx, index, "Name")?.ok_or_else(|| {
                CodecError::malformed("admitted dependency lost its original source name")
            })?;
            dependencies.push(crate::native::native_id("object", name));
            expected.next();
        }
        if expected.next().is_some() {
            return Err(CodecError::malformed(
                "retained object dependency has no admitted source supplier",
            ));
        }
        Ok(dependencies)
    }

    pub(crate) fn record_owner(
        &self,
        ctx: &DecodeContext<'source>,
        id: &str,
    ) -> Result<(String, cadmpeg_core::decode::RecordOwner<'source>), CodecError> {
        let extent = self.source.extent(self.declaration)?;
        ctx.record_fragment(&self.owner, extent, || {
            ctx.record_owner(id, extent, || self.object_id(ctx))
        })
    }

    pub(crate) fn feature_header_into(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &cadmpeg_core::decode::RecordOwner<'source>,
    ) -> Result<(String, String, String), CodecError> {
        ctx.record_fragment(owner, self.source.extent(self.declaration)?, || {
            let name = self
                .source
                .attribute(ctx, self.declaration, "name")?
                .ok_or_else(|| CodecError::malformed("symbol source has no object name"))?;
            let kind = self
                .source
                .attribute(ctx, self.declaration, "type")?
                .ok_or_else(|| CodecError::malformed("symbol source has no object type"))?;
            Ok((crate::native::native_id("object", &name), name, kind))
        })
    }

    pub(crate) fn feature_record_owner(
        &self,
        ctx: &DecodeContext<'source>,
        id: &str,
    ) -> Result<((String, String), cadmpeg_core::decode::RecordOwner<'source>), CodecError> {
        let extent = self.source.extent(self.declaration)?;
        ctx.record_fragment(&self.owner, extent, || {
            ctx.record_owner(id, extent, || {
                let object = self.object_id(ctx)?;
                let kind = self
                    .source
                    .attribute(ctx, self.declaration, "type")?
                    .ok_or_else(|| CodecError::malformed("feature source has no runtime type"))?;
                Ok((object, kind))
            })
        })
    }

    pub(crate) fn record<T>(
        &self,
        ctx: &DecodeContext<'source>,
        id: &str,
        construct: impl FnOnce(String) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let extent = self.source.extent(self.declaration)?;
        ctx.record_fragment(&self.owner, extent, || {
            ctx.record(id, extent, || construct(self.object_id(ctx)?))
        })
    }
}

fn declaration_position(index: &[(&str, usize)], id: &str) -> Option<usize> {
    index
        .binary_search_by_key(&id, |(candidate, _)| *candidate)
        .ok()
        .map(|position| index[position].1)
}

#[derive(Default)]
pub(crate) struct ObjectDeclarations<'source> {
    pub(crate) xml_source: Option<Rc<crate::xml_framing::XmlSource<'source>>>,
    pub(crate) entry_contents: Vec<crate::native::entry_contents::EntryContentUpdate>,
    pub(crate) entry_interpretations: crate::gui::payloads::sources::EntryInterpretations<'source>,
    pub(crate) sources: BTreeMap<String, ObjectSource<'source>>,
    pub(crate) declared_objects: BTreeSet<String>,
    pub(crate) admitted_objects: BTreeSet<String>,
    pub(crate) objects: Vec<ObjectRecord>,
    pub(crate) payload_sources: crate::gui::payloads::sources::PayloadSources<'source>,
    pub(crate) properties: BTreeMap<String, document_properties::RootProperties>,
    pub(crate) data_sources: BTreeMap<String, std::ops::Range<usize>>,
    pub(crate) extensions: Vec<crate::native::ExtensionRecord>,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    pub(crate) extension_sources: Vec<(String, std::ops::Range<usize>)>,
    body_scopes: Vec<namespace::UnavailableBodyScope>,
}

impl ObjectDeclarations<'_> {
    pub(crate) fn bind_failure_homes(
        &self,
        ctx: &DecodeContext<'_>,
        registry: &cadmpeg_ir::source_fidelity::parts::FinalizedFailureRegistry,
        homes: &mut BTreeMap<String, cadmpeg_ir::source_fidelity::parts::GivenUpHome>,
    ) -> Result<(), CodecError> {
        for scope in &self.body_scopes {
            scope.bind(ctx, registry, homes)?;
        }
        Ok(())
    }
}

pub(crate) fn decode_scanned<'source>(
    ctx: &DecodeContext<'source>,
    source: &Rc<crate::xml_framing::XmlSource<'source>>,
    root: usize,
    schema: Vocabulary,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<ObjectDeclarations<'source>, CodecError> {
    let (objects_tag, data_tag, record_tag) = schema.tags();
    let mut result = ObjectDeclarations {
        xml_source: Some(Rc::clone(source)),
        ..ObjectDeclarations::default()
    };
    let mut owners = BTreeMap::new();
    let mut saw_objects = false;
    let mut unavailable = Vec::new();
    for section in source.children(root) {
        if !matches!(source.name(section)?, "Objects" | "Features") {
            continue;
        }
        let section_result = ctx.decode_part(
            output,
            source.extent(section)?,
            source.spec(section, "freecad.objects")?,
            |stage| {
                source.validate(section)?;
                if source.name(section)? != objects_tag {
                    return Err(CodecError::malformed(
                        "object declarations use the wrong schema envelope",
                    ));
                }
                if saw_objects {
                    return Err(CodecError::malformed(
                        "duplicate object declarations section",
                    ));
                }
                saw_objects = true;
                let header = source.peek_attributes(ctx, section)?;
                let dependencies_enabled =
                    schema != Vocabulary::Features && header.contains_key("Dependencies");
                let first_declaration = source
                    .children(section)
                    .find(|index| source.name(*index).is_ok_and(|name| name == record_tag));
                let mut losses = Vec::new();
                let mut objects = Vec::new();
                let mut names = BTreeSet::new();
                let mut unavailable = Vec::new();
                for (order, index) in source
                    .children(section)
                    .filter(|index| source.name(*index).is_ok_and(|name| name == record_tag))
                    .enumerate()
                {
                    let mut located_identity = None;
                    let object = ctx.decode_part(
                        stage,
                        source.extent(index)?,
                        source.spec(index, "freecad.object_declaration")?,
                        |_stage| {
                            let name = source
                                .peek_attribute(ctx, index, "name")?
                                .filter(|name| !name.trim().is_empty())
                                .ok_or_else(|| {
                                    CodecError::malformed("object declaration has no name")
                                })?;
                            let id = crate::native::native_id("object", &name);
                            located_identity = Some(id.clone());
                            result.declared_objects.insert(id);
                            source.validate(index)?;
                            if !names.insert(name.clone()) {
                                return Err(CodecError::malformed(format_args!(
                                    "duplicate object declaration name {name}"
                                )));
                            }
                            let id = crate::native::native_id("object", &name);
                            ctx.record_owner(&id, source.extent(index)?, || {
                                declaration(ctx, source, index, order)
                            })
                            .map(|(object, owner)| {
                                (
                                    object,
                                    ObjectSource {
                                        owner,
                                        source: Rc::clone(source),
                                        declaration: index,
                                        dependencies: Vec::new(),
                                    },
                                )
                            })
                        },
                    )?;
                    if let Some((object, owner)) = object {
                        objects.push((object, owner));
                    } else {
                        namespace::reserve(ctx, &mut unavailable)?;
                        unavailable.push(UnavailableDeclarationScope::from_given_up(
                            ctx,
                            source,
                            index,
                            "freecad.object_declaration",
                            located_identity,
                            stage,
                        )?);
                    }
                }
                if dependencies_enabled {
                    let declarations = source
                        .children(section)
                        .filter(|index| source.name(*index).is_ok_and(|name| name == record_tag))
                        .count();
                    let dependencies = source
                        .children(section)
                        .filter(|index| source.name(*index).is_ok_and(|name| name == "ObjectDeps"))
                        .count();
                    if dependencies != declarations {
                        return Err(CodecError::malformed(
                            "Objects dependency population does not match its declarations",
                        ));
                    }
                }
                let mut declaration_index = ctx.alloc_filled(
                    objects.len(),
                    ("", 0_usize),
                    "object declaration navigation",
                )?;
                for (position, (object, _)) in objects.iter().enumerate() {
                    declaration_index[position] = (object.id().as_str(), position);
                }
                declaration_index.sort_unstable_by_key(|(id, _)| *id);
                let mut dependency_updates =
                    ctx.alloc_filled(objects.len(), None, "object dependency updates")?;
                let mut dependency_names = BTreeSet::new();
                for (order, index) in source
                    .children(section)
                    .filter(|index| source.name(*index).is_ok_and(|name| name == "ObjectDeps"))
                    .enumerate()
                {
                    let dependency = ctx.decode_part(
                        stage,
                        source.extent(index)?,
                        source.spec(index, "freecad.object_deps")?,
                        |deps_stage| {
                            let name = source
                                .peek_attribute(ctx, index, "Name")?
                                .filter(|name| !name.trim().is_empty())
                                .ok_or_else(|| CodecError::malformed("ObjectDeps has no Name"))?;
                            let id = crate::native::native_id("object", &name);
                            let Some(position) = declaration_position(&declaration_index, &id)
                            else {
                                if namespace::unavailable(&unavailable, &id).is_some() {
                                    namespace::decode_unavailable_dependencies(
                                        ctx, source, index, deps_stage,
                                    )?;
                                    return Ok((None, vec![namespace::object_identities(&name)]));
                                }
                                if result.declared_objects.contains(&id) {
                                    return Ok((None, Vec::new()));
                                }
                                return Err(CodecError::malformed(
                                    "ObjectDeps target was never declared",
                                ));
                            };
                            let (object, owner) = &objects[position];
                            if schema == Vocabulary::Features {
                                return Err(CodecError::malformed(
                                    "schema 2 Features cannot carry dependencies",
                                ));
                            }
                            if !dependency_names.insert(name.to_owned()) {
                                return Err(CodecError::malformed(format_args!(
                                    "duplicate ObjectDeps name {name}"
                                )));
                            }
                            if !dependencies_enabled
                                || object.order != order
                                || first_declaration.is_some_and(|first| index > first)
                            {
                                return Err(CodecError::malformed(
                                    "ObjectDeps do not match the Objects dependency envelope",
                                ));
                            }
                            ctx.record_fragment(&owner.owner, source.extent(index)?, || {
                                source.validate(index)?;
                                let actual_name =
                                    source.attribute(ctx, index, "Name")?.ok_or_else(|| {
                                        CodecError::malformed("ObjectDeps has no Name")
                                    })?;
                                let allow_partial = source
                                    .attribute(ctx, index, "AllowPartial")?
                                    .map(|text| {
                                        text.parse::<std::num::NonZeroU64>().map_err(|_| {
                                            CodecError::malformed(
                                                "ObjectDeps AllowPartial must be positive",
                                            )
                                        })
                                    })
                                    .transpose()?;
                                let mut dependencies = Vec::new();
                                let mut dependency_sources = Vec::new();
                                let mut unformed = Vec::new();
                                for child in source.children(index).filter(|child| {
                                    source.name(*child).is_ok_and(|name| name == "Dep")
                                }) {
                                    if let Some((name, omitted)) = ctx.decode_part(
                                        deps_stage,
                                        source.extent(child)?,
                                        source.spec(child, "freecad.object_dep")?,
                                        |_stage| {
                                            source.validate(child)?;
                                            let name = source
                                                .peek_attribute(ctx, child, "Name")?
                                                .filter(|name| !name.trim().is_empty())
                                                .ok_or_else(|| {
                                                    CodecError::malformed("Dep has no Name")
                                                })?;
                                            let target = crate::native::native_id("object", &name);
                                            if !result.declared_objects.contains(&target) {
                                                if namespace::unavailable(&unavailable, &target)
                                                    .is_some()
                                                {
                                                    return Ok((
                                                        None,
                                                        vec![namespace::object_identities(&name)],
                                                    ));
                                                }
                                                return Err(CodecError::malformed(
                                                    "Dep target was never declared",
                                                ));
                                            }
                                            if declaration_position(&declaration_index, &target)
                                                .is_none()
                                            {
                                                return Ok((None, Vec::new()));
                                            }
                                            source
                                                .attribute(ctx, child, "Name")
                                                .map(|name| (name, Vec::new()))
                                        },
                                    )? {
                                        unformed.extend(omitted);
                                        if let Some(name) = name {
                                            namespace::reserve(ctx, &mut dependency_sources)?;
                                            dependency_sources.push(child);
                                            dependencies
                                                .push(crate::native::native_id("object", name));
                                        }
                                    }
                                }
                                Ok((
                                    Some((
                                        actual_name,
                                        dependencies,
                                        dependency_sources,
                                        allow_partial,
                                    )),
                                    unformed,
                                ))
                            })
                        },
                    )?;
                    if let Some((dependency, unformed)) = dependency {
                        namespace::sponsor(
                            ctx,
                            &unavailable,
                            stage,
                            unformed,
                            &mut result.declared_objects,
                        )?;
                        if let Some((name, dependencies, dependency_sources, allow_partial)) =
                            dependency
                        {
                            count(source, ctx, index, "Dep", &mut losses)?;
                            let id = crate::native::native_id("object", name);
                            if let Some(position) = declaration_position(&declaration_index, &id) {
                                dependency_updates[position] =
                                    Some((dependencies, dependency_sources, allow_partial));
                            }
                        }
                    }
                }
                drop(declaration_index);
                for ((object, owner), update) in objects.iter_mut().zip(dependency_updates) {
                    if let Some((dependencies, sources, allow_partial)) = update {
                        object.dependencies = dependencies;
                        object.dependency_allow_partial = allow_partial;
                        owner.dependencies = sources;
                    }
                }
                for (object, _) in &objects {
                    stage.insert_native(
                        ctx,
                        "objects",
                        crate::native::object_declaration::ObjectDeclaration::new(object.clone())?,
                    )?;
                }
                count(source, ctx, section, record_tag, &mut losses)?;
                Ok((objects, losses, unavailable))
            },
        )?;
        if let Some((objects, losses, scopes)) = section_result {
            for scope in scopes {
                namespace::reserve(ctx, &mut unavailable)?;
                unavailable.push(scope);
            }
            result.losses.extend(losses);
            for (object, owner) in objects {
                owners.insert(object.id().clone(), owner);
                result.objects.push(object);
            }
        } else {
            namespace::reserve(ctx, &mut unavailable)?;
            unavailable.push(UnavailableDeclarationScope::from_given_up(
                ctx,
                source,
                section,
                "freecad.objects",
                None,
                output,
            )?);
        }
    }
    result.admitted_objects = result
        .objects
        .iter()
        .map(|object| object.id().clone())
        .collect();
    context.set_declared_objects(result.declared_objects.clone());
    context.set_admitted_objects(result.admitted_objects.clone());
    let mut newly_declared = BTreeSet::new();
    let mut saw_data = false;
    for section in source.children(root) {
        if !matches!(source.name(section)?, "ObjectData" | "FeatureData") {
            continue;
        }
        let section_checkpoint = context.pending_checkpoint();
        let data = ctx.decode_part(
            output,
            source.extent(section)?,
            source.spec(section, "freecad.object_data_section")?,
            |stage| {
                source.validate(section)?;
                if source.name(section)? != data_tag {
                    return Err(CodecError::malformed(
                        "object values use the wrong schema envelope",
                    ));
                }
                if saw_data {
                    return Err(CodecError::malformed("duplicate object data section"));
                }
                saw_data = true;
                let mut result = ObjectDeclarations::default();
                let mut names = BTreeSet::new();
                let mut unformed = Vec::new();
                for index in source
                    .children(section)
                    .filter(|index| source.name(*index).is_ok_and(|name| name == record_tag))
                {
                    let object_checkpoint = context.pending_checkpoint();
                    let located = source.peek_attribute(ctx, index, "name");
                    let located_identity = located
                        .as_ref()
                        .ok()
                        .and_then(|name| name.as_ref())
                        .map(|name| crate::native::native_id("object", name));
                    let owner = located_identity
                        .as_ref()
                        .and_then(|id| owners.get_key_value(id));
                    let decode_part = || {
                        ctx.decode_part(
                            stage,
                            source.extent(index)?,
                            source.spec(index, "freecad.object_data")?,
                            |object_stage| {
                                let name =
                                    located?.filter(|name| !name.trim().is_empty()).ok_or_else(
                                        || CodecError::malformed("ObjectData has no name"),
                                    )?;
                                let id = crate::native::native_id("object", &name);
                                let owner = owners.get(&id);
                                let unavailable_owner = namespace::unavailable(&unavailable, &id);
                                if owner.is_none() && unavailable_owner.is_none() {
                                    return Err(CodecError::malformed(
                                        "ObjectData has no readable declaration",
                                    ));
                                }
                                if !names.insert(name.to_owned()) {
                                    return Err(CodecError::malformed(format_args!(
                                        "duplicate ObjectData name {name}"
                                    )));
                                }
                                let decode = || {
                                    source.validate(index)?;
                                    let id = if owner.is_some() {
                                        let actual_name =
                                            source.attribute(ctx, index, "name")?.ok_or_else(
                                                || CodecError::malformed("ObjectData has no name"),
                                            )?;
                                        crate::native::native_id("object", actual_name)
                                    } else {
                                        id
                                    };
                                    let mut result = ObjectDeclarations::default();
                                    let mut properties_seen = false;
                                    let mut extensions_seen = false;
                                    for child in source.children(index) {
                                        match source.name(child)? {
                                            "Properties" => {
                                                let properties = properties(
                                                    ctx,
                                                    source,
                                                    child,
                                                    &id,
                                                    properties_seen,
                                                    context,
                                                    object_stage,
                                                )?;
                                                properties_seen = true;
                                                if let Some(properties) = properties {
                                                    result
                                                        .properties
                                                        .insert(id.clone(), properties);
                                                }
                                            }
                                            "Extensions" => {
                                                let extensions = extensions(
                                                    ctx,
                                                    source,
                                                    child,
                                                    &id,
                                                    extensions_seen || properties_seen,
                                                    context,
                                                    object_stage,
                                                )?;
                                                extensions_seen = true;
                                                if let Some(extensions) = extensions {
                                                    result.extensions.extend(extensions.extensions);
                                                    result
                                                        .extension_sources
                                                        .extend(extensions.extension_sources);
                                                    result.properties.extend(extensions.properties);
                                                    result.losses.extend(extensions.losses);
                                                }
                                            }
                                            _ => {
                                                return Err(CodecError::malformed(
                                                    "uncovered element in ObjectData",
                                                ))
                                            }
                                        }
                                    }
                                    if owner.is_some() {
                                        let extent = source.extent(index)?;
                                        result.data_sources.insert(
                                            id,
                                            extent.start() - source.view().start()
                                                ..extent.end() - source.view().start(),
                                        );
                                    }
                                    Ok(result)
                                };
                                let data = decode()?;
                                Ok((
                                    data,
                                    if owner.is_none() {
                                        vec![namespace::object_identities(&name)]
                                    } else {
                                        Vec::new()
                                    },
                                ))
                            },
                        )
                    };
                    let object = if let Some((_, owner)) = owner {
                        ctx.record_fragment(&owner.owner, source.extent(index)?, decode_part)?
                    } else {
                        decode_part()?
                    };
                    if let Some((object, omitted)) = object {
                        unformed.extend(omitted);
                        result.data_sources.extend(object.data_sources);
                        result.properties.extend(object.properties);
                        result.extensions.extend(object.extensions);
                        result.extension_sources.extend(object.extension_sources);
                        result.losses.extend(object.losses);
                    } else {
                        context.rollback_pending(object_checkpoint);
                        if let Some((id, _)) = owner {
                            namespace::reserve(ctx, &mut result.body_scopes)?;
                            result.body_scopes.push(
                                namespace::UnavailableBodyScope::from_given_up(
                                    ctx,
                                    source,
                                    index,
                                    "freecad.object_data",
                                    std::iter::once(id.as_str()),
                                    stage,
                                )?,
                            );
                        }
                    }
                }
                count(source, ctx, section, record_tag, &mut result.losses)?;
                Ok((result, unformed))
            },
        )?;
        if let Some((data, unformed)) = data {
            namespace::sponsor(ctx, &unavailable, output, unformed, &mut newly_declared)?;
            result.data_sources.extend(data.data_sources);
            result.properties.extend(data.properties);
            result.extensions.extend(data.extensions);
            result.extension_sources.extend(data.extension_sources);
            result.losses.extend(data.losses);
            for scope in data.body_scopes {
                namespace::reserve(ctx, &mut result.body_scopes)?;
                result.body_scopes.push(scope);
            }
        } else {
            context.rollback_pending(section_checkpoint);
            namespace::reserve(ctx, &mut result.body_scopes)?;
            result
                .body_scopes
                .push(namespace::UnavailableBodyScope::from_given_up(
                    ctx,
                    source,
                    section,
                    "freecad.object_data_section",
                    owners.keys().map(String::as_str),
                    output,
                )?);
        }
    }
    result.declared_objects.extend(newly_declared);
    context.set_declared_objects(result.declared_objects.clone());
    result.sources = owners;
    Ok(result)
}

fn required<'a>(
    attributes: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, CodecError> {
    attributes
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CodecError::malformed(format_args!("element has no {name} attribute")))
}

fn properties<'source>(
    ctx: &DecodeContext<'source>,
    source: &crate::xml_framing::XmlSource<'source>,
    index: usize,
    owner: &str,
    duplicate: bool,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<Option<document_properties::RootProperties>, CodecError> {
    if duplicate {
        return ctx.decode_part(
            output,
            source.extent(index)?,
            source.spec(index, "freecad.properties")?,
            |_stage| {
                Err(CodecError::malformed(
                    "duplicate direct Properties container",
                ))
            },
        );
    }
    document_properties::prepare_owned_scanned(ctx, source, index, owner, context, output).map(Some)
}

fn extensions<'source>(
    ctx: &DecodeContext<'source>,
    source: &crate::xml_framing::XmlSource<'source>,
    index: usize,
    owner: &str,
    invalid_position: bool,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<Option<ObjectDeclarations<'source>>, CodecError> {
    let checkpoint = context.pending_checkpoint();
    let parsed = ctx.decode_part(
        output,
        source.extent(index)?,
        source.spec(index, "freecad.extensions")?,
        |stage| {
            source.validate(index)?;
            if invalid_position {
                return Err(CodecError::malformed(
                    "duplicate or misordered Extensions container",
                ));
            }
            let mut result = ObjectDeclarations::default();
            let mut names = BTreeSet::new();
            let mut types = BTreeSet::new();
            for (order, child) in source
                .children(index)
                .filter(|child| source.name(*child).is_ok_and(|name| name == "Extension"))
                .enumerate()
            {
                let extension_checkpoint = context.pending_checkpoint();
                let extension = ctx.decode_part(
                    stage,
                    source.extent(child)?,
                    source.spec(child, "freecad.extension")?,
                    |extension_stage| {
                        source.validate(child)?;
                        let attributes = source.peek_attributes(ctx, child)?;
                        let name = required(&attributes, "name")?;
                        let type_name = required(&attributes, "type")?;
                        if !names.insert(name.to_owned()) || !types.insert(type_name.to_owned()) {
                            return Err(CodecError::malformed("duplicate extension name or type"));
                        }
                        let id = super::extension_id(owner, name, order);
                        ctx.record(&id, source.extent(child)?, || {
                            let name = source
                                .attribute(ctx, child, "name")?
                                .ok_or_else(|| CodecError::malformed("extension name is absent"))?;
                            let type_name = source
                                .attribute(ctx, child, "type")?
                                .ok_or_else(|| CodecError::malformed("extension type is absent"))?;
                            let extension = crate::native::ExtensionRecord {
                                id: id.clone(),
                                owner: owner.to_owned(),
                                name,
                                type_name,
                                order,
                            };
                            extension_stage.insert_native(ctx, "extensions", extension.clone())?;
                            let mut result = ObjectDeclarations::default();
                            let extent = source.extent(child)?;
                            result.extension_sources.push((
                                id.clone(),
                                extent.start() - source.view().start()
                                    ..extent.end() - source.view().start(),
                            ));
                            result.extensions.push(extension);
                            let mut seen = false;
                            for container in source.children(child) {
                                if source.name(container)? != "Properties" {
                                    // The common extension grammar retains its identity;
                                    // opaque extension bodies stay in the source layout.
                                    continue;
                                }
                                if let Some(properties) = properties(
                                    ctx,
                                    source,
                                    container,
                                    &id,
                                    seen,
                                    context,
                                    extension_stage,
                                )? {
                                    result.properties.insert(id.clone(), properties);
                                }
                                seen = true;
                            }
                            Ok(result)
                        })
                    },
                )?;
                if let Some(extension) = extension {
                    result.extensions.extend(extension.extensions);
                    result.extension_sources.extend(extension.extension_sources);
                    result.properties.extend(extension.properties);
                } else {
                    context.rollback_pending(extension_checkpoint);
                }
            }
            count(source, ctx, index, "Extension", &mut result.losses)?;
            Ok(result)
        },
    )?;
    if parsed.is_none() {
        context.rollback_pending(checkpoint);
    }
    Ok(parsed)
}

pub(crate) fn finish_properties<'source>(
    ctx: &DecodeContext<'source>,
    source: &crate::xml_framing::XmlSource<'source>,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    root: &mut Option<document_properties::RootProperties>,
    declarations: &mut ObjectDeclarations<'source>,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    let mut completed = document_properties::finish_pending(ctx, source, context, output)?;
    declarations
        .entry_contents
        .append(&mut context.take_entry_contents());
    declarations.entry_interpretations = context.take_entry_interpretations();
    for (owner, snapshot) in root
        .iter_mut()
        .map(|root| (crate::native::native_id("document", "0"), root))
        .chain(
            declarations
                .properties
                .iter_mut()
                .map(|(owner, properties)| (owner.clone(), properties)),
        )
    {
        if let Some(properties) = completed.remove(&owner) {
            snapshot.properties = properties.properties;
            snapshot.sources = properties.sources;
        }
    }
    declarations
        .payload_sources
        .append(context.take_payload_sources())?;
    Ok(())
}

fn count(
    source: &crate::xml_framing::XmlSource<'_>,
    ctx: &DecodeContext<'_>,
    index: usize,
    tag: &str,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let attributes = source.peek_attributes(ctx, index)?;
    let actual = source
        .children(index)
        .filter(|child| source.name(*child).is_ok_and(|name| name == tag))
        .count();
    if attributes
        .get("Count")
        .and_then(|text| text.parse::<usize>().ok())
        != Some(actual)
    {
        losses.push(
            crate::loss::FreecadLossCode::PersistenceCountNoncanonical.note(format!(
                "{} Count={:?}; reading {actual} framed elements",
                source.name(index)?,
                attributes.get("Count")
            )),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn declaration(
    ctx: &DecodeContext<'_>,
    source: &crate::xml_framing::XmlSource<'_>,
    index: usize,
    order: usize,
) -> Result<ObjectRecord, CodecError> {
    let peek = source.peek_attributes(ctx, index)?;
    let name = source
        .attribute(ctx, index, "name")?
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| CodecError::malformed("object declaration has no name"))?;
    let type_name = source
        .attribute(ctx, index, "type")?
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| CodecError::malformed("object declaration has no type"))?;
    let persistent_id = if peek
        .get("id")
        .is_some_and(|value| value.parse::<i64>().is_ok())
    {
        source
            .attribute(ctx, index, "id")?
            .and_then(|value| value.parse().ok())
    } else {
        None
    };
    let view_type = source.attribute(ctx, index, "ViewType")?;
    let mut attributes = BTreeMap::new();
    for key in peek
        .keys()
        .filter(|key| !matches!(key.as_str(), "name" | "type" | "id" | "ViewType"))
    {
        if let Some(value) = source.attribute(ctx, index, key)? {
            attributes.insert(key.clone(), value);
        }
    }
    Ok(ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::from_name(name),
        type_name,
        persistent_id,
        view_type,
        attributes,
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order,
    })
}
