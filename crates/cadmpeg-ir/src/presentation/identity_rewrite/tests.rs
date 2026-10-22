// SPDX-License-Identifier: Apache-2.0
//! Layer item references are list entries, not required fields of the layer.

use crate::ids::PointId;
use crate::presentation::PresentationItem;
use crate::schema::rewrite::typed::prune::{analyze, DependencyPosition, ReferencePruneEffect};
use crate::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeSet;

fn point(key: &str) -> PresentationItem {
    PresentationItem::Point {
        point: PointId::mint(format!("test:model:point#{key}")).unwrap(),
    }
}

#[test]
fn presentation_item_lists_prune_only_missing_targets_and_preserve_order_and_duplicates() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        b"typed reference operation",
        &arena,
        &DecodePolicy::service(),
    )
    .unwrap();
    let mut items = vec![
        point("kept-b"),
        point("gone"),
        point("kept-a"),
        point("gone"),
        point("kept-b"),
    ];
    let deleted = BTreeSet::from(["test:model:point#gone".to_owned()]);
    let mut references = Vec::new();
    items
        .visit_dependency_references(&ctx, &mut |reference| {
            references.push((reference.target.to_owned(), reference.position));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        references,
        ["kept-b", "gone", "kept-a", "gone", "kept-b"]
            .into_iter()
            .enumerate()
            .map(|(index, key)| (
                format!("test:model:point#{key}"),
                DependencyPosition::List { length: 5, index },
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        analyze(&items, &ctx, &deleted).unwrap(),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(
        items.prune_reference_lists(&ctx, &deleted).unwrap(),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(items, [point("kept-b"), point("kept-a"), point("kept-b")]);
}

#[test]
fn presentation_item_scalar_optional_and_fixed_array_positions_keep_required_ownership() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        b"typed reference operation",
        &arena,
        &DecodePolicy::service(),
    )
    .unwrap();
    let deleted = BTreeSet::from(["test:model:point#gone".to_owned()]);
    assert_eq!(
        analyze(&point("gone"), &ctx, &deleted).unwrap(),
        ReferencePruneEffect::DropRequired
    );
    assert_eq!(
        analyze(&Some(point("gone")), &ctx, &deleted).unwrap(),
        ReferencePruneEffect::DropRequired
    );
    let mut fixed = [point("gone"), point("kept")];
    let original = fixed.clone();
    let mut positions = Vec::new();
    fixed
        .visit_dependency_references(&ctx, &mut |reference| {
            positions.push(reference.position);
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        positions.as_slice(),
        [
            DependencyPosition::UnrepresentablePrunedList {
                length: 2,
                index: 0,
                ..
            },
            DependencyPosition::UnrepresentablePrunedList {
                length: 2,
                index: 1,
                ..
            }
        ]
    ));
    assert!(matches!(
        fixed.prune_reference_lists(&ctx, &deleted).unwrap(),
        ReferencePruneEffect::UnrepresentablePrunedList { .. }
    ));
    assert_eq!(fixed, original);
}
