use super::{identity_references, FaceIdentity, IdentityReference};
use crate::layout::identity_reference_header as header;

const FACE_REFERENCE_TAG: u16 = 0x8040;
const HOP_TAG: u16 = 0x8042;
const FEATURE_RECORD_TAG: u16 = 0x8044;
const STRING_HANDLE_TAG: u16 = 0x8046;
const DOCUMENT_INDEX: u16 = 0x001e;
const UNRELATED_TAG: u16 = 0x8099;
const STAMP: u32 = 0x5486_f06c;

fn declaration(class: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 0x01, 0x00];
    bytes.extend_from_slice(
        &u16::try_from(class.len())
            .expect("class length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(class.as_bytes());
    bytes
}

fn framed(text: &str) -> Vec<u8> {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut bytes = vec![
        0xff,
        0xfe,
        0xff,
        u8::try_from(units.len()).expect("short text"),
    ];
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn header(hops: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header::FORM_VALUE.to_le_bytes());
    bytes.extend_from_slice(&header::ZERO_WORD_VALUE.to_le_bytes());
    bytes.extend_from_slice(&hops.to_le_bytes());
    bytes.extend_from_slice(&[0, 2, 0, 0]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&header::MARKER_VALUE);
    bytes.extend_from_slice(&header::ZERO_TAIL_VALUE.to_le_bytes());
    assert_eq!(bytes.len(), header::LEN);
    bytes
}

/// The head of a `moExtObject_c` written in place, up to its strings.
fn document_head() -> Vec<u8> {
    let mut bytes = declaration("moExtObject_c");
    bytes.extend(declaration("moCStringHandle_c"));
    bytes.extend(framed("C:\\part.SLDPRT"));
    bytes.extend_from_slice(&STRING_HANDLE_TAG.to_le_bytes());
    bytes.extend(framed("part"));
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&STAMP.to_le_bytes());
    bytes
}

/// A `moExtObject_c` written in place with `strings` empty strings and a
/// `gap`-byte run before its configuration name.
fn document_with(strings: usize, gap: usize) -> Vec<u8> {
    let mut bytes = document_head();
    for _ in 0..strings {
        bytes.extend(framed(""));
    }
    bytes.extend(vec![0; gap]);
    bytes.extend(framed("Default"));
    bytes.extend_from_slice(&[0; 12]);
    bytes
}

fn document() -> Vec<u8> {
    document_with(2, 17)
}

