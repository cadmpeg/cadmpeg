//! SWIFT presentation archive: geometric-tolerance displays, their feature
//! paths, and the semantic annotations they depict.

use std::collections::{BTreeSet, HashMap};
use std::num::NonZeroU32;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{FaceId, PmiId};
use cadmpeg_ir::pmi::{
    DatumReference, DatumReferences, GeometricToleranceKind, PmiAnnotation, PmiDefinition,
    PmiTarget,
};

use cadmpeg_ir::report::loss::LossNote;

use crate::brep::PersistentFaceIdentity;
use crate::loss::SldprtLossCode;

use super::{decode_rendered_utf16, short_class, suppressed, tolerance_kind, Entity};
use crate::layout::named_object_header as named_header;
use crate::layout::swift_feature_reference as feature_reference;
use crate::layout::swift_gtol_display_head as gtol_head;

const HEAD_CLASSES: [&[u8]; 3] = [b"moSwiftSchema_c", b"moNodeName_c", b"moDetailFolder_c"];
const GTOL_CLASS: &[u8] = b"moDisplayGtol_c";
const DATUM_TAG_CLASS: &[u8] = b"moDisplayDatumTag_c";
const FEATURE_REFERENCE_CLASS: &[u8] = b"moSwiftFR_c";
const CLASS_DECLARATION: u16 = 0xffff;
const OBJECT_TAG: u16 = 0x8000;
const NAME_TAG: u16 = 0x8003;
const NO_OBJECT_ID: u32 = u32::MAX;
const MAX_PATH_FEATURES: u16 = 8;
const STRING_MARKER: [u8; 3] = [0xff, 0xfe, 0xff];
const WIDE_STRING_COUNT: u8 = 0xff;
const FRAME_TEMPLATE_TOKEN: &str = "<Gtol>";
const MODIFIER_PREFIX: &str = "<MOD-";
const COMPOSITE_LOWER_TIER_SUFFIX: &str = ":lower-tier";
const EPS_TIER_RATIO_E9: f64 = 1.0e-9;
const EPS_INTERNAL_FLATNESS_UNIT_RELATIVE: f64 = 1.0e-9;

mod faces;
use faces::{DocumentLayout, FaceIdentity, IdentityReference};

/// The geometric-tolerance displays of one presentation archive.
#[derive(Debug, Default)]
pub(super) struct Archive {
    names: HashMap<u32, Option<String>>,
    gtols: Vec<GtolDisplay>,
    unbounded: Vec<usize>,
    unframed: bool,
}

impl Archive {
    /// An archive that declares geometric-tolerance displays whose list the
    /// head, count, or item openings do not frame.
    fn unframed() -> Self {
        Self {
            unframed: true,
            ..Self::default()
        }
    }

    /// Whether the archive declares displays that its framing discards: a
    /// list the head, count, or item openings do not frame, or a last display
    /// that no following non-empty list bounds.
    pub(super) fn is_unframed(&self) -> bool {
        self.unframed || !self.unbounded.is_empty()
    }
}

#[derive(Debug)]
struct GtolDisplay {
    offset: usize,
    name: Option<String>,
    characteristics: Vec<String>,
    path: FeaturePath,
    faces: FaceAttachment,
    document: DocumentLayout,
    rows: Vec<Row>,
    note: Option<String>,
}

/// The faces a display without a feature path attaches to.
#[derive(Debug, PartialEq)]
enum FaceAttachment {
    Absent,
    Unresolved,
    Faces(Vec<FaceIdentity>),
}

/// One feature-control-frame row: its characteristic token, tolerance
/// literal, and compartment labels with whether a modifier follows each.
#[derive(Debug, PartialEq)]
struct Row {
    characteristic: String,
    tolerance: Option<String>,
    labels: Vec<(String, bool)>,
}

#[derive(Debug, PartialEq)]
enum FeaturePath {
    Absent,
    Inconsistent,
    Features(Vec<u32>),
}

struct NamedObject {
    offset: usize,
    name: String,
}

struct Text {
    offset: usize,
    end: usize,
    value: String,
}

struct Path {
    offset: usize,
    features: Vec<u32>,
}

