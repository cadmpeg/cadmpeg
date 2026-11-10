// SPDX-License-Identifier: Apache-2.0
//! Native `f64` Vulkan `wgpu` compute and hardware `wgpu_ray_query` RT Core backend.

use std::sync::OnceLock;

/// Hardware context for safe `wgpu` native `SHADER_F64` and `EXPERIMENTAL_RAY_QUERY` compute execution.
pub struct VulkanContext {
    device_name: String,
    supports_ray_query: bool,
}

static GLOBAL_VULKAN: OnceLock<Option<VulkanContext>> = OnceLock::new();

/// Returns the process-wide [`VulkanContext`] if a `wgpu` adapter with native `SHADER_F64` is available.
#[must_use]
pub fn global_vulkan() -> Option<&'static VulkanContext> {
    GLOBAL_VULKAN.get_or_init(VulkanContext::probe).as_ref()
}

impl VulkanContext {
    /// Probes the primary `wgpu` high-performance adapter for `SHADER_F64` and `EXPERIMENTAL_RAY_QUERY` support.
    #[must_use]
    pub fn probe() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .ok()?;
        let info = adapter.get_info();
        let adapter_features = adapter.features();
        if !adapter_features.contains(wgpu::Features::SHADER_F64) {
            return None;
        }
        let supports_ray_query = adapter_features.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY);
        Some(Self {
            device_name: info.name,
            supports_ray_query,
        })
    }

    /// Name of the active `wgpu` physical adapter.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Returns whether hardware RT Core ray queries (`EXPERIMENTAL_RAY_QUERY`) are active.
    #[must_use]
    pub const fn supports_ray_query(&self) -> bool {
        self.supports_ray_query
    }
}
