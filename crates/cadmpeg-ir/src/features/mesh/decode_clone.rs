use super::{
    MeshConversion, MeshConversionMethod, MeshFaceDistribution, MeshModification, MeshRebuildMode,
    MeshReduceTarget, MeshRepair, MeshSelection,
};

rewrite_scalar!(super::MeshFaceGroupMethod);
rewrite_record!(super::MeshFaceGroupGenerationSettings, []; { method, angle_threshold, boundary_tolerance, minimum_area_fraction });

rewrite_enum!(MeshSelection, []; {
    Tessellations(field0),
    Native { native, members },
    Unresolved { feature },
});

rewrite_scalar!(super::MeshPlaneCutMode);
rewrite_scalar!(super::MeshPlaneCutFill);
rewrite_scalar!(super::MeshPlaneCutSide);
rewrite_record!(super::MeshPlaneCut, []; { origin, normal, mode, fill, retained_side });

rewrite_scalar!(MeshFaceDistribution);
rewrite_scalar!(MeshConversionMethod);
rewrite_record!(MeshConversion, []; { sources, method });
rewrite_scalar!(MeshRebuildMode);

rewrite_enum!(MeshReduceTarget, []; {
    MaximumDeviation { value },
    FaceProportion { value },
    FaceCount { value },
});

rewrite_enum!(MeshRepair, []; {
    CloseHoles {},
    StitchAndRemove {},
    Rebuild { mode, density, offset },
});

rewrite_enum!(MeshModification, []; {
    Separate { separation },
    PlaneCut { cut },
    Reduce { distribution, target },
    Repair { method },
    Remesh { distribution, edge_length, shape_preservation },
    Smooth { smoothness },
});

rewrite_scalar!(super::MeshSeparationMethod);
rewrite_enum!(super::MeshSeparationGroups, []; {
    All {},
    Selected { members },
});
rewrite_record!(super::MeshSeparation, []; { method, groups, multiple_bodies, keep_body });
