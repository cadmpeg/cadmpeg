// SPDX-License-Identifier: Apache-2.0
//! Generic `FreeCAD` object and property graph recovery.

use cadmpeg_core::CodecError;

use crate::native::{
    ExtensionRecord, LinkTarget, LinkTargetWire, ObjectRecord, PropertyFamily, PropertyRecord,
    ValueRecord,
};

pub(crate) mod document_properties;
pub(crate) mod object_parts;
pub(crate) mod source;

/// Persistence vocabulary follows the numeric schema value, independently of
/// whether its source spelling matches a catalog discriminant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Vocabulary {
    Features,
    Objects,
}

impl Vocabulary {
    pub(crate) fn from_declaration(declared: &str) -> Result<Self, CodecError> {
        match declared.parse::<u32>() {
            Ok(2) => Ok(Self::Features),
            Ok(_) => Ok(Self::Objects),
            Err(_) => Err(CodecError::Malformed(
                "Document.xml SchemaVersion is invalid".into(),
            )),
        }
    }

    pub(crate) const fn tags(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Features => ("Features", "FeatureData", "Feature"),
            Self::Objects => ("Objects", "ObjectData", "Object"),
        }
    }
}

/// Recovered persistence graph.
pub(crate) struct Graph {
    /// Declared objects.
    pub(crate) objects: Vec<ObjectRecord>,
    pub(crate) object_sources: Vec<(String, std::ops::Range<usize>)>,
    pub(crate) property_sources:
        std::collections::BTreeMap<String, crate::fidelity::ValueSourceRanges>,
    /// Dynamic extensions.
    pub(crate) extensions: Vec<ExtensionRecord>,
    pub(crate) extension_sources: Vec<(String, std::ops::Range<usize>)>,
    /// Document and object properties.
    pub(crate) properties: Vec<PropertyRecord>,
    /// Diagnostics for source-only persistence metadata.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Recover the graph through the same source parts used by archive decode.
#[cfg(test)]
pub(crate) fn parse_document(
    xml: &roxmltree::Document<'_>,
    schema: Vocabulary,
) -> Result<Graph, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, view) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        xml.input_text().as_bytes(),
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    parse_document_source(&ctx, view, xml, schema)
}

#[cfg(test)]
pub(crate) fn parse_document_source<'source>(
    ctx: &cadmpeg_core::decode::DecodeContext<'source>,
    view: cadmpeg_core::decode::View<'source>,
    xml: &roxmltree::Document<'_>,
    schema: Vocabulary,
) -> Result<Graph, CodecError> {
    let entries = std::collections::BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, None);
    parse_document_source_with_context(ctx, view, xml, schema, &context)
}

pub(crate) fn parse_document_source_with_context<'source>(
    ctx: &cadmpeg_core::decode::DecodeContext<'source>,
    view: cadmpeg_core::decode::View<'source>,
    xml: &roxmltree::Document<'_>,
    schema: Vocabulary,
    context: &crate::element_map::adopted::Context<'_, 'source>,
) -> Result<Graph, CodecError> {
    if xml.input_text().as_bytes().as_ptr() != view.window().as_ptr()
        || xml.input_text().len() != view.window().len()
    {
        return Err(CodecError::malformed(
            "XML DOM does not borrow its original source View",
        ));
    }
    let source = std::rc::Rc::new(crate::xml_framing::XmlSource::new(ctx, view)?);
    let root = source
        .elements()
        .iter()
        .position(|element| element.parent().is_none())
        .ok_or_else(|| CodecError::malformed("Document.xml has no framed root"))?;
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let mut properties =
        document_properties::prepare_scanned(ctx, &source, root, context, &mut output)?;
    let mut declarations =
        object_parts::decode_scanned(ctx, &source, root, schema, context, &mut output)?;
    object_parts::finish_properties(
        ctx,
        &source,
        context,
        &mut properties,
        &mut declarations,
        &mut output,
    )?;
    let mut graph = graph(properties.as_ref(), &declarations);
    let (_, native, _, losses) = output.into_parts(ctx)?.into_parts();
    if let Some(namespace) = native.namespace("fcstd") {
        graph.objects = namespace.arena_as::<ObjectRecord>("objects")?;
        graph.extensions = namespace.arena_as::<ExtensionRecord>("extensions")?;
        graph.properties = namespace.arena_as::<PropertyRecord>("properties")?;
    }
    graph.losses.extend(losses);
    Ok(graph)
}

