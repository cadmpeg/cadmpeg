// SPDX-License-Identifier: Apache-2.0
use super::{design_object, feature, parameter};
use crate::design_feature::{
    native_operation_owned_objects, nearest_feature_for_design_object, normalize_parameter_names,
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn feature_owner_memo_resolves_shared_chains_missing_links_and_cycles() {
    let mut objects = Vec::new();
    let root = "synthetic:test:object#root";
    objects.push(design_object(root, None));
    let mut parent = root.to_string();
    for index in 0..512 {
        let id = format!("synthetic:test:object#helper-{index:04}");
        objects.push(design_object(&id, Some(&parent)));
        parent = id;
    }
    objects.push(design_object(
        "synthetic:test:object#missing",
        Some("absent"),
    ));
    objects.push(design_object(
        "synthetic:test:object#cycle-a",
        Some("synthetic:test:object#cycle-b"),
    ));
    objects.push(design_object(
        "synthetic:test:object#cycle-b",
        Some("synthetic:test:object#cycle-a"),
    ));
    let sources = objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    let id = feature("root", root).id;
    let features = HashMap::from([(root.to_string(), id.clone())]);
    crate::test_support::with_work_limit(512 * 4096, |ctx| {
        let mut memo = HashMap::new();
        let mut storage = ctx.reserve_scoped(0, "catia_test_feature_owners")?;
        for object in objects.iter().rev() {
            let result = nearest_feature_for_design_object(
                ctx,
                &object.id,
                &sources,
                &features,
                &mut memo,
                &mut storage,
            )?;
            assert_eq!(
                result,
                if object.id.contains("cycle") || object.id.contains("missing") {
                    None
                } else {
                    Some(&id)
                }
            );
        }
        assert_eq!(memo.len(), objects.len() + 1);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("one resolution per chain node fits the allowance");
}

#[test]
fn operation_groups_preserve_source_order_and_nearest_operation() {
    let objects = [
        design_object("synthetic:test:object#outer", None),
        design_object(
            "synthetic:test:object#inner",
            Some("synthetic:test:object#outer"),
        ),
        design_object(
            "synthetic:test:object#value-b",
            Some("synthetic:test:object#inner"),
        ),
        design_object(
            "synthetic:test:object#value-a",
            Some("synthetic:test:object#inner"),
        ),
        design_object(
            "synthetic:test:object#cycle",
            Some("synthetic:test:object#cycle"),
        ),
        design_object("synthetic:test:object#missing", Some("absent")),
    ];
    let sources = objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    let operations = HashSet::from([objects[0].id.as_str(), objects[1].id.as_str()]);
    crate::test_support::with_service_context(|ctx| {
        let (groups, storage) = ctx.with_scoped_storage("catia_test_operation_groups", || {
            native_operation_owned_objects(ctx, &sources, &operations)
        })?;
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups[objects[0].id.as_str()]
                .iter()
                .map(|object| object.id.as_str())
                .collect::<Vec<_>>(),
            [objects[0].id.as_str()]
        );
        assert_eq!(
            groups[objects[1].id.as_str()]
                .iter()
                .map(|object| object.id.as_str())
                .collect::<Vec<_>>(),
            [
                objects[1].id.as_str(),
                objects[3].id.as_str(),
                objects[2].id.as_str()
            ]
        );
        drop(storage);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("operation ownership");
}

#[test]
fn parameter_suffix_index_skips_reserved_names_without_restarting() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    for index in 0..512 {
        let mut value = parameter(&format!("parameter-{index}"), "source");
        value.name = "Length".to_string();
        ir.model.parameters.push(value);
    }
    let mut reserved = parameter("reserved", "source");
    reserved.name = "Length#1".to_string();
    ir.model.parameters.push(reserved);
    crate::test_support::with_work_limit(512 * 4096, |ctx| normalize_parameter_names(ctx, &mut ir))
        .expect("each suffix is tried once for the base name");
    assert_eq!(ir.model.parameters[0].name, "Length");
    for (index, value) in ir.model.parameters[1..512].iter().enumerate() {
        assert_eq!(value.name, format!("Length#{}", index + 2));
        assert_eq!(value.properties["source_name"], "Length");
    }
    assert_eq!(ir.model.parameters[512].name, "Length#1");
}

#[test]
fn operation_groups_admit_many_independent_operations_without_cross_products() {
    let objects = (0..512)
        .map(|index| design_object(&format!("synthetic:test:object#operation-{index:04}"), None))
        .collect::<Vec<_>>();
    let sources = objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    let operations = objects
        .iter()
        .map(|object| object.id.as_str())
        .collect::<HashSet<_>>();
    crate::test_support::with_work_limit(512 * 4096, |ctx| {
        let (groups, _storage) = ctx.with_scoped_storage("catia_test_operation_groups", || {
            native_operation_owned_objects(ctx, &sources, &operations)
        })?;
        assert_eq!(groups.len(), 512);
        for object in &objects {
            assert_eq!(groups[object.id.as_str()].len(), 1);
            assert_eq!(groups[object.id.as_str()][0].id, object.id);
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("one group per operation fits the allowance");
}
