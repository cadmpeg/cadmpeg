// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` page and view graph recovery.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::names::{CodecId, NativeName, NativeNameValue};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::drawings::{
    Drawing, DrawingId, DrawingKind, HiddenLineMethod, ProjectionAngle, ScaleFrom,
};
use cadmpeg_ir::scalar::{Angle, FiniteReal, PositiveLength, PositiveReal};
use cadmpeg_ir::units::{FiniteVector, NonzeroVector};
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::annotation::{
    bool_property, enumeration_entry, enumeration_property, optional_scalar_property,
};
use crate::native::{
    sole_named_property, DrawingRecord, ObjectRecord, PropertyRecord, TechDrawKind, ValueRecord,
};

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<DrawingRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            owned.push(property);
        }
    }
    let mut drawings = Vec::new();
    for object in objects
        .iter()
        .filter(|object| is_registered_drawing_type(&object.type_name))
    {
        let source = by_owner
            .get(object.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut owned = Vec::new();
        owned.extend_from_slice(source);
        ensure_unique_property_names(&owned)?;
        let kind = if is_page_type(&object.type_name) {
            let view_links = typed_property(&owned, "Views", "App::PropertyLinkList")?
                .map_or(&[][..], PropertyRecord::links);
            let mut views = Vec::new();
            for link in view_links.iter().flatten() {
                if let Some(name) = link.object() {
                    views.push(name.to_owned());
                }
            }
            let template_link = typed_single_link(&owned, "Template", "App::PropertyLink")?;
            let template = template_link
                .and_then(|link| link.object())
                .map(ToOwned::to_owned);
            TechDrawKind::Page {
                runtime: if object.type_name == "TechDraw::DrawPage" {
                    crate::native::TechDrawPageKind::Page
                } else {
                    crate::native::TechDrawPageKind::Python
                },
                views,
                template,
            }
        } else {
            TechDrawKind::try_new(object.type_name.clone(), Vec::new(), None)
                .map_err(CodecError::malformed)?
        };
        let mut sources = Vec::new();
        for name in [
            "Source",
            "XSource",
            "Sources",
            "References2D",
            "References3D",
            "Source3d",
        ] {
            let links = source_links(&owned, name)?;
            sources.extend(links);
        }
        let mut relationships = BTreeMap::new();
        for property in owned.iter().filter(|property| !property.links().is_empty()) {
            let mut links = Vec::new();
            for link in property.links() {
                links.push(link.clone());
            }
            relationships.insert(property.name.clone(), links);
        }
        let mut side_entries = Vec::new();
        for property in &owned {
            for name in property.side_entries() {
                side_entries.push(name.to_owned());
            }
        }
        drawings.push(DrawingRecord {
            id: crate::native::native_id("drawing", object.name()),
            object: object.id().to_owned(),
            kind,
            sources,
            relationships,
            parameters: drawing_parameters(ctx, &owned)?,
            side_entries,
        });
    }
    Ok(drawings)
}

/// The references a native link states: the explicit empty reference, an
/// object of a document that was not read, or one unresolved reference per
/// selected sub-element of a local object, which the decoder does not
/// resolve to the geometry it decoded. A reference that names a sub-element
/// keeps it in its native name after the object's source name and a `.`.
pub(crate) fn link_references(
    link: Option<&crate::native::LinkTarget>,
) -> Result<Vec<ReferenceSelection>, CodecError> {
    let Some(link) = link else {
        return Ok(vec![ReferenceSelection::new(ReferenceTarget::Null)]);
    };
    let Some(object) = link.object() else {
        if link.document_name().is_some() {
            return Err(CodecError::malformed(
                "drawing reference names a document and no object",
            ));
        }
        return Ok(vec![ReferenceSelection::new(ReferenceTarget::Null)]);
    };
    let name = crate::native::id_key(object);
    let names = if link.subelements().is_empty() {
        vec![None]
    } else {
        link.subelements()
            .iter()
            .map(|subelement| native_name(&format!("{name}.{subelement}")).map(Some))
            .collect::<Result<Vec<_>, _>>()?
    };
    names
        .into_iter()
        .map(|native| {
            Ok(match link.document_name() {
                Some(document) => ReferenceSelection {
                    target: ReferenceTarget::External {
                        document: document.to_owned(),
                        object: object.to_owned(),
                    },
                    native,
                },
                None => ReferenceSelection::unresolved(match native {
                    Some(native) => native,
                    None => native_name(name)?,
                }),
            })
        })
        .collect()
}