pub(crate) fn graph(
    properties: Option<&document_properties::RootProperties>,
    declarations: &object_parts::ObjectDeclarations<'_>,
) -> Graph {
    let mut graph = Graph {
        objects: declarations.objects.clone(),
        object_sources: declarations
            .data_sources
            .iter()
            .map(|(id, range)| (id.clone(), range.clone()))
            .collect(),
        properties: Vec::new(),
        property_sources: std::collections::BTreeMap::new(),
        extensions: declarations.extensions.clone(),
        extension_sources: declarations.extension_sources.clone(),
        losses: declarations.losses.clone(),
    };
    for properties in properties
        .into_iter()
        .chain(declarations.properties.values())
    {
        graph.losses.extend(properties.losses.iter().cloned());
        graph
            .properties
            .extend(properties.properties.iter().cloned());
        graph.property_sources.extend(
            properties
                .sources
                .iter()
                .map(|(id, source)| (id.clone(), source.clone())),
        );
    }
    graph
}

pub(crate) fn parse_link_targets(
    property: roxmltree::Node<'_, '_>,
    type_name: &str,
) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    if document_properties::value_parts::links::expression_container(type_name) {
        let tag = if type_name == "Spreadsheet::PropertySheet" {
            "Cells"
        } else {
            "ExpressionEngine"
        };
        let root = single_value_element(property, tag, type_name)?;
        let mut containers = root.children().filter(|node| node.has_tag_name("XLinks"));
        let Some(container) = containers.next() else {
            return Ok(Vec::new());
        };
        if containers.next().is_some() {
            return Err(CodecError::malformed(
                "expression has multiple XLinks containers",
            ));
        }
        let children: Vec<_> = container
            .children()
            .filter(|node| node.has_tag_name("XLink"))
            .collect();
        if container
            .attribute("count")
            .and_then(|value| value.parse::<usize>().ok())
            != Some(children.len())
        {
            return Err(CodecError::malformed(
                "expression XLinks count differs from its records",
            ));
        }
        return children.into_iter().map(xlink).collect();
    }
    let grammar = link_grammar(type_name);
    let Some(grammar) = grammar else {
        return Ok(Vec::new());
    };
    let root = single_value_element(property, grammar.root_tag(), type_name)?;
    match grammar {
        LinkGrammar::Link => {
            reject_nested_link_value(root)?;
            Ok(vec![local_link(root, "value", Vec::new())?])
        }
        LinkGrammar::LinkList => {
            let children = counted_children(root, "Link", type_name)?;
            let mut targets = Vec::new();
            for node in children {
                reject_nested_link_value(node)?;
                targets.push(local_link(node, "value", Vec::new())?);
            }
            Ok(targets)
        }
        LinkGrammar::LinkSub => {
            let children = counted_children(root, "Sub", type_name)?;
            let mut subelements = Vec::new();
            for node in children {
                reject_nested_link_value(node)?;
                subelements.push(restored_subelement(node, "value")?);
            }
            Ok(vec![local_link(root, "value", subelements)?])
        }
        LinkGrammar::LinkSubList => {
            let children = counted_children(root, "Link", type_name)?;
            let mut targets = Vec::new();
            for node in children {
                reject_nested_link_value(node)?;
                let sub = restored_subelement(node, "sub")?;
                targets.push(local_link(node, "obj", vec![sub])?);
            }
            Ok(targets)
        }
        LinkGrammar::XLink => Ok(vec![xlink(root)?]),
        LinkGrammar::XLinkSubList => {
            let children = counted_children(root, "XLink", type_name)?;
            let mut targets = Vec::new();
            for node in children {
                targets.push(xlink(node)?);
            }
            Ok(targets)
        }
    }
}

