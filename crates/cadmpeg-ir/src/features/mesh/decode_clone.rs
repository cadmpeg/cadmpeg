use super::MeshSelection;

rewrite_enum!(MeshSelection, []; {
    Tessellations(field0),
    Native { native, members },
    Unresolved { feature },
});
