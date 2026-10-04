// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::CacheContract;
use cadmpeg_ir::features::DistinctMembers;
use cadmpeg_ir::schema::structural::project;
use cadmpeg_ir::sketches::SketchProfiles;
use serde::Serialize;

#[derive(Serialize)]
struct FixedBooleanPredicates {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    disabled: bool,
    #[serde(skip_serializing_if = "is_false")]
    false_value: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub fn project_fixed_boolean_predicates(
    ctx: &DecodeContext<'_>,
    source: &FixedBooleanPredicates,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "fixed boolean predicates")?);
    Ok(())
}

#[derive(Serialize)]
struct DistinctMembersPredicate {
    #[serde(skip_serializing_if = "DistinctMembers::is_empty")]
    members: DistinctMembers<u32>,
}

pub fn project_distinct_members_predicate(
    ctx: &DecodeContext<'_>,
    source: &DistinctMembersPredicate,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "distinct members predicate")?);
    Ok(())
}

#[derive(Serialize)]
struct CacheContractPredicate {
    #[serde(skip_serializing_if = "CacheContract::is_bare_legacy")]
    contract: CacheContract<u32>,
}

pub fn project_cache_contract_predicate(
    ctx: &DecodeContext<'_>,
    source: &CacheContractPredicate,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "cache contract predicate")?);
    Ok(())
}

#[derive(Serialize)]
struct SketchProfilesPredicate {
    #[serde(skip_serializing_if = "SketchProfiles::is_empty")]
    profiles: SketchProfiles,
}

pub fn project_sketch_profiles_predicate(
    ctx: &DecodeContext<'_>,
    source: &SketchProfilesPredicate,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "sketch profiles predicate")?);
    Ok(())
}

fn scans_text(value: &String) -> bool {
    for character in value.chars() {
        std::hint::black_box(character);
    }
    false
}

#[derive(Serialize)]
struct ScanningPredicate {
    #[serde(skip_serializing_if = "scans_text")]
    text: String,
}

pub fn project_scanning_predicate(
    ctx: &DecodeContext<'_>,
    source: &ScanningPredicate,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "scanning predicate")?); // finding: unproven_decode_charge
    Ok(())
}

fn allocating_predicate(value: &bool) -> bool {
    let copy = Box::new(*value);
    *copy
}

#[derive(Serialize)]
struct AllocatingPredicate {
    #[serde(skip_serializing_if = "allocating_predicate")]
    value: bool,
}

pub fn project_allocating_predicate(
    ctx: &DecodeContext<'_>,
    source: &AllocatingPredicate,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "allocating predicate")?); // finding: unproven_decode_charge
    Ok(())
}

mod shadowed_predicate {
    use serde::Serialize;

    pub struct Option;

    impl Option {
        pub fn is_none(value: &String) -> bool {
            for byte in value.as_bytes() {
                std::hint::black_box(byte);
            }
            false
        }
    }

    #[derive(Serialize)]
    pub struct Source {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub text: String,
    }
}

pub fn project_shadowed_predicate(
    ctx: &DecodeContext<'_>,
    source: &shadowed_predicate::Source,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "shadowed predicate")?); // finding: unproven_decode_charge
    Ok(())
}
