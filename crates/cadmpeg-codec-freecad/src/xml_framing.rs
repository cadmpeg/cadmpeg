// SPDX-License-Identifier: Apache-2.0
//! Navigation fields from the already parsed FreeCAD XML source tree.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

mod markup;

const STORAGE: &str = "FreeCAD XML framing storage";

/// Original XML syntax fields and plain inter-element whitespace.
#[derive(Debug)]
pub(crate) struct XmlFraming<'a> {
    framing: Vec<View<'a>>,
    insignificant: Vec<View<'a>>,
    supported: bool,
}

impl<'a> XmlFraming<'a> {
    pub(crate) fn framing(&self) -> &[View<'a>] {
        &self.framing
    }

    pub(crate) fn insignificant(&self) -> &[View<'a>] {
        &self.insignificant
    }

    /// Whether every element has a hierarchy-qualified Appendix C XML row.
    pub(crate) fn supported(&self) -> bool {
        self.supported
    }
}

#[derive(Clone, Copy)]
enum Role {
    Document,
    Objects,
    Deps,
    Dep,
    Declaration,
    Data,
    Object,
    Extensions,
    Extension,
    Properties,
    Property,
    Value,
    ValueElement,
    Hasher,
    Providers,
    Provider,
    State,
    StateElement,
}

impl Role {
    fn structural(self) -> bool {
        matches!(
            self,
            Self::Document
                | Self::Objects
                | Self::Deps
                | Self::Data
                | Self::Object
                | Self::Extensions
                | Self::Extension
                | Self::Properties
                | Self::Property
                | Self::Providers
                | Self::Provider
        )
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    role: Option<Role>,
    element_children: bool,
}

fn role(gui: bool, name: &str, parent: Option<Role>) -> Option<Role> {
    use Role::{
        Data, Declaration, Dep, Deps, Document, Extension, Extensions, Hasher, Object, Objects,
        Properties, Property, Provider, Providers, State, StateElement, Value, ValueElement,
    };
    if gui {
        Some(match (parent, name) {
            (None, "Document") => Document,
            (Some(Document), "ViewProviderData") => Providers,
            (Some(Document), _) => State,
            (Some(State | StateElement), _) => StateElement,
            (Some(Providers), "ViewProvider") => Provider,
            (Some(Provider), "Extensions") => Extensions,
            (Some(Extensions), "Extension") => Extension,
            (Some(Provider), "Properties") => Properties,
            (Some(Properties), "Property") => Property,
            (Some(Property), _) => Value,
            (Some(Value | ValueElement), _) => ValueElement,
            _ => return None,
        })
    } else {
        Some(match (parent, name) {
            (None, "Document") => Document,
            (Some(Document), "StringHasher" | "StringHasher2") => Hasher,
            (Some(Document), "Objects" | "Features") => Objects,
            (Some(Objects), "ObjectDeps") => Deps,
            (Some(Deps), "Dep") => Dep,
            (Some(Objects), "Object" | "Feature") => Declaration,
            (Some(Document), "ObjectData" | "FeatureData") => Data,
            (Some(Data), "Object" | "Feature") => Object,
            (Some(Object), "Extensions") => Extensions,
            (Some(Extensions), "Extension") => Extension,
            (Some(Document | Object | Extension), "Properties") => Properties,
            (Some(Properties), "Property" | "_Property") => Property,
            (Some(Property), _) => Value,
            (Some(Value | ValueElement), _) => ValueElement,
            _ => return None,
        })
    }
}

/// Uses the existing tree's node extents to navigate actual markup boundaries.
/// The tree must be the parse of `source`; no XML parser or semantic read is run.
/// Unknown hierarchy branches remain unclassified and make `supported` false.
pub(crate) fn frame<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    xml: &roxmltree::Document<'_>,
    gui: bool,
) -> Result<XmlFraming<'a>, CodecError> {
    ctx.capture_part_extent(source)?;
    if source.window().len() != xml.input_text().len()
        || source.window().as_ptr() != xml.input_text().as_ptr()
    {
        return Err(CodecError::malformed(
            "FreeCAD XML tree does not fit its original source",
        ));
    }
    let count = xml
        .descendants()
        .map(|node| node.id().get_usize())
        .max()
        .and_then(|last| last.checked_add(1))
        .ok_or_else(|| CodecError::malformed("FreeCAD XML node inventory overflows"))?;
    let capacity = count
        .checked_mul(3)
        .ok_or_else(|| CodecError::malformed("FreeCAD XML framing count overflows"))?;
    let bytes = count
        .checked_mul(size_of::<State>())
        .and_then(|states| {
            capacity
                .checked_add(count)?
                .checked_mul(size_of::<View<'_>>())?
                .checked_add(states)
        })
        .ok_or_else(|| CodecError::malformed("FreeCAD XML framing storage overflows"))?;
    ctx.retain_bytes(u64_from_index(bytes), STORAGE)?;
    let mut states = ctx.alloc_filled(count, State::default(), STORAGE)?;
    let mut framing = ctx.alloc_filled(capacity, source, STORAGE)?;
    let mut insignificant = ctx.alloc_filled(count, source, STORAGE)?;
    let mut supported = true;
    for node in xml.descendants().filter(roxmltree::Node::is_element) {
        let parent = node.parent().filter(|node| node.is_element());
        let parent_role = parent.and_then(|parent| states[parent.id().get_usize()].role);
        let admitted = if parent.is_some() && parent_role.is_none() {
            None
        } else {
            role(gui, node.tag_name().name(), parent_role)
        };
        supported &= admitted.is_some();
        states[node.id().get_usize()].role = admitted;
        if let Some(parent) = parent {
            states[parent.id().get_usize()].element_children = true;
        }
    }
    let mut frame_count = 0;
    let mut insignificant_count = 0;
    for node in xml.descendants() {
        if node.is_element() && states[node.id().get_usize()].role.is_some() {
            let extent = relative(source, node.range())?;
            for view in markup::fields(extent, node.tag_name().name())?
                .into_iter()
                .flatten()
            {
                framing[frame_count] = view;
                frame_count += 1;
            }
        } else if node.is_text() {
            let Some(parent) = node.parent().map(|parent| states[parent.id().get_usize()]) else {
                continue;
            };
            if parent.element_children && parent.role.is_some_and(Role::structural) {
                let raw = relative(source, node.range())?;
                if raw.window().iter().all(|byte| xml_space(*byte))
                    && node
                        .text()
                        .is_some_and(|text| text.as_bytes() == raw.window())
                {
                    insignificant[insignificant_count] = raw;
                    insignificant_count += 1;
                }
            }
        }
    }
    framing.truncate(frame_count);
    insignificant.truncate(insignificant_count);
    Ok(XmlFraming {
        framing,
        insignificant,
        supported,
    })
}

