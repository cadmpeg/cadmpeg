// SPDX-License-Identifier: Apache-2.0
//! Independent Appendix C XML framing for the two FreeCAD document members.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View, XmlElementScan, XmlEncoding};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

use super::{FrameParent, FramedPart};

mod dispatch;

/// Reached XML nodes, including damaged boundaries and unassigned source faults.
/// This inventory does not assert that any element decoded successfully.
#[derive(Debug)]
pub struct XmlFraming<'a> {
    scan: XmlElementScan<'a>,
    frames: Vec<Option<usize>>,
}
impl<'a> XmlFraming<'a> {
    /// Original lexical observations, including cut-short elements and faults.
    pub fn scan(&self) -> &XmlElementScan<'a> {
        &self.scan
    }
    /// Appended part index for a reached XML element, when its locator applies.
    pub fn frame_for_element(&self, element: usize) -> Option<usize> {
        self.frames.get(element).copied().flatten()
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    frame: Option<usize>,
    level: Option<&'static str>,
    document: Option<usize>,
    value: Option<usize>,
    gui_state: Option<usize>,
}

/// Frame a genuine Document.xml or GuiDocument.xml ZIP member.
///
/// `zip_parent` must identify the source entry frame. Both its stored name and
/// the opened member view remain source-addressed. This function adds no
/// semantic reads and does not drain record captures. Non-XML character-content
/// formats (string tables, element maps, BREP and list side entries) need their
/// own Appendix C framers; they are not invented from XML child counts.
pub fn frame_xml_member<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    entry_name: View<'a>,
    zip_parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<XmlFraming<'a>, String> {
    let gui = match entry_name.window() {
        b"Document.xml" => false,
        b"GuiDocument.xml" => true,
        _ => return Err("FreeCAD XML member name is not an exact root member".into()),
    };
    let wrapper_level = if gui {
        "freecad.gui_document_xml"
    } else {
        "freecad.document_xml"
    };
    let wrapper = FramedPart::new(
        ctx,
        level(wrapper_level)?,
        source,
        FrameParent::Part(zip_parent),
        vec![],
        vec![],
    )?
    .with_external_dispatch(ctx, vec![entry_name])?;
    let wrapper_index = out.len();
    append(ctx, out, wrapper)?;
    let scan = ctx
        .scan_xml_elements(source, XmlEncoding::Utf8, "FreeCAD XML framing")
        .map_err(|error| error.to_string())?;
    let count = scan.elements().len();
    let bytes = count
        .checked_mul(size_of::<State>() + size_of::<Option<usize>>())
        .ok_or("FreeCAD XML frame inventory overflows")?;
    ctx.retain_bytes(u64_from_index(bytes), "FreeCAD XML frame inventory")
        .map_err(|error| error.to_string())?;
    let mut states = ctx
        .alloc_filled(count, State::default(), "FreeCAD XML frame states")
        .map_err(|error| error.to_string())?;
    let mut frames = ctx
        .alloc_filled(count, None, "FreeCAD XML frame mapping")
        .map_err(|error| error.to_string())?;
    for (index, element) in scan.elements().iter().copied().enumerate() {
        let parent = element
            .parent()
            .and_then(|parent| states.get(parent))
            .copied();
        let raw_name = element.name().window();
        let name = raw_name
            .rsplit(|byte| *byte == b':')
            .next()
            .unwrap_or(raw_name);
        let Some(row) = classify(gui, name, parent.and_then(|state| state.level)) else {
            continue;
        };
        // Unknown intervening elements never promote a nested tag to a sibling.
        if element.parent().is_some() && parent.and_then(|state| state.frame).is_none() {
            continue;
        }
        let parent_state = parent.unwrap_or_default();
        let physical_parent = parent_state.frame.unwrap_or(wrapper_index);
        let owner = match row {
            "freecad.properties" => parent_state.document,
            "freecad.value_element" | "freecad.gui_value_element" => parent_state.value,
            "freecad.gui_state_element" => parent_state.gui_state,
            _ => Some(physical_parent),
        }
        .ok_or("FreeCAD XML node has no declared source owner")?;
        let framing = element.framing().map(range).collect();
        let dispatch = dispatch::ranges(ctx, element, row)?;
        let mut frame = FramedPart::new(
            ctx,
            level(row)?,
            element.extent(),
            FrameParent::Part(physical_parent),
            framing,
            dispatch,
        )?;
        if element.parent().is_some() {
            frame = frame.with_xml_parent(ctx, &scan, index, owner)?;
        }
        if matches!(row, "freecad.property_value" | "freecad.gui_property_value") {
            let parent_node = element
                .parent()
                .and_then(|parent| scan.elements().get(parent))
                .ok_or("value lacks source property")?;
            let external = dispatch::attributes(ctx, *parent_node, &[b"type"])?;
            frame = frame.with_external_dispatch(ctx, external)?;
        }
        let frame_index = out.len();
        append(ctx, out, frame)?;
        frames[index] = Some(frame_index);
        let mut state = parent_state;
        state.frame = Some(frame_index);
        state.level = Some(row);
        if matches!(row, "freecad.document" | "freecad.gui_document") {
            state.document = Some(frame_index);
        }
        if matches!(row, "freecad.property_value" | "freecad.gui_property_value") {
            state.value = Some(frame_index);
        }
        if row == "freecad.gui_state" {
            state.gui_state = Some(frame_index);
        }
        states[index] = state;
    }
    Ok(XmlFraming { scan, frames })
}

