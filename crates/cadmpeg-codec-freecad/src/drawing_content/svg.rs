// SPDX-License-Identifier: Apache-2.0
//! Bounded SVG template subset with explicit refusals for other constructs.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::drawings::{DrawingCurveGeometry, DrawingSheet, TemplateGeometry, TemplateText};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{PositiveLength, PositiveReal};
use cadmpeg_ir::units::FiniteVector;
use roxmltree::{Document, Node};

use crate::loss::FreecadLossCode;

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const EDITABLE_NAMESPACE: &str = "http://www.freecadweb.org/wiki/index.php?title=Svg_Namespace";

pub(super) fn parse(
    xml: &Document<'_>,
    size: &DrawingSheet,
    editable: &BTreeMap<&str, &str>,
    owner: &str,
    losses: &mut Vec<LossNote>,
) -> Result<TemplateGeometry, CodecError> {
    if xml.descendants().any(|node| {
        node.has_tag_name((SVG_NAMESPACE, "style"))
            || node.has_tag_name((SVG_NAMESPACE, "script"))
            || node.pi().is_some_and(|pi| pi.target == "xml-stylesheet")
    }) {
        return Err(CodecError::malformed(
            "SVG stylesheet or script prevents static geometry normalization",
        ));
    }
    let root = xml.root_element();
    let width = size.width_mm.get();
    let height = size.height_mm.get();
    let view_box = numbers(root.attribute("viewBox").unwrap_or(""));
    if root.tag_name().name() != "svg"
        || root.tag_name().namespace() != Some(SVG_NAMESPACE)
        || mm(root.attribute("width")) != Some(width)
        || mm(root.attribute("height")) != Some(height)
        || view_box.as_deref() != Some(&[0.0, 0.0, width, height])
        || unsupported(root, &["width", "height", "viewBox", "id", "version"])
    {
        return Err(CodecError::malformed("SVG needs a millimetre viewport and matching zero-origin viewBox without root presentation overrides"));
    }
    let mut output = TemplateGeometry::default();
    let mut pending = root
        .children()
        .rev()
        .filter(Node::is_element)
        .collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if node.tag_name().namespace() == Some(SVG_NAMESPACE)
            && node.tag_name().name() == "g"
            && !unsupported(node, &["id"])
        {
            pending.extend(node.children().rev().filter(Node::is_element));
        } else {
            element(node, height, editable, owner, losses, &mut output);
        }
    }
    if !output.is_valid() {
        return Err(CodecError::malformed("invalid template curve population"));
    }
    losses.push(FreecadLossCode::DrawingStyleNotTransferred.note(format!(
        "{owner}: SVG stroke, fill and font appearance remain in the asset; text em sizes are millimetres"
    )));
    Ok(output)
}

fn element(
    node: Node<'_, '_>,
    height: f64,
    editable: &BTreeMap<&str, &str>,
    owner: &str,
    losses: &mut Vec<LossNote>,
    output: &mut TemplateGeometry,
) {
    let tag = node.tag_name().name();
    let result = if node.tag_name().namespace() != Some(SVG_NAMESPACE)
        || (tag != "text" && node.children().any(|child| child.is_element()))
    {
        None
    } else {
        match tag {
            "rect"
                if !unsupported(
                    node,
                    &[
                        "id",
                        "x",
                        "y",
                        "width",
                        "height",
                        "fill",
                        "stroke",
                        "stroke-width",
                    ],
                ) =>
            {
                rectangle(node, height).map(|curves| output.curves.extend(curves))
            }
            "circle"
                if !unsupported(
                    node,
                    &["id", "cx", "cy", "r", "fill", "stroke", "stroke-width"],
                ) =>
            {
                circle(node, height).map(|curve| output.curves.push(curve))
            }
            "line"
                if !unsupported(
                    node,
                    &[
                        "id",
                        "x1",
                        "y1",
                        "x2",
                        "y2",
                        "fill",
                        "stroke",
                        "stroke-width",
                    ],
                ) =>
            {
                line(node, height).map(|curve| output.curves.push(curve))
            }
            "polyline" | "polygon"
                if !unsupported(node, &["id", "points", "fill", "stroke", "stroke-width"]) =>
            {
                polyline(node, height, tag == "polygon").map(|curves| output.curves.extend(curves))
            }
            "path" if !unsupported(node, &["id", "d", "fill", "stroke", "stroke-width"]) => {
                path(node.attribute("d").unwrap_or(""), height)
                    .map(|curves| output.curves.extend(curves))
            }
            "text"
                if !unsupported(
                    node,
                    &[
                        "id",
                        "x",
                        "y",
                        "font-size",
                        "editable",
                        "fill",
                        "stroke",
                        "stroke-width",
                    ],
                ) =>
            {
                text(node, height, editable).map(|text| output.texts.push(text))
            }
            _ => None,
        }
    };
    if result.is_none() {
        losses.push(FreecadLossCode::TemplateNotTransferred.note(format!(
            "{owner}: unsupported or invalid SVG {tag} subtree; original asset is retained"
        )));
    }
}

