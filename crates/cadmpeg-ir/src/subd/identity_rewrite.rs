// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{SubdCage, SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdGripDirection, SubdGripWedge, SubdPlaneFrame, SubdRadialMapSelector, SubdRadialSymmetry, SubdRadialSymmetryMap, SubdScheme, SubdSecondaryGrip, SubdSurface, SubdSymmetry, SubdSymmetryKind, SubdVertex, SubdVertexGripLayout, SubdVertexTag};

rewrite_record!(SubdCage, []; {vertices, edges, faces, symmetries});
rewrite_record!(SubdEdge, []; {vertices, sharpness, tag, knot_interval, sector_coefficients});
rewrite_scalar!(SubdEdgeTag);
rewrite_scalar!(SubdEdgeUse);
rewrite_record!(SubdFace, []; {edges});
rewrite_scalar!(SubdGripDirection);
rewrite_enum!(SubdGripWedge, []; {
    Phantom {},
    Slot {edge, sector_face, spokes, sectors},
});
rewrite_scalar!(SubdPlaneFrame);
rewrite_scalar!(SubdRadialMapSelector);
rewrite_record!(SubdRadialSymmetry, []; {segments, sweep, radial_maps});
rewrite_record!(SubdRadialSymmetryMap, []; {selector, pairs});
rewrite_scalar!(SubdScheme);
rewrite_record!(SubdSecondaryGrip, []; {source_index, point, weight});
rewrite_record!(SubdSurface, []; {id, scheme, cage, source_object});
rewrite_record!(SubdSymmetry, []; {kind, plane, face_pairs, edge_pairs, vertex_pairs});
rewrite_enum!(SubdSymmetryKind, []; {
    Correspondence {},
    Radial(field0),
});
rewrite_record!(SubdVertex, []; {point, tag, secondary_grips});
rewrite_record!(SubdVertexGripLayout, []; {direction, wedges});
rewrite_scalar!(SubdVertexTag);
