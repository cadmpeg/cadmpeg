// SPDX-License-Identifier: Apache-2.0
//! The element grammar of every expanded PVS stream (§3.4).

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::token;

/// Bit 7 of a descriptor: a child list follows the payload.
const CHILD_LIST: u8 = 0x80;

/// One element: its family, descriptor, optional type, payload, and child
/// list.
#[derive(Debug)]
pub(crate) struct Element<'a> {
    pub(crate) family: u8,
    pub(crate) descriptor: u8,
    /// The type field, when the descriptor selects one.
    pub(crate) kind: Option<u16>,
    pub(crate) payload: View<'a>,
    /// The child list, when the descriptor selects one.
    pub(crate) children: Option<Vec<Element<'a>>>,
}

impl<'a> Element<'a> {
    /// Reads the one element that fills `stream` exactly.
    pub(crate) fn read_stream(
        ctx: &DecodeContext<'_>,
        mut stream: View<'a>,
    ) -> Result<Self, CodecError> {
        let element = read_element(ctx, &mut stream)?.ok_or_else(|| {
            CodecError::malformed(format_args!(
                "PVS stream at offset {} holds a child-list terminator, not an element",
                stream.start()
            ))
        })?;
        if !stream.is_empty() {
            return Err(CodecError::malformed(format_args!(
                "PVS stream leaves {} bytes after its root element",
                stream.remaining()
            )));
        }
        Ok(element)
    }

    /// The child list, empty for a descriptor without one.
    pub(crate) fn children(&self) -> &[Element<'a>] {
        self.children.as_deref().unwrap_or_default()
    }

    /// Whether the element is `family` with `descriptor` and type `kind`.
    pub(crate) fn is(&self, family: u8, descriptor: u8, kind: Option<u16>) -> bool {
        self.family == family && self.descriptor == descriptor && self.kind == kind
    }
}

/// The optional parts one descriptor selects.
struct Descriptor {
    length_width: usize,
    type_width: usize,
    children: bool,
}

/// The parts of a defined descriptor value.
fn descriptor(value: u8) -> Option<Descriptor> {
    let (length_width, type_width) = match value {
        token::DESC_U8_LENGTH | token::DESC_U8_LENGTH_CHILDREN => (1, 0),
        token::DESC_U16_LENGTH => (2, 0),
        token::DESC_U8_LENGTH_U8_TYPE | token::DESC_U8_LENGTH_U8_TYPE_CHILDREN => (1, 1),
        token::DESC_U8_LENGTH_U16_TYPE => (1, 2),
        token::DESC_CHILDREN => (0, 0),
        _ => return None,
    };
    Some(Descriptor {
        length_width,
        type_width,
        children: value & CHILD_LIST != 0,
    })
}

/// Reads one element, or `None` at the `00 00` that closes a child list.
fn read_element<'a>(
    ctx: &DecodeContext<'_>,
    view: &mut View<'a>,
) -> Result<Option<Element<'a>>, CodecError> {
    let start = view.position();
    let family = view.req_u8()?;
    let descriptor_byte = view.req_u8()?;
    if family == 0 && descriptor_byte == 0 {
        return Ok(None);
    }
    let parts = descriptor(descriptor_byte).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "PVS element at offset {start} has undefined descriptor {descriptor_byte:#04x}"
        ))
    })?;
    let length = match parts.length_width {
        0 => 0,
        1 => usize::from(view.req_u8()?),
        _ => usize::from(view.req_u16_le()?),
    };
    let kind = match parts.type_width {
        0 => None,
        1 => Some(u16::from(view.req_u8()?)),
        _ => Some(view.req_u16_le()?),
    };
    let payload = view.req_take_child(length)?;
    let children = if parts.children {
        let _depth = ctx.enter_nested("pvz_pvs_element")?;
        let mut children = Vec::new();
        while let Some(child) = read_element(ctx, view)? {
            children.push(child);
        }
        Some(children)
    } else {
        None
    };
    Ok(Some(Element {
        family,
        descriptor: descriptor_byte,
        kind,
        payload,
        children,
    }))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::View;
    use cadmpeg_core::CodecError;

    use super::Element;
    use crate::test_support::pvs::Node;

    /// Family, descriptor, type, payload, and child count of one element.
    type Parts = (u8, u8, Option<u16>, Vec<u8>, usize);

    fn read(bytes: &[u8]) -> Result<Parts, CodecError> {
        let ctx = cadmpeg_test_support::service_decode_context();
        Element::read_stream(&ctx, View::over_retained(bytes)).map(|element| {
            (
                element.family,
                element.descriptor,
                element.kind,
                element.payload.window().to_vec(),
                element.children().len(),
            )
        })
    }

    #[test]
    fn each_defined_descriptor_selects_its_length_type_and_children() {
        assert_eq!(
            read(&[0x01, 0x02, 2, 7, 8]).unwrap(),
            (1, 2, None, vec![7, 8], 0)
        );
        assert_eq!(
            read(&[0x01, 0x04, 1, 0, 9]).unwrap(),
            (1, 4, None, vec![9], 0)
        );
        assert_eq!(
            read(&[0x23, 0x12, 1, 0x01, 5]).unwrap(),
            (0x23, 0x12, Some(1), vec![5], 0)
        );
        assert_eq!(
            read(&[0x21, 0x22, 0, 0x08, 0x02]).unwrap(),
            (0x21, 0x22, Some(0x0208), vec![], 0)
        );
        assert_eq!(
            read(&[0x02, 0x80, 0x20, 0x02, 0, 0x20, 0x02, 0, 0, 0]).unwrap(),
            (2, 0x80, None, vec![], 2)
        );
        assert_eq!(
            read(&[0x20, 0x82, 1, 6, 0, 0]).unwrap(),
            (0x20, 0x82, None, vec![6], 0)
        );
        assert_eq!(
            read(&[0x20, 0x92, 1, 0x1e, 6, 0x30, 0x02, 0, 0, 0]).unwrap(),
            (0x20, 0x92, Some(0x1e), vec![6], 1)
        );
    }

    #[test]
    fn nested_child_lists_close_at_their_own_terminator() {
        let tree = Node::list(
            0x02,
            vec![Node::parent(
                0x20,
                vec![0],
                vec![Node::leaf(0x23, vec![0]), Node::leaf(0x21, vec![1])],
            )],
        )
        .encode();
        let ctx = cadmpeg_test_support::service_decode_context();
        let root = Element::read_stream(&ctx, View::over_retained(&tree)).unwrap();
        let definition = &root.children()[0];
        assert_eq!(definition.family, 0x20);
        let families: Vec<_> = definition.children().iter().map(|c| c.family).collect();
        assert_eq!(families, [0x23, 0x21]);
        assert!(definition.children()[0].children.is_none());
    }

    #[test]
    fn undefined_descriptors_terminators_and_trailing_bytes_are_refused() {
        for bytes in [
            &[0x01, 0x06, 0][..],
            &[0x01, 0x84, 0, 0],
            &[0x00, 0x00],
            &[0x01, 0x02, 0, 0],
            &[0x01, 0x02, 3, 0],
            &[0x02, 0x80, 0x20, 0x02, 0],
        ] {
            assert!(read(bytes).is_err(), "{bytes:02x?}");
        }
    }
}
