// SPDX-License-Identifier: Apache-2.0
//! `.pvs` structure members (§3): the header, the stream directory, and the
//! model the streams carry.

mod atom;
mod element;
mod properties;
mod structure;
mod xml;

use std::collections::BTreeMap;
use std::rc::Rc;

use cadmpeg_core::decode::{index_from_u32, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::layout::{pvs_header, pvs_stream_entry_head as stream_entry};
use crate::loss::PvzLossCode;
use crate::read::{at, record};
use atom::StringPool;
use element::Element;

/// Leading member bytes that carry the structure signature.
const SIGNATURE_LEN: usize = 10;
/// Leading bytes of a binary structure member.
const BINARY_PREFIX: &[u8] = b"PV-PVS";
/// Leading bytes of an XML structure member.
const XML_PREFIX: &[u8] = b"<PV_FILE";

/// Element families of the PVS streams.
mod family {
    /// String-pool stream root (§3.5).
    pub(super) const STRING_POOL: u8 = 0x01;
    /// Structure stream root (§3.6).
    pub(super) const STRUCTURE: u8 = 0x02;
    /// Property stream root (§3.13).
    pub(super) const PROPERTIES: u8 = 0x03;
    /// Component definition (§3.6).
    pub(super) const DEFINITION: u8 = 0x20;
    /// Child occurrence (§3.9).
    pub(super) const OCCURRENCE: u8 = 0x21;
    /// Geometry element (§3.8).
    pub(super) const GEOMETRY: u8 = 0x23;
    /// Property set (§3.13).
    pub(super) const PROPERTY_SET: u8 = 0x2a;
    /// Property (§3.13).
    pub(super) const PROPERTY: u8 = 0x2b;
}

/// Stream types of the directory (§3.3).
mod stream_type {
    /// String pool.
    pub(super) const STRING_POOL: u8 = 1;
    /// Structure.
    pub(super) const STRUCTURE: u8 = 2;
    /// Property sets.
    pub(super) const PROPERTIES: u8 = 3;
}

/// Structure-member generation: the four digits of the binary signature
/// (§3.1) or of the XML `version` attribute (§3.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generation {
    /// Digits `0301`.
    Pvs0301,
    /// Digits `0302`.
    Pvs0302,
    /// Digits `0303`.
    Pvs0303,
    /// Digits `0304`.
    Pvs0304,
    /// Digits `0305`.
    Pvs0305,
}

impl Generation {
    const ALL: [Self; 5] = [
        Self::Pvs0301,
        Self::Pvs0302,
        Self::Pvs0303,
        Self::Pvs0304,
        Self::Pvs0305,
    ];

    /// The ASCII signature at the start of a binary member.
    pub(crate) const fn signature(self) -> &'static str {
        match self {
            Self::Pvs0301 => "PV-PVS0301",
            Self::Pvs0302 => "PV-PVS0302",
            Self::Pvs0303 => "PV-PVS0303",
            Self::Pvs0304 => "PV-PVS0304",
            Self::Pvs0305 => "PV-PVS0305",
        }
    }

    /// The four digits of the generation.
    pub(crate) const fn digits(self) -> &'static str {
        match self {
            Self::Pvs0301 => "0301",
            Self::Pvs0302 => "0302",
            Self::Pvs0303 => "0303",
            Self::Pvs0304 => "0304",
            Self::Pvs0305 => "0305",
        }
    }
}

/// The form of a structure member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    /// The binary form (§3.1).
    Binary,
    /// The XML form (§3.14).
    Xml,
}

/// A known generation and the form of the member that declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) generation: Generation,
    pub(crate) form: Form,
}

impl Declared {
    /// The declaration as the member stores it: the binary signature, or the
    /// digits of the XML `version` attribute.
    pub(crate) fn text(self) -> &'static str {
        match self.form {
            Form::Binary => self.generation.signature(),
            Form::Xml => self.generation.digits(),
        }
    }
}