fn reject_nested_link_value(node: roxmltree::Node<'_, '_>) -> Result<(), CodecError> {
    if node.children().any(|child| child.is_element()) {
        return Err(CodecError::Malformed(
            "link carrier contains nested element values".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum LinkGrammar {
    Link,
    LinkList,
    LinkSub,
    LinkSubList,
    XLink,
    XLinkSubList,
}

impl LinkGrammar {
    pub(crate) fn root_tag(self) -> &'static str {
        match self {
            Self::Link => "Link",
            Self::LinkList => "LinkList",
            Self::LinkSub => "LinkSub",
            Self::LinkSubList => "LinkSubList",
            Self::XLink => "XLink",
            Self::XLinkSubList => "XLinkSubList",
        }
    }
}

pub(crate) fn link_grammar(type_name: &str) -> Option<LinkGrammar> {
    let grammar = match type_name {
        "App::PropertyPlacementLink"
        | "App::PropertyLink"
        | "App::PropertyLinkChild"
        | "App::PropertyLinkGlobal"
        | "App::PropertyLinkHidden" => LinkGrammar::Link,
        "App::PropertyLinkList"
        | "App::PropertyLinkListChild"
        | "App::PropertyLinkListGlobal"
        | "App::PropertyLinkListHidden" => LinkGrammar::LinkList,
        "App::PropertyLinkSub"
        | "App::PropertyLinkSubChild"
        | "App::PropertyLinkSubGlobal"
        | "App::PropertyLinkSubHidden" => LinkGrammar::LinkSub,
        "App::PropertyLinkSubList"
        | "App::PropertyLinkSubListChild"
        | "App::PropertyLinkSubListGlobal"
        | "App::PropertyLinkSubListHidden" => LinkGrammar::LinkSubList,
        "App::PropertyXLink" | "App::PropertyXLinkSub" | "App::PropertyXLinkSubHidden" => {
            LinkGrammar::XLink
        }
        "App::PropertyXLinkSubList" | "App::PropertyXLinkList" => LinkGrammar::XLinkSubList,
        _ => return None,
    };
    Some(grammar)
}

fn single_value_element<'a, 'input>(
    property: roxmltree::Node<'a, 'input>,
    tag: &str,
    type_name: &str,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let mut elements = property.children().filter(roxmltree::Node::is_element);
    let value = elements.next().ok_or_else(|| {
        CodecError::malformed(format_args!("{type_name} requires one {tag} value"))
    })?;
    if !value.has_tag_name(tag) || elements.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "{type_name} requires exactly one {tag} value"
        )));
    }
    Ok(value)
}

fn counted_children<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    tag: &'static str,
    type_name: &str,
) -> Result<impl Iterator<Item = roxmltree::Node<'a, 'input>>, CodecError> {
    let count = parent
        .attribute("count")
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "{} element has no count attribute",
                parent.tag_name().name()
            ))
        })?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("{type_name} count is invalid")))?;
    let mut actual_count = 0_usize;
    let mut valid_tags = true;
    for child in parent.children().filter(roxmltree::Node::is_element) {
        actual_count += 1;
        valid_tags &= child.has_tag_name(tag);
    }
    if actual_count != count || !valid_tags {
        return Err(CodecError::malformed(format_args!(
            "{type_name} count={count} but {actual_count} {tag} values were found"
        )));
    }
    Ok(parent.children().filter(roxmltree::Node::is_element))
}

