// SPDX-License-Identifier: Apache-2.0
use std::collections::HashSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
use cadmpeg_ir::transform::Transform;

use super::{Located, Locations};
use crate::brep::location_identity::LocationIdentityError;
use crate::brep::{Tables, TextLocation, TextTShapes};

fn with_locations(test: impl FnOnce(&DecodeContext<'_>, Tables<'_>)) {
    let rows = [
        TextLocation {
            factors: Vec::new(),
            transform: Transform::identity(),
        },
        TextLocation {
            factors: serde_json::from_str(r#"[{"location":1,"power":-1}]"#).unwrap(),
            transform: Transform::identity(),
        },
    ];
    let shapes = TextTShapes::default();
    let tables = Tables {
        locations: &rows,
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &shapes,
        triangulations: &[],
        roots: &[],
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    test(&ctx, tables);
}

#[test]
fn cached_and_identity_compositions_each_consume_admission_work() {
    with_locations(|ctx, tables| {
        let mut locations = Locations::new(ctx, tables).unwrap();
        let datum = locations.source(tables, 1).unwrap();
        locations.admit(ctx, tables, datum, datum).unwrap();
        locations.work = WorkBudget::new(2);
        locations.admit(ctx, tables, datum, datum).unwrap();
        locations
            .admit(ctx, tables, Located::identity(), datum)
            .unwrap();
        assert!(matches!(
            locations.admit(ctx, tables, datum, datum),
            Err(LocationIdentityError::Bounded)
        ));
        assert!(locations.compose(datum, datum).is_ok());
    });
}

#[test]
fn cancellation_into_existing_identity_still_charges_input_term_scan() {
    with_locations(|ctx, tables| {
        let mut locations = Locations::new(ctx, tables).unwrap();
        let datum = locations.source(tables, 1).unwrap();
        let inverse = locations.source(tables, 2).unwrap();
        let identity_count = locations.identities.len();
        locations.work = WorkBudget::new(2);
        assert!(matches!(
            locations.admit(ctx, tables, datum, inverse),
            Err(LocationIdentityError::Bounded)
        ));
        assert!(!locations.products.contains_key(&(datum.identity, inverse.identity)));
        locations.work = WorkBudget::new(4);
        let canceled = locations.admit(ctx, tables, datum, inverse).unwrap();
        assert_eq!(canceled.identity, 0);
        assert_eq!(canceled.transform, Transform::identity());
        assert_eq!(locations.identities.len(), identity_count);
    });
}

#[test]
fn repeated_visited_state_is_deduplicated_and_still_consumes_work() {
    with_locations(|ctx, tables| {
        let mut locations = Locations::new(ctx, tables).unwrap();
        let datum = locations.source(tables, 1).unwrap();
        let mut visited = HashSet::new();
        locations.work = WorkBudget::new(2);
        assert!(locations.visit(ctx, &mut visited, 7, datum).unwrap());
        assert!(!locations.visit(ctx, &mut visited, 7, datum).unwrap());
        assert!(matches!(
            locations.visit(ctx, &mut visited, 8, datum),
            Err(LocationIdentityError::Bounded)
        ));
        assert_eq!(visited.len(), 1);
    });
}
