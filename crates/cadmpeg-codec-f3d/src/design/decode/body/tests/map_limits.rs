// SPDX-License-Identifier: Apache-2.0

use super::super::body_bindings;
use super::{
    assert_refuses_at, body_map_bytes, body_map_metadata, snapshot_body_map_bytes,
    snapshot_body_map_metadata, snapshot_records,
};
use cadmpeg_core::decode::ResourceDimension;

#[test]
fn snapshot_body_map_refuses_name_and_collection_limits() {
    let bytes = snapshot_body_map_bytes(0);
    let metadata = snapshot_body_map_metadata();
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "f3d body-map primary index",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d registered Design entities",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d snapshot body-map pairs",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d snapshot body-map records",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d Design UTF-16 text",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d snapshot body-map pairs",
        ),
        (
            ResourceDimension::WorkUnits,
            "check F3D snapshot body-map pair types",
        ),
    ] {
        assert_refuses_at(dimension, operation, |ctx| {
            snapshot_records(ctx, &bytes, &metadata).map(|_| ())
        });
    }
}

#[test]
fn modern_body_map_refuses_name_and_collection_limits() {
    let bytes = body_map_bytes(10, 1, &[(7, 500)]);
    let metadata = body_map_metadata();
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "f3d body-map primary index",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d body-map typed entities",
        ),
        (ResourceDimension::CollectionItems, "f3d body-map pairs"),
        (ResourceDimension::CollectionItems, "f3d body-map records"),
        (
            ResourceDimension::CollectionItems,
            "f3d flattened body-map pairs",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d Design UTF-16 text",
        ),
        (ResourceDimension::MaterializedBytes, "f3d body-map pairs"),
        (
            ResourceDimension::MaterializedBytes,
            "f3d flattened body-map pairs",
        ),
    ] {
        assert_refuses_at(dimension, operation, |ctx| {
            body_bindings(ctx, &bytes, &metadata).map(|_| ())
        });
    }
}
