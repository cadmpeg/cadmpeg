// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    ChannelAddressing, ColorAddressing, MeshColors, Meshed, ShadedTriangle, ShadedVertex, Strip,
    Strips, Tessellation, TessellationChannel, TessellationMesh, TessellationTextureAssignment,
    TessellationTriangleGroup, TextureCoordinateAddressing, TextureCoordinates,
};

rewrite_enum!(ChannelAddressing, []; {
    Vertex {},
    Corner {indices},
    Triangle {indices},
});
rewrite_enum!(Meshed, []; {
    Body {body, faces},
    Display {definition},
    Definition {definition, name, color, visible},
    Pmi {pmi},
    Unresolved,
});
rewrite_record!(ShadedTriangle<N>, [N]; {corners, normals});
rewrite_record!(ShadedVertex<P, N>, [P, N]; {position, normal});
rewrite_record!(Strip<V>, [V]; (field0));
rewrite_record!(Strips<V>, [V]; (field0));
rewrite_record!(Tessellation, []; {id, of, state, chordal_deflection, source_object, mesh, feature_edges, triangle_groups, texture_assignments, texture_coordinates, colors, channels, native_ref});
rewrite_record!(TessellationChannel, []; {addressing, item_size, kind, flags, data, count});
rewrite_enum!(TessellationMesh<P, N>, [P, N]; {
    List {vertices, triangles},
    ShadedList {vertices, triangles},
    CornerShadedList {vertices, triangles},
    Strips {strips},
    ShadedStrips {strips},
});
rewrite_record!(TessellationTextureAssignment, []; {source_id, texture, triangles});
rewrite_record!(TessellationTriangleGroup, []; {source_id, face, triangles});
rewrite_scalar!(TextureCoordinateAddressing);
rewrite_record!(TextureCoordinates, []; {name, addressing, values});
rewrite_scalar!(ColorAddressing);
rewrite_record!(MeshColors, []; {addressing, values});