/// The known generation that `member` declares, if any: by its leading
/// binary signature, or by the `version` attribute of a leading
/// `<PV_FILE type="PVS">` start tag.
pub(crate) fn declared(member: View<'_>) -> Option<Declared> {
    let window = member.window();
    if let Some(leading) = window.get(..SIGNATURE_LEN) {
        if let Some(generation) = Generation::ALL
            .into_iter()
            .find(|generation| generation.signature().as_bytes() == leading)
        {
            return Some(Declared {
                generation,
                form: Form::Binary,
            });
        }
    }
    let version = xml::declared_version(window)?;
    Generation::ALL
        .into_iter()
        .find(|generation| generation.digits().as_bytes() == version)
        .map(|generation| Declared {
            generation,
            form: Form::Xml,
        })
}

/// The role a definition plays in the product tree (§3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionRole {
    /// A component definition.
    Component,
    /// The root of an illustration package (§3.11).
    IllustrationRoot,
}

/// One component definition (§3.6, §3.7). Text fields share string-pool
/// entries.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Definition {
    pub(crate) name: Rc<str>,
    pub(crate) role: DefinitionRole,
    /// The geometry member the definition's geometry element names.
    pub(crate) geometry: Option<Rc<str>>,
    /// Child occurrences, in stored order.
    pub(crate) occurrences: Vec<ChildOccurrence>,
    /// Properties from every property stream, in stream then stored order.
    pub(crate) properties: Vec<Property>,
}

/// One child occurrence (§3.9).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChildOccurrence {
    /// Definition index of the placed definition.
    pub(crate) definition: usize,
    pub(crate) placement: Placement,
    pub(crate) feature_id: Rc<str>,
}

/// A placement (§3.10), in stored metres.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Placement {
    /// `tx, ty, tz`; absent is the zero translation.
    pub(crate) translation: Option<[f64; 3]>,
    /// The child X, Y, and Z axes in parent coordinates; absent is the
    /// identity basis.
    pub(crate) basis: Option<[f64; 9]>,
}

/// One property (§3.13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Property {
    /// Name of the property stream; empty for the general stream.
    pub(crate) stream: Rc<str>,
    pub(crate) name: Rc<str>,
    pub(crate) value: Rc<str>,
}

/// The decoded model of one structure member.
#[derive(Debug, Default)]
pub(crate) struct Structure {
    /// Definitions in definition-index order.
    pub(crate) definitions: Vec<Definition>,
    /// Content the decoder read but did not transfer.
    pub(crate) residue: Residue,
}

/// Structure content that the model does not carry, counted for loss notes.
#[derive(Debug, Default)]
pub(crate) struct Residue {
    /// Elements of families or descriptors the model does not map, by family.
    elements: BTreeMap<u8, usize>,
    /// Typed definitions with bytes after the name atom (§3.7).
    definition_attributes: usize,
    /// Occurrences with placement flags or extension bytes (§3.9).
    occurrence_attributes: usize,
    /// Occurrences with an undefined placement encoding (§3.9).
    occurrences_undecoded: usize,
    /// Geometry elements with bounds (§3.8).
    bounds: usize,
    /// Typed properties with bytes after their value (§3.13).
    typed_properties: usize,
    /// XML elements outside the mapped subset (§3.14), by name.
    xml_elements: BTreeMap<String, usize>,
    /// Property streams whose grammar failed, with the reason.
    failed_property_streams: Vec<String>,
    /// Directory streams the decoder does not read, with the reason.
    skipped_streams: Vec<String>,
}

impl Residue {
    fn element(&mut self, element: &Element<'_>) {
        *self.elements.entry(element.family).or_default() += 1;
    }

    fn xml_element(&mut self, name: &str) {
        *self.xml_elements.entry(name.to_owned()).or_default() += 1;
    }

    fn absorb(&mut self, other: Self) {
        for (family, count) in other.elements {
            *self.elements.entry(family).or_default() += count;
        }
        for (name, count) in other.xml_elements {
            *self.xml_elements.entry(name).or_default() += count;
        }
        self.definition_attributes += other.definition_attributes;
        self.occurrence_attributes += other.occurrence_attributes;
        self.occurrences_undecoded += other.occurrences_undecoded;
        self.bounds += other.bounds;
        self.typed_properties += other.typed_properties;
        self.failed_property_streams
            .extend(other.failed_property_streams);
        self.skipped_streams.extend(other.skipped_streams);
    }

