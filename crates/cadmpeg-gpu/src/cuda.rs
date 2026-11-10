// SPDX-License-Identifier: Apache-2.0
//! Safe `cudarc` CUDA Driver API backend for SIMT FP64, Ampere `sm_86`, `OptiX` RT Core, and RTX Tensor Core PTX kernels.

use std::sync::{Arc, Mutex, OnceLock};

use cadmpeg_ir::math::Point3;
use cudarc::driver::{
    CudaContext as CudarcContext, CudaFunction, CudaSlice, CudaStream, DevicePtr, LaunchConfig,
    PushKernelArg,
};
use cudarc::nvrtc::Ptx;
use cudarc::optix::{
    sys as optix_sys, OptixAccel, OptixContext, OptixModule, OptixPipeline, OptixProgramGroup,
    OptixSbt,
};

use crate::cpu;
use crate::{RayHit3, RayQuery3, TriangleMeshView};

const OPTIX_GEOMETRY_FLAG_DISABLE_ANYHIT_U32: u32 = 1;

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

// 6. Standard CUDA SIMT Ray-Mesh intersection kernel (FP64 Moller-Trumbore)
.visible .entry cuda_simt_ray_mesh_f64(
    .param .u64 param_rays,
    .param .u64 param_tris,
    .param .u64 param_out,
    .param .u32 param_n_rays,
    .param .u32 param_n_tris
) {
    .reg .pred %p<8>;
    .reg .b32 %r<10>;
    .reg .b64 %rd<14>;
    .reg .f64 %fd<42>;

    ld.param.u64 %rd1, [param_rays];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n_rays];
    ld.param.u32 %r2, [param_n_tris];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_RAY_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    // Each ray is 8 f64s (64 bytes): ox, oy, oz, tmin, dx, dy, dz, tmax
    mul.wide.u32 %rd7, %r6, 64;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.f64 %fd0, [%rd8 + 0];  // ox
    ld.global.f64 %fd1, [%rd8 + 8];  // oy
    ld.global.f64 %fd2, [%rd8 + 16]; // oz
    ld.global.f64 %fd3, [%rd8 + 24]; // tmin
    ld.global.f64 %fd4, [%rd8 + 32]; // dx
    ld.global.f64 %fd5, [%rd8 + 40]; // dy
    ld.global.f64 %fd6, [%rd8 + 48]; // dz
    ld.global.f64 %fd7, [%rd8 + 56]; // tmax

    mov.f64 %fd30, %fd7;             // best_t = tmax
    mov.u32 %r7, 0xffffffff;         // best_tri = u32::MAX
    mov.f64 %fd31, 0d0000000000000000; // best_u
    mov.f64 %fd32, 0d0000000000000000; // best_v

    mov.u32 %r8, 0;
L_RAY_LOOP:
    setp.ge.u32 %p1, %r8, %r2;
    @%p1 bra L_RAY_DONE;

    // Each triangle is 9 f64s (72 bytes): v0(x,y,z), e1(x,y,z), e2(x,y,z)
    mul.wide.u32 %rd9, %r8, 72;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.f64 %fd8,  [%rd10 + 0];  // v0.x
    ld.global.f64 %fd9,  [%rd10 + 8];  // v0.y
    ld.global.f64 %fd10, [%rd10 + 16]; // v0.z
    ld.global.f64 %fd11, [%rd10 + 24]; // e1.x
    ld.global.f64 %fd12, [%rd10 + 32]; // e1.y
    ld.global.f64 %fd13, [%rd10 + 40]; // e1.z
    ld.global.f64 %fd14, [%rd10 + 48]; // e2.x
    ld.global.f64 %fd15, [%rd10 + 56]; // e2.y
    ld.global.f64 %fd16, [%rd10 + 64]; // e2.z

    // pvec = d x e2
    mul.f64 %fd17, %fd6, %fd15;
    neg.f64 %fd17, %fd17;
    fma.rn.f64 %fd17, %fd5, %fd16, %fd17; // pvec.x
    mul.f64 %fd18, %fd4, %fd16;
    neg.f64 %fd18, %fd18;
    fma.rn.f64 %fd18, %fd6, %fd14, %fd18; // pvec.y
    mul.f64 %fd19, %fd5, %fd14;
    neg.f64 %fd19, %fd19;
    fma.rn.f64 %fd19, %fd4, %fd15, %fd19; // pvec.z

    // det = dot(e1, pvec)
    mul.f64 %fd20, %fd13, %fd19;
    fma.rn.f64 %fd20, %fd12, %fd18, %fd20;
    fma.rn.f64 %fd20, %fd11, %fd17, %fd20;

    abs.f64 %fd21, %fd20;
    setp.le.f64 %p2, %fd21, 0d3D719799812DEA11; // 1e-12
    @%p2 bra L_RAY_NEXT;

    div.rn.f64 %fd22, 0d3FF0000000000000, %fd20; // inv_det = 1.0 / det

    // tvec = origin - v0
    sub.f64 %fd23, %fd0, %fd8;
    sub.f64 %fd24, %fd1, %fd9;
    sub.f64 %fd25, %fd2, %fd10;

    // u = dot(tvec, pvec) * inv_det
    mul.f64 %fd26, %fd25, %fd19;
    fma.rn.f64 %fd26, %fd24, %fd18, %fd26;
    fma.rn.f64 %fd26, %fd23, %fd17, %fd26;
    mul.f64 %fd26, %fd26, %fd22;

    setp.lt.f64 %p3, %fd26, 0dBDA5FD7FE1796495; // -1e-11
    setp.gt.f64 %p4, %fd26, 0d3FF000000000AFEC; // 1.0 + 1e-11
    or.pred %p3, %p3, %p4;
    @%p3 bra L_RAY_NEXT;

    // qvec = tvec x e1
    mul.f64 %fd27, %fd25, %fd12;
    neg.f64 %fd27, %fd27;
    fma.rn.f64 %fd27, %fd24, %fd13, %fd27; // qvec.x
    mul.f64 %fd28, %fd23, %fd13;
    neg.f64 %fd28, %fd28;
    fma.rn.f64 %fd28, %fd25, %fd11, %fd28; // qvec.y
    mul.f64 %fd29, %fd24, %fd11;
    neg.f64 %fd29, %fd29;
    fma.rn.f64 %fd29, %fd23, %fd12, %fd29; // qvec.z

    // v = dot(d, qvec) * inv_det
    mul.f64 %fd33, %fd6, %fd29;
    fma.rn.f64 %fd33, %fd5, %fd28, %fd33;
    fma.rn.f64 %fd33, %fd4, %fd27, %fd33;
    mul.f64 %fd33, %fd33, %fd22;

    add.f64 %fd34, %fd26, %fd33;
    setp.lt.f64 %p3, %fd33, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd34, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_RAY_NEXT;

    // t = dot(e2, qvec) * inv_det
    mul.f64 %fd35, %fd16, %fd29;
    fma.rn.f64 %fd35, %fd15, %fd28, %fd35;
    fma.rn.f64 %fd35, %fd14, %fd27, %fd35;
    mul.f64 %fd35, %fd35, %fd22;

    setp.lt.f64 %p5, %fd35, %fd3;
    setp.ge.f64 %p6, %fd35, %fd30;
    or.pred %p5, %p5, %p6;
    @%p5 bra L_RAY_NEXT;

    mov.f64 %fd30, %fd35;
    mov.u32 %r7, %r8;
    mov.f64 %fd31, %fd26;
    mov.f64 %fd32, %fd33;

L_RAY_NEXT:
    add.u32 %r8, %r8, 1;
    bra L_RAY_LOOP;

L_RAY_DONE:
    // Output per ray: 4 f64s (32 bytes): t, tri_idx_bits, u, v
    mul.wide.u32 %rd11, %r6, 32;
    add.u64 %rd12, %rd6, %rd11;
    cvt.u64.u32 %rd13, %r7;
    mov.b64 %fd36, %rd13;
    st.global.f64 [%rd12 + 0],  %fd30;
    st.global.f64 [%rd12 + 8],  %fd36;
    st.global.f64 [%rd12 + 16], %fd31;
    st.global.f64 [%rd12 + 24], %fd32;

L_RAY_EXIT:
    ret;
}

// 6b. Ampere sm_86 non-coherent load Ray-Mesh intersection kernel (FP64 Moller-Trumbore)
.visible .entry cuda_sm86_ray_mesh_f64(
    .param .u64 param_rays,
    .param .u64 param_tris,
    .param .u64 param_out,
    .param .u32 param_n_rays,
    .param .u32 param_n_tris
) {
    .reg .pred %p<8>;
    .reg .b32 %r<10>;
    .reg .b64 %rd<14>;
    .reg .f64 %fd<42>;

    ld.param.u64 %rd1, [param_rays];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n_rays];
    ld.param.u32 %r2, [param_n_tris];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_SM86_RAY_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mul.wide.u32 %rd7, %r6, 64;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.nc.f64 %fd0, [%rd8 + 0];
    ld.global.nc.f64 %fd1, [%rd8 + 8];
    ld.global.nc.f64 %fd2, [%rd8 + 16];
    ld.global.nc.f64 %fd3, [%rd8 + 24];
    ld.global.nc.f64 %fd4, [%rd8 + 32];
    ld.global.nc.f64 %fd5, [%rd8 + 40];
    ld.global.nc.f64 %fd6, [%rd8 + 48];
    ld.global.nc.f64 %fd7, [%rd8 + 56];

    mov.f64 %fd30, %fd7;
    mov.u32 %r7, 0xffffffff;
    mov.f64 %fd31, 0d0000000000000000;
    mov.f64 %fd32, 0d0000000000000000;

    mov.u32 %r8, 0;
