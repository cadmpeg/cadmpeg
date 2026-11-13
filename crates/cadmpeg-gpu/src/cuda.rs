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
use crate::{ClosestPointResult, RayHit3, RayQuery3, TriangleMeshView};

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

// 7. CUDA SIMT Stack-Based Flattened KD-Tree Nearest-Neighbor Query (FP64)
.visible .entry cuda_simt_kdtree_query_f64(
    .param .u64 param_nodes,
    .param .u64 param_queries,
    .param .u64 param_out,
    .param .u32 param_n_queries,
    .param .u32 param_n_nodes
) {
    .local .align 16 .b8 %stack[512];
    .reg .pred %p<12>;
    .reg .b32 %r<24>;
    .reg .b64 %rd<18>;
    .reg .f64 %fd<16>;

    ld.param.u64 %rd1, [param_nodes];
    ld.param.u64 %rd2, [param_queries];
    ld.param.u64 %rd3, [param_out];
    ld.param.u32 %r1, [param_n_queries];
    ld.param.u32 %r2, [param_n_nodes];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_KD_EXIT;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mul.wide.u32 %rd7, %r6, 32;
    add.u64 %rd8, %rd5, %rd7;
    ld.global.f64 %fd0, [%rd8 + 0];  // qx
    ld.global.f64 %fd1, [%rd8 + 8];  // qy
    ld.global.f64 %fd2, [%rd8 + 16]; // qz

    mov.f64 %fd3, 0d7FF0000000000000; // best_sq = +inf
    setp.eq.u32 %p1, %r2, 0;
    @%p1 bra L_KD_STORE;

    sub.u32 %r8, %r2, 1;
    min.u32 %r8, %r6, %r8;
    mul.wide.u32 %rd12, %r8, 32;
    add.u64 %rd13, %rd4, %rd12;
    ld.global.f64 %fd5, [%rd13 + 0];
    ld.global.f64 %fd6, [%rd13 + 8];
    ld.global.f64 %fd7, [%rd13 + 16];
    sub.f64 %fd8,  %fd0, %fd5;
    sub.f64 %fd9,  %fd1, %fd6;
    sub.f64 %fd10, %fd2, %fd7;
    mul.f64 %fd3, %fd10, %fd10;
    fma.rn.f64 %fd3, %fd9, %fd9, %fd3;
    fma.rn.f64 %fd3, %fd8, %fd8, %fd3;

    mov.u64 %rd9, %stack;
    st.local.u32 [%rd9 + 0], 0;
    st.local.u32 [%rd9 + 4], %r2;
    st.local.f64 [%rd9 + 8], 0d0000000000000000;
    mov.u32 %r7, 1; // sp = 1

L_KD_LOOP:
    setp.eq.u32 %p2, %r7, 0;
    @%p2 bra L_KD_STORE;

    sub.u32 %r7, %r7, 1;
    shl.b32 %r8, %r7, 4;
    cvt.u64.u32 %rd10, %r8;
    add.u64 %rd11, %rd9, %rd10;
    ld.local.u32 %r9,  [%rd11 + 0]; // node_idx
    ld.local.u32 %r10, [%rd11 + 4]; // subtree_end
    ld.local.f64 %fd4, [%rd11 + 8]; // lower_bound_sq

    setp.ge.f64 %p3, %fd4, %fd3;
    @%p3 bra L_KD_LOOP;

    mul.wide.u32 %rd12, %r9, 32;
    add.u64 %rd13, %rd4, %rd12;
    ld.global.f64 %fd5, [%rd13 + 0];  // nx
    ld.global.f64 %fd6, [%rd13 + 8];  // ny
    ld.global.f64 %fd7, [%rd13 + 16]; // nz
    ld.global.u32 %r11, [%rd13 + 24]; // split_axis
    ld.global.u32 %r12, [%rd13 + 28]; // right_child

    sub.f64 %fd8,  %fd0, %fd5;
    sub.f64 %fd9,  %fd1, %fd6;
    sub.f64 %fd10, %fd2, %fd7;

    mul.f64 %fd11, %fd10, %fd10;
    fma.rn.f64 %fd11, %fd9, %fd9, %fd11;
    fma.rn.f64 %fd11, %fd8, %fd8, %fd11;
    min.f64 %fd3, %fd3, %fd11;

    setp.eq.u32 %p4, %r11, 0;
    setp.eq.u32 %p5, %r11, 1;
    selp.f64 %fd12, %fd9,  %fd10, %p5;
    selp.f64 %fd12, %fd8,  %fd12, %p4;
    mul.f64 %fd13, %fd12, %fd12; // axis_diff_sq

    min.u32 %r13, %r12, %r10; // left_end = min(right_child, subtree_end)
    add.u32 %r14, %r9, 1;     // left_idx = node_idx + 1

    setp.le.f64 %p6, %fd12, 0d0000000000000000;
    selp.u32 %r15, %r14, %r12, %p6; // near_idx
    selp.u32 %r16, %r13, %r10, %p6; // near_end
    selp.u32 %r17, %r12, %r14, %p6; // far_idx
    selp.u32 %r18, %r10, %r13, %p6; // far_end

    setp.lt.u32 %p7, %r17, %r18;
    setp.lt.f64 %p8, %fd13, %fd3;
    and.pred %p7, %p7, %p8;
    setp.lt.u32 %p9, %r7, 32;
    and.pred %p7, %p7, %p9;
    @!%p7 bra L_KD_PUSH_NEAR;

    shl.b32 %r19, %r7, 4;
    cvt.u64.u32 %rd14, %r19;
    add.u64 %rd15, %rd9, %rd14;
    st.local.u32 [%rd15 + 0], %r17;
    st.local.u32 [%rd15 + 4], %r18;
    st.local.f64 [%rd15 + 8], %fd13;
    add.u32 %r7, %r7, 1;

L_KD_PUSH_NEAR:
    setp.lt.u32 %p10, %r15, %r16;
    setp.lt.u32 %p11, %r7, 32;
    and.pred %p10, %p10, %p11;
    @!%p10 bra L_KD_LOOP;

    shl.b32 %r20, %r7, 4;
    cvt.u64.u32 %rd16, %r20;
    add.u64 %rd17, %rd9, %rd16;
    st.local.u32 [%rd17 + 0], %r15;
    st.local.u32 [%rd17 + 4], %r16;
    st.local.f64 [%rd17 + 8], 0d0000000000000000;
    add.u32 %r7, %r7, 1;
    bra L_KD_LOOP;

L_KD_STORE:
    mul.wide.u32 %rd12, %r6, 8;
    add.u64 %rd13, %rd6, %rd12;
    st.global.f64 [%rd13], %fd3;

L_KD_EXIT:
    ret;
}