    /// One loss note per kind of content the model does not carry.
    pub(crate) fn losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        if !self.elements.is_empty() || !self.xml_elements.is_empty() {
            let families = self
                .elements
                .iter()
                .map(|(family, count)| format!("{family:#04x} x{count}"))
                .chain(
                    self.xml_elements
                        .iter()
                        .map(|(name, count)| format!("<{name}> x{count}")),
                )
                .collect::<Vec<_>>()
                .join(", ");
            losses.push(PvzLossCode::StructureElementUndecoded.note(format!(
                "structure elements outside the decoded model were not transferred: {families} \
                 (appearance palettes, illustrations, groups, and occurrence attributes; \
                 PVZ-06, PVZ-07, PVZ-08)"
            )));
        }
        if self.definition_attributes > 0 {
            losses.push(PvzLossCode::StructureDefinitionAttributes.note(format!(
                "{} typed definitions carry bytes after their name whose meaning is an open \
                 item (PVZ-04); the bytes were not transferred",
                self.definition_attributes
            )));
        }
        if self.occurrence_attributes > 0 {
            losses.push(PvzLossCode::StructureOccurrenceAttributes.note(format!(
                "{} occurrences carry placement flags or extension bytes whose meaning is an \
                 open item (PVZ-05); the placements were transferred without them",
                self.occurrence_attributes
            )));
        }
        if self.occurrences_undecoded > 0 {
            losses.push(PvzLossCode::StructureOccurrenceUndecoded.note(format!(
                "{} occurrences use an undefined placement encoding and were not placed",
                self.occurrences_undecoded
            )));
        }
        if self.bounds > 0 {
            losses.push(PvzLossCode::StructureBoundsNotTransferred.note(format!(
                "{} geometry elements carry bounds; the model has no definition bounds field, \
                 so the bounds were not transferred (PVZ-27)",
                self.bounds
            )));
        }
        if self.typed_properties > 0 {
            losses.push(PvzLossCode::StructurePropertyAttributes.note(format!(
                "{} typed properties carry three bytes after their value whose meaning is an \
                 open item (PVZ-09); the bytes were not transferred",
                self.typed_properties
            )));
        }
        for reason in &self.failed_property_streams {
            losses.push(PvzLossCode::StructurePropertyStreamMalformed.note(reason.clone()));
        }
        for reason in &self.skipped_streams {
            losses.push(PvzLossCode::StructureStreamUndecoded.note(reason.clone()));
        }
        losses
    }
}

/// One stream directory entry with its stored payload (§3.2).
struct Stream<'a> {
    kind: u8,
    codec: u8,
    name: String,
    payload: View<'a>,
}

/// Decodes a structure member in its binary (§3.1) or XML (§3.14) form.
pub(crate) fn read(ctx: &DecodeContext<'_>, member: View<'_>) -> Result<Structure, CodecError> {
    let window = member.window();
    if window.starts_with(BINARY_PREFIX) {
        read_binary(ctx, member)
    } else if window.starts_with(XML_PREFIX) {
        xml::read(ctx, window)
    } else {
        Err(CodecError::malformed(
            "structure member opens with neither PV-PVS nor <PV_FILE",
        ))
    }
}