L_SM86_RAY_LOOP:
    setp.ge.u32 %p1, %r8, %r2;
    @%p1 bra L_SM86_RAY_DONE;

    mul.wide.u32 %rd9, %r8, 72;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.nc.f64 %fd8,  [%rd10 + 0];
    ld.global.nc.f64 %fd9,  [%rd10 + 8];
    ld.global.nc.f64 %fd10, [%rd10 + 16];
    ld.global.nc.f64 %fd11, [%rd10 + 24];
    ld.global.nc.f64 %fd12, [%rd10 + 32];
    ld.global.nc.f64 %fd13, [%rd10 + 40];
    ld.global.nc.f64 %fd14, [%rd10 + 48];
    ld.global.nc.f64 %fd15, [%rd10 + 56];
    ld.global.nc.f64 %fd16, [%rd10 + 64];

    mul.f64 %fd17, %fd6, %fd15;
    neg.f64 %fd17, %fd17;
    fma.rn.f64 %fd17, %fd5, %fd16, %fd17;
    mul.f64 %fd18, %fd4, %fd16;
    neg.f64 %fd18, %fd18;
    fma.rn.f64 %fd18, %fd6, %fd14, %fd18;
    mul.f64 %fd19, %fd5, %fd14;
    neg.f64 %fd19, %fd19;
    fma.rn.f64 %fd19, %fd4, %fd15, %fd19;

    mul.f64 %fd20, %fd13, %fd19;
    fma.rn.f64 %fd20, %fd12, %fd18, %fd20;
    fma.rn.f64 %fd20, %fd11, %fd17, %fd20;

    abs.f64 %fd21, %fd20;
    setp.le.f64 %p2, %fd21, 0d3D719799812DEA11;
    @%p2 bra L_SM86_RAY_NEXT;

    div.rn.f64 %fd22, 0d3FF0000000000000, %fd20;

    sub.f64 %fd23, %fd0, %fd8;
    sub.f64 %fd24, %fd1, %fd9;
    sub.f64 %fd25, %fd2, %fd10;

    mul.f64 %fd26, %fd25, %fd19;
    fma.rn.f64 %fd26, %fd24, %fd18, %fd26;
    fma.rn.f64 %fd26, %fd23, %fd17, %fd26;
    mul.f64 %fd26, %fd26, %fd22;

    setp.lt.f64 %p3, %fd26, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd26, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_SM86_RAY_NEXT;

    mul.f64 %fd27, %fd25, %fd12;
    neg.f64 %fd27, %fd27;
    fma.rn.f64 %fd27, %fd24, %fd13, %fd27;
    mul.f64 %fd28, %fd23, %fd13;
    neg.f64 %fd28, %fd28;
    fma.rn.f64 %fd28, %fd25, %fd11, %fd28;
    mul.f64 %fd29, %fd24, %fd11;
    neg.f64 %fd29, %fd29;
    fma.rn.f64 %fd29, %fd23, %fd12, %fd29;

    mul.f64 %fd33, %fd6, %fd29;
    fma.rn.f64 %fd33, %fd5, %fd28, %fd33;
    fma.rn.f64 %fd33, %fd4, %fd27, %fd33;
    mul.f64 %fd33, %fd33, %fd22;

    add.f64 %fd34, %fd26, %fd33;
    setp.lt.f64 %p3, %fd33, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd34, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_SM86_RAY_NEXT;

    mul.f64 %fd35, %fd16, %fd29;
    fma.rn.f64 %fd35, %fd15, %fd28, %fd35;
    fma.rn.f64 %fd35, %fd14, %fd27, %fd35;
    mul.f64 %fd35, %fd35, %fd22;

    setp.lt.f64 %p5, %fd35, %fd3;
    setp.ge.f64 %p6, %fd35, %fd30;
    or.pred %p5, %p5, %p6;
    @%p5 bra L_SM86_RAY_NEXT;

    mov.f64 %fd30, %fd35;
    mov.u32 %r7, %r8;
    mov.f64 %fd31, %fd26;
    mov.f64 %fd32, %fd33;

L_SM86_RAY_NEXT:
    add.u32 %r8, %r8, 1;
    bra L_SM86_RAY_LOOP;

L_SM86_RAY_DONE:
    mul.wide.u32 %rd11, %r6, 32;
    add.u64 %rd12, %rd6, %rd11;
    cvt.u64.u32 %rd13, %r7;
    mov.b64 %fd36, %rd13;
    st.global.f64 [%rd12 + 0],  %fd30;
    st.global.f64 [%rd12 + 8],  %fd36;
    st.global.f64 [%rd12 + 16], %fd31;
    st.global.f64 [%rd12 + 24], %fd32;

L_SM86_RAY_EXIT:
    ret;
}

// 6c. Standard CUDA SIMT Point-in-Solid ray-casting parity kernel (FP64)
.visible .entry cuda_simt_point_in_solid_f64(
    .param .u64 param_pts,
    .param .u64 param_tris,
    .param .u64 param_out,
    .param .u32 param_n_pts,
    .param .u32 param_n_tris
) {
    .local .align 8 .b8 %hit_ts[256];
    .reg .pred %p<8>;
    .reg .b32 %r<14>;
    .reg .b64 %rd<16>;
    .reg .f64 %fd<38>;

    ld.param.u64 %rd1, [param_pts];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n_pts];
    ld.param.u32 %r2, [param_n_tris];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_SIMT_PIS_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;
    mov.u64 %rd13, %hit_ts;

    mul.wide.u32 %rd7, %r6, 32;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.f64 %fd0, [%rd8 + 0];  // ox
    ld.global.f64 %fd1, [%rd8 + 8];  // oy
    ld.global.f64 %fd2, [%rd8 + 16]; // oz

    // PARITY_RAY_DIRECTION
    mov.f64 %fd4, 0d3FE279A74590331D; // dx = 0.5773502691896258
    mov.f64 %fd5, 0d3FE11ACEE560242A; // dy = 0.5345224838248488
    mov.f64 %fd6, 0d3FE3C03650E00E03; // dz = 0.6172133998483676

    mov.u32 %r7, 0; // hits
    mov.u32 %r8, 0; // tri_idx
L_SIMT_PIS_LOOP:
    setp.ge.u32 %p1, %r8, %r2;
    @%p1 bra L_SIMT_PIS_DONE;

    mul.wide.u32 %rd9, %r8, 72;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.f64 %fd8,  [%rd10 + 0];
    ld.global.f64 %fd9,  [%rd10 + 8];
    ld.global.f64 %fd10, [%rd10 + 16];
    ld.global.f64 %fd11, [%rd10 + 24];
    ld.global.f64 %fd12, [%rd10 + 32];
    ld.global.f64 %fd13, [%rd10 + 40];
    ld.global.f64 %fd14, [%rd10 + 48];
    ld.global.f64 %fd15, [%rd10 + 56];
    ld.global.f64 %fd16, [%rd10 + 64];

    mul.f64 %fd17, %fd6, %fd15;
    neg.f64 %fd17, %fd17;
    fma.rn.f64 %fd17, %fd5, %fd16, %fd17;
    mul.f64 %fd18, %fd4, %fd16;
    neg.f64 %fd18, %fd18;
    fma.rn.f64 %fd18, %fd6, %fd14, %fd18;
    mul.f64 %fd19, %fd5, %fd14;
    neg.f64 %fd19, %fd19;
    fma.rn.f64 %fd19, %fd4, %fd15, %fd19;

    mul.f64 %fd20, %fd13, %fd19;
    fma.rn.f64 %fd20, %fd12, %fd18, %fd20;
    fma.rn.f64 %fd20, %fd11, %fd17, %fd20;

    abs.f64 %fd21, %fd20;
    setp.le.f64 %p2, %fd21, 0d3D719799812DEA11;
    @%p2 bra L_SIMT_PIS_NEXT;

    div.rn.f64 %fd22, 0d3FF0000000000000, %fd20;

    sub.f64 %fd23, %fd0, %fd8;
    sub.f64 %fd24, %fd1, %fd9;
    sub.f64 %fd25, %fd2, %fd10;

    mul.f64 %fd26, %fd25, %fd19;
    fma.rn.f64 %fd26, %fd24, %fd18, %fd26;
    fma.rn.f64 %fd26, %fd23, %fd17, %fd26;
    mul.f64 %fd26, %fd26, %fd22;

    setp.lt.f64 %p3, %fd26, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd26, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_SIMT_PIS_NEXT;

    mul.f64 %fd27, %fd25, %fd12;
    neg.f64 %fd27, %fd27;
    fma.rn.f64 %fd27, %fd24, %fd13, %fd27;
    mul.f64 %fd28, %fd23, %fd13;
    neg.f64 %fd28, %fd28;
    fma.rn.f64 %fd28, %fd25, %fd11, %fd28;
    mul.f64 %fd29, %fd24, %fd11;
    neg.f64 %fd29, %fd29;
    fma.rn.f64 %fd29, %fd23, %fd12, %fd29;

    mul.f64 %fd33, %fd6, %fd29;
    fma.rn.f64 %fd33, %fd5, %fd28, %fd33;
    fma.rn.f64 %fd33, %fd4, %fd27, %fd33;
    mul.f64 %fd33, %fd33, %fd22;

    add.f64 %fd34, %fd26, %fd33;
    setp.lt.f64 %p3, %fd33, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd34, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_SIMT_PIS_NEXT;

    mul.f64 %fd35, %fd16, %fd29;
    fma.rn.f64 %fd35, %fd15, %fd28, %fd35;
    fma.rn.f64 %fd35, %fd14, %fd27, %fd35;
    mul.f64 %fd35, %fd35, %fd22;

    setp.lt.f64 %p5, %fd35, 0d3E112E0BE826D695; // 1.0e-9
    @%p5 bra L_SIMT_PIS_NEXT;

    // Deduplicate against up to 32 previous hits within 1.0e-9
    min.u32 %r9, %r7, 32;
    mov.u32 %r10, 0;
L_SIMT_PIS_DEDUP:
    setp.ge.u32 %p6, %r10, %r9;
    @%p6 bra L_SIMT_PIS_RECORD;
    mul.wide.u32 %rd14, %r10, 8;
    add.u64 %rd15, %rd13, %rd14;
    ld.local.f64 %fd36, [%rd15];
    sub.f64 %fd37, %fd35, %fd36;
    abs.f64 %fd37, %fd37;
    setp.le.f64 %p7, %fd37, 0d3E112E0BE826D695;
    @%p7 bra L_SIMT_PIS_NEXT;
    add.u32 %r10, %r10, 1;
    bra L_SIMT_PIS_DEDUP;

L_SIMT_PIS_RECORD:
    setp.lt.u32 %p6, %r7, 32;
    @!%p6 bra L_SIMT_PIS_INC;
    mul.wide.u32 %rd14, %r7, 8;
    add.u64 %rd15, %rd13, %rd14;
    st.local.f64 [%rd15], %fd35;
L_SIMT_PIS_INC:
    add.u32 %r7, %r7, 1;

L_SIMT_PIS_NEXT:
    add.u32 %r8, %r8, 1;
    bra L_SIMT_PIS_LOOP;

L_SIMT_PIS_DONE:
    and.b32 %r11, %r7, 1;
    mul.wide.u32 %rd11, %r6, 4;
    add.u64 %rd12, %rd6, %rd11;
    st.global.u32 [%rd12], %r11;

L_SIMT_PIS_EXIT:
    ret;
}