fn native_name(text: &str) -> Result<NativeName, CodecError> {
    let blank = || CodecError::malformed("drawing reference has an empty source name");
    Ok(NativeName {
        codec: CodecId::from(NonBlankString::new("fcstd").ok_or_else(blank)?),
        value: NativeNameValue::Text(NonBlankString::new(text).ok_or_else(blank)?),
    })
}

pub(crate) fn transfer_neutral(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    records: &[DrawingRecord],
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    let mut neutral_ids = HashMap::new();
    for record in records {
        neutral_ids.insert(
            record.object.as_str(),
            DrawingId::mint(crate::native::model_id("drawing", &record.object, "entity"))
                .map_err(CodecError::malformed)?,
        );
    }
    for (order, record) in records.iter().enumerate() {
        let mut owned = Vec::new();
        owned.extend(
            properties
                .iter()
                .filter(|property| property.owner == record.object),
        );
        let parameter = |name: &str| scalar_property(ctx, &owned, name);
        let x = parameter("X")?;
        let y = parameter("Y")?;
        let position = match (x, y) {
            (None, None) => None,
            (Some(x), Some(y)) => Some([x, y]),
            _ => {
                return Err(CodecError::malformed(format_args!(
                    "drawing {} position requires both X and Y",
                    record.id
                )))
            }
        };
        let scale = parameter("Scale")?
            .map(|value| {
                PositiveReal::from_finite(value).ok_or_else(|| {
                    CodecError::malformed("drawing scale must be positive and finite")
                })
            })
            .transpose()?;
        let rotation = parameter("Rotation")?
            .map(|degrees| {
                Angle::new(degrees.get().to_radians())
                    .ok_or_else(|| CodecError::malformed("drawing rotation must be finite"))
            })
            .transpose()?;
        let projection = view_projection(ctx, &owned, record.kind.as_str())?;
        let direction = if record.parameters.contains_key("Direction") {
            let value = vector_property(ctx, &owned, "Direction")?
                .ok_or_else(|| CodecError::malformed("drawing direction is absent"))?;
            Some(NonzeroVector::from_finite(value).ok_or_else(|| {
                CodecError::malformed("drawing direction must be finite and nonzero")
            })?)
        } else {
            None
        };
        let position = if classify(record.kind.as_str()) == DrawingKind::Dimension {
            None
        } else {
            position.map(FiniteVector::from)
        };
        let mut relationships = BTreeMap::new();
        for (role, targets) in &record.relationships {
            let mut selections = Vec::new();
            for link in targets {
                selections.extend(link_references(link.as_ref())?);
            }
            relationships.insert(role.to_owned(), selections);
        }
        let template_id = if matches!(record.kind, TechDrawKind::Page { .. }) {
            record
                .relationships
                .get("Template")
                .and_then(|targets| targets.first())
                .and_then(Option::as_ref)
                .and_then(|link| {
                    if link.document().is_some() {
                        return None;
                    }
                    let object = link.object()?;
                    neutral_ids.get(object)
                })
        } else {
            None
        };
        let template = template_id.cloned();
        let mut parameters = BTreeMap::new();
        for (name, value) in &record.parameters {
            parameters.insert(name.to_owned(), value.to_owned());
        }
        let mut assets = Vec::new();
        for name in &record.side_entries {
            assets.push(crate::native::native_id("entry", name));
        }
        model.drawings.push(Drawing {
            visible: None,
            relationships: cadmpeg_core::text::named_entries(&record.object, relationships)?,
            template,
            position,
            scale,
            direction,
            rotation,
            scale_from: projection.scale_from,
            projection: projection.projection,
            perspective: projection.perspective,
            hidden_line: projection.hidden_line,
            parameters: cadmpeg_core::text::named_entries(&record.object, parameters)?,
            assets,
            ..Drawing::new(
                neutral_ids
                    .get(record.object.as_str())
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "drawing {} has no admitted neutral identity",
                            record.id
                        ))
                    })
                    .cloned()?,
                record.object.clone(),
                classify(record.kind.as_str()),
                record.kind.as_str().to_owned(),
                u32::try_from(order).map_err(|_| {
                    CodecError::malformed(format_args!("FreeCAD ordinal {order} exceeds u32"))
                })?,
                Some(record.id.clone()),
            )
        });
    }
    Ok(())
}

