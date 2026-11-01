// SPDX-License-Identifier: Apache-2.0
//! Object display values projected from neutral bindings.

use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::{CadIr, SourceObjectAssociation};

#[derive(Clone, Copy)]
pub(super) struct Display<'a> {
    pub(super) name: Option<&'a str>,
    pub(super) color: Option<Color>,
    pub(super) visible: Option<bool>,
}

pub(super) fn display<'a>(
    ir: &'a CadIr,
    target: &AppearanceTarget,
    _source: Option<&SourceObjectAssociation>,
) -> Display<'a> {
    Display {
        name: object_name(ir, target),
        color: ir.effective_display_color(target),
        visible: ir.effective_visibility(target),
    }
}

pub(super) fn object_name<'a>(ir: &'a CadIr, target: &AppearanceTarget) -> Option<&'a str> {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    let typed_target = target.typed_record_target(&ir.model);
    let target = typed_target.as_ref().unwrap_or(target);
    let attribute_target = match target {
        AppearanceTarget::Body(id) => {
            return ir
                .model
                .bodies
                .iter()
                .find(|row| row.id == *id)
                .and_then(|row| row.name.as_deref());
        }
        AppearanceTarget::Face(id) => {
            return ir
                .model
                .faces
                .iter()
                .find(|row| row.id == *id)
                .and_then(|row| row.name.as_deref());
        }
        AppearanceTarget::Occurrence(id) => {
            return ir
                .model
                .occurrences
                .iter()
                .find(|row| row.id == *id)
                .and_then(|row| row.name.as_deref());
        }
        AppearanceTarget::Curve(id) => AttributeTarget::Curve(id.clone()),
        AppearanceTarget::Surface(id) => AttributeTarget::Surface(id.clone()),
        AppearanceTarget::Point(id) => AttributeTarget::Point(id.clone()),
        AppearanceTarget::Edge(id) => AttributeTarget::Edge(id.clone()),
        AppearanceTarget::Vertex(id) => AttributeTarget::Vertex(id.clone()),
        AppearanceTarget::Tessellation(id) => AttributeTarget::Tessellation(id.clone()),
        AppearanceTarget::TriangleGroup { tessellation, .. } => {
            AttributeTarget::Tessellation(tessellation.clone())
        }
        AppearanceTarget::Record { arena, id } => AttributeTarget::Record {
            arena: *arena,
            id: id.clone(),
        },
        AppearanceTarget::Source { source_id } => AttributeTarget::Source {
            source_id: source_id.clone(),
        },
    };
    ir.model.attributes.iter().find_map(|attribute| {
        if attribute.target != attribute_target || attribute.name != "name" {
            return None;
        }
        match attribute.values.as_slice() {
            [Some(AttributeValue::String(name))] => Some(name.as_str()),
            _ => None,
        }
    })
}

pub(super) fn apply(
    ir: &CadIr,
    target: &AppearanceTarget,
    color: &mut Option<Color>,
    visible: &mut Option<bool>,
) {
    *color = ir.effective_display_color(target).or(*color);
    *visible = match (*visible, ir.effective_visibility(target)) {
        (Some(first), Some(second)) => Some(first && second),
        (first, second) => first.or(second),
    };
}

pub(super) fn color_quantized(color: Color) -> bool {
    [color.r(), color.g(), color.b(), color.a()]
        .into_iter()
        .any(|channel| (channel * 255.0).round() / 255.0 != channel)
}
