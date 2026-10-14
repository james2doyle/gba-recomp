//! High-level BIOS (SPEC-060, BIOS-01). Every implemented SWI is listed in
//! [`IMPLEMENTED_SWIS`]; anything else stops the system with an explicit
//! `UnsupportedSwi` fault instead of returning success.

use std::sync::OnceLock;

use crate::bus::{Bus, SystemFault};
use crate::cpu::{Cpu, MODE_IRQ, MODE_SVC, MODE_SYS};

pub const HLE_VERSION: u32 = 1;

pub const IMPLEMENTED_SWIS: &[u8] = &[
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1F, 0x25, 0x27,
];

#[derive(Clone, Debug, Default)]
pub struct HleState {
    /// IntrWait in progress (discard already applied).
    waiting: bool,
    pub calls: std::collections::BTreeMap<u8, u64>,
}

/// Minimal ARM code for the BIOS IRQ vector, used with HLE: it saves
/// r0–r3/r12/lr, calls the user handler at [0x03FFFFFC] and returns with
/// `SUBS pc, lr, #4`, matching the documented BIOS dispatcher contract.
pub fn install_stub(bios: &mut [u8]) {
    let words: &[(usize, u32)] = &[
        (0x00, 0xEA00_001E),  // b 0x80 (reset → idle loop)
        (0x08, 0xEA00_001C),  // b 0x80 (SWI vector unused with HLE)
        (0x18, 0xEA00_0042),  // b 0x128
        (0x80, 0xEAFF_FFFE),  // b . (parked)
        (0x128, 0xE92D_500F), // stmfd sp!, {r0-r3, r12, lr}
        (0x12C, 0xE3A0_0301), // mov r0, #0x04000000
        (0x130, 0xE28F_E000), // add lr, pc, #0
        (0x134, 0xE510_F004), // ldr pc, [r0, #-4]
        (0x138, 0xE8BD_500F), // ldmfd sp!, {r0-r3, r12, lr}
        (0x13C, 0xE25E_F004), // subs pc, lr, #4
    ];
    for &(offset, word) in words {
        bios[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
}

fn sine_table() -> &'static [i32; 256] {
    static TABLE: OnceLock<[i32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0i32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (f64::from(i as u32) * std::f64::consts::TAU / 256.0)
                .sin()
                .mul_add(16384.0, 0.0)
                .round() as i32;
        }
        t
    })
}

pub fn swi(bus: &mut Bus, cpu: &mut Cpu, number: u8) {
    *bus.hle.calls.entry(number).or_insert(0) += 1;
    // Typical BIOS entry/exit overhead.
    bus.add_cycles(20);
    match number {
        0x00 => soft_reset(bus, cpu),
        0x01 => register_ram_reset(bus, cpu.r[0]),
        0x02 => bus.halt_requested = true,
        0x03 => bus.stop_requested = true,
        0x04 => intr_wait(bus, cpu, cpu.r[0] != 0, cpu.r[1] as u16),
        0x05 => {
            cpu.r[0] = 1;
            cpu.r[1] = 1;
            intr_wait(bus, cpu, true, 1);
        }
        0x06 => div(bus, cpu, cpu.r[0] as i32, cpu.r[1] as i32),
        0x07 => div(bus, cpu, cpu.r[1] as i32, cpu.r[0] as i32),
        0x08 => {
            cpu.r[0] = isqrt(cpu.r[0]);
            bus.add_cycles(40);
        }
        0x09 => cpu.r[0] = arctan(cpu.r[0] as i32) as u32,
        0x0A => cpu.r[0] = arctan2(cpu.r[0] as i32, cpu.r[1] as i32),
        0x0B => cpu_set(bus, cpu.r[0], cpu.r[1], cpu.r[2]),
        0x0C => cpu_fast_set(bus, cpu.r[0], cpu.r[1], cpu.r[2]),
        0x0D => cpu.r[0] = 0xBAAE_187F,
        0x0E => bg_affine_set(bus, cpu.r[0], cpu.r[1], cpu.r[2]),
        0x0F => obj_affine_set(bus, cpu.r[0], cpu.r[1], cpu.r[2], cpu.r[3]),
        0x10 => bit_unpack(bus, cpu.r[0], cpu.r[1], cpu.r[2]),
        0x11 => lz77(bus, cpu.r[0], cpu.r[1], false),
        0x12 => lz77(bus, cpu.r[0], cpu.r[1], true),
        0x13 => huffman(bus, cpu.r[0], cpu.r[1]),
        0x14 => run_length(bus, cpu.r[0], cpu.r[1], false),
        0x15 => run_length(bus, cpu.r[0], cpu.r[1], true),
        0x16 => diff_filter(bus, cpu.r[0], cpu.r[1], false, false),
        0x17 => diff_filter(bus, cpu.r[0], cpu.r[1], false, true),
        0x18 => diff_filter(bus, cpu.r[0], cpu.r[1], true, true),
        0x19 => {
            let level = if cpu.r[0] != 0 { 0x200 } else { 0 };
            let bias = bus.peek16(0x0400_0088);
            bus.poke16(0x0400_0088, (bias & !0x3FE) | level);
        }
        0x1F => midi_key_to_freq(bus, cpu),
        0x25 => cpu.r[0] = 1, // MultiBoot: no link partner, documented failure.
        0x27 => {
            let v = cpu.r[2] as u8;
            bus.poke8(0x0400_0301, v);
        }
        _ => {
            bus.fault = Some(SystemFault::UnsupportedSwi {
                number,
                pc: cpu.current_pc,
            });
        }
    }
}

