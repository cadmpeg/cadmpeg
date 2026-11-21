// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::design::selection::{OccurrencePath, PathStep, StepRef};

use super::{assert_close, fixture, instance, millimetres, path, pose, project, Kind, EYE, STREAM};

#[test]
fn a_twice_placed_stack_composes_each_child_to_its_stored_complete_pose() {
    let (mut carriers, mut components) = fixture(2);
    carriers.retain(|carrier| matches!(carrier.record_index, 100 | 200 | 400));
    let roots = &mut components.get_mut(STREAM).unwrap().roots;
    let module = roots.remove(&100).unwrap();
    roots.insert(700, module);
    for carrier in &mut carriers {
        if carrier.component_record_index == 100 {
            carrier.component_record_index = 700;
        }
    }
    let parents = [
        pose(EYE, [25.2473, -42.15, 43.375]),
        pose(
            [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [25.2473, -3.35, 43.375],
        ),
    ];
    let children = [
        pose(EYE, [25.7473, -42.15, 47.105]),
        pose(EYE, [25.7473, -3.35, 46.095]),
    ];
    let mut paths = Vec::new();
    for (index, module) in [100, 400].into_iter().enumerate() {
        let record = u64::try_from(index).unwrap() * 2 + 900;
        paths.push(path(
            record,
            &[(700, Kind::Component), (module, Kind::Occurrence)],
            parents[index],
        ));
        paths.push(path(
            record + 1,
            &[
                (159, Kind::Component),
                (200, Kind::Occurrence),
                (module, Kind::Occurrence),
            ],
            children[index],
        ));
    }
    let projection = project(&carriers, &components, &paths);
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::tree::assemble(&mut ir.model, projection.into(), None);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    crate::tree::own_once(&ctx, &mut ir, &std::collections::BTreeMap::new()).unwrap();
    let child = instance(&carriers, &[200]);
    for (index, module) in [100, 400].into_iter().enumerate() {
        let parent = instance(&carriers, &[module]);
        let mut chain = OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(parent.clone()),
            configuration: None,
        }]);
        assert_close(
            ir.effective_transform(&chain).unwrap().unwrap().rows(),
            millimetres(parents[index]),
        );
        let definition = &ir
            .model
            .occurrences
            .iter()
            .find(|o| o.id == parent)
            .unwrap()
            .prototype;
        let owned = &ir
            .model
            .product_definitions
            .iter()
            .find(|d| &d.id == definition)
            .unwrap()
            .occurrences;
        assert_eq!(owned, std::slice::from_ref(&child));
        chain.0.push(PathStep {
            occurrence: StepRef::Occurrence(child.clone()),
            configuration: None,
        });
        assert_close(
            ir.effective_transform(&chain).unwrap().unwrap().rows(),
            millimetres(children[index]),
        );
    }
}
