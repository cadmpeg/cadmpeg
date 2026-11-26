// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    AnchoredVertexUse, Body, BodyKind, Coedge, CoedgeUseCurve, Color, Edge, EdgeCarrier, EdgeEnds,
    Face, FaceLoops, IncreasingParameterInterval, Loop, LoopBoundary, LoopRing, ParameterInterval,
    PcurveUse, Point, Region, RegionUnstated, Sense, Shell, ShellMember, Unstated, Vertex,
};

rewrite_record!(AnchoredVertexUse, []; {vertex, after, pcurves});
rewrite_record!(Body, []; {id, kind, transform, name, color, visible, unstated, native_ref});
rewrite_scalar!(BodyKind);
rewrite_record!(Coedge, []; {id, owner_loop, edge, radial_next, sense, pcurves, use_curve, native_ref});
rewrite_record!(CoedgeUseCurve, []; {curve, parameter_range});
rewrite_scalar!(Color);
rewrite_record!(Edge, []; {id, carrier, ends, tolerance, native_ref});
rewrite_record!(EdgeEnds, []; {start, end});
rewrite_enum!(EdgeCarrier, []; {
    Free,
    Endpoints(field0),
    Curve(field0),
    Bounded(field0, field1),
});
rewrite_record!(Face, []; {id, shell, surface, sense, loops, name, color, tolerance, native_ref});
rewrite_enum!(FaceLoops, []; {
    Unspecified {loops},
    Classified {outer, inner},
    PartialUnspecified {loops},
    PartialClassified {outer, inner},
});
rewrite_scalar!(IncreasingParameterInterval);
rewrite_record!(Loop, []; {id, face, boundary, native_ref});
rewrite_enum!(LoopBoundary, []; {
    Vertex {vertex, pcurves},
    Ring(field0),
});
rewrite_record!(LoopRing, []; {coedges, vertex_uses});
rewrite_scalar!(ParameterInterval);
rewrite_record!(PcurveUse, []; {pcurve, isoparametric, parameter_range});
rewrite_record!(Point, []; {id, position, source_object, native_ref});
rewrite_record!(Region, []; {id, body, shells, unstated, native_ref});
rewrite_enum!(RegionUnstated, []; {
    Voids,
    Exterior {voids},
    Unclassified {shells},
});
rewrite_scalar!(Sense);
rewrite_record!(Shell, []; {id, region, members, unstated, native_ref});
rewrite_scalar!(Unstated);
rewrite_enum!(ShellMember, []; {
    WireEdge {id},
    FreeVertex {id},
    Subd {id},
});
rewrite_record!(Vertex, []; {id, point, tolerance, native_ref});
