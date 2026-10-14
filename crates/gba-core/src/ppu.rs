//! Software PPU (SPEC-058): scanline renderer producing a raw 240×160
//! BGR555 framebuffer (VID-01).

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;

const TRANSPARENT: u32 = 0x8000_0000;

/// Layer ids used for priority and blending: BG0–BG3, OBJ, backdrop.
const LAYER_OBJ: usize = 4;
const LAYER_BD: usize = 5;

#[derive(Clone, Copy, Default)]
struct ObjPixel {
    color: u16,
    priority: u8,
    opaque: bool,
    semi: bool,
}

pub struct Ppu {
    pub palette: Box<[u8]>,
    pub vram: Box<[u8]>,
    pub oam: Box<[u8]>,
    pub dispcnt: u16,
    pub dispstat: u16,
    pub vcount: u16,
    pub framebuffer: Box<[u16]>,
    /// Internal affine reference points for BG2/BG3 (fixed 20.8, signed).
    ref_x: [i32; 2],
    ref_y: [i32; 2],
    last_pb: [i32; 2],
    last_pd: [i32; 2],
    bg_line: [[u32; SCREEN_WIDTH]; 4],
    obj_line: [ObjPixel; SCREEN_WIDTH],
    objwin_line: [bool; SCREEN_WIDTH],
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

fn reg16(io: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([io[offset], io[offset + 1]])
}

fn reg32(io: &[u8], offset: usize) -> u32 {
    u32::from(reg16(io, offset)) | (u32::from(reg16(io, offset + 2)) << 16)
}

/// Sign-extend the 28-bit affine reference point register.
fn ref_point(value: u32) -> i32 {
    ((value << 4) as i32) >> 4
}

const OBJ_SIZES: [[(u32, u32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

impl Ppu {
    pub fn new() -> Self {
        Self {
            palette: vec![0; 0x400].into_boxed_slice(),
            vram: vec![0; 0x18000].into_boxed_slice(),
            oam: vec![0; 0x400].into_boxed_slice(),
            dispcnt: 0x0080,
            dispstat: 0,
            vcount: 0,
            framebuffer: vec![0x7FFF; SCREEN_WIDTH * SCREEN_HEIGHT].into_boxed_slice(),
            ref_x: [0; 2],
            ref_y: [0; 2],
            last_pb: [0; 2],
            last_pd: [0; 2],
            bg_line: [[TRANSPARENT; SCREEN_WIDTH]; 4],
            obj_line: [ObjPixel::default(); SCREEN_WIDTH],
            objwin_line: [false; SCREEN_WIDTH],
        }
    }

    // ---------------------------------------------------------------- memory

    #[inline]
    fn vram_offset(addr: u32) -> usize {
        let a = (addr & 0x1FFFF) as usize;
        if a >= 0x18000 { a - 0x8000 } else { a }
    }

    pub fn read_palette16(&self, addr: u32) -> u16 {
        let a = (addr & 0x3FE) as usize;
        u16::from_le_bytes([self.palette[a], self.palette[a + 1]])
    }

    pub fn write_palette16(&mut self, addr: u32, value: u16) {
        let a = (addr & 0x3FE) as usize;
        self.palette[a..a + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_palette8(&mut self, addr: u32, value: u8) {
        let v = u16::from(value) * 0x0101;
        self.write_palette16(addr, v);
    }

    pub fn read_vram16(&self, addr: u32) -> u16 {
        let a = Self::vram_offset(addr) & !1;
        u16::from_le_bytes([self.vram[a], self.vram[a + 1]])
    }

    pub fn write_vram16(&mut self, addr: u32, value: u16) {
        let a = Self::vram_offset(addr) & !1;
        self.vram[a..a + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_vram8(&mut self, addr: u32, value: u8) {
        let a = Self::vram_offset(addr);
        let bitmap = (self.dispcnt & 7) >= 3;
        let obj_start = if bitmap { 0x14000 } else { 0x10000 };
        if a >= obj_start {
            return;
        }
        let a = a & !1;
        self.vram[a] = value;
        self.vram[a + 1] = value;
    }

    pub fn read_oam16(&self, addr: u32) -> u16 {
        let a = (addr & 0x3FE) as usize;
        u16::from_le_bytes([self.oam[a], self.oam[a + 1]])
    }

    pub fn write_oam16(&mut self, addr: u32, value: u16) {
        let a = (addr & 0x3FE) as usize;
        self.oam[a..a + 2].copy_from_slice(&value.to_le_bytes());
    }

    // ---------------------------------------------------------------- affine

    /// Reload an internal reference point after a CPU write to BGxX/BGxY.
    pub fn latch_reference(&mut self, io: &[u8], offset: u32) {
        let bg = usize::from(offset >= 0x38);
        let base = if bg == 0 { 0x28 } else { 0x38 };
        if offset as usize == base {
            self.ref_x[bg] = ref_point(reg32(io, base));
        } else {
            self.ref_y[bg] = ref_point(reg32(io, base + 4));
        }
    }

    pub fn vblank_latch(&mut self, io: &[u8]) {
        for bg in 0..2 {
            let base = 0x28 + bg * 0x10;
            self.ref_x[bg] = ref_point(reg32(io, base));
            self.ref_y[bg] = ref_point(reg32(io, base + 4));
        }
    }

    pub fn advance_affine(&mut self) {
        // Called at the start of each visible line after line 0; the io
        // values are read during render, so increments use the stored
        // PB/PD captured at the previous render.
        for bg in 0..2 {
            self.ref_x[bg] = self.ref_x[bg].wrapping_add(self.last_pb[bg]);
            self.ref_y[bg] = self.ref_y[bg].wrapping_add(self.last_pd[bg]);
        }
    }

    // ---------------------------------------------------------------- render

    fn color(&self, index: usize) -> u16 {
        u16::from_le_bytes([self.palette[index * 2], self.palette[index * 2 + 1]]) & 0x7FFF
    }

    pub fn render_line(&mut self, io: &[u8]) {
        let line = self.vcount as usize;
        let dispcnt = reg16(io, 0);
        self.dispcnt = dispcnt;
        for bg in 0..2 {
            let base = 0x20 + bg * 0x10;
            self.last_pb[bg] = i32::from(reg16(io, base + 2) as i16);
            self.last_pd[bg] = i32::from(reg16(io, base + 6) as i16);
        }
        let out = &mut [0u16; SCREEN_WIDTH];
        if dispcnt & 0x80 != 0 {
            out.fill(0x7FFF);
            self.framebuffer[line * SCREEN_WIDTH..(line + 1) * SCREEN_WIDTH].copy_from_slice(out);
            return;
        }
        let mode = dispcnt & 7;
        let mosaic = reg16(io, 0x4C);
        let mut enabled = [false; 4];
        for (bg, slot) in enabled.iter_mut().enumerate() {
            let on = dispcnt & (0x100 << bg) != 0;
            *slot = on
                && match mode {
                    0 => true,
                    1 => bg <= 2,
                    2 => bg >= 2,
                    3..=5 => bg == 2,
                    _ => false,
                };
        }
        for bg in 0..4 {
            if !enabled[bg] {
                continue;
            }
            let cnt = reg16(io, 0x08 + bg * 2);
            match (mode, bg) {
                (0, _) | (1, 0) | (1, 1) => self.render_text(io, bg, cnt, line, mosaic),
                (1, 2) | (2, _) => self.render_affine(io, bg, cnt, mosaic),
                (3..=5, 2) => self.render_bitmap(io, mode, cnt, mosaic),
                _ => {}
            }
        }
        self.obj_line.fill(ObjPixel::default());
        self.objwin_line.fill(false);
        if dispcnt & 0x1000 != 0 {
            self.render_objects(io, line, mode, dispcnt, mosaic);
        }
        self.compose(io, line, enabled, dispcnt, out);
        self.framebuffer[line * SCREEN_WIDTH..(line + 1) * SCREEN_WIDTH].copy_from_slice(out);
    }

    fn render_text(&mut self, io: &[u8], bg: usize, cnt: u16, line: usize, mosaic: u16) {
        let hofs = (reg16(io, 0x10 + bg * 4) & 0x1FF) as usize;
        let vofs = (reg16(io, 0x12 + bg * 4) & 0x1FF) as usize;
        let char_base = ((cnt >> 2) & 3) as usize * 0x4000;
        let screen_base = ((cnt >> 8) & 0x1F) as usize * 0x800;
        let color256 = cnt & 0x80 != 0;
        let size = (cnt >> 14) as usize;
        let width = if size & 1 != 0 { 512 } else { 256 };
        let height = if size & 2 != 0 { 512 } else { 256 };
        let (mos_h, mos_v) = if cnt & 0x40 != 0 {
            (
                (mosaic & 0xF) as usize + 1,
                ((mosaic >> 4) & 0xF) as usize + 1,
            )
        } else {
            (1, 1)
        };
        let src_line = line - line % mos_v;
        let y = (src_line + vofs) % height;
        let mut out = [TRANSPARENT; SCREEN_WIDTH];
        for (sx, slot) in out.iter_mut().enumerate() {
            let mx = sx - sx % mos_h;
            let x = (mx + hofs) % width;
            let mut block = (y / 256) * (width / 256) + x / 256;
            if width == 256 && height == 512 {
                block = y / 256;
            }
            let entry_addr =
                screen_base + block * 0x800 + ((y % 256) / 8) * 64 + ((x % 256) / 8) * 2;
            if entry_addr + 1 >= 0x10000 {
                continue;
            }
            let entry = u16::from_le_bytes([self.vram[entry_addr], self.vram[entry_addr + 1]]);
            let tile = (entry & 0x3FF) as usize;
            let mut px = x % 8;
            let mut py = y % 8;
            if entry & 0x400 != 0 {
                px = 7 - px;
            }
            if entry & 0x800 != 0 {
                py = 7 - py;
            }
            if color256 {
                let addr = char_base + tile * 64 + py * 8 + px;
                if addr >= 0x10000 {
                    continue;
                }
                let index = self.vram[addr] as usize;
                if index != 0 {
                    *slot = u32::from(self.color(index));
                }
            } else {
                let addr = char_base + tile * 32 + py * 4 + px / 2;
                if addr >= 0x10000 {
                    continue;
                }
                let byte = self.vram[addr];
                let index = (if px & 1 != 0 { byte >> 4 } else { byte & 0xF }) as usize;
                if index != 0 {
                    let pal = (entry >> 12) as usize;
                    *slot = u32::from(self.color(pal * 16 + index));
                }
            }
        }
        self.bg_line[bg] = out;
    }

    fn render_affine(&mut self, io: &[u8], bg: usize, cnt: u16, mosaic: u16) {
        let i = bg - 2;
        let base = 0x20 + i * 0x10;
        let pa = i32::from(reg16(io, base) as i16);
        let pc = i32::from(reg16(io, base + 4) as i16);
        let char_base = ((cnt >> 2) & 3) as usize * 0x4000;
        let screen_base = ((cnt >> 8) & 0x1F) as usize * 0x800;
        let wrap = cnt & 0x2000 != 0;
        let size = 128i32 << (cnt >> 14);
        let tiles = size / 8;
        let mos_h = if cnt & 0x40 != 0 {
            (mosaic & 0xF) as usize + 1
        } else {
            1
        };
        let mut out = [TRANSPARENT; SCREEN_WIDTH];
        for (sx, slot) in out.iter_mut().enumerate() {
            let mx = (sx - sx % mos_h) as i32;
            let mut tx = (self.ref_x[i] + pa * mx) >> 8;
            let mut ty = (self.ref_y[i] + pc * mx) >> 8;
            if wrap {
                tx = tx.rem_euclid(size);
                ty = ty.rem_euclid(size);
            } else if tx < 0 || ty < 0 || tx >= size || ty >= size {
                continue;
            }
            let map_addr = screen_base + ((ty / 8) * tiles + tx / 8) as usize;
            if map_addr >= 0x10000 {
                continue;
            }
            let tile = self.vram[map_addr] as usize;
            let addr = char_base + tile * 64 + (ty % 8) as usize * 8 + (tx % 8) as usize;
            if addr >= 0x10000 {
                continue;
            }
            let index = self.vram[addr] as usize;
            if index != 0 {
                *slot = u32::from(self.color(index));
            }
        }
        self.bg_line[bg] = out;
    }

    fn render_bitmap(&mut self, io: &[u8], mode: u16, cnt: u16, mosaic: u16) {
        let pa = i32::from(reg16(io, 0x20) as i16);
        let pc = i32::from(reg16(io, 0x24) as i16);
        let frame = if self.dispcnt & 0x10 != 0 { 0xA000 } else { 0 };
        let (w, h) = if mode == 5 { (160, 128) } else { (240, 160) };
        let mos_h = if cnt & 0x40 != 0 {
            (mosaic & 0xF) as usize + 1
        } else {
            1
        };
        let mut out = [TRANSPARENT; SCREEN_WIDTH];
        for (sx, slot) in out.iter_mut().enumerate() {
            let mx = (sx - sx % mos_h) as i32;
            let tx = (self.ref_x[0] + pa * mx) >> 8;
            let ty = (self.ref_y[0] + pc * mx) >> 8;
            if tx < 0 || ty < 0 || tx >= w || ty >= h {
                continue;
            }
            let p = (ty * w + tx) as usize;
            match mode {
                3 => {
                    let a = p * 2;
                    *slot =
                        u32::from(u16::from_le_bytes([self.vram[a], self.vram[a + 1]]) & 0x7FFF);
                }
                4 => {
                    let index = self.vram[frame + p] as usize;
                    if index != 0 {
                        *slot = u32::from(self.color(index));
                    }
                }
                _ => {
                    let a = frame + p * 2;
                    *slot =
                        u32::from(u16::from_le_bytes([self.vram[a], self.vram[a + 1]]) & 0x7FFF);
                }
            }
        }
        self.bg_line[2] = out;
    }

    fn render_objects(&mut self, io: &[u8], line: usize, mode: u16, dispcnt: u16, mosaic: u16) {
        let one_d = dispcnt & 0x40 != 0;
        let mos_h = ((mosaic >> 8) & 0xF) as i32 + 1;
        let mos_v = ((mosaic >> 12) & 0xF) as i32 + 1;
        let _ = io;
        for n in 0..128 {
            let base = n * 8;
            let attr0 = u16::from_le_bytes([self.oam[base], self.oam[base + 1]]);
            let attr1 = u16::from_le_bytes([self.oam[base + 2], self.oam[base + 3]]);
            let attr2 = u16::from_le_bytes([self.oam[base + 4], self.oam[base + 5]]);
            let affine = attr0 & 0x100 != 0;
            let double = attr0 & 0x200 != 0;
            if !affine && double {
                continue; // OBJ disabled.
            }
            let obj_mode = (attr0 >> 10) & 3;
            if obj_mode == 3 {
                continue;
            }
            let shape = (attr0 >> 14) as usize;
            if shape == 3 {
                continue;
            }
            let (w, h) = OBJ_SIZES[shape][(attr1 >> 14) as usize];
            let (w, h) = (w as i32, h as i32);
            let (box_w, box_h) = if affine && double {
                (w * 2, h * 2)
            } else {
                (w, h)
            };
            let mut y = i32::from(attr0 & 0xFF);
            if y >= 160 {
                y -= 256;
            }
            let ly = line as i32;
            if ly < y || ly >= y + box_h {
                continue;
            }
            let mut x = i32::from(attr1 & 0x1FF);
            if x >= 240 {
                x -= 512;
            }
            let color256 = attr0 & 0x2000 != 0;
            let tile_base = (attr2 & 0x3FF) as i32;
            if mode >= 3 && tile_base < 512 {
                continue;
            }
            let priority = ((attr2 >> 10) & 3) as u8;
            let pal = ((attr2 >> 12) & 0xF) as usize;
            let obj_mosaic = attr0 & 0x1000 != 0;
            let mut row = ly - y;
            if obj_mosaic {
                row -= row % mos_v;
            }
            let (pa, pb, pc, pd) = if affine {
                let group = ((attr1 >> 9) & 0x1F) as usize * 32;
                let p = |o: usize| {
                    i32::from(i16::from_le_bytes([
                        self.oam[group + o],
                        self.oam[group + o + 1],
                    ]))
                };
                (p(6), p(14), p(22), p(30))
            } else {
                (0, 0, 0, 0)
            };
            for bx in 0..box_w {
                let sx = x + bx;
                if !(0..240).contains(&sx) {
                    continue;
                }
                let (tx, ty) = if affine {
                    let dx = bx - box_w / 2;
                    let dy = row - box_h / 2;
                    let tx = ((pa * dx + pb * dy) >> 8) + w / 2;
                    let ty = ((pc * dx + pd * dy) >> 8) + h / 2;
                    if tx < 0 || ty < 0 || tx >= w || ty >= h {
                        continue;
                    }
                    (tx, ty)
                } else {
                    let mut tx = bx;
                    let mut ty = row;
                    if attr1 & 0x1000 != 0 {
                        tx = w - 1 - tx;
                    }
                    if attr1 & 0x2000 != 0 {
                        ty = h - 1 - ty;
                    }
                    (tx, ty)
                };
                let tx = if obj_mosaic { tx - tx % mos_h } else { tx };
                let factor = if color256 { 2 } else { 1 };
                let row_stride = if one_d { (w / 8) * factor } else { 32 };
                let tile = tile_base + (ty / 8) * row_stride + (tx / 8) * factor;
                let tile = (tile & 0x3FF) as usize;
                let index = if color256 {
                    let addr = 0x10000 + tile * 32 + (ty % 8) as usize * 8 + (tx % 8) as usize;
                    self.vram[addr.min(0x17FFF)] as usize
                } else {
                    let addr = 0x10000 + tile * 32 + (ty % 8) as usize * 4 + (tx % 8) as usize / 2;
                    let byte = self.vram[addr.min(0x17FFF)];
                    (if tx & 1 != 0 { byte >> 4 } else { byte & 0xF }) as usize
                };
                if index == 0 {
                    continue;
                }
                let sxu = sx as usize;
                if obj_mode == 2 {
                    self.objwin_line[sxu] = true;
                    continue;
                }
                let color = if color256 {
                    self.color(256 + index)
                } else {
                    self.color(256 + pal * 16 + index)
                };
                let slot = &mut self.obj_line[sxu];
                if !slot.opaque || priority < slot.priority {
                    *slot = ObjPixel {
                        color,
                        priority,
                        opaque: true,
                        semi: obj_mode == 1,
                    };
                }
            }
        }
    }

    fn window_mask(&self, io: &[u8], dispcnt: u16, x: usize, line: usize) -> u8 {
        if dispcnt & 0xE000 == 0 {
            return 0x3F;
        }
        let winin = reg16(io, 0x48);
        let winout = reg16(io, 0x4A);
        for w in 0..2 {
            if dispcnt & (0x2000 << w) == 0 {
                continue;
            }
            let h = reg16(io, 0x40 + w * 2);
            let v = reg16(io, 0x44 + w * 2);
            let (x1, x2) = ((h >> 8) as usize, (h & 0xFF) as usize);
            let (y1, y2) = ((v >> 8) as usize, (v & 0xFF) as usize);
            let inside = |p: usize, a: usize, b: usize| {
                if a <= b {
                    p >= a && p < b
                } else {
                    p >= a || p < b
                }
            };
            let x2 = if x2 > 240 { 240 } else { x2 };
            let y2 = if y2 > 228 { 228 } else { y2 };
            if inside(x, x1, x2) && inside(line, y1, y2) {
                return ((winin >> (w * 8)) & 0x3F) as u8;
            }
        }
        if dispcnt & 0x8000 != 0 && self.objwin_line[x] {
            return ((winout >> 8) & 0x3F) as u8;
        }
        (winout & 0x3F) as u8
    }

    fn compose(
        &self,
        io: &[u8],
        line: usize,
        enabled: [bool; 4],
        dispcnt: u16,
        out: &mut [u16; SCREEN_WIDTH],
    ) {
        let bldcnt = reg16(io, 0x50);
        let bldalpha = reg16(io, 0x52);
        let eva = u32::from(bldalpha & 0x1F).min(16);
        let evb = u32::from((bldalpha >> 8) & 0x1F).min(16);
        let evy = u32::from(reg16(io, 0x54) & 0x1F).min(16);
        let effect = (bldcnt >> 6) & 3;
        let backdrop = self.color(0);
        let mut bg_prio = [0u8; 4];
        for (bg, p) in bg_prio.iter_mut().enumerate() {
            *p = (reg16(io, 0x08 + bg * 2) & 3) as u8;
        }
        for (x, pixel) in out.iter_mut().enumerate() {
            let mask = self.window_mask(io, dispcnt, x, line);
            // Collect the top two layers: (priority key, layer, color).
            let mut top: (u16, usize, u16) = (u16::MAX, LAYER_BD, backdrop);
            let mut second: (u16, usize, u16) = (u16::MAX, LAYER_BD, backdrop);
            let mut consider = |key: u16, layer: usize, color: u16| {
                if key < top.0 {
                    second = top;
                    top = (key, layer, color);
                } else if key < second.0 {
                    second = (key, layer, color);
                }
            };
            for bg in 0..4 {
                if !enabled[bg] || mask & (1 << bg) == 0 {
                    continue;
                }
                let c = self.bg_line[bg][x];
                if c & TRANSPARENT == 0 {
                    consider(u16::from(bg_prio[bg]) * 8 + 1 + bg as u16, bg, c as u16);
                }
            }
            let obj = self.obj_line[x];
            if obj.opaque && mask & 0x10 != 0 {
                consider(u16::from(obj.priority) * 8, LAYER_OBJ, obj.color);
            }
            let first_target = |layer: usize| bldcnt & (1 << layer) != 0;
            let second_target = |layer: usize| bldcnt & (0x100 << layer) != 0;
            let effects = mask & 0x20 != 0;
            let semi = top.1 == LAYER_OBJ && obj.semi;
            *pixel = if effects && semi && second_target(second.1) {
                blend(top.2, second.2, eva, evb)
            } else if effects && first_target(top.1) {
                match effect {
                    1 if second_target(second.1) => blend(top.2, second.2, eva, evb),
                    2 => brighten(top.2, evy),
                    3 => darken(top.2, evy),
                    _ => top.2,
                }
            } else {
                top.2
            };
        }
    }

    /// Convert the raw framebuffer to RGBA8888 for host presentation.
    pub fn framebuffer_rgba(&self, out: &mut [u8]) {
        for (i, &c) in self.framebuffer.iter().enumerate() {
            let r = (c & 0x1F) as u8;
            let g = ((c >> 5) & 0x1F) as u8;
            let b = ((c >> 10) & 0x1F) as u8;
            out[i * 4] = (r << 3) | (r >> 2);
            out[i * 4 + 1] = (g << 3) | (g >> 2);
            out[i * 4 + 2] = (b << 3) | (b >> 2);
            out[i * 4 + 3] = 0xFF;
        }
    }
}

fn channels(c: u16) -> [u32; 3] {
    [
        u32::from(c & 0x1F),
        u32::from((c >> 5) & 0x1F),
        u32::from((c >> 10) & 0x1F),
    ]
}

fn pack(c: [u32; 3]) -> u16 {
    (c[0] | (c[1] << 5) | (c[2] << 10)) as u16
}

fn blend(a: u16, b: u16, eva: u32, evb: u32) -> u16 {
    let (a, b) = (channels(a), channels(b));
    pack([0, 1, 2].map(|i| ((a[i] * eva + b[i] * evb) >> 4).min(31)))
}

fn brighten(a: u16, evy: u32) -> u16 {
    let a = channels(a);
    pack([0, 1, 2].map(|i| a[i] + (((31 - a[i]) * evy) >> 4)))
}

fn darken(a: u16, evy: u32) -> u16 {
    let a = channels(a);
    pack([0, 1, 2].map(|i| a[i] - ((a[i] * evy) >> 4)))
}
