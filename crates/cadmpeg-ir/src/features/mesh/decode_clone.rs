use super::MeshSelection;

rewrite_scalar!(super::MeshFaceGroupMethod);
rewrite_record!(super::MeshFaceGroupGenerationSettings, []; { method, angle_threshold, boundary_tolerance, minimum_area_fraction });

rewrite_enum!(MeshSelection, []; {
    Tessellations(field0),
    Native { native, members },
    Unresolved { feature },
});
