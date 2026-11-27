// SPDX-License-Identifier: Apache-2.0
//! Manufacture workspace definitions from Iron.Document XML.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

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
        read_entry(ctx, view, &entry.name, target, losses)?;
    }
    Ok(())
}

/// Reads one CAM document. A document that cannot be read or exceeds the
/// per-expansion budget is a loss and contributes no records; resource
/// exhaustion of the whole decode propagates.
fn read_entry(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    name: &str,
    target: &mut Manufacturing,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut part = Manufacturing::default();
    let mut part_losses = Vec::new();
    match parse(ctx, view, name, &mut part, &mut part_losses) {
        Ok(()) => {
            target.setups.append(&mut part.setups);
            target.operations.append(&mut part.operations);
            target.tools.append(&mut part.tools);
            losses.append(&mut part_losses);
            Ok(())
        }
        Err(CodecError::Malformed(error)) => {
            losses.push(
                F3dLossCode::CamDocumentUnreadable.note(format!("CAM document {name}: {error}")),
            );
            Ok(())
        }
        Err(CodecError::NotImplemented(error)) => {
            losses.push(
                F3dLossCode::CamDocumentUnreadable.note(format!("CAM document {name}: {error}")),
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
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

/// The element forms the format states (`object`, `tool`, `toolpath`).
fn record(node: XmlNode<'_>) -> bool {
    matches!(node.tag_name().name(), "object" | "tool" | "toolpath")
}

/// Element forms that other CAM documents name records with. The format
/// does not state them, so a record in one of them stays in the native XML.
fn unrecognised_form(node: XmlNode<'_>) -> bool {
    matches!(
        node.tag_name().name(),
        "section" | "operation" | "setup" | "job"
    )
}

/// Per-document classification facts, computed in one pass so no record
/// test rescans a subtree or an ancestor chain.
struct Index<'a> {
    /// Local descendant elements of each record, in document order.
    locals: HashMap<usize, Vec<XmlNode<'a>>>,
    setup: Vec<bool>,
    setup_ancestor: Vec<Option<XmlNode<'a>>>,
}

fn id(node: XmlNode<'_>) -> usize {
    node.id().get_usize()
}

fn object_operation(node: XmlNode<'_>) -> bool {
    node.has_tag_name("object")
        && node.attribute("parent").is_some()
        && node
            .attribute("strategy")
            .is_some_and(|strategy| !strategy.is_empty() && strategy != "folder")
}

impl<'a> Index<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        document: &'a roxmltree::Document<'a>,
    ) -> Result<Self, CodecError> {
        let count = document.descendants().count();
        let mut owner = ctx.alloc_filled(count, None::<usize>, "index CAM owners")?;
        let mut locals: HashMap<usize, Vec<XmlNode<'a>>> = HashMap::new();
        for node in document.descendants() {
            let Some(parent) = node.parent() else {
                continue;
            };
            let near = if record(parent) {
                Some(id(parent))
            } else {
                owner[id(parent)]
            };
            owner[id(node)] = near;
            if let (Some(near), true) = (near, node.is_element()) {
                locals.entry(near).or_default().push(node);
            }
        }
        let mut setup = ctx.alloc_filled(count, false, "index CAM setups")?;
        let mut setup_ancestor = ctx.alloc_filled(count, None, "index CAM setup ancestors")?;
        for node in document.descendants() {
            let own = id(node);
            setup[own] = node.has_tag_name("object") && node.attribute("strategy") == Some("setup");
            if let Some(parent) = node.parent() {
                let up = id(parent);
                setup_ancestor[own] = if setup[up] {
                    Some(parent)
                } else {
                    setup_ancestor[up]
                };
            }
        }
        Ok(Self {
            locals,
            setup,
            setup_ancestor,
        })
    }

    fn is_setup(&self, node: XmlNode<'_>) -> bool {
        self.setup[id(node)]
    }

    fn is_operation(&self, node: XmlNode<'a>) -> bool {
        object_operation(node) && !self.setup[id(node)]
    }

    fn setup_of(&self, node: XmlNode<'a>) -> Option<XmlNode<'a>> {
        self.setup_ancestor[id(node)]
    }

    /// The elements below `node` that no nested record owns.
    fn local(&self, node: XmlNode<'a>) -> Cow<'_, [XmlNode<'a>]> {
        if record(node) {
            return Cow::Borrowed(self.locals.get(&id(node)).map_or(&[], Vec::as_slice));
        }
        Cow::Owned(
            node.descendants()
                .skip(1)
                .filter(|child| {
                    child.is_element()
                        && child
                            .ancestors()
                            .skip(1)
                            .take_while(|parent| *parent != node)
                            .all(|parent| !record(parent))
                })
                .collect(),
        )
    }
}

/// The numeric literal and unit suffix of an `expression` of the form
/// `2mm`, `1000mmpm`, `10000rpm`.
fn literal_suffix(expression: &str) -> Option<&str> {
    let expression = expression.trim();
    let split = expression.find(|c: char| c.is_ascii_alphabetic())?;
    expression[..split].replace(',', ".").parse::<f64>().ok()?;
    Some(&expression[split..])
}

/// The unit an `expression` literal names. Only the suffixes the format
/// states name a unit: `mm`, `mmpm`, and `rpm`. The parameter's `value` is
/// the number in that unit.
fn expression_unit(expression: &str) -> Option<&'static str> {
    match literal_suffix(expression)? {
        "mm" => Some("mm"),
        "mmpm" => Some("mm/min"),
        "rpm" => Some("rpm"),
        _ => None,
    }
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

fn parameters<'a>(
    ctx: &DecodeContext<'_>,
    index: &Index<'a>,
    node: XmlNode<'a>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<ManufacturingParameter>, CodecError> {
    let unread = index.local(node).iter().any(|child| {
        child.has_tag_name("Parameter")
            && child.attribute("expression").is_some_and(|text| {
                literal_suffix(text).is_some() && expression_unit(text).is_none()
            })
    });
    if unread {
        losses.push(F3dLossCode::CamParameterUnitUnread.note(
            "a CAM parameter states a unit suffix the format does not state; its value has no unit and no process binding",
        ));
    }
    index
        .local(node)
        .iter()
        .filter(|child| child.has_tag_name("Parameter"))
        .map(|parameter| {
            Ok(ManufacturingParameter {
                name: owned(ctx, parameter.attribute("name").unwrap_or(""))?,
                value: owned(ctx, parameter_value(*parameter))?,
                unit: parameter
                    .attribute("expression")
                    .and_then(expression_unit)
                    .map(|text| owned(ctx, text))
                    .transpose()?,
                kind: parameter
                    .attribute("type")
                    .or_else(|| parameter.attribute("kind"))
                    .map(|text| owned(ctx, text))
                    .transpose()?,
                native_xml: owned(ctx, xml(*parameter))?,
            })
        })
        .collect()
}

fn selections<'a>(
    ctx: &DecodeContext<'_>,
    index: &Index<'a>,
    node: XmlNode<'a>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<ManufacturingSelection>, CodecError> {
    let mut result = Vec::new();
    for child in index.local(node).iter().filter(|child| {
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
            native_xml: owned(ctx, xml(*child))?,
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

/// The record's name: `displayName`, or `name`, where it is not blank.
fn record_name(node: XmlNode<'_>) -> Option<&str> {
    ["displayName", "name"]
        .into_iter()
        .filter_map(|attribute| node.attribute(attribute))
        .find(|text| !text.trim().is_empty())
}

fn identity(member: &str, node: XmlNode<'_>, kind: &str) -> String {
    let digest = cadmpeg_ir::hash::digest::Sha256Digest::digest(member.as_bytes());
    format!("f3d:cam:{kind}#{digest}:{}", node.range().start)
}

/// The strategy names the format states; every other name is `Other`.
fn family(strategy: &str) -> ManufacturingStrategy {
    match strategy {
        "face" => ManufacturingStrategy::Face,
        _ => ManufacturingStrategy::Other,
    }
}

fn object_setup<'a>(
    node: XmlNode<'a>,
    objects: &BTreeMap<&'a str, XmlNode<'a>>,
    setups: &BTreeMap<usize, SetupId>,
    memo: &mut HashMap<&'a str, Option<SetupId>>,
) -> Option<SetupId> {
    let mut visited = Vec::new();
    let mut found = None;
    let mut parent = node.attribute("parent");
    for _ in 0..objects.len() {
        let Some(key) = parent else { break };
        if let Some(known) = memo.get(key) {
            found.clone_from(known);
            break;
        }
        let Some(object) = objects.get(key) else {
            break;
        };
        visited.push(*objects.get_key_value(key).map(|(key, _)| key)?);
        if let Some(setup) = setups.get(&object.range().start) {
            found = Some(setup.clone());
            break;
        }
        parent = object.attribute("parent");
    }
    for key in visited {
        memo.insert(key, found.clone());
    }
    found
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
    if document.descendants().any(unrecognised_form) {
        losses.push(F3dLossCode::CamRecordFormUnread.note(
            "CAM records in section, operation, setup, or job elements are not forms the format states; they stay in the native XML",
        ));
    }
    if document.descendants().any(|node| record(node)) {
        losses.push(F3dLossCode::CamStructureAssumed.note("CAM object nesting, strategy and tool attributes, and named parameter metadata use the source-definition reading; native XML retains all source fields"));
    }
    let index = Index::new(ctx, &document)?;
    let mut setups = BTreeMap::new();
    let mut memo = HashMap::new();
    let objects: BTreeMap<_, _> = document
        .descendants()
        .filter(|node| node.has_tag_name("object"))
        .filter_map(|node| node.attribute("id").map(|id| (id, node)))
        .collect();
    let mut tools = BTreeMap::new();
    let mut tool_numbers = BTreeMap::<String, Vec<(usize, ToolId)>>::new();
    for node in document.descendants().filter(|node| index.is_setup(*node)) {
        let parameters = parameters(ctx, &index, node, losses)?;
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
            name: record_name(node).map(|text| owned(ctx, text)).transpose()?,
            stock: stock(&parameters),
            wcs: wcs(&parameters),
            model_refs: selections(ctx, &index, node, losses)?,
            parameters,
            source: source(ctx, node, member)?,
        });
    }
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("tool"))
    {
        let parameters = tools::parameters(ctx, &index, node, losses)?;
        let id = ToolId::mint(identity(member, node, "cam_tool"))
            .map_err(|error| CodecError::malformed(format_args!("CAM tool identity: {error}")))?;
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
            .map(|child| self::parameters(ctx, &index, child, losses))
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
        let number = value(&parameters, "tool_number")
            .or_else(|| value(&parameters, "nc.number"))
            .or_else(|| node.attribute("number"))
            .map(str::to_owned);
        if let Some(number) = &number {
            tool_numbers
                .entry(number.clone())
                .or_default()
                .push((node.range().start, id.clone()));
        }
        target.tools.push(ManufacturingTool {
            id,
            number,
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
    for node in document
        .descendants()
        .filter(|node| index.is_operation(*node))
    {
        let setup = if let Some(setup) = index
            .setup_of(node)
            .and_then(|parent| setups.get(&parent.range().start))
            .cloned()
            .or_else(|| object_setup(node, &objects, &setups, &mut memo))
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
        let mut parameters = parameters(ctx, &index, node, losses)?;
        let path = node
            .children()
            .find(|child| child.has_tag_name("toolpath") || child.has_tag_name("toolpathToolView"));
        if let Some(path) = path {
            parameters.extend(self::parameters(ctx, &index, path, losses)?);
        }
        if parameters.iter().any(|parameter| parameter.name.is_empty()) {
            losses.push(
                F3dLossCode::CamOperationUnreadable.note(
                    "CAM operation contains an unnamed parameter; its source XML is retained",
                ),
            );
        }
        let strategy = owned(ctx, node.attribute("strategy").unwrap_or(""))?;
        if family(&strategy) == ManufacturingStrategy::Other {
            losses.push(F3dLossCode::CamRecordFormUnread.note(format!(
                "CAM operation strategy {strategy:?} is not one the format states; the operation keeps its strategy name and source XML"
            )));
        }
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
        let tool_key = node.attribute("toolId").filter(|key| !key.is_empty());
        let mut tool = tool_key.and_then(|key| tools.get(key).cloned());
        if tool_key.is_none() && tool.is_none() {
            if let Some(number) = value(&parameters, "tool_number") {
                let candidates = tool_numbers.get(number).map_or(&[][..], Vec::as_slice);
                let split = candidates.partition_point(|(start, _)| *start <= node.range().start);
                tool = split
                    .checked_sub(1)
                    .and_then(|last| candidates.get(last))
                    .or_else(|| candidates.first())
                    .map(|(_, id)| id.clone());
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
        let mut geometry = selections(ctx, &index, node, losses)?;
        if let Some(path) = path {
            geometry.extend(selections(ctx, &index, path, losses)?);
        }
        let order = orders.entry(setup.clone()).or_default();
        target.operations.push(ManufacturingOperation {
            id: OperationId::mint(identity(member, node, "cam_operation")).map_err(|error| {
                CodecError::malformed(format_args!("CAM operation identity: {error}"))
            })?,
            setup,
            order: *order,
            name: record_name(node).map(|text| owned(ctx, text)).transpose()?,
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
