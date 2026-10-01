// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{PolygonalSurface, PolylineCurve, PolylineSamples, PolylineVertex};

rewrite_record!(PolygonalSurface, []; {vertices, triangles, chordal_deflection});
rewrite_record!(PolylineCurve, []; {samples, chordal_deflection});
rewrite_enum!(PolylineSamples<R, P>, [R, P]; {
    Unparameterized {points},
    Parameterized {vertices},
});
rewrite_record!(PolylineVertex<R, P>, [R, P]; {parameter, point});