fn unsupported(node: Node<'_, '_>, allowed: &[&str]) -> bool {
    node.attributes().any(|attribute| {
        if let Some(namespace) = attribute.namespace() {
            namespace != EDITABLE_NAMESPACE
                || attribute.name() != "editable"
                || node.tag_name().name() != "text"
        } else {
            !allowed.contains(&attribute.name()) || attribute.name() == "editable"
        }
    })
}

fn mm(text: Option<&str>) -> Option<f64> {
    text?
        .strip_suffix("mm")?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn number(node: Node<'_, '_>, name: &str, default: Option<f64>) -> Option<f64> {
    match node.attribute(name) {
        Some(text) => text.parse::<f64>().ok().filter(|value| value.is_finite()),
        None => default,
    }
}

fn point(x: f64, y: f64, height: f64) -> Option<FiniteVector<2>> {
    FiniteVector::new([x, height - y])
}

fn rectangle(node: Node<'_, '_>, height: f64) -> Option<Vec<DrawingCurveGeometry>> {
    let x = number(node, "x", Some(0.0))?;
    let y = number(node, "y", Some(0.0))?;
    let w = number(node, "width", None)?;
    let h = number(node, "height", None)?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    segments(
        vec![
            point(x, y, height)?,
            point(x + w, y, height)?,
            point(x + w, y + h, height)?,
            point(x, y + h, height)?,
        ],
        true,
    )
}

fn circle(node: Node<'_, '_>, height: f64) -> Option<DrawingCurveGeometry> {
    Some(DrawingCurveGeometry::Circle {
        center: point(
            number(node, "cx", Some(0.0))?,
            number(node, "cy", Some(0.0))?,
            height,
        )?,
        radius: PositiveReal::new(number(node, "r", None)?)?,
    })
}

fn line(node: Node<'_, '_>, height: f64) -> Option<DrawingCurveGeometry> {
    Some(DrawingCurveGeometry::Line {
        start: point(
            number(node, "x1", Some(0.0))?,
            number(node, "y1", Some(0.0))?,
            height,
        )?,
        end: point(
            number(node, "x2", Some(0.0))?,
            number(node, "y2", Some(0.0))?,
            height,
        )?,
    })
}

fn numbers(text: &str) -> Option<Vec<f64>> {
    text.split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<f64>().ok().filter(|value| value.is_finite()))
        .collect()
}

fn polyline(node: Node<'_, '_>, height: f64, closed: bool) -> Option<Vec<DrawingCurveGeometry>> {
    let values = numbers(node.attribute("points")?)?;
    if values.len() < 4 || values.len() % 2 != 0 {
        return None;
    }
    let points = values
        .chunks_exact(2)
        .map(|pair| point(pair[0], pair[1], height))
        .collect::<Option<Vec<_>>>()?;
    segments(points, closed)
}

fn text(node: Node<'_, '_>, height: f64, editable: &BTreeMap<&str, &str>) -> Option<TemplateText> {
    if node.children().filter(Node::is_element).any(|child| {
        child.tag_name().name() != "tspan"
            || child.tag_name().namespace() != Some(SVG_NAMESPACE)
            || unsupported(child, &["id"])
            || child.children().any(|child| child.is_element())
    }) {
        return None;
    }
    let text = if let Some(key) = node.attribute((EDITABLE_NAMESPACE, "editable")) {
        (*editable.get(key)?).to_owned()
    } else {
        node.descendants()
            .filter(Node::is_text)
            .filter_map(|child| child.text())
            .collect()
    };
    Some(TemplateText {
        text,
        position: point(number(node, "x", None)?, number(node, "y", None)?, height)?,
        height: PositiveLength::new(number(node, "font-size", None)?)?,
    })
}