fn local_link(
    node: roxmltree::Node<'_, '_>,
    object_attribute: &str,
    subelements: Vec<String>,
) -> Result<Option<LinkTarget>, CodecError> {
    if object_attribute == "obj" {
        reject_link_aliases(node, &["obj", "sub"])?;
    } else {
        reject_link_aliases(node, &[object_attribute])?;
    }
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some(retained_attr(node, object_attribute)?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn xlink(node: roxmltree::Node<'_, '_>) -> Result<Option<LinkTarget>, CodecError> {
    reject_link_aliases(node, &["name", "file", "sub"])?;
    let file = node.attribute("file").map(str::to_owned);
    let has_children = node.children().any(|child| child.is_element());
    let subelements = match (node.attribute("sub"), node.attribute("count")) {
        (Some(_), None) if !has_children => vec![restored_subelement(node, "sub")?],
        (Some(_), None) => {
            return Err(CodecError::Malformed(
                "App::PropertyXLink sub carrier has nested values".into(),
            ));
        }
        (None, Some(_)) => {
            if node
                .attribute("count")
                .and_then(|count| count.parse::<usize>().ok())
                == Some(0)
            {
                return Err(CodecError::Malformed(
                    "App::PropertyXLink uses count only for one or more Sub values".into(),
                ));
            }
            let children = counted_children(node, "Sub", "App::PropertyXLink")?;
            let mut subelements = Vec::new();
            for child in children {
                if child.children().any(|value| value.is_element()) {
                    return Err(CodecError::Malformed(
                        "App::PropertyXLink Sub carrier has nested values".into(),
                    ));
                }
                subelements.push(restored_subelement(child, "value")?);
            }
            subelements
        }
        (None, None) if !has_children => Vec::new(),
        (None, None) => {
            return Err(CodecError::Malformed(
                "App::PropertyXLink has nested values without a sub carrier".into(),
            ));
        }
        (Some(_), Some(_)) => {
            return Err(CodecError::Malformed(
                "App::PropertyXLink has both sub and count carriers".into(),
            ));
        }
    };
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: file.filter(|file| !file.is_empty()),
        document_attribute: Some("file".to_owned()),
        object: Some(retained_attr(node, "name")?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn restored_subelement(
    node: roxmltree::Node<'_, '_>,
    primary_attribute: &str,
) -> Result<String, CodecError> {
    let primary = node.attribute(primary_attribute).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{} element has no {primary_attribute} attribute",
            node.tag_name().name()
        ))
    })?;
    Ok(node.attribute("shadowed").unwrap_or(primary).to_owned())
}

fn reject_link_aliases(node: roxmltree::Node<'_, '_>, allowed: &[&str]) -> Result<(), CodecError> {
    const CARRIERS: &[&str] = &[
        "value", "Value", "object", "Object", "obj", "Obj", "name", "Name", "document", "Document",
        "doc", "Doc", "file", "File", "sub", "Sub",
    ];
    if let Some(attribute) = node.attributes().find(|attribute| {
        CARRIERS.contains(&attribute.name()) && !allowed.contains(&attribute.name())
    }) {
        return Err(CodecError::malformed(format_args!(
            "{} has unsupported link carrier {}",
            node.tag_name().name(),
            attribute.name()
        )));
    }
    Ok(())
}

