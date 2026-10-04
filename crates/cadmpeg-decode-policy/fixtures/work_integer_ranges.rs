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
