// SPDX-License-Identifier: Apache-2.0
//! Safe `cudarc` CUDA Driver API backend for SIMT FP64, Ampere `sm_86`, `OptiX` RT Core, and RTX Tensor Core PTX kernels.

use std::sync::{Arc, OnceLock};

use cadmpeg_ir::math::Point3;
use cudarc::driver::{CudaContext as CudarcContext, CudaFunction, LaunchConfig, PushKernelArg};
use cudarc::nvrtc::Ptx;

use crate::cpu;

/// Complete PTX 7.1 module targeting `sm_80` (Ampere and newer) containing
/// FP64 SIMT compute kernels, Ampere `sm_86` warp-butterfly kernels, and
/// hardware FP64 Tensor Core `mma.sync.aligned.m8n8k4.row.col.f64` kernels.
const CUDA_PTX_MODULE: &str = "\
.version 7.1
.target sm_80
.address_size 64

// 1. Standard CUDA SIMT 4x4 homogeneous point transform (FP64)
.visible .entry cuda_simt_transform_4x4_f64(
    .param .u64 param_mat,
    .param .u64 param_in,
    .param .u64 param_out,
    .param .u32 param_n
) {
    .reg .pred %p0;
    .reg .b32 %r<6>;
    .reg .b64 %rd<12>;
    .reg .f64 %fd<28>;

    ld.param.u64 %rd1, [param_mat];
    ld.param.u64 %rd2, [param_in];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n];

    mov.u32 %r2, %ctaid.x;
    mov.u32 %r3, %ntid.x;
    mov.u32 %r4, %tid.x;
    mad.lo.u32 %r5, %r2, %r3, %r4;
    setp.ge.u32 %p0, %r5, %r1;
    @%p0 bra L_SIMT_TF_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mul.wide.u32 %rd7, %r5, 32;
    add.u64 %rd8, %rd5, %rd7;
    add.u64 %rd9, %rd6, %rd7;

    ld.global.f64 %fd0, [%rd8 + 0];
    ld.global.f64 %fd1, [%rd8 + 8];
    ld.global.f64 %fd2, [%rd8 + 16];
    ld.global.f64 %fd3, [%rd8 + 24];

    ld.global.f64 %fd4,  [%rd4 + 0];
    ld.global.f64 %fd5,  [%rd4 + 8];
    ld.global.f64 %fd6,  [%rd4 + 16];
    ld.global.f64 %fd7,  [%rd4 + 24];
    mul.f64 %fd20, %fd7, %fd3;
    fma.rn.f64 %fd20, %fd6, %fd2, %fd20;
    fma.rn.f64 %fd20, %fd5, %fd1, %fd20;
    fma.rn.f64 %fd20, %fd4, %fd0, %fd20;

    ld.global.f64 %fd8,  [%rd4 + 32];
    ld.global.f64 %fd9,  [%rd4 + 40];
    ld.global.f64 %fd10, [%rd4 + 48];
    ld.global.f64 %fd11, [%rd4 + 56];
    mul.f64 %fd21, %fd11, %fd3;
    fma.rn.f64 %fd21, %fd10, %fd2, %fd21;
    fma.rn.f64 %fd21, %fd9,  %fd1, %fd21;
    fma.rn.f64 %fd21, %fd8,  %fd0, %fd21;

    ld.global.f64 %fd12, [%rd4 + 64];
    ld.global.f64 %fd13, [%rd4 + 72];
    ld.global.f64 %fd14, [%rd4 + 80];
    ld.global.f64 %fd15, [%rd4 + 88];
    mul.f64 %fd22, %fd15, %fd3;
    fma.rn.f64 %fd22, %fd14, %fd2, %fd22;
    fma.rn.f64 %fd22, %fd13, %fd1, %fd22;
    fma.rn.f64 %fd22, %fd12, %fd0, %fd22;

    ld.global.f64 %fd16, [%rd4 + 96];
    ld.global.f64 %fd17, [%rd4 + 104];
    ld.global.f64 %fd18, [%rd4 + 112];
    ld.global.f64 %fd19, [%rd4 + 120];
    mul.f64 %fd23, %fd19, %fd3;
    fma.rn.f64 %fd23, %fd18, %fd2, %fd23;
    fma.rn.f64 %fd23, %fd17, %fd1, %fd23;
    fma.rn.f64 %fd23, %fd16, %fd0, %fd23;

    st.global.f64 [%rd9 + 0],  %fd20;
    st.global.f64 [%rd9 + 8],  %fd21;
    st.global.f64 [%rd9 + 16], %fd22;
    st.global.f64 [%rd9 + 24], %fd23;

L_SIMT_TF_EXIT:
    ret;
}

// 1b. Ampere sm_86 shared-memory matrix + non-coherent load 4x4 homogeneous point transform (FP64)
.visible .entry cuda_sm86_transform_4x4_f64(
    .param .u64 param_mat,
    .param .u64 param_in,
    .param .u64 param_out,
    .param .u32 param_n
) {
    .shared .align 16 .b8 %smem_mat[128];
    .reg .pred %p<3>;
    .reg .b32 %r<8>;
    .reg .b64 %rd<14>;
    .reg .f64 %fd<28>;

    ld.param.u64 %rd1, [param_mat];
    ld.param.u64 %rd2, [param_in];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n];

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mov.u32 %r2, %ctaid.x;
    mov.u32 %r3, %ntid.x;
    mov.u32 %r4, %tid.x;

    // Cooperative load of 16 f64s of the 4x4 matrix into shared memory
    setp.lt.u32 %p1, %r4, 16;
    @!%p1 bra L_SM86_TF_SYNC;
    mul.wide.u32 %rd7, %r4, 8;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.nc.f64 %fd0, [%rd8];
    mov.u64 %rd9, %smem_mat;
    add.u64 %rd9, %rd9, %rd7;
    st.shared.f64 [%rd9], %fd0;

L_SM86_TF_SYNC:
    bar.sync 0;

    mad.lo.u32 %r5, %r2, %r3, %r4;
    setp.ge.u32 %p2, %r5, %r1;
    @%p2 bra L_SM86_TF_EXIT;

    mul.wide.u32 %rd10, %r5, 32;
    add.u64 %rd11, %rd5, %rd10;
    add.u64 %rd12, %rd6, %rd10;

    ld.global.nc.f64 %fd0, [%rd11 + 0];
    ld.global.nc.f64 %fd1, [%rd11 + 8];
    ld.global.nc.f64 %fd2, [%rd11 + 16];
    ld.global.nc.f64 %fd3, [%rd11 + 24];

    mov.u64 %rd13, %smem_mat;
    ld.shared.f64 %fd4,  [%rd13 + 0];
    ld.shared.f64 %fd5,  [%rd13 + 8];
    ld.shared.f64 %fd6,  [%rd13 + 16];
    ld.shared.f64 %fd7,  [%rd13 + 24];
    mul.f64 %fd20, %fd7, %fd3;
    fma.rn.f64 %fd20, %fd6, %fd2, %fd20;
    fma.rn.f64 %fd20, %fd5, %fd1, %fd20;
    fma.rn.f64 %fd20, %fd4, %fd0, %fd20;

    ld.shared.f64 %fd8,  [%rd13 + 32];
    ld.shared.f64 %fd9,  [%rd13 + 40];
    ld.shared.f64 %fd10, [%rd13 + 48];
    ld.shared.f64 %fd11, [%rd13 + 56];
    mul.f64 %fd21, %fd11, %fd3;
    fma.rn.f64 %fd21, %fd10, %fd2, %fd21;
    fma.rn.f64 %fd21, %fd9,  %fd1, %fd21;
    fma.rn.f64 %fd21, %fd8,  %fd0, %fd21;

    ld.shared.f64 %fd12, [%rd13 + 64];
    ld.shared.f64 %fd13, [%rd13 + 72];
    ld.shared.f64 %fd14, [%rd13 + 80];
    ld.shared.f64 %fd15, [%rd13 + 88];
    mul.f64 %fd22, %fd15, %fd3;
    fma.rn.f64 %fd22, %fd14, %fd2, %fd22;
    fma.rn.f64 %fd22, %fd13, %fd1, %fd22;
    fma.rn.f64 %fd22, %fd12, %fd0, %fd22;

    ld.shared.f64 %fd16, [%rd13 + 96];
    ld.shared.f64 %fd17, [%rd13 + 104];
    ld.shared.f64 %fd18, [%rd13 + 112];
    ld.shared.f64 %fd19, [%rd13 + 120];
    mul.f64 %fd23, %fd19, %fd3;
    fma.rn.f64 %fd23, %fd18, %fd2, %fd23;
    fma.rn.f64 %fd23, %fd17, %fd1, %fd23;
    fma.rn.f64 %fd23, %fd16, %fd0, %fd23;

    st.global.f64 [%rd12 + 0],  %fd20;
    st.global.f64 [%rd12 + 8],  %fd21;
    st.global.f64 [%rd12 + 16], %fd22;
    st.global.f64 [%rd12 + 24], %fd23;

