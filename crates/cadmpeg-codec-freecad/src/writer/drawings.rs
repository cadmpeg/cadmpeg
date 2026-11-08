// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` page and view objects authored from neutral drawings.

use std::collections::HashMap;
use std::fmt::Write as _;

use cadmpeg_ir::drawings::DrawingKind;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;

pub(super) fn scalar_value(object: &Object, name: &str) -> Option<f64> {
    let xml = object.properties.get(name)?;
    let document = roxmltree::Document::parse(xml).ok()?;
    document
        .root_element()
        .children()
        .find(roxmltree::Node::is_element)?
        .attribute("value")?
        .parse::<f64>()
        .ok()
}

pub(super) fn scalar(object: &mut Object, name: &str, kind: &str, value: f64) {
    if scalar_value(object, name) != Some(value) {
        object.set(name, xml::scalar(name, kind, "Float", value));
    }
}

pub(super) fn parameters(
    object: &mut Object,
    (owner, record): (&str, &str),
    parameters: &std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    properties: &[crate::native::PropertyRecord],
    (names, available): (&HashMap<String, String>, &std::collections::HashSet<String>),
    dedicated: &[&str],
    losses: &mut Vec<LossNote>,
) {
    for (name, value) in parameters {
        if dedicated.contains(&name.as_str()) {
            continue;
        }
        let Some(source) = properties
            .iter()
            .find(|property| property.owner == owner && property.name == name.as_str())
        else {
            let authored = match name.as_str() {
                "Caption" | "FormatSpec" => Some(xml::string(name.as_str(), value)),
                "ScaleType" | "MeasureType" | "ProjectionType" => {
                    value.parse::<i64>().ok().map(|value| {
                        xml::scalar(name.as_str(), "App::PropertyEnumeration", "Integer", value)
                    })
                }
                "LockPosition" => match value.as_str() {
                    "true" => Some(xml::scalar(name.as_str(), "App::PropertyBool", "Bool", 1)),
                    "false" => Some(xml::scalar(name.as_str(), "App::PropertyBool", "Bool", 0)),
                    _ => None,
                },
                _ => None,
            };
            if let Some(xml) = authored {
                object.set(name.as_str(), xml);
            } else {
                object.properties.shift_remove(name.as_str());
                losses.push(
                    FreecadLossCode::DrawingParameterNotTransferred.note(format!(
                        "{record} parameter {name} has no writable property and is omitted"
                    )),
                );
            }
            continue;
        };
        if crate::attributes::semantic_text(source).as_deref() == Some(value.as_str())
            && object.properties.contains_key(name.as_str())
        {
            continue;
        }
        let encoded = 'mapping: {
            let mut property = source.clone();
            let Some(crate::native::PropertyBody::Persisted { values, .. }) = &mut property.body
            else {
                break 'mapping None;
            };
            let mut roots = values.iter_mut().filter(|entry| {
                entry.parent.is_none()
                    && matches!(entry.tag.as_str(), "String" | "Integer" | "Float" | "Bool")
            });
            let Some(entry) = roots.next() else {
                break 'mapping None;
            };
            if roots.next().is_some() {
                break 'mapping None;
            }
            let emitted = if entry.tag == "Bool" {
                match value.as_str() {
                    "true" => "1",
                    "false" => "0",
                    _ => break 'mapping None,
                }
            } else {
                value.as_str()
            };
            entry.attributes.insert("value".into(), emitted.to_owned());
            if crate::attributes::semantic_text(&property).as_deref() != Some(value.as_str()) {
                break 'mapping None;
            }
            xml::native_named(&property, names, available)
        };
        if let Some(xml) = encoded {
            object.set(name.as_str(), xml);
        } else {
            object.properties.shift_remove(name.as_str());
            losses.push(
                FreecadLossCode::DrawingParameterNotTransferred.note(format!(
                    "{record} parameter {name} does not parse as property {} and is omitted",
                    source.id
                )),
            );
        }
    }
}

