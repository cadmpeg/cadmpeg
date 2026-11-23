// SPDX-License-Identifier: Apache-2.0
//! Synthetic hem, contour flange, unfold, refold and close-corner documents
//! (`docs/formats/solidedge.md` §10.4 to §10.7).

use super::{
    feature_with_streams, part_body, profile_group, put_reals, slot_object, Member2d, Object,
    ProfileFrame, Slot, EXTENT_CLASSES, EXTENT_SET,
};

/// The class names of a kind document, by form.
const KIND_CLASSES: [&str; 18] = [
    EXTENT_CLASSES[0],
    EXTENT_CLASSES[1],
    EXTENT_CLASSES[2],
    EXTENT_CLASSES[3],
    EXTENT_CLASSES[4],
    EXTENT_CLASSES[5],
    EXTENT_CLASSES[6],
    EXTENT_CLASSES[7],
    EXTENT_CLASSES[8],
    EXTENT_CLASSES[9],
    EXTENT_CLASSES[10],
    EXTENT_CLASSES[11],
    EXTENT_CLASSES[12],
    EXTENT_CLASSES[13],
    EXTENT_CLASSES[14],
    EXTENT_CLASSES[15],
    EXTENT_CLASSES[16],
    "CLSID_JMTopologyProxy",
];
const PROXY_FORM: u16 = 17;
/// The first slot object and the edge proxy of a kind document.
const SLOT_OBJECT: u32 = 400;
const PROXY_OBJECT: u32 = 380;
/// Bytes of the slot area after the function name.
const AREA: usize = 300;
/// Bytes after the function name of the edge and profile slots.
const OBJECT_AFTER_NAME: usize = 68;

/// The straight edge of a hem proxy, in metres.
#[derive(Clone, Copy)]
pub(crate) struct EdgeSpec {
    pub(crate) start: [f64; 3],
    pub(crate) direction: [f64; 3],
    pub(crate) length: f64,
}

impl Default for EdgeSpec {
    fn default() -> Self {
        Self {
            start: [0.05, 0.06, 0.022],
            direction: [-1.0, 0.0, 0.0],
            length: 0.05,
        }
    }
}

/// What the slot at 68 bytes after the function name names.
pub(crate) enum Operand<'a> {
    /// No object.
    None,
    /// A `CLSID_JMTopologyProxy` of 559 bytes that stores a straight edge.
    Edge(EdgeSpec),
    /// A `CLSID_JMTopologyProxy` of 559 bytes that stores no edge.
    BlankEdge,
    /// A profile group.
    Profile(&'a ProfileFrame, &'a [Member2d]),
}

/// A document with one feature, object 10, of `function`. `operand` fills the
/// slot at 68 bytes after the name, and `slots` give the parameter slots as
/// (bytes after the name, object). The streams `streams` stand beside the
/// cluster.
pub(crate) fn kind_document(
    function: &str,
    operand: &Operand<'_>,
    slots: &[(usize, Slot)],
    streams: &[(&str, Vec<u8>)],
) -> Vec<u8> {
    let mut body = part_body(function);
    let end = body.len();
    body.resize(end + AREA + 4, 0);
    let mut extra = Vec::new();
    match operand {
        Operand::None => {}
        Operand::Edge(_) | Operand::BlankEdge => {
            let mut proxy = vec![0_u8; 559 - 8];
            if let Operand::Edge(edge) = operand {
                put_reals(&mut proxy, 121, &edge.start);
                put_reals(&mut proxy, 145, &edge.direction);
                put_reals(&mut proxy, 177, &[edge.length]);
            }
            extra.push(Object {
                id: PROXY_OBJECT,
                form: PROXY_FORM,
                flags: 0,
                body: proxy,
            });
            body[end + OBJECT_AFTER_NAME..end + OBJECT_AFTER_NAME + 4]
                .copy_from_slice(&PROXY_OBJECT.to_le_bytes());
        }
        Operand::Profile(frame, members) => {
            extra.extend(profile_group(EXTENT_SET, frame, 1, 200, members));
            body[end + OBJECT_AFTER_NAME..end + OBJECT_AFTER_NAME + 4]
                .copy_from_slice(&(EXTENT_SET + 1).to_le_bytes());
        }
    }
    for (index, (after, slot)) in slots.iter().enumerate() {
        let id = SLOT_OBJECT + u32::try_from(index).unwrap();
        if let Some(object) = slot_object(id, *slot) {
            body[end + after..end + after + 4].copy_from_slice(&id.to_le_bytes());
            extra.push(object);
        }
    }
    feature_with_streams(body, extra, &KIND_CLASSES, streams)
}
