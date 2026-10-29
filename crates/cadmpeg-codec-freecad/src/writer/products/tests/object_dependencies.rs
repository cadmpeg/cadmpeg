// SPDX-License-Identifier: Apache-2.0
//! Directed object dependencies remain separate from product membership.

use super::*;

fn native_product(dependencies: &[&str]) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(definition("Part"));
    let native: crate::native::ObjectRecord = serde_json::from_value(json!({
        "id": "fcstd:native:object#Part", "name": "Part", "type_name": "App::Part",
        "attributes": {}, "dependencies": dependencies, "order": 0
    }))
    .expect("typed native product");
    crate::test_support::with_service_context(&[], |ctx| {
        ir.native
            .namespace_mut("fcstd")
            .set_arena(ctx, "objects", &[native])
    })
    .expect("native object arena");
    ir
}

fn output(ir: &CadIr, names: &mut HashMap<String, String>) -> (Vec<Object>, Vec<LossNote>) {
    let mut objects = vec![Object::new(
        "RenamedTarget".into(),
        "Vendor::Node",
        "Target",
    )];
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(ctx, ir, &mut objects, names, &HashMap::new(), &mut losses)
    })
    .expect("product output");
    (objects, losses)
}

fn output_names() -> HashMap<String, String> {
    HashMap::from([
        (
            "authored:model:product_definition#Part".into(),
            "RenamedPart".into(),
        ),
        ("fcstd:native:object#Part".into(), "RenamedPart".into()),
        ("fcstd:native:object#Target".into(), "RenamedTarget".into()),
    ])
}

#[test]
fn native_directed_dependencies_survive_without_group_members() {
    let ir = native_product(&["fcstd:native:object#Target"]);
    let (objects, losses) = output(&ir, &mut output_names());
    let part = objects
        .iter()
        .find(|object| object.name == "RenamedPart")
        .expect("product");
    assert_eq!(part.dependencies, ["RenamedTarget"]);
    assert_eq!(part.properties["Group"], group(&[]));
    assert!(!part.properties.contains_key("Placement"));
    assert_eq!(
        losses,
        [FreecadLossCode::ExportFeatureNotTransferred.note(
            "product authored:model:product_definition#Part source name is emitted as RenamedPart"
        )]
    );
}

#[test]
fn omitted_unknown_transform_occurrence_does_not_erase_native_dependencies() {
    let mut ir = native_product(&["fcstd:native:object#Target"]);
    let mut omitted = occurrence("Unknown", &definition("Part"), None);
    omitted.transform = None;
    omitted.native_ref = Some("fcstd:native:object#Target".into());
    let id = omitted.id.clone();
    ir.model.occurrences.push(omitted);
    let mut names = output_names();
    names.insert(id.as_str().to_owned(), "Unknown".into());
    let (objects, losses) = output(&ir, &mut names);
    let part = objects
        .iter()
        .find(|object| object.name == "RenamedPart")
        .expect("product");
    assert_eq!(part.dependencies, ["RenamedTarget"]);
    assert_eq!(part.properties["Group"], group(&[]));
    assert!(!part.properties.contains_key("Placement"));
    assert!(!objects.iter().any(|object| object.name == "Unknown"));
    assert!(!names.contains_key(id.as_str()));
    assert_eq!(losses, [FreecadLossCode::ExportFeatureNotTransferred.note(
                "product authored:model:product_definition#Part source name is emitted as RenamedPart"
            ),
            FreecadLossCode::ExportOccurrenceNotTransferred.note(format!(
                "occurrence {id} omitted because its placement or an ancestor placement is unknown"
            )),
        ]);
    assert!(losses[1].message.contains(id.as_str()));
}

#[test]
fn current_occurrence_membership_does_not_replace_directed_dependencies() {
    let mut ir = native_product(&["fcstd:native:object#Target"]);
    let part_definition = definition("Part");
    let mut parent = occurrence("Part", &part_definition, None);
    parent.native_ref = part_definition.native_ref.clone();
    let child = occurrence("Member", &part_definition, Some(&parent));
    let mut names = output_names();
    names.insert(parent.id.as_str().to_owned(), "RenamedPart".into());
    names.insert(child.id.as_str().to_owned(), "Member".into());
    ir.model.occurrences.extend([parent, child]);
    let (objects, _) = output(&ir, &mut names);
    let part = objects
        .iter()
        .find(|object| object.name == "RenamedPart")
        .expect("product");
    assert_eq!(part.dependencies, ["RenamedTarget"]);
    assert_eq!(part.properties["Group"], group(&["Member".into()]));
}

#[test]
fn unresolved_native_identity_cannot_bind_an_unrelated_output_name() {
    let ir = native_product(&["fcstd:native:object#Omitted"]);
    let mut objects = vec![Object::new(
        "fcstd:native:object#Omitted".into(),
        "Vendor::Node",
        "Unrelated",
    )];
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(
            ctx,
            &ir,
            &mut objects,
            &mut output_names(),
            &HashMap::new(),
            &mut losses,
        )
    })
    .expect("product output");
    let part = objects
        .iter()
        .find(|object| object.name == "RenamedPart")
        .expect("product");
    assert!(part.dependencies.is_empty());
    assert_eq!(losses, [
            FreecadLossCode::ExportFeatureNotTransferred.note(
                "object RenamedPart (fcstd:native:object#Part) dependencies on omitted records are removed"
            ),
            FreecadLossCode::ExportFeatureNotTransferred.note(
                "product authored:model:product_definition#Part source name is emitted as RenamedPart"
            ),
        ]);
    let native_objects = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let [native_owner] = native_objects.as_slice() else {
        panic!("one authored native owner")
    };
    assert!(losses[0].message.contains(native_owner.id().as_str()));
    assert!(objects
        .iter()
        .any(|object| object.name == "fcstd:native:object#Omitted"));
}
