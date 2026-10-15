// SPDX-License-Identifier: Apache-2.0

use crate::products::{Occurrence, OccurrenceParent, PrototypeReference};
use crate::transform::Transform;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn occurrence(id: &str, parent: Option<&str>) -> Occurrence {
    Occurrence {
        id: id.try_into().unwrap(),
        prototype: PrototypeReference::Unresolved {},
        parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
            OccurrenceParent::Occurrence {
                occurrence: parent.try_into().unwrap(),
            }
        }),
        ordinal: 0,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [crate::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

#[test]
fn assembly_validation_uses_active_session_depth_in_parent_chains() {
    let rows = [
        occurrence(
            "test:model:occurrence#child",
            Some("test:model:occurrence#root"),
        ),
        occurrence("test:model:occurrence#root", None),
    ];
    for (limit, caller) in [(1, false), (2, true)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let guard = caller.then(|| ctx.enter_nested("active assembly caller").unwrap());
        let Err(CodecError::ResourceLimit(resource)) = super::validate(&ctx, &rows) else {
            panic!("parent frame must refuse");
        };
        assert_eq!(resource.dimension, ResourceDimension::RecursionDepth);
        drop(guard);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == resource)
        );
    }
}