/// How a view or a page projects the model: `TechDraw` 1.1.4 `DrawView`,
/// `DrawPage`, `DrawProjGroup`, and `DrawViewPart` properties.
struct ViewProjection {
    scale_from: Option<ScaleFrom>,
    projection: Option<ProjectionAngle>,
    perspective: Option<PositiveLength>,
    hidden_line: Option<HiddenLineMethod>,
}

fn view_projection(
    ctx: &DecodeContext<'_>,
    owned: &[&PropertyRecord],
    runtime_type: &str,
) -> Result<ViewProjection, CodecError> {
    let scale_type = enumeration_property(ctx, owned, "ScaleType")?;
    let scale_from = match enumeration_entry(scale_type.as_ref(), &["Page", "Automatic", "Custom"])
    {
        Some("Page") => Some(ScaleFrom::Sheet),
        Some("Automatic") => Some(ScaleFrom::Fit),
        Some("Custom") => Some(ScaleFrom::Own),
        _ => None,
    };
    let projection_type = enumeration_property(ctx, owned, "ProjectionType")?;
    let built_in: &[&str] = if is_page_type(runtime_type) {
        &["First angle", "Third angle"]
    } else {
        &["Default", "First angle", "Third angle"]
    };
    let projection = match enumeration_entry(projection_type.as_ref(), built_in) {
        Some("First angle") => Some(ProjectionAngle::First),
        Some("Third angle") => Some(ProjectionAngle::Third),
        _ => None,
    };
    let perspective = if bool_property(ctx, owned, "Perspective")? == Some(true) {
        optional_scalar_property(
            ctx,
            owned,
            "Focus",
            &[
                "App::PropertyDistance",
                "App::PropertyLength",
                "App::PropertyFloat",
            ],
        )?
        .and_then(PositiveLength::new)
    } else {
        None
    };
    let hidden_line = bool_property(ctx, owned, "CoarseView")?.map(|coarse| {
        if coarse {
            HiddenLineMethod::Polygonal
        } else {
            HiddenLineMethod::Exact
        }
    });
    Ok(ViewProjection {
        scale_from,
        projection,
        perspective,
        hidden_line,
    })
}

fn is_registered_drawing_type(runtime_type: &str) -> bool {
    registered_drawing_kind(runtime_type).is_some()
}

fn is_page_type(runtime_type: &str) -> bool {
    matches!(
        runtime_type,
        "TechDraw::DrawPage" | "TechDraw::DrawPagePython"
    )
}

fn classify(runtime_type: &str) -> DrawingKind {
    registered_drawing_kind(runtime_type).unwrap_or(DrawingKind::Other)
}