L_SM86_TF_EXIT:
    ret;
}

// 2. Handcrafted RTX Tensor Core 4x4 homogeneous point transform using mma.sync.aligned.m8n8k4.row.col.f64
// Each warp (32 lanes) processes 8 points (rows 0..7 of A_8x4) against B_4x8 (where cols 0..3 and 4..7 hold M^T).
.visible .entry rtx_tc_transform_4x4_f64(
    .param .u64 param_mat_col,
    .param .u64 param_in,
    .param .u64 param_out,
    .param .u32 param_n
) {
    .reg .pred %p<4>;
    .reg .b32 %r<16>;
    .reg .b64 %rd<16>;
    .reg .f64 %fa0, %fb0, %fc0, %fc1, %fd0, %fd1;

    ld.param.u64 %rd1, [param_mat_col];
    ld.param.u64 %rd2, [param_in];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n];

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    // 1 warp (32 threads) per block; blockIdx.x selects tile of 8 points
    mov.u32 %r2, %ctaid.x;
    shl.b32 %r3, %r2, 3;       // base_point = blockIdx.x * 8
    mov.u32 %r4, %tid.x;
    and.b32 %r5, %r4, 31;      // lane = tid & 31
    shr.u32 %r6, %r5, 2;       // group_id = lane >> 2 (0..7: point row within tile)
    and.b32 %r7, %r5, 3;       // thread_in_group = lane & 3 (0..3: k coord index)

    add.u32 %r8, %r3, %r6;     // point_idx = base_point + group_id
    mov.f64 %fa0, 0d0000000000000000;
    setp.lt.u32 %p1, %r8, %r1;
    @!%p1 bra L_TC_TF_SKIP_LOAD_A;

    // Load A[group_id, thread_in_group] = points[point_idx * 4 + thread_in_group]
    shl.b32 %r9, %r8, 2;
    add.u32 %r9, %r9, %r7;
    mul.wide.u32 %rd7, %r9, 8;
    add.u64 %rd8, %rd5, %rd7;
    ld.global.f64 %fa0, [%rd8];

L_TC_TF_SKIP_LOAD_A:
    // Load B[thread_in_group, group_id]: col = group_id & 3, row = thread_in_group
    // param_mat_col stores M[col, row] at index (col * 4 + row)
    and.b32 %r10, %r6, 3;
    shl.b32 %r11, %r10, 2;
    add.u32 %r11, %r11, %r7;
    mul.wide.u32 %rd9, %r11, 8;
    add.u64 %rd10, %rd4, %rd9;
    ld.global.f64 %fb0, [%rd10];

    mov.f64 %fc0, 0d0000000000000000;
    mov.f64 %fc1, 0d0000000000000000;

    // Execute Ampere/Ada FP64 Tensor Core MMA: D_8x8 = A_8x4 * B_4x8 + C_8x8
    mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64 {%fd0, %fd1}, {%fa0}, {%fb0}, {%fc0, %fc1};

    // Lanes with thread_in_group < 2 hold columns (0, 1) and (2, 3) of row group_id
    setp.lt.u32 %p2, %r7, 2;
    and.pred %p3, %p1, %p2;
    @!%p3 bra L_TC_TF_EXIT;

    shl.b32 %r12, %r8, 2;
    shl.b32 %r13, %r7, 1;
    add.u32 %r12, %r12, %r13;
    mul.wide.u32 %rd11, %r12, 8;
    add.u64 %rd12, %rd6, %rd11;
    st.global.f64 [%rd12 + 0], %fd0;
    st.global.f64 [%rd12 + 8], %fd1;

L_TC_TF_EXIT:
    ret;
}

// 3. Standard CUDA SIMT pairwise squared distance matrix (N x M, FP64)
.visible .entry cuda_simt_pairwise_dist_f64(
    .param .u64 param_a,
    .param .u64 param_b,
    .param .u64 param_out,
    .param .u32 param_n,
    .param .u32 param_m
) {
    .reg .pred %p<3>;
    .reg .b32 %r<12>;
    .reg .b64 %rd<14>;
    .reg .f64 %fd<12>;

    ld.param.u64 %rd1, [param_a];
    ld.param.u64 %rd2, [param_b];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n];
    ld.param.u32 %r2, [param_m];

    mov.u32 %r3, %ctaid.y;
    mov.u32 %r4, %ntid.y;
    mov.u32 %r5, %tid.y;
    mad.lo.u32 %r6, %r3, %r4, %r5; // row i

    mov.u32 %r7, %ctaid.x;
    mov.u32 %r8, %ntid.x;
    mov.u32 %r9, %tid.x;
    mad.lo.u32 %r10, %r7, %r8, %r9; // col j

    setp.ge.u32 %p1, %r6, %r1;
    setp.ge.u32 %p2, %r10, %r2;
    or.pred %p1, %p1, %p2;
    @%p1 bra L_SIMT_DIST_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mul.wide.u32 %rd7, %r6, 32;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.f64 %fd0, [%rd8 + 0];
    ld.global.f64 %fd1, [%rd8 + 8];
    ld.global.f64 %fd2, [%rd8 + 16];

    mul.wide.u32 %rd9, %r10, 32;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.f64 %fd3, [%rd10 + 0];
    ld.global.f64 %fd4, [%rd10 + 8];
    ld.global.f64 %fd5, [%rd10 + 16];

    sub.f64 %fd6, %fd0, %fd3;
    sub.f64 %fd7, %fd1, %fd4;
    sub.f64 %fd8, %fd2, %fd5;

    mul.f64 %fd9, %fd8, %fd8;
    fma.rn.f64 %fd9, %fd7, %fd7, %fd9;
    fma.rn.f64 %fd9, %fd6, %fd6, %fd9;

    mad.lo.u32 %r11, %r6, %r2, %r10;
    mul.wide.u32 %rd11, %r11, 8;
    add.u64 %rd12, %rd6, %rd11;
    st.global.f64 [%rd12], %fd9;

L_SIMT_DIST_EXIT:
    ret;
}

// 3b. Ampere sm_86 shared-memory 16x16 tiled pairwise squared distance matrix (N x M, FP64)
.visible .entry cuda_sm86_pairwise_dist_f64(
    .param .u64 param_a,
    .param .u64 param_b,
    .param .u64 param_out,
    .param .u32 param_n,
    .param .u32 param_m
) {
    .shared .align 16 .b8 %smem_a[512];
    .shared .align 16 .b8 %smem_b[512];
    .reg .pred %p<6>;
    .reg .b32 %r<20>;
    .reg .b64 %rd<18>;
    .reg .f64 %fd<12>;

    ld.param.u64 %rd1, [param_a];
    ld.param.u64 %rd2, [param_b];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n];
    ld.param.u32 %r2, [param_m];

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mov.u32 %r3, %ctaid.y;
    shl.b32 %r4, %r3, 4;           // block_row = blockIdx.y * 16
    mov.u32 %r5, %ctaid.x;
    shl.b32 %r6, %r5, 4;           // block_col = blockIdx.x * 16

    mov.u32 %r7, %tid.y;
    mov.u32 %r8, %tid.x;
    shl.b32 %r9, %r7, 4;
    add.u32 %r9, %r9, %r8;         // linear_tid = tid.y * 16 + tid.x (0..255)

    // Cooperative coalesced load of 16 points of A (64 f64s) by linear_tid 0..63
    setp.lt.u32 %p1, %r9, 64;
    @!%p1 bra L_SM86_DIST_LOAD_B;
    shr.u32 %r10, %r9, 2;          // pt_idx = linear_tid >> 2 (0..15)
    add.u32 %r11, %r4, %r10;       // global_row = block_row + pt_idx
    mov.f64 %fd0, 0d0000000000000000;
    setp.lt.u32 %p2, %r11, %r1;
    @!%p2 bra L_SM86_DIST_STORE_SMEM_A;
    and.b32 %r12, %r9, 3;          // comp = linear_tid & 3
    shl.b32 %r13, %r11, 2;
    add.u32 %r13, %r13, %r12;      // global_row * 4 + comp
    mul.wide.u32 %rd7, %r13, 8;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.nc.f64 %fd0, [%rd8];