fn soft_reset(bus: &mut Bus, cpu: &mut Cpu) {
    let flag = bus.peek8(0x0300_7FFA);
    for addr in (0x0300_7E00..0x0300_8000).step_by(4) {
        bus.poke32(addr, 0);
    }
    let entry = if flag != 0 { 0x0200_0000 } else { 0x0800_0000 };
    cpu.write_cpsr(MODE_SVC);
    cpu.r[13] = 0x0300_7FE0;
    cpu.r[14] = 0;
    cpu.write_cpsr(MODE_IRQ);
    cpu.r[13] = 0x0300_7FA0;
    cpu.r[14] = 0;
    cpu.write_cpsr(MODE_SYS);
    cpu.r[13] = 0x0300_7F00;
    for r in 0..13 {
        cpu.r[r] = 0;
    }
    cpu.r[14] = entry;
    cpu.r[15] = entry;
}

fn register_ram_reset(bus: &mut Bus, flags: u32) {
    if flags & 0x01 != 0 {
        bus.ewram.fill(0);
    }
    if flags & 0x02 != 0 {
        bus.iwram[..0x7E00].fill(0);
    }
    if flags & 0x04 != 0 {
        bus.ppu.palette.fill(0);
    }
    if flags & 0x08 != 0 {
        bus.ppu.vram.fill(0);
    }
    if flags & 0x10 != 0 {
        bus.ppu.oam.fill(0);
    }
    if flags & 0x20 != 0 {
        for addr in (0x0400_0120..0x0400_0160).step_by(2) {
            bus.poke16(addr, 0);
        }
    }
    if flags & 0x40 != 0 {
        for addr in (0x0400_0060..0x0400_00B0).step_by(2) {
            if addr != 0x0400_0088 && !(0x0400_00A0..0x0400_00A8).contains(&addr) {
                bus.poke16(addr, 0);
            }
        }
    }
    if flags & 0x80 != 0 {
        for addr in (0x0400_0000..0x0400_0060).step_by(2) {
            bus.poke16(addr, 0);
        }
        for addr in (0x0400_00B0..0x0400_0120).step_by(2) {
            bus.poke16(addr, 0);
        }
        bus.poke16(0x0400_0200, 0);
        bus.poke16(0x0400_0202, 0xFFFF);
        bus.poke16(0x0400_0204, 0);
        bus.poke16(0x0400_0208, 0);
    }
    bus.poke16(0x0400_0000, 0x0080);
    bus.add_cycles(4096);
}

fn intr_wait(bus: &mut Bus, cpu: &mut Cpu, discard: bool, mask: u16) {
    if !bus.hle.waiting && discard {
        let flags = bus.peek16(0x0300_7FF8);
        bus.poke16(0x0300_7FF8, flags & !mask);
    }
    bus.ime = 1;
    let flags = bus.peek16(0x0300_7FF8);
    if flags & mask != 0 {
        bus.poke16(0x0300_7FF8, flags & !mask);
        bus.hle.waiting = false;
        return;
    }
    // Not yet: halt and re-execute this SWI after the next interrupt.
    bus.hle.waiting = true;
    bus.halt_requested = true;
    cpu.r[15] = cpu.current_pc;
}