fn registered_drawing_kind(runtime_type: &str) -> Option<DrawingKind> {
    match runtime_type {
        "TechDraw::DrawPage" | "TechDraw::DrawPagePython" => Some(DrawingKind::Page),
        "TechDraw::DrawTemplate"
        | "TechDraw::DrawTemplatePython"
        | "TechDraw::DrawSVGTemplate"
        | "TechDraw::DrawSVGTemplatePython"
        | "TechDraw::DrawDXFTemplate"
        | "TechDraw::DrawParametricTemplate"
        | "TechDraw::DrawParametricTemplatePython" => Some(DrawingKind::Template),
        "TechDraw::DrawView" | "TechDraw::DrawViewPython" | "TechDraw::DrawViewCollection" => {
            Some(DrawingKind::Other)
        }
        "TechDraw::DrawViewPart"
        | "TechDraw::DrawViewPartPython"
        | "TechDraw::DrawViewMulti"
        | "TechDraw::DrawViewMultiPython"
        | "TechDraw::DrawViewArch"
        | "TechDraw::DrawViewDraft"
        | "TechDraw::DrawViewDraftPython"
        | "TechDraw::DrawViewSpreadsheet"
        | "TechDraw::DrawViewSpreadsheetPython"
        | "TechDraw::DrawViewClip"
        | "TechDraw::DrawViewClipPython"
        | "TechDraw::DrawBrokenView"
        | "TechDraw::DrawBrokenViewPython" => Some(DrawingKind::View),
        "TechDraw::DrawViewDimension"
        | "TechDraw::DrawViewDimExtent"
        | "TechDraw::LandmarkDimension" => Some(DrawingKind::Dimension),
        "TechDraw::DrawViewSection"
        | "TechDraw::DrawViewSectionPython"
        | "TechDraw::DrawComplexSection"
        | "TechDraw::DrawComplexSectionPython" => Some(DrawingKind::Section),
        "TechDraw::DrawProjGroup" | "TechDraw::DrawProjGroupItem" => Some(DrawingKind::Projection),
        "TechDraw::DrawViewDetail" | "TechDraw::DrawViewDetailPython" => Some(DrawingKind::Detail),
        "TechDraw::DrawViewImage" | "TechDraw::DrawViewImagePython" => Some(DrawingKind::Image),
        "TechDraw::DrawViewAnnotation"
        | "TechDraw::DrawViewAnnotationPython"
        | "TechDraw::DrawRichAnno"
        | "TechDraw::DrawRichAnnoPython" => Some(DrawingKind::Annotation),
        "TechDraw::DrawViewBalloon" => Some(DrawingKind::Balloon),
        "TechDraw::DrawLeaderLine" | "TechDraw::DrawLeaderLinePython" => Some(DrawingKind::Leader),
        "TechDraw::DrawViewSymbol"
        | "TechDraw::DrawViewSymbolPython"
        | "TechDraw::DrawWeldSymbol"
        | "TechDraw::DrawWeldSymbolPython" => Some(DrawingKind::Symbol),
        "TechDraw::DrawHatch"
        | "TechDraw::DrawHatchPython"
        | "TechDraw::DrawGeomHatch"
        | "TechDraw::DrawGeomHatchPython"
        | "TechDraw::DrawTile"
        | "TechDraw::DrawTilePython"
        | "TechDraw::DrawTileWeld"
        | "TechDraw::DrawTileWeldPython" => Some(DrawingKind::Other),
        _ => None,
    }
}

fn scalar_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<FiniteReal>, CodecError> {
    let Some(property) = sole_named_property("drawing", properties, name)? else {
        return Ok(None);
    };
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(CodecError::malformed(format_args!(
            "drawing property {name} has no root value"
        )));
    };
    scalar_value(name, &property.type_name, value)
        .map(Some)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "drawing property {name} has an invalid scalar value"
            ))
        })
}

fn vector_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<FiniteVector<3>>, CodecError> {
    let Some(property) = sole_named_property("drawing", properties, name)? else {
        return Ok(None);
    };
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(CodecError::malformed(format_args!(
            "drawing property {name} has no root value"
        )));
    };
    vector_value(value).map(Some).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "drawing property {name} has an invalid vector value"
        ))
    })
}

fn source_links(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Vec<Option<crate::native::LinkTarget>>, CodecError> {
    let Some(property) = sole_named_property("drawing", properties, name)? else {
        return Ok(Vec::new());
    };
    let valid_type = match name {
        "Source" => is_link_carrier_type(&property.type_name),
        "XSource" => property.type_name == "App::PropertyXLinkList",
        "Sources" => property.type_name == "App::PropertyLinkList",
        "References2D" | "References3D" | "Source3d" => {
            property.type_name == "App::PropertyLinkSubList"
        }
        _ => false,
    };
    if !valid_type {
        return Err(CodecError::malformed(format_args!(
            "drawing source {name} has runtime type {}, which is not a source carrier",
            property.type_name
        )));
    }
    let is_list = is_link_list_type(&property.type_name);
    if !is_list && property.links().len() > 1 {
        return Err(CodecError::malformed(format_args!(
            "drawing source {name} has multiple targets"
        )));
    }
    let mut links = Vec::new();
    for link in property.links() {
        links.push(link.clone());
    }
    Ok(links)
}

fn is_link_carrier_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyLink"
            | "App::PropertyLinkChild"
            | "App::PropertyLinkGlobal"
            | "App::PropertyLinkHidden"
            | "App::PropertyLinkSub"
            | "App::PropertyLinkSubChild"
            | "App::PropertyLinkSubGlobal"
            | "App::PropertyLinkSubHidden"
            | "App::PropertyLinkList"
            | "App::PropertyLinkListChild"
            | "App::PropertyLinkListGlobal"
            | "App::PropertyLinkListHidden"
            | "App::PropertyLinkSubList"
            | "App::PropertyLinkSubListChild"
            | "App::PropertyLinkSubListGlobal"
            | "App::PropertyLinkSubListHidden"
            | "App::PropertyXLink"
            | "App::PropertyXLinkSub"
            | "App::PropertyXLinkSubHidden"
            | "App::PropertyXLinkList"
            | "App::PropertyXLinkSubList"
    )
}