pub(super) fn dimension_kind(
    annotation: &cadmpeg_ir::semantic_annotations::SemanticAnnotation,
    object: &mut Object,
) {
    use cadmpeg_ir::semantic_annotations::DimensionKind;
    let Some(kind) = &annotation.dimension_kind else {
        object.properties.shift_remove("Type");
        return;
    };
    let (number, label) = match kind {
        DimensionKind::Distance {} => (Some("0"), "Distance"),
        DimensionKind::HorizontalDistance {} => (Some("1"), "DistanceX"),
        DimensionKind::VerticalDistance {} => (Some("2"), "DistanceY"),
        DimensionKind::Radius {} => (Some("4"), "Radius"),
        DimensionKind::Diameter {} => (Some("5"), "Diameter"),
        DimensionKind::Angle {} => (Some("6"), "Angle"),
        DimensionKind::ThreePointAngle {} => (None, "Angle3Pt"),
        DimensionKind::Other { native_type } => (None, native_type.as_str()),
    };
    let current = object.properties.get("Type").and_then(|xml| {
        let document = roxmltree::Document::parse(xml).ok()?;
        let root = document.root_element();
        let selected = root
            .children()
            .find(|node| node.has_tag_name("Integer"))?
            .attribute("value")?;
        if let Some(list) = root
            .children()
            .find(|node| node.has_tag_name("CustomEnumList"))
        {
            let index = selected.parse::<usize>().ok()?;
            Some(
                list.children()
                    .filter(|node| node.has_tag_name("Enum"))
                    .nth(index)?
                    .attribute("value")?
                    == label,
            )
        } else {
            Some(number == Some(selected) || matches!(kind, DimensionKind::Other { native_type } if native_type == selected))
        }
    });
    if current == Some(true) {
        return;
    }
    let content = if let Some(number) = number {
        xml::element("Integer", &[("value", number.to_owned())])
    } else {
        format!("<Integer value=\"0\" CustomEnum=\"true\"/><CustomEnumList count=\"1\">{}</CustomEnumList>", xml::element("Enum", &[("value", label.to_owned())]))
    };
    object.set(
        "Type",
        xml::property("Type", "App::PropertyEnumeration", &content),
    );
}

