//! CPU/GPU temperature and fan speed from the System Management Controller.
//!
//! The SMC is reached through the `AppleSMC` I/O Kit service, which any user
//! may open read-only. Every value lives under a four-character key; the
//! source lists all keys once (a few thousand on Apple Silicon, ~100 ms) and
//! keeps the ones it uses:
//!
//! | metric | Apple Silicon | Intel |
//! |---|---|---|
//! | CPU temperature | mean of the core sensors `Tp*` (P-cores) and `Te*` (E-cores) | mean of `TC*` |
//! | GPU temperature | mean of `Tg*` | mean of `TG*` |
//! | fan speed | fastest `F<n>Ac` (actual RPM) | same |
//!
//! The mean of the core sensors is what macmon and asitop report as CPU
//! temperature. Some keys of each family are placeholders on a given chip
//! (an M4 Pro's `Tp00` reads -4 °C), so readings outside 10–120 °C are dropped.
//! A fan reading 0 RPM is stopped, not missing: MacBook Pro fans stop at idle.
//! The Intel key families are the classic SMC ones but untested here.

use crate::metrics::{metric, MetricId};
use crate::num::count_f64;
use crate::source::{MetricSource, Readings, SourceStatus};
use core_foundation_sys::dictionary::CFMutableDictionaryRef;
use std::ffi::{c_char, c_void};

const PROVIDES: &[MetricId] = &[metric::CPU_TEMP, metric::GPU_TEMP, metric::FAN_SPEED];

const PLAUSIBLE_TEMP: std::ops::RangeInclusive<f64> = 10.0..=120.0;
const PLAUSIBLE_RPM: std::ops::RangeInclusive<f64> = 0.0..=10_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Cpu,
    Gpu,
    Fan,
}

/// Which reading a key feeds, if any. `apple_silicon` picks the key families.
fn classify(key: [u8; 4], apple_silicon: bool) -> Option<Group> {
    if key[0] == b'F' && key[1].is_ascii_digit() && &key[2..] == b"Ac" {
        return Some(Group::Fan);
    }
    let family = &key[..2];
    let (cpu, gpu): (&[&[u8]], &[u8]) = if apple_silicon { (&[b"Tp", b"Te"], b"Tg") } else { (&[b"TC"], b"TG") };
    if cpu.contains(&family) {
        Some(Group::Cpu)
    } else if family == gpu {
        Some(Group::Gpu)
    } else {
        None
    }
}

/// An SMC value as a number, by its data type. Apple Silicon uses
/// little-endian `flt `; Intel Macs big-endian fixed point (`sp78` signed
/// 8.8 for temperatures, `fpe2` unsigned 14.2 for fan RPM).
fn decode(data_type: [u8; 4], bytes: &[u8]) -> Option<f64> {
    let take2 = || bytes.get(..2).and_then(|b| <[u8; 2]>::try_from(b).ok());
    let take4 = || bytes.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok());
    match &data_type {
        b"flt " => take4().map(|b| f64::from(f32::from_le_bytes(b))),
        b"sp78" => take2().map(|b| f64::from(i16::from_be_bytes(b)) / 256.0),
        b"fpe2" => take2().map(|b| f64::from(u16::from_be_bytes(b)) / 4.0),
        b"ui8 " => bytes.first().map(|b| f64::from(*b)),
        b"ui16" => take2().map(|b| f64::from(u16::from_be_bytes(b))),
        b"ui32" => take4().map(|b| f64::from(u32::from_be_bytes(b))),
        _ => None,
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / count_f64(values.len()))
}

pub struct SmcSource {
    smc: Option<Smc>,
    /// Keys in use, with their type and size, found on the first poll.
    keys: Vec<(Group, u32, KeyInfo)>,
    /// Opening the SMC failed once: don't retry every poll.
    failed: bool,
    status: SourceStatus,
}

impl SmcSource {
    /// Cheap: the SMC is opened on the first poll.
    #[must_use]
    pub fn new() -> Self {
        Self { smc: None, keys: Vec::new(), failed: false, status: SourceStatus::not_yet_polled() }
    }

