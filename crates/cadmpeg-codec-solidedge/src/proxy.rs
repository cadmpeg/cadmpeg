// SPDX-License-Identifier: Apache-2.0
//! Topology proxies (`docs/formats/solidedge.md` §7.6): the geometry record of
//! a `CLSID_JMTopologyProxy` object, read from the byte after its geometry
//! class identifier. A straight edge is stated as a pick whose witness holds
//! the edge end points.
//!
//! Stored lengths are metres; the design states millimetres.

use cadmpeg_core::decode::View;
use cadmpeg_ir::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection, Witness,
};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

use crate::extents::MM_PER_M;
use crate::psm::Object;

/// Bytes of one `f64`.
const REAL: usize = 8;
/// The last ten bytes of the geometry class identifier of a topology proxy
/// (§7.6).
const GEOMETRY_TAIL: [u8; 10] = [0xce, 0x11, 0x9a, 0x5e, 0x08, 0x00, 0x36, 0x9b, 0x41, 0x02];
/// The geometry kind byte of a straight edge (§7.6).
const GEOMETRY_LINE: u8 = 3;
/// Bytes from the geometry kind byte of a straight edge to its start, its
/// direction and its length (§7.6).
const LINE_START: usize = 1;
const LINE_DIRECTION: usize = 25;
const LINE_LENGTH: usize = 57;
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

/// The offset of the geometry record of a proxy: the byte after the geometry
/// class identifier (§7.6).
fn geometry_at(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(GEOMETRY_TAIL.len())
        .position(|window| window == GEOMETRY_TAIL)
        .map(|at| at + GEOMETRY_TAIL.len())
}

/// Three consecutive finite `f64` at `at`, in metres.
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

/// The edge line of the bytes of a topology proxy. `None` where the proxy
/// holds no straight edge, a value is not finite, the direction is not a unit
/// vector, or the length is not positive.
pub(crate) fn edge_line(bytes: &[u8]) -> Option<EdgeLine> {
    let at = geometry_at(bytes)?;
    if bytes.get(at) != Some(&GEOMETRY_LINE) {
        return None;
    }
    let start = vector(bytes, at.checked_add(LINE_START)?)?;
    let direction = vector(bytes, at.checked_add(LINE_DIRECTION)?)?;
    let length = View::f64_le_at(bytes, at.checked_add(LINE_LENGTH)?)?;
    if (direction.norm() - 1.0).abs() > EPS_DIRECTION_UNIT || !length.is_finite() || length <= 0.0 {
        return None;
    }
    Some(EdgeLine {
        start: Vector3::new(start.x * MM_PER_M, start.y * MM_PER_M, start.z * MM_PER_M),
        direction,
        length: length * MM_PER_M,
    })
}

/// The point a vertex proxy stores, in millimetres. `None` where the proxy
/// holds no geometry record or a value is not finite.
pub(crate) fn vertex_point(bytes: &[u8]) -> Option<Vector3> {
    let point = vector(bytes, geometry_at(bytes)?)?;
    Some(Vector3::new(
        point.x * MM_PER_M,
        point.y * MM_PER_M,
        point.z * MM_PER_M,
    ))
}

fn point(at: Vector3) -> Option<FinitePoint3> {
    FinitePoint3::new(Point3::new(at.x, at.y, at.z))
}

/// The selection of the edge a proxy object stores: one
/// native pick whose witness holds the edge end points and direction.
/// `Picks::Unread` where the object does not store a straight edge.
pub(crate) fn edge_selection<O: MemberOrder>(proxy: &Object<'_>) -> (Selection<O>, bool) {
    let read = edge_line(proxy.bytes).and_then(|line| {
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
