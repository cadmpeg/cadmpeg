//! Persistent face identities that SWIFT presentation displays reference.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;

use super::{declaration, declared_class, framed_text, CLASS_DECLARATION, OBJECT_TAG};
use crate::layout::identity_reference_header as header;

const IDENTITY_CLASS_SUFFIX: &[u8] = b"IdRep_c";
const FEATURE_RECORD_CLASS: &[u8] = b"moFR_c";
const DOCUMENT_CLASS: &[u8] = b"moExtObject_c";
const STRING_HANDLE_CLASS: &[u8] = b"moCStringHandle_c";
const FACE_REFERENCE_CLASS: &[u8] = b"moFaceRef_c";
const REFERENCE_CLASSES: [&[u8]; 4] = [
    b"moFaceRef_c",
    b"moEdgeRef_c",
    b"moLoopRef_c",
    b"moSilhEdgeRef_c",
];
const MAX_REFERENCE_HOPS: u32 = 64;
const DOCUMENT_FORM: u16 = 2;
const DOCUMENT_STRINGS: usize = 3;
const DOCUMENT_BYTES_BEFORE_CONFIGURATION: [usize; 2] = [17, 18];
const DOCUMENT_BYTES_AFTER_CONFIGURATION: usize = 12;
const ENTRY_TRAILER: usize = 8;
const FEATURE_RECORD_WORDS: usize = 8;

/// The persistent identity a face reference's first hop names: the producing
/// feature, its feature-local face identity, and the ordered path fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::swift) struct FaceIdentity {
    pub(in crate::swift) feature: u32,
    pub(in crate::swift) local: u32,
    pub(in crate::swift) tail: Vec<u32>,
}

/// One identity reference: its header offset and, for a resolved face
/// reference, the identity of the face it selects.
#[derive(Debug)]
pub(super) struct IdentityReference {
    pub(super) offset: usize,
    pub(super) face: Option<FaceIdentity>,
}

/// Own `u32` words after the feature record for identity classes whose hop
/// names one face directly.
fn own_words(class: &[u8]) -> Option<usize> {
    Some(match class {
        b"moSimpleSurfIdRep_c"
        | b"moFromSktEntSurfIdRep_c"
        | b"moFilletSurfIdRep_c"
        | b"moThinOffsetSurfIdRep_c" => 1,
        b"moEndFaceSurfIdRep_c"
        | b"moThinClosingSurfIdRep_c"
        | b"moUnBendSurfIdRep_c"
        | b"moBendEndSurfIdRep_c"
        | b"moWzdHoleSurfIdRep_c" => 2,
        b"moFromSktEnt3IntSurfIdRep_c" | b"moEndFace3IntSurfIdRep_c" => 3,
        _ => return None,
    })
}

/// Every identity reference of the archive, in stream order.
pub(super) fn identity_references(archive: &[u8]) -> Vec<IdentityReference> {
    let headers = headers(archive);
    let Some(reader) = Reader::new(archive, &headers) else {
        return headers
            .iter()
            .map(|&offset| IdentityReference { offset, face: None })
            .collect();
    };
    headers
        .iter()
        .map(|&offset| IdentityReference {
            offset,
            face: reader.face(offset),
        })
        .collect()
}

fn headers(archive: &[u8]) -> Vec<usize> {
    (0..archive.len())
        .filter(|&offset| {
            let field = |at: usize| offset.checked_add(at);
            let marker = field(header::MARKER).and_then(|start| {
                archive.get(start..start.checked_add(header::MARKER_VALUE.len())?)
            });
            marker == Some(&header::MARKER_VALUE[..])
                && field(header::FORM).and_then(|at| View::u32_le_at(archive, at))
                    == Some(header::FORM_VALUE)
                && field(header::ZERO_WORD).and_then(|at| View::u32_le_at(archive, at))
                    == Some(header::ZERO_WORD_VALUE)
                && field(header::ZERO_TAIL).and_then(|at| View::u16_le_at(archive, at))
                    == Some(header::ZERO_TAIL_VALUE)
                && field(header::HOP_COUNT)
                    .and_then(|at| View::u32_le_at(archive, at))
                    .is_some_and(|count| (1..=MAX_REFERENCE_HOPS).contains(&count))
        })
        .collect()
}

