// SPDX-License-Identifier: Apache-2.0
use super::{
    resolve, ParasolidAttributeDefinition, ParasolidEntity51Record, ParasolidEntity52IntegerRecord,
};

fn definition() -> ParasolidAttributeDefinition {
    serde_json::from_value(serde_json::json!({"id":"definition", "stream_ordinal":0,"xmt":12,"next_definition_xmt":1,"identifier_xmt":13,"identifier_inflated_offset":0,"name":"body_id","type_id":9000,"action_codes":[0,0,0,0,0,0,0,0],"field_names_xmt":1,"legal_owner_flags":"00000000000000000000000000000000","field_count":1,"field_codes":[0],"inflated_offset":0})).unwrap()
}
fn entity() -> ParasolidEntity51Record {
    serde_json::from_value(serde_json::json!({"id":"entity", "stream_ordinal":0,"xmt":3,"flags":1,"sequence":1,"definition_xmt":12,"leading_references":[2,13,1,1,1],"trailing_references":[14],"byte_len":28,"inflated_offset":0})).unwrap()
}
fn integer() -> ParasolidEntity52IntegerRecord {
    serde_json::from_value(serde_json::json!({"id":"value", "stream_ordinal":0,"xmt":14,"values":[11],"byte_len":16,"inflated_offset":0})).unwrap()
}
#[test]
fn mesh_body_id_requires_unique_complete_same_stream_scalar_storage() {
    crate::test_support::with_decode_context(|ctx| {
        let definitions = [definition()];
        let entities = [entity()];
        let integers = [integer()];
        let join = resolve(ctx, 11, &definitions, &entities, &integers)
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                join.stream,
                join.body_xmt,
                join.definition,
                join.entity,
                join.value_record
            ),
            (0, 2, "definition", "entity", "value")
        );
        assert!(resolve(ctx, 12, &definitions, &entities, &integers)
            .unwrap()
            .is_none());
        assert!(resolve(ctx, 11, &[], &entities, &integers)
            .unwrap()
            .is_none());
        assert!(resolve(ctx, 11, &definitions, &[], &integers)
            .unwrap()
            .is_none());
        assert!(resolve(ctx, 11, &definitions, &entities, &[])
            .unwrap()
            .is_none());
        assert!(
            resolve(ctx, 11, &[definition(), definition()], &entities, &integers)
                .unwrap()
                .is_none()
        );
        assert!(
            resolve(ctx, 11, &definitions, &[entity(), entity()], &integers)
                .unwrap()
                .is_none()
        );
        assert!(
            resolve(ctx, 11, &definitions, &entities, &[integer(), integer()])
                .unwrap()
                .is_none()
        );
        let mut aliased = definition();
        aliased.name = crate::printable_string::PrintableString::new("other".to_string()).unwrap();
        assert!(
            resolve(ctx, 11, &[definition(), aliased], &entities, &integers)
                .unwrap()
                .is_none()
        );
        let mut aliased = entity();
        aliased.definition_xmt = 99;
        assert!(
            resolve(ctx, 11, &definitions, &[entity(), aliased], &integers)
                .unwrap()
                .is_none()
        );
        let mut changed = integer();
        changed.stream_ordinal = 1;
        assert!(resolve(ctx, 11, &definitions, &entities, &[changed])
            .unwrap()
            .is_none());
        let mut changed = integer();
        changed.values =
            crate::parasolid::counted_values::CountedValues::new(vec![11, 12]).unwrap();
        assert!(resolve(ctx, 11, &definitions, &entities, &[changed])
            .unwrap()
            .is_none());
        let mut changed = definition();
        changed.field_codes = vec![crate::parasolid::attribute_field::AttributeField::Integer];
        assert!(resolve(ctx, 11, &[changed], &entities, &integers)
            .unwrap()
            .is_none());
        let mut changed = entity();
        changed.leading_references[0] = 1;
        assert!(resolve(ctx, 11, &definitions, &[changed], &integers)
            .unwrap()
            .is_none());
        let mut d = definition();
        d.stream_ordinal = 1;
        let mut e = entity();
        e.stream_ordinal = 1;
        let mut i = integer();
        i.stream_ordinal = 1;
        assert!(
            resolve(ctx, 11, &[definition(), d], &[entity(), e], &[integer(), i])
                .unwrap()
                .is_none()
        );
    });
}
