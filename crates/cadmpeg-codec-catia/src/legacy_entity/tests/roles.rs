//! Field-bound role storage and lazy boundary membership.

use super::super::*;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};

#[test]
fn legacy_field_bound_roles_keep_one_admitted_backing() {
    const COUNT: usize = 64;
    for (record, encoding, selector) in [
        (
            &[0x01, 0xd1, 0x00, 0xe8, 0x28, 0x17, 0x01, 0xfe][..],
            LegacyRoleSelectorEncoding::Paged,
            1,
        ),
        (
            &[0x01, 0x80, 7, 0, 0, 0, 0xe8, 0x28, 0x17, 0x01, 0xfe][..],
            LegacyRoleSelectorEncoding::FixedU32,
            7,
        ),
    ] {
        let bytes = record.repeat(COUNT);
        let assert_roles = |roles: &[LegacyRoleSelector]| {
            assert_eq!(roles.len(), COUNT);
            for (index, role) in roles.iter().enumerate() {
                assert_eq!(role.offset, index * record.len());
                assert_eq!(role.entity_id, 19);
                assert_eq!(role.name, LegacyRoleName::Selector(1));
                assert_eq!(role.encoding, encoding);
                assert_eq!(role.selector, selector);
                assert_eq!(role.field_code, Some(0x1728));
            }
        };
        let backing = COUNT * std::mem::size_of::<LegacyRoleSelector>();
        let roles = crate::test_support::with_retained_limit(
            u64::try_from(backing).expect("role backing"),
            |ctx| parse_role_selectors(ctx, &bytes, 0, bytes.len(), 19),
        )
        .expect("only the destination role backing is retained");
        assert_roles(&roles);

        // Each role has one output slot. The large stable sort has two index slots.
        let roles = crate::test_support::with_collection_limit(
            u64::try_from(3 * COUNT).expect("role and sort slots"),
            |ctx| parse_role_selectors(ctx, &bytes, 0, bytes.len(), 19),
        )
        .expect("field roles have no intermediate collection slots");
        assert_roles(&roles);

        // The final growth overlaps the 64-slot backing with the old 32 slots.
        // The two sort indices fit inside that extra 32-role allowance.
        crate::test_support::with_materialized_limit(
            u64::try_from(backing + backing / 2).expect("growth overlap"),
            |ctx| {
                let mut parent = ctx.reserve_scoped(0, "test role parent")?;
                let roles = parent
                    .with_storage(|| parse_role_selectors(ctx, &bytes, 0, bytes.len(), 19))?;
                assert_roles(&roles);
                assert_eq!(ctx.resource_refusal(), None);
                drop(roles);
                drop(parent);
                Ok::<_, cadmpeg_core::CodecError>(())
            },
        )
        .expect("a parent keeps only the live role backing");
    }
}

#[test]
fn legacy_role_end_index_waits_for_a_bounded_field_header() {
    const COUNT: usize = 4096;
    for tail in [
        &[][..],
        &[0xe8, 0x28, 0x17, 0x00][..],
        &[0xe8, 0x28, 0x17][..],
    ] {
        let mut bytes = [5, b'n', b'a', b'm', b'e', 0xd1, 0].repeat(COUNT);
        bytes.extend_from_slice(tail);
        for operation in ["catia_legacy_role_end_visits", "catia_legacy_role_ends"] {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            crate::test_support::with_work_limit(u64::MAX, |ctx| {
                let roles = parse_role_selectors(ctx, &bytes, 0, bytes.len(), 19)?;
                assert_eq!(roles.len(), COUNT);
                for (index, role) in roles.iter().enumerate() {
                    assert_eq!(role.offset, index * 7);
                    assert_eq!(role.entity_id, 19);
                    assert_eq!(role.name.literal(), Some("name"));
                    assert_eq!(role.encoding, LegacyRoleSelectorEncoding::Paged);
                    assert_eq!(role.selector, 1);
                    assert_eq!(role.field_code, None);
                }
                assert_eq!(ctx.resource_refusal(), None);
                Ok::<_, cadmpeg_core::CodecError>(())
            })
            .expect("unqueried role-end membership has no work");
        }
    }
}

#[test]
fn legacy_role_end_snapshot_preserves_literal_boundary_precedence() {
    const COUNT: usize = 64;
    let record = [
        5, b'n', b'a', b'm', b'e', 0xd1, 0, 0xe8, 0x28, 0x17, 0x01, 0xfe,
    ];
    let bytes = record.repeat(COUNT);
    let roles = crate::test_support::with_service_context(|ctx| {
        parse_role_selectors(ctx, &bytes, 0, bytes.len(), 19)
    })
    .expect("one membership snapshot");
    assert_eq!(roles.len(), COUNT);
    for (index, role) in roles.iter().enumerate() {
        assert_eq!(role.offset, index * record.len());
        assert_eq!(role.name.literal(), Some("name"));
        assert_eq!(role.field_code, Some(0x1728));
    }
    for operation in ["catia_legacy_role_end_visits", "catia_legacy_role_ends"] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            parse_role_selectors(ctx, &bytes, 0, bytes.len(), 19)
        })
        .expect_err("a queried snapshot keeps visit and key admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}