fn classify(gui: bool, name: &[u8], parent: Option<&str>) -> Option<&'static str> {
    let row = if gui {
        match (parent, name) {
            (None, b"Document") => "freecad.gui_document",
            (Some("freecad.gui_document"), b"ViewProviderData") => "freecad.view_provider_data",
            (Some("freecad.gui_document"), _) => "freecad.gui_state",
            (Some("freecad.gui_state" | "freecad.gui_state_element"), _) => {
                "freecad.gui_state_element"
            }
            (Some("freecad.view_provider_data"), b"ViewProvider") => "freecad.view_provider",
            (Some("freecad.view_provider"), b"Extensions") => "freecad.gui_extensions",
            (Some("freecad.gui_extensions"), b"Extension") => "freecad.gui_extension",
            (Some("freecad.view_provider"), b"Properties") => "freecad.gui_properties",
            (Some("freecad.gui_properties"), b"Property") => "freecad.gui_property",
            (Some("freecad.gui_property"), _) => "freecad.gui_property_value",
            (Some("freecad.gui_property_value" | "freecad.gui_value_element"), _) => {
                "freecad.gui_value_element"
            }
            _ => return None,
        }
    } else {
        match (parent, name) {
            (None, b"Document") => "freecad.document",
            (Some("freecad.document"), b"StringHasher" | b"StringHasher2") => {
                "freecad.string_hasher"
            }
            (Some("freecad.document"), b"Objects" | b"Features") => "freecad.objects",
            (Some("freecad.objects"), b"ObjectDeps") => "freecad.object_deps",
            (Some("freecad.object_deps"), b"Dep") => "freecad.object_dep",
            (Some("freecad.objects"), b"Object" | b"Feature") => "freecad.object_declaration",
            (Some("freecad.document"), b"ObjectData" | b"FeatureData") => {
                "freecad.object_data_section"
            }
            (Some("freecad.object_data_section"), b"Object" | b"Feature") => "freecad.object_data",
            (Some("freecad.object_data"), b"Extensions") => "freecad.extensions",
            (Some("freecad.extensions"), b"Extension") => "freecad.extension",
            (
                Some("freecad.document" | "freecad.object_data" | "freecad.extension"),
                b"Properties",
            ) => "freecad.properties",
            (Some("freecad.properties"), b"Property" | b"_Property") => "freecad.property",
            (Some("freecad.property"), _) => "freecad.property_value",
            (Some("freecad.property_value" | "freecad.value_element"), _) => {
                "freecad.value_element"
            }
            _ => return None,
        }
    };
    Some(row)
}

fn level(name: &str) -> Result<PartLevel, String> {
    PartLevel::parse(name).ok_or_else(|| format!("unregistered FreeCAD Appendix C level {name}"))
}
fn range(view: View<'_>) -> Range<u64> {
    u64_from_index(view.start())..u64_from_index(view.end())
}
fn append<'a>(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<FramedPart<'a>>,
    frame: FramedPart<'a>,
) -> Result<(), String> {
    ctx.retain_bytes(
        u64_from_index(size_of::<FramedPart<'a>>()),
        "FreeCAD XML part frame",
    )
    .map_err(|error| error.to_string())?;
    out.try_reserve(1).map_err(|error| error.to_string())?;
    out.push(frame);
    Ok(())
}

#[cfg(test)]
mod tests;