fn div(bus: &mut Bus, cpu: &mut Cpu, num: i32, den: i32) {
    bus.add_cycles(50);
    if den == 0 {
        // The BIOS never returns from divide-by-zero with valid output;
        // keep the documented sign behaviour instead of hanging the host.
        cpu.r[0] = if num < 0 { u32::MAX } else { 1 };
        cpu.r[1] = num as u32;
        cpu.r[3] = 1;
        return;
    }
    let q = num.wrapping_div(den);
    let r = num.wrapping_rem(den);
    cpu.r[0] = q as u32;
    cpu.r[1] = r as u32;
    cpu.r[3] = q.unsigned_abs();
}

fn isqrt(value: u32) -> u32 {
    let mut x = 0u32;
    let mut bit = 1u32 << 30;
    let mut v = value;
    while bit > v {
        bit >>= 2;
    }
    while bit != 0 {
        if v >= x + bit {
            v -= x + bit;
            x = (x >> 1) + bit;
        } else {
            x >>= 1;
        }
        bit >>= 2;
    }
    x
}

fn arctan(i: i32) -> i32 {
    let a = -((i.wrapping_mul(i)) >> 14);
    let mut b = ((0xA9 * a) >> 14) + 0x390;
    for k in [0x91C, 0xFB6, 0x16AA, 0x2081, 0x3651, 0xA2F9] {
        b = ((b.wrapping_mul(a)) >> 14) + k;
    }
    (i.wrapping_mul(b)) >> 16
}

fn arctan2(x: i32, y: i32) -> u32 {
    if y == 0 {
        return if x >= 0 { 0 } else { 0x8000 };
    }
    if x == 0 {
        return if y >= 0 { 0x4000 } else { 0xC000 };
    }
    let at = |n: i32, d: i32| arctan(((i64::from(n) << 14) / i64::from(d)) as i32);
    let r = if y >= 0 {
        if x >= 0 {
            if x >= y { at(y, x) } else { 0x4000 - at(x, y) }
        } else if -x >= y {
            0x8000 + at(y, x)
        } else {
            0x4000 - at(x, y)
        }
    } else if x <= 0 {
        if -x > -y {
            0x8000 + at(y, x)
        } else {
            0xC000 - at(x, y)
        }
    } else if x >= -y {
        0x10000 + at(y, x)
    } else {
        0xC000 - at(x, y)
    };
    (r as u32) & 0xFFFF
}

fn cpu_set(bus: &mut Bus, src: u32, dst: u32, control: u32) {
    let count = control & 0x1F_FFFF;
    let fill = control & (1 << 24) != 0;
    let word = control & (1 << 26) != 0;
    let (mut s, mut d) = (src, dst);
    if word {
        let (s0, d0) = (s & !3, d & !3);
        s = s0;
        d = d0;
        let v = bus.peek32(s);
        for _ in 0..count {
            let value = if fill { v } else { bus.peek32(s) };
            bus.poke32(d, value);
            if !fill {
                s += 4;
            }
            d += 4;
        }
    } else {
        s &= !1;
        d &= !1;
        let v = bus.peek16(s);
        for _ in 0..count {
            let value = if fill { v } else { bus.peek16(s) };
            bus.poke16(d, value);
            if !fill {
                s += 2;
            }
            d += 2;
        }
    }
    bus.add_cycles(u64::from(count) * 6);
}

fn cpu_fast_set(bus: &mut Bus, src: u32, dst: u32, control: u32) {
    let count = (control & 0x1F_FFFF).div_ceil(8) * 8;
    let fill = control & (1 << 24) != 0;
    let (mut s, mut d) = (src & !3, dst & !3);
    let v = bus.peek32(s);
    for _ in 0..count {
        let value = if fill { v } else { bus.peek32(s) };
        bus.poke32(d, value);
        if !fill {
            s += 4;
        }
        d += 4;
    }
    bus.add_cycles(u64::from(count) * 2);
}

