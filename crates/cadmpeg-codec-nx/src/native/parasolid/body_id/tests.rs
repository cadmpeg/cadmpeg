// SPDX-License-Identifier: Apache-2.0
use super::{
    resolve, BodyIdEncoding, ParasolidAttributeDefinition, ParasolidEntity51Record,
    ParasolidEntity52IntegerRecord,
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
        let join = resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &entities,
            &integers,
        )
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
        assert!(resolve(
            ctx,
            12,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &entities,
            &integers
        )
        .unwrap()
        .is_none());
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &[],
            &entities,
            &integers
        )
        .unwrap()
        .is_none());
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &[],
            &integers
        )
        .unwrap()
        .is_none());
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &entities,
            &[]
        )
        .unwrap()
        .is_none());
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &[definition(), definition()],
            &entities,
            &integers
        )
        .unwrap()
        .is_none());
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &[entity(), entity()],
            &integers
        )
        .unwrap()
        .is_none());
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &entities,
            &[integer(), integer()]
        )
        .unwrap()
        .is_none());
        let mut aliased = definition();
        aliased.name = crate::printable_string::PrintableString::new("other".to_string()).unwrap();
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &[definition(), aliased],
            &entities,
            &integers
        )
        .unwrap()
        .is_none());
        let mut aliased = entity();
        aliased.definition_xmt = 99;
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &[entity(), aliased],
            &integers
        )
        .unwrap()
        .is_none());
        let mut changed = integer();
        changed.stream_ordinal = 1;
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &entities,
            &[changed]
        )
        .unwrap()
        .is_none());
        let mut changed = integer();
        changed.values =
            crate::parasolid::counted_values::CountedValues::new(vec![11, 12]).unwrap();
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &entities,
            &[changed]
        )
        .unwrap()
        .is_none());
        let mut changed = definition();
        changed.field_codes = vec![crate::parasolid::attribute_field::AttributeField::Integer];
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &[changed],
            &entities,
            &integers
        )
        .unwrap()
        .is_none());
        let mut changed = entity();
        changed.leading_references[0] = 1;
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &definitions,
            &[changed],
            &integers
        )
        .unwrap()
        .is_none());
        let mut d = definition();
        d.stream_ordinal = 1;
        let mut e = entity();
        e.stream_ordinal = 1;
        let mut i = integer();
        i.stream_ordinal = 1;
        assert!(resolve(
            ctx,
            11,
            BodyIdEncoding::CurveMeshIgnored,
            &[definition(), d],
            &[entity(), e],
            &[integer(), i]
        )
        .unwrap()
        .is_none());
    });
}

#[test]
fn brep_body_id_uses_declared_integer_predecessor_and_abstains_on_ambiguity() {
    crate::test_support::with_decode_context(|ctx| {
        let mut d = definition();
        d.field_codes = vec![crate::parasolid::attribute_field::AttributeField::Integer];
        let mut i = integer();
        i.values = crate::parasolid::counted_values::CountedValues::new(vec![10]).unwrap();
        let e = entity();
        let check = |identity, definitions: &[_], entities: &[_], integers: &[_]| {
            resolve(
                ctx,
                identity,
                BodyIdEncoding::BrepInteger,
                definitions,
                entities,
                integers,
            )
            .unwrap()
            .map(|m| (m.stream, m.body_xmt))
        };
        assert_eq!(
            check(
                11,
                std::slice::from_ref(&d),
                std::slice::from_ref(&e),
                std::slice::from_ref(&i)
            ),
            Some((0, 2))
        );
        for identity in [0, 10, 12] {
            assert_eq!(
                check(
                    identity,
                    std::slice::from_ref(&d),
                    std::slice::from_ref(&e),
                    std::slice::from_ref(&i)
                ),
                None
            );
        }
        assert_eq!(
            check(11, &[], std::slice::from_ref(&e), std::slice::from_ref(&i)),
            None
        );
        assert_eq!(
            check(11, std::slice::from_ref(&d), &[], std::slice::from_ref(&i)),
            None
        );
        assert_eq!(
            check(11, std::slice::from_ref(&d), std::slice::from_ref(&e), &[]),
            None
        );
        assert_eq!(
            check(
                11,
                &[d.clone(), d.clone()],
                std::slice::from_ref(&e),
                std::slice::from_ref(&i)
            ),
            None
        );
        assert_eq!(
            check(
                11,
                std::slice::from_ref(&d),
                &[e.clone(), e.clone()],
                std::slice::from_ref(&i)
            ),
            None
        );
        assert_eq!(
            check(
                11,
                std::slice::from_ref(&d),
                std::slice::from_ref(&e),
                &[i.clone(), i.clone()]
            ),
            None
        );
        let mut other = i.clone();
        other.stream_ordinal = 1;
        assert_eq!(
            check(
                11,
                std::slice::from_ref(&d),
                std::slice::from_ref(&e),
                &[other]
            ),
            None
        );
        let mut other = i.clone();
        other.values = crate::parasolid::counted_values::CountedValues::new(vec![10, 10]).unwrap();
        assert_eq!(
            check(
                11,
                std::slice::from_ref(&d),
                std::slice::from_ref(&e),
                &[other]
            ),
            None
        );
        assert_eq!(
            check(
                11,
                &[definition()],
                std::slice::from_ref(&e),
                std::slice::from_ref(&i)
            ),
            None
        );
        let mut other = d.clone();
        other.type_id = std::num::NonZeroU32::new(9001).unwrap();
        assert_eq!(
            check(
                11,
                &[other],
                std::slice::from_ref(&e),
                std::slice::from_ref(&i)
            ),
            None
        );
        let mut other = e.clone();
        other.leading_references[0] = 1;
        assert_eq!(
            check(
                11,
                std::slice::from_ref(&d),
                &[other],
                std::slice::from_ref(&i)
            ),
            None
        );
        let mut other_d = d.clone();
        other_d.stream_ordinal = 1;
        let mut other_e = e.clone();
        other_e.stream_ordinal = 1;
        let mut other_i = i.clone();
        other_i.stream_ordinal = 1;
        assert_eq!(check(11, &[d, other_d], &[e, other_e], &[i, other_i]), None);
    });
}
