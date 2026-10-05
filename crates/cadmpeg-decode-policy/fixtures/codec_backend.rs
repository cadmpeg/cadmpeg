// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;

impl DecodeContext {
    pub fn charge_work(&self, _units: u64, _operation: &'static str) -> Result<(), ()> {
        Ok(())
    }
}

pub mod codec {
    use super::DecodeContext;

    pub trait CodecBackend {
        fn detect_impl(&self, ctx: &DecodeContext, prefix: &[u8]) -> Result<bool, ()>;
        fn decode_impl(&self, ctx: &DecodeContext, root: &[u8]) -> Result<usize, ()>;
        fn validate_native(_ctx: &DecodeContext, _root: &[u8]) -> Result<usize, ()> {
            Ok(0)
        }
    }

    pub trait Codec {
        fn detect(&self, ctx: &DecodeContext, prefix: &[u8]) -> Result<bool, ()>;
        fn decode(&self, ctx: &DecodeContext, root: &[u8]) -> Result<usize, ()>;
        fn validate(&self, ctx: &DecodeContext, root: &[u8]) -> Result<usize, ()>;
    }

    // Every backend implementation is checked as its own decode entry point.
    impl<C: CodecBackend + ?Sized> Codec for C {
        fn detect(&self, ctx: &DecodeContext, prefix: &[u8]) -> Result<bool, ()> {
            self.detect_impl(ctx, prefix)
        }
        fn decode(&self, ctx: &DecodeContext, root: &[u8]) -> Result<usize, ()> {
            self.decode_impl(ctx, root)
        }
        fn validate(&self, ctx: &DecodeContext, root: &[u8]) -> Result<usize, ()> {
            C::validate_native(ctx, root)
        }
    }

    pub trait Other {
        fn scan(&self, ctx: &DecodeContext, root: &[u8]) -> Result<usize, ()>;
    }

    pub fn other<T: Other>(ctx: &DecodeContext, value: &T, root: &[u8]) -> Result<usize, ()> {
        value.scan(ctx, root) // finding: unproven_decode_charge
    }
}

pub struct Backend;

impl codec::CodecBackend for Backend {
    fn detect_impl(&self, ctx: &DecodeContext, prefix: &[u8]) -> Result<bool, ()> {
        ctx.charge_work(1, "detect")?;
        Ok(prefix.first() == Some(&0))
    }
    fn decode_impl(&self, _ctx: &DecodeContext, root: &[u8]) -> Result<usize, ()> {
        Ok(root.iter().filter(|byte| **byte == 0).count()) // finding: unproven_decode_charge
    }
}
