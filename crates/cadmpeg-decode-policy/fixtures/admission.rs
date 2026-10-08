// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::admission::{Admission, AdmissionScope, StandardAdmission};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

// Shared code generic over the admission is checked as the core operations
// its calls forward to.
pub fn shared<A: Admission>(
    admission: &A,
    name: &str,
    groups: &mut BTreeMap<u32, Vec<u32>>,
) -> Result<String, A::Error> {
    admission.retain_btree_map(
        groups,
        |_, members| {
            admission.charge_work(1, "keep")?;
            Ok(!members.is_empty())
        },
        "retain",
    )?;
    let _matches = admission.equal_bytes(name.as_bytes(), b"name", "name comparison")?;
    let mut scope = admission.reserve_scoped(0, "scope")?;
    scope.with_storage(|| admission.format_retained(format_args!("{name}"), "text"))
}

pub fn decode(
    ctx: &DecodeContext<'_>,
    groups: &mut BTreeMap<u32, Vec<u32>>,
) -> Result<String, CodecError> {
    let mut values = [2_u32, 1];
    let _ = complete_operations(ctx, &mut values)?;
    shared(ctx, "name", groups)
}

pub fn complete_operations<A: Admission>(
    admission: &A,
    values: &mut [u32],
) -> Result<(bool, bool), A::Error> {
    let _depth = admission.enter_nested("nested")?;
    let was_active = admission.resource_refusal().is_none();
    let sorted = admission.is_sorted_by(values, |value| value, Ord::cmp, "sorted")?;
    admission.stable_sort_by(values, |value| value, Ord::cmp, "sort")?;
    Ok((was_active, sorted))
}

#[derive(PartialEq, Eq)]
pub struct CustomHashKey(String);

impl Hash for CustomHashKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            state.write_u8(byte);
            state.write_u8(byte);
        }
    }
}

impl DecodeCost for CustomHashKey {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(u64_from_index(self.0.len()))
    }
}

// The forwarded lookup keeps the core operation's key proof.
pub fn custom_key<A: Admission>(
    admission: &A,
    values: &HashMap<CustomHashKey, u32>,
    query: &CustomHashKey,
) -> Result<bool, A::Error> {
    let found = admission // finding: unproven_decode_charge
        .get_hash_map(values, query, "custom key")?
        .is_some();
    Ok(found)
}

pub fn passed_standard<'values>(
    admission: &StandardAdmission,
    names: &'values HashMap<String, u32>,
) -> Result<Option<&'values u32>, std::convert::Infallible> {
    admission.get_hash_map(names, "name", "lookup") // finding: uncharged_decode_work
}

// The standard admission charges nothing, so decode code does not use it.
pub fn uncharged(
    ctx: &DecodeContext<'_>,
    standard: &StandardAdmission,
    names: &HashMap<String, u32>,
    values: &HashMap<CustomHashKey, u32>,
    query: &CustomHashKey,
) -> Result<Option<u32>, CodecError> {
    let mut groups = BTreeMap::new();
    let _shared = shared(standard, "name", &mut groups); // finding: uncharged_decode_work
    let _passed = passed_standard(standard, names);
    let admission = StandardAdmission::default(); // finding: uncharged_decode_work
    let Ok(found) = admission.get_hash_map(names, "name", "lookup"); // finding: uncharged_decode_work
    let direct = ctx.get_hash_map(names, "name", "direct")?.copied();
    let _custom = custom_key(ctx, values, query)?; // finding: unproven_decode_charge
    Ok(found.copied().or(direct))
}
