// SPDX-License-Identifier: Apache-2.0
//! Cycle classification from source formulas under the caller's actual producer.

use super::*;

pub(super) struct Locator {
    id: ParameterId,
    owner: Option<FeatureId>,
    property: String,
    order: usize,
    selector: Selector,
}

#[derive(Clone, Copy)]
enum Selector {
    Scalar,
    Cell,
    Constraint(usize),
}

pub(super) fn locators(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &[DesignParameter],
    properties: &[PropertyRecord],
) -> Result<Vec<Locator>, CodecError> {
    let mut sources = Vec::new();
    ctx.reserve_working(
        &mut sources,
        parameters.len(),
        "FreeCAD cycle source locators",
    )?;
    let mut property_index = HashMap::new();
    let mut row_indices = HashMap::new();
    reserve_map(
        ctx,
        &mut property_index,
        properties.len(),
        "FreeCAD locator property index",
    )?;
    reserve_map(
        ctx,
        &mut row_indices,
        properties.len(),
        "FreeCAD locator row indices",
    )?;
    for property in properties {
        property_index.insert(property.id.as_str(), property);
        let tag = match property.type_name.as_str() {
            "Spreadsheet::PropertySheet" => "Cell",
            "Sketcher::PropertyConstraintList" => "Constrain",
            _ => continue,
        };
        let mut rows = Vec::new();
        ctx.reserve_working(
            &mut rows,
            property
                .values()
                .iter()
                .filter(|row| row.tag == tag)
                .count(),
            "FreeCAD locator source rows",
        )?;
        rows.extend(
            property
                .values()
                .iter()
                .filter(|row| row.tag == tag)
                .map(|row| row.order),
        );
        row_indices.insert(property.id.as_str(), rows);
    }
    for parameter in parameters {
        let Some(property) = parameter
            .native_ref
            .as_deref()
            .and_then(|id| property_index.get(id))
            .copied()
        else {
            continue;
        };
        let (order, selector) = match property.type_name.as_str() {
            "Spreadsheet::PropertySheet" | "Sketcher::PropertyConstraintList" => {
                let index = usize::try_from(parameter.ordinal).map_err(CodecError::malformed)?;
                let selector = if property.type_name == "Spreadsheet::PropertySheet" {
                    Selector::Cell
                } else {
                    Selector::Constraint(index)
                };
                (
                    row_indices
                        .get(property.id.as_str())
                        .and_then(|rows| rows.get(index))
                        .copied(),
                    selector,
                )
            }
            _ => (
                property
                    .values()
                    .iter()
                    .find(|row| row.parent.is_none())
                    .map(|row| row.order),
                Selector::Scalar,
            ),
        };
        let Some(order) = order else { continue };
        sources.push(Locator {
            id: parameter.id.clone(),
            owner: parameter.owner.clone(),
            property: property.id.clone(),
            order,
            selector,
        });
    }
    Ok(sources)
}

struct SourceNode<'a> {
    locator: &'a Locator,
    name: String,
    aliases: Vec<String>,
    path: Option<String>,
}

pub(super) fn working_error(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    count: usize,
    width: usize,
    operation: &'static str,
) -> CodecError {
    let Some(bytes) = count.checked_mul(width) else {
        return cadmpeg_core::decode::ResourceLimit::allocation_failed(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            u64::MAX,
            "FreeCAD working allocation size overflow",
        )
        .into();
    };
    let bytes = cadmpeg_core::decode::u64_from_index(bytes);
    cadmpeg_core::decode::ResourceLimit::allocation_failed(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        ctx.policy().limits.max_retained_bytes,
        bytes,
        operation,
    )
    .into()
}

pub(super) fn reserve_map<K: Eq + std::hash::Hash, V>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    map: &mut HashMap<K, V>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    map.try_reserve(count)
        .map_err(|_| working_error(ctx, count, std::mem::size_of::<(K, V)>(), operation))
}

pub(super) fn add_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    map: &mut HashMap<String, Vec<ParameterId>>,
    name: String,
    id: &ParameterId,
) -> Result<(), CodecError> {
    reserve_map(ctx, map, 1, "FreeCAD qualified symbol keys")?;
    let ids = map.entry(name).or_default();
    ctx.reserve_working(ids, 1, "FreeCAD qualified symbol candidates")?;
    ids.push(id.clone());
    Ok(())
}

