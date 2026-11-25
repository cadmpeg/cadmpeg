// SPDX-License-Identifier: Apache-2.0
//! Manufacture workspace definitions from Iron.Document XML.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::ids::{OperationId, SetupId, ToolId};
use cadmpeg_ir::report::loss::LossNote;
use records::{
    Manufacturing, ManufacturingOperation, ManufacturingParameter, ManufacturingSelection,
    ManufacturingSetup, ManufacturingSource, ManufacturingStock, ManufacturingStrategy,
    ManufacturingTool, ManufacturingWcs,
};
use roxmltree::Node;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

type XmlNode<'a> = Node<'a, 'a>;

pub(crate) mod project;
pub(crate) mod records;
mod tools;

fn document_member(name: &str) -> bool {
    let mut parts = name.split('/');
    while let Some(part) = parts.next() {
        if (part == "Toolpath"
            || part
                .strip_prefix("Toolpath[")
                .is_some_and(|suffix| suffix.ends_with(']')))
            && parts.next() == Some("Iron.Document")
        {
            return name.ends_with(".irondoc");
        }
    }
    false
}

fn read(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    target: &mut Manufacturing,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if !ctx
        .depth()
        .scope()
        .holds(cadmpeg_core::decode::ArenaClass::Process)
    {
        return Ok(());
    }
    for entry in &scan.entries {
        if !document_member(&entry.name) {
            continue;
        }
        let Some(view) = scan.entry_view(&entry.name) else {
            losses.push(
                F3dLossCode::CamDocumentUnreadable
                    .note(format!("CAM document {} could not be opened", entry.name)),
            );
            continue;
        };
        match parse(ctx, view, &entry.name, target, losses) {
            Ok(()) => {}
            Err(CodecError::Malformed(error)) => losses.push(
                F3dLossCode::CamDocumentUnreadable
                    .note(format!("CAM document {}: {error}", entry.name)),
            ),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn owned(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    ctx.retain_bytes(u64_from_index(text.len()), "retain CAM text")?;
    Ok(text.to_owned())
}

fn xml(node: XmlNode<'_>) -> &str {
    &node.document().input_text()[node.range()]
}

fn source(
    ctx: &DecodeContext<'_>,
    node: XmlNode<'_>,
    member: &str,
) -> Result<ManufacturingSource, CodecError> {
    Ok(ManufacturingSource {
        exactness: cadmpeg_ir::Exactness::ByteExact,
        member: owned(ctx, member)?,
        start: u64_from_index(node.range().start),
        end: u64_from_index(node.range().end),
        native_xml: owned(ctx, xml(node))?,
    })
}

fn record(node: XmlNode<'_>) -> bool {
    matches!(
        node.tag_name().name(),
        "object" | "section" | "tool" | "toolpath" | "operation" | "setup" | "job"
    )
}

fn local_nodes(node: XmlNode<'_>) -> impl Iterator<Item = XmlNode<'_>> {
    node.descendants().skip(1).filter(move |child| {
        child
            .ancestors()
            .skip(1)
            .take_while(|parent| *parent != node)
            .all(|parent| !record(parent))
    })
}

fn parameter_value(node: XmlNode<'_>) -> &str {
    node.attribute("value")
        .or_else(|| node.text().filter(|text| !text.trim().is_empty()))
        .or_else(|| {
            node.descendants().skip(1).find_map(|child| {
                child
                    .attribute("value")
                    .or_else(|| child.text().filter(|text| !text.trim().is_empty()))
            })
        })
        .unwrap_or("")
}

fn parameters(
    ctx: &DecodeContext<'_>,
    node: XmlNode<'_>,
) -> Result<Vec<ManufacturingParameter>, CodecError> {
    local_nodes(node)
        .filter(|child| child.has_tag_name("Parameter"))
        .map(|parameter| {
            Ok(ManufacturingParameter {
                name: owned(ctx, parameter.attribute("name").unwrap_or(""))?,
                value: owned(ctx, parameter_value(parameter))?,
                unit: parameter
                    .attribute("unit")
                    .or_else(|| parameter.attribute("units"))
                    .map(|text| owned(ctx, text))
                    .transpose()?,
                kind: parameter
                    .attribute("type")
                    .or_else(|| parameter.attribute("kind"))
                    .map(|text| owned(ctx, text))
                    .transpose()?,
                native_xml: owned(ctx, xml(parameter))?,
            })
        })
        .collect()
}

fn selections(
    ctx: &DecodeContext<'_>,
    node: XmlNode<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<ManufacturingSelection>, CodecError> {
    let mut result = Vec::new();
    for child in local_nodes(node).filter(|child| {
        matches!(
            child.tag_name().name(),
            "CadRef" | "CadChain" | "CadContours2D"
        )
    }) {
        if child
            .ancestors()
            .skip(1)
            .take_while(|parent| *parent != node)
            .any(|parent| {
                matches!(
                    parent.tag_name().name(),
                    "CadRef" | "CadChain" | "CadContours2D"
                )
            })
        {
            continue;
        }
        result.push(ManufacturingSelection {
            kind: owned(ctx, child.tag_name().name())?,
            native_xml: owned(ctx, xml(child))?,
        });
    }
    if !result.is_empty() {
        losses.push(F3dLossCode::CamSelectionUnresolved.note(
            "CAM geometry selections retain native XML references; topology resolution is pending",
        ));
    }
    Ok(result)
}

fn value<'a>(parameters: &'a [ManufacturingParameter], name: &str) -> Option<&'a str> {
    parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .map(|parameter| parameter.value.as_str())
}

fn stock(parameters: &[ManufacturingParameter]) -> ManufacturingStock {
    let stock: Vec<_> = parameters
        .iter()
        .filter(|parameter| parameter.name.starts_with("job_stock"))
        .cloned()
        .collect();
    ManufacturingStock {
        kind: value(parameters, "job_stockMode")
            .or_else(|| value(parameters, "job_stock_mode"))
            .or_else(|| value(parameters, "job_stock"))
            .map(str::to_owned),
        dimensions: stock
            .iter()
            .filter(|parameter| {
                let name = parameter.name.to_ascii_lowercase();
                ["size", "diameter", "height", "length", "width"]
                    .iter()
                    .any(|part| name.contains(*part))
                    && !name.contains("offset")
            })
            .cloned()
            .collect(),
        offsets: stock
            .iter()
            .filter(|parameter| parameter.name.to_ascii_lowercase().contains("offset"))
            .cloned()
            .collect(),
        parameters: stock,
    }
}

fn wcs(parameters: &[ManufacturingParameter]) -> ManufacturingWcs {
    ManufacturingWcs {
        orientation: parameters
            .iter()
            .filter(|parameter| parameter.name.starts_with("wcs_orientation_"))
            .cloned()
            .collect(),
        origin_mode: value(parameters, "wcs_origin_mode").map(str::to_owned),
        origin: parameters
            .iter()
            .filter(|parameter| {
                parameter.name.starts_with("wcs_origin_") && parameter.name != "wcs_origin_mode"
            })
            .cloned()
            .collect(),
        parameters: parameters
            .iter()
            .filter(|parameter| parameter.name.starts_with("wcs_"))
            .cloned()
            .collect(),
    }
}

fn identity(member: &str, node: XmlNode<'_>, kind: &str) -> String {
    let digest = cadmpeg_ir::hash::digest::Sha256Digest::digest(member.as_bytes());
    format!("f3d:cam:{kind}#{digest}:{}", node.range().start)
}

fn family(strategy: &str) -> ManufacturingStrategy {
    match strategy.to_ascii_lowercase().as_str() {
        "face" | "face2d" | "2dface" => ManufacturingStrategy::Face,
        "adaptive2d" | "2dadaptive" => ManufacturingStrategy::Adaptive2d,
        "pocket2d" | "2dpocket" => ManufacturingStrategy::Pocket2d,
        "contour2d" | "2dcontour" => ManufacturingStrategy::Contour2d,
        "drill" | "drilling" => ManufacturingStrategy::Drilling,
        "adaptive" | "adaptive3d" | "3dadaptive" => ManufacturingStrategy::Adaptive3d,
        "parallel" => ManufacturingStrategy::Parallel,
        "scallop" => ManufacturingStrategy::Scallop,
        _ => ManufacturingStrategy::Other,
    }
}

fn is_setup(node: XmlNode<'_>) -> bool {
    node.has_tag_name("setup")
        || node.has_tag_name("job")
        || (node.has_tag_name("object") && node.attribute("strategy") == Some("setup"))
        || (node.has_tag_name("section")
            && (matches!(node.attribute("type"), Some("setup" | "job"))
                || local_nodes(node).any(|child| {
                    child.has_tag_name("Parameter")
                        && child
                            .attribute("name")
                            .is_some_and(|name| name.starts_with("job_"))
                })))
}

fn is_operation(node: XmlNode<'_>) -> bool {
    node.has_tag_name("operation")
        || (node.has_tag_name("object")
            && !is_setup(node)
            && node.attribute("parent").is_some()
            && node
                .attribute("strategy")
                .is_some_and(|strategy| !strategy.is_empty() && strategy != "folder"))
        || (node.has_tag_name("section")
            && !is_setup(node)
            && (node.attribute("strategy").is_some()
                || node.children().any(|child| child.has_tag_name("toolpath"))))
        || (node.has_tag_name("toolpath")
            && !node.ancestors().skip(1).any(|parent| {
                parent.has_tag_name("operation")
                    || (parent.has_tag_name("section") && !is_setup(parent))
            }))
}

fn object_setup(
    node: XmlNode<'_>,
    objects: &BTreeMap<&str, XmlNode<'_>>,
    setups: &BTreeMap<usize, SetupId>,
) -> Option<SetupId> {
    let mut parent = node.attribute("parent");
    for _ in 0..objects.len() {
        let object = objects.get(parent?)?;
        if let Some(setup) = setups.get(&object.range().start) {
            return Some(setup.clone());
        }
        parent = object.attribute("parent");
    }
    None
}

fn parse(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    member: &str,
    target: &mut Manufacturing,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if u64_from_index(view.window().len()) > ctx.per_expand_allowance() {
        return Err(CodecError::NotImplemented(
            "CAM XML exceeds the per-expansion byte budget".into(),
        ));
    }
    let text = std::str::from_utf8(view.window())
        .map_err(|error| CodecError::malformed(format_args!("CAM XML encoding: {error}")))?;
    let document = ctx.parse_xml(text, "parse CAM XML")?;
    ctx.retain_bytes(
        u64_from_index(text.len())
            .checked_mul(4)
            .ok_or_else(|| CodecError::malformed("CAM retained-byte size overflow"))?,
        "retain CAM parameter projections",
    )?;
    ctx.charge_entities(
        u64_from_index(document.descendants().count()),
        "admit CAM XML elements",
    )?;
    if document.descendants().any(|node| record(node)) {
        losses.push(F3dLossCode::CamStructureAssumed.note("CAM section nesting, strategy and tool attributes, and named parameter metadata use the source-definition reading; native XML retains all source fields"));
    }
    let mut setups = BTreeMap::new();
    let objects: BTreeMap<_, _> = document
        .descendants()
        .filter(|node| node.has_tag_name("object"))
        .filter_map(|node| node.attribute("id").map(|id| (id, node)))
        .collect();
    let mut tools = BTreeMap::new();
    let mut tool_nodes = BTreeMap::new();
    for node in document.descendants().filter(|node| is_setup(*node)) {
        let parameters = parameters(ctx, node)?;
        let id = SetupId::mint(identity(member, node, "cam_setup"))
            .map_err(|error| CodecError::malformed(format_args!("CAM setup identity: {error}")))?;
        if parameters.iter().any(|parameter| parameter.name.is_empty()) {
            losses.push(
                F3dLossCode::CamSetupUnreadable
                    .note("CAM setup contains an unnamed parameter; its source XML is retained"),
            );
        }
        setups.insert(node.range().start, id.clone());
        target.setups.push(ManufacturingSetup {
            id,
            name: node
                .attribute("name")
                .or_else(|| node.attribute("displayName"))
                .map(|text| owned(ctx, text))
                .transpose()?,
            stock: stock(&parameters),
            wcs: wcs(&parameters),
            model_refs: selections(ctx, node, losses)?,
            parameters,
            source: source(ctx, node, member)?,
        });
    }
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("tool"))
    {
        let parameters = tools::parameters(ctx, node)?;
        let id = ToolId::mint(identity(member, node, "cam_tool"))
            .map_err(|error| CodecError::malformed(format_args!("CAM tool identity: {error}")))?;
        tool_nodes.insert(node.range().start, id.clone());
        if parameters.iter().any(|parameter| parameter.name.is_empty()) {
            losses.push(
                F3dLossCode::CamToolUnreadable
                    .note("CAM tool contains an unnamed parameter; its source XML is retained"),
            );
        }
        if let Some(key) = node.attribute("id") {
            if tools.insert(key, id.clone()).is_some() {
                losses.push(
                    F3dLossCode::CamToolReferenceLastAssumed
                        .note("CAM tool reference is repeated; the last tool is selected"),
                );
            }
        }
        let holder = node
            .children()
            .filter(|child| child.has_tag_name("holder"))
            .map(|child| self::parameters(ctx, child))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .chain(
                parameters
                    .iter()
                    .filter(|parameter| {
                        parameter.name.starts_with("holder_")
                            || parameter.name.starts_with("holder.")
                    })
                    .cloned(),
            )
            .collect();
        target.tools.push(ManufacturingTool {
            id,
            number: value(&parameters, "tool_number")
                .or_else(|| value(&parameters, "nc.number"))
                .or_else(|| node.attribute("number"))
                .map(str::to_owned),
            tool_type: value(&parameters, "tool_type")
                .or_else(|| node.attribute("type"))
                .map(str::to_owned),
            vendor: value(&parameters, "tool_vendor").map(str::to_owned),
            product: value(&parameters, "tool_productId").map(str::to_owned),
            holder,
            parameters,
            source: source(ctx, node, member)?,
        });
    }
    let mut orders = BTreeMap::<SetupId, u64>::new();
    for node in document.descendants().filter(|node| is_operation(*node)) {
        let setup = if let Some(setup) = node
            .ancestors()
            .skip(1)
            .find_map(|parent| setups.get(&parent.range().start))
            .cloned()
            .or_else(|| object_setup(node, &objects, &setups))
        {
            setup
        } else {
            let root = document.root_element();
            if let Some(setup) = setups.get(&root.range().start) {
                setup.clone()
            } else {
                let id = SetupId::mint(identity(member, root, "cam_setup")).map_err(|error| {
                    CodecError::malformed(format_args!("CAM setup identity: {error}"))
                })?;
                losses.push(
                    F3dLossCode::CamSetupAssumed
                        .note("CAM operation has no enclosing setup; a document setup is assumed"),
                );
                let mut inferred_source = source(ctx, root, member)?;
                inferred_source.exactness = cadmpeg_ir::Exactness::Inferred;
                target.setups.push(ManufacturingSetup {
                    id: id.clone(),
                    name: None,
                    stock: ManufacturingStock::default(),
                    wcs: ManufacturingWcs::default(),
                    model_refs: Vec::new(),
                    parameters: Vec::new(),
                    source: inferred_source,
                });
                setups.insert(root.range().start, id.clone());
                id
            }
        };
        let mut parameters = parameters(ctx, node)?;
        let path = if node.has_tag_name("toolpath") {
            Some(node)
        } else {
            node.children().find(|child| {
                child.has_tag_name("toolpath") || child.has_tag_name("toolpathToolView")
            })
        };
        if let Some(path) = path.filter(|path| *path != node) {
            parameters.extend(self::parameters(ctx, path)?);
        }
        if parameters.iter().any(|parameter| parameter.name.is_empty()) {
            losses.push(
                F3dLossCode::CamOperationUnreadable.note(
                    "CAM operation contains an unnamed parameter; its source XML is retained",
                ),
            );
        }
        let strategy = node
            .attribute("strategy")
            .or_else(|| path.and_then(|path| path.attribute("strategy")))
            .or_else(|| value(&parameters, "strategy"))
            .or_else(|| value(&parameters, "operation-strategy"))
            .or_else(|| value(&parameters, "operation_strategy"))
            .or_else(|| path.and_then(|path| path.attribute("type")))
            .or_else(|| node.attribute("type"))
            .unwrap_or("");
        if strategy.is_empty() {
            losses.push(F3dLossCode::CamOperationUnreadable.note("CAM operation strategy is absent; the operation retains its parameters and source XML"));
        }
        let strategy = owned(ctx, strategy)?;
        let valid = path
            .and_then(|path| path.attribute("valid"))
            .or_else(|| node.attribute("valid"));
        let computed_path_valid = match valid {
            Some("true" | "1") => Some(true),
            Some("false" | "0") => Some(false),
            None => None,
            Some(_) => {
                losses.push(
                    F3dLossCode::CamOperationUnreadable
                        .note("CAM operation has an unreadable computed-path validity flag"),
                );
                None
            }
        };
        let tool_key = node
            .attribute("tool")
            .or_else(|| node.attribute("toolId").filter(|key| !key.is_empty()))
            .or_else(|| path.and_then(|path| path.attribute("tool")))
            .or_else(|| value(&parameters, "tool_id"));
        let mut tool = if let Some(key) = tool_key {
            tools.get(key).cloned()
        } else {
            node.children()
                .find(|child| child.has_tag_name("tool"))
                .or_else(|| {
                    path.and_then(|path| path.children().find(|child| child.has_tag_name("tool")))
                })
                .and_then(|tool| tool_nodes.get(&tool.range().start))
                .cloned()
        };
        if tool_key.is_none() && tool.is_none() {
            if let Some(number) = value(&parameters, "tool_number") {
                let candidates = target.tools.iter().filter(|tool| {
                    tool.source.member == member && tool.number.as_deref() == Some(number)
                });
                let preceding = candidates
                    .clone()
                    .filter(|tool| tool.source.start <= u64_from_index(node.range().start))
                    .max_by_key(|tool| tool.source.start);
                tool = preceding
                    .or_else(|| candidates.min_by_key(|tool| tool.source.start))
                    .map(|tool| tool.id.clone());
                if tool.is_some() {
                    losses.push(F3dLossCode::CamToolNumberReferenceAssumed.note("CAM operation tool number selects the nearest preceding tool definition, or the first following definition"));
                } else {
                    losses.push(
                        F3dLossCode::CamToolUnreadable
                            .note("CAM operation tool number is unresolved"),
                    );
                }
            }
        }
        if tool_key.is_some() && tool.is_none() {
            losses.push(
                F3dLossCode::CamToolUnreadable.note("CAM operation tool reference is unresolved"),
            );
        }
        let mut geometry = selections(ctx, node, losses)?;
        if let Some(path) = path.filter(|path| *path != node) {
            geometry.extend(selections(ctx, path, losses)?);
        }
        let order = orders.entry(setup.clone()).or_default();
        target.operations.push(ManufacturingOperation {
            id: OperationId::mint(identity(member, node, "cam_operation")).map_err(|error| {
                CodecError::malformed(format_args!("CAM operation identity: {error}"))
            })?,
            setup,
            order: *order,
            name: node
                .attribute("name")
                .or_else(|| node.attribute("displayName"))
                .map(|text| owned(ctx, text))
                .transpose()?,
            family: family(&strategy),
            strategy,
            tool,
            parameters,
            geometry,
            computed_path_present: path.is_some(),
            computed_path_valid,
            source: source(ctx, node, member)?,
        });
        *order = order
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("CAM operation order overflow"))?;
    }
    Ok(())
}

pub(crate) fn scope_member(target: &mut Manufacturing, member: &str) -> Result<(), CodecError> {
    let scope = cadmpeg_ir::hash::digest::Sha256Digest::digest(member.as_bytes());
    let rewrite = |id: &str| {
        format!(
            "f3d:cam/{scope}:{}",
            id.strip_prefix("f3d:cam:").unwrap_or(id)
        )
    };
    for setup in &mut target.setups {
        setup.id = SetupId::mint(rewrite(setup.id.as_str()))
            .map_err(|error| CodecError::malformed(format_args!("CAM setup identity: {error}")))?;
        setup.source.member = format!("{member}/{}", setup.source.member);
    }
    for tool in &mut target.tools {
        tool.id = ToolId::mint(rewrite(tool.id.as_str()))
            .map_err(|error| CodecError::malformed(format_args!("CAM tool identity: {error}")))?;
        tool.source.member = format!("{member}/{}", tool.source.member);
    }
    for operation in &mut target.operations {
        operation.id = OperationId::mint(rewrite(operation.id.as_str())).map_err(|error| {
            CodecError::malformed(format_args!("CAM operation identity: {error}"))
        })?;
        operation.setup = SetupId::mint(rewrite(operation.setup.as_str()))
            .map_err(|error| CodecError::malformed(format_args!("CAM setup reference: {error}")))?;
        operation.tool = operation
            .tool
            .as_ref()
            .map(|tool| ToolId::mint(rewrite(tool.as_str())))
            .transpose()
            .map_err(|error| CodecError::malformed(format_args!("CAM tool reference: {error}")))?;
        operation.source.member = format!("{member}/{}", operation.source.member);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