// 6d. Ampere sm_86 non-coherent load Point-in-Solid ray-casting parity kernel (FP64)
.visible .entry cuda_sm86_point_in_solid_f64(
    .param .u64 param_pts,
    .param .u64 param_tris,
    .param .u64 param_out,
    .param .u32 param_n_pts,
    .param .u32 param_n_tris
) {
    .local .align 8 .b8 %hit_ts[256];
    .reg .pred %p<8>;
    .reg .b32 %r<14>;
    .reg .b64 %rd<16>;
    .reg .f64 %fd<38>;

    ld.param.u64 %rd1, [param_pts];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n_pts];
    ld.param.u32 %r2, [param_n_tris];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_SM86_PIS_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;
    mov.u64 %rd13, %hit_ts;

    mul.wide.u32 %rd7, %r6, 32;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.nc.f64 %fd0, [%rd8 + 0];
    ld.global.nc.f64 %fd1, [%rd8 + 8];
    ld.global.nc.f64 %fd2, [%rd8 + 16];

    mov.f64 %fd4, 0d3FE279A74590331D;
    mov.f64 %fd5, 0d3FE11ACEE560242A;
    mov.f64 %fd6, 0d3FE3C03650E00E03;

    mov.u32 %r7, 0;
    mov.u32 %r8, 0;
L_SM86_PIS_LOOP:
    setp.ge.u32 %p1, %r8, %r2;
    @%p1 bra L_SM86_PIS_DONE;

    mul.wide.u32 %rd9, %r8, 72;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.nc.f64 %fd8,  [%rd10 + 0];
    ld.global.nc.f64 %fd9,  [%rd10 + 8];
    ld.global.nc.f64 %fd10, [%rd10 + 16];
    ld.global.nc.f64 %fd11, [%rd10 + 24];
    ld.global.nc.f64 %fd12, [%rd10 + 32];
    ld.global.nc.f64 %fd13, [%rd10 + 40];
    ld.global.nc.f64 %fd14, [%rd10 + 48];
    ld.global.nc.f64 %fd15, [%rd10 + 56];
    ld.global.nc.f64 %fd16, [%rd10 + 64];

    mul.f64 %fd17, %fd6, %fd15;
    neg.f64 %fd17, %fd17;
    fma.rn.f64 %fd17, %fd5, %fd16, %fd17;
    mul.f64 %fd18, %fd4, %fd16;
    neg.f64 %fd18, %fd18;
    fma.rn.f64 %fd18, %fd6, %fd14, %fd18;
    mul.f64 %fd19, %fd5, %fd14;
    neg.f64 %fd19, %fd19;
    fma.rn.f64 %fd19, %fd4, %fd15, %fd19;

    mul.f64 %fd20, %fd13, %fd19;
    fma.rn.f64 %fd20, %fd12, %fd18, %fd20;
    fma.rn.f64 %fd20, %fd11, %fd17, %fd20;

    abs.f64 %fd21, %fd20;
    setp.le.f64 %p2, %fd21, 0d3D719799812DEA11;
    @%p2 bra L_SM86_PIS_NEXT;

    div.rn.f64 %fd22, 0d3FF0000000000000, %fd20;

    sub.f64 %fd23, %fd0, %fd8;
    sub.f64 %fd24, %fd1, %fd9;
    sub.f64 %fd25, %fd2, %fd10;

    mul.f64 %fd26, %fd25, %fd19;
    fma.rn.f64 %fd26, %fd24, %fd18, %fd26;
    fma.rn.f64 %fd26, %fd23, %fd17, %fd26;
    mul.f64 %fd26, %fd26, %fd22;

    setp.lt.f64 %p3, %fd26, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd26, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_SM86_PIS_NEXT;

    mul.f64 %fd27, %fd25, %fd12;
    neg.f64 %fd27, %fd27;
    fma.rn.f64 %fd27, %fd24, %fd13, %fd27;
    mul.f64 %fd28, %fd23, %fd13;
    neg.f64 %fd28, %fd28;
    fma.rn.f64 %fd28, %fd25, %fd11, %fd28;
    mul.f64 %fd29, %fd24, %fd11;
    neg.f64 %fd29, %fd29;
    fma.rn.f64 %fd29, %fd23, %fd12, %fd29;

    mul.f64 %fd33, %fd6, %fd29;
    fma.rn.f64 %fd33, %fd5, %fd28, %fd33;
    fma.rn.f64 %fd33, %fd4, %fd27, %fd33;
    mul.f64 %fd33, %fd33, %fd22;

    add.f64 %fd34, %fd26, %fd33;
    setp.lt.f64 %p3, %fd33, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd34, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_SM86_PIS_NEXT;

    mul.f64 %fd35, %fd16, %fd29;
    fma.rn.f64 %fd35, %fd15, %fd28, %fd35;
    fma.rn.f64 %fd35, %fd14, %fd27, %fd35;
    mul.f64 %fd35, %fd35, %fd22;

    setp.lt.f64 %p5, %fd35, 0d3E112E0BE826D695;
    @%p5 bra L_SM86_PIS_NEXT;

    min.u32 %r9, %r7, 32;
    mov.u32 %r10, 0;
L_SM86_PIS_DEDUP:
    setp.ge.u32 %p6, %r10, %r9;
    @%p6 bra L_SM86_PIS_RECORD;
    mul.wide.u32 %rd14, %r10, 8;
    add.u64 %rd15, %rd13, %rd14;
    ld.local.f64 %fd36, [%rd15];
    sub.f64 %fd37, %fd35, %fd36;
    abs.f64 %fd37, %fd37;
    setp.le.f64 %p7, %fd37, 0d3E112E0BE826D695;
    @%p7 bra L_SM86_PIS_NEXT;
    add.u32 %r10, %r10, 1;
    bra L_SM86_PIS_DEDUP;

L_SM86_PIS_RECORD:
    setp.lt.u32 %p6, %r7, 32;
    @!%p6 bra L_SM86_PIS_INC;
    mul.wide.u32 %rd14, %r7, 8;
    add.u64 %rd15, %rd13, %rd14;
    st.local.f64 [%rd15], %fd35;
L_SM86_PIS_INC:
    add.u32 %r7, %r7, 1;

L_SM86_PIS_NEXT:
    add.u32 %r8, %r8, 1;
    bra L_SM86_PIS_LOOP;

L_SM86_PIS_DONE:
    and.b32 %r11, %r7, 1;
    mul.wide.u32 %rd11, %r6, 4;
    add.u64 %rd12, %rd6, %rd11;
    st.global.u32 [%rd12], %r11;

L_SM86_PIS_EXIT:
    ret;
}

// 11. CUDA OptiX RT Core + FP64 Ray-Mesh intersection refinement kernel
.visible .entry cuda_optix_ray_mesh_f64(
    .param .u64 param_rays,
    .param .u64 param_tris,
    .param .u64 param_cand_prim,
    .param .u64 param_out,
    .param .u32 param_n_rays,
    .param .u32 param_n_tris
) {
    .reg .pred %p<8>;
    .reg .b32 %r<12>;
    .reg .b64 %rd<16>;
    .reg .f64 %fd<42>;

    ld.param.u64 %rd1, [param_rays];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd14, [param_cand_prim];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n_rays];
    ld.param.u32 %r2, [param_n_tris];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_OPTIX_RAY_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd15, %rd14;
    cvta.to.global.u64 %rd6, %rd3;

    mul.wide.u32 %rd7, %r6, 64;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.nc.f64 %fd0, [%rd8 + 0];
    ld.global.nc.f64 %fd1, [%rd8 + 8];
    ld.global.nc.f64 %fd2, [%rd8 + 16];
    ld.global.nc.f64 %fd3, [%rd8 + 24];
    ld.global.nc.f64 %fd4, [%rd8 + 32];
    ld.global.nc.f64 %fd5, [%rd8 + 40];
    ld.global.nc.f64 %fd6, [%rd8 + 48];
    ld.global.nc.f64 %fd7, [%rd8 + 56];

    mov.f64 %fd30, %fd7;
    mov.u32 %r7, 0xffffffff;
    mov.f64 %fd31, 0d0000000000000000;
    mov.f64 %fd32, 0d0000000000000000;

    mul.wide.u32 %rd7, %r6, 4;
    add.u64 %rd8, %rd15, %rd7;
    ld.global.nc.u32 %r9, [%rd8];

    mov.u32 %r10, 0;
    setp.lt.u32 %p1, %r9, %r2;
    @!%p1 bra L_OPTIX_RAY_CHECK_SMALL;
    mov.u32 %r8, %r9;
    bra L_OPTIX_RAY_EVAL;

L_OPTIX_RAY_CHECK_SMALL:
    setp.le.u32 %p1, %r2, 16;
    @!%p1 bra L_OPTIX_RAY_DONE;

L_OPTIX_RAY_START_LOOP:
    mov.u32 %r10, 1;
    mov.u32 %r8, 0;

L_OPTIX_RAY_LOOP:
    setp.ge.u32 %p1, %r8, %r2;
    @%p1 bra L_OPTIX_RAY_DONE;
    setp.eq.u32 %p1, %r8, %r9;
    @%p1 bra L_OPTIX_RAY_NEXT;

