// SPDX-License-Identifier: Apache-2.0
//! Registered scalar and collection element rules shared by XML value readers.

use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(crate) fn validate_root(
    kind: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
    child_count: usize,
) -> Result<(), CodecError> {
    if crate::persistence::document_properties::value_parts::spreadsheet::root_tag(kind).is_some() {
        return crate::persistence::document_properties::value_parts::spreadsheet::validate_root(
            kind,
            tag,
            attributes,
            child_count,
        );
    }
    if matches!(
        kind,
        "Part::PropertyShapeHistory" | "Part::PropertyShapeCache"
    ) {
        return Err(CodecError::malformed(
            "registered property emits no value element",
        ));
    }
    let auxiliary_tag = match kind {
        "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList" => Some("FloatList"),
        "Points::PropertyNormalList" | "Mesh::PropertyNormalList" => Some("VectorList"),
        "Points::PropertyCurvatureList" | "Mesh::PropertyCurvatureList" => Some("CurvatureList"),
        "Mesh::PropertyMaterial" => Some("Material"),
        _ => None,
    };
    if let Some(expected_tag) = auxiliary_tag {
        expected(tag, expected_tag)?;
        if matches!(
            kind,
            "Points::PropertyNormalList"
                | "Mesh::PropertyNormalList"
                | "Points::PropertyCurvatureList"
                | "Mesh::PropertyCurvatureList"
        ) {
            required(attributes, "file")?;
        }
        if matches!(
            kind,
            "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList"
        ) && !attributes.contains_key("file")
        {
            count(attributes, "count", child_count)?;
        }
        return Ok(());
    }
    if kind == "Part::PropertyPartShape"
        && matches!(
            tag,
            "ElementMap" | "ElementMap2" | "StringHasher" | "StringHasher2"
        )
    {
        return Ok(());
    }
    if let Some(expected) = super::gui_value_tag(kind) {
        if tag != expected.as_str()
            && !(kind == "App::PropertyEnumeration" && tag == "CustomEnumList")
            && !(kind == "App::PropertyPersistentObject" && tag == "PersistentObject")
        {
            return Err(CodecError::malformed(
                "property value tag differs from its registered runtime type",
            ));
        }
        use super::GuiValueTag;
        match expected {
            GuiValueTag::Bool => {
                if crate::native::parse_bool(required(attributes, "value")?).is_none() {
                    return Err(CodecError::malformed("invalid Boolean value"));
                }
            }
            GuiValueTag::Integer => {
                if tag != "CustomEnumList" {
                    integer(attributes, "value")?;
                }
                if super::is_gui_integer_constraint_type(kind) {
                    constraints(attributes, true)?;
                }
            }
            GuiValueTag::Float => {
                number(attributes, "value")?;
                if super::is_gui_float_constraint_type(kind) {
                    constraints(attributes, false)?;
                }
            }
            GuiValueTag::Python => {
                crate::persistence::document_properties::value_parts::validate_python_mode(
                    tag, attributes,
                )?;
                if !attributes.contains_key("value") && !attributes.contains_key("file") {
                    return Err(CodecError::malformed(
                        "Python value has no value or file carrier",
                    ));
                }
            }
            GuiValueTag::String => {}
            GuiValueTag::Uuid | GuiValueTag::Path => {
                if tag != "PersistentObject" {
                    required(attributes, "value")?;
                }
            }
            GuiValueTag::PropertyColor => {
                required(attributes, "value")?
                    .parse::<u32>()
                    .map_err(|_| CodecError::malformed("invalid packed color"))?;
            }
            GuiValueTag::PropertyVector => {
                for field in ["valueX", "valueY", "valueZ"] {
                    number(attributes, field)?;
                }
            }
            GuiValueTag::PropertyMatrix => {
                for row in 1..=4 {
                    for column in 1..=4 {
                        number(attributes, &format!("a{row}{column}"))?;
                    }
                }
            }
            GuiValueTag::PropertyRotation => {
                for field in ["A", "Ox", "Oy", "Oz"] {
                    number(attributes, field)?;
                }
            }
            GuiValueTag::PropertyPlacement => placement(attributes)?,
            GuiValueTag::PropertyMaterial => material(attributes)?,
            GuiValueTag::BoolList => {
                if !required(attributes, "value")?
                    .bytes()
                    .all(|byte| matches!(byte, b'0' | b'1'))
                {
                    return Err(CodecError::malformed("invalid Boolean list"));
                }
            }
            GuiValueTag::ColorList
            | GuiValueTag::FloatList
            | GuiValueTag::VectorList
            | GuiValueTag::PlacementList
            | GuiValueTag::MaterialList => {
                required(attributes, "file")?;
                if expected == GuiValueTag::MaterialList {
                    if let Some(version) = attributes.get("version") {
                        let version = version
                            .parse::<u32>()
                            .map_err(|_| CodecError::malformed("invalid material-list version"))?;
                        if version > 3 {
                            return Err(CodecError::NotImplemented(format!(
                                "FCStd GUI material-list version {version}"
                            )));
                        }
                    }
                }
            }
            GuiValueTag::StringList
            | GuiValueTag::IntegerList
            | GuiValueTag::IntegerSet
            | GuiValueTag::Map => count(attributes, "count", child_count)?,
            GuiValueTag::FileIncluded => {
                if attributes.contains_key("file") == attributes.contains_key("data") {
                    return Err(CodecError::malformed(
                        "FileIncluded requires one file or data attribute",
                    ));
                }
            }
        }
        if tag == "CustomEnumList" {
            count(attributes, "count", child_count)?;
        }
        return Ok(());
    }
    let expected = match kind {
        "Mesh::PropertyMeshKernel" => Some("Mesh"),
        "Points::PropertyPointKernel" => Some("Points"),
        "TechDraw::PropertyGeomFormatList" => Some("GeomFormatList"),
        "TechDraw::PropertyCosmeticVertexList" => Some("CosmeticVertexList"),
        "TechDraw::PropertyCosmeticEdgeList" => Some("CosmeticEdgeList"),
        "TechDraw::PropertyCenterLineList" => Some("CenterLineList"),
        "App::PropertyExpressionEngine" => Some("ExpressionEngine"),
        "Materials::PropertyMaterial" => Some("PropertyMaterial"),
        "Part::PropertyPartShape" => Some("Part"),
        "Part::PropertyGeometryList" => Some("GeometryList"),
        "Part::PropertyFilletEdges" => Some("FilletEdges"),
        "Part::PropertyTopoShapeList" => Some("ShapeList"),
        "Sketcher::PropertyConstraintList" => Some("ConstraintList"),
        _ => None,
    };
    if let Some(expected) = expected {
        if tag != expected {
            return Err(CodecError::malformed(
                "property value has an unregistered root tag",
            ));
        }
        if matches!(
            kind,
            "TechDraw::PropertyGeomFormatList"
                | "TechDraw::PropertyCosmeticVertexList"
                | "TechDraw::PropertyCosmeticEdgeList"
                | "TechDraw::PropertyCenterLineList"
                | "App::PropertyExpressionEngine"
                | "Part::PropertyGeometryList"
                | "Part::PropertyTopoShapeList"
                | "Sketcher::PropertyConstraintList"
        ) || attributes.contains_key("count")
        {
            if kind == "App::PropertyExpressionEngine" {
                required(attributes, "count")?
                    .parse::<usize>()
                    .map_err(|_| CodecError::malformed("invalid expression count"))?;
            } else {
                count(attributes, "count", child_count)?;
            }
        }
        if kind == "Materials::PropertyMaterial" {
            required(attributes, "uuid")?;
        }
        if kind == "Part::PropertyFilletEdges" {
            required(attributes, "file")?;
        }
        if kind == "Points::PropertyPointKernel" {
            if let Some(matrix) = attributes.get("mtrx") {
                let mut count = 0;
                for token in matrix.split_whitespace() {
                    if count == 16 || !token.parse::<f64>().is_ok_and(f64::is_finite) {
                        return Err(CodecError::malformed(
                            "Points transform requires sixteen finite scalars",
                        ));
                    }
                    count += 1;
                }
                if count != 16 {
                    return Err(CodecError::malformed(
                        "Points transform requires sixteen finite scalars",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_child(
    kind: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<(), CodecError> {
    if let Some(tag) = super::gui_value_tag(kind) {
        if !matches!(
            tag,
            super::GuiValueTag::StringList
                | super::GuiValueTag::IntegerList
                | super::GuiValueTag::IntegerSet
                | super::GuiValueTag::Map
        ) && !matches!(
            kind,
            "App::PropertyEnumeration" | "App::PropertyPersistentObject"
        ) {
            return Err(CodecError::malformed(
                "scalar value has an unexpected child",
            ));
        }
    }
    match kind {
        "App::PropertyStringList" => {
            expected(tag, "String")?;
            required(attributes, "value")?;
        }
        "App::PropertyIntegerList" | "App::PropertyIntegerSet" => {
            expected(tag, "I")?;
            integer(attributes, "v")?;
        }
        "App::PropertyMap" => {
            expected(tag, "Item")?;
            required(attributes, "key")?;
            required(attributes, "value")?;
        }
        "App::PropertyEnumeration" => {
            expected(tag, "Enum")?;
            required(attributes, "value")?;
        }
        "App::PropertyExpressionEngine" => {
            expected(tag, "Expression")?;
            required(attributes, "path")?;
            required(attributes, "expression")?;
        }
        "App::PropertyPlacementList" if tag == "PropertyPlacement" => placement(attributes)?,
        _ => {}
    }
    Ok(())
}

fn count(
    attributes: &BTreeMap<String, String>,
    key: &str,
    actual: usize,
) -> Result<(), CodecError> {
    let declared = required(attributes, key)?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed("invalid value collection count"))?;
    if declared != actual {
        return Err(CodecError::malformed(
            "value collection count differs from its child count",
        ));
    }
    Ok(())
}
fn expected(tag: &str, expected: &str) -> Result<(), CodecError> {
    if tag != expected {
        return Err(CodecError::malformed("invalid collection child tag"));
    }
    Ok(())
}
fn required<'a>(
    attributes: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, CodecError> {
    attributes
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| CodecError::malformed(format_args!("value has no {key} attribute")))
}
fn integer(attributes: &BTreeMap<String, String>, key: &str) -> Result<i64, CodecError> {
    required(attributes, key)?
        .parse()
        .map_err(|_| CodecError::malformed(format_args!("invalid integer {key}")))
}
fn number(attributes: &BTreeMap<String, String>, key: &str) -> Result<f64, CodecError> {
    let value = required(attributes, key)?
        .parse::<f64>()
        .map_err(|_| CodecError::malformed(format_args!("invalid scalar {key}")))?;
    if !value.is_finite() {
        return Err(CodecError::malformed(format_args!(
            "non-finite scalar {key}"
        )));
    }
    Ok(value)
}
fn constraints(attributes: &BTreeMap<String, String>, integral: bool) -> Result<(), CodecError> {
    for key in ["min", "max", "step"] {
        if attributes.contains_key(key) {
            if integral {
                integer(attributes, key)?;
            } else {
                number(attributes, key)?;
            }
        }
    }
    Ok(())
}
fn placement(attributes: &BTreeMap<String, String>) -> Result<(), CodecError> {
    for key in ["Px", "Py", "Pz"] {
        number(attributes, key)?;
    }
    let orientation = if attributes.contains_key("A") {
        ["A", "Ox", "Oy", "Oz"]
    } else {
        ["Q0", "Q1", "Q2", "Q3"]
    };
    let mut nonzero = false;
    for key in orientation {
        nonzero |= number(attributes, key)? != 0.0;
    }
    if !attributes.contains_key("A") && !nonzero {
        return Err(CodecError::malformed("placement quaternion is zero"));
    }
    Ok(())
}
fn material(attributes: &BTreeMap<String, String>) -> Result<(), CodecError> {
    for key in [
        "ambientColor",
        "diffuseColor",
        "specularColor",
        "emissiveColor",
    ] {
        required(attributes, key)?
            .parse::<u32>()
            .map_err(|_| CodecError::malformed("invalid material packed color"))?;
    }
    for key in ["shininess", "transparency"] {
        number(attributes, key)?;
    }
    Ok(())
}

pub(crate) fn requires_single_root(kind: &str) -> bool {
    if crate::persistence::document_properties::value_parts::spreadsheet::root_tag(kind).is_some() {
        return true;
    }
    if crate::persistence::link_grammar(kind).is_some() {
        return true;
    }
    if super::gui_value_tag(kind).is_some() {
        return !matches!(
            kind,
            "App::PropertyEnumeration" | "App::PropertyPersistentObject"
        );
    }
    matches!(
        kind,
        "Points::PropertyGreyValueList"
            | "Inspection::PropertyDistanceList"
            | "Points::PropertyNormalList"
            | "Mesh::PropertyNormalList"
            | "Points::PropertyCurvatureList"
            | "Mesh::PropertyCurvatureList"
            | "Mesh::PropertyMaterial"
            | "Mesh::PropertyMeshKernel"
            | "Points::PropertyPointKernel"
            | "TechDraw::PropertyGeomFormatList"
            | "TechDraw::PropertyCosmeticVertexList"
            | "TechDraw::PropertyCosmeticEdgeList"
            | "TechDraw::PropertyCenterLineList"
            | "App::PropertyExpressionEngine"
            | "Materials::PropertyMaterial"
            | "Part::PropertyGeometryList"
            | "Part::PropertyFilletEdges"
            | "Part::PropertyTopoShapeList"
            | "Sketcher::PropertyConstraintList"
    )
}

pub(crate) fn validate_child_of(
    kind: &str,
    tag: &str,
    parent: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<(), CodecError> {
    if (crate::persistence::document_properties::value_parts::links::expression_container(kind)
        && (tag == "XLinks" || matches!(parent, "XLinks" | "XLink")))
        || crate::persistence::document_properties::value_parts::spreadsheet::root_tag(kind)
            .is_some()
    {
        return crate::persistence::document_properties::value_parts::spreadsheet::validate_child(
            kind, tag, parent, attributes,
        );
    }
    if kind.starts_with("TechDraw::Property") {
        return techdraw_child(kind, tag, parent, attributes);
    }
    if matches!(
        kind,
        "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList"
    ) {
        expected(parent, "FloatList")?;
        expected(tag, "F")?;
        if !required(attributes, "v")?
            .parse::<f32>()
            .is_ok_and(f32::is_finite)
        {
            return Err(CodecError::malformed("invalid inline f32 scalar"));
        }
        return Ok(());
    }
    if matches!(
        kind,
        "Points::PropertyNormalList"
            | "Mesh::PropertyNormalList"
            | "Points::PropertyCurvatureList"
            | "Mesh::PropertyCurvatureList"
            | "Mesh::PropertyMaterial"
    ) {
        return Err(CodecError::malformed(
            "binary auxiliary carrier has an unexpected XML child",
        ));
    }

    let expected_parent = match kind {
        "App::PropertyStringList" => Some("StringList"),
        "App::PropertyIntegerList" => Some("IntegerList"),
        "App::PropertyIntegerSet" => Some("IntegerSet"),
        "App::PropertyMap" => Some("Map"),
        "App::PropertyEnumeration" => Some("CustomEnumList"),
        "App::PropertyExpressionEngine" => Some("ExpressionEngine"),
        _ => None,
    };
    if expected_parent.is_some_and(|expected| parent != expected) {
        return Err(CodecError::malformed(
            "collection value has an unexpected nested element",
        ));
    }
    if matches!(
        kind,
        "Mesh::PropertyMeshKernel" | "Points::PropertyPointKernel"
    ) && attributes
        .iter()
        .any(|(key, value)| matches!(key.as_str(), "file" | "File") && !value.is_empty())
    {
        return Err(CodecError::malformed(
            "kernel descendant has an unowned side-entry reference",
        ));
    }
    match (kind, parent) {
        ("Part::PropertyGeometryList", "GeometryList") => expected(tag, "Geometry")?,
        ("Part::PropertyTopoShapeList", "ShapeList") => {
            expected(tag, "TopoShape")?;
            if !["file", "binary", "brep"]
                .iter()
                .any(|key| attributes.contains_key(*key))
            {
                return Err(CodecError::malformed(
                    "TopoShape has no registered shape carrier",
                ));
            }
        }
        ("Sketcher::PropertyConstraintList", "ConstraintList") => expected(tag, "Constrain")?,
        ("Part::PropertyFilletEdges", _) | ("Materials::PropertyMaterial", _) => {
            return Err(CodecError::malformed(
                "registered leaf value has an unexpected child",
            ))
        }
        _ => {}
    }
    validate_child(kind, tag, attributes)
}

/// The container owns field order and cardinality; each child owns its values.
pub(crate) fn validate_layout(
    kind: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
    children: &[&str],
) -> Result<(), CodecError> {
    if crate::persistence::document_properties::value_parts::spreadsheet::root_tag(kind).is_some()
        || kind == "App::PropertyExpressionEngine"
    {
        return crate::persistence::document_properties::value_parts::spreadsheet::validate_layout(
            kind, tag, attributes, children,
        );
    }
    if !kind.starts_with("TechDraw::Property") {
        return Ok(());
    }
    let record = match kind {
        "TechDraw::PropertyGeomFormatList" => "GeomFormat",
        "TechDraw::PropertyCosmeticVertexList" => "CosmeticVertex",
        "TechDraw::PropertyCosmeticEdgeList" => "CosmeticEdge",
        "TechDraw::PropertyCenterLineList" => "CenterLine",
        _ => return Ok(()),
    };
    if tag.ends_with("List") {
        count(attributes, "count", children.len())?;
        return Ok(());
    }
    let mut expected = match tag {
        "GeomFormat" => vec!["GeomIndex", "Style", "Weight", "Color", "Visible"],
        "CosmeticVertex" => vec![
            "Point",
            "Extract",
            "HLRVisible",
            "Ref3D",
            "IsCenter",
            "Cosmetic",
            "CosmeticLink",
            "CosmeticTag",
        ],
        "CosmeticEdge" => vec!["Style", "Weight", "Color", "Visible", "GeometryType"],
        "CenterLine" => vec![
            "Start",
            "End",
            "Mode",
            "HShift",
            "VShift",
            "Rotate",
            "Extend",
            "Type",
            "Flip",
            "Faces",
            "Edges",
            "CLPoints",
            "Style",
            "Weight",
            "Color",
            "Visible",
            "GeometryType",
        ],
        "Points" | "Faces" | "Edges" | "CLPoints" => {
            let key = match tag {
                "Points" => "PointsCount",
                "Faces" => "FaceCount",
                "Edges" => "EdgeCount",
                _ => "CLPointCount",
            };
            return count(attributes, key, children.len());
        }
        _ => return Ok(()),
    };
    if tag == record {
        expected_type(attributes, &format!("TechDraw::{record}"))?;
    }
    if tag == "CosmeticVertex" {
        if children.get(expected.len()) == Some(&"VertexTag") {
            expected.push("VertexTag");
        }
        expected.extend([
            "PermaPoint",
            "LinkGeom",
            "Color",
            "Size",
            "Style",
            "Visible",
            "Tag",
        ]);
    }
    if matches!(tag, "CosmeticEdge" | "CenterLine") {
        expected.extend([
            "GeomType",
            "ExtractType",
            "EdgeClass",
            "HLRVisible",
            "Reversed",
            "Ref3D",
            "Cosmetic",
            "Source",
            "SourceIndex",
            "CosmeticTag",
        ]);
        let suffix = children.get(expected.len()..).unwrap_or_default();
        if suffix.first() == Some(&"Points") {
            expected.push("Points");
        } else if suffix.get(2) == Some(&"Start") {
            expected.extend([
                "Center",
                "Radius",
                "Start",
                "End",
                "Middle",
                "StartAngle",
                "EndAngle",
                "Clockwise",
                "Large",
            ]);
        } else {
            expected.extend(["Center", "Radius"]);
        }
    }
    if children.get(expected.len()).is_some_and(|tag| {
        *tag == "LineNumber"
            || (*tag == "ISOLineNumber" && matches!(record, "GeomFormat" | "CenterLine"))
    }) {
        expected.push(children[expected.len()]);
    }
    if children != expected {
        return Err(CodecError::malformed(
            "TechDraw record has an invalid field sequence",
        ));
    }
    Ok(())
}

fn expected_type(attributes: &BTreeMap<String, String>, expected: &str) -> Result<(), CodecError> {
    if required(attributes, "type")? != expected {
        return Err(CodecError::malformed(
            "TechDraw record has an invalid runtime type",
        ));
    }
    Ok(())
}

fn techdraw_child(
    kind: &str,
    tag: &str,
    parent: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<(), CodecError> {
    if parent.ends_with("List") {
        let expected = match kind {
            "TechDraw::PropertyGeomFormatList" => "GeomFormat",
            "TechDraw::PropertyCosmeticVertexList" => "CosmeticVertex",
            "TechDraw::PropertyCosmeticEdgeList" => "CosmeticEdge",
            "TechDraw::PropertyCenterLineList" => "CenterLine",
            _ => return Ok(()),
        };
        return self::expected(tag, expected);
    }
    if matches!(tag, "Points" | "Faces" | "Edges" | "CLPoints") {
        if matches!(parent, "CosmeticEdge" | "CenterLine") {
            return Ok(());
        }
        return Err(CodecError::malformed(
            "TechDraw collection has an unexpected parent",
        ));
    }
    if matches!(parent, "Points" | "Faces" | "Edges" | "CLPoints") {
        let expected = match parent {
            "Points" => "Point",
            "Faces" => "Face",
            "Edges" => "Edge",
            _ => "CLPoint",
        };
        self::expected(tag, expected)?;
    } else if !matches!(
        parent,
        "GeomFormat" | "CosmeticVertex" | "CosmeticEdge" | "CenterLine"
    ) {
        return Err(CodecError::malformed(
            "TechDraw leaf has an unexpected nested element",
        ));
    }
    match tag {
        "Point" | "PermaPoint" | "Center" | "Start" | "End" | "Middle" => {
            for key in ["X", "Y", "Z"] {
                number(attributes, key)?;
            }
        }
        "Weight" | "Size" | "Radius" | "StartAngle" | "EndAngle" | "HShift" | "VShift"
        | "Rotate" | "Extend" => {
            number(attributes, "value")?;
        }
        "Visible" | "HLRVisible" | "IsCenter" | "Cosmetic" | "Reversed" | "Flip" | "Clockwise"
        | "Large" => {
            if crate::native::parse_bool(required(attributes, "value")?).is_none() {
                return Err(CodecError::malformed("invalid TechDraw Boolean"));
            }
        }
        "Color" => {
            let color = required(attributes, "value")?;
            if !matches!(color.len(), 7 | 9)
                || !color.starts_with('#')
                || !color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(CodecError::malformed("invalid TechDraw color"));
            }
        }
        "Tag" | "VertexTag" => {
            let uuid = required(attributes, "value")?;
            if uuid.len() != 36
                || !uuid.bytes().enumerate().all(|(index, byte)| {
                    if matches!(index, 8 | 13 | 18 | 23) {
                        byte == b'-'
                    } else {
                        byte.is_ascii_hexdigit()
                    }
                })
            {
                return Err(CodecError::malformed("invalid TechDraw UUID"));
            }
        }
        "CosmeticTag" | "Face" | "Edge" | "CLPoint" => {
            required(attributes, "value")?;
        }
        "GeometryType" | "GeomType" => {
            if !matches!(integer(attributes, "value")?, 1 | 2 | 7) {
                return Err(CodecError::malformed(
                    "unsupported TechDraw geometry selector",
                ));
            }
        }
        "Mode" | "Type" if kind == "TechDraw::PropertyCenterLineList" => {
            if !(0..=2).contains(&integer(attributes, "value")?) {
                return Err(CodecError::malformed(
                    "unsupported TechDraw center-line selector",
                ));
            }
        }
        _ => {
            integer(attributes, "value")?;
        }
    }
    Ok(())
}

/// Ordered collections compare only retained peers, so an unreadable peer stays local.
pub(crate) fn validate_previous(
    kind: &str,
    tag: &str,
    parent: Option<usize>,
    attributes: &BTreeMap<String, String>,
    previous: &[crate::native::ValueRecord],
) -> Result<(), CodecError> {
    crate::persistence::document_properties::value_parts::spreadsheet::validate_previous(
        kind, tag, parent, attributes, previous,
    )?;
    let peers = || previous.iter().filter(|value| value.parent == parent);
    if parent.is_none() {
        let unique = match kind {
            "App::PropertyEnumeration" => {
                matches!(tag, "Integer" | "CustomEnumList")
            }
            "App::PropertyPersistentObject" => matches!(tag, "String" | "PersistentObject"),
            "Part::PropertyPartShape" => tag == "Part",
            _ => false,
        };
        if unique && peers().any(|peer| peer.tag == tag) {
            return Err(CodecError::malformed(
                "registered value has an extra root carrier",
            ));
        }
        if kind == "App::PropertyEnumeration"
            && tag == "CustomEnumList"
            && !peers().any(|peer| peer.tag == "Integer")
        {
            return Err(CodecError::malformed(
                "custom enumeration list has no selecting Integer carrier",
            ));
        }
        if kind == "App::PropertyPersistentObject"
            && tag == "PersistentObject"
            && !peers().any(|peer| peer.tag == "String")
        {
            return Err(CodecError::malformed(
                "persistent object has no leading String carrier",
            ));
        }
    }

    if kind == "App::PropertyIntegerSet" && tag == "I" {
        let value = integer(attributes, "v")?;
        if peers()
            .filter(|peer| peer.tag == "I")
            .filter_map(|peer| peer.attributes.get("v"))
            .filter_map(|value| value.parse::<i64>().ok())
            .any(|previous| previous >= value)
        {
            return Err(CodecError::malformed(
                "IntegerSet values are not sorted and unique",
            ));
        }
    }
    if kind.starts_with("TechDraw::Property") && tag == "GeomType" {
        let value = integer(attributes, "value")?;
        if peers()
            .filter(|peer| peer.tag == "GeometryType")
            .filter_map(|peer| peer.attributes.get("value"))
            .filter_map(|previous| previous.parse::<i64>().ok())
            .any(|previous| previous != value)
        {
            return Err(CodecError::malformed(
                "TechDraw GeometryType and GeomType disagree",
            ));
        }
    }
    Ok(())
}

/// A selector owns the agreement between its value and the record's branch tags.
pub(crate) fn validate_branch(
    kind: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
    siblings: &[&str],
) -> Result<(), CodecError> {
    if !kind.starts_with("TechDraw::Property") || !matches!(tag, "GeometryType" | "GeomType") {
        return Ok(());
    }
    let selected = integer(attributes, "value")?;
    let branch = if siblings.contains(&"Points") {
        7
    } else if siblings.contains(&"StartAngle") {
        2
    } else {
        1
    };
    if selected != branch {
        return Err(CodecError::malformed(
            "TechDraw selector differs from its geometry branch",
        ));
    }
    Ok(())
}

pub(crate) fn validate_visual_layer(
    tag: &str,
    parent: Option<&str>,
    attributes: &BTreeMap<String, String>,
    children: usize,
) -> Result<(), CodecError> {
    if let Some(parent) = parent {
        expected(parent, "VisualLayerList")?;
        expected(tag, "VisualLayer")?;
        if !matches!(required(attributes, "visible")?, "true" | "false") {
            return Err(CodecError::malformed("invalid visual layer visibility"));
        }
        required(attributes, "linePattern")?
            .parse::<u32>()
            .map_err(|_| CodecError::malformed("invalid visual layer line pattern"))?;
        number(attributes, "lineWidth")?;
    } else {
        expected(tag, "VisualLayerList")?;
        count(attributes, "count", children)?;
    }
    Ok(())
}

pub(crate) fn validate_value_roots(
    kind: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
    roots: &[&str],
) -> Result<(), CodecError> {
    if kind == "App::PropertyEnumeration"
        && tag == "Integer"
        && attributes.contains_key("CustomEnum")
        && !roots.contains(&"CustomEnumList")
    {
        return Err(CodecError::malformed(
            "custom enumeration has no choice-list carrier",
        ));
    }
    if kind == "App::PropertyPersistentObject"
        && tag == "String"
        && !roots.contains(&"PersistentObject")
    {
        return Err(CodecError::malformed(
            "persistent object has no object carrier",
        ));
    }
    Ok(())
}