struct Reader<'a> {
    archive: &'a [u8],
    feature_record: Option<usize>,
    feature_record_tag: Option<u16>,
    document: Option<usize>,
    string_handle: Option<usize>,
    hop_classes: BTreeMap<u16, &'a [u8]>,
    face_references: BTreeSet<u16>,
}

impl<'a> Reader<'a> {
    fn new(archive: &'a [u8], headers: &[usize]) -> Option<Self> {
        let feature_record = declaration(archive, FEATURE_RECORD_CLASS, 0);
        let mut reader = Self {
            archive,
            feature_record,
            feature_record_tag: None,
            document: declaration(archive, DOCUMENT_CLASS, 0),
            string_handle: declaration(archive, STRING_HANDLE_CLASS, 0),
            hop_classes: BTreeMap::new(),
            face_references: BTreeSet::new(),
        };
        let tags = reader.feature_record_tags(headers);
        if tags.len() > 1 {
            return None;
        }
        reader.feature_record_tag = tags.first().copied();
        reader.hop_classes = reader.hop_classes()?;
        reader.face_references = reader.face_reference_tags(headers);
        Some(reader)
    }

    fn u16_at(&self, offset: usize) -> Option<u16> {
        View::u16_le_at(self.archive, offset)
    }

    fn u32_at(&self, offset: usize) -> Option<u32> {
        View::u32_le_at(self.archive, offset)
    }

    fn is_tag(value: u16) -> bool {
        value & OBJECT_TAG != 0 && value != CLASS_DECLARATION
    }

    /// The values that follow a tagged first hop and its zero word; the one
    /// such value is the class tag of tagged `moFR_c` objects.
    fn feature_record_tags(&self, headers: &[usize]) -> BTreeSet<u16> {
        let mut tags = BTreeSet::new();
        for &offset in headers {
            let Some(entry) = offset.checked_add(header::LEN) else {
                continue;
            };
            let (Some(zero), Some(record)) = (entry.checked_add(2), entry.checked_add(4)) else {
                continue;
            };
            if self.u16_at(entry).is_some_and(Self::is_tag)
                && self.u16_at(zero) == Some(0)
                && Some(record) != self.feature_record
            {
                if let Some(tag) = self.u16_at(record) {
                    tags.insert(tag);
                }
            }
        }
        tags
    }

    fn feature_record_at(&self, offset: usize) -> bool {
        Some(offset) == self.feature_record
            || (self.feature_record_tag.is_some() && self.u16_at(offset) == self.feature_record_tag)
    }

    /// A tag opens an identity hop when a zero word and a `moFR_c` object
    /// follow it.
    fn hop_tag_at(&self, offset: usize) -> Option<u16> {
        let tag = self.u16_at(offset).filter(|tag| Self::is_tag(*tag))?;
        (self.u16_at(offset.checked_add(2)?) == Some(0)
            && self.feature_record_at(offset.checked_add(4)?))
        .then_some(tag)
    }