pub(super) fn dimension_references(
    annotation: &cadmpeg_ir::semantic_annotations::SemanticAnnotation,
    object: &mut Object,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) {
    use cadmpeg_ir::references::ReferenceTarget;
    for role in ["References2D", "References3D", "Source3d"] {
        let Some(selections) = annotation.references.get(role) else {
            if annotation
                .parameters
                .get(role)
                .is_some_and(|value| value == "[]")
            {
                let empty = object.properties.get(role).and_then(|xml| {
                    let document = roxmltree::Document::parse(xml).ok()?;
                    if document.root_element().attribute("type") != Some("App::PropertyLinkSubList")
                    {
                        return Some(false);
                    }
                    let list = document
                        .descendants()
                        .find(|node| node.has_tag_name("LinkSubList"))?;
                    Some(
                        list.attribute("count") == Some("0")
                            && !list.children().any(|node| node.has_tag_name("Link")),
                    )
                });
                if empty != Some(true) {
                    object.set(
                        role,
                        xml::property(
                            role,
                            "App::PropertyLinkSubList",
                            "<LinkSubList count=\"0\"></LinkSubList>",
                        ),
                    );
                }
            } else {
                object.properties.shift_remove(role);
            }
            continue;
        };
        let mut links = Vec::new();
        for selection in selections {
            let target = match &selection.target {
                ReferenceTarget::Null => Some(String::new()),
                ReferenceTarget::Local(id) => names.get(id).cloned(),
                ReferenceTarget::External { .. } => None,
            };
            let Some(target) = target else {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "annotation {} has an unresolved {role} reference",
                    annotation.id
                )));
                continue;
            };
            for sub in &selection.subelements {
                links.push((target.clone(), sub.clone()));
            }
            if selection.subelements.is_empty() {
                links.push((target, String::new()));
            }
        }
        let current = object.properties.get(role).and_then(|xml| {
            let document = roxmltree::Document::parse(xml).ok()?;
            let root = document.root_element();
            if root.attribute("type") != Some("App::PropertyLinkSubList") {
                return None;
            }
            let list = root
                .children()
                .find(|node| node.has_tag_name("LinkSubList"))?;
            let found = list
                .children()
                .filter(|node| node.has_tag_name("Link"))
                .map(|node| {
                    Some((
                        node.attribute("obj")?.to_owned(),
                        node.attribute("sub")?.to_owned(),
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            (list.attribute("count")?.parse::<usize>().ok()? == found.len()).then_some(found)
        });
        if current.as_ref() == Some(&links) {
            continue;
        }
        let content = links
            .iter()
            .map(|(target, sub)| {
                xml::element("Link", &[("obj", target.clone()), ("sub", sub.clone())])
            })
            .collect::<String>();
        object.set(
            role,
            xml::property(
                role,
                "App::PropertyLinkSubList",
                &format!(
                    "<LinkSubList count=\"{}\">{content}</LinkSubList>",
                    links.len()
                ),
            ),
        );
    }
}

fn view_xsources(
    drawing: &cadmpeg_ir::drawings::Drawing,
    object: &mut Object,
    role: &str,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) {
    use cadmpeg_ir::references::ReferenceTarget;
    let Some(selections) = drawing.relationships.get(role) else {
        object.properties.shift_remove(role);
        return;
    };
    let mut links = Vec::new();
    for selection in selections {
        let (file, target) = match &selection.target {
            ReferenceTarget::Null => (String::new(), String::new()),
            ReferenceTarget::Local(id) => {
                let Some(target) = names.get(id) else {
                    losses.push(FreecadLossCode::DrawingSourceNotTransferred.note(format!(
                        "drawing {} has an unresolved {role} reference",
                        drawing.id
                    )));
                    continue;
                };
                (String::new(), target.clone())
            }
            ReferenceTarget::External { document, object } => (document.clone(), object.clone()),
        };
        links.push((file, target, selection.subelements.clone()));
    }
    let current = object.properties.get(role).and_then(|xml| {
        let document = roxmltree::Document::parse(xml).ok()?;
        let root = document
            .root_element()
            .children()
            .find(roxmltree::Node::is_element)?;
        let mut nodes = Vec::new();
        if root.has_tag_name("XLink") {
            nodes.push(root);
        } else if root.has_tag_name("XLinkSubList") {
            nodes.extend(root.children().filter(|node| node.has_tag_name("XLink")));
        } else {
            return None;
        }
        nodes
            .into_iter()
            .map(|node| {
                let subs = if let Some(sub) = node.attribute("sub") {
                    vec![node.attribute("shadowed").unwrap_or(sub).to_owned()]
                } else {
                    node.children()
                        .filter(|child| child.has_tag_name("Sub"))
                        .map(|child| {
                            child
                                .attribute("shadowed")
                                .or_else(|| child.attribute("value"))
                                .map(str::to_owned)
                        })
                        .collect::<Option<Vec<_>>>()?
                };
                Some((
                    node.attribute("file").unwrap_or("").to_owned(),
                    node.attribute("name")?.to_owned(),
                    subs,
                ))
            })
            .collect::<Option<Vec<_>>>()
    });
    if current.as_ref() == Some(&links) {
        return;
    }
    let mut output = String::new();
    for (file, target, subs) in &links {
        if subs.is_empty() {
            output.push_str(&xml::element(
                "XLink",
                &[("file", file.clone()), ("name", target.clone())],
            ));
        } else {
            let children = subs
                .iter()
                .map(|sub| xml::element("Sub", &[("value", sub.clone())]))
                .collect::<String>();
            write!(
                output,
                "<XLink file=\"{}\" name=\"{}\" count=\"{}\">{children}</XLink>",
                xml::escaped(file),
                xml::escaped(target),
                subs.len()
            )
            .expect("write to string");
        }
    }
    object.set(
        role,
        xml::property(
            role,
            "App::PropertyXLinkList",
            &format!(
                "<XLinkSubList count=\"{}\">{output}</XLinkSubList>",
                links.len()
            ),
        ),
    );
}

fn view_sources(
    drawing: &cadmpeg_ir::drawings::Drawing,
    object: &mut Object,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) {
    use cadmpeg_ir::references::ReferenceTarget;
    if drawing.relationships.contains_key("XSource") {
        view_xsources(drawing, object, "XSource", names, losses);
    } else {
        let empty = object
            .properties
            .get("XSource")
            .and_then(|xml| {
                let document = roxmltree::Document::parse(xml).ok()?;
                let root = document
                    .root_element()
                    .children()
                    .find(roxmltree::Node::is_element)?;
                Some(
                    root.has_tag_name("XLinkSubList")
                        && root
                            .attribute("count")
                            .and_then(|count| count.parse::<u64>().ok())
                            == Some(0),
                )
            })
            .unwrap_or(false);
        if !empty {
            object.properties.shift_remove("XSource");
        }
    }
    for role in ["Source", "Sources"] {
        let xlink_source = object
            .properties
            .get(role)
            .and_then(|xml| {
                let document = roxmltree::Document::parse(xml).ok()?;
                Some(
                    document
                        .root_element()
                        .attribute("type")?
                        .starts_with("App::PropertyXLink"),
                )
            })
            .unwrap_or(false);
        if role == "Source"
            && drawing.relationships.get(role).is_some_and(|selections| {
                xlink_source
                    || selections.iter().any(|selection| {
                        matches!(&selection.target, ReferenceTarget::External { .. })
                    })
            })
        {
            view_xsources(drawing, object, role, names, losses);
            continue;
        }
        let Some(selections) = drawing.relationships.get(role) else {
            let empty = object
                .properties
                .get(role)
                .and_then(|xml| {
                    let document = roxmltree::Document::parse(xml).ok()?;
                    let root = document
                        .root_element()
                        .children()
                        .find(roxmltree::Node::is_element)?;
                    Some(
                        matches!(root.tag_name().name(), "LinkList" | "LinkSubList")
                            && root
                                .attribute("count")
                                .and_then(|count| count.parse::<u64>().ok())
                                == Some(0),
                    )
                })
                .unwrap_or(false);
            if !empty {
                object.properties.shift_remove(role);
            }
            continue;
        };
        let mut links = Vec::new();
        for selection in selections {
            let target = match &selection.target {
                ReferenceTarget::Null => Some(String::new()),
                ReferenceTarget::Local(id) => names.get(id).cloned(),
                ReferenceTarget::External { .. } => None,
            };
            let Some(target) = target else {
                losses.push(FreecadLossCode::DrawingSourceNotTransferred.note(format!(
                    "drawing {} has an unresolved {role} reference",
                    drawing.id
                )));
                continue;
            };
            if role == "Sources" && !selection.subelements.is_empty() {
                losses.push(FreecadLossCode::DrawingSourceNotTransferred.note(format!(
                    "drawing {} {role} subelements are omitted",
                    drawing.id
                )));
            }
            links.push((target, selection.subelements.clone()));
        }
        let current = object.properties.get(role).and_then(|xml| {
            let document = roxmltree::Document::parse(xml).ok()?;
            let root = document
                .root_element()
                .children()
                .find(roxmltree::Node::is_element)?;
            match root.tag_name().name() {
                "Link" => Some(vec![(root.attribute("value")?.to_owned(), Vec::new())]),
                "LinkList" => root
                    .children()
                    .filter(|node| node.has_tag_name("Link"))
                    .map(|node| Some((node.attribute("value")?.to_owned(), Vec::new())))
                    .collect::<Option<Vec<_>>>(),
                "LinkSub" => Some(vec![(
                    root.attribute("value")?.to_owned(),
                    root.children()
                        .filter(|node| node.has_tag_name("Sub"))
                        .map(|node| {
                            node.attribute("shadowed")
                                .or_else(|| node.attribute("value"))
                                .map(str::to_owned)
                        })
                        .collect::<Option<Vec<_>>>()?,
                )]),
                "XLink" if root.attribute("file").unwrap_or("").is_empty() => {
                    let subs = if let Some(sub) = root.attribute("sub") {
                        vec![root.attribute("shadowed").unwrap_or(sub).to_owned()]
                    } else {
                        root.children()
                            .filter(|node| node.has_tag_name("Sub"))
                            .map(|node| {
                                node.attribute("shadowed")
                                    .or_else(|| node.attribute("value"))
                                    .map(str::to_owned)
                            })
                            .collect::<Option<Vec<_>>>()?
                    };
                    Some(vec![(root.attribute("name")?.to_owned(), subs)])
                }
                "LinkSubList" => root
                    .children()
                    .filter(|node| node.has_tag_name("Link"))
                    .map(|node| {
                        Some((
                            node.attribute("obj")?.to_owned(),
                            vec![node
                                .attribute("shadowed")
                                .or_else(|| node.attribute("sub"))?
                                .to_owned()],
                        ))
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            }
        });
        if current.as_ref() == Some(&links) {
            continue;
        }
        let single = role == "Source" && links.len() == 1;
        if single && !links[0].1.is_empty() {
            let subs = links[0]
                .1
                .iter()
                .map(|sub| xml::element("Sub", &[("value", sub.clone())]))
                .collect::<String>();
            let content = format!(
                "<LinkSub value=\"{}\" count=\"{}\">{subs}</LinkSub>",
                xml::escaped(&links[0].0),
                links[0].1.len()
            );
            object.set(role, xml::property(role, "App::PropertyLinkSub", &content));
        } else if single {
            object.set(
                role,
                xml::property(
                    role,
                    "App::PropertyLink",
                    &xml::element("Link", &[("value", links[0].0.clone())]),
                ),
            );
        } else if role == "Source" && links.iter().any(|(_, subs)| !subs.is_empty()) {
            view_xsources(drawing, object, role, names, losses);
        } else {
            let content = links
                .iter()
                .map(|(target, _)| xml::element("Link", &[("value", target.clone())]))
                .collect::<String>();
            object.set(
                role,
                xml::property(
                    role,
                    "App::PropertyLinkList",
                    &format!("<LinkList count=\"{}\">{content}</LinkList>", links.len()),
                ),
            );
        }
    }
}

pub(super) fn write(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) {
    let mut drawings: Vec<_> = ir.model.drawings.iter().enumerate().collect();
    drawings.sort_by_key(|(index, drawing)| (drawing.order, *index));
    for (index, drawing) in &drawings {
        let kind = match drawing.kind {
            DrawingKind::Page => Some("TechDraw::DrawPage"),
            DrawingKind::View => Some("TechDraw::DrawViewPart"),
            DrawingKind::Annotation => Some("TechDraw::DrawViewAnnotation"),
            DrawingKind::Dimension => Some("TechDraw::DrawViewDimension"),
            DrawingKind::Balloon => Some("TechDraw::DrawViewBalloon"),
            DrawingKind::Symbol => Some("TechDraw::DrawViewSymbol"),
            _ => None,
        };
        let name = names
            .get(&drawing.object)
            .filter(|name| {
                objects.iter().any(|object| {
                    object.name == **name
                        && (object.kind.starts_with("TechDraw::")
                            || object.kind.starts_with("Drawing::"))
                })
            })
            .cloned();
        let name = if let Some(name) = name {
            name
        } else if let Some(kind) = kind {
            let mut name = format!("Drawing{index}");
            while objects.iter().any(|object| object.name == name) {
                name.push('_');
            }
            objects.push(Object::new(name.clone(), kind, &name));
            name
        } else {
            continue;
        };
        names.insert(drawing.object.clone(), name.clone());
        names.insert(drawing.id.as_str().to_owned(), name);
    }
    for (_, drawing) in drawings {
        let Some(name) = names.get(drawing.id.as_str()) else {
            continue;
        };
        let Some(object) = objects.iter_mut().find(|object| object.name == *name) else {
            continue;
        };
        if let Some(position) = drawing.position {
            let [x, y] = position.get();
            scalar(object, "X", "App::PropertyDistance", x);
            scalar(object, "Y", "App::PropertyDistance", y);
        } else {
            object.properties.shift_remove("X");
            object.properties.shift_remove("Y");
        }
        if let Some(scale) = drawing.scale {
            scalar(object, "Scale", "App::PropertyFloatConstraint", scale.get());
        } else {
            object.properties.shift_remove("Scale");
        }
        if let Some(rotation) = drawing.rotation_degrees {
            scalar(object, "Rotation", "App::PropertyAngle", rotation.get());
        } else {
            object.properties.shift_remove("Rotation");
        }
        for (property, direction) in [
            ("Direction", drawing.direction),
            ("XDirection", drawing.x_direction),
        ] {
            let current = object.properties.get(property).and_then(|xml| {
                let document = roxmltree::Document::parse(xml).ok()?;
                let root = document.root_element();
                if root.attribute("type") != Some("App::PropertyVector") {
                    return None;
                }
                let value = root.children().find(roxmltree::Node::is_element)?;
                Some([
                    value.attribute("valueX")?.parse::<f64>().ok()?,
                    value.attribute("valueY")?.parse::<f64>().ok()?,
                    value.attribute("valueZ")?.parse::<f64>().ok()?,
                ])
            });
            if let Some(direction) = direction {
                let [x, y, z] = direction.get();
                if current == Some([x, y, z]) {
                    continue;
                }
                object.set(
                    property,
                    xml::property(
                        property,
                        "App::PropertyVector",
                        &xml::element(
                            "PropertyVector",
                            &[
                                ("valueX", x.to_string()),
                                ("valueY", y.to_string()),
                                ("valueZ", z.to_string()),
                            ],
                        ),
                    ),
                );
            } else if property == "Direction"
                || current
                    .is_some_and(|value| cadmpeg_ir::units::NonzeroVector::new(value).is_some())
            {
                object.properties.shift_remove(property);
            }
        }
        if drawing.kind == DrawingKind::View {
            view_sources(drawing, object, names, losses);
        }
        if drawing.kind != DrawingKind::Page {
            continue;
        }
        if let Some(template) = &drawing.template {
            if let Some(target) = names.get(template.as_str()) {
                let current = object.properties.get("Template").and_then(|xml| {
                    let document = roxmltree::Document::parse(xml).ok()?;
                    document
                        .descendants()
                        .find(|node| node.has_tag_name("Link"))?
                        .attribute("value")
                        .map(str::to_owned)
                });
                if current.as_deref() != Some(target.as_str()) {
                    object.set(
                        "Template",
                        xml::property(
                            "Template",
                            "App::PropertyLink",
                            &xml::element("Link", &[("value", target.clone())]),
                        ),
                    );
                }
            } else {
                object.properties.shift_remove("Template");
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "drawing {} references an unavailable template {}",
                    drawing.id, template
                )));
            }
        } else if !drawing.relationships.contains_key("Template") {
            object.properties.shift_remove("Template");
        }
        if let Some(views) = drawing.relationships.get("Views") {
            let mut links = String::new();
            let mut targets = Vec::new();
            let mut count = 0;
            for selection in views {
                let Some(target) = selection
                    .local_target()
                    .and_then(|target| names.get(target))
                else {
                    losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "drawing {} has an unresolved page view",
                        drawing.id
                    )));
                    continue;
                };
                if !selection.subelements.is_empty() {
                    losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "drawing {} page view subelements are omitted",
                        drawing.id
                    )));
                }
                links.push_str(&xml::element("Link", &[("value", target.clone())]));
                targets.push(target.as_str());
                count += 1;
            }
            let current = object.properties.get("Views").and_then(|xml| {
                let document = roxmltree::Document::parse(xml).ok()?;
                document
                    .descendants()
                    .filter(|node| node.has_tag_name("Link"))
                    .map(|node| node.attribute("value").map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            });
            if current
                .as_ref()
                .is_none_or(|current| current.iter().map(String::as_str).ne(targets))
            {
                object.set(
                    "Views",
                    xml::property(
                        "Views",
                        "App::PropertyLinkList",
                        &format!("<LinkList count=\"{count}\">{links}</LinkList>"),
                    ),
                );
            }
        } else {
            object.properties.shift_remove("Views");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::references::{ReferenceSelection, ReferenceTarget};
    use cadmpeg_ir::semantic_annotations::{
        SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
    };

    #[test]
    fn mapped_dimension_fields_are_not_reported_as_an_omitted_arena() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::semantic_annotations::DimensionKind;
        use cadmpeg_ir::{Codec, DecodeOptions};

        let mut ir = CadIr::empty();
        let mut annotation = SemanticAnnotation::new(
            SemanticAnnotationId::mint("cadir:model:annotation#dimension").unwrap(),
            "cadir:model:feature#dimension".into(),
            SemanticAnnotationKind::Dimension,
            "TechDraw::DrawViewDimension".into(),
            0,
            "fcstd:native:annotation#dimension".into(),
        );
        annotation.dimension_kind = Some(DimensionKind::Distance {});
        annotation.text = vec!["%0.2f".into()];
        annotation.format = Some("%0.2f".into());
        annotation.references.insert(
            cadmpeg_core::text::NonBlankString::new("References2D").unwrap(),
            vec![ReferenceSelection::new(ReferenceTarget::Null, Vec::new())],
        );
        let id = annotation.id.clone();
        ir.model.semantic_annotations.push(annotation);
        let plan = crate::FcstdCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        assert!(!plan.report().losses.iter().any(|loss| loss.code
            == FreecadLossCode::ExportArenaNotTransferred.note("").code
            && loss.message.contains(id.as_str())));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let dimension = &output.ir().model.semantic_annotations[0];
        assert_eq!(dimension.dimension_kind, Some(DimensionKind::Distance {}));
        assert_eq!(dimension.format.as_deref(), Some("%0.2f"));
        assert!(dimension.references.contains_key("References2D"));
    }

    #[test]
    fn mapped_view_source_and_x_direction_are_not_reported_as_omitted() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::{Codec, DecodeOptions};

        let mut ir = CadIr::empty();
        let mut drawing = cadmpeg_ir::drawings::Drawing::new(
            cadmpeg_ir::drawings::DrawingId::mint("cadir:model:drawing#view").unwrap(),
            "cadir:model:feature#view".into(),
            DrawingKind::View,
            "TechDraw::DrawViewPart".into(),
            0,
            None,
        );
        drawing.x_direction = cadmpeg_ir::units::NonzeroVector::new([1.0, 0.0, 0.0]);
        drawing.relationships.insert(
            cadmpeg_core::text::NonBlankString::new("Source").unwrap(),
            vec![ReferenceSelection::new(ReferenceTarget::Null, Vec::new())],
        );
        let id = drawing.id.clone();
        ir.model.drawings.push(drawing);
        let plan = crate::FcstdCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        assert!(!plan.report().losses.iter().any(|loss| loss.code
            == FreecadLossCode::ExportArenaNotTransferred.note("").code
            && loss.message.contains(id.as_str())));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let view = &output.ir().model.drawings[0];
        assert_eq!(view.x_direction.unwrap().get(), [1.0, 0.0, 0.0]);
        assert!(view.relationships.contains_key("Source"));
    }

    #[test]
    fn edited_view_parameters_redecode_from_their_native_properties() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::{Codec, DecodeOptions};

        let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View"/></Objects><ObjectData Count="1"><Object name="View"><Properties Count="3"><Property name="Caption" type="App::PropertyString"><String value="before"/></Property><Property name="ScaleType" type="App::PropertyEnumeration"><Integer value="0"/><CustomEnumList count="2"><Enum value="Page"/><Enum value="Custom"/></CustomEnumList></Property><Property name="LockPosition" type="App::PropertyBool"><Bool value="0"/></Property></Properties></Object></ObjectData></Document>"#;
        let source = crate::test_support::test_archive::archive(xml);
        let input = crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut ir = input.ir().clone();
        let view = &mut ir.model.drawings[0];
        *view.parameters.get_mut("Caption").unwrap() = "after".into();
        *view.parameters.get_mut("ScaleType").unwrap() = "1".into();
        *view.parameters.get_mut("LockPosition").unwrap() = "true".into();
        let plan = crate::FcstdCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let parameters = &output.ir().model.drawings[0].parameters;
        assert_eq!(parameters["Caption"], "after");
        assert_eq!(parameters["ScaleType"], "1");
        assert_eq!(parameters["LockPosition"], "true");
    }

    #[test]
    fn authored_view_parameters_use_standard_techdraw_properties() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::{Codec, DecodeOptions};

        let mut ir = CadIr::empty();
        let mut view = cadmpeg_ir::drawings::Drawing::new(
            cadmpeg_ir::drawings::DrawingId::mint("cadir:model:drawing#view").unwrap(),
            "cadir:model:feature#view".into(),
            DrawingKind::View,
            "TechDraw::DrawViewPart".into(),
            0,
            None,
        );
        for (name, value) in [
            ("Caption", "authored"),
            ("ScaleType", "1"),
            ("LockPosition", "true"),
        ] {
            view.parameters.insert(
                cadmpeg_core::text::NonBlankString::new(name).unwrap(),
                value.into(),
            );
        }
        ir.model.drawings.push(view);
        let plan = crate::FcstdCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let parameters = &output.ir().model.drawings[0].parameters;
        assert_eq!(parameters["Caption"], "authored");
        assert_eq!(parameters["ScaleType"], "1");
        assert_eq!(parameters["LockPosition"], "true");
    }

    #[test]
    fn unencodable_view_parameter_does_not_replay_its_old_value() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::{Codec, DecodeOptions};

        let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View"/></Objects><ObjectData Count="1"><Object name="View"><Properties Count="2"><Property name="Caption" type="App::PropertyString"><String value="kept"/></Property><Property name="ScaleType" type="App::PropertyEnumeration"><Integer value="0"/></Property></Properties></Object></ObjectData></Document>"#;
        let source = crate::test_support::test_archive::archive(xml);
        let input = crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut ir = input.ir().clone();
        *ir.model.drawings[0]
            .parameters
            .get_mut("ScaleType")
            .unwrap() = "invalid".into();
        let plan = crate::FcstdCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("ScaleType") && loss.message.contains("omitted")));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let parameters = &output.ir().model.drawings[0].parameters;
        assert!(!parameters.contains_key("ScaleType"));
        assert_eq!(parameters["Caption"], "kept");
    }

    #[test]
    fn unresolvable_view_source_is_named_in_a_specific_loss() {
        let mut drawing = cadmpeg_ir::drawings::Drawing::new(
            cadmpeg_ir::drawings::DrawingId::mint("cadir:model:drawing#view").unwrap(),
            "view".into(),
            DrawingKind::View,
            "TechDraw::DrawViewPart".into(),
            0,
            None,
        );
        drawing.relationships.insert(
            cadmpeg_core::text::NonBlankString::new("Source").unwrap(),
            vec![ReferenceSelection::new(
                ReferenceTarget::Local("missing".into()),
                Vec::new(),
            )],
        );
        let mut object = Object::new("View".into(), "TechDraw::DrawViewPart", "View");
        let mut losses = Vec::new();
        view_sources(&drawing, &mut object, &HashMap::new(), &mut losses);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            FreecadLossCode::DrawingSourceNotTransferred.note("").code
        );
        assert!(losses[0].message.contains(drawing.id.as_str()));
    }

    #[test]
    fn dimension_references_use_emitted_object_names() {
        let mut annotation = SemanticAnnotation::new(
            SemanticAnnotationId::mint("cadir:model:annotation#dimension").unwrap(),
            "dimension".into(),
            SemanticAnnotationKind::Dimension,
            "TechDraw::DrawViewDimension".into(),
            0,
            "native".into(),
        );
        annotation.references.insert(
            cadmpeg_core::text::NonBlankString::new("References2D").unwrap(),
            vec![ReferenceSelection::new(
                ReferenceTarget::Local("view-id".into()),
                vec!["Edge2".into()],
            )],
        );
        let mut object = Object::new(
            "Dimension".into(),
            "TechDraw::DrawViewDimension",
            "Dimension",
        );
        let names = HashMap::from([("view-id".into(), "View".into())]);
        let mut losses = Vec::new();
        dimension_references(&annotation, &mut object, &names, &mut losses);
        let document =
            roxmltree::Document::parse(object.properties.get("References2D").unwrap()).unwrap();
        let link = document
            .descendants()
            .find(|node| node.has_tag_name("Link"))
            .unwrap();
        assert_eq!(link.attribute("obj"), Some("View"));
        assert_eq!(link.attribute("sub"), Some("Edge2"));
        assert!(losses.is_empty());
    }

    #[test]
    fn dimension_reference_replaces_incompatible_native_property() {
        let mut annotation = SemanticAnnotation::new(
            SemanticAnnotationId::mint("cadir:model:annotation#dimension").unwrap(),
            "dimension".into(),
            SemanticAnnotationKind::Dimension,
            "TechDraw::DrawViewDimension".into(),
            0,
            "native".into(),
        );
        annotation.references.insert(
            cadmpeg_core::text::NonBlankString::new("References2D").unwrap(),
            vec![ReferenceSelection::new(
                ReferenceTarget::Local("view-id".into()),
                vec!["Edge2".into()],
            )],
        );
        let mut object = Object::new(
            "Dimension".into(),
            "TechDraw::DrawViewDimension",
            "Dimension",
        );
        object.set(
            "References2D",
            xml::property(
                "References2D",
                "App::PropertyString",
                "<Link obj=\"View\" sub=\"Edge2\"/>",
            ),
        );
        dimension_references(
            &annotation,
            &mut object,
            &HashMap::from([("view-id".into(), "View".into())]),
            &mut Vec::new(),
        );
        let document = roxmltree::Document::parse(&object.properties["References2D"]).unwrap();
        assert_eq!(
            document.root_element().attribute("type"),
            Some("App::PropertyLinkSubList")
        );
        let list = document
            .descendants()
            .find(|node| node.has_tag_name("LinkSubList"))
            .unwrap();
        assert_eq!(list.attribute("count"), Some("1"));
        let link = list
            .children()
            .find(|node| node.has_tag_name("Link"))
            .unwrap();
        assert_eq!(link.attribute("obj"), Some("View"));
        assert_eq!(link.attribute("sub"), Some("Edge2"));
    }

    #[test]
    fn empty_dimension_reference_parameter_authors_an_empty_list() {
        let mut annotation = SemanticAnnotation::new(
            SemanticAnnotationId::mint("cadir:model:annotation#dimension").unwrap(),
            "dimension".into(),
            SemanticAnnotationKind::Dimension,
            "TechDraw::DrawViewDimension".into(),
            0,
            "native".into(),
        );
        annotation.parameters.insert(
            cadmpeg_core::text::NonBlankString::new("References3D").unwrap(),
            "[]".into(),
        );
        let mut object = Object::new(
            "Dimension".into(),
            "TechDraw::DrawViewDimension",
            "Dimension",
        );
        let mut losses = Vec::new();
        dimension_references(&annotation, &mut object, &HashMap::new(), &mut losses);
        let document = roxmltree::Document::parse(&object.properties["References3D"]).unwrap();
        let list = document
            .descendants()
            .find(|node| node.has_tag_name("LinkSubList"))
            .unwrap();
        assert_eq!(list.attribute("count"), Some("0"));
        assert!(!list.children().any(|node| node.has_tag_name("Link")));
        assert!(losses.is_empty());
    }
}