L_OPTIX_RAY_EVAL:
    mul.wide.u32 %rd9, %r8, 72;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.nc.f64 %fd8,  [%rd10 + 0];
    ld.global.nc.f64 %fd9,  [%rd10 + 8];
    ld.global.nc.f64 %fd10, [%rd10 + 16];
    ld.global.nc.f64 %fd11, [%rd10 + 24];
    ld.global.nc.f64 %fd12, [%rd10 + 32];
    ld.global.nc.f64 %fd13, [%rd10 + 40];
    ld.global.nc.f64 %fd14, [%rd10 + 48];
    ld.global.nc.f64 %fd15, [%rd10 + 56];
    ld.global.nc.f64 %fd16, [%rd10 + 64];

    mul.f64 %fd17, %fd6, %fd15;
    neg.f64 %fd17, %fd17;
    fma.rn.f64 %fd17, %fd5, %fd16, %fd17;
    mul.f64 %fd18, %fd4, %fd16;
    neg.f64 %fd18, %fd18;
    fma.rn.f64 %fd18, %fd6, %fd14, %fd18;
    mul.f64 %fd19, %fd5, %fd14;
    neg.f64 %fd19, %fd19;
    fma.rn.f64 %fd19, %fd4, %fd15, %fd19;

    mul.f64 %fd20, %fd13, %fd19;
    fma.rn.f64 %fd20, %fd12, %fd18, %fd20;
    fma.rn.f64 %fd20, %fd11, %fd17, %fd20;

    abs.f64 %fd21, %fd20;
    setp.le.f64 %p2, %fd21, 0d3D719799812DEA11;
    @%p2 bra L_OPTIX_RAY_AFTER_EVAL;

    div.rn.f64 %fd22, 0d3FF0000000000000, %fd20;

    sub.f64 %fd23, %fd0, %fd8;
    sub.f64 %fd24, %fd1, %fd9;
    sub.f64 %fd25, %fd2, %fd10;

    mul.f64 %fd26, %fd25, %fd19;
    fma.rn.f64 %fd26, %fd24, %fd18, %fd26;
    fma.rn.f64 %fd26, %fd23, %fd17, %fd26;
    mul.f64 %fd26, %fd26, %fd22;

    setp.lt.f64 %p3, %fd26, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd26, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_OPTIX_RAY_AFTER_EVAL;

    mul.f64 %fd27, %fd25, %fd12;
    neg.f64 %fd27, %fd27;
    fma.rn.f64 %fd27, %fd24, %fd13, %fd27;
    mul.f64 %fd28, %fd23, %fd13;
    neg.f64 %fd28, %fd28;
    fma.rn.f64 %fd28, %fd25, %fd11, %fd28;
    mul.f64 %fd29, %fd24, %fd11;
    neg.f64 %fd29, %fd29;
    fma.rn.f64 %fd29, %fd23, %fd12, %fd29;

    mul.f64 %fd33, %fd6, %fd29;
    fma.rn.f64 %fd33, %fd5, %fd28, %fd33;
    fma.rn.f64 %fd33, %fd4, %fd27, %fd33;
    mul.f64 %fd33, %fd33, %fd22;

    add.f64 %fd34, %fd26, %fd33;
    setp.lt.f64 %p3, %fd33, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd34, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_OPTIX_RAY_AFTER_EVAL;

    mul.f64 %fd35, %fd16, %fd29;
    fma.rn.f64 %fd35, %fd15, %fd28, %fd35;
    fma.rn.f64 %fd35, %fd14, %fd27, %fd35;
    mul.f64 %fd35, %fd35, %fd22;

    setp.lt.f64 %p5, %fd35, %fd3;
    setp.ge.f64 %p6, %fd35, %fd30;
    or.pred %p5, %p5, %p6;
    @%p5 bra L_OPTIX_RAY_AFTER_EVAL;

    mov.f64 %fd30, %fd35;
    mov.u32 %r7, %r8;
    mov.f64 %fd31, %fd26;
    mov.f64 %fd32, %fd33;

L_OPTIX_RAY_AFTER_EVAL:
    setp.eq.u32 %p1, %r10, 0;
    @%p1 bra L_OPTIX_RAY_CHECK_SMALL;

L_OPTIX_RAY_NEXT:
    add.u32 %r8, %r8, 1;
    bra L_OPTIX_RAY_LOOP;

L_OPTIX_RAY_DONE:
    mul.wide.u32 %rd11, %r6, 32;
    add.u64 %rd12, %rd6, %rd11;
    cvt.u64.u32 %rd13, %r7;
    mov.b64 %fd36, %rd13;
    st.global.f64 [%rd12 + 0],  %fd30;
    st.global.f64 [%rd12 + 8],  %fd36;
    st.global.f64 [%rd12 + 16], %fd31;
    st.global.f64 [%rd12 + 24], %fd32;

L_OPTIX_RAY_EXIT:
    ret;
}

// 12. CUDA OptiX RT Core + FP64 Point-in-Solid ray-casting parity kernel
.visible .entry cuda_optix_point_in_solid_f64(
    .param .u64 param_pts,
    .param .u64 param_tris,
    .param .u64 param_cand_prim8,
    .param .u64 param_out,
    .param .u32 param_n_pts,
    .param .u32 param_n_tris
) {
    .local .align 8 .b8 %hit_ts[256];
    .reg .pred %p<8>;
    .reg .b32 %r<16>;
    .reg .b64 %rd<20>;
    .reg .f64 %fd<38>;

    ld.param.u64 %rd1, [param_pts];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd16, [param_cand_prim8];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n_pts];
    ld.param.u32 %r2, [param_n_tris];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_OPTIX_PIS_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd17, %rd16;
    cvta.to.global.u64 %rd6, %rd3;
    mov.u64 %rd13, %hit_ts;

    mul.wide.u32 %rd7, %r6, 32;
    add.u64 %rd8, %rd4, %rd7;
    ld.global.nc.f64 %fd0, [%rd8 + 0];
    ld.global.nc.f64 %fd1, [%rd8 + 8];
    ld.global.nc.f64 %fd2, [%rd8 + 16];

    mov.f64 %fd4, 0d3FE279A74590331D;
    mov.f64 %fd5, 0d3FE11ACEE560242A;
    mov.f64 %fd6, 0d3FE3C03650E00E03;

    add.u64 %rd18, %rd17, %rd7;
    ld.global.nc.u32 %r12, [%rd18 + 0];

    mov.u32 %r7, 0;
    setp.le.u32 %p1, %r2, 16;
    setp.gt.u32 %p2, %r12, 7;
    or.pred %p1, %p1, %p2;
    @%p1 bra L_OPTIX_PIS_START_LOOP;

    mov.u32 %r13, 0;
    mov.u32 %r14, 0;

L_OPTIX_PIS_CAND_LOOP:
    setp.ge.u32 %p1, %r14, %r12;
    @%p1 bra L_OPTIX_PIS_DONE;
    add.u32 %r14, %r14, 1;
    mul.wide.u32 %rd19, %r14, 4;
    add.u64 %rd19, %rd18, %rd19;
    ld.global.nc.u32 %r8, [%rd19];
    setp.lt.u32 %p1, %r8, %r2;
    @%p1 bra L_OPTIX_PIS_EVAL;
    bra L_OPTIX_PIS_CAND_LOOP;

L_OPTIX_PIS_START_LOOP:
    mov.u32 %r13, 1;
    mov.u32 %r8, 0;

L_OPTIX_PIS_LOOP:
    setp.ge.u32 %p1, %r8, %r2;
    @%p1 bra L_OPTIX_PIS_DONE;

L_OPTIX_PIS_EVAL:
    mul.wide.u32 %rd9, %r8, 72;
    add.u64 %rd10, %rd5, %rd9;
    ld.global.nc.f64 %fd8,  [%rd10 + 0];
    ld.global.nc.f64 %fd9,  [%rd10 + 8];
    ld.global.nc.f64 %fd10, [%rd10 + 16];
    ld.global.nc.f64 %fd11, [%rd10 + 24];
    ld.global.nc.f64 %fd12, [%rd10 + 32];
    ld.global.nc.f64 %fd13, [%rd10 + 40];
    ld.global.nc.f64 %fd14, [%rd10 + 48];
    ld.global.nc.f64 %fd15, [%rd10 + 56];
    ld.global.nc.f64 %fd16, [%rd10 + 64];

    mul.f64 %fd17, %fd6, %fd15;
    neg.f64 %fd17, %fd17;
    fma.rn.f64 %fd17, %fd5, %fd16, %fd17;
    mul.f64 %fd18, %fd4, %fd16;
    neg.f64 %fd18, %fd18;
    fma.rn.f64 %fd18, %fd6, %fd14, %fd18;
    mul.f64 %fd19, %fd5, %fd14;
    neg.f64 %fd19, %fd19;
    fma.rn.f64 %fd19, %fd4, %fd15, %fd19;

    mul.f64 %fd20, %fd13, %fd19;
    fma.rn.f64 %fd20, %fd12, %fd18, %fd20;
    fma.rn.f64 %fd20, %fd11, %fd17, %fd20;

    abs.f64 %fd21, %fd20;
    setp.le.f64 %p2, %fd21, 0d3D719799812DEA11;
    @%p2 bra L_OPTIX_PIS_AFTER_EVAL;

    div.rn.f64 %fd22, 0d3FF0000000000000, %fd20;

    sub.f64 %fd23, %fd0, %fd8;
    sub.f64 %fd24, %fd1, %fd9;
    sub.f64 %fd25, %fd2, %fd10;

    mul.f64 %fd26, %fd25, %fd19;
    fma.rn.f64 %fd26, %fd24, %fd18, %fd26;
    fma.rn.f64 %fd26, %fd23, %fd17, %fd26;
    mul.f64 %fd26, %fd26, %fd22;

    setp.lt.f64 %p3, %fd26, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd26, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_OPTIX_PIS_AFTER_EVAL;

    mul.f64 %fd27, %fd25, %fd12;
    neg.f64 %fd27, %fd27;
    fma.rn.f64 %fd27, %fd24, %fd13, %fd27;
    mul.f64 %fd28, %fd23, %fd13;
    neg.f64 %fd28, %fd28;
    fma.rn.f64 %fd28, %fd25, %fd11, %fd28;
    mul.f64 %fd29, %fd24, %fd11;
    neg.f64 %fd29, %fd29;
    fma.rn.f64 %fd29, %fd23, %fd12, %fd29;

    mul.f64 %fd33, %fd6, %fd29;
    fma.rn.f64 %fd33, %fd5, %fd28, %fd33;
    fma.rn.f64 %fd33, %fd4, %fd27, %fd33;
    mul.f64 %fd33, %fd33, %fd22;

    add.f64 %fd34, %fd26, %fd33;
    setp.lt.f64 %p3, %fd33, 0dBDA5FD7FE1796495;
    setp.gt.f64 %p4, %fd34, 0d3FF000000000AFEC;
    or.pred %p3, %p3, %p4;
    @%p3 bra L_OPTIX_PIS_AFTER_EVAL;

    mul.f64 %fd35, %fd16, %fd29;
    fma.rn.f64 %fd35, %fd15, %fd28, %fd35;
    fma.rn.f64 %fd35, %fd14, %fd27, %fd35;
    mul.f64 %fd35, %fd35, %fd22;

    setp.lt.f64 %p5, %fd35, 0d3E112E0BE826D695;
    @%p5 bra L_OPTIX_PIS_AFTER_EVAL;

    min.u32 %r9, %r7, 32;
    mov.u32 %r10, 0;
L_OPTIX_PIS_DEDUP:
    setp.ge.u32 %p6, %r10, %r9;
    @%p6 bra L_OPTIX_PIS_RECORD;
    mul.wide.u32 %rd14, %r10, 8;
    add.u64 %rd15, %rd13, %rd14;
    ld.local.f64 %fd36, [%rd15];
    sub.f64 %fd37, %fd35, %fd36;
    abs.f64 %fd37, %fd37;
    setp.le.f64 %p7, %fd37, 0d3E112E0BE826D695;
    @%p7 bra L_OPTIX_PIS_AFTER_EVAL;
    add.u32 %r10, %r10, 1;
    bra L_OPTIX_PIS_DEDUP;

L_OPTIX_PIS_RECORD:
    setp.lt.u32 %p6, %r7, 32;
    @!%p6 bra L_OPTIX_PIS_INC;
    mul.wide.u32 %rd14, %r7, 8;
    add.u64 %rd15, %rd13, %rd14;
    st.local.f64 [%rd15], %fd35;
L_OPTIX_PIS_INC:
    add.u32 %r7, %r7, 1;