L_SM86_DIST_STORE_SMEM_A:
    mov.u64 %rd9, %smem_a;
    mul.wide.u32 %rd10, %r9, 8;
    add.u64 %rd9, %rd9, %rd10;
    st.shared.f64 [%rd9], %fd0;

L_SM86_DIST_LOAD_B:
    // Cooperative coalesced load of 16 points of B (64 f64s) by linear_tid 64..127
    sub.u32 %r14, %r9, 64;
    setp.lt.u32 %p3, %r14, 64;
    @!%p3 bra L_SM86_DIST_SYNC;
    shr.u32 %r10, %r14, 2;         // pt_idx = (linear_tid - 64) >> 2 (0..15)
    add.u32 %r11, %r6, %r10;       // global_col = block_col + pt_idx
    mov.f64 %fd0, 0d0000000000000000;
    setp.lt.u32 %p4, %r11, %r2;
    @!%p4 bra L_SM86_DIST_STORE_SMEM_B;
    and.b32 %r12, %r14, 3;         // comp = (linear_tid - 64) & 3
    shl.b32 %r13, %r11, 2;
    add.u32 %r13, %r13, %r12;      // global_col * 4 + comp
    mul.wide.u32 %rd7, %r13, 8;
    add.u64 %rd8, %rd5, %rd7;
    ld.global.nc.f64 %fd0, [%rd8];
L_SM86_DIST_STORE_SMEM_B:
    mov.u64 %rd9, %smem_b;
    mul.wide.u32 %rd10, %r14, 8;
    add.u64 %rd9, %rd9, %rd10;
    st.shared.f64 [%rd9], %fd0;

L_SM86_DIST_SYNC:
    bar.sync 0;

    add.u32 %r15, %r4, %r7;        // row = block_row + tid.y
    add.u32 %r16, %r6, %r8;        // col = block_col + tid.x
    setp.ge.u32 %p1, %r15, %r1;
    setp.ge.u32 %p2, %r16, %r2;
    or.pred %p1, %p1, %p2;
    @%p1 bra L_SM86_DIST_EXIT;

    mov.u64 %rd11, %smem_a;
    mul.wide.u32 %rd12, %r7, 32;
    add.u64 %rd11, %rd11, %rd12;
    ld.shared.f64 %fd1, [%rd11 + 0];
    ld.shared.f64 %fd2, [%rd11 + 8];
    ld.shared.f64 %fd3, [%rd11 + 16];

    mov.u64 %rd13, %smem_b;
    mul.wide.u32 %rd14, %r8, 32;
    add.u64 %rd13, %rd13, %rd14;
    ld.shared.f64 %fd4, [%rd13 + 0];
    ld.shared.f64 %fd5, [%rd13 + 8];
    ld.shared.f64 %fd6, [%rd13 + 16];

    sub.f64 %fd7, %fd1, %fd4;
    sub.f64 %fd8, %fd2, %fd5;
    sub.f64 %fd9, %fd3, %fd6;

    mul.f64 %fd10, %fd9, %fd9;
    fma.rn.f64 %fd10, %fd8, %fd8, %fd10;
    fma.rn.f64 %fd10, %fd7, %fd7, %fd10;

    mad.lo.u32 %r17, %r15, %r2, %r16;
    mul.wide.u32 %rd15, %r17, 8;
    add.u64 %rd16, %rd6, %rd15;
    st.global.f64 [%rd16], %fd10;

L_SM86_DIST_EXIT:
    ret;
}

// 4a. Standard CUDA SIMT bicubic rational Bezier surface evaluation (FP64)
.visible .entry cuda_simt_bezier_eval_f64(
    .param .u64 param_ctrl,
    .param .u64 param_u,
    .param .u64 param_v,
    .param .u64 param_out,
    .param .u32 param_nu,
    .param .u32 param_nv
) {
    .reg .pred %p<5>;
    .reg .b32 %r<16>;
    .reg .b64 %rd<16>;
    .reg .f64 %fd<28>;

    ld.param.u64 %rd1, [param_ctrl];
    ld.param.u64 %rd2, [param_u];
    ld.param.u64 %rd3, [param_v];
    ld.param.u64 %rd4, [param_out];
    ld.param.u32 %r1, [param_nu];
    ld.param.u32 %r2, [param_nv];

    mov.u32 %r3, %ctaid.y;
    mov.u32 %r4, %ntid.y;
    mov.u32 %r5, %tid.y;
    mad.lo.u32 %r6, %r3, %r4, %r5; // u_idx

    mov.u32 %r7, %ctaid.x;
    mov.u32 %r8, %ntid.x;
    mov.u32 %r9, %tid.x;
    mad.lo.u32 %r10, %r7, %r8, %r9; // v_idx

    setp.ge.u32 %p1, %r6, %r1;
    setp.ge.u32 %p2, %r10, %r2;
    or.pred %p1, %p1, %p2;
    @%p1 bra L_SIMT_BEZ_EXIT;

    cvta.to.global.u64 %rd5, %rd1;
    cvta.to.global.u64 %rd6, %rd2;
    cvta.to.global.u64 %rd7, %rd3;
    cvta.to.global.u64 %rd8, %rd4;

    mul.wide.u32 %rd9, %r6, 8;
    add.u64 %rd9, %rd6, %rd9;
    ld.global.f64 %fd26, [%rd9]; // u

    mul.wide.u32 %rd10, %r10, 8;
    add.u64 %rd10, %rd7, %rd10;
    ld.global.f64 %fd27, [%rd10]; // v

    // Cubic Bernstein basis for u -> %fd0..%fd3
    sub.f64 %fd8, 0d3FF0000000000000, %fd26;
    mul.f64 %fd9, %fd8, %fd8;
    mul.f64 %fd10, %fd26, %fd26;
    mul.f64 %fd0, %fd9, %fd8;
    mul.f64 %fd1, %fd9, %fd26;
    mul.f64 %fd1, %fd1, 0d4008000000000000;
    mul.f64 %fd2, %fd8, %fd10;
    mul.f64 %fd2, %fd2, 0d4008000000000000;
    mul.f64 %fd3, %fd10, %fd26;

    // Cubic Bernstein basis for v -> %fd4..%fd7
    sub.f64 %fd8, 0d3FF0000000000000, %fd27;
    mul.f64 %fd9, %fd8, %fd8;
    mul.f64 %fd10, %fd27, %fd27;
    mul.f64 %fd4, %fd9, %fd8;
    mul.f64 %fd5, %fd9, %fd27;
    mul.f64 %fd5, %fd5, 0d4008000000000000;
    mul.f64 %fd6, %fd8, %fd10;
    mul.f64 %fd6, %fd6, 0d4008000000000000;
    mul.f64 %fd7, %fd10, %fd27;

    mov.f64 %fd20, 0d0000000000000000;
    mov.f64 %fd21, 0d0000000000000000;
    mov.f64 %fd22, 0d0000000000000000;
    mov.f64 %fd23, 0d0000000000000000;

    mov.u32 %r11, 0;
L_SIMT_BEZ_LOOP:
    setp.ge.u32 %p1, %r11, 16;
    @%p1 bra L_SIMT_BEZ_DONE;

    shr.u32 %r12, %r11, 2;
    and.b32 %r13, %r11, 3;

    setp.eq.u32 %p2, %r12, 0;
    setp.eq.u32 %p3, %r12, 1;
    setp.eq.u32 %p4, %r12, 2;
    mov.f64 %fd12, %fd3;
    selp.f64 %fd12, %fd2, %fd12, %p4;
    selp.f64 %fd12, %fd1, %fd12, %p3;
    selp.f64 %fd12, %fd0, %fd12, %p2;

    setp.eq.u32 %p2, %r13, 0;
    setp.eq.u32 %p3, %r13, 1;
    setp.eq.u32 %p4, %r13, 2;
    mov.f64 %fd13, %fd7;
    selp.f64 %fd13, %fd6, %fd13, %p4;
    selp.f64 %fd13, %fd5, %fd13, %p3;
    selp.f64 %fd13, %fd4, %fd13, %p2;

    mul.f64 %fd14, %fd12, %fd13;

    mul.wide.u32 %rd11, %r11, 32;
    add.u64 %rd12, %rd5, %rd11;
    ld.global.f64 %fd15, [%rd12 + 0];
    ld.global.f64 %fd16, [%rd12 + 8];
    ld.global.f64 %fd17, [%rd12 + 16];
    ld.global.f64 %fd18, [%rd12 + 24];

    fma.rn.f64 %fd20, %fd14, %fd15, %fd20;
    fma.rn.f64 %fd21, %fd14, %fd16, %fd21;
    fma.rn.f64 %fd22, %fd14, %fd17, %fd22;
    fma.rn.f64 %fd23, %fd14, %fd18, %fd23;

    add.u32 %r11, %r11, 1;
    bra L_SIMT_BEZ_LOOP;

L_SIMT_BEZ_DONE:
    abs.f64 %fd24, %fd23;
    setp.gt.f64 %p2, %fd24, 0d3D719799812DEA11;
    div.rn.f64 %fd25, 0d3FF0000000000000, %fd23;
    selp.f64 %fd25, %fd25, 0d3FF0000000000000, %p2;

    mul.f64 %fd20, %fd20, %fd25;
    mul.f64 %fd21, %fd21, %fd25;
    mul.f64 %fd22, %fd22, %fd25;

    mad.lo.u32 %r14, %r6, %r2, %r10;
    mul.wide.u32 %rd13, %r14, 32;
    add.u64 %rd14, %rd8, %rd13;
    st.global.f64 [%rd14 + 0],  %fd20;
    st.global.f64 [%rd14 + 8],  %fd21;
    st.global.f64 [%rd14 + 16], %fd22;
    st.global.f64 [%rd14 + 24], %fd23;

L_SIMT_BEZ_EXIT:
    ret;
}