pub(super) fn affected(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    owner: &FeatureId,
    bindings: &ExpressionBindings,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    model: &cadmpeg_ir::document::Model,
) -> Result<bool, CodecError> {
    if !model
        .parameters
        .iter()
        .any(|parameter| parameter.owner.as_ref() == Some(owner))
    {
        return Ok(false);
    }
    if !reader.has_feature_sources() {
        let mut nodes = Vec::new();
        reader.reserve_working(
            &mut nodes,
            model.parameters.len(),
            "FreeCAD typed cycle nodes",
        )?;
        for parameter in &model.parameters {
            let mut dependencies = Vec::new();
            reader.reserve_working(
                &mut dependencies,
                parameter.expression.dependencies().count(),
                "FreeCAD typed cycle edges",
            )?;
            dependencies.extend(parameter.expression.dependencies().cloned());
            nodes.push(DependencyNode {
                id: parameter.id.clone(),
                owner: parameter.owner.clone(),
                dependencies,
            });
        }
        return Ok(dependency_order(reader, &nodes)?.1.contains(owner));
    }
    let mut source_nodes = Vec::new();
    reader.reserve_working(
        &mut source_nodes,
        bindings.sources.len(),
        "FreeCAD cycle source nodes",
    )?;
    let mut object_symbols = HashMap::new();
    let mut property_index = HashMap::new();
    let mut object_index = HashMap::new();
    let mut owned_properties = HashMap::<&str, Vec<&PropertyRecord>>::new();
    reserve_map(
        reader,
        &mut property_index,
        properties.len(),
        "FreeCAD cycle property index",
    )?;
    reserve_map(
        reader,
        &mut owned_properties,
        objects.len(),
        "FreeCAD cycle property owners",
    )?;
    reserve_map(
        reader,
        &mut object_index,
        objects.len(),
        "FreeCAD cycle object index",
    )?;
    for property in properties {
        property_index.insert(property.id.as_str(), property);
        let owned = owned_properties.entry(property.owner.as_str()).or_default();
        reader.reserve_working(owned, 1, "FreeCAD cycle owned properties")?;
        owned.push(property);
    }
    for object in objects {
        object_index.insert(object.id().as_str(), object);
    }
    for locator in &bindings.sources {
        let property = property_index
            .get(locator.property.as_str())
            .copied()
            .ok_or_else(|| CodecError::malformed("cycle control property is absent"))?;
        if let Some(object) = object_index.get(property.owner.as_str()).copied() {
            if !object_symbols.contains_key(object.id().as_str()) {
                let (name, _) = reader.feature_object_header(object)?.ok_or_else(|| {
                    CodecError::malformed("cycle symbol has no original object header")
                })?;
                let label = if let Some(label) = owned_properties
                    .get(property.owner.as_str())
                    .and_then(|owned| owned.iter().find(|property| property.name == "Label"))
                {
                    reader.feature_field(label, "String", "value")?.flatten()
                } else {
                    None
                };
                let labels = vec![label.unwrap_or_else(|| name.clone())];
                reserve_map(
                    reader,
                    &mut object_symbols,
                    1,
                    "FreeCAD cycle object symbols",
                )?;
                object_symbols.insert(
                    object.id().as_str().to_owned(),
                    ObjectSymbols { name, labels },
                );
            }
        }
        let fields: &[&str] = match locator.selector {
            Selector::Scalar => &[],
            Selector::Cell => &["address", "alias"],
            Selector::Constraint(_) => &["Name"],
        };
        let source = reader
            .feature_fields(property, locator.order, fields)?
            .ok_or_else(|| CodecError::malformed("cycle symbol has no original control header"))?;
        let (name, aliases, path) = match locator.selector {
            Selector::Scalar => (source.name.clone(), Vec::new(), Some(source.name)),
            Selector::Cell => {
                let address = source
                    .attributes
                    .get("address")
                    .ok_or_else(|| CodecError::malformed("cycle cell has no source address"))?;
                (
                    source.attributes.get("alias").unwrap_or(address).clone(),
                    Vec::new(),
                    None,
                )
            }
            Selector::Constraint(index) => {
                if reader.parameter_ordinal(property, locator.order)? != index {
                    return Err(CodecError::malformed(
                        "cycle constraint source ordinal differs",
                    ));
                }
                let mut aliases = vec![format!("Constraints[{index}]")];
                if let Some(name) = source.attributes.get("Name") {
                    aliases.push(name.clone());
                    aliases.push(format!("Constraints.{name}"));
                }
                (
                    format!("Constraint{}", index + 1),
                    aliases,
                    Some(format!("Constraints[{index}]")),
                )
            }
        };
        source_nodes.push(SourceNode {
            locator,
            name,
            aliases,
            path,
        });
    }
    let mut symbol_rows = Vec::new();
    reader.reserve_working(
        &mut symbol_rows,
        source_nodes.len(),
        "FreeCAD cycle symbol rows",
    )?;
    let mut source_index = HashMap::new();
    let mut held = HashMap::new();
    reserve_map(
        reader,
        &mut source_index,
        source_nodes.len(),
        "FreeCAD cycle source identity index",
    )?;
    reserve_map(
        reader,
        &mut held,
        model.parameters.len(),
        "FreeCAD cycle held identities",
    )?;
    for parameter in &model.parameters {
        held.insert(&parameter.id, ());
    }
    for (index, source) in source_nodes.iter().enumerate() {
        let mut names = Vec::new();
        reader.reserve_working(
            &mut names,
            source.aliases.len() + 1,
            "FreeCAD cycle symbol aliases",
        )?;
        names.push(source.name.clone());
        names.extend(source.aliases.iter().cloned());
        symbol_rows.push(ExpressionSymbol {
            id: source.locator.id.clone(),
            owner: source.locator.owner.clone(),
            native_ref: Some(source.locator.property.clone()),
            names,
        });
        source_index.insert(&source.locator.id, index);
    }
    let symbols = expression_symbol_table(
        reader,
        &symbol_rows,
        objects,
        &model.features,
        properties,
        Some(&object_symbols),
    )?;
    let mut formulas = reader.alloc_filled(
        source_nodes.len(),
        None::<Vec<usize>>,
        "FreeCAD source cycle edges",
    )?;
    let mut pending = Vec::new();
    reader.reserve_working(
        &mut pending,
        source_nodes.len(),
        "FreeCAD pending source formulas",
    )?;
    pending.extend(
        source_nodes
            .iter()
            .enumerate()
            .filter(|(_, source)| {
                source.locator.owner.as_ref() == Some(owner)
                    && held.contains_key(&source.locator.id)
            })
            .map(|(index, _)| index),
    );
    while let Some(index) = pending.pop() {
        if formulas[index].is_some() {
            continue;
        }
        let source = &source_nodes[index];
        let property = property_index[source.locator.property.as_str()];
        let formula = if let Some(path) = &source.path {
            let owned = owned_properties
                .get(property.owner.as_str())
                .map_or(&[][..], Vec::as_slice);
            match reader.feature_binding_lookup(property, owned, path)? {
                crate::property_reader::ParameterBinding::Bound(text) => Some(text),
                crate::property_reader::ParameterBinding::ProvedNoMatch => None,
                crate::property_reader::ParameterBinding::Unavailable => {
                    return Err(CodecError::malformed(
                        "held cycle formula has no original binding",
                    ))
                }
            }
        } else {
            reader
                .feature_fields(property, source.locator.order, &["content"])?
                .ok_or_else(|| CodecError::malformed("cycle cell formula has no supplier"))?
                .attributes
                .remove("content")
                .filter(|text| text.starts_with('='))
        };
        let mut dependencies = Vec::new();
        if let Some(text) = formula {
            for (_, identifier) in expression_identifiers(&text) {
                if let Some(dependency) = symbols
                    .dependency(source.locator.owner.as_ref(), identifier)
                    .and_then(|id| source_index.get(id))
                {
                    reader.reserve_working(
                        &mut dependencies,
                        1,
                        "FreeCAD source formula dependencies",
                    )?;
                    dependencies.push(*dependency);
                }
            }
        }
        reader.reserve_working(
            &mut pending,
            dependencies.len(),
            "FreeCAD reachable source formulas",
        )?;
        pending.extend(dependencies.iter().copied());
        formulas[index] = Some(dependencies);
    }
    let mut nodes = Vec::new();
    reader.reserve_working(&mut nodes, held.len(), "FreeCAD held source cycle graph")?;
    let mut visited =
        reader.alloc_filled(source_nodes.len(), false, "FreeCAD source alias visits")?;
    let mut touched = Vec::new();
    let mut pending_aliases = Vec::new();
    for (index, source) in source_nodes
        .iter()
        .enumerate()
        .filter(|(_, source)| held.contains_key(&source.locator.id))
    {
        let Some(dependencies) = &formulas[index] else {
            continue;
        };
        let mut expanded = Vec::new();
        reader.reserve_working(
            &mut pending_aliases,
            dependencies.len(),
            "FreeCAD pending source aliases",
        )?;
        pending_aliases.extend(dependencies.iter().copied());
        while let Some(dependency) = pending_aliases.pop() {
            if visited[dependency] {
                continue;
            }
            visited[dependency] = true;
            reader.reserve_working(&mut touched, 1, "FreeCAD visited source aliases")?;
            touched.push(dependency);
            let target = &source_nodes[dependency].locator.id;
            if held.contains_key(target) {
                reader.reserve_working(&mut expanded, 1, "FreeCAD current source graph edges")?;
                expanded.push(target.clone());
            } else {
                let aliases = formulas[dependency].as_ref().ok_or_else(|| {
                    CodecError::malformed("removed control bridge has no source formula")
                })?;
                if aliases.len() != 1 {
                    return Err(CodecError::malformed(
                        "removed control bridge has no unique source target",
                    ));
                }
                reader.reserve_working(
                    &mut pending_aliases,
                    aliases.len(),
                    "FreeCAD transitive source aliases",
                )?;
                pending_aliases.extend(aliases.iter().copied());
            }
        }
        for dependency in touched.drain(..) {
            visited[dependency] = false;
        }
        nodes.push(DependencyNode {
            id: source.locator.id.clone(),
            owner: source.locator.owner.clone(),
            dependencies: expanded,
        });
    }
    Ok(dependency_order(reader, &nodes)?.1.contains(owner))
}