L_OPTIX_PIS_AFTER_EVAL:
    setp.eq.u32 %p1, %r13, 0;
    @%p1 bra L_OPTIX_PIS_CAND_LOOP;

    add.u32 %r8, %r8, 1;
    bra L_OPTIX_PIS_LOOP;

L_OPTIX_PIS_DONE:
    and.b32 %r11, %r7, 1;
    mul.wide.u32 %rd11, %r6, 4;
    add.u64 %rd12, %rd6, %rd11;
    st.global.u32 [%rd12], %r11;

L_OPTIX_PIS_EXIT:
    ret;
}

";

const OPTIX_PTX_MODULE: &str = "\
.version 7.0
.target sm_75
.address_size 64

.const .align 8 .b8 params[32];

.extern .func (.reg .u32 $value) _optix_get_launch_index_x ();
.extern .func (.reg .u32 $value) _optix_read_primitive_idx ();
.extern .func () _optix_set_payload_0 (.reg .u32 $value);
.extern .func () _optix_ignore_intersection ();
.extern .func (.reg .u32 p0) _optix_trace_1 (
    .reg .u64 handle,
    .reg .f32 rayOriginX,
    .reg .f32 rayOriginY,
    .reg .f32 rayOriginZ,
    .reg .f32 rayDirectionX,
    .reg .f32 rayDirectionY,
    .reg .f32 rayDirectionZ,
    .reg .f32 tmin,
    .reg .f32 tmax,
    .reg .f32 rayTime,
    .reg .u32 visibilityMask,
    .reg .u32 rayFlags,
    .reg .u32 SBToffset,
    .reg .u32 SBTstride,
    .reg .u32 missSBTIndex,
    .reg .u32 p0
);

.visible .entry __raygen__candidate()
{
    .reg .u32 %idx, %prim, %vis, %flags, %sbt_off, %sbt_stride, %miss_idx;
    .reg .u64 %handle, %rays_base, %out_prim_base, %off64, %off4, %ray_ptr, %out_prim_ptr;
    .reg .f64 %ox64, %oy64, %oz64, %dx64, %dy64, %dz64;
    .reg .f32 %ox, %oy, %oz, %dx, %dy, %dz, %tmin, %tmax, %rtime;

    call (%idx), _optix_get_launch_index_x, ();
    ld.const.u64 %handle, [params + 0];
    ld.const.u64 %rays_base, [params + 8];
    ld.const.u64 %out_prim_base, [params + 16];

    mul.wide.u32 %off64, %idx, 64;
    add.u64 %ray_ptr, %rays_base, %off64;

    ld.global.f64 %ox64, [%ray_ptr + 0];
    ld.global.f64 %oy64, [%ray_ptr + 8];
    ld.global.f64 %oz64, [%ray_ptr + 16];
    ld.global.f64 %dx64, [%ray_ptr + 32];
    ld.global.f64 %dy64, [%ray_ptr + 40];
    ld.global.f64 %dz64, [%ray_ptr + 48];

    cvt.rn.f32.f64 %ox, %ox64;
    cvt.rn.f32.f64 %oy, %oy64;
    cvt.rn.f32.f64 %oz, %oz64;
    cvt.rn.f32.f64 %dx, %dx64;
    cvt.rn.f32.f64 %dy, %dy64;
    cvt.rn.f32.f64 %dz, %dz64;

    mov.f32 %tmin, 0f00000000;
    mov.f32 %tmax, 0f7149F2CA;
    mov.f32 %rtime, 0f00000000;
    mov.u32 %vis, 255;
    mov.u32 %flags, 0;
    mov.u32 %sbt_off, 0;
    mov.u32 %sbt_stride, 1;
    mov.u32 %miss_idx, 0;
    mov.u32 %prim, -1;

    call (%prim), _optix_trace_1, (
        %handle,
        %ox, %oy, %oz,
        %dx, %dy, %dz,
        %tmin, %tmax, %rtime,
        %vis, %flags, %sbt_off, %sbt_stride, %miss_idx,
        %prim
    );

    mul.wide.u32 %off4, %idx, 4;
    add.u64 %out_prim_ptr, %out_prim_base, %off4;
    st.global.u32 [%out_prim_ptr], %prim;
    ret;
}

.visible .entry __raygen__parity()
{
    .reg .u32 %idx, %prim, %vis, %flags, %sbt_off, %sbt_stride, %miss_idx;
    .reg .u64 %handle, %pts_base, %out_prim_base, %off32, %pt_ptr, %row_ptr;
    .reg .f64 %ox64, %oy64, %oz64;
    .reg .f32 %ox, %oy, %oz, %dx, %dy, %dz, %tmin, %tmax, %rtime;

    call (%idx), _optix_get_launch_index_x, ();
    ld.const.u64 %handle, [params + 0];
    ld.const.u64 %pts_base, [params + 8];
    ld.const.u64 %out_prim_base, [params + 16];

    mul.wide.u32 %off32, %idx, 32;
    add.u64 %pt_ptr, %pts_base, %off32;
    add.u64 %row_ptr, %out_prim_base, %off32;
    st.global.u32 [%row_ptr + 0], 0;

    ld.global.f64 %ox64, [%pt_ptr + 0];
    ld.global.f64 %oy64, [%pt_ptr + 8];
    ld.global.f64 %oz64, [%pt_ptr + 16];

    cvt.rn.f32.f64 %ox, %ox64;
    cvt.rn.f32.f64 %oy, %oy64;
    cvt.rn.f32.f64 %oz, %oz64;

    mov.f32 %dx, 0f3F13CD3A;
    mov.f32 %dy, 0f3F08D5B7;
    mov.f32 %dz, 0f3F1E00D2;
    mov.f32 %tmin, 0f00000000;
    mov.f32 %tmax, 0f7149F2CA;
    mov.f32 %rtime, 0f00000000;
    mov.u32 %vis, 255;
    mov.u32 %flags, 0;
    mov.u32 %sbt_off, 0;
    mov.u32 %sbt_stride, 1;
    mov.u32 %miss_idx, 0;
    mov.u32 %prim, -1;

    call (%prim), _optix_trace_1, (
        %handle,
        %ox, %oy, %oz,
        %dx, %dy, %dz,
        %tmin, %tmax, %rtime,
        %vis, %flags, %sbt_off, %sbt_stride, %miss_idx,
        %prim
    );
    ret;
}

.visible .entry __anyhit__parity()
{
    .reg .pred %p0, %p1;
    .reg .u32 %idx, %prim, %cnt, %i, %existing, %cnt_next;
    .reg .u64 %out_prim_base, %off32, %row_ptr, %off_slot, %slot_ptr;

    call (%idx), _optix_get_launch_index_x, ();
    call (%prim), _optix_read_primitive_idx, ();
    ld.const.u64 %out_prim_base, [params + 16];

    mul.wide.u32 %off32, %idx, 32;
    add.u64 %row_ptr, %out_prim_base, %off32;
    ld.global.u32 %cnt, [%row_ptr + 0];

    mov.u32 %i, 0;
L_AH_DEDUP:
    setp.ge.u32 %p0, %i, %cnt;
    @%p0 bra L_AH_INSERT;
    setp.ge.u32 %p0, %i, 7;
    @%p0 bra L_AH_INSERT;
    mul.wide.u32 %off_slot, %i, 4;
    add.u64 %slot_ptr, %row_ptr, %off_slot;
    ld.global.u32 %existing, [%slot_ptr + 4];
    setp.eq.u32 %p1, %existing, %prim;
    @%p1 bra L_AH_IGNORE;
    add.u32 %i, %i, 1;
    bra L_AH_DEDUP;

L_AH_INSERT:
    setp.ge.u32 %p0, %cnt, 7;
    @%p0 bra L_AH_COUNT_ONLY;
    mul.wide.u32 %off_slot, %cnt, 4;
    add.u64 %slot_ptr, %row_ptr, %off_slot;
    st.global.u32 [%slot_ptr + 4], %prim;

L_AH_COUNT_ONLY:
    add.u32 %cnt_next, %cnt, 1;
    st.global.u32 [%row_ptr + 0], %cnt_next;

L_AH_IGNORE:
    call (), _optix_ignore_intersection, ();
    ret;
}

.visible .entry __closesthit__ch()
{
    .reg .u32 %prim;
    call (%prim), _optix_read_primitive_idx, ();
    call (), _optix_set_payload_0, (%prim);
    ret;
}

.visible .entry __miss__ms()
{
    .reg .u32 %none;
    mov.u32 %none, -1;
    call (), _optix_set_payload_0, (%none);
    ret;
}
";

fn f64_to_blas_f32(val: f64) -> f32 {
    if !val.is_finite() || val == 0.0 {
        return 0.0;
    }
    let clamped = val.clamp(-1.0e15, 1.0e15);
    let bits = clamped.to_bits();
    let sign_u32 = u32::try_from(bits >> 63).unwrap_or(0) << 31;
    let exp_i32 = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0) - 1023;
    if exp_i32 < -126 {
        return 0.0;
    }
    if exp_i32 > 127 {
        return f32::from_bits(sign_u32 | (254 << 23) | 0x007f_ffff);
    }
    let exp_f32 = u32::try_from(exp_i32 + 127).unwrap_or(0) << 23;
    let mant_24 = (bits & 0x000f_ffff_ffff_ffff) >> 28;
    let sticky = (bits & ((1_u64 << 28) - 1)) != 0;
    let frac_23 = mant_24 >> 1;
    let round_bit = mant_24 & 1;
    let round_up = round_bit == 1 && (sticky || (frac_23 & 1) == 1);
    let f32_bits = sign_u32 | (exp_f32 + u32::try_from(frac_23 + u64::from(round_up)).unwrap_or(0));
    f32::from_bits(f32_bits)
}

struct CachedOptixMesh {
    hash: u64,
    n_tris: u32,
    n_nodes: u32,
    r_pad_sq: f64,
    blas_closest: OptixAccel,
    blas_anyhit: OptixAccel,
    blas_aabb: OptixAccel,
    d_tris_edges: CudaSlice<f64>,
    d_tris_verts: CudaSlice<f64>,
    d_bvh: CudaSlice<f64>,
}

struct OptixPipelines {
    ctx: Arc<OptixContext>,
    ray_pipeline: Arc<OptixPipeline>,
    ray_sbt: OptixSbt,
    parity_pipeline: Arc<OptixPipeline>,
    parity_sbt: OptixSbt,
    fn_optix_ray_mesh: CudaFunction,
    fn_optix_point_in_solid: CudaFunction,
    mesh_cache: Mutex<Option<CachedOptixMesh>>,
}