// 8. CUDA SIMT Stack-Based Flattened BVH Closest-Point-on-Mesh Query (FP64)
.visible .entry cuda_simt_bvh_closest_point_f64(
    .param .u64 param_bvh,
    .param .u64 param_tris,
    .param .u64 param_queries,
    .param .u64 param_out,
    .param .u32 param_n_queries,
    .param .u32 param_n_nodes
) {
    .local .align 16 .b8 %stack[1024];
    .reg .pred %p<16>;
    .reg .b32 %r<22>;
    .reg .b64 %rd<20>;
    .reg .f64 %fd<52>;

    ld.param.u64 %rd1, [param_bvh];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd3, [param_queries];
    ld.param.u64 %rd4, [param_out];
    ld.param.u32 %r1, [param_n_queries];
    ld.param.u32 %r2, [param_n_nodes];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_BVH_EXIT;

    cvta.to.global.u64 %rd5, %rd1; // bvh
    cvta.to.global.u64 %rd6, %rd2; // tris
    cvta.to.global.u64 %rd7, %rd3; // queries
    cvta.to.global.u64 %rd8, %rd4; // out

    mul.wide.u32 %rd9, %r6, 32;
    add.u64 %rd10, %rd7, %rd9;
    ld.global.f64 %fd0, [%rd10 + 0];  // qx
    ld.global.f64 %fd1, [%rd10 + 8];  // qy
    ld.global.f64 %fd2, [%rd10 + 16]; // qz

    mov.f64 %fd3, 0d7FF0000000000000; // best_sq
    mov.f64 %fd4, 0d0000000000000000; // best_x
    mov.f64 %fd5, 0d0000000000000000; // best_y
    mov.f64 %fd6, 0d0000000000000000; // best_z
    mov.u32 %r7, 0xffffffff;          // best_tri

    setp.eq.u32 %p1, %r2, 0;
    @%p1 bra L_BVH_STORE;

    mov.u64 %rd11, %stack;
    st.local.u32 [%rd11 + 0], 0;
    st.local.u32 [%rd11 + 4], 0;
    st.local.f64 [%rd11 + 8], 0d0000000000000000;
    mov.u32 %r8, 1; // sp = 1

L_BVH_LOOP:
    setp.eq.u32 %p2, %r8, 0;
    @%p2 bra L_BVH_STORE;

    sub.u32 %r8, %r8, 1;
    shl.b32 %r9, %r8, 4;
    cvt.u64.u32 %rd12, %r9;
    add.u64 %rd13, %rd11, %rd12;
    ld.local.u32 %r10, [%rd13 + 0]; // node_idx
    ld.local.f64 %fd7, [%rd13 + 8]; // lower_bound_sq

    setp.ge.f64 %p3, %fd7, %fd3;
    setp.ne.u32 %p4, %r7, 0xffffffff;
    and.pred %p3, %p3, %p4;
    @%p3 bra L_BVH_LOOP;

    mul.wide.u32 %rd14, %r10, 64;
    add.u64 %rd15, %rd5, %rd14;
    ld.global.u32 %r11, [%rd15 + 48]; // triangle_index
    ld.global.u32 %r12, [%rd15 + 52]; // right_child

    setp.eq.u32 %p5, %r11, 0xffffffff;
    @!%p5 bra L_BVH_LEAF;

    // Internal node: evaluate left (node_idx + 1) and right (right_child) AABB distances
    add.u32 %r13, %r10, 1;
    mov.f64 %fd8, 0d7FF0000000000000;
    setp.lt.u32 %p6, %r13, %r2;
    @!%p6 bra L_BVH_SKIP_LEFT;

    mul.wide.u32 %rd16, %r13, 64;
    add.u64 %rd17, %rd5, %rd16;
    ld.global.f64 %fd10, [%rd17 + 0];
    ld.global.f64 %fd11, [%rd17 + 8];
    ld.global.f64 %fd12, [%rd17 + 16];
    ld.global.f64 %fd13, [%rd17 + 24];
    ld.global.f64 %fd14, [%rd17 + 32];
    ld.global.f64 %fd15, [%rd17 + 40];
    sub.f64 %fd16, %fd10, %fd0;
    sub.f64 %fd17, %fd0, %fd13;
    max.f64 %fd16, %fd16, %fd17;
    max.f64 %fd16, %fd16, 0d0000000000000000;
    sub.f64 %fd18, %fd11, %fd1;
    sub.f64 %fd19, %fd1, %fd14;
    max.f64 %fd18, %fd18, %fd19;
    max.f64 %fd18, %fd18, 0d0000000000000000;
    sub.f64 %fd20, %fd12, %fd2;
    sub.f64 %fd21, %fd2, %fd15;
    max.f64 %fd20, %fd20, %fd21;
    max.f64 %fd20, %fd20, 0d0000000000000000;
    mul.f64 %fd8, %fd20, %fd20;
    fma.rn.f64 %fd8, %fd18, %fd18, %fd8;
    fma.rn.f64 %fd8, %fd16, %fd16, %fd8;

L_BVH_SKIP_LEFT:
    mov.f64 %fd9, 0d7FF0000000000000;
    setp.lt.u32 %p7, %r12, %r2;
    @!%p7 bra L_BVH_SKIP_RIGHT;

    mul.wide.u32 %rd16, %r12, 64;
    add.u64 %rd17, %rd5, %rd16;
    ld.global.f64 %fd10, [%rd17 + 0];
    ld.global.f64 %fd11, [%rd17 + 8];
    ld.global.f64 %fd12, [%rd17 + 16];
    ld.global.f64 %fd13, [%rd17 + 24];
    ld.global.f64 %fd14, [%rd17 + 32];
    ld.global.f64 %fd15, [%rd17 + 40];
    sub.f64 %fd16, %fd10, %fd0;
    sub.f64 %fd17, %fd0, %fd13;
    max.f64 %fd16, %fd16, %fd17;
    max.f64 %fd16, %fd16, 0d0000000000000000;
    sub.f64 %fd18, %fd11, %fd1;
    sub.f64 %fd19, %fd1, %fd14;
    max.f64 %fd18, %fd18, %fd19;
    max.f64 %fd18, %fd18, 0d0000000000000000;
    sub.f64 %fd20, %fd12, %fd2;
    sub.f64 %fd21, %fd2, %fd15;
    max.f64 %fd20, %fd20, %fd21;
    max.f64 %fd20, %fd20, 0d0000000000000000;
    mul.f64 %fd9, %fd20, %fd20;
    fma.rn.f64 %fd9, %fd18, %fd18, %fd9;
    fma.rn.f64 %fd9, %fd16, %fd16, %fd9;

L_BVH_SKIP_RIGHT:
    setp.lt.f64 %p8, %fd8, %fd9;
    selp.u32 %r14, %r13, %r12, %p8; // near_idx
    selp.f64 %fd22, %fd8, %fd9, %p8; // d_near
    selp.u32 %r15, %r12, %r13, %p8; // far_idx
    selp.f64 %fd23, %fd9, %fd8, %p8; // d_far

    setp.lt.u32 %p9, %r15, %r2;
    setp.lt.f64 %p10, %fd23, %fd3;
    and.pred %p9, %p9, %p10;
    setp.lt.u32 %p12, %r8, 64;
    and.pred %p9, %p9, %p12;
    @!%p9 bra L_BVH_PUSH_NEAR;

    shl.b32 %r16, %r8, 4;
    cvt.u64.u32 %rd18, %r16;
    add.u64 %rd19, %rd11, %rd18;
    st.local.u32 [%rd19 + 0], %r15;
    st.local.u32 [%rd19 + 4], 0;
    st.local.f64 [%rd19 + 8], %fd23;
    add.u32 %r8, %r8, 1;

L_BVH_PUSH_NEAR:
    setp.lt.u32 %p9, %r14, %r2;
    setp.lt.f64 %p10, %fd22, %fd3;
    and.pred %p9, %p9, %p10;
    setp.lt.u32 %p12, %r8, 64;
    and.pred %p9, %p9, %p12;
    @!%p9 bra L_BVH_LOOP;

    shl.b32 %r16, %r8, 4;
    cvt.u64.u32 %rd18, %r16;
    add.u64 %rd19, %rd11, %rd18;
    st.local.u32 [%rd19 + 0], %r14;
    st.local.u32 [%rd19 + 4], 0;
    st.local.f64 [%rd19 + 8], %fd22;
    add.u32 %r8, %r8, 1;
    bra L_BVH_LOOP;

L_BVH_LEAF:
    // Load triangle vertices v0, v1, v2 (9 f64s = 72 bytes)
    mul.wide.u32 %rd16, %r11, 72;
    add.u64 %rd17, %rd6, %rd16;
    ld.global.f64 %fd10, [%rd17 + 0];  // v0.x
    ld.global.f64 %fd11, [%rd17 + 8];  // v0.y
    ld.global.f64 %fd12, [%rd17 + 16]; // v0.z
    ld.global.f64 %fd13, [%rd17 + 24]; // v1.x
    ld.global.f64 %fd14, [%rd17 + 32]; // v1.y
    ld.global.f64 %fd15, [%rd17 + 40]; // v1.z
    ld.global.f64 %fd16, [%rd17 + 48]; // v2.x
    ld.global.f64 %fd17, [%rd17 + 56]; // v2.y
    ld.global.f64 %fd18, [%rd17 + 64]; // v2.z

    // ab = v1 - v0, ac = v2 - v0, ap = q - v0
    sub.f64 %fd19, %fd13, %fd10; // ab.x
    sub.f64 %fd20, %fd14, %fd11; // ab.y
    sub.f64 %fd21, %fd15, %fd12; // ab.z
    sub.f64 %fd22, %fd16, %fd10; // ac.x
    sub.f64 %fd23, %fd17, %fd11; // ac.y
    sub.f64 %fd24, %fd18, %fd12; // ac.z
    sub.f64 %fd25, %fd0,  %fd10; // ap.x
    sub.f64 %fd26, %fd1,  %fd11; // ap.y
    sub.f64 %fd27, %fd2,  %fd12; // ap.z

    // d1 = dot(ab, ap), d2 = dot(ac, ap)
    mul.f64 %fd28, %fd21, %fd27;
    fma.rn.f64 %fd28, %fd20, %fd26, %fd28;
    fma.rn.f64 %fd28, %fd19, %fd25, %fd28; // d1
    mul.f64 %fd29, %fd24, %fd27;
    fma.rn.f64 %fd29, %fd23, %fd26, %fd29;
    fma.rn.f64 %fd29, %fd22, %fd25, %fd29; // d2

    // Degenerate triangle check: n = ab x ac, |n|^2 <= f64::EPSILON * |ab|^2 * |ac|^2
    mul.rn.f64 %fd32, %fd21, %fd23;
    neg.f64 %fd35, %fd21;
    fma.rn.f64 %fd35, %fd35, %fd23, %fd32;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd20, %fd24, %fd32;
    add.f64 %fd32, %fd32, %fd35; // nx
    mul.rn.f64 %fd33, %fd19, %fd24;
    neg.f64 %fd35, %fd19;
    fma.rn.f64 %fd35, %fd35, %fd24, %fd33;
    neg.f64 %fd33, %fd33;
    fma.rn.f64 %fd33, %fd21, %fd22, %fd33;
    add.f64 %fd33, %fd33, %fd35; // ny
    mul.rn.f64 %fd34, %fd20, %fd22;
    neg.f64 %fd35, %fd20;
    fma.rn.f64 %fd35, %fd35, %fd22, %fd34;
    neg.f64 %fd34, %fd34;
    fma.rn.f64 %fd34, %fd19, %fd23, %fd34;
    add.f64 %fd34, %fd34, %fd35; // nz
    mul.f64 %fd35, %fd34, %fd34;
    fma.rn.f64 %fd35, %fd33, %fd33, %fd35;
    fma.rn.f64 %fd35, %fd32, %fd32, %fd35; // n_sq
    mul.f64 %fd36, %fd21, %fd21;
    fma.rn.f64 %fd36, %fd20, %fd20, %fd36;
    fma.rn.f64 %fd36, %fd19, %fd19, %fd36; // ab_sq
    mul.f64 %fd37, %fd24, %fd24;
    fma.rn.f64 %fd37, %fd23, %fd23, %fd37;
    fma.rn.f64 %fd37, %fd22, %fd22, %fd37; // ac_sq
    mul.f64 %fd38, %fd36, %fd37;
    mul.f64 %fd38, %fd38, 0d3CB0000000000000; // DEGENERATE_AREA_FACTOR (f64::EPSILON)
    setp.le.f64 %p6, %fd35, %fd38;
    @%p6 bra L_TRI_DEGENERATE;

    setp.le.f64 %p6, %fd28, 0d0000000000000000;
    setp.le.f64 %p7, %fd29, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_TRI_CHECK_B;
    mov.f64 %fd40, %fd10;
    mov.f64 %fd41, %fd11;
    mov.f64 %fd42, %fd12;
    bra L_TRI_EVAL_DIST;

L_TRI_CHECK_B:
    // bp = q - v1
    sub.f64 %fd25, %fd0, %fd13;
    sub.f64 %fd26, %fd1, %fd14;
    sub.f64 %fd27, %fd2, %fd15;
    // d3 = dot(ab, bp), d4 = dot(ac, bp)
    mul.f64 %fd30, %fd21, %fd27;
    fma.rn.f64 %fd30, %fd20, %fd26, %fd30;
    fma.rn.f64 %fd30, %fd19, %fd25, %fd30; // d3
    mul.f64 %fd31, %fd24, %fd27;
    fma.rn.f64 %fd31, %fd23, %fd26, %fd31;
    fma.rn.f64 %fd31, %fd22, %fd25, %fd31; // d4

    setp.ge.f64 %p6, %fd30, 0d0000000000000000;
    setp.le.f64 %p7, %fd31, %fd30;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_TRI_CHECK_AB;
    mov.f64 %fd40, %fd13;
    mov.f64 %fd41, %fd14;
    mov.f64 %fd42, %fd15;
    bra L_TRI_EVAL_DIST;

L_TRI_CHECK_AB:
    // vc = d1 * d4 - d3 * d2
    mul.f64 %fd32, %fd30, %fd29;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd28, %fd31, %fd32; // vc

    setp.le.f64 %p6, %fd32, 0d0000000000000000;
    setp.ge.f64 %p7, %fd28, 0d0000000000000000;
    setp.le.f64 %p8, %fd30, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_TRI_CHECK_C;
    sub.f64 %fd33, %fd28, %fd30;
    setp.gt.f64 %p12, %fd33, 0d0000000000000000;
    @!%p12 bra L_TRI_CHECK_C;
    div.rn.f64 %fd34, %fd28, %fd33; // bary_v
    fma.rn.f64 %fd40, %fd34, %fd19, %fd10;
    fma.rn.f64 %fd41, %fd34, %fd20, %fd11;
    fma.rn.f64 %fd42, %fd34, %fd21, %fd12;
    bra L_TRI_EVAL_DIST;

L_TRI_CHECK_C:
    // cp = q - v2
    sub.f64 %fd25, %fd0, %fd16;
    sub.f64 %fd26, %fd1, %fd17;
    sub.f64 %fd27, %fd2, %fd18;
    // d5 = dot(ab, cp), d6 = dot(ac, cp)
    mul.f64 %fd33, %fd21, %fd27;
    fma.rn.f64 %fd33, %fd20, %fd26, %fd33;
    fma.rn.f64 %fd33, %fd19, %fd25, %fd33; // d5
    mul.f64 %fd34, %fd24, %fd27;
    fma.rn.f64 %fd34, %fd23, %fd26, %fd34;
    fma.rn.f64 %fd34, %fd22, %fd25, %fd34; // d6

    setp.ge.f64 %p6, %fd34, 0d0000000000000000;
    setp.le.f64 %p7, %fd33, %fd34;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_TRI_CHECK_AC;
    mov.f64 %fd40, %fd16;
    mov.f64 %fd41, %fd17;
    mov.f64 %fd42, %fd18;
    bra L_TRI_EVAL_DIST;

L_TRI_CHECK_AC:
    // vb = d5 * d2 - d1 * d6
    mul.f64 %fd35, %fd28, %fd34;
    neg.f64 %fd35, %fd35;
    fma.rn.f64 %fd35, %fd33, %fd29, %fd35; // vb

    setp.le.f64 %p6, %fd35, 0d0000000000000000;
    setp.ge.f64 %p7, %fd29, 0d0000000000000000;
    setp.le.f64 %p8, %fd34, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_TRI_CHECK_BC;
    sub.f64 %fd36, %fd29, %fd34;
    setp.gt.f64 %p12, %fd36, 0d0000000000000000;
    @!%p12 bra L_TRI_CHECK_BC;
    div.rn.f64 %fd37, %fd29, %fd36; // bary_w
    fma.rn.f64 %fd40, %fd37, %fd22, %fd10;
    fma.rn.f64 %fd41, %fd37, %fd23, %fd11;
    fma.rn.f64 %fd42, %fd37, %fd24, %fd12;
    bra L_TRI_EVAL_DIST;

L_TRI_CHECK_BC:
    // va = d3 * d6 - d5 * d4
    mul.f64 %fd36, %fd33, %fd31;
    neg.f64 %fd36, %fd36;
    fma.rn.f64 %fd36, %fd30, %fd34, %fd36; // va

    sub.f64 %fd37, %fd31, %fd30; // d4 - d3
    sub.f64 %fd38, %fd33, %fd34; // d5 - d6
    setp.le.f64 %p6, %fd36, 0d0000000000000000;
    setp.ge.f64 %p7, %fd37, 0d0000000000000000;
    setp.ge.f64 %p8, %fd38, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_TRI_FACE;
    add.f64 %fd39, %fd37, %fd38;
    setp.gt.f64 %p12, %fd39, 0d0000000000000000;
    @!%p12 bra L_TRI_FACE;
    div.rn.f64 %fd39, %fd37, %fd39; // bary_w
    sub.f64 %fd43, %fd16, %fd13;
    sub.f64 %fd44, %fd17, %fd14;
    sub.f64 %fd45, %fd18, %fd15;
    fma.rn.f64 %fd40, %fd39, %fd43, %fd13;
    fma.rn.f64 %fd41, %fd39, %fd44, %fd14;
    fma.rn.f64 %fd42, %fd39, %fd45, %fd15;
    bra L_TRI_EVAL_DIST;

L_TRI_FACE:
    // Accurate normal difference of products: w = c*d, e = fma(-c,d,w), f = fma(a,b,-w), res = f + e
    mul.rn.f64 %fd32, %fd21, %fd23;
    neg.f64 %fd35, %fd21;
    fma.rn.f64 %fd35, %fd35, %fd23, %fd32;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd20, %fd24, %fd32;
    add.f64 %fd32, %fd32, %fd35; // nx
    mul.rn.f64 %fd33, %fd19, %fd24;
    neg.f64 %fd35, %fd19;
    fma.rn.f64 %fd35, %fd35, %fd24, %fd33;
    neg.f64 %fd33, %fd33;
    fma.rn.f64 %fd33, %fd21, %fd22, %fd33;
    add.f64 %fd33, %fd33, %fd35; // ny
    mul.rn.f64 %fd34, %fd20, %fd22;
    neg.f64 %fd35, %fd20;
    fma.rn.f64 %fd35, %fd35, %fd22, %fd34;
    neg.f64 %fd34, %fd34;
    fma.rn.f64 %fd34, %fd19, %fd23, %fd34;
    add.f64 %fd34, %fd34, %fd35; // nz
    mul.f64 %fd35, %fd34, %fd34;
    fma.rn.f64 %fd35, %fd33, %fd33, %fd35;
    fma.rn.f64 %fd35, %fd32, %fd32, %fd35; // n_sq

    setp.gt.f64 %p12, %fd35, 0d0000000000000000;
    @!%p12 bra L_TRI_FACE_ZERO;

    sub.f64 %fd36, %fd0, %fd10; // ap.x
    sub.f64 %fd37, %fd1, %fd11; // ap.y
    sub.f64 %fd38, %fd2, %fd12; // ap.z
    mul.f64 %fd39, %fd38, %fd34;
    fma.rn.f64 %fd39, %fd37, %fd33, %fd39;
    fma.rn.f64 %fd39, %fd36, %fd32, %fd39; // ap_dot_n
    div.rn.f64 %fd39, %fd39, %fd35; // k = ap_dot_n / n_sq
    neg.f64 %fd39, %fd39; // -k
    fma.rn.f64 %fd40, %fd39, %fd32, %fd0; // proj.x = query.x - k * nx
    fma.rn.f64 %fd41, %fd39, %fd33, %fd1; // proj.y = query.y - k * ny
    fma.rn.f64 %fd42, %fd39, %fd34, %fd2; // proj.z = query.z - k * nz
    bra L_TRI_EVAL_DIST;
L_TRI_FACE_ZERO:
    mov.f64 %fd40, %fd10;
    mov.f64 %fd41, %fd11;
    mov.f64 %fd42, %fd12;
    bra L_TRI_EVAL_DIST;

L_TRI_DEGENERATE:
    // Segment 1: v0 -> v1 (u = ab, u_sq = ab_sq in %fd36, w = d1 in %fd28)
    setp.gt.f64 %p7, %fd36, 0d0000000000000000;
    div.rn.f64 %fd30, %fd28, %fd36;
    max.f64 %fd30, %fd30, 0d0000000000000000;
    min.f64 %fd30, %fd30, 0d3FF0000000000000;
    selp.f64 %fd30, %fd30, 0d0000000000000000, %p7;
    fma.rn.f64 %fd40, %fd30, %fd19, %fd10; // p1.x
    fma.rn.f64 %fd41, %fd30, %fd20, %fd11; // p1.y
    fma.rn.f64 %fd42, %fd30, %fd21, %fd12; // p1.z
    sub.f64 %fd31, %fd0, %fd40;
    sub.f64 %fd32, %fd1, %fd41;
    sub.f64 %fd33, %fd2, %fd42;
    mul.f64 %fd47, %fd33, %fd33;
    fma.rn.f64 %fd47, %fd32, %fd32, %fd47;
    fma.rn.f64 %fd47, %fd31, %fd31, %fd47; // d1_sq

    // Segment 2: v1 -> v2 (u = v2 - v1)
    sub.f64 %fd30, %fd16, %fd13; // bc.x
    sub.f64 %fd31, %fd17, %fd14; // bc.y
    sub.f64 %fd32, %fd18, %fd15; // bc.z
    mul.f64 %fd33, %fd32, %fd32;
    fma.rn.f64 %fd33, %fd31, %fd31, %fd33;
    fma.rn.f64 %fd33, %fd30, %fd30, %fd33; // bc_sq
    sub.f64 %fd34, %fd0, %fd13;  // bp.x
    sub.f64 %fd35, %fd1, %fd14;  // bp.y
    sub.f64 %fd36, %fd2, %fd15;  // bp.z
    mul.f64 %fd37, %fd32, %fd36;
    fma.rn.f64 %fd37, %fd31, %fd35, %fd37;
    fma.rn.f64 %fd37, %fd30, %fd34, %fd37; // bp_dot_bc
    setp.gt.f64 %p7, %fd33, 0d0000000000000000;
    div.rn.f64 %fd38, %fd37, %fd33;
    max.f64 %fd38, %fd38, 0d0000000000000000;
    min.f64 %fd38, %fd38, 0d3FF0000000000000;
    selp.f64 %fd38, %fd38, 0d0000000000000000, %p7;
    fma.rn.f64 %fd34, %fd38, %fd30, %fd13; // p2.x
    fma.rn.f64 %fd35, %fd38, %fd31, %fd14; // p2.y
    fma.rn.f64 %fd36, %fd38, %fd32, %fd15; // p2.z
    sub.f64 %fd37, %fd0, %fd34;
    sub.f64 %fd38, %fd1, %fd35;
    sub.f64 %fd39, %fd2, %fd36;
    mul.f64 %fd43, %fd39, %fd39;
    fma.rn.f64 %fd43, %fd38, %fd38, %fd43;
    fma.rn.f64 %fd43, %fd37, %fd37, %fd43; // d2_sq
    setp.lt.f64 %p8, %fd43, %fd47;
    selp.f64 %fd40, %fd34, %fd40, %p8;
    selp.f64 %fd41, %fd35, %fd41, %p8;
    selp.f64 %fd42, %fd36, %fd42, %p8;
    selp.f64 %fd47, %fd43, %fd47, %p8;

    // Segment 3: v2 -> v0 (u = v0 - v2)
    sub.f64 %fd30, %fd10, %fd16; // ca.x
    sub.f64 %fd31, %fd11, %fd17; // ca.y
    sub.f64 %fd32, %fd12, %fd18; // ca.z
    mul.f64 %fd33, %fd32, %fd32;
    fma.rn.f64 %fd33, %fd31, %fd31, %fd33;
    fma.rn.f64 %fd33, %fd30, %fd30, %fd33; // ca_sq
    sub.f64 %fd34, %fd0, %fd16;  // cp.x
    sub.f64 %fd35, %fd1, %fd17;  // cp.y
    sub.f64 %fd36, %fd2, %fd18;  // cp.z
    mul.f64 %fd37, %fd32, %fd36;
    fma.rn.f64 %fd37, %fd31, %fd35, %fd37;
    fma.rn.f64 %fd37, %fd30, %fd34, %fd37; // cp_dot_ca
    setp.gt.f64 %p7, %fd33, 0d0000000000000000;
    div.rn.f64 %fd38, %fd37, %fd33;
    max.f64 %fd38, %fd38, 0d0000000000000000;
    min.f64 %fd38, %fd38, 0d3FF0000000000000;
    selp.f64 %fd38, %fd38, 0d0000000000000000, %p7;
    fma.rn.f64 %fd34, %fd38, %fd30, %fd16; // p3.x
    fma.rn.f64 %fd35, %fd38, %fd31, %fd17; // p3.y
    fma.rn.f64 %fd36, %fd38, %fd32, %fd18; // p3.z
    sub.f64 %fd37, %fd0, %fd34;
    sub.f64 %fd38, %fd1, %fd35;
    sub.f64 %fd39, %fd2, %fd36;
    mul.f64 %fd43, %fd39, %fd39;
    fma.rn.f64 %fd43, %fd38, %fd38, %fd43;
    fma.rn.f64 %fd43, %fd37, %fd37, %fd43; // d3_sq
    setp.lt.f64 %p8, %fd43, %fd47;
    selp.f64 %fd40, %fd34, %fd40, %p8;
    selp.f64 %fd41, %fd35, %fd41, %p8;
    selp.f64 %fd42, %fd36, %fd42, %p8;
    bra L_TRI_EVAL_DIST;

L_TRI_EVAL_DIST:
    sub.f64 %fd43, %fd0, %fd40;
    sub.f64 %fd44, %fd1, %fd41;
    sub.f64 %fd45, %fd2, %fd42;
    mul.f64 %fd46, %fd45, %fd45;
    fma.rn.f64 %fd46, %fd44, %fd44, %fd46;
    fma.rn.f64 %fd46, %fd43, %fd43, %fd46;

    setp.lt.f64 %p9, %fd46, %fd3;
    @!%p9 bra L_BVH_LOOP;

    mov.f64 %fd3, %fd46;
    mov.f64 %fd4, %fd40;
    mov.f64 %fd5, %fd41;
    mov.f64 %fd6, %fd42;
    mov.u32 %r7, %r11;
    bra L_BVH_LOOP;

L_BVH_STORE:
    // Output per query: 5 f64s (40 bytes): best_x, best_y, best_z, best_sq, tri_idx_bits
    mul.wide.u32 %rd16, %r6, 40;
    add.u64 %rd17, %rd8, %rd16;
    cvt.u64.u32 %rd18, %r7;
    mov.b64 %fd47, %rd18;
    st.global.f64 [%rd17 + 0],  %fd4;
    st.global.f64 [%rd17 + 8],  %fd5;
    st.global.f64 [%rd17 + 16], %fd6;
    st.global.f64 [%rd17 + 24], %fd3;
    st.global.f64 [%rd17 + 32], %fd47;

L_BVH_EXIT:
    ret;
}

// 9. Ampere sm_86 Specialized KD-Tree Query with Warp-Synchronous Butterfly Reduction (shfl.sync.bfly.b32)
.visible .entry cuda_sm86_kdtree_query_f64(
    .param .u64 param_nodes,
    .param .u64 param_queries,
    .param .u64 param_warp_out,
    .param .u32 param_n_queries,
    .param .u32 param_n_nodes
) {
    .local .align 16 .b8 %stack[512];
    .reg .pred %p<14>;
    .reg .b32 %r<30>;
    .reg .b64 %rd<18>;
    .reg .f64 %fd<16>;

    ld.param.u64 %rd1, [param_nodes];
    ld.param.u64 %rd2, [param_queries];
    ld.param.u64 %rd3, [param_warp_out];
    ld.param.u32 %r1, [param_n_queries];
    ld.param.u32 %r2, [param_n_nodes];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd6, %rd3;

    mov.f64 %fd3, 0d0000000000000000;
    setp.ge.u32 %p0, %r6, %r1;
    setp.eq.u32 %p1, %r2, 0;
    or.pred %p0, %p0, %p1;
    @%p0 bra L_SM86_KD_WARP_REDUCE;

    mul.wide.u32 %rd7, %r6, 32;
    add.u64 %rd8, %rd5, %rd7;
    ld.global.nc.f64 %fd0, [%rd8 + 0];
    ld.global.nc.f64 %fd1, [%rd8 + 8];
    ld.global.nc.f64 %fd2, [%rd8 + 16];

    sub.u32 %r8, %r2, 1;
    min.u32 %r8, %r6, %r8;
    mul.wide.u32 %rd12, %r8, 32;
    add.u64 %rd13, %rd4, %rd12;
    ld.global.nc.f64 %fd5, [%rd13 + 0];
    ld.global.nc.f64 %fd6, [%rd13 + 8];
    ld.global.nc.f64 %fd7, [%rd13 + 16];
    sub.f64 %fd8,  %fd0, %fd5;
    sub.f64 %fd9,  %fd1, %fd6;
    sub.f64 %fd10, %fd2, %fd7;
    mul.f64 %fd3, %fd10, %fd10;
    fma.rn.f64 %fd3, %fd9, %fd9, %fd3;
    fma.rn.f64 %fd3, %fd8, %fd8, %fd3;

    mov.u64 %rd9, %stack;
    st.local.u32 [%rd9 + 0], 0;
    st.local.u32 [%rd9 + 4], %r2;
    st.local.f64 [%rd9 + 8], 0d0000000000000000;
    mov.u32 %r7, 1;

L_SM86_KD_LOOP:
    setp.eq.u32 %p2, %r7, 0;
    @%p2 bra L_SM86_KD_WARP_REDUCE;

    sub.u32 %r7, %r7, 1;
    shl.b32 %r8, %r7, 4;
    cvt.u64.u32 %rd10, %r8;
    add.u64 %rd11, %rd9, %rd10;
    ld.local.u32 %r9,  [%rd11 + 0];
    ld.local.u32 %r10, [%rd11 + 4];
    ld.local.f64 %fd4, [%rd11 + 8];

    setp.ge.f64 %p3, %fd4, %fd3;
    @%p3 bra L_SM86_KD_LOOP;

    mul.wide.u32 %rd12, %r9, 32;
    add.u64 %rd13, %rd4, %rd12;
    ld.global.nc.f64 %fd5, [%rd13 + 0];
    ld.global.nc.f64 %fd6, [%rd13 + 8];
    ld.global.nc.f64 %fd7, [%rd13 + 16];
    ld.global.nc.u32 %r11, [%rd13 + 24];
    ld.global.nc.u32 %r12, [%rd13 + 28];

    sub.f64 %fd8,  %fd0, %fd5;
    sub.f64 %fd9,  %fd1, %fd6;
    sub.f64 %fd10, %fd2, %fd7;

    mul.f64 %fd11, %fd10, %fd10;
    fma.rn.f64 %fd11, %fd9, %fd9, %fd11;
    fma.rn.f64 %fd11, %fd8, %fd8, %fd11;
    min.f64 %fd3, %fd3, %fd11;

    setp.eq.u32 %p4, %r11, 0;
    setp.eq.u32 %p5, %r11, 1;
    selp.f64 %fd12, %fd9,  %fd10, %p5;
    selp.f64 %fd12, %fd8,  %fd12, %p4;
    mul.f64 %fd13, %fd12, %fd12;

    min.u32 %r13, %r12, %r10;
    add.u32 %r14, %r9, 1;

    setp.le.f64 %p6, %fd12, 0d0000000000000000;
    selp.u32 %r15, %r14, %r12, %p6;
    selp.u32 %r16, %r13, %r10, %p6;
    selp.u32 %r17, %r12, %r14, %p6;
    selp.u32 %r18, %r10, %r13, %p6;

    setp.lt.u32 %p7, %r17, %r18;
    setp.lt.f64 %p8, %fd13, %fd3;
    and.pred %p7, %p7, %p8;
    setp.lt.u32 %p9, %r7, 32;
    and.pred %p7, %p7, %p9;
    @!%p7 bra L_SM86_KD_PUSH_NEAR;

    shl.b32 %r19, %r7, 4;
    cvt.u64.u32 %rd14, %r19;
    add.u64 %rd15, %rd9, %rd14;
    st.local.u32 [%rd15 + 0], %r17;
    st.local.u32 [%rd15 + 4], %r18;
    st.local.f64 [%rd15 + 8], %fd13;
    add.u32 %r7, %r7, 1;

L_SM86_KD_PUSH_NEAR:
    setp.lt.u32 %p10, %r15, %r16;
    setp.lt.u32 %p11, %r7, 32;
    and.pred %p10, %p10, %p11;
    @!%p10 bra L_SM86_KD_LOOP;

    shl.b32 %r20, %r7, 4;
    cvt.u64.u32 %rd16, %r20;
    add.u64 %rd17, %rd9, %rd16;
    st.local.u32 [%rd17 + 0], %r15;
    st.local.u32 [%rd17 + 4], %r16;
    st.local.f64 [%rd17 + 8], 0d0000000000000000;
    add.u32 %r7, %r7, 1;
    bra L_SM86_KD_LOOP;

L_SM86_KD_WARP_REDUCE:
    // 5-stage butterfly warp reduction across 32 lanes using shfl.sync.bfly.b32 on 64-bit halves
    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 16, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 16, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 8, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 8, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 4, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 4, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 2, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 2, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 1, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 1, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    and.b32 %r25, %r5, 31;
    setp.ne.u32 %p12, %r25, 0;
    @%p12 bra L_SM86_KD_EXIT;

    shr.u32 %r26, %r6, 5; // global_warp_id = global_tid >> 5
    mul.wide.u32 %rd12, %r26, 8;
    add.u64 %rd13, %rd6, %rd12;
    st.global.f64 [%rd13], %fd3;

L_SM86_KD_EXIT:
    ret;
}

