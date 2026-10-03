// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    ChannelAddressing, ShadedTriangle, ShadedVertex, Strip, Strips, Tessellation,
    TessellationChannel, TessellationMesh, TessellationTextureAssignment,
    TessellationTriangleGroup,
};

rewrite_enum!(ChannelAddressing, []; {
    Vertex {},
    Corner {indices},
    Triangle {indices},
});
rewrite_record!(ShadedTriangle<N>, [N]; {corners, normals});
rewrite_record!(ShadedVertex<P, N>, [P, N]; {position, normal});
rewrite_record!(Strip<V>, [V]; (field0));
rewrite_record!(Strips<V>, [V]; (field0));
rewrite_record!(Tessellation, []; {id, body, faces, chordal_deflection, source_object, mesh, feature_edges, triangle_groups, texture_assignments, channels});
rewrite_record!(TessellationChannel, []; {addressing, item_size, kind, flags, data, count});
rewrite_enum!(TessellationMesh<P, N>, [P, N]; {
    List {vertices, triangles},
    ShadedList {vertices, triangles},
    CornerShadedList {vertices, triangles},
    Strips {strips},
    ShadedStrips {strips},
});
rewrite_record!(TessellationTextureAssignment, []; {source_id, texture, triangles});
rewrite_record!(TessellationTriangleGroup, []; {source_id, triangles});
