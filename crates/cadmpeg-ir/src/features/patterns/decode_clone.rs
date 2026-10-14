// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::{
    CompositePattern, LinearPatternDirection, PatternKind, PatternScaleCenter, PatternSeed,
    PatternStage, PatternTransform,
};

rewrite_record!(CompositePattern, []; (field0));

rewrite_record!(LinearPatternDirection, []; { direction, spacing, count });

rewrite_record!(PatternKind<C>, [C]; (field0));

rewrite_enum!(PatternScaleCenter, []; {
    FirstSeedCentroid,
    Point(field0),
    Native(field0),
});

rewrite_enum!(PatternSeed, []; {
    Feature(field0),
    Faces(field0),
    Bodies(field0),
    Occurrences(field0),
});

rewrite_record!(PatternStage, []; { pattern });

rewrite_enum!(PatternTransform<C>, [C]; {
    Unresolved { form },
    Linear { direction, spacing, count, second },
    LinearOffsets { direction, offsets },
    Circular { axis_origin, axis_dir, angle, count },
    CircularAngles { axis_origin, axis_dir, angles },
    CurveDriven { path, spacing, count },
    Mirror { plane_origin, plane_normal },
    MirrorReference { plane },
    Scale { center, final_factor, count },
    Composite { stages },
});