/// Parse the presentation archive that precedes the GDT-analysis part object.
pub(super) fn parse(archive: &[u8]) -> Archive {
    let Some(gtol) = declaration(archive, GTOL_CLASS, 0) else {
        return Archive::default();
    };
    if !head_declares_only(archive, gtol) {
        return Archive::unframed();
    }
    let Some(count) = gtol
        .checked_sub(2)
        .and_then(|at| View::u16_le_at(archive, at))
    else {
        return Archive::unframed();
    };
    let mut starts = vec![gtol];
    starts.extend(gtol_tags(archive, gtol));
    if starts.len() != usize::from(count) {
        return Archive::unframed();
    }
    let (named, names) = named_objects(archive);
    let paths = feature_paths(archive, &names);
    let texts = texts(archive);
    let references = faces::identity_references(archive);
    let mut ends: Vec<Option<usize>> = starts.iter().skip(1).copied().map(Some).collect();
    let last = starts.last().copied().unwrap_or(gtol);
    ends.push(declaration(archive, DATUM_TAG_CLASS, last).and_then(|start| start.checked_sub(2)));
    let mut parsed = Archive {
        names,
        ..Archive::default()
    };
    for (&start, end) in starts.iter().zip(ends) {
        match end.filter(|end| *end > start) {
            Some(end) => parsed.gtols.push(gtol_display(
                start..end,
                &named,
                &paths,
                &texts,
                &references,
            )),
            None => parsed.unbounded.push(start),
        }
    }
    parsed
}

fn gtol_display(
    span: std::ops::Range<usize>,
    named: &[NamedObject],
    paths: &[Path],
    texts: &[Text],
    references: &[IdentityReference],
) -> GtolDisplay {
    let inside = |offset: usize| span.contains(&offset);
    let name = named
        .iter()
        .find(|object| inside(object.offset))
        .map(|object| object.name.clone());
    let texts = texts
        .iter()
        .filter(|text| inside(text.offset))
        .collect::<Vec<_>>();
    let characteristics = texts
        .iter()
        .filter_map(|text| characteristic(&text.value))
        .map(str::to_owned)
        .collect();
    let (faces, document) = face_attachment(
        references
            .iter()
            .filter(|reference| inside(reference.offset)),
    );
    GtolDisplay {
        offset: span.start,
        name,
        characteristics,
        path: display_path(paths.iter().filter(|path| inside(path.offset))),
        faces,
        document,
        rows: frame_rows(&texts),
        note: frame_note(&texts),
    }
}

/// The faces a display's references select, and how their first hops read
/// documents written in place.
fn face_attachment<'a>(
    references: impl Iterator<Item = &'a IdentityReference>,
) -> (FaceAttachment, DocumentLayout) {
    let mut faces = Vec::new();
    let mut document = DocumentLayout::default();
    for reference in references {
        let Some(face) = &reference.face else {
            return (FaceAttachment::Unresolved, DocumentLayout::default());
        };
        document = document.merge(reference.document);
        if !faces.contains(face) {
            faces.push(face.clone());
        }
    }
    if faces.is_empty() {
        (FaceAttachment::Absent, DocumentLayout::default())
    } else {
        (FaceAttachment::Faces(faces), document)
    }
}

/// The frame rows before the four-string block: each characteristic token,
/// the tolerance literal after it, and the labels up to the next row.
fn frame_rows(texts: &[&Text]) -> Vec<Row> {
    let Some(block) = frame_block(texts) else {
        return Vec::new();
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut texts = texts.iter().take(block).map(|text| text.value.as_str());
    while let Some(text) = texts.next() {
        if characteristic(text).is_some() {
            rows.push(Row {
                characteristic: text.to_owned(),
                tolerance: texts.next().map(str::to_owned),
                labels: Vec::new(),
            });
            continue;
        }
        let Some(row) = rows.last_mut() else {
            continue;
        };
        if text.starts_with(MODIFIER_PREFIX) {
            if let Some(label) = row.labels.last_mut() {
                label.1 = true;
            }
        } else if !text.is_empty() {
            row.labels.push((text.to_owned(), false));
        }
    }
    rows
}

/// The characteristic token of one frame row, such as `POSI` or `IGTOL-FLAT`.
fn characteristic(text: &str) -> Option<&str> {
    let token = text.strip_prefix('<')?.strip_suffix('>')?;
    let symbol = token
        .strip_prefix("GTOL-")
        .or_else(|| token.strip_prefix("IGTOL-"))?;
    (!symbol.is_empty() && symbol.bytes().all(|byte| byte.is_ascii_uppercase())).then_some(
        if token.starts_with('I') {
            token
        } else {
            symbol
        },
    )
}

/// The index of the first string of the unique four-string block whose
/// third string is the frame template.
fn frame_block(texts: &[&Text]) -> Option<usize> {
    let mut blocks = texts
        .windows(4)
        .enumerate()
        .filter_map(|(index, block)| match block {
            [first, second, template, note]
                if first.end == second.offset
                    && second.end == template.offset
                    && template.end == note.offset
                    && template.value.contains(FRAME_TEMPLATE_TOKEN) =>
            {
                Some(index)
            }
            _ => None,
        });
    let block = blocks.next()?;
    blocks.next().is_none().then_some(block)
}

/// The note of the four-string block: its fourth string.
fn frame_note(texts: &[&Text]) -> Option<String> {
    let note = texts.get(frame_block(texts)?.checked_add(3)?)?;
    Some(note.value.clone()).filter(|note| !note.trim().is_empty())
}

fn display_path<'a>(paths: impl Iterator<Item = &'a Path>) -> FeaturePath {
    let paths = paths.collect::<Vec<_>>();
    let Some(longest) = paths.iter().max_by_key(|path| path.features.len()) else {
        return FeaturePath::Absent;
    };
    if paths
        .iter()
        .all(|path| longest.features.ends_with(&path.features))
    {
        FeaturePath::Features(longest.features.clone())
    } else {
        FeaturePath::Inconsistent
    }
}

