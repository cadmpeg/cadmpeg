// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    BsplineSurface, KnotVector, NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, WeightedPole3,
};

rewrite_record!(BsplineSurface, []; {u_degree, v_degree, u_knots, v_knots, control_points});
rewrite_record!(KnotVector, []; (field0));
rewrite_record!(NurbsCurve, []; {degree, knots, poles, periodic});
rewrite_enum!(NurbsPoleGrid<P>, [P]; {
    Polynomial {rows},
    Rational {rows},
});
rewrite_enum!(NurbsPoles3<P>, [P]; {
    Polynomial {points},
    Rational {points},
});
rewrite_record!(NurbsSurface, []; {u_degree, v_degree, u_knots, v_knots, poles, normal_reversed, u_periodic, v_periodic});
rewrite_record!(WeightedPole3<P>, [P]; {point, weight});