impl OptixPipelines {
    fn probe(
        ctx: &Arc<CudarcContext>,
        cuda_module: &Arc<cudarc::driver::CudaModule>,
    ) -> Option<Self> {
        if !OptixContext::is_available() {
            return None;
        }
        let optix_ctx = OptixContext::new(ctx).ok()?;
        if optix_ctx.rtcore_version().ok()? == 0 {
            return None;
        }
        let stream = ctx.default_stream();
        let module_opts = optix_sys::OptixModuleCompileOptions::default();
        let pipeline_opts = optix_sys::OptixPipelineCompileOptions {
            numPayloadValues: 1,
            numAttributeValues: 2,
            ..Default::default()
        };
        let link_opts = optix_sys::OptixPipelineLinkOptions::default();

        let optix_module =
            OptixModule::from_ptx(&optix_ctx, OPTIX_PTX_MODULE, &module_opts, &pipeline_opts)
                .ok()?;
        let rg_ray =
            OptixProgramGroup::new_raygen(&optix_ctx, &optix_module, "__raygen__candidate").ok()?;
        let rg_parity =
            OptixProgramGroup::new_raygen(&optix_ctx, &optix_module, "__raygen__parity").ok()?;
        let ms =
            OptixProgramGroup::new_miss(&optix_ctx, Some((&optix_module, "__miss__ms"))).ok()?;
        let hg_closest = OptixProgramGroup::new_hitgroup(
            &optix_ctx,
            Some((&optix_module, "__closesthit__ch")),
            None,
            None,
        )
        .ok()?;
        let hg_parity = OptixProgramGroup::new_hitgroup(
            &optix_ctx,
            None,
            Some((&optix_module, "__anyhit__parity")),
            None,
        )
        .ok()?;

        let ray_groups = [rg_ray.clone(), ms.clone(), hg_closest.clone()];
        let ray_pipeline =
            OptixPipeline::new(&optix_ctx, &pipeline_opts, &link_opts, &ray_groups, 1).ok()?;
        let ray_sbt = OptixSbt::from_groups(&stream, &rg_ray, &[&ms], &[&hg_closest]).ok()?;

        let parity_groups = [rg_parity.clone(), ms.clone(), hg_parity.clone()];
        let parity_pipeline =
            OptixPipeline::new(&optix_ctx, &pipeline_opts, &link_opts, &parity_groups, 1).ok()?;
        let parity_sbt = OptixSbt::from_groups(&stream, &rg_parity, &[&ms], &[&hg_parity]).ok()?;

        Some(Self {
            ctx: optix_ctx,
            ray_pipeline,
            ray_sbt,
            parity_pipeline,
            parity_sbt,
            fn_optix_ray_mesh: cuda_module.load_function("cuda_optix_ray_mesh_f64").ok()?,
            fn_optix_point_in_solid: cuda_module
                .load_function("cuda_optix_point_in_solid_f64")
                .ok()?,
            mesh_cache: Mutex::new(None),
        })
    }
}