/// Offset of the first declaration of `class` at or after `from`.
fn declaration(archive: &[u8], class: &[u8], from: usize) -> Option<usize> {
    let length = u16::try_from(class.len()).ok()?;
    (from..archive.len()).find(|&offset| {
        View::u16_le_at(archive, offset) == Some(CLASS_DECLARATION)
            && offset
                .checked_add(4)
                .and_then(|at| View::u16_le_at(archive, at))
                == Some(length)
            && offset
                .checked_add(6)
                .and_then(|at| archive.get(at..at.checked_add(class.len())?))
                == Some(class)
    })
}

/// Whether the class declarations before `end` are exactly the archive head.
fn head_declares_only(archive: &[u8], end: usize) -> bool {
    let mut declared = Vec::new();
    let mut next = 0;
    for offset in 0..end {
        if offset < next {
            continue;
        }
        if let Some(class) = declared_class(archive, offset) {
            declared.push(class);
            next = offset
                .checked_add(6)
                .and_then(|at| at.checked_add(class.len()))
                .unwrap_or(end);
        }
    }
    declared == HEAD_CLASSES
}

fn declared_class(archive: &[u8], offset: usize) -> Option<&[u8]> {
    if View::u16_le_at(archive, offset)? != CLASS_DECLARATION {
        return None;
    }
    let length = usize::from(View::u16_le_at(archive, offset.checked_add(4)?)?);
    let start = offset.checked_add(6)?;
    let class = archive.get(start..start.checked_add(length)?)?;
    let identifier = class.first().is_some_and(u8::is_ascii_alphabetic)
        && class
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
    (length >= 3 && identifier).then_some(class)
}

/// Offsets of the tagged `moDisplayGtol_c` objects after its declaration.
fn gtol_tags(archive: &[u8], declaration: usize) -> Vec<usize> {
    (declaration..archive.len())
        .filter(|&offset| {
            View::u16_le_at(archive, offset) == Some(gtol_head::DISPLAY_TAG_VALUE)
                && offset
                    .checked_add(gtol_head::BODY_WORD)
                    .and_then(|at| View::u32_le_at(archive, at))
                    == Some(gtol_head::BODY_WORD_VALUE)
                && offset
                    .checked_add(gtol_head::COMPOUND_TAG)
                    .and_then(|at| View::u16_le_at(archive, at))
                    == Some(gtol_head::COMPOUND_TAG_VALUE)
        })
        .collect()
}

/// Named objects in stream order and the name of each presentation object ID.
fn named_objects(archive: &[u8]) -> (Vec<NamedObject>, HashMap<u32, Option<String>>) {
    let mut objects = Vec::new();
    let mut names: HashMap<u32, Option<String>> = HashMap::new();
    for offset in 0..archive.len() {
        if View::u16_le_at(archive, offset) != Some(NAME_TAG) {
            continue;
        }
        let Some(text) = offset
            .checked_add(2)
            .and_then(|at| framed_text(archive, at))
        else {
            continue;
        };
        let id = text
            .end
            .checked_add(named_header::OBJECT_ID)
            .and_then(|at| View::u32_le_at(archive, at));
        if let Some(id) = id.filter(|id| *id != NO_OBJECT_ID) {
            names
                .entry(id)
                .and_modify(|name| *name = None)
                .or_insert_with(|| Some(text.value.clone()));
        }
        objects.push(NamedObject {
            offset,
            name: text.value,
        });
    }
    (objects, names)
}

