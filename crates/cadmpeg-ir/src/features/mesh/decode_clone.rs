use super::{
    MeshFaceDistribution, MeshModification, MeshRebuildMode, MeshReduceTarget, MeshRepair,
    MeshSelection,
};

rewrite_enum!(MeshSelection, []; {
    Tessellations(field0),
    Native { native, members },
    Unresolved { feature },
});

rewrite_scalar!(MeshFaceDistribution);
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
    Reduce { distribution, target },
    Repair { method },
    Remesh { distribution, edge_length, shape_preservation },
    Smooth { smoothness },
});
