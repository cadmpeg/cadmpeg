// SPDX-License-Identifier: Apache-2.0
pub mod decode {
    pub mod scan {
        pub struct AdmittedIter<I>(pub(crate) I);

        impl<I: Iterator> Iterator for AdmittedIter<I> {
            type Item = I::Item;

            fn next(&mut self) -> Option<Self::Item> {
                self.0.next()
            }
        }

        impl<I: DoubleEndedIterator> DoubleEndedIterator for AdmittedIter<I> {
            fn next_back(&mut self) -> Option<Self::Item> {
                self.0.next_back()
            }
        }

        impl<I: ExactSizeIterator> ExactSizeIterator for AdmittedIter<I> {}
    }
}

pub struct DecodeContext;

impl DecodeContext {
    pub fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }

    pub fn admit_iter<I: IntoIterator>(
        &self,
        source: I,
        _operation: &str,
    ) -> Result<decode::scan::AdmittedIter<I::IntoIter>, ()> {
        Ok(decode::scan::AdmittedIter(source.into_iter()))
    }
}

pub fn raw_exclusive_ranges(
    _ctx: &DecodeContext,
    start_u8: u8,
    end_u8: u8,
    start_u16: u16,
    end_u16: u16,
    start_u32: u32,
    end_u32: u32,
    start_u64: u64,
    end_u64: u64,
    start_u128: u128,
    end_u128: u128,
    start_usize: usize,
    end_usize: usize,
) {
    // replacement: admit_iter
    for value in start_u8..end_u8 {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in start_u16..end_u16 {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in start_u32..end_u32 {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in start_u64..end_u64 {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in start_u128..end_u128 {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in start_usize..end_usize {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
}

pub fn raw_inclusive_ranges(
    _ctx: &DecodeContext,
    start_u8: u8,
    end_u8: u8,
    start_u16: u16,
    end_u16: u16,
    start_u32: u32,
    end_u32: u32,
    start_u64: u64,
    end_u64: u64,
    start_u128: u128,
    end_u128: u128,
    start_usize: usize,
    end_usize: usize,
) {
    // replacement: admit_iter
    for value in std::ops::RangeInclusive::new(start_u8, end_u8) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in std::ops::RangeInclusive::new(start_u16, end_u16) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in std::ops::RangeInclusive::new(start_u32, end_u32) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in std::ops::RangeInclusive::new(start_u64, end_u64) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in std::ops::RangeInclusive::new(start_u128, end_u128) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    // replacement: admit_iter
    for value in std::ops::RangeInclusive::new(start_usize, end_usize) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
}

pub fn admitted_ranges(ctx: &DecodeContext, end: u32, end_u16: u16) -> Result<(), ()> {
    for value in ctx.admit_iter(0u32..end, "exclusive range")? {
        std::hint::black_box(value);
    }

    let admitted = ctx.admit_iter(0u16..=end_u16, "inclusive range")?;
    for (index, value) in admitted
        .rev()
        .enumerate()
        .filter(|(index, value)| *index == 0 || *value != 0)
    {
        std::hint::black_box((index, value));
    }

    Ok(())
}

pub fn fixed_constant_range() {
    for value in 2u8..7u8 {
        std::hint::black_box(value);
    }
}

pub fn standard_inclusive_end_is_fixed(_ctx: &DecodeContext, start: u32, end: u32) {
    let range = std::ops::RangeInclusive::new(start, end);
    std::hint::black_box(range.end());
}

pub trait CustomRangeEnd {
    fn end(&self, capacity: usize) -> Vec<u8>;
}

struct AllocatingCustomRangeEnd;

impl CustomRangeEnd for AllocatingCustomRangeEnd {
    fn end(&self, capacity: usize) -> Vec<u8> {
        Vec::with_capacity(capacity)
    }
}

pub fn custom_allocating_end_remains_unproven(_ctx: &DecodeContext, capacity: usize) {
    let source = AllocatingCustomRangeEnd;
    let source: &dyn CustomRangeEnd = &source;
    std::hint::black_box(source.end(capacity)); // finding: unproven_decode_charge
}

pub fn exact_manual_receipt(ctx: &DecodeContext, count: u32) -> Result<(), ()> {
    ctx.charge_work(u64::from(count), "range")?;
    for value in 0u32..count {
        std::hint::black_box(value);
    }
    Ok(())
}

pub fn wrong_source_receipt(
    ctx: &DecodeContext,
    charged_count: u32,
    range_count: u32,
) -> Result<(), ()> {
    ctx.charge_work(u64::from(charged_count), "wrong source")?;
    // replacement: admit_iter
    for value in 0u32..range_count {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    Ok(())
}

pub fn narrowed_width_receipt(ctx: &DecodeContext, count: u32) -> Result<(), ()> {
    let narrowed = count as u16;
    ctx.charge_work(u64::from(narrowed), "narrowed width")?;
    // replacement: admit_iter
    for value in 0u32..count {
        // finding: unproven_decode_charge
        std::hint::black_box(value);
    }
    Ok(())
}

pub fn unrelated_admitted_source(
    ctx: &DecodeContext,
    unrelated_count: u32,
    range_count: u32,
) -> Result<(), ()> {
    let _admitted = ctx.admit_iter(0u32..unrelated_count, "unrelated range")?;
    // replacement: admit_iter
    for value in 0u32..range_count {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    Ok(())
}

pub fn wrong_inclusive_bound(ctx: &DecodeContext, count: u32) -> Result<(), ()> {
    ctx.charge_work(u64::from(count), "exclusive bound")?;
    // replacement: admit_iter
    for value in std::ops::RangeInclusive::new(0u32, count) {
        // finding: uncharged_decode_work
        std::hint::black_box(value);
    }
    Ok(())
}

const SIGNED_NEGATIVE: i32 = -1;

struct SignedBounds;

impl SignedBounds {
    const NEGATIVE: i32 = -1;
    const POSITIVE: i32 = 3;
}

#[cfg(target_pointer_width = "64")]
pub fn negative_associated_count_cannot_shrink_widened_visits(
    ctx: &DecodeContext,
    input: &[u8],
) -> Result<(), ()> {
    let work = 4_294_967_295u64
        .checked_mul(input.len() as u64)
        .ok_or_else(|| ())?;
    ctx.charge_work(work, "signed associated count")?;
    for _ in 0..SignedBounds::NEGATIVE as usize {
        for value in input {
            // finding: uncharged_decode_work
            std::hint::black_box(value);
        }
    }
    Ok(())
}

#[cfg(target_pointer_width = "64")]
pub fn negative_named_count_cannot_shrink_widened_visits(
    ctx: &DecodeContext,
    input: &[u8],
) -> Result<(), ()> {
    let work = 4_294_967_295u64
        .checked_mul(input.len() as u64)
        .ok_or_else(|| ())?;
    ctx.charge_work(work, "signed named count")?;
    for _ in 0..SIGNED_NEGATIVE as usize {
        for value in input {
            // finding: uncharged_decode_work
            std::hint::black_box(value);
        }
    }
    Ok(())
}

pub fn positive_associated_count_preserves_exact_visits(
    ctx: &DecodeContext,
    input: &[u8],
) -> Result<(), ()> {
    let work = 3u64.checked_mul(input.len() as u64).ok_or_else(|| ())?;
    ctx.charge_work(work, "positive associated count")?;
    for _ in 0..SignedBounds::POSITIVE as usize {
        for value in input {
            std::hint::black_box(value);
        }
    }
    Ok(())
}

pub fn vector_range_adapter_requires_visits(_ctx: &DecodeContext, end: usize) {
    let mut visits = Vec::<()>::new();
    visits.extend((0..end).map(|_| ())); // finding: uncharged_decode_work
    std::hint::black_box(visits);
}

pub fn vector_admitted_range_adapter_keeps_visits(
    ctx: &DecodeContext,
    end: usize,
) -> Result<(), ()> {
    let source = ctx.admit_iter(0..end, "vector range traversal")?;
    let mut visits = Vec::<()>::new();
    visits.extend(source.map(|_| ()));
    std::hint::black_box(visits);
    Ok(())
}