fn read_binary(ctx: &DecodeContext<'_>, member: View<'_>) -> Result<Structure, CodecError> {
    let streams = directory(ctx, member)?;
    let mut residue = Residue::default();
    let mut pool = None;
    for stream in streams
        .iter()
        .filter(|s| s.kind == stream_type::STRING_POOL)
    {
        let root = Element::read_stream(ctx, expand(ctx, stream)?)?;
        if !root.is(
            family::STRING_POOL,
            crate::layout::token::DESC_U8_LENGTH,
            None,
        ) && !root.is(
            family::STRING_POOL,
            crate::layout::token::DESC_U16_LENGTH,
            None,
        ) {
            return Err(CodecError::malformed(format_args!(
                "string-pool root is family {:#04x} descriptor {:#04x}",
                root.family, root.descriptor
            )));
        }
        if pool.replace(StringPool::read(ctx, root.payload)?).is_some() {
            return Err(CodecError::malformed(
                "structure member has more than one string-pool stream",
            ));
        }
    }
    let pool = pool.unwrap_or_default();
    let mut definitions = None;
    for stream in streams.iter().filter(|s| s.kind == stream_type::STRUCTURE) {
        let root = Element::read_stream(ctx, expand(ctx, stream)?)?;
        let read = structure::read(&root, &pool, &mut residue)?;
        if definitions.replace(read).is_some() {
            return Err(CodecError::malformed(
                "structure member has more than one structure stream",
            ));
        }
    }
    let mut definitions = definitions
        .ok_or_else(|| CodecError::malformed("structure member has no structure stream"))?;
    for stream in &streams {
        match stream.kind {
            stream_type::STRING_POOL | stream_type::STRUCTURE => {}
            stream_type::PROPERTIES => {
                let applied = expand(ctx, stream)
                    .and_then(|expanded| Element::read_stream(ctx, expanded))
                    .and_then(|root| {
                        properties::apply(
                            &stream.name,
                            &root,
                            &pool,
                            &mut definitions,
                            &mut residue,
                        )
                    });
                match applied {
                    Ok(()) => {}
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                    Err(error) => residue.failed_property_streams.push(format!(
                        "property stream {:?} was not transferred: {error}",
                        stream.name
                    )),
                }
            }
            other => residue.skipped_streams.push(format!(
                "stream {:?} of undefined type {other} ({} stored bytes) was not decoded",
                stream.name,
                stream.payload.window().len()
            )),
        }
    }
    Ok(Structure {
        definitions,
        residue,
    })
}

/// Expands one stream payload. Codec 1 is the only defined encoding.
fn expand<'a>(ctx: &DecodeContext<'a>, stream: &Stream<'_>) -> Result<View<'a>, CodecError> {
    if stream.codec != stream_entry::CODEC_VALUE {
        return Err(CodecError::malformed(format_args!(
            "stream {:?} of type {} uses undefined codec {}",
            stream.name, stream.kind, stream.codec
        )));
    }
    crate::gzip::expand(ctx, stream.payload)
}

/// Reads the header and the stream directory, and partitions the member into
/// stream payloads (§3.1, §3.2).
fn directory<'a>(ctx: &DecodeContext<'_>, member: View<'a>) -> Result<Vec<Stream<'a>>, CodecError> {
    let mut view = member;
    let header = record(&mut view, pvs_header::LEN)?;
    let first_stream = at(header, pvs_header::FIRST_STREAM_OFFSET)?.req_u32_le()?;
    let declared = at(header, pvs_header::STREAM_COUNT)?.req_u32_le()?;
    let count = view
        .counted(u64::from(declared), stream_entry::LEN)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "structure member declares {declared} streams in {} bytes",
                view.remaining()
            ))
        })?;
    let mut entries = ctx.alloc_filled(
        count.get(),
        (0, 0, String::new(), 0),
        "PVZ structure stream directory",
    )?;
    for slot in &mut entries {
        let head = record(&mut view, stream_entry::LEN)?;
        let stream_type = at(head, stream_entry::STREAM_TYPE)?.req_u8()?;
        let codec = at(head, stream_entry::CODEC)?.req_u8()?;
        let name_len = at(head, stream_entry::NAME_LENGTH)?.req_u8()?;
        let name = String::from_utf8_lossy(view.req_take(usize::from(name_len))?).into_owned();
        let stored = view.req_u32_le()?;
        *slot = (stream_type, codec, name, index_from_u32(stored));
    }
    let first = index_from_u32(first_stream);
    if view.read_len() != first {
        return Err(CodecError::malformed(format_args!(
            "stream directory ends at offset {}, not at the first stream offset {first}",
            view.read_len()
        )));
    }
    let mut streams = Vec::with_capacity(entries.len());
    for (stream_type, codec, name, stored) in entries {
        let payload = view.req_take_child(stored)?;
        streams.push(Stream {
            kind: stream_type,
            codec,
            name,
            payload,
        });
    }
    if !view.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "structure member leaves {} bytes after its last stream",
            view.remaining()
        )));
    }
    Ok(streams)
}

#[cfg(test)]
mod tests;