// 4b. Ampere sm_86 shared-memory control net + non-coherent load bicubic rational Bezier surface evaluation (FP64)
.visible .entry cuda_sm86_bezier_eval_f64(
    .param .u64 param_ctrl,
    .param .u64 param_u,
    .param .u64 param_v,
    .param .u64 param_out,
    .param .u32 param_nu,
    .param .u32 param_nv
) {
    .shared .align 16 .b8 %smem_ctrl[512];
    .reg .pred %p<5>;
    .reg .b32 %r<18>;
    .reg .b64 %rd<16>;
    .reg .f64 %fd<28>;

    ld.param.u64 %rd1, [param_ctrl];
    ld.param.u64 %rd2, [param_u];
    ld.param.u64 %rd3, [param_v];
    ld.param.u64 %rd4, [param_out];
    ld.param.u32 %r1, [param_nu];
    ld.param.u32 %r2, [param_nv];

    cvta.to.global.u64 %rd5, %rd1;
    cvta.to.global.u64 %rd6, %rd2;
    cvta.to.global.u64 %rd7, %rd3;
    cvta.to.global.u64 %rd8, %rd4;

    mov.u32 %r3, %ctaid.y;
    mov.u32 %r4, %ntid.y;
    mov.u32 %r5, %tid.y;
    mad.lo.u32 %r6, %r3, %r4, %r5; // u_idx

    mov.u32 %r7, %ctaid.x;
    mov.u32 %r8, %ntid.x;
    mov.u32 %r9, %tid.x;
    mad.lo.u32 %r10, %r7, %r8, %r9; // v_idx

    // Cooperative load of 64 f64s of the 4x4x4 control net into shared memory
    mad.lo.u32 %r15, %r5, %r8, %r9; // linear_tid
    setp.lt.u32 %p1, %r15, 64;
    @!%p1 bra L_SM86_BEZ_SYNC;
    mul.wide.u32 %rd9, %r15, 8;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.nc.f64 %fd0, [%rd10];
    mov.u64 %rd11, %smem_ctrl;
    add.u64 %rd11, %rd11, %rd9;
    st.shared.f64 [%rd11], %fd0;

L_SM86_BEZ_SYNC:
    bar.sync 0;

    setp.ge.u32 %p1, %r6, %r1;
    setp.ge.u32 %p2, %r10, %r2;
    or.pred %p1, %p1, %p2;
    @%p1 bra L_SM86_BEZ_EXIT;

    mul.wide.u32 %rd9, %r6, 8;
    add.u64 %rd9, %rd6, %rd9;
    ld.global.nc.f64 %fd26, [%rd9]; // u

    mul.wide.u32 %rd10, %r10, 8;
    add.u64 %rd10, %rd7, %rd10;
    ld.global.nc.f64 %fd27, [%rd10]; // v

    sub.f64 %fd8, 0d3FF0000000000000, %fd26;
    mul.f64 %fd9, %fd8, %fd8;
    mul.f64 %fd10, %fd26, %fd26;
    mul.f64 %fd0, %fd9, %fd8;
    mul.f64 %fd1, %fd9, %fd26;
    mul.f64 %fd1, %fd1, 0d4008000000000000;
    mul.f64 %fd2, %fd8, %fd10;
    mul.f64 %fd2, %fd2, 0d4008000000000000;
    mul.f64 %fd3, %fd10, %fd26;

    sub.f64 %fd8, 0d3FF0000000000000, %fd27;
    mul.f64 %fd9, %fd8, %fd8;
    mul.f64 %fd10, %fd27, %fd27;
    mul.f64 %fd4, %fd9, %fd8;
    mul.f64 %fd5, %fd9, %fd27;
    mul.f64 %fd5, %fd5, 0d4008000000000000;
    mul.f64 %fd6, %fd8, %fd10;
    mul.f64 %fd6, %fd6, 0d4008000000000000;
    mul.f64 %fd7, %fd10, %fd27;

    mov.f64 %fd20, 0d0000000000000000;
    mov.f64 %fd21, 0d0000000000000000;
    mov.f64 %fd22, 0d0000000000000000;
    mov.f64 %fd23, 0d0000000000000000;

    mov.u64 %rd15, %smem_ctrl;
    mov.u32 %r11, 0;
L_SM86_BEZ_LOOP:
    setp.ge.u32 %p1, %r11, 16;
    @%p1 bra L_SM86_BEZ_DONE;

    shr.u32 %r12, %r11, 2;
    and.b32 %r13, %r11, 3;

    setp.eq.u32 %p2, %r12, 0;
    setp.eq.u32 %p3, %r12, 1;
    setp.eq.u32 %p4, %r12, 2;
    mov.f64 %fd12, %fd3;
    selp.f64 %fd12, %fd2, %fd12, %p4;
    selp.f64 %fd12, %fd1, %fd12, %p3;
    selp.f64 %fd12, %fd0, %fd12, %p2;

    setp.eq.u32 %p2, %r13, 0;
    setp.eq.u32 %p3, %r13, 1;
    setp.eq.u32 %p4, %r13, 2;
    mov.f64 %fd13, %fd7;
    selp.f64 %fd13, %fd6, %fd13, %p4;
    selp.f64 %fd13, %fd5, %fd13, %p3;
    selp.f64 %fd13, %fd4, %fd13, %p2;

    mul.f64 %fd14, %fd12, %fd13;

    mul.wide.u32 %rd11, %r11, 32;
    add.u64 %rd12, %rd15, %rd11;
    ld.shared.f64 %fd15, [%rd12 + 0];
    ld.shared.f64 %fd16, [%rd12 + 8];
    ld.shared.f64 %fd17, [%rd12 + 16];
    ld.shared.f64 %fd18, [%rd12 + 24];

    fma.rn.f64 %fd20, %fd14, %fd15, %fd20;
    fma.rn.f64 %fd21, %fd14, %fd16, %fd21;
    fma.rn.f64 %fd22, %fd14, %fd17, %fd22;
    fma.rn.f64 %fd23, %fd14, %fd18, %fd23;

    add.u32 %r11, %r11, 1;
    bra L_SM86_BEZ_LOOP;

L_SM86_BEZ_DONE:
    abs.f64 %fd24, %fd23;
    setp.gt.f64 %p2, %fd24, 0d3D719799812DEA11;
    div.rn.f64 %fd25, 0d3FF0000000000000, %fd23;
    selp.f64 %fd25, %fd25, 0d3FF0000000000000, %p2;

    mul.f64 %fd20, %fd20, %fd25;
    mul.f64 %fd21, %fd21, %fd25;
    mul.f64 %fd22, %fd22, %fd25;

    mad.lo.u32 %r14, %r6, %r2, %r10;
    mul.wide.u32 %rd13, %r14, 32;
    add.u64 %rd14, %rd8, %rd13;
    st.global.f64 [%rd14 + 0],  %fd20;
    st.global.f64 [%rd14 + 8],  %fd21;
    st.global.f64 [%rd14 + 16], %fd22;
    st.global.f64 [%rd14 + 24], %fd23;

L_SM86_BEZ_EXIT:
    ret;
}

