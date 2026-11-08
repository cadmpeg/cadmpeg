//! SWIFT presentation archive: geometric-tolerance displays, their feature
//! paths, and the semantic annotations they depict.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiAnnotation, PmiDefinition};

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

/// The geometric-tolerance displays of one presentation archive.
#[derive(Debug, Default)]
pub(super) struct Archive {
    names: HashMap<u32, Option<String>>,
    gtols: Vec<GtolDisplay>,
    unbounded: Vec<usize>,
}

#[derive(Debug)]
struct GtolDisplay {
    offset: usize,
    name: Option<String>,
    characteristics: Vec<String>,
    path: FeaturePath,
    note: Option<String>,
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
        return Archive::default();
    }
    let Some(count) = gtol
        .checked_sub(2)
        .and_then(|at| View::u16_le_at(archive, at))
    else {
        return Archive::default();
    };
    let mut starts = vec![gtol];
    starts.extend(gtol_tags(archive, gtol));
    if starts.len() != usize::from(count) {
        return Archive::default();
    }
    let (named, names) = named_objects(archive);
    let paths = feature_paths(archive, &names);
    let texts = texts(archive);
    let mut ends: Vec<Option<usize>> = starts.iter().skip(1).copied().map(Some).collect();
    let last = starts.last().copied().unwrap_or(gtol);
    ends.push(declaration(archive, DATUM_TAG_CLASS, last).and_then(|start| start.checked_sub(2)));
    let mut parsed = Archive {
        names,
        ..Archive::default()
    };
    for (&start, end) in starts.iter().zip(ends) {
        match end.filter(|end| *end > start) {
            Some(end) => parsed
                .gtols
                .push(gtol_display(start, end, &named, &paths, &texts)),
            None => parsed.unbounded.push(start),
        }
    }
    parsed
}

fn gtol_display(
    start: usize,
    end: usize,
    named: &[NamedObject],
    paths: &[Path],
    texts: &[Text],
) -> GtolDisplay {
    let inside = |offset: usize| (start..end).contains(&offset);
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
    GtolDisplay {
        offset: start,
        name,
        characteristics,
        path: display_path(paths.iter().filter(|path| inside(path.offset))),
        note: frame_note(&texts),
    }
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

/// The note of the unique four-string block whose third string is the frame
/// template.
fn frame_note(texts: &[&Text]) -> Option<String> {
    let mut notes = texts.windows(4).filter_map(|block| match block {
        [first, second, template, note]
            if first.end == second.offset
                && second.end == template.offset
                && template.end == note.offset
                && template.value.contains(FRAME_TEMPLATE_TOKEN) =>
        {
            Some(note)
        }
        _ => None,
    });
    let note = notes.next()?;
    if notes.next().is_some() {
        return None;
    }
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
pub(super) fn annotations(
    archive: &Archive,
    root: &Entity,
    projected: &[PmiAnnotation],
) -> Result<(Vec<PmiAnnotation>, usize), CodecError> {
    let semantic = SemanticIndex::new(root, projected);
    let mut unresolved = archive.unbounded.len();
    let mut presentations = Vec::new();
    for display in &archive.gtols {
        let semantics = semantic.depicted(archive, display);
        if semantics.is_none() {
            unresolved = unresolved
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("SWIFT display count overflows"))?;
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
    Ok((presentations, unresolved))
}

struct SemanticIndex<'a> {
    features_by_attribute: HashMap<i32, Vec<&'a str>>,
    features_by_name: HashMap<&'a str, Vec<&'a str>>,
    tolerances: Vec<(&'a str, BTreeSet<&'a str>)>,
    projected: BTreeMap<&'a str, Vec<(&'a PmiId, &'a GeometricToleranceKind)>>,
}

impl<'a> SemanticIndex<'a> {
    fn new(root: &'a Entity, projected: &'a [PmiAnnotation]) -> Self {
        let mut features_by_attribute: HashMap<i32, Vec<&str>> = HashMap::new();
        let mut features_by_name: HashMap<&str, Vec<&str>> = HashMap::new();
        for (reference, entity) in root.features.references.iter().zip(&root.features.entities) {
            if let Some(attribute) = entity.integers.get("UserAttribute") {
                features_by_attribute
                    .entry(*attribute)
                    .or_default()
                    .push(reference.id.as_str());
            }
            if let Some(name) = entity.strings.get("ObjectName") {
                features_by_name
                    .entry(name.as_str())
                    .or_default()
                    .push(reference.id.as_str());
            }
        }
        let tolerances = root
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
        let mut by_source: BTreeMap<&str, Vec<(&PmiId, &GeometricToleranceKind)>> = BTreeMap::new();
        for annotation in projected {
            let PmiDefinition::GeometricTolerance { tolerance, .. } = &annotation.definition else {
                continue;
            };
            let Some(source) = annotation.id.as_str().strip_prefix("sldprt:model:pmi#") else {
                continue;
            };
            let source = source.strip_suffix(":lower-tier").unwrap_or(source);
            by_source
                .entry(source)
                .or_default()
                .push((&annotation.id, tolerance));
        }
        Self {
            features_by_attribute,
            features_by_name,
            tolerances,
            projected: by_source,
        }
    }

    fn feature(&self, archive: &Archive, object: u32) -> Option<&'a str> {
        let unique = |ids: Option<&Vec<&'a str>>| match ids.map(Vec::as_slice) {
            Some([id]) => Some(*id),
            _ => None,
        };
        i32::try_from(object)
            .ok()
            .and_then(|attribute| unique(self.features_by_attribute.get(&attribute)))
            .or_else(|| {
                let name = archive.names.get(&object)?.as_deref()?;
                unique(self.features_by_name.get(name))
            })
    }

    fn depicted(&self, archive: &Archive, display: &GtolDisplay) -> Option<Vec<PmiId>> {
        let FeaturePath::Features(path) = &display.path else {
            return None;
        };
        let features = path
            .iter()
            .filter_map(|object| self.feature(archive, *object))
            .collect::<BTreeSet<_>>();
        if features.is_empty() {
            return None;
        }
        let mut expected = display
            .characteristics
            .iter()
            .map(|token| row_kind(token))
            .collect::<Option<Vec<_>>>()?;
        let mut depicted = Vec::new();
        let mut kinds = Vec::new();
        for (source, references) in &self.tolerances {
            if references.is_disjoint(&features) {
                continue;
            }
            let projected = self.projected.get(source)?;
            for (id, kind) in projected {
                depicted.push((*id).clone());
                kinds.push(*kind);
            }
        }
        let mut kinds = kinds.into_iter().cloned().collect::<Vec<_>>();
        let order = |kind: &GeometricToleranceKind| format!("{kind:?}");
        expected.sort_by_key(order);
        kinds.sort_by_key(order);
        (!depicted.is_empty() && expected == kinds).then_some(depicted)
    }
}

fn row_kind(token: &str) -> Option<GeometricToleranceKind> {
    Some(match token {
        "POSI" => GeometricToleranceKind::Position,
        "PERP" => GeometricToleranceKind::Perpendicularity,
        "PARA" => GeometricToleranceKind::Parallelism,
        "ANGULAR" => GeometricToleranceKind::Angularity,
        "FLAT" => GeometricToleranceKind::Flatness,
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
