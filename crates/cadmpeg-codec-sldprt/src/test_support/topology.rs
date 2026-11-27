// SPDX-License-Identifier: Apache-2.0
//! The cause texts of the topology give-ups that tests count.

/// The cause of the give-up of a face that names no loop.
pub(crate) const FACE_WITHHELD: &[&str] =
    &["the face names no loop and does not cover a whole closed surface"];
/// The cause of the give-up of a vertexless ring edge.
pub(crate) const RING_EDGE_WITHHELD: &[&str] =
    &["the vertexless ring edge fails reciprocal closed-carrier single-loop admission"];
/// The causes of the give-ups of an edge or a loop for an unresolved FIN
/// partner.
pub(crate) const FIN_PARTNER_UNRESOLVED: &[&str] = &[
    "the FIN partners of the edge do not corroborate its ends",
    "an edge of the loop has no canonical FIN the decode reads",
];
