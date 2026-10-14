//! GPIO buttons through the Linux gpiochip character device, uAPI v2
//! (SPEC-064 GPIO-64-01). Lines are requested as inputs with the configured
//! bias, active level and debounce, and sampled once per frame.

use crate::settings::Gpio;

/// Lines requested from one chip and the GBA key bit of each line.
pub struct GpioInput {
    #[cfg(target_os = "linux")]
    request: std::fs::File,
    keys: Vec<u16>,
}

impl GpioInput {
    /// Opens the configured lines. `Ok(None)` when GPIO is disabled or no
    /// button has a line.
    pub fn open(cfg: &Gpio) -> Result<Option<Self>, String> {
        if !cfg.enabled {
            return Ok(None);
        }
        let mut offsets = Vec::new();
        let mut keys = Vec::new();
        for (i, line) in cfg.lines.iter().enumerate() {
            if let Some(l) = line {
                offsets.push(*l);
                keys.push(1u16 << i);
            }
        }
        if offsets.is_empty() {
            return Ok(None);
        }
        imp::request(cfg, &offsets).map(|request| {
            #[cfg(not(target_os = "linux"))]
            let _ = request;
            Some(Self {
                #[cfg(target_os = "linux")]
                request,
                keys,
            })
        })
    }

    /// KEYINPUT mask of the lines that are currently active.
    pub fn mask(&self) -> u16 {
        #[cfg(target_os = "linux")]
        {
            match imp::values(&self.request, self.keys.len()) {
                Ok(bits) => self
                    .keys
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| bits & (1u64 << i) != 0)
                    .fold(0, |m, (_, k)| m | k),
                Err(_) => 0,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &self.keys;
            0
        }
    }
}

/// Read the active state of `lines` once (settings app live test,
/// GPIO-64-02). Returns one bool per line.
pub fn probe(cfg: &Gpio, lines: &[u32]) -> Result<Vec<bool>, String> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let request = imp::request(cfg, lines)?;
    #[cfg(target_os = "linux")]
    {
        let bits = imp::values(&request, lines.len())?;
        Ok((0..lines.len()).map(|i| bits & (1u64 << i) != 0).collect())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = request;
        unreachable!("request fails outside Linux")
    }
}