    fn connect(&mut self) -> Result<(), String> {
        let smc = Smc::open()?;
        let apple_silicon = cfg!(target_arch = "aarch64");
        let count = smc.key_count().ok_or("the SMC did not report its key count")?;
        for index in 0..count {
            let Some(key) = smc.key_at(index) else { continue };
            let Some(group) = classify(key.to_be_bytes(), apple_silicon) else { continue };
            if let Some(info) = smc.info(key) {
                self.keys.push((group, key, info));
            }
        }
        crate::info!("[smc] {} of {count} keys used", self.keys.len());
        self.smc = Some(smc);
        Ok(())
    }
}

impl Default for SmcSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for SmcSource {
    fn id(&self) -> &'static str {
        "smc"
    }

    fn display_name(&self) -> &'static str {
        "Apple SMC (CPU/GPU temp, fans)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        if self.smc.is_none() && !self.failed {
            if let Err(reason) = self.connect() {
                self.failed = true;
                self.status = SourceStatus::Unavailable { reason };
            }
        }
        let Some(smc) = &self.smc else { return };

        let (mut cpu, mut gpu, mut fan) = (Vec::new(), Vec::new(), None::<f64>);
        for (group, key, info) in &self.keys {
            let Some(v) = smc.read(*key, *info) else { continue };
            match group {
                Group::Cpu if PLAUSIBLE_TEMP.contains(&v) => cpu.push(v),
                Group::Gpu if PLAUSIBLE_TEMP.contains(&v) => gpu.push(v),
                Group::Fan if PLAUSIBLE_RPM.contains(&v) => fan = Some(fan.map_or(v, |f| f.max(v))),
                _ => {}
            }
        }
        let (cpu, gpu) = (mean(&cpu), mean(&gpu));
        out.set(metric::CPU_TEMP, cpu);
        out.set(metric::GPU_TEMP, gpu);
        out.set(metric::FAN_SPEED, fan);
        self.status = if cpu.or(gpu).or(fan).is_some() {
            SourceStatus::Online
        } else {
            SourceStatus::Unavailable { reason: format!("no plausible reading among {} SMC key(s)", self.keys.len()) }
        };
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

// ---------------------------------------------------------------- SMC access

type KernReturn = i32;
type IoObject = u32;

/// `kIOMainPortDefault`.
const MAIN_PORT_DEFAULT: u32 = 0;
/// The `AppleSMC` user client's one method, `kSMCHandleYPCEvent`.
const HANDLE_YPC_EVENT: u32 = 2;
const CMD_READ_BYTES: u8 = 5;
const CMD_KEY_AT_INDEX: u8 = 8;
const CMD_KEY_INFO: u8 = 9;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingService(main_port: u32, matching: CFMutableDictionaryRef) -> IoObject;
    fn IOServiceOpen(service: IoObject, owning_task: u32, kind: u32, connect: *mut IoObject) -> KernReturn;
    fn IOServiceClose(connect: IoObject) -> KernReturn;
    fn IOObjectRelease(object: IoObject) -> KernReturn;
    fn IOConnectCallStructMethod(
        connection: IoObject,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> KernReturn;
}

extern "C" {
    static mach_task_self_: u32;
}