/// Feature paths: a u16 count and that many `moSwiftFR_c` objects, each a
/// presentation object ID and a timestamp.
fn feature_paths(archive: &[u8], names: &HashMap<u32, Option<String>>) -> Vec<Path> {
    let Some(declaration) = declaration(archive, FEATURE_REFERENCE_CLASS, 0) else {
        return Vec::new();
    };
    let Some(declared_len) = FEATURE_REFERENCE_CLASS.len().checked_add(6) else {
        return Vec::new();
    };
    let reader = PathReader {
        archive,
        names,
        declaration,
        declared_len,
    };
    let mut tags = BTreeSet::new();
    for offset in 0..archive.len() {
        let Some(tag) = offset
            .checked_add(2)
            .and_then(|at| View::u16_le_at(archive, at))
            .filter(|tag| tag & OBJECT_TAG != 0 && *tag != CLASS_DECLARATION)
        else {
            continue;
        };
        if reader.read(offset, Some(tag)).is_some() {
            tags.insert(tag);
        }
    }
    let tag = match tags.len() {
        0 => None,
        1 => tags.first().copied(),
        _ => return Vec::new(),
    };
    let mut paths = Vec::new();
    let mut stamps: HashMap<u32, Option<u32>> = HashMap::new();
    let mut next = 0;
    for offset in 0..archive.len() {
        if offset < next {
            continue;
        }
        let Some((end, features)) = reader.read(offset, tag) else {
            continue;
        };
        for &(id, stamp) in &features {
            stamps
                .entry(id)
                .and_modify(|known| {
                    if *known != Some(stamp) {
                        *known = None;
                    }
                })
                .or_insert(Some(stamp));
        }
        paths.push((offset, features));
        next = end;
    }
    paths
        .into_iter()
        .filter(|(_, features)| {
            features
                .iter()
                .all(|(id, _)| stamps.get(id).is_some_and(Option::is_some))
        })
        .map(|(offset, features)| Path {
            offset,
            features: features.into_iter().map(|(id, _)| id).collect(),
        })
        .collect()
}

struct PathReader<'a> {
    archive: &'a [u8],
    names: &'a HashMap<u32, Option<String>>,
    declaration: usize,
    declared_len: usize,
}

impl PathReader<'_> {
    fn read(&self, offset: usize, tag: Option<u16>) -> Option<(usize, Vec<(u32, u32)>)> {
        let count = View::u16_le_at(self.archive, offset)?;
        if !(1..=MAX_PATH_FEATURES).contains(&count) {
            return None;
        }
        let mut cursor = offset.checked_add(2)?;
        let mut features = Vec::new();
        for _ in 0..count {
            let object = if cursor == self.declaration {
                cursor
                    .checked_add(self.declared_len)?
                    .checked_sub(feature_reference::OBJECT_ID)?
            } else if tag.is_some() && View::u16_le_at(self.archive, cursor) == tag {
                cursor
            } else {
                return None;
            };
            let id = View::u32_le_at(
                self.archive,
                object.checked_add(feature_reference::OBJECT_ID)?,
            )?;
            let stamp =
                View::u32_le_at(self.archive, object.checked_add(feature_reference::STAMP)?)?;
            if !self.names.contains_key(&id) {
                return None;
            }
            features.push((id, stamp));
            cursor = object.checked_add(feature_reference::LEN)?;
        }
        Some((cursor, features))
    }
}

/// Framed UTF-16 strings in stream order.
fn texts(archive: &[u8]) -> Vec<Text> {
    let mut texts = Vec::new();
    let mut next = 0;
    for offset in 0..archive.len() {
        if offset < next {
            continue;
        }
        if let Some(text) = framed_text(archive, offset) {
            next = text.end;
            texts.push(text);
        }
    }
    texts
}

fn framed_text(archive: &[u8], offset: usize) -> Option<Text> {
    if archive.get(offset..offset.checked_add(STRING_MARKER.len())?)? != STRING_MARKER {
        return None;
    }
    let count_at = offset.checked_add(STRING_MARKER.len())?;
    let first = *archive.get(count_at)?;
    let (units, start) = if first == WIDE_STRING_COUNT {
        let units = View::u16_le_at(archive, count_at.checked_add(1)?)?;
        (usize::from(units), count_at.checked_add(3)?)
    } else {
        (usize::from(first), count_at.checked_add(1)?)
    };
    let end = start.checked_add(units.checked_mul(2)?)?;
    if end > archive.len() {
        return None;
    }
    let mut value = String::new();
    decode_rendered_utf16(archive, start, units, &mut value)?;
    Some(Text { offset, end, value })
}