/// Navigation fields of one already parsed element in its original source.
pub(crate) fn element_fields<'source>(
    ctx: &DecodeContext<'_>,
    source: View<'source>,
    node: roxmltree::Node<'_, '_>,
) -> Result<(View<'source>, Vec<std::ops::Range<u64>>), CodecError> {
    ctx.capture_part_extent(source)?;
    let input = node.document().input_text().as_bytes();
    if input.len() != source.window().len() || input.as_ptr() != source.window().as_ptr() {
        return Err(CodecError::malformed(
            "XML element does not borrow its original source",
        ));
    }
    let extent = relative(source, node.range())?;
    let fields = markup::fields(extent, node.tag_name().name())?;
    let count = fields.iter().flatten().count();
    let bytes = count
        .checked_mul(size_of::<std::ops::Range<u64>>())
        .ok_or_else(|| CodecError::malformed("XML element framing storage overflows"))?;
    ctx.retain_bytes(u64_from_index(bytes), STORAGE)?;
    let mut ranges = ctx.alloc_filled(count, 0..0, STORAGE)?;
    for (range, field) in ranges.iter_mut().zip(fields.into_iter().flatten()) {
        *range = u64_from_index(field.start())..u64_from_index(field.end());
    }
    Ok((extent, ranges))
}

fn xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn relative(source: View<'_>, range: std::ops::Range<usize>) -> Result<View<'_>, CodecError> {
    let start = source
        .start()
        .checked_add(range.start)
        .ok_or_else(|| CodecError::malformed("FreeCAD XML source start overflows"))?;
    let end = source
        .start()
        .checked_add(range.end)
        .ok_or_else(|| CodecError::malformed("FreeCAD XML source end overflows"))?;
    source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("FreeCAD XML node escapes its source"))
}

#[cfg(test)]
mod tests;
