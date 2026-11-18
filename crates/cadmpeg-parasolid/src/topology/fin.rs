// SPDX-License-Identifier: Apache-2.0
//! The two forms of a FIN node (type 17).
//!
//! An edge FIN has sense `+` or `-` and bounds one EDGE. A vertex-loop FIN
//! has sense `?`: it is the single FIN of a LOOP that bounds a face at one
//! vertex, its forward and backward links name the FIN itself, and its
//! partner and EDGE references are null.

use cadmpeg_ir::topology::Sense;

/// The FIN ring, partner, and EDGE references that select a FIN form.
///
/// The host maps its own null convention: `other_fin` and `edge` are `None`
/// for a null reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinLinks {
    /// Index of the FIN node itself.
    pub fin: u32,
    /// `forward_fin` reference.
    pub forward_fin: u32,
    /// `backward_fin` reference.
    pub backward_fin: u32,
    /// `other_fin` reference.
    pub other_fin: Option<u32>,
    /// `edge` reference.
    pub edge: Option<u32>,
}

/// The form of one FIN node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinForm {
    /// A FIN that bounds an EDGE, with its sense relative to that EDGE.
    Edge(Sense),
    /// The single FIN of a vertex loop.
    VertexLoop,
}

impl FinForm {
    /// The form that the `sense` byte and the references `links` state.
    ///
    /// Sense `+` or `-` gives an edge FIN. Sense `?` gives a vertex-loop FIN
    /// when both ring links name the FIN itself and the partner and EDGE
    /// references are null. Every other combination gives `None`.
    #[must_use]
    pub fn classify(links: &FinLinks, sense: u8) -> Option<Self> {
        match sense {
            b'+' => Some(Self::Edge(Sense::Forward)),
            b'-' => Some(Self::Edge(Sense::Reversed)),
            b'?' => (links.forward_fin == links.fin
                && links.backward_fin == links.fin
                && links.other_fin.is_none()
                && links.edge.is_none())
            .then_some(Self::VertexLoop),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FinForm, FinLinks};
    use cadmpeg_ir::topology::Sense;

    const VERTEX_LOOP: FinLinks = FinLinks {
        fin: 49,
        forward_fin: 49,
        backward_fin: 49,
        other_fin: None,
        edge: None,
    };

    const EDGE_FIN: FinLinks = FinLinks {
        fin: 49,
        forward_fin: 51,
        backward_fin: 53,
        other_fin: Some(55),
        edge: Some(57),
    };

    #[test]
    fn edge_senses_give_edge_fins_for_any_links() {
        for links in [EDGE_FIN, VERTEX_LOOP] {
            assert_eq!(
                FinForm::classify(&links, b'+'),
                Some(FinForm::Edge(Sense::Forward))
            );
            assert_eq!(
                FinForm::classify(&links, b'-'),
                Some(FinForm::Edge(Sense::Reversed))
            );
        }
    }

    #[test]
    fn query_sense_with_self_links_and_null_partner_and_edge_is_a_vertex_loop() {
        assert_eq!(
            FinForm::classify(&VERTEX_LOOP, b'?'),
            Some(FinForm::VertexLoop)
        );
    }

    #[test]
    fn query_sense_of_any_other_form_is_refused() {
        let variants = [
            FinLinks {
                forward_fin: 51,
                ..VERTEX_LOOP
            },
            FinLinks {
                backward_fin: 51,
                ..VERTEX_LOOP
            },
            FinLinks {
                other_fin: Some(55),
                ..VERTEX_LOOP
            },
            FinLinks {
                edge: Some(57),
                ..VERTEX_LOOP
            },
            EDGE_FIN,
        ];
        for links in variants {
            assert_eq!(FinForm::classify(&links, b'?'), None, "{links:?}");
        }
    }

    #[test]
    fn other_sense_bytes_are_refused() {
        for sense in [0, b'0', b'1', b'R', b' '] {
            assert_eq!(FinForm::classify(&VERTEX_LOOP, sense), None);
            assert_eq!(FinForm::classify(&EDGE_FIN, sense), None);
        }
    }
}
