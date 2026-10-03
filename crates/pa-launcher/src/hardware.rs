use pa_types::hardware::{
    ApiAvailability, CpuFeature, GpuAdapter, GraphicsApi, HardwareProfile, InstructionSets,
    Measurement,
};
use sysinfo::System;

/// Erfasst jede Ressourcenentscheidung aus aktuellen Hostdaten statt aus Gerätekategorien.
pub fn probe_hardware() -> HardwareProfile {
    let mut system = System::new_all();
    system.refresh_all();
    let cpu_model = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|brand| !brand.is_empty())
        .unwrap_or_else(|| "unavailable CPU model".to_owned());
    let physical_cores = match System::physical_core_count() {
        Some(value) if value > 0 => Measurement::Measured {
            value,
            method: "sysinfo physical_core_count".to_owned(),
        },
        _ => Measurement::Unavailable {
            reason: "physical core count was not reported".to_owned(),
            method: "sysinfo physical_core_count".to_owned(),
        },
    };

    HardwareProfile {
        cpu_model,
        physical_cores,
        logical_processors: system.cpus().len(),
        total_ram_bytes: system.total_memory(),
        available_ram_bytes: system.available_memory(),
        operating_system: operating_system(),
        instruction_sets: instruction_sets(),
        gpus: gpu_adapters(),
        graphics_apis: graphics_apis(),
        process_elevated: process_elevated(),
    }
}

fn operating_system() -> String {
    let edition = System::long_os_version().unwrap_or_else(|| "unavailable edition".to_owned());
    let version = System::os_version().unwrap_or_else(|| "unavailable version".to_owned());
    let kernel = System::kernel_version().unwrap_or_else(|| "unavailable build".to_owned());
    format!("{edition}; Version {version}; Kernel/Build {kernel}")
}

fn instruction_sets() -> InstructionSets {
    InstructionSets {
        avx2: CpuFeature::new(avx2_available(), "Rust runtime CPU feature detection: avx2"),
        avx512: CpuFeature::new(
            avx512_available(),
            "Rust runtime CPU feature detection: avx512f",
        ),
        neon: CpuFeature::new(
            neon_available(),
            "Rust runtime CPU feature detection: neon; false on non-ARM hosts",
        ),
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn avx2_available() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn avx2_available() -> bool {
    false
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn avx512_available() -> bool {
    std::arch::is_x86_feature_detected!("avx512f")
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn avx512_available() -> bool {
    false
}

#[cfg(target_arch = "aarch64")]
fn neon_available() -> bool {
    std::arch::is_aarch64_feature_detected!("neon")
}

#[cfg(not(target_arch = "aarch64"))]
fn neon_available() -> bool {
    false
}

#[cfg(windows)]
fn gpu_adapters() -> Measurement<Vec<GpuAdapter>> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND,
    };

    let result = (|| -> windows::core::Result<Vec<GpuAdapter>> {
        // SAFETY: DXGI owns the returned COM interfaces and windows-rs releases them on drop.
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let mut adapters = Vec::new();
        for index in 0_u32.. {
            // SAFETY: The valid factory is enumerated until DXGI reports the documented end.
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(adapter) => adapter,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(error) => return Err(error),
            };
            // SAFETY: windows-rs returns a fully initialized descriptor on success.
            let description = unsafe { adapter.GetDesc1()? };
            let end = description
                .Description
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(description.Description.len());
            adapters.push(GpuAdapter {
                model: String::from_utf16_lossy(&description.Description[..end]),
                dedicated_vram_bytes: description.DedicatedVideoMemory as u64,
                shared_system_memory_bytes: description.SharedSystemMemory as u64,
                software_adapter: (description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0,
            });
        }
        Ok(adapters)
    })();

    match result {
        Ok(value) => Measurement::Measured {
            value,
            method: "DXGI 1.1 adapter enumeration".to_owned(),
        },
        Err(error) => Measurement::Unavailable {
            reason: error.to_string(),
            method: "DXGI 1.1 adapter enumeration".to_owned(),
        },
    }
}

#[cfg(not(windows))]
fn gpu_adapters() -> Measurement<Vec<GpuAdapter>> {
    Measurement::Unavailable {
        reason: "MVP hardware probing supports Windows only".to_owned(),
        method: "DXGI unavailable on this platform".to_owned(),
    }
}

#[cfg(windows)]
fn graphics_apis() -> Vec<ApiAvailability> {
    vec![
        dll_api(GraphicsApi::Vulkan, "vulkan-1.dll"),
        dll_api(GraphicsApi::Cuda, "nvcuda.dll"),
        dll_api(GraphicsApi::DirectMl, "DirectML.dll"),
        ApiAvailability {
            api: GraphicsApi::Metal,
            available: false,
            method: "Metal is not a Windows graphics API".to_owned(),
        },
    ]
}

#[cfg(not(windows))]
fn graphics_apis() -> Vec<ApiAvailability> {
    [
        GraphicsApi::Vulkan,
        GraphicsApi::Cuda,
        GraphicsApi::DirectMl,
        GraphicsApi::Metal,
    ]
    .into_iter()
    .map(|api| ApiAvailability {
        api,
        available: false,
        method: "MVP runtime probe supports Windows only".to_owned(),
    })
    .collect()
}

#[cfg(windows)]
fn dll_api(api: GraphicsApi, dll: &str) -> ApiAvailability {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::{Foundation::FreeLibrary, System::LibraryLoader::LoadLibraryW},
    };

    let wide = std::ffi::OsStr::new(dll)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide` is NUL-terminated and remains alive for this call.
    match unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) } {
        Ok(module) => {
            // SAFETY: The module handle came from the successful LoadLibraryW call above.
            let _ = unsafe { FreeLibrary(module) };
            ApiAvailability {
                api,
                available: true,
                method: format!("{dll} loadable; acceleration not exercised"),
            }
        }
        Err(error) => ApiAvailability {
            api,
            available: false,
            method: format!("{dll} not loadable: {error}"),
        },
    }
}

#[cfg(windows)]
fn process_elevated() -> Measurement<bool> {
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let result = (|| -> windows::core::Result<bool> {
        let mut token = HANDLE::default();
        // SAFETY: GetCurrentProcess returns a valid pseudo handle and token is writable.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)? };
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0_u32;
        // SAFETY: The buffer matches TOKEN_ELEVATION and its exact size is supplied.
        let query = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        // SAFETY: token was created by OpenProcessToken and is no longer used afterwards.
        let _ = unsafe { CloseHandle(token) };
        query?;
        Ok(elevation.TokenIsElevated != 0)
    })();

    match result {
        Ok(value) => Measurement::Measured {
            value,
            method: "Windows TokenElevation query".to_owned(),
        },
        Err(error) => Measurement::Unavailable {
            reason: error.to_string(),
            method: "Windows TokenElevation query".to_owned(),
        },
    }
}

#[cfg(not(windows))]
fn process_elevated() -> Measurement<bool> {
    Measurement::Unavailable {
        reason: "MVP elevation probe supports Windows only".to_owned(),
        method: "Windows TokenElevation unavailable".to_owned(),
    }
}
