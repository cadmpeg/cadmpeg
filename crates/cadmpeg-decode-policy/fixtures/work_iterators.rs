// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
    fn next_charged<I: Iterator>(
        &self,
        input: &mut I,
        operation: &str,
    ) -> Result<Option<I::Item>, ()> {
        self.charge_work(1, operation)?;
        Ok(input.next())
    }
    fn consume<I: IntoIterator>(&self, source: I) -> Result<(), ()> {
        let mut input = source.into_iter();
        let _hint = input.size_hint();
        loop {
            let Some(_value) = self.next_charged(&mut input, "step")? else {
                break;
            };
        }
        Ok(())
    }
}
pub mod decode {
    pub mod scan {
        pub struct AdmittedIter<I> {
            pub source: I,
        }
        impl<I: Iterator> Iterator for AdmittedIter<I> {
            type Item = I::Item;
            fn next(&mut self) -> Option<I::Item> {
                self.source.next()
            }
        }
    }
}
pub fn decode(
    ctx: &DecodeContext,
    values: &[u8],
    admitted: decode::scan::AdmittedIter<std::slice::Iter<'_, u8>>,
) -> Result<(), ()> {
    ctx.consume(values.iter().copied())?;
    ctx.consume(values.iter().map(|value| *value))?;
    ctx.consume(values.iter().filter(|value| **value != 0))?; // finding: unproven_decode_charge
    ctx.consume(admitted.filter(|value| **value != 0))?;
    Ok(())
}
pub fn opaque<I: Iterator>(ctx: &DecodeContext, input: I) -> Result<(), ()> {
    ctx.consume(input)?;
    Ok(())
}
pub fn raw_next<I: Iterator>(ctx: &DecodeContext, input: &mut I) {
    let _ctx = ctx;
    let _value = input.next(); // finding: unproven_decode_charge
}

impl DecodeContext {
    fn consume_while<I: IntoIterator>(&self, source: I) -> Result<(), ()> {
        let mut input = source.into_iter();
        while let Some(_value) = self.next_charged(&mut input, "step")? {}
        Ok(())
    }
}
pub fn charged_while(ctx: &DecodeContext, values: &[u8]) -> Result<(), ()> {
    ctx.consume_while(values.iter().copied())?;
    ctx.consume_while(values.iter().filter(|value| **value != 0))?; // finding: unproven_decode_charge
    let mut input = values.iter();
    while let Some(_value) = ctx.next_charged(&mut input, "step")? {
        for value in values {
            // finding: uncharged_decode_work
            std::hint::black_box(value);
        }
    }
    Ok(())
}
pub fn raw_while(ctx: &DecodeContext, values: &[u8]) {
    let _ctx = ctx;
    let mut input = values.iter();
    while let Some(_value) = input.next() {
        // finding: uncharged_decode_work
    }
}
pub fn ignored_charge(ctx: &DecodeContext, values: &[u8]) {
    let mut input = values.iter();
    while let Some(Some(_value)) = ctx.next_charged(&mut input, "step").ok() {
        // finding: uncharged_decode_work
    }
}
struct Other;
impl Other {
    fn next_charged<I: Iterator>(&self, input: &mut I) -> Result<Option<I::Item>, ()> {
        Ok(input.next())
    }
}
pub fn other_charge(ctx: &DecodeContext, values: &[u8]) -> Result<(), ()> {
    let _ctx = ctx;
    let mut input = values.iter();
    while let Some(_value) = Other.next_charged(&mut input)? {
        // finding: uncharged_decode_work
    }
    Ok(())
}