/// Presentation annotations for the archive's geometric-tolerance displays
/// and the number of displays without an established semantic association.
/// A depicted tolerance without datum references receives the datum system
/// its frame row displays.
pub(super) fn annotations(
    archive: &Archive,
    root: &Entity,
    projected: &mut Vec<PmiAnnotation>,
    face_identities: &[(FaceId, PersistentFaceIdentity)],
    sw_version: Option<u32>,
    losses: &mut Vec<LossNote>,
) -> Result<(Vec<PmiAnnotation>, usize), CodecError> {
    let faces = face_index(face_identities);
    let (links, systems) = {
        let semantic = SemanticIndex::new(root, projected);
        let links = archive
            .gtols
            .iter()
            .map(|display| semantic.depicted(archive, display, &faces))
            .collect::<Vec<_>>();
        let mut systems = Vec::new();
        for (display, link) in archive.gtols.iter().zip(&links) {
            if let Some(depicted) = link {
                systems.extend(semantic.row_datum_systems(display, depicted));
            }
        }
        (links, systems)
    };
    apply_datum_systems(projected, systems)?;
    let mut unresolved = 0usize;
    let mut short = 0usize;
    let mut tail = 0usize;
    let mut internal_flatness = 0usize;
    let tail_is_stated = DocumentLayout::tail_is_stated(sw_version);
    let mut presentations = Vec::new();
    for (display, semantics) in archive.gtols.iter().zip(links) {
        let overflow = || CodecError::malformed("SWIFT display count overflows");
        if semantics.is_none() {
            unresolved = unresolved.checked_add(1).ok_or_else(overflow)?;
        } else if display.path == FeaturePath::Absent {
            if display.document.short {
                short = short.checked_add(1).ok_or_else(overflow)?;
            }
            if display.document.in_place && !tail_is_stated {
                tail = tail.checked_add(1).ok_or_else(overflow)?;
            }
        }
        if semantics.is_some()
            && display
                .characteristics
                .iter()
                .any(|token| token == "IGTOL-FLAT")
        {
            internal_flatness = internal_flatness.checked_add(1).ok_or_else(overflow)?;
        }
        let id = PmiId::mint(format!("sldprt:model:pmi#display-{:x}", display.offset))
            .map_err(|_| CodecError::malformed("invalid SWIFT display ID"))?;
        presentations.push(PmiAnnotation {
            id,
            name: display.name.clone(),
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Presentation {
                text: display.note.clone(),
                placement: None,
                semantics: semantics.unwrap_or_default(),
            },
        });
    }
    if short > 0 {
        losses.push(SldprtLossCode::PmiSwiftFaceDocumentLayoutAssumed.note(format!(
            "{short} SWIFT geometric-tolerance display(s) select faces through a document written in place with one or two strings, read as the document layout of sldprt.md section 2.3 with one or two of its three strings"
        )));
    }
    if internal_flatness > 0 {
        losses.push(SldprtLossCode::PmiSwiftInternalFlatnessSymbolAssumed.note(format!(
            "{internal_flatness} SWIFT display(s) use IGTOL-FLAT as flatness through their owner or face references and one common magnitude unit factor; the internal prefix semantics remain native"
        )));
    }
    if tail > 0 {
        let version = sw_version.map_or_else(
            || "no declared swVersion".to_owned(),
            |version| format!("swVersion {version}"),
        );
        losses.push(SldprtLossCode::PmiSwiftFaceDocumentTailAssumed.note(format!(
            "{tail} SWIFT geometric-tolerance display(s) select faces through a document written in place in a file with {version}, read with the 12-byte tail after its configuration name that sldprt.md section 2.3 states below swVersion 17000"
        )));
    }
    Ok((presentations, unresolved))
}

fn face_index(
    identities: &[(FaceId, PersistentFaceIdentity)],
) -> HashMap<FaceIdentity, Vec<&FaceId>> {
    let mut index: HashMap<FaceIdentity, Vec<&FaceId>> = HashMap::new();
    for (face, identity) in identities {
        index
            .entry(FaceIdentity {
                feature: identity.feature_source_id.value(),
                local: identity.local_id,
                tail: identity.trailing_fields.clone(),
            })
            .or_default()
            .push(face);
    }
    index
}

fn apply_datum_systems(
    projected: &mut Vec<PmiAnnotation>,
    systems: Vec<(PmiId, DatumReferences)>,
) -> Result<(), CodecError> {
    for (tolerance, references) in systems {
        let system = PmiId::mint(format!("{}:datum-system", tolerance.as_str()))
            .map_err(|_| CodecError::malformed("invalid SWIFT datum-system ID"))?;
        if projected.iter().any(|annotation| annotation.id == system) {
            continue;
        }
        let Some(PmiDefinition::GeometricTolerance { datum_system, .. }) = projected
            .iter_mut()
            .find(|annotation| annotation.id == tolerance)
            .map(|annotation| &mut annotation.definition)
        else {
            continue;
        };
        *datum_system = Some(system.clone());
        projected.push(PmiAnnotation {
            id: system,
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem { references },
        });
    }
    Ok(())
}