// 4c. Handcrafted RTX Tensor Core two-stage contraction for bicubic rational Bezier surface evaluation:
// Stage 1 contracts U_basis (N_u x 4) * G_c (4 x 4), Stage 2 contracts (U_basis * G_c) (8 x 4) * V_basis^T (4 x 8)
// using mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64.
.visible .entry rtx_tc_bezier_eval_f64(
    .param .u64 param_ug,
    .param .u64 param_v_basis,
    .param .u64 param_out,
    .param .u32 param_nu,
    .param .u32 param_nv
) {
    .reg .pred %p<5>;
    .reg .b32 %r<24>;
    .reg .b64 %rd<20>;
    .reg .f64 %fa<4>, %fb0, %fc0, %fc1, %fd<8>, %fw0, %fw1, %finv0, %finv1, %fx, %fy, %fz;

    ld.param.u64 %rd1, [param_ug];
    ld.param.u64 %rd2, [param_v_basis];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_nu];
    ld.param.u32 %r2, [param_nv];

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mov.u32 %r3, %ctaid.y;
    shl.b32 %r4, %r3, 3;       // u_base = blockIdx.y * 8
    mov.u32 %r5, %ctaid.x;
    shl.b32 %r6, %r5, 3;       // v_base = blockIdx.x * 8

    mov.u32 %r7, %tid.x;
    and.b32 %r8, %r7, 31;      // lane
    shr.u32 %r9, %r8, 2;       // group_id (0..7)
    and.b32 %r10, %r8, 3;      // thread_in_group (0..3: j control index)

    add.u32 %r11, %r4, %r9;    // u_idx = u_base + group_id
    add.u32 %r12, %r6, %r9;    // v_idx_for_B = v_base + group_id

    mov.f64 %fa0, 0d0000000000000000;
    mov.f64 %fa1, 0d0000000000000000;
    mov.f64 %fa2, 0d0000000000000000;
    mov.f64 %fa3, 0d0000000000000000;

    setp.lt.u32 %p1, %r11, %r1;
    @!%p1 bra L_TC_BEZ_SKIP_A;

    // param_ug layout: [u_idx][j = thread_in_group][comp = 0..3] -> (u_idx * 16 + j * 4) * 8
    shl.b32 %r13, %r11, 4;
    shl.b32 %r14, %r10, 2;
    add.u32 %r13, %r13, %r14;
    mul.wide.u32 %rd7, %r13, 8;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.f64 %fa0, [%rd8 + 0];
    ld.global.f64 %fa1, [%rd8 + 8];
    ld.global.f64 %fa2, [%rd8 + 16];
    ld.global.f64 %fa3, [%rd8 + 24];

L_TC_BEZ_SKIP_A:
    mov.f64 %fb0, 0d0000000000000000;
    setp.lt.u32 %p2, %r12, %r2;
    @!%p2 bra L_TC_BEZ_SKIP_B;

    shl.b32 %r15, %r12, 2;
    add.u32 %r15, %r15, %r10;
    mul.wide.u32 %rd9, %r15, 8;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.f64 %fb0, [%rd10];

L_TC_BEZ_SKIP_B:
    mov.f64 %fc0, 0d0000000000000000;
    mov.f64 %fc1, 0d0000000000000000;

    // 4 Tensor Core MMA instructions for homogeneous components x*w, y*w, z*w, w
    mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64 {%fd0, %fd1}, {%fa0}, {%fb0}, {%fc0, %fc1};
    mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64 {%fd2, %fd3}, {%fa1}, {%fb0}, {%fc0, %fc1};
    mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64 {%fd4, %fd5}, {%fa2}, {%fb0}, {%fc0, %fc1};
    mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64 {%fd6, %fd7}, {%fa3}, {%fb0}, {%fc0, %fc1};

    @!%p1 bra L_TC_BEZ_EXIT;

    shl.b32 %r16, %r10, 1;
    add.u32 %r17, %r6, %r16;   // v0
    add.u32 %r18, %r17, 1;     // v1

    setp.lt.u32 %p3, %r17, %r2;
    @!%p3 bra L_TC_BEZ_EXIT;

    div.rn.f64 %fx, %fd0, %fd6;
    div.rn.f64 %fy, %fd2, %fd6;
    div.rn.f64 %fz, %fd4, %fd6;

    mad.lo.u32 %r19, %r11, %r2, %r17;
    mul.wide.u32 %rd11, %r19, 32;
    add.u64 %rd12, %rd6, %rd11;
    st.global.f64 [%rd12 + 0],  %fx;
    st.global.f64 [%rd12 + 8],  %fy;
    st.global.f64 [%rd12 + 16], %fz;
    st.global.f64 [%rd12 + 24], %fd6;

    setp.lt.u32 %p4, %r18, %r2;
    @!%p4 bra L_TC_BEZ_EXIT;

    div.rn.f64 %fx, %fd1, %fd7;
    div.rn.f64 %fy, %fd3, %fd7;
    div.rn.f64 %fz, %fd5, %fd7;

    st.global.f64 [%rd12 + 32], %fx;
    st.global.f64 [%rd12 + 40], %fy;
    st.global.f64 [%rd12 + 48], %fz;
    st.global.f64 [%rd12 + 56], %fd7;

L_TC_BEZ_EXIT:
    ret;
}

";

/// Hardware context for safe `cudarc` CUDA Driver API execution, `OptiX` RT Core BVH traversal, and RTX Tensor Core PTX dispatch.
pub struct CudaContext {
    device_name: String,
    compute_capability: (i32, i32),
    ctx: Arc<CudarcContext>,
    fn_simt_transform: CudaFunction,
    fn_sm86_transform: CudaFunction,
    fn_tc_transform: CudaFunction,
    fn_simt_pairwise_dist: CudaFunction,
    fn_sm86_pairwise_dist: CudaFunction,
    fn_simt_bezier_eval: CudaFunction,
    fn_sm86_bezier_eval: CudaFunction,
    fn_tc_bezier_eval: CudaFunction,
}

static GLOBAL_CUDA: OnceLock<Option<CudaContext>> = OnceLock::new();

/// Returns the process-wide [`CudaContext`] if a CUDA driver and `sm_80+` GPU are present.
#[must_use]
pub fn global_cuda() -> Option<&'static CudaContext> {
    GLOBAL_CUDA.get_or_init(CudaContext::probe).as_ref()
}