/// `SMCKeyData_t`, the AppleSMC call's argument and result (80 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyData {
    key: u32,
    vers: [u8; 6],
    p_limit: PLimitData,
    info: KeyInfo,
    result: u8,
    status: u8,
    command: u8,
    data32: u32,
    bytes: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PLimitData {
    version: u16,
    length: u16,
    cpu: u32,
    gpu: u32,
    mem: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct KeyInfo {
    size: u32,
    data_type: u32,
    attributes: u8,
}

/// An open connection to the SMC, closed on drop.
struct Smc {
    conn: IoObject,
}

impl Smc {
    fn open() -> Result<Self, String> {
        // SAFETY: plain I/O Kit calls. IOServiceGetMatchingService consumes
        // the matching dictionary; the service is released once opened.
        unsafe {
            let service = IOServiceGetMatchingService(MAIN_PORT_DEFAULT, IOServiceMatching(c"AppleSMC".as_ptr()));
            if service == 0 {
                return Err("no AppleSMC service".into());
            }
            let mut conn = 0;
            let result = IOServiceOpen(service, mach_task_self_, 0, &raw mut conn);
            IOObjectRelease(service);
            if result != 0 {
                return Err(format!("cannot open the SMC (kern_return {result:#x})"));
            }
            Ok(Self { conn })
        }
    }

    fn call(&self, input: &KeyData) -> Option<KeyData> {
        let mut output = KeyData::default();
        let mut size = std::mem::size_of::<KeyData>();
        // SAFETY: both buffers are `SMCKeyData_t`-sized and live for the call.
        let result = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                HANDLE_YPC_EVENT,
                std::ptr::from_ref(input).cast(),
                size,
                std::ptr::from_mut(&mut output).cast(),
                &raw mut size,
            )
        };
        (result == 0 && output.result == 0).then_some(output)
    }

    fn key_count(&self) -> Option<u32> {
        let info = self.info(u32::from_be_bytes(*b"#KEY"))?;
        let data = self.raw(u32::from_be_bytes(*b"#KEY"), info)?;
        Some(u32::from_be_bytes(data.bytes[..4].try_into().ok()?))
    }

    fn key_at(&self, index: u32) -> Option<u32> {
        self.call(&KeyData { command: CMD_KEY_AT_INDEX, data32: index, ..KeyData::default() }).map(|d| d.key)
    }

    fn info(&self, key: u32) -> Option<KeyInfo> {
        self.call(&KeyData { key, command: CMD_KEY_INFO, ..KeyData::default() }).map(|d| d.info)
    }

    fn raw(&self, key: u32, info: KeyInfo) -> Option<KeyData> {
        self.call(&KeyData { key, info, command: CMD_READ_BYTES, ..KeyData::default() })
    }

    fn read(&self, key: u32, info: KeyInfo) -> Option<f64> {
        let data = self.raw(key, info)?;
        let size = usize::try_from(info.size).ok()?.min(data.bytes.len());
        decode(info.data_type.to_be_bytes(), &data.bytes[..size]).filter(|v| v.is_finite())
    }
}

impl Drop for Smc {
    fn drop(&mut self) {
        // SAFETY: `conn` came from IOServiceOpen and is closed once.
        unsafe {
            IOServiceClose(self.conn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_data_matches_the_c_layout() {
        assert_eq!(std::mem::size_of::<KeyData>(), 80);
    }

    #[test]
    fn classifies_apple_silicon_keys() {
        assert_eq!(classify(*b"Tp1o", true), Some(Group::Cpu));
        assert_eq!(classify(*b"Te05", true), Some(Group::Cpu));
        assert_eq!(classify(*b"Tg0f", true), Some(Group::Gpu));
        assert_eq!(classify(*b"F0Ac", true), Some(Group::Fan));
        assert_eq!(classify(*b"F0Mx", true), None);
        assert_eq!(classify(*b"TCMz", true), None);
        assert_eq!(classify(*b"PSTR", true), None);
    }

    #[test]
    fn classifies_intel_keys() {
        assert_eq!(classify(*b"TC0P", false), Some(Group::Cpu));
        assert_eq!(classify(*b"TG0D", false), Some(Group::Gpu));
        assert_eq!(classify(*b"Tp01", false), None);
        assert_eq!(classify(*b"F1Ac", false), Some(Group::Fan));
    }

    #[test]
    fn decodes_data_types() {
        assert_eq!(decode(*b"flt ", &57.5f32.to_le_bytes()), Some(57.5));
        assert_eq!(decode(*b"sp78", &[0x3A, 0x80]), Some(58.5));
        assert_eq!(decode(*b"fpe2", &[0x1F, 0x40]), Some(2000.0));
        assert_eq!(decode(*b"ui8 ", &[2]), Some(2.0));
        assert_eq!(decode(*b"ui32", &[0, 0, 1, 23]), Some(279.0));
        assert_eq!(decode(*b"flt ", &[1, 2]), None);
        assert_eq!(decode(*b"ioft", &[0; 8]), None);
    }

    #[test]
    fn mean_of_nothing_is_nothing() {
        assert_eq!(mean(&[]), None);
        assert_eq!(mean(&[40.0, 50.0]), Some(45.0));
    }
}