fn bg_affine_set(bus: &mut Bus, src: u32, dst: u32, count: u32) {
    let sin = sine_table();
    let (mut s, mut d) = (src, dst);
    for _ in 0..count {
        let ox = bus.peek32(s) as i32;
        let oy = bus.peek32(s + 4) as i32;
        let cx = i32::from(bus.peek16(s + 8) as i16);
        let cy = i32::from(bus.peek16(s + 10) as i16);
        let sx = i32::from(bus.peek16(s + 12) as i16);
        let sy = i32::from(bus.peek16(s + 14) as i16);
        let theta = (bus.peek16(s + 16) >> 8) as usize;
        let sn = sin[theta];
        let cs = sin[(theta + 64) & 255];
        let pa = (sx * cs) >> 14;
        let pb = -((sx * sn) >> 14);
        let pc = (sy * sn) >> 14;
        let pd = (sy * cs) >> 14;
        bus.poke16(d, pa as u16);
        bus.poke16(d + 2, pb as u16);
        bus.poke16(d + 4, pc as u16);
        bus.poke16(d + 6, pd as u16);
        bus.poke32(d + 8, ox.wrapping_sub(pa * cx + pb * cy) as u32);
        bus.poke32(d + 12, oy.wrapping_sub(pc * cx + pd * cy) as u32);
        s += 20;
        d += 16;
    }
    bus.add_cycles(u64::from(count) * 60);
}

fn obj_affine_set(bus: &mut Bus, src: u32, dst: u32, count: u32, stride: u32) {
    let sin = sine_table();
    let (mut s, mut d) = (src, dst);
    for _ in 0..count {
        let sx = i32::from(bus.peek16(s) as i16);
        let sy = i32::from(bus.peek16(s + 2) as i16);
        let theta = (bus.peek16(s + 4) >> 8) as usize;
        let sn = sin[theta];
        let cs = sin[(theta + 64) & 255];
        bus.poke16(d, ((sx * cs) >> 14) as u16);
        bus.poke16(d + stride, (-((sx * sn) >> 14)) as u16);
        bus.poke16(d + stride * 2, ((sy * sn) >> 14) as u16);
        bus.poke16(d + stride * 3, ((sy * cs) >> 14) as u16);
        s += 8;
        d += stride * 4;
    }
    bus.add_cycles(u64::from(count) * 40);
}

fn bit_unpack(bus: &mut Bus, src: u32, dst: u32, info: u32) {
    let len = bus.peek16(info) as u32;
    let src_width = u32::from(bus.peek8(info + 2));
    let dst_width = u32::from(bus.peek8(info + 3));
    let offset_word = bus.peek32(info + 4);
    let offset = offset_word & 0x7FFF_FFFF;
    let zero_flag = offset_word & 0x8000_0000 != 0;
    if !matches!(src_width, 1 | 2 | 4 | 8) || !matches!(dst_width, 1 | 2 | 4 | 8 | 16 | 32) {
        return;
    }
    let mut out: u32 = 0;
    let mut out_bits = 0;
    let mut d = dst & !3;
    for i in 0..len {
        let byte = u32::from(bus.peek8(src + i));
        let mut b = 0;
        while b < 8 {
            let mut v = (byte >> b) & ((1 << src_width) - 1);
            if v != 0 || zero_flag {
                v = v.wrapping_add(offset);
            }
            out |=
                (v & (if dst_width == 32 {
                    u32::MAX
                } else {
                    (1 << dst_width) - 1
                })) << out_bits;
            out_bits += dst_width;
            if out_bits >= 32 {
                bus.poke32(d, out);
                d += 4;
                out = 0;
                out_bits = 0;
            }
            b += src_width;
        }
    }
    bus.add_cycles(u64::from(len) * 16);
}

/// Write decompressed bytes, using halfword writes for VRAM variants.
fn write_output(bus: &mut Bus, dst: u32, data: &[u8], vram: bool) {
    if vram {
        for (i, pair) in data.chunks(2).enumerate() {
            let lo = pair[0];
            let hi = if pair.len() > 1 { pair[1] } else { 0 };
            bus.poke16(dst + i as u32 * 2, u16::from_le_bytes([lo, hi]));
        }
    } else {
        for (i, &b) in data.iter().enumerate() {
            bus.poke8(dst + i as u32, b);
        }
    }
    bus.add_cycles(data.len() as u64 * 4);
}