fn identity_words(feature: u32, words: &[u32]) -> Vec<u8> {
    let mut bytes = feature.to_le_bytes().to_vec();
    bytes.extend_from_slice(&STAMP.to_le_bytes());
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

/// The first face reference of the archive, whose hop declares its identity
/// class, `moFR_c` and the document in place.
pub(in crate::swift) fn declared_reference(feature: u32, words: [u32; 3]) -> Vec<u8> {
    declared_reference_with(&document(), feature, words, &[])
}

/// A declared first face reference with the given in-place document; each
/// entry of `more` is a further hop entry.
fn declared_reference_with(
    document: &[u8],
    feature: u32,
    words: [u32; 3],
    more: &[&[u8]],
) -> Vec<u8> {
    let mut bytes = declaration("moFaceRef_c");
    let hops = more
        .len()
        .checked_add(1)
        .and_then(|hops| u32::try_from(hops).ok())
        .expect("hop count");
    bytes.extend(header(hops));
    bytes.extend(declaration("moEndFace3IntSurfIdRep_c"));
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend(declaration("moFR_c"));
    bytes.extend_from_slice(document);
    bytes.extend(identity_words(feature, &words));
    for entry in more {
        bytes.extend_from_slice(entry);
    }
    bytes.extend_from_slice(&[0; 8]);
    bytes
}

/// A later face reference whose hop and `moFR_c` use class tags and whose
/// document is a back-reference.
pub(in crate::swift) fn tagged_reference(
    feature: u32,
    words: [u32; 3],
    next: Option<&[u8]>,
) -> Vec<u8> {
    let mut bytes = FACE_REFERENCE_TAG.to_le_bytes().to_vec();
    bytes.extend(header(if next.is_some() { 2 } else { 1 }));
    bytes.extend_from_slice(&HOP_TAG.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&FEATURE_RECORD_TAG.to_le_bytes());
    bytes.extend_from_slice(&DOCUMENT_INDEX.to_le_bytes());
    bytes.extend(identity_words(feature, &words));
    match next {
        Some(entry) => bytes.extend_from_slice(entry),
        None => bytes.extend_from_slice(&[0; 8]),
    }
    bytes
}

fn archive(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = 11000u32.to_le_bytes().to_vec();
    for part in parts {
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(part);
    }
    bytes
}

fn faces(bytes: &[u8]) -> Vec<Option<FaceIdentity>> {
    identity_references(bytes)
        .into_iter()
        .map(|reference| reference.face)
        .collect()
}

fn identity(feature: u32, local: u32, tail: &[u32]) -> FaceIdentity {
    FaceIdentity {
        feature,
        local,
        tail: tail.to_vec(),
    }
}

#[test]
fn first_hops_name_the_feature_local_face_and_its_path_fields() {
    let bytes = archive(&[
        declared_reference(183, [1, 0, 0]),
        tagged_reference(44, [2, 0, 5], None),
    ]);
    assert_eq!(
        faces(&bytes),
        [
            Some(identity(183, 1, &[0, 0])),
            Some(identity(44, 2, &[0, 5]))
        ]
    );
}

#[test]
fn an_identity_class_without_a_tagged_use_leaves_the_hop_classes_undetermined() {
    let bytes = archive(&[
        declared_reference(183, [1, 0, 0]),
        declaration("moFilletSurfIdRep_c"),
        tagged_reference(44, [2, 0, 5], None),
    ]);
    assert_eq!(faces(&bytes), [None, None]);
}

#[test]
fn a_hop_followed_by_no_entry_names_no_face() {
    let mut unrelated = UNRELATED_TAG.to_le_bytes().to_vec();
    unrelated.extend_from_slice(&[1, 0, 0, 0]);
    let bytes = archive(&[
        declared_reference(183, [1, 0, 0]),
        tagged_reference(44, [2, 0, 5], Some(&unrelated)),
    ]);
    assert_eq!(faces(&bytes), [Some(identity(183, 1, &[0, 0])), None]);
}

#[test]
fn an_edge_reference_names_no_face_and_class_indices_bound_the_face_tag() {
    let mut edge = declaration("moEdgeRef_c");
    edge.extend(header(1));
    edge.extend_from_slice(&HOP_TAG.to_le_bytes());
    edge.extend_from_slice(&0u16.to_le_bytes());
    edge.extend_from_slice(&FEATURE_RECORD_TAG.to_le_bytes());
    edge.extend_from_slice(&DOCUMENT_INDEX.to_le_bytes());
    edge.extend(identity_words(44, &[3, 0, 0]));
    edge.extend_from_slice(&[0; 8]);
    let bytes = archive(&[
        declared_reference(183, [1, 0, 0]),
        edge,
        tagged_reference(44, [2, 0, 5], None),
    ]);
    assert_eq!(
        faces(&bytes),
        [
            Some(identity(183, 1, &[0, 0])),
            None,
            Some(identity(44, 2, &[0, 5]))
        ]
    );
}

fn first_reading(document: &[u8], more: &[&[u8]]) -> (Option<FaceIdentity>, bool) {
    let bytes = archive(&[
        declared_reference_with(document, 183, [1, 0, 0], more),
        tagged_reference(44, [2, 0, 5], None),
    ]);
    let references = identity_references(&bytes);
    let first: &IdentityReference = references.first().expect("first reference");
    (first.face.clone(), first.assumed_document)
}

#[test]
fn a_one_string_document_with_an_eighteen_byte_gap_reads_its_identity() {
    assert_eq!(
        first_reading(&document_with(1, 18), &[]),
        (Some(identity(183, 1, &[0, 0])), true)
    );
}

#[test]
fn a_three_string_document_reads_its_identity_after_either_gap() {
    for gap in [17, 18] {
        assert_eq!(
            first_reading(&document_with(3, gap), &[]),
            (Some(identity(183, 1, &[0, 0])), false)
        );
    }
}

#[test]
fn a_two_string_document_is_read_as_an_assumed_layout() {
    assert_eq!(
        first_reading(&document(), &[]),
        (Some(identity(183, 1, &[0, 0])), true)
    );
}

/// A document whose bytes frame a configuration string after two strings and
/// after three: an empty string, then 13 bytes, an empty configuration
/// string, and the `Default` configuration string.
fn two_count_document() -> Vec<u8> {
    let mut bytes = document_head();
    bytes.extend(framed(""));
    bytes.extend(framed(""));
    bytes.extend(framed(""));
    bytes.extend_from_slice(&[0; 13]);
    bytes.extend(framed(""));
    bytes.extend(framed("Default"));
    bytes.extend_from_slice(&[0; 12]);
    bytes
}

#[test]
fn string_counts_that_close_on_different_identities_name_no_face() {
    assert_eq!(
        first_reading(&two_count_document(), &[&[0, 0]]),
        (None, false)
    );
}
