// SPDX-License-Identifier: Apache-2.0
//! Edge selections (`docs/formats/solidedge.md` §7.6): a 559-byte
//! `CLSID_JMTopologyProxy` selects one straight edge, stated as a pick whose
//! witness holds the edge end points.
//!
//! Stored lengths are metres; the design states millimetres.

use cadmpeg_core::decode::View;
use cadmpeg_ir::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection, Witness,
};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

use crate::layout::{topology_proxy_edge, topology_proxy_edge_tail};
use crate::psm::Object;

/// Millimetres per metre.
const MM_PER_M: f64 = 1000.0;
/// Bytes of one `f64`.
const REAL: usize = 8;
/// Bytes of a topology proxy that stores one straight edge (§7.6).
const EDGE_PROXY_LEN: usize = 559;
/// A stored edge direction has unit length within this distance.
const EPS_DIRECTION_UNIT: f64 = 1e-6;

/// The straight edge a proxy stores, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EdgeLine {
    pub(crate) start: Vector3,
    pub(crate) direction: Vector3,
    pub(crate) length: f64,
}

impl EdgeLine {
    /// The end point: the start advanced by the length along the direction.
    pub(crate) fn end(&self) -> Vector3 {
        Vector3::new(
            self.start.x + self.direction.x * self.length,
            self.start.y + self.direction.y * self.length,
            self.start.z + self.direction.z * self.length,
        )
    }
}

/// Three consecutive `f64` at `at`.
fn vector(bytes: &[u8], at: usize) -> Option<Vector3> {
    let real = |index: usize| {
        index
            .checked_mul(REAL)
            .and_then(|offset| at.checked_add(offset))
            .and_then(|offset| View::f64_le_at(bytes, offset))
            .filter(|value| value.is_finite())
    };
    Some(Vector3::new(real(0)?, real(1)?, real(2)?))
}

/// The edge line stored at the given offsets of `bytes`, in millimetres.
fn read_edge(bytes: &[u8], start: usize, direction: usize, length: usize) -> Option<EdgeLine> {
    let start = vector(bytes, start)?;
    let direction = vector(bytes, direction)?;
    let length = View::f64_le_at(bytes, length)?;
    if (direction.norm() - 1.0).abs() > EPS_DIRECTION_UNIT || !length.is_finite() || length <= 0.0 {
        return None;
    }
    Some(EdgeLine {
        start: Vector3::new(start.x * MM_PER_M, start.y * MM_PER_M, start.z * MM_PER_M),
        direction,
        length: length * MM_PER_M,
    })
}

/// The edge line of the bytes of a topology proxy. `None` where the object
/// is not 559 bytes, a value is not finite, the direction is not a unit
/// vector, or the length is not positive.
pub(crate) fn edge_line(bytes: &[u8]) -> Option<EdgeLine> {
    if bytes.len() != EDGE_PROXY_LEN {
        return None;
    }
    read_edge(
        bytes,
        topology_proxy_edge::START,
        topology_proxy_edge::DIRECTION,
        topology_proxy_edge::LENGTH,
    )
}

/// The edge line of a topology proxy of any size that holds the edge fields
/// at the end of the object (§7.6). `None` where the proxy is shorter than
/// those fields or a value does not read as in [`edge_line`].
pub(crate) fn edge_line_from_end(bytes: &[u8]) -> Option<EdgeLine> {
    let base = bytes.len().checked_sub(topology_proxy_edge_tail::LEN)?;
    read_edge(
        bytes,
        base.checked_add(topology_proxy_edge_tail::START)?,
        base.checked_add(topology_proxy_edge_tail::DIRECTION)?,
        base.checked_add(topology_proxy_edge_tail::LENGTH)?,
    )
}

fn point(at: Vector3) -> Option<FinitePoint3> {
    FinitePoint3::new(Point3::new(at.x, at.y, at.z))
}

/// The selection of the edge a 559-byte proxy object stores: one
/// native pick whose witness holds the edge end points and direction.
/// `Picks::Unread` where the object does not store a straight edge.
pub(crate) fn edge_selection<O: MemberOrder>(proxy: &Object<'_>) -> (Selection<O>, bool) {
    selection_of(edge_line(proxy.bytes))
}

/// [`edge_selection`] for a proxy of any size that holds the edge fields at
/// the end of the object.
pub(crate) fn edge_selection_from_end<O: MemberOrder>(proxy: &Object<'_>) -> (Selection<O>, bool) {
    selection_of(edge_line_from_end(proxy.bytes))
}

fn selection_of<O: MemberOrder>(line: Option<EdgeLine>) -> (Selection<O>, bool) {
    let read = line.and_then(|line| {
        let pick = Picked {
            target: PickTarget::Native { within: None },
            context: None,
            native: None,
            domain: None,
            witness: Some(Witness {
                point: None,
                direction: UnitVector3::normalized(line.direction),
                endpoints: Some([point(line.start)?, point(line.end())?]),
                frame: None,
                curves: Vec::new(),
                surface: None,
                body: None,
            }),
            resolved: None,
        };
        Members::new(vec![pick]).ok()
    });
    let stated = read.is_some();
    let picks = read.map_or(Picks::Unread, Picks::Named);
    (
        Selection {
            at: Anchor::Input,
            picks,
            native: None,
        },
        stated,
    )
}

#[cfg(test)]
mod tests;