pub fn available() -> bool {
    cfg!(target_os = "linux")
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use crate::settings::Gpio;

    pub fn request(_cfg: &Gpio, _offsets: &[u32]) -> Result<(), String> {
        Err("GPIO está disponível apenas no Linux (gpiochip)".into())
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod imp {
    use std::ffi::{c_int, c_ulong};
    use std::fs::{File, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd};

    use crate::settings::{Bias, Gpio};

    const LINES_MAX: usize = 64;
    const NUM_ATTRS_MAX: usize = 10;

    const FLAG_ACTIVE_LOW: u64 = 1 << 1;
    const FLAG_INPUT: u64 = 1 << 2;
    const FLAG_BIAS_PULL_UP: u64 = 1 << 8;
    const FLAG_BIAS_PULL_DOWN: u64 = 1 << 9;
    const FLAG_BIAS_DISABLED: u64 = 1 << 10;
    const ATTR_ID_DEBOUNCE: u32 = 3;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct LineAttribute {
        id: u32,
        padding: u32,
        /// Union of flags / values / debounce_period_us (low 32 bits).
        value: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct LineConfigAttribute {
        attr: LineAttribute,
        mask: u64,
    }

    #[repr(C)]
    struct LineConfig {
        flags: u64,
        num_attrs: u32,
        padding: [u32; 5],
        attrs: [LineConfigAttribute; NUM_ATTRS_MAX],
    }

    #[repr(C)]
    struct LineRequest {
        offsets: [u32; LINES_MAX],
        consumer: [u8; 32],
        config: LineConfig,
        num_lines: u32,
        event_buffer_size: u32,
        padding: [u32; 5],
        fd: i32,
    }

    #[repr(C)]
    struct LineValues {
        bits: u64,
        mask: u64,
    }

    const _: () = {
        assert!(std::mem::size_of::<LineAttribute>() == 16);
        assert!(std::mem::size_of::<LineConfig>() == 272);
        assert!(std::mem::size_of::<LineRequest>() == 592);
        assert!(std::mem::size_of::<LineValues>() == 16);
    };

    /// `_IOWR(0xB4, nr, size)` on Linux (dir read|write = 3).
    const fn iowr(nr: c_ulong, size: usize) -> c_ulong {
        (3 << 30) | ((size as c_ulong) << 16) | (0xB4 << 8) | nr
    }
    const GET_LINE_IOCTL: c_ulong = iowr(0x07, std::mem::size_of::<LineRequest>());
    const LINE_GET_VALUES_IOCTL: c_ulong = iowr(0x0E, std::mem::size_of::<LineValues>());

    unsafe extern "C" {
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }

    pub fn request(cfg: &Gpio, offsets: &[u32]) -> Result<File, String> {
        if offsets.len() > LINES_MAX {
            return Err("too many GPIO lines".into());
        }
        let chip = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&cfg.chip)
            .map_err(|e| {
                format!(
                    "cannot open {} ({e}); check that the user can access it (group gpio)",
                    cfg.chip
                )
            })?;
        let mut flags = FLAG_INPUT;
        if cfg.active_low {
            flags |= FLAG_ACTIVE_LOW;
        }
        flags |= match cfg.bias {
            Bias::PullUp => FLAG_BIAS_PULL_UP,
            Bias::PullDown => FLAG_BIAS_PULL_DOWN,
            Bias::Disabled => FLAG_BIAS_DISABLED,
        };
        let mut req = LineRequest {
            offsets: [0; LINES_MAX],
            consumer: [0; 32],
            config: LineConfig {
                flags,
                num_attrs: 0,
                padding: [0; 5],
                attrs: [LineConfigAttribute::default(); NUM_ATTRS_MAX],
            },
            num_lines: offsets.len() as u32,
            event_buffer_size: 0,
            padding: [0; 5],
            fd: -1,
        };
        req.offsets[..offsets.len()].copy_from_slice(offsets);
        let name = b"gba-recomp";
        req.consumer[..name.len()].copy_from_slice(name);
        if cfg.debounce_ms > 0 {
            let all = if offsets.len() == 64 {
                u64::MAX
            } else {
                (1u64 << offsets.len()) - 1
            };
            req.config.attrs[0] = LineConfigAttribute {
                attr: LineAttribute {
                    id: ATTR_ID_DEBOUNCE,
                    padding: 0,
                    value: u64::from(cfg.debounce_ms * 1000),
                },
                mask: all,
            };
            req.config.num_attrs = 1;
        }
        // SAFETY: `req` is a correctly laid out gpio_v2_line_request that
        // lives for the duration of the call; the kernel fills `req.fd`.
        let rc = unsafe {
            ioctl(
                chip.as_raw_fd(),
                GET_LINE_IOCTL,
                &mut req as *mut LineRequest,
            )
        };
        if rc < 0 || req.fd < 0 {
            return Err(format!(
                "GPIO line request on {} failed: {}",
                cfg.chip,
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: the kernel returned a fresh file descriptor we now own.
        Ok(unsafe { File::from_raw_fd(req.fd) })
    }

    pub fn values(request: &File, lines: usize) -> Result<u64, String> {
        let mut v = LineValues {
            bits: 0,
            mask: if lines >= 64 {
                u64::MAX
            } else {
                (1u64 << lines) - 1
            },
        };
        // SAFETY: `v` is a gpio_v2_line_values living for the call.
        let rc = unsafe {
            ioctl(
                request.as_raw_fd(),
                LINE_GET_VALUES_IOCTL,
                &mut v as *mut LineValues,
            )
        };
        if rc < 0 {
            return Err(format!(
                "GPIO read failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(v.bits)
    }
}