// 10. Ampere sm_86 Specialized BVH Closest-Point Query with Non-Coherent L1 Cache Loads and Fast FP64 FMA
.visible .entry cuda_sm86_bvh_closest_point_f64(
    .param .u64 param_bvh,
    .param .u64 param_tris,
    .param .u64 param_queries,
    .param .u64 param_out,
    .param .u32 param_n_queries,
    .param .u32 param_n_nodes
) {
    .local .align 16 .b8 %stack[1024];
    .reg .pred %p<16>;
    .reg .b32 %r<22>;
    .reg .b64 %rd<20>;
    .reg .f64 %fd<52>;

    ld.param.u64 %rd1, [param_bvh];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd3, [param_queries];
    ld.param.u64 %rd4, [param_out];
    ld.param.u32 %r1, [param_n_queries];
    ld.param.u32 %r2, [param_n_nodes];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_SM86_BVH_EXIT;

    cvta.to.global.u64 %rd5, %rd1;
    cvta.to.global.u64 %rd6, %rd2;
    cvta.to.global.u64 %rd7, %rd3;
    cvta.to.global.u64 %rd8, %rd4;

    mul.wide.u32 %rd9, %r6, 32;
    add.u64 %rd10, %rd7, %rd9;
    ld.global.nc.f64 %fd0, [%rd10 + 0];
    ld.global.nc.f64 %fd1, [%rd10 + 8];
    ld.global.nc.f64 %fd2, [%rd10 + 16];

    mov.f64 %fd3, 0d7FF0000000000000;
    mov.f64 %fd4, 0d0000000000000000;
    mov.f64 %fd5, 0d0000000000000000;
    mov.f64 %fd6, 0d0000000000000000;
    mov.u32 %r7, 0xffffffff;

    setp.eq.u32 %p1, %r2, 0;
    @%p1 bra L_SM86_BVH_STORE;

    mov.u64 %rd11, %stack;
    st.local.u32 [%rd11 + 0], 0;
    st.local.u32 [%rd11 + 4], 0;
    st.local.f64 [%rd11 + 8], 0d0000000000000000;
    mov.u32 %r8, 1;

L_SM86_BVH_LOOP:
    setp.eq.u32 %p2, %r8, 0;
    @%p2 bra L_SM86_BVH_STORE;

    sub.u32 %r8, %r8, 1;
    shl.b32 %r9, %r8, 4;
    cvt.u64.u32 %rd12, %r9;
    add.u64 %rd13, %rd11, %rd12;
    ld.local.u32 %r10, [%rd13 + 0];
    ld.local.f64 %fd7, [%rd13 + 8];

    setp.ge.f64 %p3, %fd7, %fd3;
    setp.ne.u32 %p4, %r7, 0xffffffff;
    and.pred %p3, %p3, %p4;
    @%p3 bra L_SM86_BVH_LOOP;

    mul.wide.u32 %rd14, %r10, 64;
    add.u64 %rd15, %rd5, %rd14;
    ld.global.nc.u32 %r11, [%rd15 + 48];
    ld.global.nc.u32 %r12, [%rd15 + 52];

    setp.eq.u32 %p5, %r11, 0xffffffff;
    @!%p5 bra L_SM86_BVH_LEAF;

    add.u32 %r13, %r10, 1;
    mov.f64 %fd8, 0d7FF0000000000000;
    setp.lt.u32 %p6, %r13, %r2;
    @!%p6 bra L_SM86_BVH_SKIP_LEFT;

    mul.wide.u32 %rd16, %r13, 64;
    add.u64 %rd17, %rd5, %rd16;
    ld.global.nc.f64 %fd10, [%rd17 + 0];
    ld.global.nc.f64 %fd11, [%rd17 + 8];
    ld.global.nc.f64 %fd12, [%rd17 + 16];
    ld.global.nc.f64 %fd13, [%rd17 + 24];
    ld.global.nc.f64 %fd14, [%rd17 + 32];
    ld.global.nc.f64 %fd15, [%rd17 + 40];
    sub.f64 %fd16, %fd10, %fd0;
    sub.f64 %fd17, %fd0, %fd13;
    max.f64 %fd16, %fd16, %fd17;
    max.f64 %fd16, %fd16, 0d0000000000000000;
    sub.f64 %fd18, %fd11, %fd1;
    sub.f64 %fd19, %fd1, %fd14;
    max.f64 %fd18, %fd18, %fd19;
    max.f64 %fd18, %fd18, 0d0000000000000000;
    sub.f64 %fd20, %fd12, %fd2;
    sub.f64 %fd21, %fd2, %fd15;
    max.f64 %fd20, %fd20, %fd21;
    max.f64 %fd20, %fd20, 0d0000000000000000;
    mul.f64 %fd8, %fd20, %fd20;
    fma.rn.f64 %fd8, %fd18, %fd18, %fd8;
    fma.rn.f64 %fd8, %fd16, %fd16, %fd8;

L_SM86_BVH_SKIP_LEFT:
    mov.f64 %fd9, 0d7FF0000000000000;
    setp.lt.u32 %p7, %r12, %r2;
    @!%p7 bra L_SM86_BVH_SKIP_RIGHT;

    mul.wide.u32 %rd16, %r12, 64;
    add.u64 %rd17, %rd5, %rd16;
    ld.global.nc.f64 %fd10, [%rd17 + 0];
    ld.global.nc.f64 %fd11, [%rd17 + 8];
    ld.global.nc.f64 %fd12, [%rd17 + 16];
    ld.global.nc.f64 %fd13, [%rd17 + 24];
    ld.global.nc.f64 %fd14, [%rd17 + 32];
    ld.global.nc.f64 %fd15, [%rd17 + 40];
    sub.f64 %fd16, %fd10, %fd0;
    sub.f64 %fd17, %fd0, %fd13;
    max.f64 %fd16, %fd16, %fd17;
    max.f64 %fd16, %fd16, 0d0000000000000000;
    sub.f64 %fd18, %fd11, %fd1;
    sub.f64 %fd19, %fd1, %fd14;
    max.f64 %fd18, %fd18, %fd19;
    max.f64 %fd18, %fd18, 0d0000000000000000;
    sub.f64 %fd20, %fd12, %fd2;
    sub.f64 %fd21, %fd2, %fd15;
    max.f64 %fd20, %fd20, %fd21;
    max.f64 %fd20, %fd20, 0d0000000000000000;
    mul.f64 %fd9, %fd20, %fd20;
    fma.rn.f64 %fd9, %fd18, %fd18, %fd9;
    fma.rn.f64 %fd9, %fd16, %fd16, %fd9;

L_SM86_BVH_SKIP_RIGHT:
    setp.lt.f64 %p8, %fd8, %fd9;
    selp.u32 %r14, %r13, %r12, %p8;
    selp.f64 %fd22, %fd8, %fd9, %p8;
    selp.u32 %r15, %r12, %r13, %p8;
    selp.f64 %fd23, %fd9, %fd8, %p8;

    setp.lt.u32 %p9, %r15, %r2;
    setp.lt.f64 %p10, %fd23, %fd3;
    and.pred %p9, %p9, %p10;
    setp.lt.u32 %p12, %r8, 64;
    and.pred %p9, %p9, %p12;
    @!%p9 bra L_SM86_BVH_PUSH_NEAR;

    shl.b32 %r16, %r8, 4;
    cvt.u64.u32 %rd18, %r16;
    add.u64 %rd19, %rd11, %rd18;
    st.local.u32 [%rd19 + 0], %r15;
    st.local.u32 [%rd19 + 4], 0;
    st.local.f64 [%rd19 + 8], %fd23;
    add.u32 %r8, %r8, 1;

L_SM86_BVH_PUSH_NEAR:
    setp.lt.u32 %p9, %r14, %r2;
    setp.lt.f64 %p10, %fd22, %fd3;
    and.pred %p9, %p9, %p10;
    setp.lt.u32 %p12, %r8, 64;
    and.pred %p9, %p9, %p12;
    @!%p9 bra L_SM86_BVH_LOOP;

    shl.b32 %r16, %r8, 4;
    cvt.u64.u32 %rd18, %r16;
    add.u64 %rd19, %rd11, %rd18;
    st.local.u32 [%rd19 + 0], %r14;
    st.local.u32 [%rd19 + 4], 0;
    st.local.f64 [%rd19 + 8], %fd22;
    add.u32 %r8, %r8, 1;
    bra L_SM86_BVH_LOOP;

L_SM86_BVH_LEAF:
    mul.wide.u32 %rd16, %r11, 72;
    add.u64 %rd17, %rd6, %rd16;
    ld.global.nc.f64 %fd10, [%rd17 + 0];
    ld.global.nc.f64 %fd11, [%rd17 + 8];
    ld.global.nc.f64 %fd12, [%rd17 + 16];
    ld.global.nc.f64 %fd13, [%rd17 + 24];
    ld.global.nc.f64 %fd14, [%rd17 + 32];
    ld.global.nc.f64 %fd15, [%rd17 + 40];
    ld.global.nc.f64 %fd16, [%rd17 + 48];
    ld.global.nc.f64 %fd17, [%rd17 + 56];
    ld.global.nc.f64 %fd18, [%rd17 + 64];

    sub.f64 %fd19, %fd13, %fd10;
    sub.f64 %fd20, %fd14, %fd11;
    sub.f64 %fd21, %fd15, %fd12;
    sub.f64 %fd22, %fd16, %fd10;
    sub.f64 %fd23, %fd17, %fd11;
    sub.f64 %fd24, %fd18, %fd12;
    sub.f64 %fd25, %fd0,  %fd10;
    sub.f64 %fd26, %fd1,  %fd11;
    sub.f64 %fd27, %fd2,  %fd12;

    mul.f64 %fd28, %fd21, %fd27;
    fma.rn.f64 %fd28, %fd20, %fd26, %fd28;
    fma.rn.f64 %fd28, %fd19, %fd25, %fd28;
    mul.f64 %fd29, %fd24, %fd27;
    fma.rn.f64 %fd29, %fd23, %fd26, %fd29;
    fma.rn.f64 %fd29, %fd22, %fd25, %fd29;

    // Degenerate triangle check: n = ab x ac, |n|^2 <= f64::EPSILON * |ab|^2 * |ac|^2
    mul.rn.f64 %fd32, %fd21, %fd23;
    neg.f64 %fd35, %fd21;
    fma.rn.f64 %fd35, %fd35, %fd23, %fd32;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd20, %fd24, %fd32;
    add.f64 %fd32, %fd32, %fd35; // nx
    mul.rn.f64 %fd33, %fd19, %fd24;
    neg.f64 %fd35, %fd19;
    fma.rn.f64 %fd35, %fd35, %fd24, %fd33;
    neg.f64 %fd33, %fd33;
    fma.rn.f64 %fd33, %fd21, %fd22, %fd33;
    add.f64 %fd33, %fd33, %fd35; // ny
    mul.rn.f64 %fd34, %fd20, %fd22;
    neg.f64 %fd35, %fd20;
    fma.rn.f64 %fd35, %fd35, %fd22, %fd34;
    neg.f64 %fd34, %fd34;
    fma.rn.f64 %fd34, %fd19, %fd23, %fd34;
    add.f64 %fd34, %fd34, %fd35; // nz
    mul.f64 %fd35, %fd34, %fd34;
    fma.rn.f64 %fd35, %fd33, %fd33, %fd35;
    fma.rn.f64 %fd35, %fd32, %fd32, %fd35; // n_sq
    mul.f64 %fd36, %fd21, %fd21;
    fma.rn.f64 %fd36, %fd20, %fd20, %fd36;
    fma.rn.f64 %fd36, %fd19, %fd19, %fd36; // ab_sq
    mul.f64 %fd37, %fd24, %fd24;
    fma.rn.f64 %fd37, %fd23, %fd23, %fd37;
    fma.rn.f64 %fd37, %fd22, %fd22, %fd37; // ac_sq
    mul.f64 %fd38, %fd36, %fd37;
    mul.f64 %fd38, %fd38, 0d3CB0000000000000; // DEGENERATE_AREA_FACTOR (f64::EPSILON)
    setp.le.f64 %p6, %fd35, %fd38;
    @%p6 bra L_SM86_TRI_DEGENERATE;

    setp.le.f64 %p6, %fd28, 0d0000000000000000;
    setp.le.f64 %p7, %fd29, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_SM86_TRI_CHECK_B;
    mov.f64 %fd40, %fd10;
    mov.f64 %fd41, %fd11;
    mov.f64 %fd42, %fd12;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_CHECK_B:
    sub.f64 %fd25, %fd0, %fd13;
    sub.f64 %fd26, %fd1, %fd14;
    sub.f64 %fd27, %fd2, %fd15;
    mul.f64 %fd30, %fd21, %fd27;
    fma.rn.f64 %fd30, %fd20, %fd26, %fd30;
    fma.rn.f64 %fd30, %fd19, %fd25, %fd30;
    mul.f64 %fd31, %fd24, %fd27;
    fma.rn.f64 %fd31, %fd23, %fd26, %fd31;
    fma.rn.f64 %fd31, %fd22, %fd25, %fd31;

    setp.ge.f64 %p6, %fd30, 0d0000000000000000;
    setp.le.f64 %p7, %fd31, %fd30;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_SM86_TRI_CHECK_AB;
    mov.f64 %fd40, %fd13;
    mov.f64 %fd41, %fd14;
    mov.f64 %fd42, %fd15;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_CHECK_AB:
    mul.f64 %fd32, %fd30, %fd29;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd28, %fd31, %fd32;

    setp.le.f64 %p6, %fd32, 0d0000000000000000;
    setp.ge.f64 %p7, %fd28, 0d0000000000000000;
    setp.le.f64 %p8, %fd30, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_SM86_TRI_CHECK_C;
    sub.f64 %fd33, %fd28, %fd30;
    setp.gt.f64 %p12, %fd33, 0d0000000000000000;
    @!%p12 bra L_SM86_TRI_CHECK_C;
    div.rn.f64 %fd34, %fd28, %fd33;
    fma.rn.f64 %fd40, %fd34, %fd19, %fd10;
    fma.rn.f64 %fd41, %fd34, %fd20, %fd11;
    fma.rn.f64 %fd42, %fd34, %fd21, %fd12;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_CHECK_C:
    sub.f64 %fd25, %fd0, %fd16;
    sub.f64 %fd26, %fd1, %fd17;
    sub.f64 %fd27, %fd2, %fd18;
    mul.f64 %fd33, %fd21, %fd27;
    fma.rn.f64 %fd33, %fd20, %fd26, %fd33;
    fma.rn.f64 %fd33, %fd19, %fd25, %fd33;
    mul.f64 %fd34, %fd24, %fd27;
    fma.rn.f64 %fd34, %fd23, %fd26, %fd34;
    fma.rn.f64 %fd34, %fd22, %fd25, %fd34;

    setp.ge.f64 %p6, %fd34, 0d0000000000000000;
    setp.le.f64 %p7, %fd33, %fd34;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_SM86_TRI_CHECK_AC;
    mov.f64 %fd40, %fd16;
    mov.f64 %fd41, %fd17;
    mov.f64 %fd42, %fd18;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_CHECK_AC:
    mul.f64 %fd35, %fd28, %fd34;
    neg.f64 %fd35, %fd35;
    fma.rn.f64 %fd35, %fd33, %fd29, %fd35;

    setp.le.f64 %p6, %fd35, 0d0000000000000000;
    setp.ge.f64 %p7, %fd29, 0d0000000000000000;
    setp.le.f64 %p8, %fd34, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_SM86_TRI_CHECK_BC;
    sub.f64 %fd36, %fd29, %fd34;
    setp.gt.f64 %p12, %fd36, 0d0000000000000000;
    @!%p12 bra L_SM86_TRI_CHECK_BC;
    div.rn.f64 %fd37, %fd29, %fd36;
    fma.rn.f64 %fd40, %fd37, %fd22, %fd10;
    fma.rn.f64 %fd41, %fd37, %fd23, %fd11;
    fma.rn.f64 %fd42, %fd37, %fd24, %fd12;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_CHECK_BC:
    mul.f64 %fd36, %fd33, %fd31;
    neg.f64 %fd36, %fd36;
    fma.rn.f64 %fd36, %fd30, %fd34, %fd36;

    sub.f64 %fd37, %fd31, %fd30;
    sub.f64 %fd38, %fd33, %fd34;
    setp.le.f64 %p6, %fd36, 0d0000000000000000;
    setp.ge.f64 %p7, %fd37, 0d0000000000000000;
    setp.ge.f64 %p8, %fd38, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_SM86_TRI_FACE;
    add.f64 %fd39, %fd37, %fd38;
    setp.gt.f64 %p12, %fd39, 0d0000000000000000;
    @!%p12 bra L_SM86_TRI_FACE;
    div.rn.f64 %fd39, %fd37, %fd39;
    sub.f64 %fd43, %fd16, %fd13;
    sub.f64 %fd44, %fd17, %fd14;
    sub.f64 %fd45, %fd18, %fd15;
    fma.rn.f64 %fd40, %fd39, %fd43, %fd13;
    fma.rn.f64 %fd41, %fd39, %fd44, %fd14;
    fma.rn.f64 %fd42, %fd39, %fd45, %fd15;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_FACE:
    // Accurate normal difference of products: w = c*d, e = fma(-c,d,w), f = fma(a,b,-w), res = f + e
    mul.rn.f64 %fd32, %fd21, %fd23;
    neg.f64 %fd35, %fd21;
    fma.rn.f64 %fd35, %fd35, %fd23, %fd32;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd20, %fd24, %fd32;
    add.f64 %fd32, %fd32, %fd35; // nx
    mul.rn.f64 %fd33, %fd19, %fd24;
    neg.f64 %fd35, %fd19;
    fma.rn.f64 %fd35, %fd35, %fd24, %fd33;
    neg.f64 %fd33, %fd33;
    fma.rn.f64 %fd33, %fd21, %fd22, %fd33;
    add.f64 %fd33, %fd33, %fd35; // ny
    mul.rn.f64 %fd34, %fd20, %fd22;
    neg.f64 %fd35, %fd20;
    fma.rn.f64 %fd35, %fd35, %fd22, %fd34;
    neg.f64 %fd34, %fd34;
    fma.rn.f64 %fd34, %fd19, %fd23, %fd34;
    add.f64 %fd34, %fd34, %fd35; // nz
    mul.f64 %fd35, %fd34, %fd34;
    fma.rn.f64 %fd35, %fd33, %fd33, %fd35;
    fma.rn.f64 %fd35, %fd32, %fd32, %fd35; // n_sq

    setp.gt.f64 %p12, %fd35, 0d0000000000000000;
    @!%p12 bra L_SM86_TRI_FACE_ZERO;

    sub.f64 %fd36, %fd0, %fd10; // ap.x
    sub.f64 %fd37, %fd1, %fd11; // ap.y
    sub.f64 %fd38, %fd2, %fd12; // ap.z
    mul.f64 %fd39, %fd38, %fd34;
    fma.rn.f64 %fd39, %fd37, %fd33, %fd39;
    fma.rn.f64 %fd39, %fd36, %fd32, %fd39; // ap_dot_n
    div.rn.f64 %fd39, %fd39, %fd35; // k = ap_dot_n / n_sq
    neg.f64 %fd39, %fd39; // -k
    fma.rn.f64 %fd40, %fd39, %fd32, %fd0; // proj.x = query.x - k * nx
    fma.rn.f64 %fd41, %fd39, %fd33, %fd1; // proj.y = query.y - k * ny
    fma.rn.f64 %fd42, %fd39, %fd34, %fd2; // proj.z = query.z - k * nz
    bra L_SM86_TRI_EVAL_DIST;
L_SM86_TRI_FACE_ZERO:
    mov.f64 %fd40, %fd10;
    mov.f64 %fd41, %fd11;
    mov.f64 %fd42, %fd12;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_DEGENERATE:
    // Segment 1: v0 -> v1 (u = ab, u_sq = ab_sq in %fd36, w = d1 in %fd28)
    setp.gt.f64 %p7, %fd36, 0d0000000000000000;
    div.rn.f64 %fd30, %fd28, %fd36;
    max.f64 %fd30, %fd30, 0d0000000000000000;
    min.f64 %fd30, %fd30, 0d3FF0000000000000;
    selp.f64 %fd30, %fd30, 0d0000000000000000, %p7;
    fma.rn.f64 %fd40, %fd30, %fd19, %fd10; // p1.x
    fma.rn.f64 %fd41, %fd30, %fd20, %fd11; // p1.y
    fma.rn.f64 %fd42, %fd30, %fd21, %fd12; // p1.z
    sub.f64 %fd31, %fd0, %fd40;
    sub.f64 %fd32, %fd1, %fd41;
    sub.f64 %fd33, %fd2, %fd42;
    mul.f64 %fd47, %fd33, %fd33;
    fma.rn.f64 %fd47, %fd32, %fd32, %fd47;
    fma.rn.f64 %fd47, %fd31, %fd31, %fd47; // d1_sq

    // Segment 2: v1 -> v2 (u = v2 - v1)
    sub.f64 %fd30, %fd16, %fd13; // bc.x
    sub.f64 %fd31, %fd17, %fd14; // bc.y
    sub.f64 %fd32, %fd18, %fd15; // bc.z
    mul.f64 %fd33, %fd32, %fd32;
    fma.rn.f64 %fd33, %fd31, %fd31, %fd33;
    fma.rn.f64 %fd33, %fd30, %fd30, %fd33; // bc_sq
    sub.f64 %fd34, %fd0, %fd13;  // bp.x
    sub.f64 %fd35, %fd1, %fd14;  // bp.y
    sub.f64 %fd36, %fd2, %fd15;  // bp.z
    mul.f64 %fd37, %fd32, %fd36;
    fma.rn.f64 %fd37, %fd31, %fd35, %fd37;
    fma.rn.f64 %fd37, %fd30, %fd34, %fd37; // bp_dot_bc
    setp.gt.f64 %p7, %fd33, 0d0000000000000000;
    div.rn.f64 %fd38, %fd37, %fd33;
    max.f64 %fd38, %fd38, 0d0000000000000000;
    min.f64 %fd38, %fd38, 0d3FF0000000000000;
    selp.f64 %fd38, %fd38, 0d0000000000000000, %p7;
    fma.rn.f64 %fd34, %fd38, %fd30, %fd13; // p2.x
    fma.rn.f64 %fd35, %fd38, %fd31, %fd14; // p2.y
    fma.rn.f64 %fd36, %fd38, %fd32, %fd15; // p2.z
    sub.f64 %fd37, %fd0, %fd34;
    sub.f64 %fd38, %fd1, %fd35;
    sub.f64 %fd39, %fd2, %fd36;
    mul.f64 %fd43, %fd39, %fd39;
    fma.rn.f64 %fd43, %fd38, %fd38, %fd43;
    fma.rn.f64 %fd43, %fd37, %fd37, %fd43; // d2_sq
    setp.lt.f64 %p8, %fd43, %fd47;
    selp.f64 %fd40, %fd34, %fd40, %p8;
    selp.f64 %fd41, %fd35, %fd41, %p8;
    selp.f64 %fd42, %fd36, %fd42, %p8;
    selp.f64 %fd47, %fd43, %fd47, %p8;

    // Segment 3: v2 -> v0 (u = v0 - v2)
    sub.f64 %fd30, %fd10, %fd16; // ca.x
    sub.f64 %fd31, %fd11, %fd17; // ca.y
    sub.f64 %fd32, %fd12, %fd18; // ca.z
    mul.f64 %fd33, %fd32, %fd32;
    fma.rn.f64 %fd33, %fd31, %fd31, %fd33;
    fma.rn.f64 %fd33, %fd30, %fd30, %fd33; // ca_sq
    sub.f64 %fd34, %fd0, %fd16;  // cp.x
    sub.f64 %fd35, %fd1, %fd17;  // cp.y
    sub.f64 %fd36, %fd2, %fd18;  // cp.z
    mul.f64 %fd37, %fd32, %fd36;
    fma.rn.f64 %fd37, %fd31, %fd35, %fd37;
    fma.rn.f64 %fd37, %fd30, %fd34, %fd37; // cp_dot_ca
    setp.gt.f64 %p7, %fd33, 0d0000000000000000;
    div.rn.f64 %fd38, %fd37, %fd33;
    max.f64 %fd38, %fd38, 0d0000000000000000;
    min.f64 %fd38, %fd38, 0d3FF0000000000000;
    selp.f64 %fd38, %fd38, 0d0000000000000000, %p7;
    fma.rn.f64 %fd34, %fd38, %fd30, %fd16; // p3.x
    fma.rn.f64 %fd35, %fd38, %fd31, %fd17; // p3.y
    fma.rn.f64 %fd36, %fd38, %fd32, %fd18; // p3.z
    sub.f64 %fd37, %fd0, %fd34;
    sub.f64 %fd38, %fd1, %fd35;
    sub.f64 %fd39, %fd2, %fd36;
    mul.f64 %fd43, %fd39, %fd39;
    fma.rn.f64 %fd43, %fd38, %fd38, %fd43;
    fma.rn.f64 %fd43, %fd37, %fd37, %fd43; // d3_sq
    setp.lt.f64 %p8, %fd43, %fd47;
    selp.f64 %fd40, %fd34, %fd40, %p8;
    selp.f64 %fd41, %fd35, %fd41, %p8;
    selp.f64 %fd42, %fd36, %fd42, %p8;
    bra L_SM86_TRI_EVAL_DIST;

L_SM86_TRI_EVAL_DIST:
    sub.f64 %fd43, %fd0, %fd40;
    sub.f64 %fd44, %fd1, %fd41;
    sub.f64 %fd45, %fd2, %fd42;
    mul.f64 %fd46, %fd45, %fd45;
    fma.rn.f64 %fd46, %fd44, %fd44, %fd46;
    fma.rn.f64 %fd46, %fd43, %fd43, %fd46;

    setp.lt.f64 %p9, %fd46, %fd3;
    @!%p9 bra L_SM86_BVH_LOOP;

    mov.f64 %fd3, %fd46;
    mov.f64 %fd4, %fd40;
    mov.f64 %fd5, %fd41;
    mov.f64 %fd6, %fd42;
    mov.u32 %r7, %r11;
    bra L_SM86_BVH_LOOP;

L_SM86_BVH_STORE:
    mul.wide.u32 %rd16, %r6, 40;
    add.u64 %rd17, %rd8, %rd16;
    cvt.u64.u32 %rd18, %r7;
    mov.b64 %fd47, %rd18;
    st.global.f64 [%rd17 + 0],  %fd4;
    st.global.f64 [%rd17 + 8],  %fd5;
    st.global.f64 [%rd17 + 16], %fd6;
    st.global.f64 [%rd17 + 24], %fd3;
    st.global.f64 [%rd17 + 32], %fd47;

L_SM86_BVH_EXIT:
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

// 12b. GPU point-cloud AABB builder for OptiX custom-primitive BVH
.visible .entry cuda_build_point_aabbs_f64(
    .param .u64 param_pts,
    .param .u64 param_aabbs,
    .param .f32 param_r_pad,
    .param .u32 param_n_pts
) {
    .reg .pred %p0;
    .reg .b32 %r<6>;
    .reg .b64 %rd<8>;
    .reg .f64 %fd<3>;
    .reg .f32 %f<10>;

    ld.param.u64 %rd1, [param_pts];
    ld.param.u64 %rd2, [param_aabbs];
    ld.param.f32 %f0, [param_r_pad];
    ld.param.u32 %r1, [param_n_pts];

    mov.u32 %r2, %ctaid.x;
    mov.u32 %r3, %ntid.x;
    mov.u32 %r4, %tid.x;
    mad.lo.u32 %r5, %r2, %r3, %r4;
    setp.ge.u32 %p0, %r5, %r1;
    @%p0 bra L_BUILD_AABB_EXIT;

    cvta.to.global.u64 %rd3, %rd1;
    cvta.to.global.u64 %rd4, %rd2;

    mul.wide.u32 %rd5, %r5, 32;
    add.u64 %rd6, %rd3, %rd5;
    ld.global.nc.f64 %fd0, [%rd6 + 0];
    ld.global.nc.f64 %fd1, [%rd6 + 8];
    ld.global.nc.f64 %fd2, [%rd6 + 16];

    cvt.rn.f32.f64 %f1, %fd0;
    cvt.rn.f32.f64 %f2, %fd1;
    cvt.rn.f32.f64 %f3, %fd2;

    sub.f32 %f4, %f1, %f0;
    sub.f32 %f5, %f2, %f0;
    sub.f32 %f6, %f3, %f0;
    add.f32 %f7, %f1, %f0;
    add.f32 %f8, %f2, %f0;
    add.f32 %f9, %f3, %f0;

    mul.wide.u32 %rd5, %r5, 24;
    add.u64 %rd7, %rd4, %rd5;
    st.global.f32 [%rd7 + 0],  %f4;
    st.global.f32 [%rd7 + 4],  %f5;
    st.global.f32 [%rd7 + 8],  %f6;
    st.global.f32 [%rd7 + 12], %f7;
    st.global.f32 [%rd7 + 16], %f8;
    st.global.f32 [%rd7 + 20], %f9;

L_BUILD_AABB_EXIT:
    ret;
}

// 13. CUDA OptiX RT Core AABB-Certified Nearest-Neighbor + Warp-Butterfly Max Reduction
.visible .entry cuda_optix_kdtree_query_f64(
    .param .u64 param_targets,
    .param .u64 param_queries,
    .param .u64 param_cand_prim16,
    .param .u64 param_warp_out,
    .param .f64 param_r_pad_sq,
    .param .u32 param_n_queries,
    .param .u32 param_n_targets
) {
    .local .align 8 .b8 %stack[512];
    .reg .pred %p<12>;
    .reg .b32 %r<28>;
    .reg .b64 %rd<20>;
    .reg .f64 %fd<16>;

    ld.param.u64 %rd1, [param_targets];
    ld.param.u64 %rd2, [param_queries];
    ld.param.u64 %rd18, [param_cand_prim16];
    ld.param.u64 %rd3, [param_warp_out];
    ld.param.f64 %fd15, [param_r_pad_sq];
    ld.param.u32 %r1, [param_n_queries];
    ld.param.u32 %r2, [param_n_targets];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;

    cvta.to.global.u64 %rd4, %rd1;
    cvta.to.global.u64 %rd5, %rd2;
    cvta.to.global.u64 %rd19, %rd18;
    cvta.to.global.u64 %rd6, %rd3;

    mov.f64 %fd3, 0d0000000000000000;
    setp.ge.u32 %p0, %r6, %r1;
    setp.eq.u32 %p1, %r2, 0;
    or.pred %p0, %p0, %p1;
    @%p0 bra L_OPTIX_KD_WARP_REDUCE;

    mul.wide.u32 %rd7, %r6, 32;
    add.u64 %rd8, %rd5, %rd7;
    ld.global.nc.f64 %fd0, [%rd8 + 0];
    ld.global.nc.f64 %fd1, [%rd8 + 8];
    ld.global.nc.f64 %fd2, [%rd8 + 16];

    sub.u32 %r8, %r2, 1;
    min.u32 %r8, %r6, %r8;
    mul.wide.u32 %rd12, %r8, 32;
    add.u64 %rd13, %rd4, %rd12;
    ld.global.nc.f64 %fd5, [%rd13 + 0];
    ld.global.nc.f64 %fd6, [%rd13 + 8];
    ld.global.nc.f64 %fd7, [%rd13 + 16];
    sub.f64 %fd8,  %fd0, %fd5;
    sub.f64 %fd9,  %fd1, %fd6;
    sub.f64 %fd10, %fd2, %fd7;
    mul.f64 %fd3, %fd10, %fd10;
    fma.rn.f64 %fd3, %fd9, %fd9, %fd3;
    fma.rn.f64 %fd3, %fd8, %fd8, %fd3;

    setp.le.u32 %p1, %r2, 32;
    @%p1 bra L_OPTIX_KD_FALLBACK_START;

    mul.wide.u32 %rd7, %r6, 64;
    add.u64 %rd8, %rd19, %rd7;
    ld.global.nc.u32 %r20, [%rd8 + 0];
    min.u32 %r19, %r20, 15;
    mov.u32 %r21, 0;

L_OPTIX_KD_CAND_LOOP:
    setp.ge.u32 %p1, %r21, %r19;
    @%p1 bra L_OPTIX_KD_CAND_DONE;
    add.u32 %r21, %r21, 1;
    mul.wide.u32 %rd10, %r21, 4;
    add.u64 %rd11, %rd8, %rd10;
    ld.global.nc.u32 %r9, [%rd11];
    setp.ge.u32 %p1, %r9, %r2;
    @%p1 bra L_OPTIX_KD_CAND_LOOP;

    mul.wide.u32 %rd12, %r9, 32;
    add.u64 %rd13, %rd4, %rd12;
    ld.global.nc.f64 %fd5, [%rd13 + 0];
    ld.global.nc.f64 %fd6, [%rd13 + 8];
    ld.global.nc.f64 %fd7, [%rd13 + 16];
    sub.f64 %fd8,  %fd0, %fd5;
    sub.f64 %fd9,  %fd1, %fd6;
    sub.f64 %fd10, %fd2, %fd7;
    mul.f64 %fd11, %fd10, %fd10;
    fma.rn.f64 %fd11, %fd9, %fd9, %fd11;
    fma.rn.f64 %fd11, %fd8, %fd8, %fd11;
    min.f64 %fd3, %fd3, %fd11;
    bra L_OPTIX_KD_CAND_LOOP;

L_OPTIX_KD_CAND_DONE:
    setp.le.u32 %p2, %r20, 15;
    setp.le.f64 %p3, %fd3, %fd15;
    and.pred %p2, %p2, %p3;
    @%p2 bra L_OPTIX_KD_WARP_REDUCE;

L_OPTIX_KD_FALLBACK_START:
    mov.u64 %rd9, %stack;
    st.local.u32 [%rd9 + 0], 0;
    st.local.u32 [%rd9 + 4], %r2;
    st.local.f64 [%rd9 + 8], 0d0000000000000000;
    mov.u32 %r7, 1;

L_OPTIX_KD_FALLBACK_LOOP:
    setp.eq.u32 %p2, %r7, 0;
    @%p2 bra L_OPTIX_KD_WARP_REDUCE;

    sub.u32 %r7, %r7, 1;
    shl.b32 %r8, %r7, 4;
    cvt.u64.u32 %rd10, %r8;
    add.u64 %rd11, %rd9, %rd10;
    ld.local.u32 %r9,  [%rd11 + 0];
    ld.local.u32 %r10, [%rd11 + 4];
    ld.local.f64 %fd4, [%rd11 + 8];

    setp.ge.f64 %p3, %fd4, %fd3;
    @%p3 bra L_OPTIX_KD_FALLBACK_LOOP;

    mul.wide.u32 %rd12, %r9, 32;
    add.u64 %rd13, %rd4, %rd12;
    ld.global.nc.f64 %fd5, [%rd13 + 0];
    ld.global.nc.f64 %fd6, [%rd13 + 8];
    ld.global.nc.f64 %fd7, [%rd13 + 16];
    ld.global.nc.u32 %r11, [%rd13 + 24];
    ld.global.nc.u32 %r12, [%rd13 + 28];

    sub.f64 %fd8,  %fd0, %fd5;
    sub.f64 %fd9,  %fd1, %fd6;
    sub.f64 %fd10, %fd2, %fd7;

    mul.f64 %fd11, %fd10, %fd10;
    fma.rn.f64 %fd11, %fd9, %fd9, %fd11;
    fma.rn.f64 %fd11, %fd8, %fd8, %fd11;
    min.f64 %fd3, %fd3, %fd11;

    setp.eq.u32 %p4, %r11, 0;
    setp.eq.u32 %p5, %r11, 1;
    selp.f64 %fd12, %fd9,  %fd10, %p5;
    selp.f64 %fd12, %fd8,  %fd12, %p4;
    mul.f64 %fd13, %fd12, %fd12;

    min.u32 %r13, %r12, %r10;
    add.u32 %r14, %r9, 1;

    setp.le.f64 %p6, %fd12, 0d0000000000000000;
    selp.u32 %r15, %r14, %r12, %p6;
    selp.u32 %r16, %r13, %r10, %p6;
    selp.u32 %r17, %r12, %r14, %p6;
    selp.u32 %r18, %r10, %r13, %p6;

    setp.lt.u32 %p7, %r17, %r18;
    setp.lt.f64 %p8, %fd13, %fd3;
    and.pred %p7, %p7, %p8;
    setp.lt.u32 %p9, %r7, 32;
    and.pred %p7, %p7, %p9;
    @!%p7 bra L_OPTIX_KD_PUSH_NEAR;

    shl.b32 %r19, %r7, 4;
    cvt.u64.u32 %rd14, %r19;
    add.u64 %rd15, %rd9, %rd14;
    st.local.u32 [%rd15 + 0], %r17;
    st.local.u32 [%rd15 + 4], %r18;
    st.local.f64 [%rd15 + 8], %fd13;
    add.u32 %r7, %r7, 1;

L_OPTIX_KD_PUSH_NEAR:
    setp.lt.u32 %p10, %r15, %r16;
    setp.lt.u32 %p11, %r7, 32;
    and.pred %p10, %p10, %p11;
    @!%p10 bra L_OPTIX_KD_FALLBACK_LOOP;

    shl.b32 %r20, %r7, 4;
    cvt.u64.u32 %rd16, %r20;
    add.u64 %rd17, %rd9, %rd16;
    st.local.u32 [%rd17 + 0], %r15;
    st.local.u32 [%rd17 + 4], %r16;
    st.local.f64 [%rd17 + 8], 0d0000000000000000;
    add.u32 %r7, %r7, 1;
    bra L_OPTIX_KD_FALLBACK_LOOP;

L_OPTIX_KD_WARP_REDUCE:
    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 16, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 16, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 8, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 8, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 4, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 4, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 2, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 2, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    mov.b64 {%r21, %r22}, %fd3;
    shfl.sync.bfly.b32 %r23, %r21, 1, 31, 0xffffffff;
    shfl.sync.bfly.b32 %r24, %r22, 1, 31, 0xffffffff;
    mov.b64 %fd14, {%r23, %r24};
    max.f64 %fd3, %fd3, %fd14;

    and.b32 %r25, %r5, 31;
    setp.ne.u32 %p5, %r25, 0;
    @%p5 bra L_OPTIX_KD_EXIT;

    shr.u32 %r26, %r6, 5;
    mul.wide.u32 %rd12, %r26, 8;
    add.u64 %rd13, %rd6, %rd12;
    st.global.f64 [%rd13], %fd3;

L_OPTIX_KD_EXIT:
    ret;
}

// 14. CUDA OptiX RT Core AABB-Certified Closest-Point Query with Resident VRAM BVH Fallback
.visible .entry cuda_optix_bvh_closest_point_f64(
    .param .u64 param_bvh,
    .param .u64 param_tris,
    .param .u64 param_queries,
    .param .u64 param_cand_prim16,
    .param .u64 param_out,
    .param .f64 param_r_pad_sq,
    .param .u32 param_n_queries,
    .param .u32 param_n_nodes,
    .param .u32 param_n_tris
) {
    .local .align 16 .b8 %stack[1024];
    .reg .pred %p<16>;
    .reg .b32 %r<24>;
    .reg .b64 %rd<22>;
    .reg .f64 %fd<52>;

    ld.param.u64 %rd1, [param_bvh];
    ld.param.u64 %rd2, [param_tris];
    ld.param.u64 %rd3, [param_queries];
    ld.param.u64 %rd20, [param_cand_prim16];
    ld.param.u64 %rd4, [param_out];
    ld.param.f64 %fd51, [param_r_pad_sq];
    ld.param.u32 %r1, [param_n_queries];
    ld.param.u32 %r2, [param_n_nodes];
    ld.param.u32 %r19, [param_n_tris];

    mov.u32 %r3, %ctaid.x;
    mov.u32 %r4, %ntid.x;
    mov.u32 %r5, %tid.x;
    mad.lo.u32 %r6, %r3, %r4, %r5;
    setp.ge.u32 %p0, %r6, %r1;
    @%p0 bra L_OPTIX_BVH_EXIT;

    cvta.to.global.u64 %rd5, %rd1;
    cvta.to.global.u64 %rd6, %rd2;
    cvta.to.global.u64 %rd7, %rd3;
    cvta.to.global.u64 %rd21, %rd20;
    cvta.to.global.u64 %rd8, %rd4;

    mul.wide.u32 %rd9, %r6, 32;
    add.u64 %rd10, %rd7, %rd9;
    ld.global.nc.f64 %fd0, [%rd10 + 0];
    ld.global.nc.f64 %fd1, [%rd10 + 8];
    ld.global.nc.f64 %fd2, [%rd10 + 16];

    mov.f64 %fd3, 0d7FF0000000000000;
    mov.f64 %fd4, 0d0000000000000000;
    mov.f64 %fd5, 0d0000000000000000;
    mov.f64 %fd6, 0d0000000000000000;
    mov.u32 %r7, 0xffffffff;

    setp.eq.u32 %p1, %r2, 0;
    @%p1 bra L_OPTIX_BVH_STORE;

    mul.wide.u32 %rd9, %r6, 64;
    add.u64 %rd20, %rd21, %rd9;
    ld.global.nc.u32 %r20, [%rd20 + 0];
    min.u32 %r21, %r20, 15;
    mov.u32 %r18, 0;

    mov.u64 %rd11, %stack;
    st.local.u32 [%rd11 + 0], 0;
    st.local.u32 [%rd11 + 4], 0;
    st.local.f64 [%rd11 + 8], 0d0000000000000000;
    mov.u32 %r8, 1;

L_OPTIX_BVH_LOOP:
    setp.ge.u32 %p2, %r18, %r21;
    @%p2 bra L_OPTIX_BVH_CHECK_CERT;
    add.u32 %r18, %r18, 1;
    mul.wide.u32 %rd12, %r18, 4;
    add.u64 %rd13, %rd20, %rd12;
    ld.global.nc.u32 %r11, [%rd13];
    setp.lt.u32 %p2, %r11, %r19;
    @%p2 bra L_OPTIX_BVH_LEAF;
    bra L_OPTIX_BVH_LOOP;

L_OPTIX_BVH_CHECK_CERT:
    setp.gt.u32 %p2, %r19, 16;
    setp.le.u32 %p3, %r20, 15;
    and.pred %p2, %p2, %p3;
    setp.lt.f64 %p3, %fd3, %fd51;
    and.pred %p2, %p2, %p3;
    @%p2 bra L_OPTIX_BVH_STORE;

L_OPTIX_BVH_POP:
    setp.eq.u32 %p2, %r8, 0;
    @%p2 bra L_OPTIX_BVH_STORE;

    sub.u32 %r8, %r8, 1;
    shl.b32 %r9, %r8, 4;
    cvt.u64.u32 %rd12, %r9;
    add.u64 %rd13, %rd11, %rd12;
    ld.local.u32 %r10, [%rd13 + 0];
    ld.local.f64 %fd7, [%rd13 + 8];

    setp.ge.f64 %p3, %fd7, %fd3;
    setp.ne.u32 %p4, %r7, 0xffffffff;
    and.pred %p3, %p3, %p4;
    @%p3 bra L_OPTIX_BVH_LOOP;

    mul.wide.u32 %rd14, %r10, 64;
    add.u64 %rd15, %rd5, %rd14;
    ld.global.nc.u32 %r11, [%rd15 + 48];
    ld.global.nc.u32 %r12, [%rd15 + 52];

    setp.eq.u32 %p5, %r11, 0xffffffff;
    @!%p5 bra L_OPTIX_BVH_LEAF;

    add.u32 %r13, %r10, 1;
    mov.f64 %fd8, 0d7FF0000000000000;
    setp.lt.u32 %p6, %r13, %r2;
    @!%p6 bra L_OPTIX_BVH_SKIP_LEFT;

    mul.wide.u32 %rd16, %r13, 64;
    add.u64 %rd17, %rd5, %rd16;
    ld.global.nc.f64 %fd10, [%rd17 + 0];
    ld.global.nc.f64 %fd11, [%rd17 + 8];
    ld.global.nc.f64 %fd12, [%rd17 + 16];
    ld.global.nc.f64 %fd13, [%rd17 + 24];
    ld.global.nc.f64 %fd14, [%rd17 + 32];
    ld.global.nc.f64 %fd15, [%rd17 + 40];
    sub.f64 %fd16, %fd10, %fd0;
    sub.f64 %fd17, %fd0, %fd13;
    max.f64 %fd16, %fd16, %fd17;
    max.f64 %fd16, %fd16, 0d0000000000000000;
    sub.f64 %fd18, %fd11, %fd1;
    sub.f64 %fd19, %fd1, %fd14;
    max.f64 %fd18, %fd18, %fd19;
    max.f64 %fd18, %fd18, 0d0000000000000000;
    sub.f64 %fd20, %fd12, %fd2;
    sub.f64 %fd21, %fd2, %fd15;
    max.f64 %fd20, %fd20, %fd21;
    max.f64 %fd20, %fd20, 0d0000000000000000;
    mul.f64 %fd8, %fd20, %fd20;
    fma.rn.f64 %fd8, %fd18, %fd18, %fd8;
    fma.rn.f64 %fd8, %fd16, %fd16, %fd8;

L_OPTIX_BVH_SKIP_LEFT:
    mov.f64 %fd9, 0d7FF0000000000000;
    setp.lt.u32 %p7, %r12, %r2;
    @!%p7 bra L_OPTIX_BVH_SKIP_RIGHT;

    mul.wide.u32 %rd16, %r12, 64;
    add.u64 %rd17, %rd5, %rd16;
    ld.global.nc.f64 %fd10, [%rd17 + 0];
    ld.global.nc.f64 %fd11, [%rd17 + 8];
    ld.global.nc.f64 %fd12, [%rd17 + 16];
    ld.global.nc.f64 %fd13, [%rd17 + 24];
    ld.global.nc.f64 %fd14, [%rd17 + 32];
    ld.global.nc.f64 %fd15, [%rd17 + 40];
    sub.f64 %fd16, %fd10, %fd0;
    sub.f64 %fd17, %fd0, %fd13;
    max.f64 %fd16, %fd16, %fd17;
    max.f64 %fd16, %fd16, 0d0000000000000000;
    sub.f64 %fd18, %fd11, %fd1;
    sub.f64 %fd19, %fd1, %fd14;
    max.f64 %fd18, %fd18, %fd19;
    max.f64 %fd18, %fd18, 0d0000000000000000;
    sub.f64 %fd20, %fd12, %fd2;
    sub.f64 %fd21, %fd2, %fd15;
    max.f64 %fd20, %fd20, %fd21;
    max.f64 %fd20, %fd20, 0d0000000000000000;
    mul.f64 %fd9, %fd20, %fd20;
    fma.rn.f64 %fd9, %fd18, %fd18, %fd9;
    fma.rn.f64 %fd9, %fd16, %fd16, %fd9;

L_OPTIX_BVH_SKIP_RIGHT:
    setp.lt.f64 %p8, %fd8, %fd9;
    selp.u32 %r14, %r13, %r12, %p8;
    selp.f64 %fd22, %fd8, %fd9, %p8;
    selp.u32 %r15, %r12, %r13, %p8;
    selp.f64 %fd23, %fd9, %fd8, %p8;

    setp.lt.u32 %p9, %r15, %r2;
    setp.lt.f64 %p10, %fd23, %fd3;
    and.pred %p9, %p9, %p10;
    setp.lt.u32 %p12, %r8, 64;
    and.pred %p9, %p9, %p12;
    @!%p9 bra L_OPTIX_BVH_PUSH_NEAR;

    shl.b32 %r16, %r8, 4;
    cvt.u64.u32 %rd18, %r16;
    add.u64 %rd19, %rd11, %rd18;
    st.local.u32 [%rd19 + 0], %r15;
    st.local.u32 [%rd19 + 4], 0;
    st.local.f64 [%rd19 + 8], %fd23;
    add.u32 %r8, %r8, 1;

L_OPTIX_BVH_PUSH_NEAR:
    setp.lt.u32 %p9, %r14, %r2;
    setp.lt.f64 %p10, %fd22, %fd3;
    and.pred %p9, %p9, %p10;
    setp.lt.u32 %p12, %r8, 64;
    and.pred %p9, %p9, %p12;
    @!%p9 bra L_OPTIX_BVH_LOOP;

    shl.b32 %r16, %r8, 4;
    cvt.u64.u32 %rd18, %r16;
    add.u64 %rd19, %rd11, %rd18;
    st.local.u32 [%rd19 + 0], %r14;
    st.local.u32 [%rd19 + 4], 0;
    st.local.f64 [%rd19 + 8], %fd22;
    add.u32 %r8, %r8, 1;
    bra L_OPTIX_BVH_LOOP;

L_OPTIX_BVH_LEAF:
    mul.wide.u32 %rd16, %r11, 72;
    add.u64 %rd17, %rd6, %rd16;
    ld.global.nc.f64 %fd10, [%rd17 + 0];
    ld.global.nc.f64 %fd11, [%rd17 + 8];
    ld.global.nc.f64 %fd12, [%rd17 + 16];
    ld.global.nc.f64 %fd13, [%rd17 + 24];
    ld.global.nc.f64 %fd14, [%rd17 + 32];
    ld.global.nc.f64 %fd15, [%rd17 + 40];
    ld.global.nc.f64 %fd16, [%rd17 + 48];
    ld.global.nc.f64 %fd17, [%rd17 + 56];
    ld.global.nc.f64 %fd18, [%rd17 + 64];

    sub.f64 %fd19, %fd13, %fd10;
    sub.f64 %fd20, %fd14, %fd11;
    sub.f64 %fd21, %fd15, %fd12;
    sub.f64 %fd22, %fd16, %fd10;
    sub.f64 %fd23, %fd17, %fd11;
    sub.f64 %fd24, %fd18, %fd12;
    sub.f64 %fd25, %fd0,  %fd10;
    sub.f64 %fd26, %fd1,  %fd11;
    sub.f64 %fd27, %fd2,  %fd12;

    mul.f64 %fd28, %fd21, %fd27;
    fma.rn.f64 %fd28, %fd20, %fd26, %fd28;
    fma.rn.f64 %fd28, %fd19, %fd25, %fd28;
    mul.f64 %fd29, %fd24, %fd27;
    fma.rn.f64 %fd29, %fd23, %fd26, %fd29;
    fma.rn.f64 %fd29, %fd22, %fd25, %fd29;

    mul.rn.f64 %fd32, %fd21, %fd23;
    neg.f64 %fd35, %fd21;
    fma.rn.f64 %fd35, %fd35, %fd23, %fd32;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd20, %fd24, %fd32;
    add.f64 %fd32, %fd32, %fd35;
    mul.rn.f64 %fd33, %fd19, %fd24;
    neg.f64 %fd35, %fd19;
    fma.rn.f64 %fd35, %fd35, %fd24, %fd33;
    neg.f64 %fd33, %fd33;
    fma.rn.f64 %fd33, %fd21, %fd22, %fd33;
    add.f64 %fd33, %fd33, %fd35;
    mul.rn.f64 %fd34, %fd20, %fd22;
    neg.f64 %fd35, %fd20;
    fma.rn.f64 %fd35, %fd35, %fd22, %fd34;
    neg.f64 %fd34, %fd34;
    fma.rn.f64 %fd34, %fd19, %fd23, %fd34;
    add.f64 %fd34, %fd34, %fd35;
    mul.f64 %fd35, %fd34, %fd34;
    fma.rn.f64 %fd35, %fd33, %fd33, %fd35;
    fma.rn.f64 %fd35, %fd32, %fd32, %fd35;
    mul.f64 %fd36, %fd21, %fd21;
    fma.rn.f64 %fd36, %fd20, %fd20, %fd36;
    fma.rn.f64 %fd36, %fd19, %fd19, %fd36;
    mul.f64 %fd37, %fd24, %fd24;
    fma.rn.f64 %fd37, %fd23, %fd23, %fd37;
    fma.rn.f64 %fd37, %fd22, %fd22, %fd37;
    mul.f64 %fd38, %fd36, %fd37;
    mul.f64 %fd38, %fd38, 0d3CB0000000000000;
    setp.le.f64 %p6, %fd35, %fd38;
    @%p6 bra L_OPTIX_TRI_DEGENERATE;

    setp.le.f64 %p6, %fd28, 0d0000000000000000;
    setp.le.f64 %p7, %fd29, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_OPTIX_TRI_CHECK_B;
    mov.f64 %fd40, %fd10;
    mov.f64 %fd41, %fd11;
    mov.f64 %fd42, %fd12;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_CHECK_B:
    sub.f64 %fd25, %fd0, %fd13;
    sub.f64 %fd26, %fd1, %fd14;
    sub.f64 %fd27, %fd2, %fd15;
    mul.f64 %fd30, %fd21, %fd27;
    fma.rn.f64 %fd30, %fd20, %fd26, %fd30;
    fma.rn.f64 %fd30, %fd19, %fd25, %fd30;
    mul.f64 %fd31, %fd24, %fd27;
    fma.rn.f64 %fd31, %fd23, %fd26, %fd31;
    fma.rn.f64 %fd31, %fd22, %fd25, %fd31;

    setp.ge.f64 %p6, %fd30, 0d0000000000000000;
    setp.le.f64 %p7, %fd31, %fd30;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_OPTIX_TRI_CHECK_AB;
    mov.f64 %fd40, %fd13;
    mov.f64 %fd41, %fd14;
    mov.f64 %fd42, %fd15;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_CHECK_AB:
    mul.f64 %fd32, %fd30, %fd29;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd28, %fd31, %fd32;

    setp.le.f64 %p6, %fd32, 0d0000000000000000;
    setp.ge.f64 %p7, %fd28, 0d0000000000000000;
    setp.le.f64 %p8, %fd30, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_OPTIX_TRI_CHECK_C;
    sub.f64 %fd33, %fd28, %fd30;
    setp.gt.f64 %p12, %fd33, 0d0000000000000000;
    @!%p12 bra L_OPTIX_TRI_CHECK_C;
    div.rn.f64 %fd34, %fd28, %fd33;
    fma.rn.f64 %fd40, %fd34, %fd19, %fd10;
    fma.rn.f64 %fd41, %fd34, %fd20, %fd11;
    fma.rn.f64 %fd42, %fd34, %fd21, %fd12;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_CHECK_C:
    sub.f64 %fd25, %fd0, %fd16;
    sub.f64 %fd26, %fd1, %fd17;
    sub.f64 %fd27, %fd2, %fd18;
    mul.f64 %fd33, %fd21, %fd27;
    fma.rn.f64 %fd33, %fd20, %fd26, %fd33;
    fma.rn.f64 %fd33, %fd19, %fd25, %fd33;
    mul.f64 %fd34, %fd24, %fd27;
    fma.rn.f64 %fd34, %fd23, %fd26, %fd34;
    fma.rn.f64 %fd34, %fd22, %fd25, %fd34;

    setp.ge.f64 %p6, %fd34, 0d0000000000000000;
    setp.le.f64 %p7, %fd33, %fd34;
    and.pred %p6, %p6, %p7;
    @!%p6 bra L_OPTIX_TRI_CHECK_AC;
    mov.f64 %fd40, %fd16;
    mov.f64 %fd41, %fd17;
    mov.f64 %fd42, %fd18;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_CHECK_AC:
    mul.f64 %fd35, %fd28, %fd34;
    neg.f64 %fd35, %fd35;
    fma.rn.f64 %fd35, %fd33, %fd29, %fd35;

    setp.le.f64 %p6, %fd35, 0d0000000000000000;
    setp.ge.f64 %p7, %fd29, 0d0000000000000000;
    setp.le.f64 %p8, %fd34, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_OPTIX_TRI_CHECK_BC;
    sub.f64 %fd36, %fd29, %fd34;
    setp.gt.f64 %p12, %fd36, 0d0000000000000000;
    @!%p12 bra L_OPTIX_TRI_CHECK_BC;
    div.rn.f64 %fd37, %fd29, %fd36;
    fma.rn.f64 %fd40, %fd37, %fd22, %fd10;
    fma.rn.f64 %fd41, %fd37, %fd23, %fd11;
    fma.rn.f64 %fd42, %fd37, %fd24, %fd12;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_CHECK_BC:
    mul.f64 %fd36, %fd33, %fd31;
    neg.f64 %fd36, %fd36;
    fma.rn.f64 %fd36, %fd30, %fd34, %fd36;

    sub.f64 %fd37, %fd31, %fd30;
    sub.f64 %fd38, %fd33, %fd34;
    setp.le.f64 %p6, %fd36, 0d0000000000000000;
    setp.ge.f64 %p7, %fd37, 0d0000000000000000;
    setp.ge.f64 %p8, %fd38, 0d0000000000000000;
    and.pred %p6, %p6, %p7;
    and.pred %p6, %p6, %p8;
    @!%p6 bra L_OPTIX_TRI_FACE;
    add.f64 %fd39, %fd37, %fd38;
    setp.gt.f64 %p12, %fd39, 0d0000000000000000;
    @!%p12 bra L_OPTIX_TRI_FACE;
    div.rn.f64 %fd39, %fd37, %fd39;
    sub.f64 %fd43, %fd16, %fd13;
    sub.f64 %fd44, %fd17, %fd14;
    sub.f64 %fd45, %fd18, %fd15;
    fma.rn.f64 %fd40, %fd39, %fd43, %fd13;
    fma.rn.f64 %fd41, %fd39, %fd44, %fd14;
    fma.rn.f64 %fd42, %fd39, %fd45, %fd15;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_FACE:
    mul.rn.f64 %fd32, %fd21, %fd23;
    neg.f64 %fd35, %fd21;
    fma.rn.f64 %fd35, %fd35, %fd23, %fd32;
    neg.f64 %fd32, %fd32;
    fma.rn.f64 %fd32, %fd20, %fd24, %fd32;
    add.f64 %fd32, %fd32, %fd35;
    mul.rn.f64 %fd33, %fd19, %fd24;
    neg.f64 %fd35, %fd19;
    fma.rn.f64 %fd35, %fd35, %fd24, %fd33;
    neg.f64 %fd33, %fd33;
    fma.rn.f64 %fd33, %fd21, %fd22, %fd33;
    add.f64 %fd33, %fd33, %fd35;
    mul.rn.f64 %fd34, %fd20, %fd22;
    neg.f64 %fd35, %fd20;
    fma.rn.f64 %fd35, %fd35, %fd22, %fd34;
    neg.f64 %fd34, %fd34;
    fma.rn.f64 %fd34, %fd19, %fd23, %fd34;
    add.f64 %fd34, %fd34, %fd35;
    mul.f64 %fd35, %fd34, %fd34;
    fma.rn.f64 %fd35, %fd33, %fd33, %fd35;
    fma.rn.f64 %fd35, %fd32, %fd32, %fd35;

    setp.gt.f64 %p12, %fd35, 0d0000000000000000;
    @!%p12 bra L_OPTIX_TRI_FACE_ZERO;

    sub.f64 %fd36, %fd0, %fd10;
    sub.f64 %fd37, %fd1, %fd11;
    sub.f64 %fd38, %fd2, %fd12;
    mul.f64 %fd39, %fd38, %fd34;
    fma.rn.f64 %fd39, %fd37, %fd33, %fd39;
    fma.rn.f64 %fd39, %fd36, %fd32, %fd39;
    div.rn.f64 %fd39, %fd39, %fd35;
    neg.f64 %fd39, %fd39;
    fma.rn.f64 %fd40, %fd39, %fd32, %fd0;
    fma.rn.f64 %fd41, %fd39, %fd33, %fd1;
    fma.rn.f64 %fd42, %fd39, %fd34, %fd2;
    bra L_OPTIX_TRI_EVAL_DIST;
L_OPTIX_TRI_FACE_ZERO:
    mov.f64 %fd40, %fd10;
    mov.f64 %fd41, %fd11;
    mov.f64 %fd42, %fd12;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_DEGENERATE:
    setp.gt.f64 %p7, %fd36, 0d0000000000000000;
    div.rn.f64 %fd30, %fd28, %fd36;
    max.f64 %fd30, %fd30, 0d0000000000000000;
    min.f64 %fd30, %fd30, 0d3FF0000000000000;
    selp.f64 %fd30, %fd30, 0d0000000000000000, %p7;
    fma.rn.f64 %fd40, %fd30, %fd19, %fd10;
    fma.rn.f64 %fd41, %fd30, %fd20, %fd11;
    fma.rn.f64 %fd42, %fd30, %fd21, %fd12;
    sub.f64 %fd31, %fd0, %fd40;
    sub.f64 %fd32, %fd1, %fd41;
    sub.f64 %fd33, %fd2, %fd42;
    mul.f64 %fd47, %fd33, %fd33;
    fma.rn.f64 %fd47, %fd32, %fd32, %fd47;
    fma.rn.f64 %fd47, %fd31, %fd31, %fd47;

    sub.f64 %fd30, %fd16, %fd13;
    sub.f64 %fd31, %fd17, %fd14;
    sub.f64 %fd32, %fd18, %fd15;
    mul.f64 %fd33, %fd32, %fd32;
    fma.rn.f64 %fd33, %fd31, %fd31, %fd33;
    fma.rn.f64 %fd33, %fd30, %fd30, %fd33;
    sub.f64 %fd34, %fd0, %fd13;
    sub.f64 %fd35, %fd1, %fd14;
    sub.f64 %fd36, %fd2, %fd15;
    mul.f64 %fd37, %fd32, %fd36;
    fma.rn.f64 %fd37, %fd31, %fd35, %fd37;
    fma.rn.f64 %fd37, %fd30, %fd34, %fd37;
    setp.gt.f64 %p7, %fd33, 0d0000000000000000;
    div.rn.f64 %fd38, %fd37, %fd33;
    max.f64 %fd38, %fd38, 0d0000000000000000;
    min.f64 %fd38, %fd38, 0d3FF0000000000000;
    selp.f64 %fd38, %fd38, 0d0000000000000000, %p7;
    fma.rn.f64 %fd34, %fd38, %fd30, %fd13;
    fma.rn.f64 %fd35, %fd38, %fd31, %fd14;
    fma.rn.f64 %fd36, %fd38, %fd32, %fd15;
    sub.f64 %fd37, %fd0, %fd34;
    sub.f64 %fd38, %fd1, %fd35;
    sub.f64 %fd39, %fd2, %fd36;
    mul.f64 %fd43, %fd39, %fd39;
    fma.rn.f64 %fd43, %fd38, %fd38, %fd43;
    fma.rn.f64 %fd43, %fd37, %fd37, %fd43;
    setp.lt.f64 %p8, %fd43, %fd47;
    selp.f64 %fd40, %fd34, %fd40, %p8;
    selp.f64 %fd41, %fd35, %fd41, %p8;
    selp.f64 %fd42, %fd36, %fd42, %p8;
    selp.f64 %fd47, %fd43, %fd47, %p8;

    sub.f64 %fd30, %fd10, %fd16;
    sub.f64 %fd31, %fd11, %fd17;
    sub.f64 %fd32, %fd12, %fd18;
    mul.f64 %fd33, %fd32, %fd32;
    fma.rn.f64 %fd33, %fd31, %fd31, %fd33;
    fma.rn.f64 %fd33, %fd30, %fd30, %fd33;
    sub.f64 %fd34, %fd0, %fd16;
    sub.f64 %fd35, %fd1, %fd17;
    sub.f64 %fd36, %fd2, %fd18;
    mul.f64 %fd37, %fd32, %fd36;
    fma.rn.f64 %fd37, %fd31, %fd35, %fd37;
    fma.rn.f64 %fd37, %fd30, %fd34, %fd37;
    setp.gt.f64 %p7, %fd33, 0d0000000000000000;
    div.rn.f64 %fd38, %fd37, %fd33;
    max.f64 %fd38, %fd38, 0d0000000000000000;
    min.f64 %fd38, %fd38, 0d3FF0000000000000;
    selp.f64 %fd38, %fd38, 0d0000000000000000, %p7;
    fma.rn.f64 %fd34, %fd38, %fd30, %fd16;
    fma.rn.f64 %fd35, %fd38, %fd31, %fd17;
    fma.rn.f64 %fd36, %fd38, %fd32, %fd18;
    sub.f64 %fd37, %fd0, %fd34;
    sub.f64 %fd38, %fd1, %fd35;
    sub.f64 %fd39, %fd2, %fd36;
    mul.f64 %fd43, %fd39, %fd39;
    fma.rn.f64 %fd43, %fd38, %fd38, %fd43;
    fma.rn.f64 %fd43, %fd37, %fd37, %fd43;
    setp.lt.f64 %p8, %fd43, %fd47;
    selp.f64 %fd40, %fd34, %fd40, %p8;
    selp.f64 %fd41, %fd35, %fd41, %p8;
    selp.f64 %fd42, %fd36, %fd42, %p8;
    bra L_OPTIX_TRI_EVAL_DIST;

L_OPTIX_TRI_EVAL_DIST:
    sub.f64 %fd43, %fd0, %fd40;
    sub.f64 %fd44, %fd1, %fd41;
    sub.f64 %fd45, %fd2, %fd42;
    mul.f64 %fd46, %fd45, %fd45;
    fma.rn.f64 %fd46, %fd44, %fd44, %fd46;
    fma.rn.f64 %fd46, %fd43, %fd43, %fd46;

    setp.lt.f64 %p9, %fd46, %fd3;
    @!%p9 bra L_OPTIX_BVH_LOOP;

    mov.f64 %fd3, %fd46;
    mov.f64 %fd4, %fd40;
    mov.f64 %fd5, %fd41;
    mov.f64 %fd6, %fd42;
    mov.u32 %r7, %r11;
    bra L_OPTIX_BVH_LOOP;

L_OPTIX_BVH_STORE:
    mul.wide.u32 %rd16, %r6, 40;
    add.u64 %rd17, %rd8, %rd16;
    cvt.u64.u32 %rd18, %r7;
    mov.b64 %fd47, %rd18;
    st.global.f64 [%rd17 + 0],  %fd4;
    st.global.f64 [%rd17 + 8],  %fd5;
    st.global.f64 [%rd17 + 16], %fd6;
    st.global.f64 [%rd17 + 24], %fd3;
    st.global.f64 [%rd17 + 32], %fd47;

L_OPTIX_BVH_EXIT:
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

.visible .entry __raygen__aabb_gather()
{
    .reg .u32 %idx, %prim, %vis, %flags, %sbt_off, %sbt_stride, %miss_idx;
    .reg .u64 %handle, %pts_base, %out_prim_base, %off32, %off64, %pt_ptr, %row_ptr;
    .reg .f64 %ox64, %oy64, %oz64;
    .reg .f32 %ox, %oy, %oz, %dx, %dy, %dz, %tmin, %tmax, %rtime;

    call (%idx), _optix_get_launch_index_x, ();
    ld.const.u64 %handle, [params + 0];
    ld.const.u64 %pts_base, [params + 8];
    ld.const.u64 %out_prim_base, [params + 16];

    mul.wide.u32 %off32, %idx, 32;
    add.u64 %pt_ptr, %pts_base, %off32;
    mul.wide.u32 %off64, %idx, 64;
    add.u64 %row_ptr, %out_prim_base, %off64;
    st.global.u32 [%row_ptr + 0], 0;

    ld.global.f64 %ox64, [%pt_ptr + 0];
    ld.global.f64 %oy64, [%pt_ptr + 8];
    ld.global.f64 %oz64, [%pt_ptr + 16];

    cvt.rn.f32.f64 %ox, %ox64;
    cvt.rn.f32.f64 %oy, %oy64;
    cvt.rn.f32.f64 %oz, %oz64;

    mov.f32 %dx, 0f3F800000;
    mov.f32 %dy, 0f00000000;
    mov.f32 %dz, 0f00000000;
    mov.f32 %tmin, 0f00000000;
    mov.f32 %tmax, 0f33D6BF95;
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

.visible .entry __intersection__aabb_gather()
{
    .reg .pred %p0, %p1;
    .reg .u32 %idx, %prim, %cnt, %i, %existing, %cnt_next;
    .reg .u64 %out_prim_base, %off64, %row_ptr, %off_slot, %slot_ptr;

    call (%idx), _optix_get_launch_index_x, ();
    call (%prim), _optix_read_primitive_idx, ();
    ld.const.u64 %out_prim_base, [params + 16];

    mul.wide.u32 %off64, %idx, 64;
    add.u64 %row_ptr, %out_prim_base, %off64;
    ld.global.u32 %cnt, [%row_ptr + 0];

    mov.u32 %i, 0;
L_IS_DEDUP:
    setp.ge.u32 %p0, %i, %cnt;
    @%p0 bra L_IS_INSERT;
    setp.ge.u32 %p0, %i, 15;
    @%p0 bra L_IS_INSERT;
    mul.wide.u32 %off_slot, %i, 4;
    add.u64 %slot_ptr, %row_ptr, %off_slot;
    ld.global.u32 %existing, [%slot_ptr + 4];
    setp.eq.u32 %p1, %existing, %prim;
    @%p1 bra L_IS_DONE;
    add.u32 %i, %i, 1;
    bra L_IS_DEDUP;

L_IS_INSERT:
    setp.ge.u32 %p0, %cnt, 15;
    @%p0 bra L_IS_COUNT_ONLY;
    mul.wide.u32 %off_slot, %cnt, 4;
    add.u64 %slot_ptr, %row_ptr, %off_slot;
    st.global.u32 [%slot_ptr + 4], %prim;

L_IS_COUNT_ONLY:
    add.u32 %cnt_next, %cnt, 1;
    st.global.u32 [%row_ptr + 0], %cnt_next;

L_IS_DONE:
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

struct CachedOptixCloud {
    hash: u64,
    n_pts: u32,
    r_pad_sq: f64,
    blas_aabb: OptixAccel,
    d_pts: CudaSlice<f64>,
}

struct OptixPipelines {
    ctx: Arc<OptixContext>,
    ray_pipeline: Arc<OptixPipeline>,
    ray_sbt: OptixSbt,
    parity_pipeline: Arc<OptixPipeline>,
    parity_sbt: OptixSbt,
    aabb_gather_pipeline: Arc<OptixPipeline>,
    aabb_gather_sbt: OptixSbt,
    fn_build_point_aabbs: CudaFunction,
    fn_optix_ray_mesh: CudaFunction,
    fn_optix_point_in_solid: CudaFunction,
    fn_optix_kdtree_query: CudaFunction,
    fn_optix_bvh_closest_point: CudaFunction,
    mesh_cache: Mutex<Option<CachedOptixMesh>>,
    cloud_cache: Mutex<[Option<CachedOptixCloud>; 2]>,
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
        let rg_aabb =
            OptixProgramGroup::new_raygen(&optix_ctx, &optix_module, "__raygen__aabb_gather")
                .ok()?;
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
        let hg_aabb = OptixProgramGroup::new_hitgroup(
            &optix_ctx,
            None,
            None,
            Some((&optix_module, "__intersection__aabb_gather")),
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

        let aabb_groups = [rg_aabb.clone(), ms.clone(), hg_aabb.clone()];
        let aabb_gather_pipeline =
            OptixPipeline::new(&optix_ctx, &pipeline_opts, &link_opts, &aabb_groups, 1).ok()?;
        let aabb_gather_sbt = OptixSbt::from_groups(&stream, &rg_aabb, &[&ms], &[&hg_aabb]).ok()?;

        Some(Self {
            ctx: optix_ctx,
            ray_pipeline,
            ray_sbt,
            parity_pipeline,
            parity_sbt,
            aabb_gather_pipeline,
            aabb_gather_sbt,
            fn_build_point_aabbs: cuda_module
                .load_function("cuda_build_point_aabbs_f64")
                .ok()?,
            fn_optix_ray_mesh: cuda_module.load_function("cuda_optix_ray_mesh_f64").ok()?,
            fn_optix_point_in_solid: cuda_module
                .load_function("cuda_optix_point_in_solid_f64")
                .ok()?,
            fn_optix_kdtree_query: cuda_module
                .load_function("cuda_optix_kdtree_query_f64")
                .ok()?,
            fn_optix_bvh_closest_point: cuda_module
                .load_function("cuda_optix_bvh_closest_point_f64")
                .ok()?,
            mesh_cache: Mutex::new(None),
            cloud_cache: Mutex::new([None, None]),
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

struct CachedCudaKdCloud {
    hash: u64,
    n_nodes: u32,
    d_nodes: CudaSlice<f64>,
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
    fn_simt_kdtree_query: CudaFunction,
    fn_simt_bvh_closest_point: CudaFunction,
    fn_sm86_kdtree_query: CudaFunction,
    fn_sm86_bvh_closest_point: CudaFunction,
    compute_mesh_cache: Mutex<Option<CachedCudaMesh>>,
    compute_kd_cache: Mutex<[Option<CachedCudaKdCloud>; 2]>,
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
            fn_simt_kdtree_query: module.load_function("cuda_simt_kdtree_query_f64").ok()?,
            fn_simt_bvh_closest_point: module
                .load_function("cuda_simt_bvh_closest_point_f64")
                .ok()?,
            fn_sm86_kdtree_query: module.load_function("cuda_sm86_kdtree_query_f64").ok()?,
            fn_sm86_bvh_closest_point: module
                .load_function("cuda_sm86_bvh_closest_point_f64")
                .ok()?,
            compute_mesh_cache: Mutex::new(None),
            compute_kd_cache: Mutex::new([None, None]),
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
        f(guard.as_ref()?)
    }

    fn serialize_flat_kdtree(flat_tree: &crate::spatial::FlatKdTree3) -> Vec<f64> {
        flat_tree
            .nodes()
            .iter()
            .flat_map(|n| {
                let meta = (u64::from(n.right_child) << 32) | u64::from(n.split_axis);
                [n.point.x, n.point.y, n.point.z, f64::from_bits(meta)]
            })
            .collect()
    }

    fn with_cached_cuda_kd_pair<R>(
        &self,
        stream: &Arc<CudaStream>,
        points_a: &[Point3],
        points_b: &[Point3],
        f: impl FnOnce(&CachedCudaKdCloud, &CachedCudaKdCloud) -> Option<R>,
    ) -> Option<R> {
        let hash_a = Self::hash_point_cloud(points_a, 0);
        let hash_b = Self::hash_point_cloud(points_b, 0);
        let mut guard = self.compute_kd_cache.lock().ok()?;
        let rebuild_a = match guard[0].as_ref() {
            Some(c) => c.hash != hash_a || usize::try_from(c.n_nodes).ok()? != points_a.len(),
            None => true,
        };
        let rebuild_b = match guard[1].as_ref() {
            Some(c) => c.hash != hash_b || usize::try_from(c.n_nodes).ok()? != points_b.len(),
            None => true,
        };
        match (rebuild_a, rebuild_b) {
            (true, true) => {
                let (flat_a, flat_b) = rayon::join(
                    || {
                        let tree = crate::spatial::KdTree3::build(points_a).flatten();
                        Self::serialize_flat_kdtree(&tree)
                    },
                    || {
                        let tree = crate::spatial::KdTree3::build(points_b).flatten();
                        Self::serialize_flat_kdtree(&tree)
                    },
                );
                let n_a = u32::try_from(points_a.len()).ok()?;
                let n_b = u32::try_from(points_b.len()).ok()?;
                let d_a = stream.clone_htod(&flat_a).ok()?;
                let d_b = stream.clone_htod(&flat_b).ok()?;
                guard[0] = Some(CachedCudaKdCloud {
                    hash: hash_a,
                    n_nodes: n_a,
                    d_nodes: d_a,
                });
                guard[1] = Some(CachedCudaKdCloud {
                    hash: hash_b,
                    n_nodes: n_b,
                    d_nodes: d_b,
                });
            }
            (true, false) => {
                let tree = crate::spatial::KdTree3::build(points_a).flatten();
                let flat_a = Self::serialize_flat_kdtree(&tree);
                let n_a = u32::try_from(points_a.len()).ok()?;
                let d_a = stream.clone_htod(&flat_a).ok()?;
                guard[0] = Some(CachedCudaKdCloud {
                    hash: hash_a,
                    n_nodes: n_a,
                    d_nodes: d_a,
                });
            }
            (false, true) => {
                let tree = crate::spatial::KdTree3::build(points_b).flatten();
                let flat_b = Self::serialize_flat_kdtree(&tree);
                let n_b = u32::try_from(points_b.len()).ok()?;
                let d_b = stream.clone_htod(&flat_b).ok()?;
                guard[1] = Some(CachedCudaKdCloud {
                    hash: hash_b,
                    n_nodes: n_b,
                    d_nodes: d_b,
                });
            }
            (false, false) => {}
        }
        f(guard[0].as_ref()?, guard[1].as_ref()?)
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

    fn dispatch_kdtree_nearest_sq_cached(
        &self,
        stream: &Arc<CudaStream>,
        target: &CachedCudaKdCloud,
        queries: &CachedCudaKdCloud,
    ) -> Option<Vec<f64>> {
        let q_len = usize::try_from(queries.n_nodes).ok()?;
        let mut d_out = stream.alloc_zeros::<f64>(q_len).ok()?;

        let block = 128_u32;
        let grid = queries.n_nodes.div_ceil(block);
        let cfg = LaunchConfig {
            grid_dim: (grid, 1, 1),
            block_dim: (block, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(&self.fn_simt_kdtree_query);
        builder.arg(&target.d_nodes);
        builder.arg(&queries.d_nodes);
        builder.arg(&mut d_out);
        builder.arg(&queries.n_nodes);
        builder.arg(&target.n_nodes);
        // Safety: kernel signature matches (u64, u64, u64, u32, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        stream.clone_dtoh(&d_out).ok()
    }

    /// Executes symmetric Hausdorff distance via CUDA SIMT stack-based KD-tree traversal in $\mathcal{O}(N \log M)$ time.
    #[must_use]
    pub fn simt_spatial_hausdorff(&self, points_a: &[Point3], points_b: &[Point3]) -> Option<f64> {
        if points_a.is_empty() || points_b.is_empty() {
            return Some(0.0);
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        self.with_cached_cuda_kd_pair(&stream, points_a, points_b, |cloud_a, cloud_b| {
            let dists_ab = self.dispatch_kdtree_nearest_sq_cached(&stream, cloud_b, cloud_a)?;
            let dists_ba = self.dispatch_kdtree_nearest_sq_cached(&stream, cloud_a, cloud_b)?;
            let mut max_sq = 0.0_f64;
            for &d in dists_ab.iter().chain(&dists_ba) {
                if d > max_sq {
                    max_sq = d;
                }
            }
            Some(max_sq.sqrt())
        })
    }

    fn dispatch_bvh_closest_point_kernel(
        &self,
        func: &CudaFunction,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<ClosestPointResult>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some(cpu::closest_point_mesh(points, mesh));
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_queries = u32::try_from(points.len()).ok()?;
        let flat_queries: Vec<f64> = points.iter().flat_map(|q| [q.x, q.y, q.z, 0.0]).collect();

        let d_queries = stream.clone_htod(&flat_queries).ok()?;
        let out_f64_len = points.len().checked_mul(5)?;
        let mut d_out = stream.alloc_zeros::<f64>(out_f64_len).ok()?;

        self.with_cached_cuda_mesh(&stream, mesh, |cached| {
            let block = 128_u32;
            let grid = n_queries.div_ceil(block);
            let cfg = LaunchConfig {
                grid_dim: (grid, 1, 1),
                block_dim: (block, 1, 1),
                shared_mem_bytes: 0,
            };
            let mut builder = stream.launch_builder(func);
            builder.arg(&cached.d_bvh);
            builder.arg(&cached.d_tris_verts);
            builder.arg(&d_queries);
            builder.arg(&mut d_out);
            builder.arg(&n_queries);
            builder.arg(&cached.n_nodes);
            // Safety: kernel signature matches (u64, u64, u64, u64, u32, u32) and buffer bounds are verified.
            unsafe { builder.launch(cfg) }.ok()?;
            Some(())
        })?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(
            raw_out
                .chunks_exact(5)
                .map(|r| {
                    let Ok(tri_idx) = u32::try_from(r[4].to_bits()) else {
                        return ClosestPointResult {
                            closest_point: Point3::new(r[0], r[1], r[2]),
                            distance_sq: r[3],
                            triangle_index: u32::MAX,
                        };
                    };
                    ClosestPointResult {
                        closest_point: Point3::new(r[0], r[1], r[2]),
                        distance_sq: r[3],
                        triangle_index: tri_idx,
                    }
                })
                .collect(),
        )
    }

    /// Executes closest-point-on-mesh queries via CUDA SIMT stack-based BVH traversal in $\mathcal{O}(N \log T)$ time.
    #[must_use]
    pub fn simt_spatial_closest_point(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<ClosestPointResult>> {
        self.dispatch_bvh_closest_point_kernel(&self.fn_simt_bvh_closest_point, points, mesh)
    }

    fn dispatch_sm86_kdtree_warp_max_sq_cached(
        &self,
        stream: &Arc<CudaStream>,
        target: &CachedCudaKdCloud,
        queries: &CachedCudaKdCloud,
    ) -> Option<f64> {
        let block = 128_u32;
        let grid = queries.n_nodes.div_ceil(block);
        let warps_per_block = block / 32;
        let total_warps = usize::try_from(grid.checked_mul(warps_per_block)?).ok()?;
        let mut d_out = stream.alloc_zeros::<f64>(total_warps).ok()?;

        let cfg = LaunchConfig {
            grid_dim: (grid, 1, 1),
            block_dim: (block, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(&self.fn_sm86_kdtree_query);
        builder.arg(&target.d_nodes);
        builder.arg(&queries.d_nodes);
        builder.arg(&mut d_out);
        builder.arg(&queries.n_nodes);
        builder.arg(&target.n_nodes);
        // Safety: kernel signature matches (u64, u64, u64, u32, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        let warp_maxima = stream.clone_dtoh(&d_out).ok()?;
        let mut max_sq = 0.0_f64;
        for &w in &warp_maxima {
            if w > max_sq {
                max_sq = w;
            }
        }
        Some(max_sq)
    }

    /// Executes symmetric Hausdorff distance via Ampere `sm_86` PTX with non-coherent loads and warp-butterfly reduction.
    #[must_use]
    pub fn ptx_sm86_spatial_hausdorff(
        &self,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<f64> {
        if !self.supports_sm86() {
            return None;
        }
        if points_a.is_empty() || points_b.is_empty() {
            return Some(0.0);
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        self.with_cached_cuda_kd_pair(&stream, points_a, points_b, |cloud_a, cloud_b| {
            let max_ab_sq =
                self.dispatch_sm86_kdtree_warp_max_sq_cached(&stream, cloud_b, cloud_a)?;
            let max_ba_sq =
                self.dispatch_sm86_kdtree_warp_max_sq_cached(&stream, cloud_a, cloud_b)?;
            Some(max_ab_sq.max(max_ba_sq).sqrt())
        })
    }

    /// Executes closest-point-on-mesh queries via Ampere `sm_86` PTX BVH traversal with non-coherent read-only cache loads.
    #[must_use]
    pub fn ptx_sm86_spatial_closest_point(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<ClosestPointResult>> {
        if !self.supports_sm86() {
            return None;
        }
        self.dispatch_bvh_closest_point_kernel(&self.fn_sm86_bvh_closest_point, points, mesh)
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

    fn hash_point_cloud(points: &[Point3], r_pad_bits: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        points.len().hash(&mut hasher);
        r_pad_bits.hash(&mut hasher);
        let step = (points.len() / 256).max(1);
        for p in points.iter().step_by(step) {
            p.x.to_bits().hash(&mut hasher);
            p.y.to_bits().hash(&mut hasher);
            p.z.to_bits().hash(&mut hasher);
        }
        if let Some(last_p) = points.last() {
            last_p.x.to_bits().hash(&mut hasher);
            last_p.y.to_bits().hash(&mut hasher);
            last_p.z.to_bits().hash(&mut hasher);
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
        f(guard.as_ref()?)
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

    /// Executes closest-point-on-mesh queries via `cudarc::optix` hardware RT Core custom-AABB candidate gathering and exact FP64 distance certification.
    #[must_use]
    pub fn optix_rt_closest_point_mesh(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<ClosestPointResult>> {
        let optix = self.optix.as_ref()?;
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some(cpu::closest_point_mesh(points, mesh));
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();
        let n_queries = u32::try_from(points.len()).ok()?;

        let flat_queries: Vec<f64> = points.iter().flat_map(|q| [q.x, q.y, q.z, 0.0]).collect();
        let d_queries = stream.clone_htod(&flat_queries).ok()?;
        let d_cand_prim16 = stream
            .alloc_zeros::<u32>(points.len().checked_mul(16)?)
            .ok()?;
        let out_f64_len = points.len().checked_mul(5)?;
        let mut d_out = stream.alloc_zeros::<f64>(out_f64_len).ok()?;

        Self::with_cached_optix_mesh(optix, &stream, mesh, |cached| {
            let params_raw = {
                let (q_ptr, _s1) = d_queries.device_ptr(&stream);
                let (cand_ptr, _s2) = d_cand_prim16.device_ptr(&stream);
                [cached.blas_aabb.handle(), q_ptr, cand_ptr, 0_u64]
            };
            let d_params = stream.clone_htod(&params_raw).ok()?;
            optix
                .aabb_gather_pipeline
                .launch(&stream, &d_params, &optix.aabb_gather_sbt, n_queries, 1, 1)
                .ok()?;

            let block = 128_u32;
            let grid = n_queries.div_ceil(block);
            let cfg = LaunchConfig {
                grid_dim: (grid, 1, 1),
                block_dim: (block, 1, 1),
                shared_mem_bytes: 0,
            };
            let mut builder = stream.launch_builder(&optix.fn_optix_bvh_closest_point);
            builder.arg(&cached.d_bvh);
            builder.arg(&cached.d_tris_verts);
            builder.arg(&d_queries);
            builder.arg(&d_cand_prim16);
            builder.arg(&mut d_out);
            builder.arg(&cached.r_pad_sq);
            builder.arg(&n_queries);
            builder.arg(&cached.n_nodes);
            builder.arg(&cached.n_tris);
            // Safety: kernel signature matches (u64, u64, u64, u64, u64, f64, u32, u32, u32) and buffer bounds are verified.
            unsafe { builder.launch(cfg) }.ok()?;
            Some(())
        })?;

        let raw_out = stream.clone_dtoh(&d_out).ok()?;
        Some(
            raw_out
                .chunks_exact(5)
                .map(|r| {
                    let Ok(tri_idx) = u32::try_from(r[4].to_bits()) else {
                        return ClosestPointResult {
                            closest_point: Point3::new(r[0], r[1], r[2]),
                            distance_sq: r[3],
                            triangle_index: u32::MAX,
                        };
                    };
                    ClosestPointResult {
                        closest_point: Point3::new(r[0], r[1], r[2]),
                        distance_sq: r[3],
                        triangle_index: tri_idx,
                    }
                })
                .collect(),
        )
    }

    fn estimate_cloud_r_pad(points_a: &[Point3], points_b: &[Point3]) -> f64 {
        let min_len = points_a.len().min(points_b.len());
        let step_corresp = (min_len / 32).max(1);
        let mut max_corresp_sq = 0.0_f64;
        for idx in (0..min_len).step_by(step_corresp) {
            let pa = points_a[idx];
            let pb = points_b[idx];
            let dx = pa.x - pb.x;
            let dy = pa.y - pb.y;
            let dz = pa.z - pb.z;
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2.is_finite() && d2 > max_corresp_sq {
                max_corresp_sq = d2;
            }
        }
        let mut sum_adj_sq = 0.0_f64;
        let mut adj_cnt = 0_u32;
        if points_a.len() > 1 {
            let step_adj = ((points_a.len() - 1) / 32).max(1);
            for idx in (0..(points_a.len() - 1)).step_by(step_adj) {
                let p0 = points_a[idx];
                let p1 = points_a[idx + 1];
                let dx = p1.x - p0.x;
                let dy = p1.y - p0.y;
                let dz = p1.z - p0.z;
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2.is_finite() {
                    sum_adj_sq += d2;
                    adj_cnt += 1;
                }
            }
        }
        let avg_adj = if adj_cnt > 0 {
            (sum_adj_sq / f64::from(adj_cnt)).sqrt()
        } else {
            0.1
        };
        let corresp = max_corresp_sq.sqrt();
        if corresp > 0.0 && corresp <= avg_adj * 8.0 {
            (corresp * 1.15).max(1.0e-4)
        } else {
            (avg_adj * 1.5).max(corresp.min(1.0)).max(1.0e-4)
        }
    }

    fn build_cached_optix_cloud(
        optix: &OptixPipelines,
        stream: &Arc<CudaStream>,
        flat_pts: &[f64],
        n_pts: u32,
        r_pad: f64,
        hash: u64,
    ) -> Option<CachedOptixCloud> {
        let len = usize::try_from(n_pts).ok()?;
        let d_pts = stream.clone_htod(flat_pts).ok()?;
        let mut d_aabbs = stream.alloc_zeros::<optix_sys::OptixAabb>(len).ok()?;

        let r_pad_aabb = f64_to_blas_f32(r_pad * 1.002 + 1.0e-5);
        let block = 128_u32;
        let grid = n_pts.div_ceil(block);
        let cfg = LaunchConfig {
            grid_dim: (grid, 1, 1),
            block_dim: (block, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(&optix.fn_build_point_aabbs);
        builder.arg(&d_pts);
        builder.arg(&mut d_aabbs);
        builder.arg(&r_pad_aabb);
        builder.arg(&n_pts);
        // Safety: kernel signature matches (u64, u64, f32, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        let blas_aabb = OptixAccel::build_aabbs(
            &optix.ctx,
            stream,
            &d_aabbs,
            OPTIX_GEOMETRY_FLAG_DISABLE_ANYHIT_U32,
            None,
        )
        .ok()?;

        Some(CachedOptixCloud {
            hash,
            n_pts,
            r_pad_sq: r_pad * r_pad,
            blas_aabb,
            d_pts,
        })
    }

    fn dispatch_optix_cloud_directed_max_sq(
        optix: &OptixPipelines,
        stream: &Arc<CudaStream>,
        target: &CachedOptixCloud,
        d_queries: &CudaSlice<f64>,
        n_queries: u32,
    ) -> Option<f64> {
        let q_len = usize::try_from(n_queries).ok()?;
        let d_cand_prim16 = stream.alloc_zeros::<u32>(q_len.checked_mul(16)?).ok()?;
        let params_raw = {
            let (q_ptr, _s1) = d_queries.device_ptr(stream);
            let (cand_ptr, _s2) = d_cand_prim16.device_ptr(stream);
            [target.blas_aabb.handle(), q_ptr, cand_ptr, 0_u64]
        };
        let d_params = stream.clone_htod(&params_raw).ok()?;
        optix
            .aabb_gather_pipeline
            .launch(stream, &d_params, &optix.aabb_gather_sbt, n_queries, 1, 1)
            .ok()?;

        let block = 128_u32;
        let grid = n_queries.div_ceil(block);
        let warps_per_block = block / 32;
        let total_warps = usize::try_from(grid.checked_mul(warps_per_block)?).ok()?;
        let mut d_out = stream.alloc_zeros::<f64>(total_warps).ok()?;

        let cfg = LaunchConfig {
            grid_dim: (grid, 1, 1),
            block_dim: (block, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut builder = stream.launch_builder(&optix.fn_optix_kdtree_query);
        builder.arg(&target.d_pts);
        builder.arg(d_queries);
        builder.arg(&d_cand_prim16);
        builder.arg(&mut d_out);
        builder.arg(&target.r_pad_sq);
        builder.arg(&n_queries);
        builder.arg(&target.n_pts);
        // Safety: kernel signature matches (u64, u64, u64, u64, f64, u32, u32) and buffer bounds are verified.
        unsafe { builder.launch(cfg) }.ok()?;

        let warp_maxima = stream.clone_dtoh(&d_out).ok()?;
        let mut max_sq = 0.0_f64;
        for &w in &warp_maxima {
            if w > max_sq {
                max_sq = w;
            }
        }
        Some(max_sq)
    }

    /// Executes symmetric Hausdorff distance via `cudarc::optix` hardware RT Core custom-AABB spatial acceleration and warp-butterfly FP64 reduction.
    #[must_use]
    pub fn optix_rt_spatial_hausdorff(
        &self,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<f64> {
        let optix = self.optix.as_ref()?;
        if points_a.is_empty() || points_b.is_empty() {
            return Some(0.0);
        }
        self.ctx.bind_to_thread().ok()?;
        let stream = self.ctx.default_stream();

        let r_pad = Self::estimate_cloud_r_pad(points_a, points_b);
        let r_pad_bits = r_pad.to_bits();
        let hash_a = Self::hash_point_cloud(points_a, r_pad_bits);
        let hash_b = Self::hash_point_cloud(points_b, r_pad_bits);

        let mut guard = optix.cloud_cache.lock().ok()?;
        let rebuild_a = match guard[0].as_ref() {
            Some(c) => c.hash != hash_a || usize::try_from(c.n_pts).ok()? != points_a.len(),
            None => true,
        };
        let rebuild_b = match guard[1].as_ref() {
            Some(c) => c.hash != hash_b || usize::try_from(c.n_pts).ok()? != points_b.len(),
            None => true,
        };
        match (rebuild_a, rebuild_b) {
            (true, true) => {
                let (flat_a, flat_b) = rayon::join(
                    || {
                        let tree = crate::spatial::KdTree3::build(points_a).flatten();
                        Self::serialize_flat_kdtree(&tree)
                    },
                    || {
                        let tree = crate::spatial::KdTree3::build(points_b).flatten();
                        Self::serialize_flat_kdtree(&tree)
                    },
                );
                let n_a = u32::try_from(points_a.len()).ok()?;
                let n_b = u32::try_from(points_b.len()).ok()?;
                guard[0] = Some(Self::build_cached_optix_cloud(
                    optix, &stream, &flat_a, n_a, r_pad, hash_a,
                )?);
                guard[1] = Some(Self::build_cached_optix_cloud(
                    optix, &stream, &flat_b, n_b, r_pad, hash_b,
                )?);
            }
            (true, false) => {
                let tree = crate::spatial::KdTree3::build(points_a).flatten();
                let flat_a = Self::serialize_flat_kdtree(&tree);
                let n_a = u32::try_from(points_a.len()).ok()?;
                guard[0] = Some(Self::build_cached_optix_cloud(
                    optix, &stream, &flat_a, n_a, r_pad, hash_a,
                )?);
            }
            (false, true) => {
                let tree = crate::spatial::KdTree3::build(points_b).flatten();
                let flat_b = Self::serialize_flat_kdtree(&tree);
                let n_b = u32::try_from(points_b.len()).ok()?;
                guard[1] = Some(Self::build_cached_optix_cloud(
                    optix, &stream, &flat_b, n_b, r_pad, hash_b,
                )?);
            }
            (false, false) => {}
        }
        let cloud_a = guard[0].as_ref()?;
        let cloud_b = guard[1].as_ref()?;

        let max_ab_sq = Self::dispatch_optix_cloud_directed_max_sq(
            optix,
            &stream,
            cloud_b,
            &cloud_a.d_pts,
            cloud_a.n_pts,
        )?;
        let max_ba_sq = Self::dispatch_optix_cloud_directed_max_sq(
            optix,
            &stream,
            cloud_a,
            &cloud_b.d_pts,
            cloud_b.n_pts,
        )?;
        Some(max_ab_sq.max(max_ba_sq).sqrt())
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
    fn cuda_simt_spatial_closest_point_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let pts_a: Vec<Point3> = (0_u32..64)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 2.0, (f * 0.3).sin(), (f * 0.2).cos())
            })
            .collect();
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
        let expected_cp = cpu::closest_point_mesh(&pts_a, mesh);
        let spatial_cp = crate::spatial::closest_points_on_mesh(&pts_a, mesh);
        let actual_cp = ctx
            .simt_spatial_closest_point(&pts_a, mesh)
            .expect("CUDA SIMT spatial closest point");
        for ((e, s), a) in expected_cp.iter().zip(&spatial_cp).zip(&actual_cp) {
            assert!((e.distance_sq - a.distance_sq).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.x - a.closest_point.x).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.y - a.closest_point.y).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.z - a.closest_point.z).abs() < CUDA_PARITY_TOLERANCE);
            assert_eq!(s.triangle_index, a.triangle_index);
        }
    }

    #[test]
    fn cuda_sm86_spatial_closest_point_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        if !ctx.supports_sm86() {
            return;
        }
        let pts_a: Vec<Point3> = (0_u32..150)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.11 - 3.0, (f * 0.25).sin(), (f * 0.17).cos())
            })
            .collect();
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
        let expected_cp = cpu::closest_point_mesh(&pts_a, mesh);
        let spatial_cp = crate::spatial::closest_points_on_mesh(&pts_a, mesh);
        let actual_cp = ctx
            .ptx_sm86_spatial_closest_point(&pts_a, mesh)
            .expect("PTX sm_86 spatial closest point");
        for ((e, s), a) in expected_cp.iter().zip(&spatial_cp).zip(&actual_cp) {
            assert!((e.distance_sq - a.distance_sq).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.x - a.closest_point.x).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.y - a.closest_point.y).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.z - a.closest_point.z).abs() < CUDA_PARITY_TOLERANCE);
            assert_eq!(s.triangle_index, a.triangle_index);
        }
    }

    #[test]
    fn cuda_simt_spatial_hausdorff_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let pts_a: Vec<Point3> = (0_u32..64)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 2.0, (f * 0.3).sin(), (f * 0.2).cos())
            })
            .collect();
        let pts_b: Vec<Point3> = (0_u32..53)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 1.92, (f * 0.3).sin() - 0.04, (f * 0.2).cos())
            })
            .collect();
        let expected_h = crate::spatial::kd_hausdorff_distance(&pts_a, &pts_b);
        let actual_h = ctx
            .simt_spatial_hausdorff(&pts_a, &pts_b)
            .expect("CUDA SIMT spatial Hausdorff");
        assert!((expected_h - actual_h).abs() < CUDA_PARITY_TOLERANCE);
    }

    #[test]
    fn cuda_sm86_spatial_hausdorff_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        if !ctx.supports_sm86() {
            return;
        }
        let pts_a: Vec<Point3> = (0_u32..150)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.11 - 3.0, (f * 0.25).sin(), (f * 0.17).cos())
            })
            .collect();
        let pts_b: Vec<Point3> = (0_u32..113)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.11 - 2.91, (f * 0.25).sin() + 0.05, (f * 0.17).cos())
            })
            .collect();
        let expected_h = crate::spatial::kd_hausdorff_distance(&pts_a, &pts_b);
        let actual_h = ctx
            .ptx_sm86_spatial_hausdorff(&pts_a, &pts_b)
            .expect("PTX sm_86 spatial Hausdorff");
        assert!((expected_h - actual_h).abs() < CUDA_PARITY_TOLERANCE);
    }

    #[test]
    fn cuda_degenerate_coincident_triangle_closest_point() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(6.0, 0.0, 0.0),
            Point3::new(5.0, 1.0, 0.0),
        ];
        let triangles = [[0_u32, 0, 1], [2, 3, 4]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let query = [Point3::new(0.5, 0.5, 0.0)];

        if let Some(simt_cp) = ctx.simt_spatial_closest_point(&query, mesh) {
            assert_eq!(simt_cp.len(), 1);
            assert_eq!(simt_cp[0].triangle_index, 0);
            assert!((simt_cp[0].distance_sq - 0.25).abs() < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.x - 0.5).abs() < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.y - 0.0).abs() < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.z - 0.0).abs() < CUDA_PARITY_TOLERANCE);
        }

        if ctx.supports_sm86() {
            if let Some(sm86_cp) = ctx.ptx_sm86_spatial_closest_point(&query, mesh) {
                assert_eq!(sm86_cp.len(), 1);
                assert_eq!(sm86_cp[0].triangle_index, 0);
                assert!((sm86_cp[0].distance_sq - 0.25).abs() < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.x - 0.5).abs() < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.y - 0.0).abs() < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.z - 0.0).abs() < CUDA_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn cuda_degenerate_zero_area_and_collinear_triangle_closest_point() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ];
        let triangles_zero_area = [[0_u32, 1, 1]];
        let triangles_collinear = [[0_u32, 1, 2]];
        let query = [Point3::new(0.1, 0.5, 0.0)];

        for (triangles, label) in [
            (&triangles_zero_area, "zero-area"),
            (&triangles_collinear, "collinear"),
        ] {
            let mesh = TriangleMeshView {
                vertices: &vertices,
                triangles: triangles.as_slice(),
            };
            if let Some(simt_cp) = ctx.simt_spatial_closest_point(&query, mesh) {
                assert_eq!(simt_cp.len(), 1, "SIMT length mismatch for {label}");
                assert!(
                    (simt_cp[0].distance_sq - 0.25).abs() < CUDA_PARITY_TOLERANCE,
                    "SIMT distance_sq mismatch for {label}"
                );
                assert!(
                    (simt_cp[0].closest_point.x - 0.1).abs() < CUDA_PARITY_TOLERANCE,
                    "SIMT closest_point.x mismatch for {label}"
                );
                assert!(
                    simt_cp[0].closest_point.y.abs() < CUDA_PARITY_TOLERANCE,
                    "SIMT closest_point.y mismatch for {label}"
                );
                assert!(
                    simt_cp[0].closest_point.z.abs() < CUDA_PARITY_TOLERANCE,
                    "SIMT closest_point.z mismatch for {label}"
                );
            }
            if ctx.supports_sm86() {
                if let Some(sm86_cp) = ctx.ptx_sm86_spatial_closest_point(&query, mesh) {
                    assert_eq!(sm86_cp.len(), 1, "SM86 length mismatch for {label}");
                    assert!(
                        (sm86_cp[0].distance_sq - 0.25).abs() < CUDA_PARITY_TOLERANCE,
                        "SM86 distance_sq mismatch for {label}"
                    );
                    assert!(
                        (sm86_cp[0].closest_point.x - 0.1).abs() < CUDA_PARITY_TOLERANCE,
                        "SM86 closest_point.x mismatch for {label}"
                    );
                    assert!(
                        sm86_cp[0].closest_point.y.abs() < CUDA_PARITY_TOLERANCE,
                        "SM86 closest_point.y mismatch for {label}"
                    );
                    assert!(
                        sm86_cp[0].closest_point.z.abs() < CUDA_PARITY_TOLERANCE,
                        "SM86 closest_point.z mismatch for {label}"
                    );
                }
            }
        }
    }

    #[test]
    fn cuda_sliver_sharp_vertex_closest_point() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1000.0, 0.0, 0.0),
            Point3::new(1000.0, 2.0e-5, 0.0),
        ];
        let triangles = [[0_u32, 1, 2]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let query = [Point3::new(900.0, 9.0e-6, 0.0)];

        if let Some(simt_cp) = ctx.simt_spatial_closest_point(&query, mesh) {
            assert_eq!(simt_cp.len(), 1);
            assert_eq!(simt_cp[0].triangle_index, 0);
            assert!(simt_cp[0].distance_sq < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.x - 900.0).abs() < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.y - 9.0e-6).abs() < CUDA_PARITY_TOLERANCE);
            assert!(simt_cp[0].closest_point.z.abs() < CUDA_PARITY_TOLERANCE);
        }

        if ctx.supports_sm86() {
            if let Some(sm86_cp) = ctx.ptx_sm86_spatial_closest_point(&query, mesh) {
                assert_eq!(sm86_cp.len(), 1);
                assert_eq!(sm86_cp[0].triangle_index, 0);
                assert!(sm86_cp[0].distance_sq < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.x - 900.0).abs() < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.y - 9.0e-6).abs() < CUDA_PARITY_TOLERANCE);
                assert!(sm86_cp[0].closest_point.z.abs() < CUDA_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn cuda_rotated_sliver_sharp_vertex_closest_point() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(
                844.029_628_745_985_4,
                449.098_785_111_286_93,
                -293.128_413_857_272_2,
            ),
            Point3::new(
                844.029_622_883_417_1,
                449.098_801_991_879_5,
                -293.128_404_875_296_5,
            ),
        ];
        let triangles = [[0_u32, 1, 2]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let query = [Point3::new(
            759.626_663_233_231_2,
            404.188_914_196_424_9,
            -263.815_568_429_655_97,
        )];

        if let Some(simt_cp) = ctx.simt_spatial_closest_point(&query, mesh) {
            assert_eq!(simt_cp.len(), 1);
            assert_eq!(simt_cp[0].triangle_index, 0);
            assert!(simt_cp[0].distance_sq < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.x - query[0].x).abs() < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.y - query[0].y).abs() < CUDA_PARITY_TOLERANCE);
            assert!((simt_cp[0].closest_point.z - query[0].z).abs() < CUDA_PARITY_TOLERANCE);
        }

        if ctx.supports_sm86() {
            if let Some(sm86_cp) = ctx.ptx_sm86_spatial_closest_point(&query, mesh) {
                assert_eq!(sm86_cp.len(), 1);
                assert_eq!(sm86_cp[0].triangle_index, 0);
                assert!(sm86_cp[0].distance_sq < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.x - query[0].x).abs() < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.y - query[0].y).abs() < CUDA_PARITY_TOLERANCE);
                assert!((sm86_cp[0].closest_point.z - query[0].z).abs() < CUDA_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn cuda_degeneracy_threshold_normal_precision_closest_point() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(
                2_943.919_949_616_599_8,
                -2_484.031_550_366_579_7,
                -3_187.933_905_687_633,
            ),
            Point3::new(
                2_060.743_987_198_398,
                -1_738.822_040_387_564_4,
                -2_231.553_748_196_103_2,
            ),
        ];
        let triangles = [[0_u32, 1, 2]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let dop = |a: f64, b: f64, c: f64, d: f64| -> f64 {
            let cd = c * d;
            let err = (-c).mul_add(d, cd);
            let f = a.mul_add(b, -cd);
            f + err
        };
        let nx = dop(vertices[1].y, vertices[2].z, vertices[1].z, vertices[2].y);
        let ny = dop(vertices[1].z, vertices[2].x, vertices[1].x, vertices[2].z);
        let nz = dop(vertices[1].x, vertices[2].y, vertices[1].y, vertices[2].x);
        let n_len = nx.hypot(ny).hypot(nz);
        let centroid = Point3::new(
            (vertices[0].x + vertices[1].x + vertices[2].x) / 3.0,
            (vertices[0].y + vertices[1].y + vertices[2].y) / 3.0,
            (vertices[0].z + vertices[1].z + vertices[2].z) / 3.0,
        );
        let query = [Point3::new(
            centroid.x + nx / n_len,
            centroid.y + ny / n_len,
            centroid.z + nz / n_len,
        )];
        let cpu_cp = cpu::closest_point_mesh(&query, mesh);

        if let Some(simt_cp) = ctx.simt_spatial_closest_point(&query, mesh) {
            assert_eq!(simt_cp.len(), 1);
            assert!((simt_cp[0].distance_sq - cpu_cp[0].distance_sq).abs() < CUDA_PARITY_TOLERANCE);
            assert!(
                (simt_cp[0].closest_point.x - cpu_cp[0].closest_point.x).abs()
                    < CUDA_PARITY_TOLERANCE
            );
            assert!(
                (simt_cp[0].closest_point.y - cpu_cp[0].closest_point.y).abs()
                    < CUDA_PARITY_TOLERANCE
            );
            assert!(
                (simt_cp[0].closest_point.z - cpu_cp[0].closest_point.z).abs()
                    < CUDA_PARITY_TOLERANCE
            );
        }

        if ctx.supports_sm86() {
            if let Some(sm86_cp) = ctx.ptx_sm86_spatial_closest_point(&query, mesh) {
                assert_eq!(sm86_cp.len(), 1);
                assert!(
                    (sm86_cp[0].distance_sq - cpu_cp[0].distance_sq).abs() < CUDA_PARITY_TOLERANCE
                );
                assert!(
                    (sm86_cp[0].closest_point.x - cpu_cp[0].closest_point.x).abs()
                        < CUDA_PARITY_TOLERANCE
                );
                assert!(
                    (sm86_cp[0].closest_point.y - cpu_cp[0].closest_point.y).abs()
                        < CUDA_PARITY_TOLERANCE
                );
                assert!(
                    (sm86_cp[0].closest_point.z - cpu_cp[0].closest_point.z).abs()
                        < CUDA_PARITY_TOLERANCE
                );
            }
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
    fn cuda_hausdorff_close_points_and_infinite_closest_point_query() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        let pts_a = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 0.0, 0.0)];
        let pts_b = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 1.0e-6, 0.0)];
        let simt_h = ctx
            .simt_spatial_hausdorff(&pts_a, &pts_b)
            .expect("SIMT Hausdorff");
        assert!((simt_h - 1.0e-6).abs() < CUDA_PARITY_TOLERANCE);
        if ctx.supports_sm86() {
            let sm86_h = ctx
                .ptx_sm86_spatial_hausdorff(&pts_a, &pts_b)
                .expect("SM86 Hausdorff");
            assert!((sm86_h - 1.0e-6).abs() < CUDA_PARITY_TOLERANCE);
        }

        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ];
        let triangles = [[0_u32, 1, 2]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let inf_query = [Point3::new(f64::INFINITY, 0.0, 0.0)];
        let cpu_cp = cpu::closest_point_mesh(&inf_query, mesh);
        let simt_cp = ctx
            .simt_spatial_closest_point(&inf_query, mesh)
            .expect("SIMT closest point on inf");
        assert!(!simt_cp[0].distance_sq.is_nan());
        assert!(!simt_cp[0].closest_point.x.is_nan());
        assert!(!simt_cp[0].closest_point.y.is_nan());
        assert!(!simt_cp[0].closest_point.z.is_nan());
        assert_eq!(simt_cp, cpu_cp);
        if ctx.supports_sm86() {
            let sm86_cp = ctx
                .ptx_sm86_spatial_closest_point(&inf_query, mesh)
                .expect("SM86 closest point on inf");
            assert_eq!(sm86_cp, cpu_cp);
        }
        if ctx.supports_optix_rt() {
            let optix_h = ctx
                .optix_rt_spatial_hausdorff(&pts_a, &pts_b)
                .expect("OptiX RT Hausdorff");
            assert!((optix_h - 1.0e-6).abs() < CUDA_PARITY_TOLERANCE);
            let optix_cp = ctx
                .optix_rt_closest_point_mesh(&inf_query, mesh)
                .expect("OptiX RT closest point on inf");
            assert_eq!(optix_cp, cpu_cp);
        }
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

    #[test]
    fn cuda_optix_rt_closest_point_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        if !ctx.supports_optix_rt() {
            return;
        }
        let pts_a: Vec<Point3> = (0_u32..96)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.12 - 2.5, (f * 0.27).sin(), (f * 0.19).cos())
            })
            .collect();
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
        let expected_cp = cpu::closest_point_mesh(&pts_a, mesh);
        let actual_cp = ctx
            .optix_rt_closest_point_mesh(&pts_a, mesh)
            .expect("CUDA OptiX RT Core closest point");
        for (e, a) in expected_cp.iter().zip(&actual_cp) {
            assert!((e.distance_sq - a.distance_sq).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.x - a.closest_point.x).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.y - a.closest_point.y).abs() < CUDA_PARITY_TOLERANCE);
            assert!((e.closest_point.z - a.closest_point.z).abs() < CUDA_PARITY_TOLERANCE);
            assert!(a.triangle_index < 4);
        }
    }

    #[test]
    fn cuda_optix_rt_hausdorff_matches_cpu() {
        let Some(ctx) = global_cuda() else {
            return;
        };
        if !ctx.supports_optix_rt() {
            return;
        }
        let pts_a: Vec<Point3> = (0_u32..96)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.12 - 2.5, (f * 0.27).sin(), (f * 0.19).cos())
            })
            .collect();
        let pts_b: Vec<Point3> = (0_u32..80)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.12 - 2.41, (f * 0.27).sin() + 0.04, (f * 0.19).cos())
            })
            .collect();
        let expected_h = crate::spatial::kd_hausdorff_distance(&pts_a, &pts_b);
        let actual_h = ctx
            .optix_rt_spatial_hausdorff(&pts_a, &pts_b)
            .expect("CUDA OptiX RT Core Hausdorff");
        assert!((expected_h - actual_h).abs() < CUDA_PARITY_TOLERANCE);
    }
}
