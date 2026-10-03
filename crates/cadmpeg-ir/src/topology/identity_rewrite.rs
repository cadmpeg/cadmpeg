// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    AnchoredVertexUse, Body, BodyKind, Coedge, CoedgeUseCurve, Color, Edge, EdgeCarrier, Face,
    FaceLoops, IncreasingParameterInterval, Loop, LoopBoundary, LoopRing, ParameterInterval,
    PcurveUse, Point, Region, Sense, Shell, ShellMembers, Vertex,
};

rewrite_record!(AnchoredVertexUse, []; {vertex, after, pcurves});
rewrite_record!(Body, []; {id, kind, regions, transform, name, color, visible});
rewrite_scalar!(BodyKind);
rewrite_record!(Coedge, []; {id, owner_loop, edge, radial_next, sense, pcurves, use_curve});
rewrite_record!(CoedgeUseCurve, []; {curve, parameter_range});
rewrite_scalar!(Color);
rewrite_record!(Edge, []; {id, carrier, start, end, tolerance});
rewrite_enum!(EdgeCarrier, []; {
    Free,
    Endpoints(field0),
    Curve(field0),
    Bounded(field0, field1),
});
rewrite_record!(Face, []; {id, shell, surface, sense, loops, name, color, tolerance});
rewrite_enum!(FaceLoops, []; {
    Unspecified {loops},
    Classified {outer, inner},
});
rewrite_scalar!(IncreasingParameterInterval);
rewrite_record!(Loop, []; {id, face, boundary});
rewrite_enum!(LoopBoundary, []; {
    Vertex {vertex, pcurves},
    Ring(field0),
});
rewrite_record!(LoopRing, []; {coedges, vertex_uses});
rewrite_scalar!(ParameterInterval);
rewrite_record!(PcurveUse, []; {pcurve, isoparametric, parameter_range});
rewrite_record!(Point, []; {id, position, source_object});
rewrite_record!(Region, []; {id, body, shells});
rewrite_scalar!(Sense);
rewrite_record!(Shell, []; {id, region, members});
rewrite_record!(ShellMembers, []; {faces, wire_edges, free_vertices});
rewrite_record!(Vertex, []; {id, point, tolerance});