fn path(text: &str, height: f64) -> Option<Vec<DrawingCurveGeometry>> {
    let mut expanded = String::new();
    for character in text.chars() {
        if character.is_ascii_alphabetic() {
            expanded.push(' ');
            expanded.push(character);
            expanded.push(' ');
        } else {
            expanded.push(character);
        }
    }
    let tokens = expanded
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0;
    let mut command = "";
    let mut current = [0.0, 0.0];
    let mut start: Option<[f64; 2]> = None;
    let mut curves = Vec::new();
    while let Some(token) = tokens.get(index) {
        if token
            .chars()
            .all(|character| character.is_ascii_alphabetic())
        {
            command = token;
            index += 1;
            if index == tokens.len() && !matches!(command, "Z" | "z") {
                return None;
            }
            if matches!(command, "Z" | "z") {
                let end = start?;
                curves.push(DrawingCurveGeometry::Line {
                    start: point(current[0], current[1], height)?,
                    end: point(end[0], end[1], height)?,
                });
                current = end;
                command = "";
                continue;
            }
        }
        let mut next_number = || {
            let value = tokens
                .get(index)?
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())?;
            index += 1;
            Some(value)
        };
        if matches!(command, "A" | "a") {
            let radius = next_number()?;
            if radius <= 0.0 || radius != next_number()? {
                return None;
            }
            let rotation = next_number()?;
            if !rotation.is_finite() {
                return None;
            }
            let large = next_number()?;
            let sweep = next_number()?;
            if ![0.0, 1.0].contains(&large) || ![0.0, 1.0].contains(&sweep) {
                return None;
            }
            let end = if command == "a" {
                [current[0] + next_number()?, current[1] + next_number()?]
            } else {
                [next_number()?, next_number()?]
            };
            start?;
            curves.push(circular_arc(
                current,
                end,
                radius,
                large == 1.0,
                sweep == 1.0,
                height,
            )?);
            current = end;
            continue;
        }
        let next = match command {
            "M" | "L" => [next_number()?, next_number()?],
            "m" | "l" => [current[0] + next_number()?, current[1] + next_number()?],
            "H" => [next_number()?, current[1]],
            "h" => [current[0] + next_number()?, current[1]],
            "V" => [current[0], next_number()?],
            "v" => [current[0], current[1] + next_number()?],
            _ => return None,
        };
        if matches!(command, "M" | "m") {
            start = Some(next);
            command = if command == "M" { "L" } else { "l" };
        } else {
            start?;
            curves.push(DrawingCurveGeometry::Line {
                start: point(current[0], current[1], height)?,
                end: point(next[0], next[1], height)?,
            });
        }
        current = next;
    }
    if curves.is_empty() {
        None
    } else {
        Some(curves)
    }
}

fn circular_arc(
    start: [f64; 2],
    end: [f64; 2],
    radius: f64,
    large: bool,
    sweep: bool,
    height: f64,
) -> Option<DrawingCurveGeometry> {
    let half = [(start[0] - end[0]) / 2.0, (start[1] - end[1]) / 2.0];
    let square = half[0] * half[0] + half[1] * half[1];
    if square == 0.0 || square > radius * radius {
        return None;
    }
    let factor =
        ((radius * radius - square) / square).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let center = [
        f64::midpoint(start[0], end[0]) + factor * half[1],
        f64::midpoint(start[1], end[1]) - factor * half[0],
    ];
    let first = (center[1] - start[1])
        .atan2(start[0] - center[0])
        .to_degrees();
    let last = (center[1] - end[1]).atan2(end[0] - center[0]).to_degrees();
    let angle = if sweep {
        -((first - last).rem_euclid(360.0))
    } else {
        (last - first).rem_euclid(360.0)
    };
    if angle == 0.0 {
        return None;
    }
    Some(DrawingCurveGeometry::Arc {
        center: point(center[0], center[1], height)?,
        radius: PositiveReal::new(radius)?,
        start: point(start[0], start[1], height)?,
        middle: FiniteVector::new([
            center[0] + radius * (first + angle / 2.0).to_radians().cos(),
            height - center[1] + radius * (first + angle / 2.0).to_radians().sin(),
        ])?,
        end: point(end[0], end[1], height)?,
    })
}

fn segments(mut points: Vec<FiniteVector<2>>, closed: bool) -> Option<Vec<DrawingCurveGeometry>> {
    if points.len() < 2 {
        return None;
    }
    if closed {
        points.push(*points.first()?);
    }
    Some(
        points
            .windows(2)
            .map(|pair| DrawingCurveGeometry::Line {
                start: pair[0],
                end: pair[1],
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests;
