// SPDX-License-Identifier: Apache-2.0
//! TechDraw page and view objects authored from neutral drawings.

use std::collections::HashMap;

use cadmpeg_ir::drawings::DrawingKind;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;

pub(super) fn scalar(object: &mut Object, name: &str, kind: &str, value: f64) {
    let current = object.properties.get(name).and_then(|xml| {
        let document = roxmltree::Document::parse(xml).ok()?;
        document
            .root_element()
            .children()
            .find(|node| node.is_element())?
            .attribute("value")?
            .parse::<f64>()
            .ok()
    });
    if current != Some(value) {
        object.set(name, xml::scalar(name, kind, "Float", value));
    }
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
            object.properties.shift_remove(role);
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
            document
                .descendants()
                .filter(|node| node.has_tag_name("Link"))
                .map(|node| {
                    Some((
                        node.attribute("obj")?.to_owned(),
                        node.attribute("sub")?.to_owned(),
                    ))
                })
                .collect::<Option<Vec<_>>>()
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
        if !matches!(drawing.kind, DrawingKind::Page | DrawingKind::View) {
            continue;
        }
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
        if let Some(direction) = drawing.direction {
            let [x, y, z] = direction.get();
            let current = object.properties.get("Direction").and_then(|xml| {
                let document = roxmltree::Document::parse(xml).ok()?;
                let value = document
                    .root_element()
                    .children()
                    .find(|node| node.is_element())?;
                Some([
                    value.attribute("valueX")?.parse::<f64>().ok()?,
                    value.attribute("valueY")?.parse::<f64>().ok()?,
                    value.attribute("valueZ")?.parse::<f64>().ok()?,
                ])
            });
            if current != Some([x, y, z]) {
                object.set(
                    "Direction",
                    xml::property(
                        "Direction",
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
            }
        } else {
            object.properties.shift_remove("Direction");
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
                Some(
                    document
                        .descendants()
                        .filter(|node| node.has_tag_name("Link"))
                        .map(|node| node.attribute("value").map(str::to_owned))
                        .collect::<Option<Vec<_>>>()?,
                )
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
}
