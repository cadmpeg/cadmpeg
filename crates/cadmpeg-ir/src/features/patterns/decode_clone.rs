// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::curve_driven::CurveDrivenPatternParameters;
use super::{
    CompositePattern, LinearPatternDirection, PatternKind, PatternScaleCenter, PatternSeed,
    PatternStage, PatternTransform,
};

rewrite_record!(CurveDrivenPatternParameters, []; { path, count, spacing, distance, total_extent, reversed, path_oriented, start_parameter });

rewrite_record!(CompositePattern, []; (field0));

rewrite_record!(LinearPatternDirection, []; { direction, spacing, count });

rewrite_record!(PatternKind<C>, [C]; (field0));

rewrite_enum!(PatternScaleCenter, []; {
    FirstSeedCentroid,
    Point(field0),
    Native(field0),
});

rewrite_enum!(PatternSeed, []; {
    Native(field0),
    Feature(field0),
    Faces(field0),
    Bodies(field0),
    Occurrences(field0),
});

rewrite_record!(PatternStage, []; { pattern });

rewrite_enum!(PatternTransform<C>, [C]; {
    CurveDrivenPartial { parameters },
    Unresolved { form },
    Linear { direction, spacing, count, second, omitted_instances },
    LinearOffsets { direction, offsets },
    Circular { axis_origin, axis_dir, angle, count },
    CircularAngles { axis_origin, axis_dir, angles },
    CurveDriven { path, spacing, count },
    Mirror { plane_origin, plane_normal },
    MirrorReference { plane },
    Scale { center, final_factor, count },
    Composite { stages },
});