/// One projected geometric tolerance.
struct Tolerance<'a> {
    id: &'a PmiId,
    kind: &'a GeometricToleranceKind,
    magnitude: f64,
    datum_system: Option<&'a PmiId>,
    faces: BTreeSet<&'a FaceId>,
}

struct SemanticIndex<'a> {
    features_by_attribute: HashMap<i32, Vec<(&'a str, Option<&'a str>)>>,
    features_by_name: HashMap<&'a str, Vec<&'a str>>,
    sources: Vec<(&'a str, BTreeSet<&'a str>)>,
    tolerances: Vec<Tolerance<'a>>,
    datums: HashMap<&'a str, Vec<&'a PmiId>>,
    systems: HashMap<&'a PmiId, Vec<&'a PmiId>>,
}

impl<'a> SemanticIndex<'a> {
    fn new(root: &'a Entity, projected: &'a [PmiAnnotation]) -> Self {
        let mut features_by_attribute: HashMap<i32, Vec<(&str, Option<&str>)>> = HashMap::new();
        let mut features_by_name: HashMap<&str, Vec<&str>> = HashMap::new();
        for (reference, entity) in root.features.references.iter().zip(&root.features.entities) {
            if let Some(attribute) = entity.integers.get("UserAttribute") {
                features_by_attribute.entry(*attribute).or_default().push((
                    reference.id.as_str(),
                    entity.strings.get("ObjectName").map(String::as_str),
                ));
            }
            if let Some(name) = entity.strings.get("ObjectName") {
                features_by_name
                    .entry(name.as_str())
                    .or_default()
                    .push(reference.id.as_str());
            }
        }
        let sources = root
            .annotations
            .references
            .iter()
            .zip(&root.annotations.entities)
            .filter(|(_, entity)| {
                !suppressed(entity) && tolerance_kind(short_class(&entity.class)).is_some()
            })
            .map(|(reference, entity)| {
                let features = entity
                    .features
                    .references
                    .iter()
                    .map(|feature| feature.id.as_str())
                    .collect();
                (reference.id.as_str(), features)
            })
            .collect();
        let mut tolerances = Vec::new();
        let mut datums: HashMap<&str, Vec<&PmiId>> = HashMap::new();
        let mut systems = HashMap::new();
        for annotation in projected {
            match &annotation.definition {
                PmiDefinition::GeometricTolerance {
                    tolerance,
                    magnitude,
                    datum_system,
                    ..
                } => tolerances.push(Tolerance {
                    id: &annotation.id,
                    kind: tolerance,
                    magnitude: magnitude.get().value.get(),
                    datum_system: datum_system.as_ref(),
                    faces: annotation
                        .targets
                        .iter()
                        .filter_map(|target| match target {
                            PmiTarget::Face { face } => Some(face),
                            _ => None,
                        })
                        .collect(),
                }),
                PmiDefinition::Datum { identification } => datums
                    .entry(identification.as_str())
                    .or_default()
                    .push(&annotation.id),
                PmiDefinition::DatumSystem { references } => {
                    let mut ordered = references.as_slice().iter().collect::<Vec<_>>();
                    ordered.sort_by_key(|reference| reference.precedence);
                    systems.insert(
                        &annotation.id,
                        ordered
                            .into_iter()
                            .map(|reference| &reference.datum)
                            .collect(),
                    );
                }
                _ => {}
            }
        }
        Self {
            features_by_attribute,
            features_by_name,
            sources,
            tolerances,
            datums,
            systems,
        }
    }

