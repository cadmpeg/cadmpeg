// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::HashSet;

pub fn vector_shapes(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let mut values = ctx.collection_vec(0, "vector")?;
    ctx.extend_vec(&mut values, Some(1_u8), "optional value")?;
    ctx.extend_vec(&mut values, [2_u8, 3], "array values")?;
    ctx.extend_vec(&mut values, bytes, "borrowed values")?;
    ctx.insert_vec(&mut values, 0, 0, "indexed value")?;
    let mut storage = ctx.reserve_scoped(0, "temporary vector")?;
    let mut temporary = Vec::new();
    storage.with_storage(|| ctx.insert_vec(&mut temporary, 0, 1_u8, "temporary indexed value"))?;
    Ok(())
}

pub fn scoped_retained_children(ctx: &DecodeContext<'_>, texts: &[String]) -> Result<(), CodecError> {
    let source = ctx.admit_iter(texts, "text source")?;
    let children = source.map(|text| ctx.copy_retained_text(text, "retained child"));
    let (_values, _storage) = ctx.try_collect_scoped_vec(children, "temporary children")?;
    Ok(())
}

pub fn owned_key_costs(
    ctx: &DecodeContext<'_>,
    left: &cadmpeg_core::dialect::DialectId,
    right: &cadmpeg_core::dialect::DialectId,
    finding: &cadmpeg_ir::report::check::Finding,
    other_finding: &cadmpeg_ir::report::check::Finding,
) -> Result<(), CodecError> {
    let _equal = ctx.equal(left, right, "dialect identifiers")?;
    let _equal = ctx.equal(&finding.check, &other_finding.check, "validation checks")?;
    Ok(())
}

pub struct ScanningEqualityKey(String);

impl PartialEq for ScanningEqualityKey {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }
        for (left, right) in self.0.bytes().zip(other.0.bytes()) { // finding: uncharged_decode_work
            if left != right {
                return false;
            }
        }
        true
    }
}

pub fn raw_custom_equality(
    _ctx: &DecodeContext<'_>,
    left: &ScanningEqualityKey,
    right: &ScanningEqualityKey,
) -> bool {
    left.eq(right)
}

impl cadmpeg_core::decode::cost::DecodeCost for ScanningEqualityKey {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

pub fn custom_equality_callback_remains_unproven(
    ctx: &DecodeContext<'_>,
    left: &ScanningEqualityKey,
    right: &ScanningEqualityKey,
) -> Result<(), CodecError> {
    let _equal = ctx.equal(left, right, "custom equality callback")?; // finding: unproven_decode_charge
    Ok(())
}

pub struct LookupCostKey<'a> {
    code: u8,
    lookup: &'a HashSet<String>,
    needle: &'a str,
}

impl std::hash::Hash for LookupCostKey<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(self.code);
    }
}

impl PartialEq for LookupCostKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
    }
}

impl Eq for LookupCostKey<'_> {}

impl cadmpeg_core::decode::cost::DecodeCost for LookupCostKey<'_> {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        let _found = self.lookup.contains(self.needle); // finding: uncharged_decode_work
        Ok(1)
    }
}

pub fn set_cost_callback(
    ctx: &DecodeContext<'_>,
    values: &HashSet<LookupCostKey<'_>>,
    key: &LookupCostKey<'_>,
) -> Result<(), CodecError> {
    let _found = ctx.contains_hash_set(values, key, "custom cost callback")?; // finding: unproven_decode_charge
    Ok(())
}
