// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;

fn leaf<T, const N: usize>(_context: &DecodeContext) {
    let _marker = std::marker::PhantomData::<(T, [u8; N])>;
}

pub fn fanout<T>(context: &DecodeContext) {
    leaf::<T, 0>(context);
    leaf::<T, 1>(context);
    leaf::<T, 2>(context);
    leaf::<T, 3>(context);
    leaf::<T, 4>(context);
    leaf::<T, 5>(context);
    leaf::<T, 6>(context);
    leaf::<T, 7>(context);
    leaf::<T, 8>(context);
    leaf::<T, 9>(context);
    leaf::<T, 10>(context);
    leaf::<T, 11>(context);
    leaf::<T, 12>(context);
    leaf::<T, 13>(context);
    leaf::<T, 14>(context);
    leaf::<T, 15>(context);
}

pub fn repeated<T>(context: &DecodeContext) {
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
    leaf::<T, 0>(context);
}

pub fn deep<T>(context: &DecodeContext) {
    depth_a::<T>(context);
}

fn depth_a<T>(context: &DecodeContext) {
    depth_b::<T>(context);
}

fn depth_b<T>(context: &DecodeContext) {
    depth_c::<T>(context);
}

fn depth_c<T>(context: &DecodeContext) {
    depth_d::<T>(context);
}

fn depth_d<T>(context: &DecodeContext) {
    depth_e::<T>(context);
}

fn depth_e<T>(context: &DecodeContext) {
    depth_f::<T>(context);
}

fn depth_f<T>(context: &DecodeContext) {
    leaf::<T, 0>(context);
}
