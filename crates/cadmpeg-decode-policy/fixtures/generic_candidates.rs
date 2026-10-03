// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
struct ConversionError;
struct OtherError;
struct Matching;
impl<T> TryFrom<Vec<T>> for Matching {
    type Error = ConversionError;
    fn try_from(values: Vec<T>) -> Result<Self, Self::Error> {
        for value in &values {
            // finding: uncharged_decode_work
            std::hint::black_box(value);
        }
        Ok(Matching)
    }
}
struct WrongInput;
impl TryFrom<&str> for WrongInput {
    type Error = ConversionError;
    fn try_from(text: &str) -> Result<Self, Self::Error> {
        for byte in text.bytes() {
            std::hint::black_box(byte);
        }
        Ok(WrongInput)
    }
}
struct WrongOutput;
impl<T> TryFrom<Vec<T>> for WrongOutput {
    type Error = OtherError;
    fn try_from(values: Vec<T>) -> Result<Self, Self::Error> {
        for value in &values {
            std::hint::black_box(value);
        }
        Ok(WrongOutput)
    }
}
pub fn convert<T, C: TryFrom<Vec<T>, Error = ConversionError>>(
    ctx: &DecodeContext,
    values: Vec<T>,
) -> Result<C, ConversionError> {
    let _ctx = ctx;
    C::try_from(values) // finding: unproven_decode_charge
}
static SAME: fn(u8, u8) -> u8 = same;
static DIFFERENT: fn(u8, u16) -> u8 = different;
fn same(left: u8, right: u8) -> u8 {
    for _ in 0..left {
        // finding: uncharged_decode_work
        std::hint::black_box(right);
    }
    right
}
fn different(left: u8, right: u16) -> u8 {
    for _ in 0..left {
        std::hint::black_box(right);
    }
    left
}
pub fn pointer<T: Copy>(ctx: &DecodeContext, callback: fn(T, T) -> T, value: T) -> T {
    let _ctx = ctx;
    let _repeat = repeat(callback, value);
    callback(value, value) // finding: unproven_decode_charge
}
trait Object<T> {
    fn read(&self, bytes: &[u8]);
}
struct Reader;
impl Object<u8> for Reader {
    fn read(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
impl Object<u16> for Reader {
    fn read(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
pub fn decode(ctx: &DecodeContext, object: &dyn Object<u8>, bytes: &[u8]) {
    let _ctx = ctx;
    let _other: &dyn Object<u16> = &Reader;
    object.read(bytes); // finding: unproven_decode_charge
    dynamic(object, bytes);
}

trait Deferred {
    fn read(&self, bytes: &[u8]);
}
trait Work {
    fn work(&self, bytes: &[u8]);
}
struct Inner;
impl Work for Inner {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
struct Loose<T>(T);
impl<T: Work> Deferred for Loose<T> {
    fn read(&self, bytes: &[u8]) {
        self.0.work(bytes);
    }
}
pub fn deferred(ctx: &DecodeContext, object: &dyn Deferred, bytes: &[u8]) {
    let _ctx = ctx;
    object.read(bytes); // finding: unproven_decode_charge
}

fn repeat<T: Copy>(callback: fn(T, T) -> T, value: T) -> T {
    let invoke = || callback(value, value); // finding: unproven_decode_charge
    invoke()
}

fn dynamic<T>(object: &dyn Object<T>, bytes: &[u8]) {
    object.read(bytes); // finding: unproven_decode_charge
}
