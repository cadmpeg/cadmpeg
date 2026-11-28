// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn the_parameter_of_a_diameter_dimension_states_its_display() {
    use cadmpeg_ir::design::parameter::DimensionDisplay;
    use cadmpeg_ir::features::parameters::{DimensionDisplay as V10Display, ParameterValue as V10};
    use cadmpeg_ir::scalar::Length;
    const OWNER: &str = "f3d:model:feature#sketch:20";
    let mut legacy = parameter(
        "d1",
        Some(OWNER),
        "10 mm",
        Some(V10::Length(Length::new(10.0).unwrap())),
        &[],
        &[("source_kind", "Diameter Dimension-1")],
    );
    legacy.display = Some(V10Display::Diameter);
    let scopes = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
    let mut losses = Vec::new();
    let projected = crate::design_v11::parameters::project(
        std::slice::from_ref(&legacy),
        &HashMap::new(),
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::from([OWNER]),
        &mut losses,
    );
    assert!(losses.is_empty(), "{losses:?}");
    let [diameter] = projected.as_slice() else {
        panic!("one parameter: {projected:?}")
    };
    assert_eq!(diameter.display, Some(DimensionDisplay::Diameter));
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.parameters = vec![legacy];
    ir.design.parameters = projected;
    let charge = parameter_charge(&ir, "f3d:model:parameter#d1");
    assert!(!charge.iter().any(|field| field == "display"), "{charge:?}");
}
