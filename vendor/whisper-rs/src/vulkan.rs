use std::ffi::CStr;
use whisper_rs_sys::{
    ggml_backend_buffer_type_t, ggml_backend_dev_buffer_type, ggml_backend_dev_count,
    ggml_backend_dev_description, ggml_backend_dev_get, ggml_backend_dev_memory,
    ggml_backend_dev_t, ggml_backend_dev_type, ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU,
    ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU,
};

#[derive(Debug, Clone)]
pub struct VKVram {
    pub free: usize,
    pub total: usize,
}

/// Human-readable device information
#[derive(Debug, Clone)]
pub struct VkDeviceInfo {
    pub id: usize,
    pub name: String,
    pub vram: VKVram,
    /// Buffer type to pass to `whisper::Backend::create_buffer`
    pub buf_type: ggml_backend_buffer_type_t,
}

/// Check if a device is a GPU (discrete or integrated)
fn is_gpu_device(dev: ggml_backend_dev_t) -> bool {
    let dev_type = unsafe { ggml_backend_dev_type(dev) };
    dev_type == ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU
        || dev_type == ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU
}

/// Enumerate every physical GPU ggml can see.
///
/// Note: integrated GPUs are returned *after* discrete ones,
/// mirroring ggml's C logic.
pub fn list_devices() -> Vec<VkDeviceInfo> {
    unsafe {
        let n = ggml_backend_dev_count();
        (0..n)
            .filter_map(|id| {
                let dev = ggml_backend_dev_get(id);
                if dev.is_null() || !is_gpu_device(dev) {
                    return None;
                }
                let name_ptr = ggml_backend_dev_description(dev);
                let name = if name_ptr.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
                };
                let mut free = 0usize;
                let mut total = 0usize;
                ggml_backend_dev_memory(dev, &mut free, &mut total);
                Some(VkDeviceInfo {
                    id,
                    name,
                    vram: VKVram { free, total },
                    buf_type: ggml_backend_dev_buffer_type(dev),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod vulkan_tests {
    use super::*;

    #[test]
    fn enumerate_must_not_panic() {
        let _ = list_devices();
    }

    #[test]
    fn sane_device_info() {
        let gpus = list_devices();
        let mut seen = std::collections::HashSet::new();

        for dev in &gpus {
            assert!(seen.insert(dev.id), "duplicated id {}", dev.id);
            assert!(!dev.name.trim().is_empty(), "GPU {} has empty name", dev.id);
            assert!(
                dev.vram.total >= dev.vram.free,
                "GPU {} total < free",
                dev.id
            );
        }
    }
}
