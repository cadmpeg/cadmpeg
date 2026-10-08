//! Opaque link ownership and refusal tests.

use super::{OpaqueGeometryLinks, OpaqueLinkGroup};
use cadmpeg_core::decode::{
    refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::unknown::UnknownRecord;

const OLD_LINKS: usize = 128;
const FIRST: &str = "sldprt:test:record#first";
const SECOND: &str = "sldprt:test:record#second";

fn unknown(id: &str) -> UnknownRecord {
    let mut links = Vec::with_capacity(OLD_LINKS);
    for _ in 0..OLD_LINKS {
        links.push("previous".into());
    }
    UnknownRecord::retained(UnknownId::mint(id).expect("identity grammar"), 0, Vec::new(), links)
}

fn run_attachment(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let mut links = OpaqueGeometryLinks::new(ctx)?;
    links.push(FIRST, "sldprt:geometry:surface#first")?;
    links.push(SECOND, "sldprt:geometry:curve#second")?;
    let mut unknowns = [unknown(FIRST), unknown(SECOND)];
    for record in &mut unknowns {
        assert_eq!(record.links().len(), OLD_LINKS);
        assert_eq!(record.links_mut().capacity(), OLD_LINKS);
    }
    links.attach(&mut unknowns)?;
    assert_eq!(unknowns[0].links().len(), OLD_LINKS + 1);
    assert_eq!(unknowns[1].links().len(), OLD_LINKS + 1);
    assert_eq!(unknowns[0].links().last().map(String::as_str), Some("sldprt:geometry:surface#first"));
    assert_eq!(unknowns[1].links().last().map(String::as_str), Some("sldprt:geometry:curve#second"));
    assert!(ctx.resource_refusal().is_none());
    Ok(())
}

#[test]
fn opaque_link_attachment_releases_map_and_group_scratch() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // At output growth, only the two four-slot link vectors, the four-slot
    // position vector, and the old output vector's reallocation overlap live.
    // The map and unknown-position index have already dropped. After the first
    // group, its four-slot vector also drops before the second output grows.
    let group_vectors = 2 * 4 * std::mem::size_of::<String>();
    let positions = 4 * std::mem::size_of::<(usize, OpaqueLinkGroup<'_>)>();
    let output_overlap = OLD_LINKS * std::mem::size_of::<String>();
    policy.limits.max_materialized_bytes = u64::try_from(group_vectors + positions + output_overlap)
        .expect("bounded fixture storage");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    run_attachment(&ctx).expect("only live staging remains charged");
    ctx.finish_session().expect("accepted session");

    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    run_attachment(&ctx).expect("successful attachment");
    let _probe = RefusalProbe::arm(ResourceDimension::MaterializedBytes, "after opaque attachment", Some(1));
    let error = ctx.reserve_scoped(1, "after opaque attachment").err()
        .expect("post-attachment boundary refuses");
    let CodecError::ResourceLimit(limit) = error else { panic!("expected scratch refusal"); };
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn opaque_link_attachment_preserves_error_format_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let mut links = OpaqueGeometryLinks::new(&ctx).expect("staging owner");
    links.push(FIRST, "sldprt:geometry:surface#first").expect("staged link");
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "format SLDPRT opaque geometry error", None);
    let error = links.attach(&mut []).expect_err("missing source record formats an admitted error");
    let CodecError::ResourceLimit(limit) = error else { panic!("expected format refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "format SLDPRT opaque geometry error");
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