fn lz77(bus: &mut Bus, src: u32, dst: u32, vram: bool) {
    let header = bus.peek32(src);
    let size = (header >> 8) as usize;
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut s = src + 4;
    while out.len() < size {
        let flags = bus.peek8(s);
        s += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags & (1 << bit) == 0 {
                out.push(bus.peek8(s));
                s += 1;
            } else {
                let b0 = usize::from(bus.peek8(s));
                let b1 = usize::from(bus.peek8(s + 1));
                s += 2;
                let len = (b0 >> 4) + 3;
                let disp = (((b0 & 0xF) << 8) | b1) + 1;
                for _ in 0..len {
                    if out.len() >= size {
                        break;
                    }
                    let v = if disp <= out.len() {
                        out[out.len() - disp]
                    } else {
                        0
                    };
                    out.push(v);
                }
            }
        }
    }
    write_output(bus, dst, &out, vram);
}

fn huffman(bus: &mut Bus, src: u32, dst: u32) {
    let header = bus.peek32(src);
    let bits = header & 0xF;
    let size = (header >> 8) as usize;
    if bits != 4 && bits != 8 {
        return;
    }
    let tree_size = (u32::from(bus.peek8(src + 4)) + 1) * 2;
    let tree = src + 5;
    let mut stream = src + 4 + tree_size;
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut node = tree;
    let mut pending: u32 = 0;
    let mut pending_bits = 0;
    while out.len() < size {
        let word = bus.peek32(stream);
        stream += 4;
        for bit in (0..32).rev() {
            let n = u32::from(bus.peek8(node));
            let offset = n & 0x3F;
            let next = (node & !1) + offset * 2 + 2;
            let dir = (word >> bit) & 1;
            let child = next + dir;
            let is_leaf = n & (if dir == 0 { 0x80 } else { 0x40 }) != 0;
            if is_leaf {
                let value = u32::from(bus.peek8(child));
                pending |= value << pending_bits;
                pending_bits += bits;
                if pending_bits >= 8 {
                    out.push(pending as u8);
                    pending >>= 8;
                    pending_bits -= 8;
                }
                node = tree;
                if out.len() >= size {
                    break;
                }
            } else {
                node = child;
            }
        }
    }
    // Huffman output is written in 32-bit units.
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    for (i, w) in out.chunks(4).enumerate() {
        bus.poke32(
            dst + i as u32 * 4,
            u32::from_le_bytes([w[0], w[1], w[2], w[3]]),
        );
    }
    bus.add_cycles(size as u64 * 8);
}

fn run_length(bus: &mut Bus, src: u32, dst: u32, vram: bool) {
    let header = bus.peek32(src);
    let size = (header >> 8) as usize;
    let mut out = Vec::with_capacity(size);
    let mut s = src + 4;
    while out.len() < size {
        let flag = bus.peek8(s);
        s += 1;
        if flag & 0x80 != 0 {
            let len = usize::from(flag & 0x7F) + 3;
            let v = bus.peek8(s);
            s += 1;
            for _ in 0..len {
                out.push(v);
            }
        } else {
            let len = usize::from(flag & 0x7F) + 1;
            for _ in 0..len {
                out.push(bus.peek8(s));
                s += 1;
            }
        }
    }
    out.truncate(size);
    write_output(bus, dst, &out, vram);
}

fn diff_filter(bus: &mut Bus, src: u32, dst: u32, sixteen: bool, vram: bool) {
    let header = bus.peek32(src);
    let size = (header >> 8) as usize;
    let mut out = Vec::with_capacity(size);
    if sixteen {
        let mut acc: u16 = 0;
        let mut s = src + 4;
        while out.len() < size {
            acc = acc.wrapping_add(bus.peek16(s));
            out.extend_from_slice(&acc.to_le_bytes());
            s += 2;
        }
    } else {
        let mut acc: u8 = 0;
        for i in 0..size as u32 {
            acc = acc.wrapping_add(bus.peek8(src + 4 + i));
            out.push(acc);
        }
    }
    out.truncate(size);
    write_output(bus, dst, &out, vram);
}

fn midi_key_to_freq(bus: &mut Bus, cpu: &mut Cpu) {
    let freq = bus.peek32(cpu.r[0] + 4);
    let key = cpu.r[1] as f32;
    let fine = (cpu.r[2] & 0xFF) as f32 / 256.0;
    let exponent = (180.0 - key - fine) / 12.0;
    cpu.r[0] = (freq as f32 / exponent.exp2()) as u32;
    bus.add_cycles(60);
}