    /// Class map indices rise in declaration order, so the sorted hop tags
    /// name the declared identity classes in order when both sets have the
    /// same size.
    fn hop_classes(&self) -> Option<BTreeMap<u16, &'a [u8]>> {
        let tags = (0..self.archive.len())
            .filter_map(|offset| self.hop_tag_at(offset))
            .collect::<BTreeSet<_>>();
        let declared = self.declarations(|class| class.ends_with(IDENTITY_CLASS_SUFFIX));
        (tags.len() == declared.len()).then(|| {
            tags.into_iter()
                .zip(declared.into_iter().map(|(_, class)| class))
                .collect()
        })
    }

    fn declarations(&self, select: impl Fn(&[u8]) -> bool) -> Vec<(usize, &'a [u8])> {
        let mut found = Vec::new();
        let mut next = 0;
        for offset in 0..self.archive.len() {
            if offset < next {
                continue;
            }
            if let Some(class) = declared_class(self.archive, offset) {
                if select(class) {
                    found.push((offset, class));
                }
                next = offset
                    .checked_add(6)
                    .and_then(|at| at.checked_add(class.len()))
                    .unwrap_or(self.archive.len());
            }
        }
        found
    }

    /// The tags of tagged `moFaceRef_c` objects. Sorted reference tags name
    /// the declared reference classes in order when the counts agree;
    /// otherwise a tag names the one class whose declaration lies between the
    /// identity classes with the next lower and next higher map indices.
    fn face_reference_tags(&self, headers: &[usize]) -> BTreeSet<u16> {
        let declared = self.declarations(|class| REFERENCE_CLASSES.contains(&class));
        let tags = headers
            .iter()
            .filter(|&&offset| self.declared_reference(offset).is_none())
            .filter_map(|offset| offset.checked_sub(2).and_then(|at| self.u16_at(at)))
            .filter(|tag| Self::is_tag(*tag))
            .collect::<BTreeSet<_>>();
        if tags.len() == declared.len() {
            return tags
                .into_iter()
                .zip(&declared)
                .filter(|(_, (_, class))| *class == FACE_REFERENCE_CLASS)
                .map(|(tag, _)| tag)
                .collect();
        }
        let anchors = self.anchors();
        tags.into_iter()
            .filter(|tag| {
                let index = tag & !OBJECT_TAG;
                let mut feasible = declared.iter().filter(|(offset, _)| {
                    let below = anchors
                        .iter()
                        .filter(|(at, _)| at < offset)
                        .map(|(_, index)| *index)
                        .max();
                    let above = anchors
                        .iter()
                        .filter(|(at, _)| at > offset)
                        .map(|(_, index)| *index)
                        .min();
                    below.is_none_or(|below| below < index)
                        && above.is_none_or(|above| index < above)
                });
                matches!(
                    (feasible.next(), feasible.next()),
                    (Some((_, class)), None) if *class == FACE_REFERENCE_CLASS
                )
            })
            .collect()
    }

    /// Declaration offsets with known class map indices.
    fn anchors(&self) -> Vec<(usize, u16)> {
        let declared = self.declarations(|class| class.ends_with(IDENTITY_CLASS_SUFFIX));
        let mut anchors = self
            .hop_classes
            .keys()
            .zip(declared)
            .map(|(tag, (offset, _))| (offset, tag & !OBJECT_TAG))
            .collect::<Vec<_>>();
        if let (Some(offset), Some(tag)) = (self.feature_record, self.feature_record_tag) {
            anchors.push((offset, tag & !OBJECT_TAG));
        }
        anchors
    }

    fn declared_reference(&self, header: usize) -> Option<&'a [u8]> {
        REFERENCE_CLASSES.iter().copied().find(|class| {
            class
                .len()
                .checked_add(6)
                .and_then(|length| header.checked_sub(length))
                .and_then(|start| declared_class(self.archive, start))
                == Some(class)
        })
    }

    fn is_face_reference(&self, header: usize) -> bool {
        match self.declared_reference(header) {
            Some(class) => class == FACE_REFERENCE_CLASS,
            None => header
                .checked_sub(2)
                .and_then(|at| self.u16_at(at))
                .is_some_and(|tag| self.face_references.contains(&tag)),
        }
    }

    /// The identity that the first hop of the face reference at `header`
    /// names.
    fn face(&self, header_offset: usize) -> Option<FaceIdentity> {
        if !self.is_face_reference(header_offset) {
            return None;
        }
        let hops = self.u32_at(header_offset.checked_add(header::HOP_COUNT)?)?;
        let entry = header_offset.checked_add(header::LEN)?;
        let (class, body) = match self.u16_at(entry)? {
            CLASS_DECLARATION => {
                let class = declared_class(self.archive, entry)?;
                (class, entry.checked_add(6)?.checked_add(class.len())?)
            }
            tag if Self::is_tag(tag) => (*self.hop_classes.get(&tag)?, entry.checked_add(2)?),
            _ => return None,
        };
        let words = own_words(class)?;
        if self.u16_at(body)? != 0 {
            return None;
        }
        let record = body.checked_add(2)?;
        let document = if Some(record) == self.feature_record {
            record
                .checked_add(6)?
                .checked_add(FEATURE_RECORD_CLASS.len())?
        } else if self.u16_at(record) == self.feature_record_tag {
            record.checked_add(2)?
        } else {
            return None;
        };
        let mut identities = self
            .after_document(document)
            .into_iter()
            .filter_map(|identity| self.closed_identity(identity, words, hops));
        let identity = identities.next()?;
        identities
            .all(|other| other == identity)
            .then_some(identity)
    }

    /// The identity a hop's feature record and own words name when the next
    /// entry, or the reference trailer after the last hop, follows them.
    fn closed_identity(&self, identity: usize, words: usize, hops: u32) -> Option<FaceIdentity> {
        let feature = self.u32_at(identity).filter(|feature| *feature != 0)?;
        let own = identity.checked_add(FEATURE_RECORD_WORDS)?;
        let values = (0..words)
            .map(|word| self.u32_at(own.checked_add(word.checked_mul(4)?)?))
            .collect::<Option<Vec<_>>>()?;
        let next = own.checked_add(words.checked_mul(4)?)?;
        let closed = if hops == 1 {
            self.archive.get(next..next.checked_add(ENTRY_TRAILER)?)
                == Some(&[0u8; ENTRY_TRAILER][..])
        } else {
            self.entry_starts_at(next)
        };
        let (&local, tail) = values.split_first()?;
        closed.then(|| FaceIdentity {
            feature,
            local,
            tail: tail.to_vec(),
        })
    }

    fn entry_starts_at(&self, offset: usize) -> bool {
        match self.u16_at(offset) {
            Some(0) => true,
            Some(CLASS_DECLARATION) => declared_class(self.archive, offset)
                .is_some_and(|class| class.ends_with(IDENTITY_CLASS_SUFFIX)),
            Some(value) if value & OBJECT_TAG == 0 => true,
            Some(_) => self.hop_tag_at(offset).is_some(),
            None => false,
        }
    }

    /// The offsets after a `moFR_c` object's document pointer. A document
    /// written in place holds one to three Unicode strings, then 17 or 18
    /// bytes, its configuration string, and 12 bytes (open item EV-14), so
    /// each gap that frames a configuration string gives a candidate.
    fn after_document(&self, pointer: usize) -> Vec<usize> {
        self.document_candidates(pointer).unwrap_or_default()
    }

    fn document_candidates(&self, pointer: usize) -> Option<Vec<usize>> {
        let value = self.u16_at(pointer)?;
        let body = if Some(pointer) == self.document {
            pointer.checked_add(6)?.checked_add(DOCUMENT_CLASS.len())?
        } else if Self::is_tag(value) {
            pointer.checked_add(2)?
        } else if value != 0 {
            return Some(vec![pointer.checked_add(2)?]);
        } else {
            return None;
        };
        let mut cursor = self.after_string_handle(body)?;
        cursor = self.after_string_handle(cursor)?;
        if self.u16_at(cursor)? != DOCUMENT_FORM {
            return None;
        }
        cursor = cursor.checked_add(7)?;
        let mut strings = false;
        for _ in 0..DOCUMENT_STRINGS {
            let Some(text) = framed_text(self.archive, cursor) else {
                break;
            };
            cursor = text.end;
            strings = true;
        }
        if !strings {
            return None;
        }
        let mut candidates = Vec::new();
        for gap in DOCUMENT_BYTES_BEFORE_CONFIGURATION {
            let Some(configuration) = cursor
                .checked_add(gap)
                .and_then(|at| framed_text(self.archive, at))
            else {
                continue;
            };
            candidates.extend(
                configuration
                    .end
                    .checked_add(DOCUMENT_BYTES_AFTER_CONFIGURATION),
            );
        }
        Some(candidates)
    }

    fn after_string_handle(&self, pointer: usize) -> Option<usize> {
        let value = self.u16_at(pointer)?;
        let text = if Some(pointer) == self.string_handle {
            pointer
                .checked_add(6)?
                .checked_add(STRING_HANDLE_CLASS.len())?
        } else if Self::is_tag(value) {
            pointer.checked_add(2)?
        } else if value != 0 {
            return pointer.checked_add(2);
        } else {
            return None;
        };
        Some(framed_text(self.archive, text)?.end)
    }
}

#[cfg(test)]
pub(super) mod tests;
