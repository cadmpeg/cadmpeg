// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::{
    CompositePattern, LinearPatternDirection, NoNestedComposite, PatternForm, PatternKind,
    PatternScaleCenter, PatternSeed, PatternStage, PatternTransform,
};

clone_record_for_decode!(CompositePattern; (field0));

clone_record_for_decode!(LinearPatternDirection; { direction, spacing, count });

clone_copy_for_decode!(NoNestedComposite);

clone_copy_for_decode!(PatternForm);

clone_record_for_decode!(PatternKind<C>, [C]; (field0));

clone_enum_for_decode!(PatternScaleCenter; {
    FirstSeedCentroid,
    Point(field0),
    Native(field0),
});

clone_enum_for_decode!(PatternSeed; {
    Feature(field0),
    Faces(field0),
    Bodies(field0),
    Occurrences(field0),
});

clone_record_for_decode!(PatternStage; { pattern });

clone_enum_for_decode!(PatternTransform<C>, [C]; {
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