fn is_link_list_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyLinkList"
            | "App::PropertyLinkListChild"
            | "App::PropertyLinkListGlobal"
            | "App::PropertyLinkListHidden"
            | "App::PropertyLinkSubList"
            | "App::PropertyLinkSubListChild"
            | "App::PropertyLinkSubListGlobal"
            | "App::PropertyLinkSubListHidden"
            | "App::PropertyXLinkList"
            | "App::PropertyXLinkSubList"
    )
}

fn typed_single_link<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<&'a crate::native::LinkTarget>, CodecError> {
    let Some(property) = typed_property(properties, name, type_name)? else {
        return Ok(None);
    };
    match property.links() {
        [] => Ok(None),
        [link] => Ok(link.as_ref()),
        _ => Err(CodecError::malformed(format_args!(
            "{name} has multiple links"
        ))),
    }
}

fn typed_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let Some(property) = sole_named_property("drawing", properties, name)? else {
        return Ok(None);
    };
    if property.type_name != type_name {
        return Err(CodecError::malformed(format_args!(
            "{name} has runtime type {}, expected {type_name}",
            property.type_name
        )));
    }
    Ok(Some(property))
}

fn drawing_parameters(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<BTreeMap<String, String>, CodecError> {
    const NAMES: &[&str] = &[
        "X",
        "Y",
        "Scale",
        "Direction",
        "Rotation",
        "Caption",
        "FormatSpec",
        "MeasureType",
        "LockPosition",
    ];
    const VALIDATED_ONLY_NAMES: &[&str] = &[
        "ScaleType",
        "ProjectionType",
        "XDirection",
        "FormatSpecOverTolerance",
        "FormatSpecUnderTolerance",
        "Type",
        "Perspective",
    ];
    let mut parameters = BTreeMap::new();
    for name in NAMES {
        let Some(property) = sole_named_property("drawing", properties, name)? else {
            continue;
        };
        validate_drawing_property(ctx, name, property)?;
        let value = root_value(ctx, property, name)?.ok_or_else(|| {
            CodecError::malformed(format_args!("drawing property {name} has no root value"))
        })?;
        parameters.insert((*name).to_owned(), value.raw_xml.clone());
    }
    for name in VALIDATED_ONLY_NAMES {
        if let Some(property) = sole_named_property("drawing", properties, name)? {
            validate_drawing_property(ctx, name, property)?;
        }
    }
    Ok(parameters)
}

fn validate_drawing_property(
    ctx: &DecodeContext<'_>,
    name: &str,
    property: &PropertyRecord,
) -> Result<(), CodecError> {
    if !drawing_property_type_matches(name, &property.type_name) {
        return Err(CodecError::malformed(format_args!(
            "drawing property {name} has runtime type {}, which is not its registered carrier",
            property.type_name
        )));
    }
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(CodecError::malformed(format_args!(
            "drawing property {name} has no root value"
        )));
    };
    if matches!(name, "Direction" | "XDirection") {
        if vector_value(value).is_none() {
            return Err(CodecError::malformed(format_args!(
                "drawing property {name} has an invalid vector value"
            )));
        }
    } else if matches!(name, "X" | "Y" | "Scale" | "Rotation")
        && scalar_value(name, &property.type_name, value).is_none()
    {
        return Err(CodecError::malformed(format_args!(
            "drawing property {name} has an invalid scalar value"
        )));
    }
    Ok(())
}