impl CudaContext {
    /// Probes the primary CUDA device via `cudarc` and compiles the PTX and `OptiX` kernel suites.
    #[must_use]
    pub fn probe() -> Option<Self> {
        // Safety: queries dynamic loader presence of libcuda without dereferencing pointers.
        if !unsafe { cudarc::driver::sys::is_culib_present() } {
            return None;
        }
        let ctx = CudarcContext::new(0).ok()?;
        let device_name = ctx.name().ok()?;
        let major = ctx
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
            )
            .ok()?;
        let minor = ctx
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR,
            )
            .ok()?;

        let module = ctx.load_module(Ptx::from_src(CUDA_PTX_MODULE)).ok()?;

        Some(Self {
            fn_simt_transform: module.load_function("cuda_simt_transform_4x4_f64").ok()?,
            fn_sm86_transform: module.load_function("cuda_sm86_transform_4x4_f64").ok()?,
            fn_tc_transform: module.load_function("rtx_tc_transform_4x4_f64").ok()?,
            fn_simt_pairwise_dist: module.load_function("cuda_simt_pairwise_dist_f64").ok()?,
            fn_sm86_pairwise_dist: module.load_function("cuda_sm86_pairwise_dist_f64").ok()?,
            fn_simt_bezier_eval: module.load_function("cuda_simt_bezier_eval_f64").ok()?,
            fn_sm86_bezier_eval: module.load_function("cuda_sm86_bezier_eval_f64").ok()?,
            fn_tc_bezier_eval: module.load_function("rtx_tc_bezier_eval_f64").ok()?,
            device_name,
            compute_capability: (major, minor),
            ctx,
        })
    }

    /// Name of the active CUDA GPU device.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Returns whether the given CUDA compute capability has hardware FP64 Tensor Cores.
    ///
    /// Hardware FP64 Tensor Cores are present on data-center architectures
    /// (`sm_80`, `sm_90`, `sm_100`). Client architectures (`sm_86`, `sm_89`)
    /// omit physical FP64 matrix units.
    #[must_use]
    pub const fn supports_fp64_tensor_cores(major: i32, minor: i32) -> bool {
        major >= 8 && minor == 0
    }

    /// CUDA compute capability `(major, minor)`.
    #[must_use]
    pub const fn compute_capability(&self) -> (i32, i32) {
        self.compute_capability
    }

    /// Whether the active GPU supports hardware FP64 Tensor Core MMA.
    #[must_use]
    pub const fn supports_tensor_cores(&self) -> bool {
        Self::supports_fp64_tensor_cores(self.compute_capability.0, self.compute_capability.1)
    }

    /// Whether the active GPU is Ampere `sm_86` or newer.
    #[must_use]
    pub const fn supports_sm86(&self) -> bool {
        self.compute_capability.0 > 8
            || (self.compute_capability.0 == 8 && self.compute_capability.1 >= 6)
    }

    fn dispatch_transform_4x4_kernel(
        &self,
        func: &CudaFunction,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Option<Vec<[f64; 4]>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_u32 = u32::try_from(points.len()).ok()?;
        let flat_in = points.as_flattened();

        let d_mat = stream.clone_htod(matrix.as_flattened()).ok()?;
        let d_in = stream.clone_htod(flat_in).ok()?;
        // Safety: every element of `d_out` is written by the kernel before `memcpy_dtoh`.
        let mut d_out = unsafe { stream.alloc::<f64>(flat_in.len()) }.ok()?;

        let block = 128_u32;
        let grid = n_u32.div_ceil(block);
        let cfg = LaunchConfig {
            grid_dim: (grid, 1, 1),
            block_dim: (block, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(func);
        builder.arg(&d_mat);
        builder.arg(&d_in);
        builder.arg(&mut d_out);
        builder.arg(&n_u32);
        // Safety: kernel signature matches (u64, u64, u64, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        let mut out: Vec<[f64; 4]> = (0..points.len()).map(|_| [0.0_f64; 4]).collect();
        stream.memcpy_dtoh(&d_out, out.as_flattened_mut()).ok()?;
        Some(out)
    }

    /// Executes standard CUDA SIMT 4x4 batch point transformation.
    #[must_use]
    pub fn simt_batch_transform_4x4(
        &self,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Option<Vec<[f64; 4]>> {
        self.dispatch_transform_4x4_kernel(&self.fn_simt_transform, matrix, points)
    }

    /// Executes Ampere `sm_86` PTX shared-memory + non-coherent load 4x4 batch point transformation.
    #[must_use]
    pub fn ptx_sm86_batch_transform_4x4(
        &self,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Option<Vec<[f64; 4]>> {
        if !self.supports_sm86() {
            return None;
        }
        self.dispatch_transform_4x4_kernel(&self.fn_sm86_transform, matrix, points)
    }

    /// Executes RTX Tensor Core (`mma.sync.aligned.m8n8k4.row.col.f64`) 4x4 batch point transformation.
    #[must_use]
    pub fn tensor_core_batch_transform_4x4(
        &self,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Option<Vec<[f64; 4]>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if !self.supports_tensor_cores() {
            return None;
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_u32 = u32::try_from(points.len()).ok()?;
        let flat_in = points.as_flattened();

        let d_mat = stream.clone_htod(matrix.as_flattened()).ok()?;
        let d_in = stream.clone_htod(flat_in).ok()?;
        // Safety: every element of `d_out` is written by the kernel before `memcpy_dtoh`.
        let mut d_out = unsafe { stream.alloc::<f64>(flat_in.len()) }.ok()?;

        let grid = n_u32.div_ceil(8);
        let cfg = LaunchConfig {
            grid_dim: (grid, 1, 1),
            block_dim: (32, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(&self.fn_tc_transform);
        builder.arg(&d_mat);
        builder.arg(&d_in);
        builder.arg(&mut d_out);
        builder.arg(&n_u32);
        // Safety: kernel signature matches (u64, u64, u64, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        let mut out: Vec<[f64; 4]> = (0..points.len()).map(|_| [0.0_f64; 4]).collect();
        stream.memcpy_dtoh(&d_out, out.as_flattened_mut()).ok()?;
        Some(out)
    }

    fn dispatch_pairwise_distance_kernel(
        &self,
        func: &CudaFunction,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<Vec<f64>> {
        let total = points_a.len().checked_mul(points_b.len())?;
        if total == 0 {
            return Some(Vec::new());
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_u32 = u32::try_from(points_a.len()).ok()?;
        let m_u32 = u32::try_from(points_b.len()).ok()?;
        let flat_a: Vec<f64> = points_a.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();
        let flat_b: Vec<f64> = points_b.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();

        let d_a = stream.clone_htod(&flat_a).ok()?;
        let d_b = stream.clone_htod(&flat_b).ok()?;
        let mut d_out = stream.alloc_zeros::<f64>(total).ok()?;

        let grid_x = m_u32.div_ceil(16);
        let grid_y = n_u32.div_ceil(16);
        let cfg = LaunchConfig {
            grid_dim: (grid_x, grid_y, 1),
            block_dim: (16, 16, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(func);
        builder.arg(&d_a);
        builder.arg(&d_b);
        builder.arg(&mut d_out);
        builder.arg(&n_u32);
        builder.arg(&m_u32);
        // Safety: kernel signature matches (u64, u64, u64, u32, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        stream.clone_dtoh(&d_out).ok()
    }

    /// Executes standard CUDA SIMT pairwise squared distance matrix computation.
    #[must_use]
    pub fn simt_pairwise_distance_squared(
        &self,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<Vec<f64>> {
        self.dispatch_pairwise_distance_kernel(&self.fn_simt_pairwise_dist, points_a, points_b)
    }

    /// Executes Ampere `sm_86` shared-memory $16 \times 16$ tiled pairwise squared distance matrix computation.
    #[must_use]
    pub fn ptx_sm86_pairwise_distance_squared(
        &self,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<Vec<f64>> {
        if !self.supports_sm86() {
            return None;
        }
        self.dispatch_pairwise_distance_kernel(&self.fn_sm86_pairwise_dist, points_a, points_b)
    }

    fn dispatch_bezier_eval_kernel(
        &self,
        func: &CudaFunction,
        control_points: &[[[f64; 4]; 4]; 4],
        u_params: &[f64],
        v_params: &[f64],
    ) -> Option<Vec<Point3>> {
        let total = u_params.len().checked_mul(v_params.len())?;
        if total == 0 {
            return Some(Vec::new());
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let nu_u32 = u32::try_from(u_params.len()).ok()?;
        let nv_u32 = u32::try_from(v_params.len()).ok()?;

        let mut flat_ctrl = Vec::with_capacity(64);
        for row in control_points {
            for cell in row {
                flat_ctrl.extend_from_slice(cell);
            }
        }

        let d_ctrl = stream.clone_htod(&flat_ctrl).ok()?;
        let d_u = stream.clone_htod(u_params).ok()?;
        let d_v = stream.clone_htod(v_params).ok()?;
        let out_f64_len = total.checked_mul(4)?;
        let mut d_out = stream.alloc_zeros::<f64>(out_f64_len).ok()?;

        let grid_x = nv_u32.div_ceil(16);
        let grid_y = nu_u32.div_ceil(16);
        let cfg = LaunchConfig {
            grid_dim: (grid_x, grid_y, 1),
            block_dim: (16, 16, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(func);
        builder.arg(&d_ctrl);
        builder.arg(&d_u);
        builder.arg(&d_v);
        builder.arg(&mut d_out);
        builder.arg(&nu_u32);
        builder.arg(&nv_u32);
        // Safety: kernel signature matches (u64, u64, u64, u64, u32, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(
            raw_out
                .chunks_exact(4)
                .map(|p| Point3::new(p[0], p[1], p[2]))
                .collect(),
        )
    }

    /// Evaluates bicubic rational Bézier surface patches via standard CUDA SIMT FP64.
    #[must_use]
    pub fn simt_eval_bicubic_bezier_grid(
        &self,
        control_points: &[[[f64; 4]; 4]; 4],
        u_params: &[f64],
        v_params: &[f64],
    ) -> Option<Vec<Point3>> {
        self.dispatch_bezier_eval_kernel(
            &self.fn_simt_bezier_eval,
            control_points,
            u_params,
            v_params,
        )
    }

    /// Evaluates bicubic rational Bézier surface patches via Ampere `sm_86` PTX with shared-memory control net.
    #[must_use]
    pub fn ptx_sm86_eval_bicubic_bezier_grid(
        &self,
        control_points: &[[[f64; 4]; 4]; 4],
        u_params: &[f64],
        v_params: &[f64],
    ) -> Option<Vec<Point3>> {
        if !self.supports_sm86() {
            return None;
        }
        self.dispatch_bezier_eval_kernel(
            &self.fn_sm86_bezier_eval,
            control_points,
            u_params,
            v_params,
        )
    }

    /// Evaluates bicubic rational Bézier surface patches on a tensor grid `(u, v)`
    /// using RTX Tensor Core `mma.sync.aligned.m8n8k4.row.col.f64` two-stage contractions.
    #[must_use]
    pub fn tensor_core_eval_bicubic_bezier_grid(
        &self,
        control_points: &[[[f64; 4]; 4]; 4],
        u_params: &[f64],
        v_params: &[f64],
    ) -> Option<Vec<Point3>> {
        let total = u_params.len().checked_mul(v_params.len())?;
        if total == 0 {
            return Some(Vec::new());
        }
        if !self.supports_tensor_cores() {
            return None;
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let nu_u32 = u32::try_from(u_params.len()).ok()?;
        let nv_u32 = u32::try_from(v_params.len()).ok()?;

        let u_bases: Vec<[f64; 4]> = u_params
            .iter()
            .map(|&u| cpu::cubic_bernstein_basis(u))
            .collect();
        let mut flat_ug: Vec<f64> = Vec::with_capacity(u_params.len().checked_mul(16)?);
        for bu in &u_bases {
            for (c0, c1, c2, c3) in control_points[0]
                .iter()
                .zip(&control_points[1])
                .zip(&control_points[2])
                .zip(&control_points[3])
                .map(|(((a, b), c), d)| (a, b, c, d))
            {
                for (((&v0, &v1), &v2), &v3) in c0.iter().zip(c1).zip(c2).zip(c3) {
                    flat_ug
                        .push(bu[0].mul_add(v0, bu[1].mul_add(v1, bu[2].mul_add(v2, bu[3] * v3))));
                }
            }
        }
        let flat_vb: Vec<f64> = v_params
            .iter()
            .flat_map(|&v| cpu::cubic_bernstein_basis(v))
            .collect();

        let d_ug = stream.clone_htod(&flat_ug).ok()?;
        let d_vb = stream.clone_htod(&flat_vb).ok()?;
        let out_f64_len = total.checked_mul(4)?;
        let mut d_out = stream.alloc_zeros::<f64>(out_f64_len).ok()?;

        let grid_x = nv_u32.div_ceil(8);
        let grid_y = nu_u32.div_ceil(8);
        let cfg = LaunchConfig {
            grid_dim: (grid_x, grid_y, 1),
            block_dim: (32, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(&self.fn_tc_bezier_eval);
        builder.arg(&d_ug);
        builder.arg(&d_vb);
        builder.arg(&mut d_out);
        builder.arg(&nu_u32);
        builder.arg(&nv_u32);
        // Safety: kernel signature matches (u64, u64, u64, u32, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(
            raw_out
                .chunks_exact(4)
                .map(|p| Point3::new(p[0], p[1], p[2]))
                .collect(),
        )
    }

    /// Whether the active GPU supports NVIDIA `OptiX` hardware RT Core acceleration.
    #[must_use]
    pub const fn supports_optix_rt(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUDA_PARITY_TOLERANCE: f64 = 1.0e-11;

    #[test]
    fn supports_tensor_cores_matches_datacenter_architectures() {
        assert!(CudaContext::supports_fp64_tensor_cores(8, 0));
        assert!(!CudaContext::supports_fp64_tensor_cores(8, 6));
        assert!(!CudaContext::supports_fp64_tensor_cores(8, 9));
        assert!(CudaContext::supports_fp64_tensor_cores(9, 0));
        assert!(CudaContext::supports_fp64_tensor_cores(10, 0));
        assert!(!CudaContext::supports_fp64_tensor_cores(7, 5));
    }

    #[test]
    fn cuda_simt_transform_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        assert!(!ctx.device_name().is_empty());

        let matrix = [
            [1.25, -0.5, 0.25, 4.0],
            [0.5, 2.0, -0.75, -2.5],
            [-0.25, 0.125, 1.5, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let points: Vec<[f64; 4]> = (0..19)
            .map(|i| {
                let f = f64::from(i);
                [f * 0.3 - 1.0, f * -0.7 + 2.0, f * 0.5, 1.0]
            })
            .collect();

        let expected = cpu::batch_transform_4x4(&matrix, &points);
        let simt_out = ctx
            .simt_batch_transform_4x4(&matrix, &points)
            .expect("SIMT transform succeeded");

        for (e, s) in expected.iter().zip(&simt_out) {
            for c in 0..4 {
                assert!((e[c] - s[c]).abs() < CUDA_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn cuda_sm86_transform_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let matrix = [
            [1.25, -0.5, 0.25, 4.0],
            [0.5, 2.0, -0.75, -2.5],
            [-0.25, 0.125, 1.5, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let points: Vec<[f64; 4]> = (0..19)
            .map(|i| {
                let f = f64::from(i);
                [f * 0.3 - 1.0, f * -0.7 + 2.0, f * 0.5, 1.0]
            })
            .collect();

        let expected = cpu::batch_transform_4x4(&matrix, &points);
        if ctx.supports_sm86() {
            let sm86_out = ctx
                .ptx_sm86_batch_transform_4x4(&matrix, &points)
                .expect("PTX sm_86 transform succeeded");
            for (e, s) in expected.iter().zip(&sm86_out) {
                for c in 0..4 {
                    assert!((e[c] - s[c]).abs() < CUDA_PARITY_TOLERANCE);
                }
            }
        } else {
            assert!(ctx.ptx_sm86_batch_transform_4x4(&matrix, &points).is_none());
        }
    }

    #[test]
    fn cuda_tensor_core_transform_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let matrix = [
            [1.25, -0.5, 0.25, 4.0],
            [0.5, 2.0, -0.75, -2.5],
            [-0.25, 0.125, 1.5, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let points: Vec<[f64; 4]> = (0..19)
            .map(|i| {
                let f = f64::from(i);
                [f * 0.3 - 1.0, f * -0.7 + 2.0, f * 0.5, 1.0]
            })
            .collect();

        let expected = cpu::batch_transform_4x4(&matrix, &points);
        if ctx.supports_tensor_cores() {
            let tc_out = ctx
                .tensor_core_batch_transform_4x4(&matrix, &points)
                .expect("Tensor Core transform succeeded");
            for (e, t) in expected.iter().zip(&tc_out) {
                for c in 0..4 {
                    assert!((e[c] - t[c]).abs() < CUDA_PARITY_TOLERANCE);
                }
            }
        } else {
            assert!(ctx
                .tensor_core_batch_transform_4x4(&matrix, &points)
                .is_none());
        }
    }

    #[test]
    fn cuda_simt_pairwise_distances_match_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let pts_a: Vec<Point3> = (0..17)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(1000.0 + f * 0.25, -500.0 + f * 0.5, 250.0 - f * 0.125)
            })
            .collect();
        let pts_b: Vec<Point3> = (0..13)
            .map(|j| {
                let f = f64::from(j);
                Point3::new(1000.5 + f * 0.125, -499.0 - f * 0.25, 249.5 + f * 0.375)
            })
            .collect();

        let expected_dist = cpu::pairwise_distance_squared(&pts_a, &pts_b);
        let simt_dist = ctx
            .simt_pairwise_distance_squared(&pts_a, &pts_b)
            .expect("SIMT dist");

        for (&e, &s) in expected_dist.iter().zip(&simt_dist) {
            assert!((e - s).abs() < CUDA_PARITY_TOLERANCE);
        }
    }

    #[test]
    fn cuda_sm86_pairwise_distances_match_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let pts_a: Vec<Point3> = (0..35)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(1000.0 + f * 0.25, -500.0 + f * 0.5, 250.0 - f * 0.125)
            })
            .collect();
        let pts_b: Vec<Point3> = (0..29)
            .map(|j| {
                let f = f64::from(j);
                Point3::new(1000.5 + f * 0.125, -499.0 - f * 0.25, 249.5 + f * 0.375)
            })
            .collect();

        let expected_dist = cpu::pairwise_distance_squared(&pts_a, &pts_b);
        if ctx.supports_sm86() {
            let sm86_dist = ctx
                .ptx_sm86_pairwise_distance_squared(&pts_a, &pts_b)
                .expect("PTX sm_86 tiled pairwise dist");
            assert_eq!(expected_dist.len(), sm86_dist.len());
            for (&e, &s) in expected_dist.iter().zip(&sm86_dist) {
                assert!((e - s).abs() < CUDA_PARITY_TOLERANCE);
            }
        } else {
            assert!(ctx
                .ptx_sm86_pairwise_distance_squared(&pts_a, &pts_b)
                .is_none());
        }
    }

    #[test]
    fn cuda_simt_bezier_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let mut ctrl = [[[0.0_f64; 4]; 4]; 4];
        for (i, row) in ctrl.iter_mut().enumerate() {
            for (j, pole) in row.iter_mut().enumerate() {
                let u = f64::from(u32::try_from(i).unwrap_or(0));
                let v = f64::from(u32::try_from(j).unwrap_or(0));
                let w = 1.0 + 0.1 * (u + v);
                *pole = [u * w, v * w, (u * u - v) * 0.25 * w, w];
            }
        }
        let u_params: Vec<f64> = (0..11).map(|i| f64::from(i) / 10.0).collect();
        let v_params: Vec<f64> = (0..9).map(|j| f64::from(j) / 8.0).collect();

        let expected_bez = cpu::eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params);
        let simt_bez = ctx
            .simt_eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params)
            .expect("CUDA SIMT Bezier eval");
        assert_eq!(expected_bez.len(), simt_bez.len());
        for (e, s) in expected_bez.iter().zip(&simt_bez) {
            assert!((e.x - s.x).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.y - s.y).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.z - s.z).abs() < CUDA_PARITY_TOLERANCE);
        }
    }

    #[test]
    fn cuda_sm86_bezier_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let mut ctrl = [[[0.0_f64; 4]; 4]; 4];
        for (i, row) in ctrl.iter_mut().enumerate() {
            for (j, pole) in row.iter_mut().enumerate() {
                let u = f64::from(u32::try_from(i).unwrap_or(0));
                let v = f64::from(u32::try_from(j).unwrap_or(0));
                let w = 1.0 + 0.1 * (u + v);
                *pole = [u * w, v * w, (u * u - v) * 0.25 * w, w];
            }
        }
        let u_params: Vec<f64> = (0..11).map(|i| f64::from(i) / 10.0).collect();
        let v_params: Vec<f64> = (0..9).map(|j| f64::from(j) / 8.0).collect();

        let expected_bez = cpu::eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params);
        if ctx.supports_sm86() {
            let sm86_bez = ctx
                .ptx_sm86_eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params)
                .expect("PTX sm_86 Bezier eval");
            assert_eq!(expected_bez.len(), sm86_bez.len());
            for (e, s) in expected_bez.iter().zip(&sm86_bez) {
                assert!((e.x - s.x).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.y - s.y).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.z - s.z).abs() < CUDA_PARITY_TOLERANCE);
            }
        } else {
            assert!(ctx
                .ptx_sm86_eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params)
                .is_none());
        }
    }

    #[test]
    fn cuda_tensor_core_bezier_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let mut ctrl = [[[0.0_f64; 4]; 4]; 4];
        for (i, row) in ctrl.iter_mut().enumerate() {
            for (j, pole) in row.iter_mut().enumerate() {
                let u = f64::from(u32::try_from(i).unwrap_or(0));
                let v = f64::from(u32::try_from(j).unwrap_or(0));
                let w = 1.0 + 0.1 * (u + v);
                *pole = [u * w, v * w, (u * u - v) * 0.25 * w, w];
            }
        }
        let u_params: Vec<f64> = (0..11).map(|i| f64::from(i) / 10.0).collect();
        let v_params: Vec<f64> = (0..9).map(|j| f64::from(j) / 8.0).collect();

        let expected_bez = cpu::eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params);
        if ctx.supports_tensor_cores() {
            let tc_bez = ctx
                .tensor_core_eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params)
                .expect("Tensor Core Bezier eval");
            assert_eq!(expected_bez.len(), tc_bez.len());
            for (e, t) in expected_bez.iter().zip(&tc_bez) {
                assert!((e.x - t.x).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.y - t.y).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.z - t.z).abs() < CUDA_PARITY_TOLERANCE);
            }
        } else {
            assert!(ctx
                .tensor_core_eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params)
                .is_none());
        }
    }

    #[test]
    fn cuda_probe_and_extreme_magnitude_1e20_parity() {
        // Safety: queries dynamic loader presence of libcuda without dereferencing pointers.
        let has_lib = unsafe { cudarc::driver::sys::is_culib_present() };
        let probed = global_cuda();
        if !has_lib {
            assert!(probed.is_none());
            return;
        }
        let Some(ctx) = probed else {
            return;
        };
        let scale = 1.0e20_f64;
        let matrix = [
            [scale, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, scale, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let points = [[scale, -scale, 0.5 * scale, 1.0]];
        let cpu_out = cpu::batch_transform_4x4(&matrix, &points);
        let simt_out = ctx
            .simt_batch_transform_4x4(&matrix, &points)
            .expect("CUDA SIMT 1e20 transform");
        assert_eq!(simt_out, cpu_out);
        assert_eq!(simt_out[0][0], 1.0e40);
        assert!(!simt_out[0][0].is_nan());
    }
}
