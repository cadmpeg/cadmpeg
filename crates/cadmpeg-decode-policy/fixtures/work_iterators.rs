// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
    fn next_charged<I: Iterator>(&self, input: &mut I, operation: &str) -> Result<Option<I::Item>, ()> {
        self.charge_work(1, operation)?;
        Ok(input.next())
    }
    fn consume<I: IntoIterator>(&self, source: I) -> Result<(), ()> {
        let mut input = source.into_iter();
        let _hint = input.size_hint();
        loop {
            let Some(_value) = self.next_charged(&mut input, "step")? else { break };
        }
        Ok(())
    }
}
pub mod decode { pub mod scan {
    pub struct AdmittedIter<I> { pub source: I }
    impl<I: Iterator> Iterator for AdmittedIter<I> {
        type Item = I::Item;
        fn next(&mut self) -> Option<I::Item> { self.source.next() }
    }
} }
pub fn decode(ctx: &DecodeContext, values: &[u8], admitted: decode::scan::AdmittedIter<std::slice::Iter<'_, u8>>) -> Result<(), ()> {
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
