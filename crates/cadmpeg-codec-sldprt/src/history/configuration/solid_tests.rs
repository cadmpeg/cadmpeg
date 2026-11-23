// SPDX-License-Identifier: Apache-2.0
use super::configuration_solid;
use cadmpeg_ir::features::parameters::DesignConfiguration;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Point3;
use std::collections::BTreeMap;

/// The example cube, whose corner stands at (10, 10, 10) mm, and one
/// configuration whose body membership is `bodies`.
fn cube_with_configuration(bodies: Option<DistinctMembers<BodyId>>) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").unwrap(),
        ordinal: 0,
        active: false,
        source_index: Some(0),
        name: Some("configuration".into()),
        material: None,
        properties: BTreeMap::new(),
        bodies,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir
}

#[test]
fn a_configuration_solid_holds_the_vertices_of_its_own_bodies_only() {
    let corner = Point3::new(10.0, 10.0, 10.0);
    let cube = cadmpeg_ir::examples::unit_cube().unwrap().geometry.bodies[0]
        .id
        .clone();
    let own = cube_with_configuration(Some(std::iter::once(cube).collect()));
    assert!(configuration_solid(&own, 0).marks(corner));
    let other = cube_with_configuration(Some(DistinctMembers::default()));
    assert!(!configuration_solid(&other, 0).marks(corner));
    assert!(configuration_solid(&other, 0).is_empty());
    let unresolved = cube_with_configuration(None);
    assert!(configuration_solid(&unresolved, 0).marks(corner));
}