pub(crate) fn property_family(type_name: &str) -> PropertyFamily {
    match type_name {
        "Materials::PropertyMaterial" => PropertyFamily::Map,
        "Part::PropertyFilletEdges" => PropertyFamily::List,
        "Spreadsheet::PropertySheet"
        | "Spreadsheet::PropertyColumnWidths"
        | "Spreadsheet::PropertyRowHeights" => PropertyFamily::List,
        "App::PropertyPythonObject" => PropertyFamily::PythonObject,
        "App::PropertyExpression" | "App::PropertyExpressionEngine" => PropertyFamily::Expression,
        "App::PropertyPlacementLink"
        | "App::PropertyLink"
        | "App::PropertyLinkChild"
        | "App::PropertyLinkGlobal"
        | "App::PropertyLinkHidden"
        | "App::PropertyLinkList"
        | "App::PropertyLinkListChild"
        | "App::PropertyLinkListGlobal"
        | "App::PropertyLinkListHidden"
        | "App::PropertyLinkSub"
        | "App::PropertyLinkSubChild"
        | "App::PropertyLinkSubGlobal"
        | "App::PropertyLinkSubHidden"
        | "App::PropertyLinkSubList"
        | "App::PropertyLinkSubListChild"
        | "App::PropertyLinkSubListGlobal"
        | "App::PropertyLinkSubListHidden"
        | "App::PropertyXLink"
        | "App::PropertyXLinkList"
        | "App::PropertyXLinkSub"
        | "App::PropertyXLinkSubHidden"
        | "App::PropertyXLinkSubList" => PropertyFamily::Link,
        "App::PropertyFile" | "App::PropertyFileIncluded" => PropertyFamily::File,
        "Part::PropertyPartShape"
        | "Part::PropertyGeometryList"
        | "Mesh::PropertyMeshKernel"
        | "Points::PropertyPointKernel" => PropertyFamily::Geometry,
        "App::PropertyPlacement" | "App::PropertyPlacementList" | "App::PropertyRotation" => {
            PropertyFamily::Placement
        }
        "App::PropertyMatrix" => PropertyFamily::Matrix,
        "App::PropertyVector"
        | "App::PropertyVectorList"
        | "App::PropertyVectorDistance"
        | "App::PropertyPosition"
        | "App::PropertyDirection" => PropertyFamily::Vector,
        "App::PropertyEnumeration" => PropertyFamily::Enumeration,
        "App::PropertyAcceleration"
        | "App::PropertyAngle"
        | "App::PropertyArea"
        | "App::PropertyDistance"
        | "App::PropertyForce"
        | "App::PropertyLength"
        | "App::PropertyPressure"
        | "App::PropertyQuantity"
        | "App::PropertyQuantityConstraint"
        | "App::PropertySpeed"
        | "App::PropertyVolume" => PropertyFamily::Quantity,
        "App::PropertyMap" | "App::PropertyMaterial" | "Mesh::PropertyMaterial" => {
            PropertyFamily::Map
        }
        "App::PropertyColorList"
        | "App::PropertyMaterialList"
        | "App::PropertyBoolList"
        | "App::PropertyFloatList"
        | "App::PropertyIntegerList"
        | "App::PropertyIntegerSet"
        | "App::PropertyStringList"
        | "Part::PropertyTopoShapeList"
        | "Sketcher::PropertyConstraintList"
        | "TechDraw::PropertyCenterLineList"
        | "TechDraw::PropertyCosmeticEdgeList"
        | "TechDraw::PropertyCosmeticVertexList"
        | "TechDraw::PropertyGeomFormatList" => PropertyFamily::List,
        "Points::PropertyGreyValueList"
        | "Inspection::PropertyDistanceList"
        | "Points::PropertyNormalList"
        | "Mesh::PropertyNormalList"
        | "Points::PropertyCurvatureList"
        | "Mesh::PropertyCurvatureList" => PropertyFamily::List,
        "App::PropertyFont"
        | "App::PropertyPersistentObject"
        | "App::PropertyPath"
        | "App::PropertyString"
        | "App::PropertyUUID"
        | "Path::PropertyPath" => PropertyFamily::String,
        "App::PropertyColor"
        | "App::PropertyPrecision"
        | "App::PropertyBool"
        | "App::PropertyFloat"
        | "App::PropertyFloatConstraint"
        | "App::PropertyInteger"
        | "App::PropertyIntegerConstraint"
        | "App::PropertyPercent" => PropertyFamily::Scalar,
        _ if scalar_value_tag(type_name) == Some("Float") => PropertyFamily::Quantity,
        _ => PropertyFamily::Unknown,
    }
}

pub(crate) fn is_xlink_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyXLink"
            | "App::PropertyXLinkList"
            | "App::PropertyXLinkSub"
            | "App::PropertyXLinkSubHidden"
            | "App::PropertyXLinkSubList"
    )
}

fn extension_id(owner: &str, name: &str, order: usize) -> String {
    crate::native::native_child_id("extension", owner, &format!("{order}:{name}"))
}