fn drawing_property_type_matches(name: &str, type_name: &str) -> bool {
    match name {
        "X" | "Y" => matches!(
            type_name,
            "App::PropertyDistance" | "App::PropertyLength" | "App::PropertyFloat"
        ),
        "Scale" => matches!(
            type_name,
            "App::PropertyFloatConstraint" | "App::PropertyFloat"
        ),
        "Rotation" => matches!(type_name, "App::PropertyAngle" | "App::PropertyFloat"),
        "Direction" | "XDirection" => type_name == "App::PropertyVector",
        "Caption" | "FormatSpec" | "FormatSpecOverTolerance" | "FormatSpecUnderTolerance" => {
            type_name == "App::PropertyString"
        }
        "ScaleType" | "MeasureType" | "Type" | "ProjectionType" => {
            type_name == "App::PropertyEnumeration"
        }
        "LockPosition" | "Perspective" => type_name == "App::PropertyBool",
        _ => false,
    }
}

fn ensure_unique_property_names(properties: &[&PropertyRecord]) -> Result<(), CodecError> {
    let mut names = BTreeSet::new();
    for property in properties {
        if names.contains(property.name.as_str()) {
            return Err(CodecError::malformed(format_args!(
                "drawing property {} occurs more than once",
                property.name
            )));
        }
        names.insert(property.name.as_str());
    }
    Ok(())
}

fn root_value<'a>(
    ctx: &DecodeContext<'_>,
    property: &'a PropertyRecord,
    name: &str,
) -> Result<Option<&'a ValueRecord>, CodecError> {
    let (expected_tag, allowed_extra_tags): (&str, &[&str]) = match name {
        "X" | "Y" | "Scale" | "Rotation" => ("Float", &[]),
        "Direction" | "XDirection" => ("PropertyVector", &[]),
        "Caption" | "FormatSpec" | "FormatSpecOverTolerance" | "FormatSpecUnderTolerance" => {
            ("String", &[])
        }
        "ScaleType" | "MeasureType" | "Type" | "ProjectionType" => ("Integer", &["CustomEnumList"]),
        "LockPosition" | "Perspective" => ("Bool", &[]),
        _ => return Ok(None),
    };
    let xml = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "drawing property {} has invalid XML: {error}",
                property.id
            ))
        })?;
    let property_node = xml.root_element();
    let mut selected_order = None;
    let mut order = 0;
    for node in property_node.descendants() {
        if !node.is_element() {
            continue;
        }
        if node.id() == property_node.id() {
            continue;
        }
        if node
            .parent()
            .is_some_and(|parent| parent.id() == property_node.id())
        {
            let tag = node.tag_name().name();
            if tag != expected_tag && !allowed_extra_tags.contains(&tag) {
                return Err(CodecError::malformed(format_args!(
                    "drawing property {name} has unexpected root element {tag}"
                )));
            }
            if tag == expected_tag && selected_order.replace(order).is_some() {
                return Err(CodecError::malformed(format_args!(
                    "drawing property {name} has multiple root values"
                )));
            }
        }
        order += 1;
    }
    match selected_order {
        None => Ok(None),
        Some(selected_order) => property
            .values()
            .iter()
            .find(|value| value.order == selected_order)
            .map(Some)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "drawing property {} has an unretained root value",
                    property.id
                ))
            }),
    }
}

fn scalar_value(name: &str, type_name: &str, value: &ValueRecord) -> Option<FiniteReal> {
    let allowed_attributes: &[&str] =
        if name == "Scale" && type_name == "App::PropertyFloatConstraint" {
            &["value", "min", "max", "step"]
        } else {
            &["value"]
        };
    let mut scalar = None;
    for (attribute, text) in &value.attributes {
        if !allowed_attributes.contains(&attribute.as_str()) {
            return None;
        }
        let admitted = FiniteReal::new(text.parse().ok()?)?;
        if attribute == "value" {
            scalar = Some(admitted);
        }
    }
    scalar
}

fn vector_value(value: &ValueRecord) -> Option<FiniteVector<3>> {
    let vector = [
        value.attributes.get("valueX")?.parse().ok()?,
        value.attributes.get("valueY")?.parse().ok()?,
        value.attributes.get("valueZ")?.parse().ok()?,
    ];
    FiniteVector::new(vector)
}

#[cfg(test)]
pub(crate) mod tests;

pub(crate) mod content;