    fn source_tolerances<'s>(
        &'s self,
        source: &'s str,
    ) -> impl Iterator<Item = &'s Tolerance<'a>> + 's {
        self.tolerances.iter().filter(move |tolerance| {
            tolerance
                .id
                .as_str()
                .strip_prefix("sldprt:model:pmi#")
                .map(|id| id.strip_suffix(COMPOSITE_LOWER_TIER_SUFFIX).unwrap_or(id))
                == Some(source)
        })
    }

    /// The semantic feature that a presentation object names: the one whose
    /// `UserAttribute` equals the object identifier, when its `ObjectName`
    /// and the object's name agree, or else the one whose `ObjectName` equals
    /// the object's name.
    fn feature(&self, archive: &Archive, object: u32) -> Option<&'a str> {
        let presented = archive.names.get(&object).and_then(Option::as_deref);
        let attributed = i32::try_from(object)
            .ok()
            .and_then(|attribute| self.features_by_attribute.get(&attribute))
            .map(Vec::as_slice);
        if let Some([(id, name)]) = attributed {
            return match (name, presented) {
                (Some(name), Some(presented)) if name != &presented => None,
                _ => Some(*id),
            };
        }
        match self.features_by_name.get(presented?).map(Vec::as_slice) {
            Some([id]) => Some(*id),
            _ => None,
        }
    }

    fn depicted(
        &self,
        archive: &Archive,
        display: &GtolDisplay,
        faces: &HashMap<FaceIdentity, Vec<&FaceId>>,
    ) -> Option<Vec<PmiId>> {
        let expected = display
            .characteristics
            .iter()
            .map(|token| row_kind(token))
            .collect::<Option<Vec<_>>>()?;
        let depicted = match (&display.path, &display.faces) {
            (FeaturePath::Features(path), _) => self.depicted_by_features(archive, path)?,
            (FeaturePath::Absent, FaceAttachment::Faces(identities)) => {
                self.depicted_by_faces(identities, faces, &expected)?
            }
            _ => return None,
        };
        let mut kinds = depicted
            .iter()
            .map(|tolerance| tolerance.kind.clone())
            .collect::<Vec<_>>();
        let mut expected = expected;
        let order = |kind: &GeometricToleranceKind| format!("{kind:?}");
        expected.sort_by_key(order);
        kinds.sort_by_key(order);
        if depicted.is_empty() || expected != kinds {
            return None;
        }
        if display
            .characteristics
            .iter()
            .any(|token| token == "IGTOL-FLAT")
        {
            if !display
                .characteristics
                .iter()
                .all(|token| token == "IGTOL-FLAT")
                || display.rows.len() < 2
                || display.rows.len() != depicted.len()
            {
                return None;
            }
            let mut displayed = display
                .rows
                .iter()
                .map(|row| {
                    row.tolerance
                        .as_deref()?
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|value| value.is_finite() && *value > 0.0)
                })
                .collect::<Option<Vec<_>>>()?;
            let mut stored = depicted
                .iter()
                .map(|tolerance| tolerance.magnitude)
                .collect::<Vec<_>>();
            displayed.sort_by(f64::total_cmp);
            stored.sort_by(f64::total_cmp);
            let scale = stored.first()? / displayed.first()?;
            if !scale.is_finite()
                || scale <= 0.0
                || !stored.iter().zip(displayed).all(|(stored, displayed)| {
                    (stored - displayed * scale).abs()
                        <= EPS_INTERNAL_FLATNESS_UNIT_RELATIVE * stored.abs()
                })
            {
                return None;
            }
        }
        Some(
            depicted
                .iter()
                .map(|tolerance| tolerance.id.clone())
                .collect(),
        )
    }

    /// The tolerances of every semantic annotation that directly references a
    /// feature on the path; `None` when one of them is not projected.
    fn depicted_by_features(&self, archive: &Archive, path: &[u32]) -> Option<Vec<&Tolerance<'a>>> {
        let features = path
            .iter()
            .filter_map(|object| self.feature(archive, *object))
            .collect::<BTreeSet<_>>();
        if features.is_empty() {
            return None;
        }
        let mut depicted = Vec::new();
        for (source, references) in &self.sources {
            if references.is_disjoint(&features) {
                continue;
            }
            let before = depicted.len();
            depicted.extend(self.source_tolerances(source));
            if depicted.len() == before {
                return None;
            }
        }
        Some(depicted)
    }

    /// The tolerances of the displayed characteristics that target a face the
    /// display's face references select; `None` when a reference selects no
    /// face.
    fn depicted_by_faces(
        &self,
        identities: &[FaceIdentity],
        faces: &HashMap<FaceIdentity, Vec<&FaceId>>,
        expected: &[GeometricToleranceKind],
    ) -> Option<Vec<&Tolerance<'a>>> {
        let mut selected = BTreeSet::new();
        for identity in identities {
            let matched = faces.get(identity).filter(|matched| !matched.is_empty())?;
            selected.extend(matched.iter().copied());
        }
        Some(
            self.tolerances
                .iter()
                .filter(|tolerance| {
                    expected.contains(tolerance.kind) && !tolerance.faces.is_disjoint(&selected)
                })
                .collect(),
        )
    }

    /// Datum systems that a display's rows supply to depicted tolerances
    /// without datum references. Rows pair with tolerances by characteristic
    /// when each characteristic occurs once, and a composite profile's upper
    /// and lower tiers pair with its two rows in order. A row whose datum
    /// compartments disagree with an existing datum system withholds the
    /// display's datum systems.
    fn row_datum_systems(
        &self,
        display: &GtolDisplay,
        depicted: &[PmiId],
    ) -> Vec<(PmiId, DatumReferences)> {
        let tolerances = depicted
            .iter()
            .filter_map(|id| self.tolerances.iter().find(|tolerance| tolerance.id == id))
            .collect::<Vec<_>>();
        let Some(pairs) = pair_rows(&display.rows, &tolerances) else {
            return Vec::new();
        };
        let mut systems = Vec::new();
        for (row, tolerance) in pairs {
            let mut datums = Vec::new();
            for (label, modified) in &row.labels {
                let Some(ids) = self.datums.get(label.as_str()) else {
                    continue;
                };
                let [datum] = ids.as_slice() else {
                    return Vec::new();
                };
                if *modified {
                    return Vec::new();
                }
                datums.push(*datum);
            }
            match tolerance.datum_system {
                Some(system) => {
                    if self.systems.get(system) != Some(&datums) {
                        return Vec::new();
                    }
                }
                None if datums.is_empty() => {}
                None => {
                    let Some(references) = datum_references(&datums) else {
                        return Vec::new();
                    };
                    systems.push((tolerance.id.clone(), references));
                }
            }
        }
        systems
    }
}