fn retained_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Result<String, CodecError> {
    required_attr(node, name).map(str::to_owned)
}

fn required_attr<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Result<&'a str, CodecError> {
    node.attribute(name).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{} element has no {name} attribute",
            node.tag_name().name()
        ))
    })
}

pub(crate) fn scalar_value_tag(type_name: &str) -> Option<&'static str> {
    match type_name {
        "App::PropertyBool" => Some("Bool"),
        "App::PropertyEnumeration"
        | "App::PropertyInteger"
        | "App::PropertyIntegerConstraint"
        | "App::PropertyPercent" => Some("Integer"),
        "App::PropertyFloat"
        | "App::PropertyFloatConstraint"
        | "App::PropertyPrecision"
        | "App::PropertyAcceleration"
        | "App::PropertyAmountOfSubstance"
        | "App::PropertyAngle"
        | "App::PropertyArea"
        | "App::PropertyCompressiveStrength"
        | "App::PropertyCurrentDensity"
        | "App::PropertyDensity"
        | "App::PropertyDissipationRate"
        | "App::PropertyDistance"
        | "App::PropertyDynamicViscosity"
        | "App::PropertyElectricalCapacitance"
        | "App::PropertyElectricalConductance"
        | "App::PropertyElectricalConductivity"
        | "App::PropertyElectricalInductance"
        | "App::PropertyElectricalResistance"
        | "App::PropertyElectricCharge"
        | "App::PropertySurfaceChargeDensity"
        | "App::PropertyVolumeChargeDensity"
        | "App::PropertyElectricCurrent"
        | "App::PropertyElectricPotential"
        | "App::PropertyElectromagneticPotential"
        | "App::PropertyFrequency"
        | "App::PropertyForce"
        | "App::PropertyHeatFlux"
        | "App::PropertyInverseArea"
        | "App::PropertyInverseLength"
        | "App::PropertyInverseVolume"
        | "App::PropertyKinematicViscosity"
        | "App::PropertyLength"
        | "App::PropertyLuminousIntensity"
        | "App::PropertyMagneticFieldStrength"
        | "App::PropertyMagneticFlux"
        | "App::PropertyMagneticFluxDensity"
        | "App::PropertyMagnetization"
        | "App::PropertyMass"
        | "App::PropertyMoment"
        | "App::PropertyPressure"
        | "App::PropertyPower"
        | "App::PropertyQuantity"
        | "App::PropertyQuantityConstraint"
        | "App::PropertyShearModulus"
        | "App::PropertySpecificEnergy"
        | "App::PropertySpecificHeat"
        | "App::PropertySpeed"
        | "App::PropertyStiffness"
        | "App::PropertyStiffnessDensity"
        | "App::PropertyStress"
        | "App::PropertyTemperature"
        | "App::PropertyThermalConductivity"
        | "App::PropertyThermalExpansionCoefficient"
        | "App::PropertyThermalTransferCoefficient"
        | "App::PropertyTime"
        | "App::PropertyUltimateTensileStrength"
        | "App::PropertyVacuumPermittivity"
        | "App::PropertyVelocity"
        | "App::PropertyVolume"
        | "App::PropertyVolumeFlowRate"
        | "App::PropertyVolumetricThermalExpansionCoefficient"
        | "App::PropertyWork"
        | "App::PropertyYieldStrength"
        | "App::PropertyYoungsModulus" => Some("Float"),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;

/// The same opacity rule for property owners without application source wrappers.
pub(crate) fn opaque_python_values(type_name: &str, values: &[ValueRecord]) -> bool {
    type_name.contains("PropertyPythonObject")
        && values.iter().any(|value| {
            value.decoded_json.is_none()
                && (value
                    .text
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty())
                    || value.attributes.iter().any(|(key, value)| {
                        !matches!(key.as_str(), "file" | "File" | "name" | "Name")
                            && !value.is_empty()
                    }))
        })
}