struct CachedCudaMesh {
    hash: u64,
    n_tris: u32,
    n_nodes: u32,
    d_tris_edges: CudaSlice<f64>,
    d_tris_verts: CudaSlice<f64>,
    d_bvh: CudaSlice<f64>,
}

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
    fn_simt_ray_mesh: CudaFunction,
    fn_sm86_ray_mesh: CudaFunction,
    fn_simt_point_in_solid: CudaFunction,
    fn_sm86_point_in_solid: CudaFunction,
    compute_mesh_cache: Mutex<Option<CachedCudaMesh>>,
    optix: Option<OptixPipelines>,
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
        let optix = OptixPipelines::probe(&ctx, &module);

        Some(Self {
            fn_simt_transform: module.load_function("cuda_simt_transform_4x4_f64").ok()?,
            fn_sm86_transform: module.load_function("cuda_sm86_transform_4x4_f64").ok()?,
            fn_tc_transform: module.load_function("rtx_tc_transform_4x4_f64").ok()?,
            fn_simt_pairwise_dist: module.load_function("cuda_simt_pairwise_dist_f64").ok()?,
            fn_sm86_pairwise_dist: module.load_function("cuda_sm86_pairwise_dist_f64").ok()?,
            fn_simt_bezier_eval: module.load_function("cuda_simt_bezier_eval_f64").ok()?,
            fn_sm86_bezier_eval: module.load_function("cuda_sm86_bezier_eval_f64").ok()?,
            fn_tc_bezier_eval: module.load_function("rtx_tc_bezier_eval_f64").ok()?,
            fn_simt_ray_mesh: module.load_function("cuda_simt_ray_mesh_f64").ok()?,
            fn_sm86_ray_mesh: module.load_function("cuda_sm86_ray_mesh_f64").ok()?,
            fn_simt_point_in_solid: module.load_function("cuda_simt_point_in_solid_f64").ok()?,
            fn_sm86_point_in_solid: module.load_function("cuda_sm86_point_in_solid_f64").ok()?,
            compute_mesh_cache: Mutex::new(None),
            optix,
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

    fn build_cached_cuda_mesh(
        stream: &Arc<CudaStream>,
        mesh: TriangleMeshView<'_>,
        hash: u64,
    ) -> Option<CachedCudaMesh> {
        let n_tris = u32::try_from(mesh.triangles.len()).ok()?;
        let flat_bvh = crate::spatial::MeshBvh3::build(mesh).flatten();
        let n_nodes = u32::try_from(flat_bvh.nodes().len()).ok()?;

        let mut flat_tris_edges: Vec<f64> =
            Vec::with_capacity(mesh.triangles.len().checked_mul(9)?);
        let mut flat_tris_verts: Vec<f64> =
            Vec::with_capacity(mesh.triangles.len().checked_mul(9)?);
        for tri in mesh.triangles {
            let i0 = usize::try_from(tri[0]).ok()?;
            let i1 = usize::try_from(tri[1]).ok()?;
            let i2 = usize::try_from(tri[2]).ok()?;
            let v0 = *mesh.vertices.get(i0)?;
            let v1 = *mesh.vertices.get(i1)?;
            let v2 = *mesh.vertices.get(i2)?;
            flat_tris_edges.extend_from_slice(&[
                v0.x,
                v0.y,
                v0.z,
                v1.x - v0.x,
                v1.y - v0.y,
                v1.z - v0.z,
                v2.x - v0.x,
                v2.y - v0.y,
                v2.z - v0.z,
            ]);
            flat_tris_verts
                .extend_from_slice(&[v0.x, v0.y, v0.z, v1.x, v1.y, v1.z, v2.x, v2.y, v2.z]);
        }

        let flat_bvh_buf: Vec<f64> = flat_bvh
            .nodes()
            .iter()
            .flat_map(|n| {
                let meta = (u64::from(n.right_child) << 32) | u64::from(n.triangle_index);
                [
                    n.aabb_min.x,
                    n.aabb_min.y,
                    n.aabb_min.z,
                    n.aabb_max.x,
                    n.aabb_max.y,
                    n.aabb_max.z,
                    f64::from_bits(meta),
                    0.0,
                ]
            })
            .collect();

        let d_tris_edges = stream.clone_htod(&flat_tris_edges).ok()?;
        let d_tris_verts = stream.clone_htod(&flat_tris_verts).ok()?;
        let d_bvh = stream.clone_htod(&flat_bvh_buf).ok()?;

        Some(CachedCudaMesh {
            hash,
            n_tris,
            n_nodes,
            d_tris_edges,
            d_tris_verts,
            d_bvh,
        })
    }

    fn with_cached_cuda_mesh<R>(
        &self,
        stream: &Arc<CudaStream>,
        mesh: TriangleMeshView<'_>,
        f: impl FnOnce(&CachedCudaMesh) -> Option<R>,
    ) -> Option<R> {
        let hash = Self::hash_mesh(mesh);
        let mut guard = self.compute_mesh_cache.lock().ok()?;
        let needs_rebuild = match guard.as_ref() {
            Some(cached) => {
                cached.hash != hash || usize::try_from(cached.n_tris).ok()? != mesh.triangles.len()
            }
            None => true,
        };
        if needs_rebuild {
            *guard = Some(Self::build_cached_cuda_mesh(stream, mesh, hash)?);
        }
        let c = guard.as_ref()?;
        let _ = (c.n_nodes, &c.d_tris_verts, &c.d_bvh);
        f(c)
    }

    fn dispatch_ray_mesh_kernel(
        &self,
        func: &CudaFunction,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<RayHit3>> {
        if rays.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some((0..rays.len()).map(|_| RayHit3::miss()).collect());
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_rays = u32::try_from(rays.len()).ok()?;
        let flat_rays: Vec<f64> = rays
            .iter()
            .flat_map(|r| {
                [
                    r.origin.x,
                    r.origin.y,
                    r.origin.z,
                    r.t_min,
                    r.direction.x,
                    r.direction.y,
                    r.direction.z,
                    r.t_max,
                ]
            })
            .collect();

        let d_rays = stream.clone_htod(&flat_rays).ok()?;
        let out_f64_len = rays.len().checked_mul(4)?;
        let mut d_out = stream.alloc_zeros::<f64>(out_f64_len).ok()?;

        self.with_cached_cuda_mesh(&stream, mesh, |cached| {
            let block = 128_u32;
            let grid = n_rays.div_ceil(block);
            let cfg = LaunchConfig {
                grid_dim: (grid, 1, 1),
                block_dim: (block, 1, 1),
                shared_mem_bytes: 0,
            };
            let mut builder = stream.launch_builder(func);
            builder.arg(&d_rays);
            builder.arg(&cached.d_tris_edges);
            builder.arg(&mut d_out);
            builder.arg(&n_rays);
            builder.arg(&cached.n_tris);
            // Safety: kernel signature matches (u64, u64, u64, u32, u32) and buffer bounds are verified.
            unsafe { builder.launch(cfg) }.ok()?;
            Some(())
        })?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(
            raw_out
                .chunks_exact(4)
                .map(|r| {
                    let Ok(tri_idx) = u32::try_from(r[1].to_bits()) else {
                        return RayHit3::miss();
                    };
                    if tri_idx == u32::MAX {
                        RayHit3::miss()
                    } else {
                        RayHit3 {
                            hit: true,
                            t: r[0],
                            triangle_index: tri_idx,
                            barycentric_u: r[2].clamp(0.0, 1.0),
                            barycentric_v: r[3].clamp(0.0, 1.0),
                        }
                    }
                })
                .collect(),
        )
    }

    /// Executes CUDA SIMT FP64 ray-mesh intersection.
    #[must_use]
    pub fn simt_ray_mesh_intersect(
        &self,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<RayHit3>> {
        self.dispatch_ray_mesh_kernel(&self.fn_simt_ray_mesh, rays, mesh)
    }

    /// Executes Ampere `sm_86` PTX non-coherent load FP64 ray-mesh intersection.
    #[must_use]
    pub fn ptx_sm86_ray_mesh_intersect(
        &self,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<RayHit3>> {
        if !self.supports_sm86() {
            return None;
        }
        self.dispatch_ray_mesh_kernel(&self.fn_sm86_ray_mesh, rays, mesh)
    }

    fn dispatch_point_in_solid_kernel(
        &self,
        func: &CudaFunction,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<bool>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some((0..points.len()).map(|_| false).collect());
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_pts = u32::try_from(points.len()).ok()?;
        let flat_pts: Vec<f64> = points.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();

        let d_pts = stream.clone_htod(&flat_pts).ok()?;
        let mut d_out = stream.alloc_zeros::<u32>(points.len()).ok()?;

        self.with_cached_cuda_mesh(&stream, mesh, |cached| {
            let block = 128_u32;
            let grid = n_pts.div_ceil(block);
            let cfg = LaunchConfig {
                grid_dim: (grid, 1, 1),
                block_dim: (block, 1, 1),
                shared_mem_bytes: 0,
            };
            let mut builder = stream.launch_builder(func);
            builder.arg(&d_pts);
            builder.arg(&cached.d_tris_edges);
            builder.arg(&mut d_out);
            builder.arg(&n_pts);
            builder.arg(&cached.n_tris);
            // Safety: kernel signature matches (u64, u64, u64, u32, u32) and buffer bounds are verified.
            unsafe { builder.launch(cfg) }.ok()?;
            Some(())
        })?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(raw_out.into_iter().map(|v| v != 0).collect())
    }

    /// Evaluates point-in-solid containment via standard CUDA SIMT ray-casting parity.
    #[must_use]
    pub fn simt_point_in_solid_parity(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<bool>> {
        self.dispatch_point_in_solid_kernel(&self.fn_simt_point_in_solid, points, mesh)
    }

    /// Evaluates point-in-solid containment via Ampere `sm_86` PTX non-coherent load ray-casting parity.
    #[must_use]
    pub fn ptx_sm86_point_in_solid_parity(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<bool>> {
        if !self.supports_sm86() {
            return None;
        }
        self.dispatch_point_in_solid_kernel(&self.fn_sm86_point_in_solid, points, mesh)
    }

    /// Whether the active GPU supports NVIDIA `OptiX` hardware RT Core acceleration.
    #[must_use]
    pub const fn supports_optix_rt(&self) -> bool {
        self.optix.is_some()
    }

    fn hash_mesh(mesh: TriangleMeshView<'_>) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        mesh.vertices.len().hash(&mut hasher);
        mesh.triangles.len().hash(&mut hasher);
        let v_step = (mesh.vertices.len() / 256).max(1);
        for v in mesh.vertices.iter().step_by(v_step) {
            v.x.to_bits().hash(&mut hasher);
            v.y.to_bits().hash(&mut hasher);
            v.z.to_bits().hash(&mut hasher);
        }
        if let Some(last_v) = mesh.vertices.last() {
            last_v.x.to_bits().hash(&mut hasher);
            last_v.y.to_bits().hash(&mut hasher);
            last_v.z.to_bits().hash(&mut hasher);
        }
        let t_step = (mesh.triangles.len() / 256).max(1);
        for tri in mesh.triangles.iter().step_by(t_step) {
            tri.hash(&mut hasher);
        }
        if let Some(last_t) = mesh.triangles.last() {
            last_t.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn build_cached_optix_mesh(
        optix: &OptixPipelines,
        stream: &Arc<CudaStream>,
        mesh: TriangleMeshView<'_>,
        hash: u64,
    ) -> Option<CachedOptixMesh> {
        let n_tris = u32::try_from(mesh.triangles.len()).ok()?;
        let flat_bvh = crate::spatial::MeshBvh3::build(mesh).flatten();
        let n_nodes = u32::try_from(flat_bvh.nodes().len()).ok()?;

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut max_z = f64::NEG_INFINITY;

        let mut verts_f32: Vec<[f32; 3]> = Vec::with_capacity(mesh.vertices.len());
        for v in mesh.vertices {
            if v.x.is_finite() && v.y.is_finite() && v.z.is_finite() {
                min_x = min_x.min(v.x);
                min_y = min_y.min(v.y);
                min_z = min_z.min(v.z);
                max_x = max_x.max(v.x);
                max_y = max_y.max(v.y);
                max_z = max_z.max(v.z);
            }
            verts_f32.push([
                f64_to_blas_f32(v.x),
                f64_to_blas_f32(v.y),
                f64_to_blas_f32(v.z),
            ]);
        }
        let diag = if min_x <= max_x {
            let dx = max_x - min_x;
            let dy = max_y - min_y;
            let dz = max_z - min_z;
            (dx * dx + dy * dy + dz * dz).sqrt().max(1.0e-3)
        } else {
            1.0
        };
        let sqrt_tris = f64::from(n_tris.max(1)).sqrt();
        let r_pad = (diag * 0.42 / sqrt_tris).max(1.0e-3);
        let r_pad_sq = r_pad * r_pad;
        let r_pad_aabb = r_pad * 1.002 + 1.0e-5;

        let mut indices_u32: Vec<[u32; 3]> = Vec::with_capacity(mesh.triangles.len());
        let mut flat_tris_edges: Vec<f64> =
            Vec::with_capacity(mesh.triangles.len().checked_mul(9)?);
        let mut flat_tris_verts: Vec<f64> =
            Vec::with_capacity(mesh.triangles.len().checked_mul(9)?);
        let mut tri_aabbs: Vec<optix_sys::OptixAabb> = Vec::with_capacity(mesh.triangles.len());

        for tri in mesh.triangles {
            let i0 = usize::try_from(tri[0]).ok()?;
            let i1 = usize::try_from(tri[1]).ok()?;
            let i2 = usize::try_from(tri[2]).ok()?;
            let v0 = *mesh.vertices.get(i0)?;
            let v1 = *mesh.vertices.get(i1)?;
            let v2 = *mesh.vertices.get(i2)?;
            let valid = v0.x.is_finite()
                && v0.y.is_finite()
                && v0.z.is_finite()
                && v1.x.is_finite()
                && v1.y.is_finite()
                && v1.z.is_finite()
                && v2.x.is_finite()
                && v2.y.is_finite()
                && v2.z.is_finite();
            indices_u32.push(if valid { *tri } else { [0, 0, 0] });
            flat_tris_edges.extend_from_slice(&[
                v0.x,
                v0.y,
                v0.z,
                v1.x - v0.x,
                v1.y - v0.y,
                v1.z - v0.z,
                v2.x - v0.x,
                v2.y - v0.y,
                v2.z - v0.z,
            ]);
            flat_tris_verts
                .extend_from_slice(&[v0.x, v0.y, v0.z, v1.x, v1.y, v1.z, v2.x, v2.y, v2.z]);

            if valid {
                let tmin_x = v0.x.min(v1.x).min(v2.x) - r_pad_aabb;
                let tmin_y = v0.y.min(v1.y).min(v2.y) - r_pad_aabb;
                let tmin_z = v0.z.min(v1.z).min(v2.z) - r_pad_aabb;
                let tmax_x = v0.x.max(v1.x).max(v2.x) + r_pad_aabb;
                let tmax_y = v0.y.max(v1.y).max(v2.y) + r_pad_aabb;
                let tmax_z = v0.z.max(v1.z).max(v2.z) + r_pad_aabb;
                tri_aabbs.push(optix_sys::OptixAabb {
                    minX: f64_to_blas_f32(tmin_x),
                    minY: f64_to_blas_f32(tmin_y),
                    minZ: f64_to_blas_f32(tmin_z),
                    maxX: f64_to_blas_f32(tmax_x),
                    maxY: f64_to_blas_f32(tmax_y),
                    maxZ: f64_to_blas_f32(tmax_z),
                });
            } else {
                tri_aabbs.push(optix_sys::OptixAabb::default());
            }
        }

        let flat_bvh_buf: Vec<f64> = flat_bvh
            .nodes()
            .iter()
            .flat_map(|n| {
                let meta = (u64::from(n.right_child) << 32) | u64::from(n.triangle_index);
                [
                    n.aabb_min.x,
                    n.aabb_min.y,
                    n.aabb_min.z,
                    n.aabb_max.x,
                    n.aabb_max.y,
                    n.aabb_max.z,
                    f64::from_bits(meta),
                    0.0,
                ]
            })
            .collect();

        let d_verts_f32 = stream.clone_htod(&verts_f32).ok()?;
        let d_indices_u32 = stream.clone_htod(&indices_u32).ok()?;
        let d_tri_aabbs = stream.clone_htod(&tri_aabbs).ok()?;

        let blas_closest = OptixAccel::build_triangles(
            &optix.ctx,
            stream,
            &d_verts_f32,
            Some(&d_indices_u32),
            OPTIX_GEOMETRY_FLAG_DISABLE_ANYHIT_U32,
            None,
        )
        .ok()?;
        let blas_anyhit = OptixAccel::build_triangles(
            &optix.ctx,
            stream,
            &d_verts_f32,
            Some(&d_indices_u32),
            0,
            None,
        )
        .ok()?;
        let blas_aabb = OptixAccel::build_aabbs(
            &optix.ctx,
            stream,
            &d_tri_aabbs,
            OPTIX_GEOMETRY_FLAG_DISABLE_ANYHIT_U32,
            None,
        )
        .ok()?;

        let d_tris_edges = stream.clone_htod(&flat_tris_edges).ok()?;
        let d_tris_verts = stream.clone_htod(&flat_tris_verts).ok()?;
        let d_bvh = stream.clone_htod(&flat_bvh_buf).ok()?;

        Some(CachedOptixMesh {
            hash,
            n_tris,
            n_nodes,
            r_pad_sq,
            blas_closest,
            blas_anyhit,
            blas_aabb,
            d_tris_edges,
            d_tris_verts,
            d_bvh,
        })
    }

    fn with_cached_optix_mesh<R>(
        optix: &OptixPipelines,
        stream: &Arc<CudaStream>,
        mesh: TriangleMeshView<'_>,
        f: impl FnOnce(&CachedOptixMesh) -> Option<R>,
    ) -> Option<R> {
        let hash = Self::hash_mesh(mesh);
        let mut guard = optix.mesh_cache.lock().ok()?;
        let needs_rebuild = match guard.as_ref() {
            Some(cached) => {
                cached.hash != hash || usize::try_from(cached.n_tris).ok()? != mesh.triangles.len()
            }
            None => true,
        };
        if needs_rebuild {
            *guard = Some(Self::build_cached_optix_mesh(optix, stream, mesh, hash)?);
        }
        let c = guard.as_ref()?;
        let _ = (
            c.n_nodes,
            c.r_pad_sq,
            &c.blas_anyhit,
            &c.blas_aabb,
            &c.d_tris_verts,
            &c.d_bvh,
        );
        f(c)
    }

    /// Executes hardware RT Core ray-mesh intersection via `cudarc::optix` (`OptixAccel` + `OptixPipeline::launch`) and exact FP64 Möller-Trumbore refinement.
    #[must_use]
    pub fn optix_rt_ray_mesh_intersect(
        &self,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<RayHit3>> {
        let optix = self.optix.as_ref()?;
        if rays.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some((0..rays.len()).map(|_| RayHit3::miss()).collect());
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_rays = u32::try_from(rays.len()).ok()?;

        let flat_rays: Vec<f64> = rays
            .iter()
            .flat_map(|r| {
                [
                    r.origin.x,
                    r.origin.y,
                    r.origin.z,
                    r.t_min,
                    r.direction.x,
                    r.direction.y,
                    r.direction.z,
                    r.t_max,
                ]
            })
            .collect();
        let d_rays = stream.clone_htod(&flat_rays).ok()?;
        let d_cand_prim = stream.alloc_zeros::<u32>(rays.len()).ok()?;
        let out_f64_len = rays.len().checked_mul(4)?;
        let mut d_out = stream.alloc_zeros::<f64>(out_f64_len).ok()?;

        Self::with_cached_optix_mesh(optix, &stream, mesh, |cached| {
            let params_raw = {
                let (rays_ptr, _s1) = d_rays.device_ptr(&stream);
                let (cand_ptr, _s2) = d_cand_prim.device_ptr(&stream);
                [cached.blas_closest.handle(), rays_ptr, cand_ptr, 0_u64]
            };
            let d_params = stream.clone_htod(&params_raw).ok()?;
            optix
                .ray_pipeline
                .launch(&stream, &d_params, &optix.ray_sbt, n_rays, 1, 1)
                .ok()?;

            let block = 128_u32;
            let grid = n_rays.div_ceil(block);
            let cfg = LaunchConfig {
                grid_dim: (grid, 1, 1),
                block_dim: (block, 1, 1),
                shared_mem_bytes: 0,
            };
            let mut builder = stream.launch_builder(&optix.fn_optix_ray_mesh);
            builder.arg(&d_rays);
            builder.arg(&cached.d_tris_edges);
            builder.arg(&d_cand_prim);
            builder.arg(&mut d_out);
            builder.arg(&n_rays);
            builder.arg(&cached.n_tris);
            // Safety: kernel signature matches (u64, u64, u64, u64, u32, u32) and buffer bounds are verified.
            unsafe { builder.launch(cfg) }.ok()?;
            Some(())
        })?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(
            raw_out
                .chunks_exact(4)
                .map(|r| {
                    let Ok(tri_idx) = u32::try_from(r[1].to_bits()) else {
                        return RayHit3::miss();
                    };
                    if tri_idx == u32::MAX {
                        RayHit3::miss()
                    } else {
                        RayHit3 {
                            hit: true,
                            t: r[0],
                            triangle_index: tri_idx,
                            barycentric_u: r[2].clamp(0.0, 1.0),
                            barycentric_v: r[3].clamp(0.0, 1.0),
                        }
                    }
                })
                .collect(),
        )
    }

    /// Evaluates point-in-solid containment via `cudarc::optix` hardware RT Core anyhit triangle gathering and exact FP64 parity verification.
    #[must_use]
    pub fn optix_rt_point_in_solid_parity(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<bool>> {
        let optix = self.optix.as_ref()?;
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some((0..points.len()).map(|_| false).collect());
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_pts = u32::try_from(points.len()).ok()?;

        let flat_pts: Vec<f64> = points.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();
        let d_pts = stream.clone_htod(&flat_pts).ok()?;
        let d_cand_prim8 = stream
            .alloc_zeros::<u32>(points.len().checked_mul(8)?)
            .ok()?;
        let mut d_out = stream.alloc_zeros::<u32>(points.len()).ok()?;

        Self::with_cached_optix_mesh(optix, &stream, mesh, |cached| {
            let params_raw = {
                let (pts_ptr, _s1) = d_pts.device_ptr(&stream);
                let (cand_ptr, _s2) = d_cand_prim8.device_ptr(&stream);
                [cached.blas_anyhit.handle(), pts_ptr, cand_ptr, 0_u64]
            };
            let d_params = stream.clone_htod(&params_raw).ok()?;
            optix
                .parity_pipeline
                .launch(&stream, &d_params, &optix.parity_sbt, n_pts, 1, 1)
                .ok()?;

            let block = 128_u32;
            let grid = n_pts.div_ceil(block);
            let cfg = LaunchConfig {
                grid_dim: (grid, 1, 1),
                block_dim: (block, 1, 1),
                shared_mem_bytes: 0,
            };
            let mut builder = stream.launch_builder(&optix.fn_optix_point_in_solid);
            builder.arg(&d_pts);
            builder.arg(&cached.d_tris_edges);
            builder.arg(&d_cand_prim8);
            builder.arg(&mut d_out);
            builder.arg(&n_pts);
            builder.arg(&cached.n_tris);
            // Safety: kernel signature matches (u64, u64, u64, u64, u32, u32) and buffer bounds are verified.
            unsafe { builder.launch(cfg) }.ok()?;
            Some(())
        })?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(raw_out.into_iter().map(|v| v != 0).collect())
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
    fn cuda_simt_ray_mesh_intersect_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0_u32, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let rays: Vec<RayQuery3> = (0_u32..25)
            .map(|i| {
                let f = f64::from(i) * 0.1;
                RayQuery3 {
                    origin: Point3::new(0.1 + f, 0.2, -1.0),
                    direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    t_min: 0.0,
                    t_max: 10.0,
                }
            })
            .collect();
        let expected = cpu::ray_mesh_intersect(&rays, mesh);
        let actual = ctx
            .simt_ray_mesh_intersect(&rays, mesh)
            .expect("SIMT ray-mesh intersect");
        assert_eq!(expected.len(), actual.len());
        for (e, a) in expected.iter().zip(&actual) {
            assert_eq!(e.hit, a.hit);
            assert_eq!(e.triangle_index, a.triangle_index);
            if e.hit {
                assert!((e.t - a.t).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.barycentric_u - a.barycentric_u).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.barycentric_v - a.barycentric_v).abs() < CUDA_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn cuda_sm86_ray_mesh_intersect_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0_u32, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let rays: Vec<RayQuery3> = (0_u32..25)
            .map(|i| {
                let f = f64::from(i) * 0.1;
                RayQuery3 {
                    origin: Point3::new(0.1 + f, 0.2, -1.0),
                    direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    t_min: 0.0,
                    t_max: 10.0,
                }
            })
            .collect();
        let expected = cpu::ray_mesh_intersect(&rays, mesh);
        if ctx.supports_sm86() {
            let actual = ctx
                .ptx_sm86_ray_mesh_intersect(&rays, mesh)
                .expect("PTX sm_86 ray-mesh intersect");
            assert_eq!(expected.len(), actual.len());
            for (e, a) in expected.iter().zip(&actual) {
                assert_eq!(e.hit, a.hit);
                assert_eq!(e.triangle_index, a.triangle_index);
                if e.hit {
                    assert!((e.t - a.t).abs() < CUDA_PARITY_TOLERANCE);
                    assert!((e.barycentric_u - a.barycentric_u).abs() < CUDA_PARITY_TOLERANCE);
                    assert!((e.barycentric_v - a.barycentric_v).abs() < CUDA_PARITY_TOLERANCE);
                }
            }
        } else {
            assert!(ctx.ptx_sm86_ray_mesh_intersect(&rays, mesh).is_none());
        }
    }

    #[test]
    fn cuda_simt_point_in_solid_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0_u32, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let points = [
            Point3::new(0.25, 0.25, 0.25),
            Point3::new(1.5, 1.5, 1.5),
            Point3::new(-0.5, 0.2, 0.2),
            Point3::new(0.1, 0.1, 0.1),
        ];
        let expected = cpu::point_in_solid_parity(&points, mesh);
        let actual = ctx
            .simt_point_in_solid_parity(&points, mesh)
            .expect("SIMT point-in-solid parity");
        assert_eq!(actual, expected);
    }

    #[test]
    fn cuda_sm86_point_in_solid_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0_u32, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let points = [
            Point3::new(0.25, 0.25, 0.25),
            Point3::new(1.5, 1.5, 1.5),
            Point3::new(-0.5, 0.2, 0.2),
            Point3::new(0.1, 0.1, 0.1),
        ];
        let expected = cpu::point_in_solid_parity(&points, mesh);
        if ctx.supports_sm86() {
            let actual = ctx
                .ptx_sm86_point_in_solid_parity(&points, mesh)
                .expect("PTX sm_86 point-in-solid parity");
            assert_eq!(actual, expected);
        } else {
            assert!(ctx.ptx_sm86_point_in_solid_parity(&points, mesh).is_none());
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

    #[test]
    fn cuda_optix_rt_ray_mesh_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        if !ctx.supports_optix_rt() {
            return;
        }
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let rays: Vec<RayQuery3> = (0_u32..48)
            .map(|i| {
                let f = f64::from(i) * 0.05;
                RayQuery3 {
                    origin: Point3::new(0.1 + f, 0.25, -1.5),
                    direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    t_min: 0.0,
                    t_max: 50.0,
                }
            })
            .collect();
        let expected = cpu::ray_mesh_intersect(&rays, mesh);
        let actual = ctx
            .optix_rt_ray_mesh_intersect(&rays, mesh)
            .expect("CUDA OptiX RT Core ray-mesh intersection");
        for (e, a) in expected.iter().zip(&actual) {
            assert_eq!(e.hit, a.hit);
            assert_eq!(e.triangle_index, a.triangle_index);
            if e.hit {
                assert!((e.t - a.t).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.barycentric_u - a.barycentric_u).abs() < CUDA_PARITY_TOLERANCE);
                assert!((e.barycentric_v - a.barycentric_v).abs() < CUDA_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn cuda_optix_rt_point_in_solid_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        if !ctx.supports_optix_rt() {
            return;
        }
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let points = [
            Point3::new(0.25, 0.25, 0.25),
            Point3::new(0.4, 0.4, 0.4),
            Point3::new(1.5, 1.5, 1.5),
            Point3::new(-0.5, 0.2, 0.2),
            Point3::new(0.1, 0.2, 0.3),
        ];
        let expected = cpu::point_in_solid_parity(&points, mesh);
        let actual = ctx
            .optix_rt_point_in_solid_parity(&points, mesh)
            .expect("CUDA OptiX RT Core point-in-solid parity");
        assert_eq!(actual, expected);
    }
}
