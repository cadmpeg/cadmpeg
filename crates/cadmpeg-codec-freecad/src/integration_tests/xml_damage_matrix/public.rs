// SPDX-License-Identifier: Apache-2.0
//! Every public golden uses the same independently framed XML damage controls.

macro_rules! fixture_controls {
    ($damage:ident, $cuts:ident, $file:literal) => {
        #[test]
        fn $damage() {
            super::assert_damage_matrix(
                $file,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../corpus/freecad_fcstd/fixtures/",
                    $file
                )),
                false,
            );
        }

        #[test]
        fn $cuts() {
            super::assert_archive_cuts(
                $file,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../corpus/freecad_fcstd/fixtures/",
                    $file
                )),
            );
        }
    };
}

fixture_controls!(
    external_component_damage,
    external_component_cuts,
    "external_component.FCStd"
);
fixture_controls!(
    product_assembly_damage,
    product_assembly_cuts,
    "product_assembly.FCStd"
);
fixture_controls!(
    core_operations_damage,
    core_operations_cuts,
    "core_operations.FCStd"
);
fixture_controls!(
    sketch_constraints_damage,
    sketch_constraints_cuts,
    "sketch_constraints.FCStd"
);
fixture_controls!(
    sketch_conics_damage,
    sketch_conics_cuts,
    "sketch_conics.FCStd"
);
fixture_controls!(
    gui_appearance_damage,
    gui_appearance_cuts,
    "gui_appearance.FCStd"
);
fixture_controls!(
    design_history_damage,
    design_history_cuts,
    "design_history.FCStd"
);
fixture_controls!(
    binary_exact_shape_damage,
    binary_exact_shape_cuts,
    "binary_exact_shape.FCStd"
);
fixture_controls!(
    application_payloads_damage,
    application_payloads_cuts,
    "application_payloads.FCStd"
);
fixture_controls!(
    geometry_topology_damage,
    geometry_topology_cuts,
    "geometry_topology.FCStd"
);
fixture_controls!(
    core_design_product_damage,
    core_design_product_cuts,
    "core_design_product.FCStd"
);
fixture_controls!(
    techdraw_annotations_damage,
    techdraw_annotations_cuts,
    "techdraw_annotations.FCStd"
);