fn pair_rows<'r, 't, 'a>(
    rows: &'r [Row],
    tolerances: &[&'t Tolerance<'a>],
) -> Option<Vec<(&'r Row, &'t Tolerance<'a>)>> {
    if rows.len() != tolerances.len() {
        return None;
    }
    let kinds = rows
        .iter()
        .map(|row| characteristic(&row.characteristic).and_then(row_kind))
        .collect::<Option<Vec<_>>>()?;
    let distinct = kinds
        .iter()
        .all(|kind| kinds.iter().filter(|other| *other == kind).count() == 1);
    if distinct {
        return rows
            .iter()
            .zip(&kinds)
            .map(|(row, kind)| {
                let mut matching = tolerances.iter().filter(|tolerance| tolerance.kind == kind);
                let tolerance = matching.next()?;
                matching.next().is_none().then_some((row, *tolerance))
            })
            .collect();
    }
    let ([upper_row, lower_row], [first, second]) = (rows, tolerances) else {
        return None;
    };
    let (upper, lower) = if second.id.as_str().strip_suffix(COMPOSITE_LOWER_TIER_SUFFIX)
        == Some(first.id.as_str())
    {
        (*first, *second)
    } else if first.id.as_str().strip_suffix(COMPOSITE_LOWER_TIER_SUFFIX)
        == Some(second.id.as_str())
    {
        (*second, *first)
    } else {
        return None;
    };
    let literal = |row: &Row| {
        row.tolerance
            .as_deref()?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| *value > 0.0)
    };
    let displayed = literal(upper_row)? / literal(lower_row)?;
    let stored = upper.magnitude / lower.magnitude;
    ((displayed - stored).abs() <= EPS_TIER_RATIO_E9 * stored.abs().max(1.0))
        .then(|| vec![(upper_row, upper), (lower_row, lower)])
}

fn datum_references(datums: &[&PmiId]) -> Option<DatumReferences> {
    let references = datums
        .iter()
        .enumerate()
        .map(|(index, datum)| {
            let precedence = u32::try_from(index.checked_add(1)?)
                .ok()
                .and_then(NonZeroU32::new)?;
            Some(DatumReference {
                datum: (*datum).clone(),
                precedence,
                common_group: None,
                modifiers: Vec::new(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    DatumReferences::try_from(references).ok()
}

fn row_kind(token: &str) -> Option<GeometricToleranceKind> {
    Some(match token {
        "POSI" => GeometricToleranceKind::Position,
        "PERP" => GeometricToleranceKind::Perpendicularity,
        "PARA" => GeometricToleranceKind::Parallelism,
        "ANGULAR" => GeometricToleranceKind::Angularity,
        "FLAT" | "IGTOL-FLAT" => GeometricToleranceKind::Flatness,
        "STRAIGHT" => GeometricToleranceKind::Straightness,
        "CIRC" => GeometricToleranceKind::Roundness,
        "CYL" => GeometricToleranceKind::Cylindricity,
        "SPROF" => GeometricToleranceKind::SurfaceProfile,
        "LPROF" => GeometricToleranceKind::LineProfile,
        "CONC" => GeometricToleranceKind::Concentricity,
        "SYMMETRY" => GeometricToleranceKind::Symmetry,
        "SRUN" => GeometricToleranceKind::CircularRunout,
        "TRUN" => GeometricToleranceKind::TotalRunout,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
