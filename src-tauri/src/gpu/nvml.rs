//! NVIDIA's management library (NVML), loaded at run time: the NVIDIA driver installs
//! `libnvidia-ml.so.1`, so nothing is linked and a machine without it simply has no reading.
//! Only the machine's load and memory; NVIDIA's driver gives no DRM client statistics.
//! Declarations from `nvml.h` (evidence: docs/wiki/pages/platforms/gpu-readings.md).

use std::ffi::{c_int, c_uint, c_ulonglong, c_void, CStr};

/// `nvmlReturn_t`; `NVML_SUCCESS` is 0.
type Return = c_int;
const SUCCESS: Return = 0;
/// `nvmlDevice_t`, an opaque pointer.
type Device = *mut c_void;
/// More devices than any machine has; the count is capped so a broken answer cannot loop long.
const MAX_DEVICES: c_uint = 64;

/// `nvmlUtilization_t`. Written by NVML; Hangar reads only part of it.
#[repr(C)]
#[derive(Default)]
#[allow(dead_code)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

/// `nvmlMemory_t`, in bytes. Written by NVML; Hangar reads only part of it.
#[repr(C)]
#[derive(Default)]
#[allow(dead_code)]
struct Memory {
    total: c_ulonglong,
    free: c_ulonglong,
    used: c_ulonglong,
}

/// The `dlopen` handle, closed when dropped.
struct Library(*mut c_void);

impl Library {
    fn open(name: &CStr) -> Option<Self> {
        // SAFETY: `name` is NUL-terminated; dlopen has no other preconditions.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        (!handle.is_null()).then_some(Self(handle))
    }

    /// The address of `name` as a function pointer of type `F`.
    ///
    /// # Safety
    /// `F` must be an `extern "C"` function pointer type matching the symbol's C declaration.
    unsafe fn function<F: Copy>(&self, name: &CStr) -> Option<F> {
        // Checked when compiling: only a type of a data pointer's size can be a function pointer.
        const { assert!(std::mem::size_of::<F>() == std::mem::size_of::<*mut c_void>()) };
        // SAFETY: the handle is open for as long as `self` lives; `name` is NUL-terminated.
        let address = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        if address.is_null() {
            return None;
        }
        // SAFETY: the caller guarantees `F` is a function pointer type, which has the size of a
        // data pointer on every platform Hangar runs on.
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&address) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: the handle came from dlopen and is closed exactly once, here.
        unsafe { libc::dlclose(self.0) };
    }
}

/// An initialized NVML; shut down when dropped, before the library closes.
pub struct Nvml {
    shutdown: unsafe extern "C" fn() -> Return,
    count: unsafe extern "C" fn(*mut c_uint) -> Return,
    handle: unsafe extern "C" fn(c_uint, *mut Device) -> Return,
    utilization: unsafe extern "C" fn(Device, *mut Utilization) -> Return,
    memory: unsafe extern "C" fn(Device, *mut Memory) -> Return,
    /// Declared last, so it is dropped after `Drop::drop` has shut NVML down.
    _library: Library,
}

// SAFETY: NVML's functions are thread-safe (NVML API reference, "Thread safety"), and the GPU
// monitor's mutex lets only one thread use them at a time anyway; the library handle is a plain
// pointer that dlclose accepts from any thread.
unsafe impl Send for Nvml {}

impl Nvml {
    /// NVML when the NVIDIA driver is installed and starts; `None` otherwise.
    pub fn open() -> Option<Self> {
        let library = Library::open(c"libnvidia-ml.so.1")?;
        // SAFETY: each type is the C declaration of the symbol in nvml.h.
        let (init, shutdown, count, handle, utilization, memory) = unsafe {
            (
                library.function::<unsafe extern "C" fn() -> Return>(c"nvmlInit_v2")?,
                library.function::<unsafe extern "C" fn() -> Return>(c"nvmlShutdown")?,
                library.function::<unsafe extern "C" fn(*mut c_uint) -> Return>(
                    c"nvmlDeviceGetCount_v2",
                )?,
                library.function::<unsafe extern "C" fn(c_uint, *mut Device) -> Return>(
                    c"nvmlDeviceGetHandleByIndex_v2",
                )?,
                library.function::<unsafe extern "C" fn(Device, *mut Utilization) -> Return>(
                    c"nvmlDeviceGetUtilizationRates",
                )?,
                library.function::<unsafe extern "C" fn(Device, *mut Memory) -> Return>(
                    c"nvmlDeviceGetMemoryInfo",
                )?,
            )
        };
        // SAFETY: nvmlInit_v2 takes no arguments; a failure leaves nothing to shut down.
        if unsafe { init() } != SUCCESS {
            return None;
        }
        Some(Self {
            shutdown,
            count,
            handle,
            utilization,
            memory,
            _library: library,
        })
    }

    /// Load in percent and memory in use in bytes of every device; a value NVML does not give
    /// (a device without utilization counters, say) stays `None`.
    pub fn read(&self) -> Vec<(Option<f64>, Option<u64>)> {
        let mut count: c_uint = 0;
        // SAFETY: `count` is a valid, writable location for the duration of the call.
        if unsafe { (self.count)(&mut count) } != SUCCESS {
            return Vec::new();
        }
        (0..count.min(MAX_DEVICES))
            .filter_map(|index| {
                let mut device: Device = std::ptr::null_mut();
                // SAFETY: `device` is a valid, writable location; `index` is below the count.
                if unsafe { (self.handle)(index, &mut device) } != SUCCESS {
                    return None;
                }
                let mut utilization = Utilization::default();
                // SAFETY: `device` came from NVML, `utilization` is valid and writable.
                let load = (unsafe { (self.utilization)(device, &mut utilization) } == SUCCESS)
                    .then(|| f64::from(utilization.gpu));
                let mut memory = Memory::default();
                // SAFETY: as above, with `memory`.
                let used = (unsafe { (self.memory)(device, &mut memory) } == SUCCESS)
                    .then_some(memory.used);
                Some((load, used))
            })
            .collect()
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: NVML was initialized in `open`; shutting it down once balances that.
        unsafe { (self.shutdown)() };
    }
}
