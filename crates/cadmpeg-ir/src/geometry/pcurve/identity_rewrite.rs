// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    CirclePcurve, EllipsePcurve, HarmonicPcurve, HyperbolaPcurve, HyperbolicPcurve, LinePcurve,
    OffsetPcurve, ParabolaPcurve, Pcurve, PcurveGeneralForm, PcurveGeometry, PcurveInlineForm,
    PcurveMetadata, PcurveNurbs, PcurveNurbsPoles, PlacedPcurve, PolarHarmonicPcurve,
    PolarNurbsPole, PolarNurbsPoles, PolarPcurveNurbs, SphericalGreatCirclePcurve, TrimmedPcurve,
    WeightedPolarNurbsPole, WeightedPole2,
};

rewrite_scalar!(CirclePcurve);
rewrite_scalar!(EllipsePcurve);
rewrite_scalar!(HarmonicPcurve);
rewrite_scalar!(HyperbolaPcurve);
rewrite_scalar!(HyperbolicPcurve);
rewrite_scalar!(LinePcurve);
rewrite_record!(OffsetPcurve, []; {distance, basis, depth});
rewrite_scalar!(ParabolaPcurve);
rewrite_record!(Pcurve, []; {id, geometry, metadata});
rewrite_record!(PcurveGeneralForm, []; {wrapper_reversed, parameter_range, fit_tolerance});
rewrite_enum!(PcurveGeometry, []; {
    Line(field0),
    PolarHarmonic(field0),
    PolarNurbs {nurbs},
    SphericalGreatCircle(field0),
    Circle(field0),
    Ellipse(field0),
    Harmonic(field0),
    Parabola(field0),
    Hyperbola(field0),
    Hyperbolic(field0),
    Nurbs {nurbs},
    Transformed(field0),
    Trimmed(field0),
    Offset(field0),
});
rewrite_record!(PcurveInlineForm, []; {wrapper_reversed, native_tail_flags, parameter_range, fit_tolerance});
rewrite_enum!(PcurveMetadata, []; {
    AsmInline {form},
    General {form},
});
rewrite_record!(PcurveNurbs, []; {degree, knots, poles, periodic});
rewrite_enum!(PcurveNurbsPoles<P>, [P]; {
    Polynomial {points},
    Rational {points},
});
rewrite_record!(PlacedPcurve, []; {basis, transform, depth});
rewrite_scalar!(PolarHarmonicPcurve);
rewrite_record!(PolarNurbsPole<P, S>, [P, S]; {radial, axial});
rewrite_enum!(PolarNurbsPoles<P, S>, [P, S]; {
    Polynomial {poles},
    Rational {poles},
});
rewrite_record!(PolarPcurveNurbs, []; {degree, knots, poles, periodic});
rewrite_scalar!(SphericalGreatCirclePcurve);
rewrite_record!(TrimmedPcurve, []; {parameter_range, same_sense, basis, depth});
rewrite_record!(WeightedPolarNurbsPole<P, S>, [P, S]; {radial, axial, weight});
rewrite_record!(WeightedPole2<P>, [P]; {point, weight});
