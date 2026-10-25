//! SPEC-058 acceptance: small synthetic scenes written through the bus,
//! checked pixel by pixel after rendering one scanline.

use crate::bus::Bus;
use crate::bus::tests::test_bus;
use crate::ppu::{MarginCounts, MarginKind};

const IO: u32 = 0x0400_0000;
const PAL: u32 = 0x0500_0000;
const VRAM: u32 = 0x0600_0000;
const OAM: u32 = 0x0700_0000;

const BD: u16 = 0x0421;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;

fn scene() -> Bus {
    let mut bus = test_bus();
    bus.poke16(PAL, BD);
    for n in 0..128 {
        bus.poke16(OAM + n * 8, 0x0200); // OBJ disabled
    }
    bus.poke16(PAL + 2, RED);
    bus.poke16(PAL + 4, GREEN);
    bus.poke16(PAL + 6, BLUE);
    bus
}

fn render(bus: &mut Bus, y: u16) -> Vec<u16> {
    bus.ppu.vcount = y;
    bus.ppu.render_line(&bus.io);
    let y = usize::from(y);
    bus.ppu.framebuffer[y * 240..(y + 1) * 240].to_vec()
}

fn reg(bus: &mut Bus, offset: u32, value: u16) {
    bus.poke16(IO + offset, value);
}

fn set_pixel4(bus: &mut Bus, base: u32, tile: u32, x: u32, y: u32, index: u16) {
    let addr = base + tile * 32 + y * 4 + x / 2;
    let half = bus.peek16(addr & !1);
    let shift = (addr & 1) * 8 + (x & 1) * 4;
    let half = (half & !(0xF << shift)) | (index << shift);
    bus.poke16(addr & !1, half);
}

fn set_pixel8(bus: &mut Bus, base: u32, tile: u32, x: u32, y: u32, index: u16) {
    let addr = base + tile * 64 + y * 8 + x;
    let half = bus.peek16(addr & !1);
    let shift = (addr & 1) * 8;
    bus.poke16(addr & !1, (half & !(0xFF << shift)) | (index << shift));
}

fn solid4(bus: &mut Bus, base: u32, tile: u32, index: u16) {
    let v = index * 0x1111;
    for i in 0..16 {
        bus.poke16(base + tile * 32 + i * 2, v);
    }
}

fn map_entry(bus: &mut Bus, screen_block: u32, tx: u32, ty: u32, entry: u16) {
    bus.poke16(VRAM + screen_block * 0x800 + (ty * 32 + tx) * 2, entry);
}

/// BG0: screen block 31, char block 0, tile 1 with (0,0)=1, (7,0)=2, (0,7)=3.
fn text_scene() -> Bus {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0100);
    reg(&mut bus, 0x008, 0x1F00);
    set_pixel4(&mut bus, VRAM, 1, 0, 0, 1);
    set_pixel4(&mut bus, VRAM, 1, 7, 0, 2);
    set_pixel4(&mut bus, VRAM, 1, 0, 7, 3);
    bus
}

// ------------------------------------------------------------ text BGs

#[test]
fn text_bg_4bpp_tile_and_flips() {
    let mut bus = text_scene();
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[0], l0[1], l0[7], l0[8]], [RED, BD, GREEN, BD]);
    assert_eq!(render(&mut bus, 7)[0], BLUE);

    map_entry(&mut bus, 31, 0, 0, 0x0401); // H flip
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[0], l0[7]], [GREEN, RED]);
    map_entry(&mut bus, 31, 0, 0, 0x0801); // V flip
    assert_eq!(render(&mut bus, 0)[0], BLUE);
    assert_eq!(render(&mut bus, 7)[0], RED);
    map_entry(&mut bus, 31, 0, 0, 0x0C01); // both
    assert_eq!(render(&mut bus, 0)[7], BLUE);
}

#[test]
fn text_bg_palette_bank() {
    let mut bus = text_scene();
    bus.poke16(PAL + 2 * 33, 0x1234);
    map_entry(&mut bus, 31, 0, 0, 0x2001);
    assert_eq!(render(&mut bus, 0)[0], 0x1234);
}

#[test]
fn text_bg_scroll_and_wrap() {
    let mut bus = text_scene();
    map_entry(&mut bus, 31, 1, 1, 0x0001); // tile pixel (0,0) at map (8,8)
    reg(&mut bus, 0x010, 3);
    reg(&mut bus, 0x012, 2);
    assert_eq!(render(&mut bus, 6)[5], RED);
    assert_eq!(render(&mut bus, 8)[5], BD);
    // Offsets wrap around the 256×256 map (and only 9 bits are used).
    reg(&mut bus, 0x010, 0x1FA);
    reg(&mut bus, 0x012, 0xFE);
    assert_eq!(render(&mut bus, 10)[14], RED);
}

#[test]
fn text_bg_8bpp_ignores_palette_bank() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0100);
    reg(&mut bus, 0x008, 0x1F80);
    set_pixel8(&mut bus, VRAM, 1, 0, 0, 200);
    set_pixel8(&mut bus, VRAM, 1, 3, 0, 201);
    bus.poke16(PAL + 400, 0x5555);
    bus.poke16(PAL + 402, 0x2AAA);
    map_entry(&mut bus, 31, 0, 0, 0xF001);
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[0], l0[1], l0[3]], [0x5555, BD, 0x2AAA]);
}

#[test]
fn text_bg_512_wide_uses_second_screen_block() {
    let mut bus = text_scene();
    reg(&mut bus, 0x008, 0x5C00); // size 1 (512×256), screen block 28
    map_entry(&mut bus, 29, 0, 0, 0x0001);
    assert_eq!(render(&mut bus, 0)[0], BD);
    reg(&mut bus, 0x010, 256);
    assert_eq!(render(&mut bus, 0)[0], RED);
}

#[test]
fn bg_priority_between_layers() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0300);
    solid4(&mut bus, VRAM, 3, 1);
    solid4(&mut bus, VRAM, 4, 3);
    reg(&mut bus, 0x008, 0x1F01); // BG0 prio 1, block 31
    reg(&mut bus, 0x00A, 0x1E00); // BG1 prio 0, block 30
    map_entry(&mut bus, 31, 0, 0, 3);
    map_entry(&mut bus, 30, 0, 0, 4);
    assert_eq!(render(&mut bus, 0)[0], BLUE);
    reg(&mut bus, 0x00A, 0x1E01); // same priority: lower BG index wins
    assert_eq!(render(&mut bus, 0)[0], RED);
    // A transparent upper layer shows the lower one.
    map_entry(&mut bus, 31, 0, 0, 0);
    assert_eq!(render(&mut bus, 0)[0], BLUE);
}

#[test]
fn bg_mosaic() {
    let mut bus = text_scene();
    set_pixel4(&mut bus, VRAM, 1, 1, 0, 2);
    set_pixel4(&mut bus, VRAM, 1, 4, 0, 3);
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[1], l0[4]], [GREEN, BLUE]);
    reg(&mut bus, 0x008, 0x1F40);
    reg(&mut bus, 0x04C, 0x0033); // 4×4 blocks
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[0], l0[1], l0[3], l0[4]], [RED, RED, RED, BLUE]);
    assert_eq!(
        render(&mut bus, 3)[0],
        RED,
        "vertical mosaic repeats line 0"
    );
    assert_eq!(render(&mut bus, 7)[0], BD, "line 7 samples line 4");
}

// ------------------------------------------------------------ affine BGs

fn affine_scene() -> Bus {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0401); // mode 1, BG2
    reg(&mut bus, 0x00C, 0x1004); // 128×128, char block 1, screen block 16
    for i in 0..32 {
        bus.poke16(VRAM + 0x4040 + i * 2, 0x0505); // tile 1: all index 5
    }
    bus.poke16(PAL + 10, GREEN);
    bus.poke16(VRAM + 0x8000, 0x0001); // map (0,0) = tile 1
    bus
}

#[test]
fn affine_bg_identity_wrap_and_no_wrap() {
    let mut bus = affine_scene();
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[0], l0[7], l0[8], l0[128]], [GREEN, GREEN, BD, BD]);
    reg(&mut bus, 0x00C, 0x3004); // wrap
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[127], l0[128], l0[135], l0[136]], [BD, GREEN, GREEN, BD]);
}

#[test]
fn affine_bg_reference_point_and_scale() {
    let mut bus = affine_scene();
    bus.poke32(IO + 0x28, 4 << 8);
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[3], l0[4]], [GREEN, BD]);
    bus.poke32(IO + 0x28, 0);
    reg(&mut bus, 0x020, 0x0080); // PA = 0.5
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[15], l0[16]], [GREEN, BD]);
    // Negative reference points are sign-extended from 28 bits.
    reg(&mut bus, 0x020, 0x0100);
    bus.poke32(IO + 0x28, 0x0FFF_FC00); // -4.0
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[3], l0[4], l0[11], l0[12]], [BD, GREEN, GREEN, BD]);
}

#[test]
fn affine_reference_advances_per_line_and_reloads_on_vblank() {
    let mut bus = affine_scene();
    for y in 0..8 {
        assert_eq!(render(&mut bus, y)[0], GREEN, "line {y}");
        bus.ppu.advance_affine();
    }
    assert_eq!(render(&mut bus, 8)[0], BD);
    bus.ppu.vblank_latch(&bus.io);
    assert_eq!(
        render(&mut bus, 8)[0],
        GREEN,
        "reference reloaded from BG2Y"
    );
}

// ------------------------------------------------------------ bitmap modes

#[test]
fn mode3_direct_color() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0403);
    bus.poke16(VRAM + 5 * 2, 0x1234);
    bus.poke16(VRAM + (2 * 240 + 7) * 2, 0x4321);
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[5], l0[6]], [0x1234, 0]);
    bus.poke32(IO + 0x2C, 2 << 8);
    assert_eq!(render(&mut bus, 2)[7], 0x4321);
}

#[test]
fn mode4_paletted_with_page_flip() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0404);
    bus.poke16(PAL + 14, BLUE);
    bus.poke16(PAL + 16, RED);
    bus.poke16(VRAM + 10, 0x0007);
    bus.poke16(VRAM + 0xA000 + 10, 0x0008);
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[10], l0[11]], [BLUE, BD]);
    reg(&mut bus, 0x000, 0x0414);
    assert_eq!(render(&mut bus, 0)[10], RED);
}

#[test]
fn mode5_small_bitmap_with_page_flip() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0405);
    bus.poke16(VRAM + 3 * 2, 0x2222);
    bus.poke16(VRAM + 0xA000 + 3 * 2, 0x3333);
    bus.poke16(VRAM + 159 * 2, 0x1111);
    let l0 = render(&mut bus, 0);
    assert_eq!([l0[3], l0[159], l0[160]], [0x2222, 0x1111, BD]);
    reg(&mut bus, 0x000, 0x0415);
    assert_eq!(render(&mut bus, 0)[3], 0x3333);
    bus.poke32(IO + 0x2C, 128 << 8);
    assert_eq!(render(&mut bus, 128)[3], BD, "only 128 lines");
}

// ------------------------------------------------------------ OBJs

const OBJ_VRAM: u32 = VRAM + 0x10000;

fn obj_scene() -> Bus {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x1040); // OBJ on, 1D mapping
    bus.poke16(PAL + 0x200 + 2, RED);
    bus.poke16(PAL + 0x200 + 4, GREEN);
    bus.poke16(PAL + 0x200 + 0x20 + 2, BLUE); // OBJ palette bank 1, index 1
    set_pixel4(&mut bus, OBJ_VRAM, 0, 0, 0, 1);
    set_pixel4(&mut bus, OBJ_VRAM, 0, 7, 0, 2);
    bus
}

fn obj(bus: &mut Bus, n: u32, attr0: u16, attr1: u16, attr2: u16) {
    bus.poke16(OAM + n * 8, attr0);
    bus.poke16(OAM + n * 8 + 2, attr1);
    bus.poke16(OAM + n * 8 + 4, attr2);
}

#[test]
fn obj_normal_position_flip_and_offscreen_x() {
    let mut bus = obj_scene();
    obj(&mut bus, 0, 10, 20, 0);
    assert_eq!(render(&mut bus, 9)[20], BD);
    let l = render(&mut bus, 10);
    assert_eq!([l[20], l[21], l[27], l[28]], [RED, BD, GREEN, BD]);
    obj(&mut bus, 0, 10, 20 | 0x1000, 0);
    let l = render(&mut bus, 10);
    assert_eq!([l[20], l[27]], [GREEN, RED]);
    obj(&mut bus, 0, 10, 0x1FC, 0); // x = -4
    assert_eq!(render(&mut bus, 10)[3], GREEN);
    obj(&mut bus, 0, 250, 20, 0); // y = -6: rows 6..7 visible on lines 0..1
    set_pixel4(&mut bus, OBJ_VRAM, 0, 1, 7, 1);
    assert_eq!(render(&mut bus, 1)[21], RED);
}

#[test]
fn obj_1d_vs_2d_mapping() {
    let mut bus = obj_scene();
    set_pixel4(&mut bus, OBJ_VRAM, 2, 0, 0, 1);
    set_pixel4(&mut bus, OBJ_VRAM, 32, 0, 0, 2);
    obj(&mut bus, 0, 10, 20 | 0x4000, 0); // 16×16
    assert_eq!(render(&mut bus, 18)[20], RED, "1D: next row is tile 2");
    reg(&mut bus, 0x000, 0x1000);
    assert_eq!(render(&mut bus, 18)[20], GREEN, "2D: next row is tile 32");
}

#[test]
fn obj_8bpp() {
    let mut bus = obj_scene();
    set_pixel8(&mut bus, OBJ_VRAM, 0, 0, 0, 9);
    bus.poke16(PAL + 0x200 + 18, 0x4567);
    obj(&mut bus, 0, 10 | 0x2000, 20, 0);
    assert_eq!(render(&mut bus, 10)[20], 0x4567);
}

#[test]
fn obj_priority_between_objs_and_bgs() {
    let mut bus = obj_scene();
    obj(&mut bus, 0, 10, 20, 0x0400); // prio 1, RED
    obj(&mut bus, 1, 10, 20, 0x1000); // prio 0, palette 1 → BLUE
    assert_eq!(render(&mut bus, 10)[20], BLUE);
    obj(&mut bus, 1, 10, 20, 0x1400); // same priority: lower OAM index wins
    assert_eq!(render(&mut bus, 10)[20], RED);
    obj(&mut bus, 1, 0x0200, 0, 0);

    // BG0 (GREEN, solid tile) behind/in front of the OBJ.
    reg(&mut bus, 0x000, 0x1140);
    solid4(&mut bus, VRAM, 2, 2);
    map_entry(&mut bus, 31, 2, 1, 2);
    reg(&mut bus, 0x008, 0x1F00); // BG0 prio 0
    assert_eq!(render(&mut bus, 10)[20], GREEN);
    obj(&mut bus, 0, 10, 20, 0x0000); // OBJ prio 0 beats BG prio 0
    assert_eq!(render(&mut bus, 10)[20], RED);
    assert_eq!(render(&mut bus, 10)[21], GREEN, "transparent OBJ pixel");
}

#[test]
fn obj_affine_identity_and_double_size() {
    let mut bus = obj_scene();
    bus.poke16(OAM + 0x06, 0x0100); // PA
    bus.poke16(OAM + 0x0E, 0);
    bus.poke16(OAM + 0x16, 0);
    bus.poke16(OAM + 0x1E, 0x0100); // PD
    obj(&mut bus, 0, 10 | 0x0100, 20, 0);
    let l = render(&mut bus, 10);
    assert_eq!([l[20], l[27], l[28]], [RED, GREEN, BD]);
    // Double size: 16×16 box, the 8×8 sprite centred at +4,+4.
    obj(&mut bus, 0, 10 | 0x0300, 20, 0);
    assert_eq!(render(&mut bus, 10)[20], BD);
    let l = render(&mut bus, 14);
    assert_eq!([l[24], l[31], l[32]], [RED, GREEN, BD]);
    // Horizontal mirror through PA = -1: texel x = 8 - bx, so texel 7 lands
    // on bx = 1 and texel 1 on bx = 7 (texel 0 falls outside the box).
    bus.poke16(OAM + 0x06, 0xFF00);
    set_pixel4(&mut bus, OBJ_VRAM, 0, 1, 0, 1);
    obj(&mut bus, 0, 10 | 0x0100, 20, 0);
    let l = render(&mut bus, 10);
    assert_eq!([l[20], l[21], l[27]], [BD, GREEN, RED]);
}

#[test]
fn obj_tiles_below_512_unavailable_in_bitmap_modes() {
    let mut bus = obj_scene();
    reg(&mut bus, 0x000, 0x1043);
    obj(&mut bus, 0, 10, 20, 0);
    assert_ne!(render(&mut bus, 10)[20], RED);
    set_pixel4(&mut bus, OBJ_VRAM, 512, 0, 0, 1);
    obj(&mut bus, 0, 10, 20, 512);
    assert_eq!(render(&mut bus, 10)[20], RED);
}

#[test]
fn obj_semi_transparency_forces_alpha() {
    let mut bus = obj_scene();
    reg(&mut bus, 0x000, 0x1140);
    solid4(&mut bus, VRAM, 2, 2);
    map_entry(&mut bus, 31, 2, 1, 2);
    reg(&mut bus, 0x008, 0x1F01);
    obj(&mut bus, 0, 10 | 0x0400, 20, 0);
    reg(&mut bus, 0x052, 0x0808);
    assert_eq!(render(&mut bus, 10)[20], RED, "BG0 not a second target");
    reg(&mut bus, 0x050, 0x0100); // BG0 second target, effect none
    assert_eq!(render(&mut bus, 10)[20], 0x01EF);
}

#[test]
fn obj_window_masks_layers() {
    let mut bus = obj_scene();
    reg(&mut bus, 0x000, 0x9140); // OBJ window, OBJ, BG0
    solid4(&mut bus, VRAM, 2, 2);
    for tx in 0..32 {
        map_entry(&mut bus, 31, tx, 1, 2);
    }
    reg(&mut bus, 0x008, 0x1F00);
    obj(&mut bus, 0, 10 | 0x0800, 20, 0); // window-only OBJ
    reg(&mut bus, 0x04A, 0x0100); // outside: nothing; OBJ window: BG0
    let l = render(&mut bus, 10);
    assert_eq!([l[19], l[20], l[21], l[27]], [BD, GREEN, BD, GREEN]);
}

#[test]
fn obj_window_keeps_the_canonical_crop_with_a_shifted_view() {
    let mut bus = obj_scene();
    reg(&mut bus, 0x000, 0x9140); // OBJ window, OBJ, BG0
    solid4(&mut bus, VRAM, 2, 2);
    for tx in 0..32 {
        map_entry(&mut bus, 31, tx, 1, 2);
    }
    reg(&mut bus, 0x008, 0x1F00);
    obj(&mut bus, 0, 10 | 0x0800, 20, 0); // window-only OBJ at x 20..27
    reg(&mut bus, 0x04A, 0x0100); // outside: nothing; OBJ window: BG0

    bus.ppu.vcount = 10;
    bus.ppu.set_view(crate::ppu::View::NONE).unwrap();
    bus.ppu.render_line(&bus.io);
    let canonical = bus.ppu.framebuffer[10 * 240..11 * 240].to_vec();

    let line = render_wide(&mut bus, 10, 24, 24);
    assert_eq!(
        &line[24..264],
        canonical.as_slice(),
        "the OBJ window mask must be read at the same column it was drawn"
    );
    assert_eq!(
        &bus.ppu.framebuffer[10 * 240..11 * 240],
        canonical.as_slice(),
        "canonical buffer must equal the crop"
    );
}

// ------------------------------------------------------------ windows

fn window_scene() -> Bus {
    let mut bus = scene();
    solid4(&mut bus, VRAM, 2, 2);
    for tx in 0..32 {
        map_entry(&mut bus, 31, tx, 1, 2);
    }
    reg(&mut bus, 0x008, 0x1F00);
    bus
}

#[test]
fn win0_and_win1_masks() {
    let mut bus = window_scene();
    reg(&mut bus, 0x000, 0x2100);
    reg(&mut bus, 0x040, (10 << 8) | 20);
    reg(&mut bus, 0x044, 160);
    reg(&mut bus, 0x048, 0x0001);
    reg(&mut bus, 0x04A, 0x0000);
    let l = render(&mut bus, 10);
    assert_eq!([l[9], l[10], l[19], l[20]], [BD, GREEN, GREEN, BD]);
    // WIN1 overlapping: WIN0 has priority.
    reg(&mut bus, 0x000, 0x6100);
    reg(&mut bus, 0x042, (15 << 8) | 30);
    reg(&mut bus, 0x046, 160);
    reg(&mut bus, 0x04A, 0x0001);
    let l = render(&mut bus, 10);
    assert_eq!(
        [l[5], l[12], l[17], l[25], l[35]],
        [GREEN, GREEN, GREEN, BD, GREEN]
    );
    // Vertical bounds.
    reg(&mut bus, 0x000, 0x2100);
    reg(&mut bus, 0x044, (12 << 8) | 14);
    reg(&mut bus, 0x04A, 0x0000);
    assert_eq!(render(&mut bus, 10)[12], BD);
    assert_eq!(render(&mut bus, 12)[12], GREEN);
}

// ------------------------------------------------------------ blending

fn blend_scene() -> Bus {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0300);
    solid4(&mut bus, VRAM, 3, 1);
    solid4(&mut bus, VRAM, 4, 3);
    reg(&mut bus, 0x008, 0x1F00);
    reg(&mut bus, 0x00A, 0x1E01);
    map_entry(&mut bus, 31, 0, 0, 3);
    map_entry(&mut bus, 30, 0, 0, 4);
    bus
}

#[test]
fn alpha_blend_with_saturated_coefficients() {
    let mut bus = blend_scene();
    reg(&mut bus, 0x050, 0x0241);
    reg(&mut bus, 0x052, 0x0808);
    assert_eq!(render(&mut bus, 0)[0], 0x3C0F);
    reg(&mut bus, 0x052, 0x101F); // EVA 31 → 16
    assert_eq!(render(&mut bus, 0)[0], 0x7C1F);
    reg(&mut bus, 0x050, 0x0141); // second target BG0 only: no blend
    assert_eq!(render(&mut bus, 0)[0], RED);
    // Backdrop as second target where BG1 is absent.
    reg(&mut bus, 0x050, 0x2041);
    reg(&mut bus, 0x052, 0x1010);
    map_entry(&mut bus, 30, 0, 0, 0);
    assert_eq!(render(&mut bus, 0)[0], RED | BD);
}

#[test]
fn brighten_and_darken() {
    let mut bus = blend_scene();
    reg(&mut bus, 0x050, 0x0081);
    reg(&mut bus, 0x054, 8);
    assert_eq!(render(&mut bus, 0)[0], 0x3DFF);
    reg(&mut bus, 0x054, 31);
    assert_eq!(render(&mut bus, 0)[0], 0x7FFF);
    reg(&mut bus, 0x050, 0x00C1);
    reg(&mut bus, 0x054, 8);
    assert_eq!(render(&mut bus, 0)[0], 0x0010);
    // Only first-target layers are affected.
    reg(&mut bus, 0x050, 0x00C2);
    assert_eq!(render(&mut bus, 0)[0], RED);
}

#[test]
fn window_can_disable_color_effects() {
    let mut bus = blend_scene();
    reg(&mut bus, 0x050, 0x00C1);
    reg(&mut bus, 0x054, 16);
    reg(&mut bus, 0x000, 0x2300);
    reg(&mut bus, 0x040, 4); // WIN0 x 0..3
    reg(&mut bus, 0x044, 160);
    reg(&mut bus, 0x048, 0x0003); // inside: BG0+BG1, no effects
    reg(&mut bus, 0x04A, 0x0023); // outside: BG0+BG1+effects
    map_entry(&mut bus, 31, 0, 0, 3);
    let l = render(&mut bus, 0);
    assert_eq!([l[3], l[4]], [RED, 0]);
}

#[test]
fn forced_blank_is_white() {
    let mut bus = blend_scene();
    reg(&mut bus, 0x000, 0x0380);
    assert!(render(&mut bus, 0).iter().all(|&p| p == 0x7FFF));
}

// ------------------------------------------- SPEC-069 T-01: canonical identity

/// FNV-1a over a whole 240x160 frame.
fn frame_fnv(bus: &Bus) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &p in bus.ppu.framebuffer.iter() {
        for b in p.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01B3);
        }
    }
    h
}

/// Render all 160 lines and hash the canonical framebuffer.
fn render_frame(bus: &mut Bus) -> u64 {
    for y in 0..160u16 {
        bus.ppu.vcount = y;
        bus.ppu.render_line(&bus.io);
    }
    frame_fnv(bus)
}

fn golden_scene_text() -> Bus {
    let mut bus = text_scene();
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    map_entry(&mut bus, 31, 1, 1, 0x0101);
    reg(&mut bus, 0x010, 3);
    reg(&mut bus, 0x008, 0x1F01);
    obj(&mut bus, 0, 10, 20, 0);
    bus
}

fn golden_scene_affine() -> Bus {
    let mut bus = affine_scene();
    reg(&mut bus, 0x020, 0x0080);
    bus.poke32(IO + 0x28, 4 << 8);
    bus
}

fn golden_scene_mode2() -> Bus {
    let mut bus = affine_scene();
    reg(&mut bus, 0x000, 0x0402);
    reg(&mut bus, 0x020, 0x00C0);
    bus
}

fn golden_scene_mode3() -> Bus {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0403);
    bus.poke16(VRAM + 5 * 2, 0x1234);
    bus.poke16(VRAM + (2 * 240 + 7) * 2, 0x4321);
    bus
}

fn golden_scene_mode4() -> Bus {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0404);
    bus.poke16(PAL + 14, BLUE);
    bus.poke16(VRAM + 10, 0x0007);
    bus
}

fn golden_scene_mode5() -> Bus {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0405);
    bus.poke16(VRAM + 3 * 2, 0x2222);
    bus.poke16(VRAM + 159 * 2, 0x1111);
    bus
}

/// T-01: the canonical frame is byte-identical to the pre-SPEC-069 renderer.
///
/// These hashes were captured from the tree before `View` existed and are the
/// guard that the wide path did not move native output (WS-69-02, INV-1).
#[test]
fn canonical_frames_match_pre_view_goldens() {
    let cases: [(&str, fn() -> Bus, u64); 6] = [
        ("mode0", golden_scene_text, 0x82D9_1296_C6DA_8689),
        ("mode1", golden_scene_affine, 0x00D4_62EC_B601_1325),
        ("mode2", golden_scene_mode2, 0x10AD_2B31_B360_A525),
        ("mode3", golden_scene_mode3, 0x5D7B_E25F_393C_9725),
        ("mode4", golden_scene_mode4, 0x9D77_0531_C80F_ED25),
        ("mode5", golden_scene_mode5, 0x1740_69D9_9AD7_3165),
    ];
    for (name, build, expected) in cases {
        let mut bus = build();
        assert_eq!(
            bus.ppu.view(),
            crate::ppu::View::NONE,
            "{name}: default view"
        );
        assert_eq!(render_frame(&mut bus), expected, "{name}: canonical frame");
    }
}

/// T-01b: the same scenes are unchanged with an explicit zero view applied.
#[test]
fn explicit_none_view_matches_goldens() {
    let mut bus = golden_scene_text();
    bus.ppu.set_view(crate::ppu::View::NONE).unwrap();
    assert_eq!(render_frame(&mut bus), 0x82D9_1296_C6DA_8689);
}

/// T-13: a surface above the ceiling is refused, never clamped.
#[test]
fn view_above_maximum_is_refused() {
    let mut bus = scene();
    let too_wide = crate::ppu::View {
        left: 200,
        right: 200,
    };
    assert!(bus.ppu.set_view(too_wide).is_err());
    assert_eq!(bus.ppu.view(), crate::ppu::View::NONE, "view unchanged");
    let exact = crate::ppu::View {
        left: 72,
        right: 72,
    };
    assert_eq!(exact.width(), crate::ppu::MAX_VIEW_WIDTH);
    assert!(bus.ppu.set_view(exact).is_ok());
}

/// T-14: toggling the view between frames never changes native output.
#[test]
fn view_toggle_between_frames_keeps_canonical_output() {
    let mut bus = golden_scene_text();
    let baseline = render_frame(&mut bus);

    let mut bus = golden_scene_text();
    for y in 0..160u16 {
        bus.ppu.vcount = y;
        let wide = y % 2 == 0;
        bus.ppu
            .set_view(if wide {
                crate::ppu::View {
                    left: 24,
                    right: 24,
                }
            } else {
                crate::ppu::View::NONE
            })
            .unwrap();
        bus.ppu.render_line(&bus.io);
    }
    assert_eq!(frame_fnv(&bus), baseline, "canonical frame must not move");
}

// ------------------------------------- SPEC-069 T-02..T-04: text BG in margins

/// Render one line into the widened surface and return it.
fn render_wide(bus: &mut Bus, y: u16, left: u16, right: u16) -> Vec<u16> {
    bus.ppu.set_view(crate::ppu::View { left, right }).unwrap();
    bus.ppu.vcount = y;
    bus.ppu.render_line(&bus.io);
    bus.ppu.view_line().to_vec()
}

/// T-02: a 256-wide map wraps, so output column `256 + k` equals column `k`.
///
/// This is the structural ring relation: it holds for every row and does not
/// depend on what the scene happens to draw at those columns.
#[test]
fn text_bg_ring_wraps_into_right_margin() {
    let mut bus = text_scene();
    // Distinct content in map column 0 so the relation is observable.
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    map_entry(&mut bus, 31, 1, 0, 0x0101);
    reg(&mut bus, 0x008, 0x1F01);
    let left = 0;
    let right = 128;
    for y in [0u16, 1, 7, 8, 33, 159] {
        let line = render_wide(&mut bus, y, left, right);
        assert_eq!(line.len(), 240 + right as usize, "row {y} width");
        for k in 0..112usize {
            assert_eq!(
                line[256 + k],
                line[k],
                "row {y}: column {} must repeat column {} (256 ring)",
                256 + k,
                k
            );
        }
    }
}

/// T-03: counter-proof — on a 512-wide map the same relation must fail.
#[test]
fn text_bg_512_does_not_repeat_at_256() {
    let mut bus = text_scene();
    reg(&mut bus, 0x008, 0x5C01); // size 1 (512x256), screen block 28
    map_entry(&mut bus, 28, 0, 0, 0x0001);
    map_entry(&mut bus, 29, 0, 0, 0x0101); // map column 32, i.e. x = 256
    let line = render_wide(&mut bus, 0, 0, 128);
    assert_eq!(line.len(), 368);
    assert_ne!(
        line[256], line[0],
        "512-wide map must not repeat the 256 ring"
    );
}

/// T-02/T-03 companion: the canonical crop is unaffected by an active view.
#[test]
fn text_bg_margin_does_not_move_canonical_row() {
    let mut bus = text_scene();
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    map_entry(&mut bus, 31, 1, 1, 0x0101);
    reg(&mut bus, 0x008, 0x1F01);
    reg(&mut bus, 0x010, 5);

    bus.ppu.vcount = 0;
    bus.ppu.set_view(crate::ppu::View::NONE).unwrap();
    bus.ppu.render_line(&bus.io);
    let canonical: Vec<u16> = bus.ppu.framebuffer[..240].to_vec();

    let line = render_wide(&mut bus, 0, 24, 24);
    let crop = &line[24..24 + 240];
    assert_eq!(crop, canonical.as_slice(), "crop must equal canonical row");
    assert_eq!(
        &bus.ppu.framebuffer[..240],
        canonical.as_slice(),
        "canonical buffer must be the crop, not the wide row"
    );
}

/// T-04: mosaic snaps on the hardware column, so block edges stay on the
/// native grid even when the surface is offset.
#[test]
fn text_bg_mosaic_snaps_on_hardware_column() {
    let mut bus = text_scene();
    for x in 0..4 {
        set_pixel4(&mut bus, VRAM, 1, x, 0, 1); // RED block
    }
    set_pixel4(&mut bus, VRAM, 1, 4, 0, 2); // first pixel of the next block
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    reg(&mut bus, 0x008, 0x1F41); // BG0 mosaic enable
    reg(&mut bus, 0x04C, 0x0003); // 4x1 blocks

    let left = 3u16;
    let line = render_wide(&mut bus, 0, left, 32);
    // Hardware columns 0..3 form one mosaic block and all sample column 0;
    // the block boundary falls on hardware x = 4 despite the output offset.
    for hw in 0..4u16 {
        assert_eq!(
            line[usize::from(left) + usize::from(hw)],
            RED,
            "hardware column {hw} belongs to the first block"
        );
    }
    for hw in 4..8u16 {
        assert_eq!(
            line[usize::from(left) + usize::from(hw)],
            GREEN,
            "hardware column {hw} starts the second block"
        );
    }
}

// ------------------------------- SPEC-069 T-05..T-07: affine and bitmap margins

/// T-05: with no wrap the margin is outside the map, so it stays backdrop
/// while the canonical crop keeps the undocumented native sampling.
#[test]
fn affine_bg_margin_is_backdrop_without_wrap() {
    let mut bus = affine_scene();
    assert_eq!(bus.ppu.view(), crate::ppu::View::NONE);
    let mut baseline = Vec::new();
    for y in 0..4u16 {
        bus.ppu.vcount = y;
        bus.ppu.render_line(&bus.io);
        baseline.extend_from_slice(&bus.ppu.framebuffer[y as usize * 240..][..240]);
    }

    let mut bus = affine_scene();
    let left = 16u16;
    for y in 0..4u16 {
        let line = render_wide(&mut bus, y, left, 16);
        assert_eq!(line.len(), 272);
        assert_eq!(
            &line[usize::from(left)..usize::from(left) + 240],
            &baseline[y as usize * 240..][..240],
            "row {y}: canonical crop must match the native row"
        );
        assert!(
            line[..usize::from(left)].iter().all(|&p| p == BD),
            "row {y}: negative hardware columns are outside the map -> backdrop"
        );
    }
}

/// T-05b: a shifted reference point keeps working once the surface is offset,
/// which proves the sampling uses the hardware column and not the output one.
#[test]
fn affine_bg_shifted_reference_survives_the_offset() {
    let mut bus = affine_scene();
    reg(&mut bus, 0x020, 0x0080); // PA = 0.5
    bus.poke32(IO + 0x28, 4 << 8); // BG2X = 4.0
    bus.ppu.vblank_latch(&bus.io);

    bus.ppu.vcount = 0;
    bus.ppu.render_line(&bus.io);
    let native: Vec<u16> = bus.ppu.framebuffer[..240].to_vec();

    let left = 16u16;
    let line = render_wide(&mut bus, 0, left, 0);
    assert_eq!(
        &line[usize::from(left)..usize::from(left) + 240],
        native.as_slice(),
        "offset surface must sample the same hardware columns"
    );
}

/// T-06: with wrap enabled the margin keeps sampling modulo the map size,
/// instead of falling back to the backdrop.
#[test]
fn affine_bg_wrap_covers_the_margin() {
    let mut bus = affine_scene();
    reg(&mut bus, 0x020, 0x0100); // PA = 1.0
    reg(&mut bus, 0x024, 0x0100); // PC = 1.0
    reg(&mut bus, 0x00C, 0x3004); // wrap on, 128x128 map

    let left = 24u16;
    let line = render_wide(&mut bus, 0, left, 8);
    let at = |hw: usize| line[usize::from(left) + hw];
    // Texture column 0 is tile 1 (GREEN) for x 0..7; column 128..135 wraps to
    // the same tile, so the margin past the map edge is still drawn.
    assert_eq!([at(0), at(7)], [GREEN, GREEN]);
    assert_eq!(at(8), BD, "tile 1 is only 8 texels wide");
    assert_eq!(at(128), GREEN, "wrap keeps sampling past map width");
    assert_eq!(at(135), GREEN);
    assert_eq!(at(136), BD);
}

/// T-06b: without wrap the same columns fall back to the backdrop, so the two
/// modes are distinguishable in the margin.
#[test]
fn affine_bg_no_wrap_margin_is_backdrop() {
    let mut bus = affine_scene();
    reg(&mut bus, 0x020, 0x0100);
    reg(&mut bus, 0x024, 0x0100);
    reg(&mut bus, 0x00C, 0x1004); // wrap off

    let left = 24u16;
    let line = render_wide(&mut bus, 0, left, 8);
    let at = |hw: usize| line[usize::from(left) + hw];
    assert_eq!(at(128), BD, "no wrap: outside the map is backdrop");
    assert_eq!(at(135), BD);
}

/// T-07: bitmap modes cannot produce margin content, so the whole margin is
/// the backdrop in modes 3, 4 and 5.
#[test]
fn bitmap_modes_have_backdrop_margins() {
    let cases: [(&str, fn() -> Bus); 3] = [
        ("mode3", golden_scene_mode3),
        ("mode4", golden_scene_mode4),
        ("mode5", golden_scene_mode5),
    ];
    for (name, build) in cases {
        let mut bus = build();
        let left = 32u16;
        let right = 32u16;
        let line = render_wide(&mut bus, 0, left, right);
        assert_eq!(line.len(), 304, "{name}: width");
        assert!(
            line[..usize::from(left)].iter().all(|&p| p == BD),
            "{name}: left margin must be backdrop"
        );
        assert!(
            line[usize::from(left) + 240..].iter().all(|&p| p == BD),
            "{name}: right margin must be backdrop"
        );
        assert!(
            line[usize::from(left)..usize::from(left) + 240]
                .iter()
                .any(|&p| p != BD),
            "{name}: native area must still draw"
        );
    }
}

/// T-07b: mode 5 is only 160 px wide, so hardware columns 160..240 are
/// backdrop even inside the native crop.
#[test]
fn mode5_native_columns_past_160_are_backdrop() {
    let mut bus = golden_scene_mode5();
    // No margins: read the canonical row (view_line stays empty by contract).
    bus.ppu.vcount = 0;
    bus.ppu.render_line(&bus.io);
    assert!(bus.ppu.view_line().is_empty(), "NONE view has no wide line");
    let line = &bus.ppu.framebuffer[..240];
    assert_eq!(line[159], 0x1111);
    assert_eq!(line[160], BD);
    assert!(line[160..240].iter().all(|&p| p == BD));
}

// ---------------------- SPEC-069 T-08..T-12: OBJ, windows and composition

/// T-08: native OBJ clipping is on by default, so a sprite whose raw X field
/// encodes an off-screen position stays off the margin.
#[test]
fn obj_native_clip_hides_sprites_outside_240() {
    let mut bus = obj_scene();
    // Raw X 248 decodes to -264 under the preserved quirk; raw 0x1FC decodes
    // to -4 (already exercised by obj_normal_position_flip_and_offscreen_x).
    // Use a sprite starting at 236 so it straddles the native right edge.
    obj(&mut bus, 0, 10, 236, 0);
    assert!(bus.ppu.native_obj_clip(), "clip defaults on (WS-69-10)");

    let left = 0u16;
    let line = render_wide(&mut bus, 10, left, 16);
    // The tile has pixels only at x = 0 and x = 7, so the sprite covers
    // hardware 236 and 243; 243 is beyond the native edge and must not appear.
    assert_eq!(line[236], RED, "native column 236 draws");
    assert_eq!(line[237], BD, "transparent tile column");
    for x in 240..250usize {
        assert_eq!(line[x], BD, "column {x} is margin and must stay backdrop");
    }
}

/// T-09: with the widened policy opted in, the same sprite does reach the
/// margin — the exception is explicit, never the default.
#[test]
fn obj_widened_policy_draws_into_margin() {
    let mut bus = obj_scene();
    obj(&mut bus, 0, 10, 236, 0);
    bus.ppu.set_native_obj_clip(false);

    let line = render_wide(&mut bus, 10, 0, 16);
    assert_eq!(line[236], RED, "native column still draws");
    assert_eq!(
        line[243], GREEN,
        "widened policy lets the tile's x=7 pixel (hardware 243) reach the margin"
    );
    assert_eq!(line[244], BD, "8 px wide sprite ends at hardware 243");
}

/// T-08b: the signed 9-bit decode is untouched (INV-5). A raw X of 0x1FC is
/// -4, so the sprite covers hardware 0..3, not 508..511.
#[test]
fn obj_x_decode_stays_signed_nine_bit() {
    let mut bus = obj_scene();
    obj(&mut bus, 0, 10, 0x1FC, 0);
    // No margins, so read the canonical row for line 10.
    bus.ppu.vcount = 10;
    bus.ppu.render_line(&bus.io);
    let line = &bus.ppu.framebuffer[10 * 240..11 * 240];
    assert_eq!(line[0], BD, "tile pixel x=0 sits at hardware -4: clipped");
    assert_eq!(
        line[3], GREEN,
        "tile pixel x=7 sits at hardware 3, proving the signed decode"
    );
    assert_eq!(line[239], BD);
}

/// T-10: a window ending at hardware 240 does not extend into the margin.
#[test]
fn window_does_not_extend_into_margin() {
    let mut bus = text_scene();
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    reg(&mut bus, 0x008, 0x1F01);
    reg(&mut bus, 0x000, 0x2100); // WIN0 on, BG0 enabled
    reg(&mut bus, 0x040, 0x00F0); // WIN0 x 0..240
    reg(&mut bus, 0x044, 0x00A0); // y 0..160
    reg(&mut bus, 0x048, 0x0001); // inside: BG0 only
    reg(&mut bus, 0x04A, 0x0000); // outside: nothing

    let left = 8u16;
    let line = render_wide(&mut bus, 0, left, 16);
    assert_eq!(line[usize::from(left)], RED, "inside window draws BG0");
    assert_eq!(line[usize::from(left) + 240], BD, "margin is WINOUT");
}

/// T-11: when window control varies across the native row the margin is
/// undefined, so it falls closed to the backdrop.
#[test]
fn nonuniform_window_refuses_the_margin() {
    let mut bus = text_scene();
    map_entry(&mut bus, 31, 0, 0, 0x0001);
    reg(&mut bus, 0x008, 0x1F01);
    reg(&mut bus, 0x000, 0x2100);
    reg(&mut bus, 0x040, 0x2000); // WIN0 x 0..32 (x1 = 0x20)
    reg(&mut bus, 0x044, 0x00A0);
    reg(&mut bus, 0x048, 0x0001);
    reg(&mut bus, 0x04A, 0x0021); // outside: BG0 + effects

    let left = 8u16;
    let line = render_wide(&mut bus, 0, left, 16);
    assert_eq!(line[usize::from(left)], RED, "inside window still draws");
    for x in [usize::from(left) + 240, usize::from(left) + 250] {
        assert_eq!(line[x], BD, "non-uniform window -> margin backdrop");
    }
}

/// T-11b: a uniform window (same control across the whole native row) lets the
/// margin compose normally instead of failing closed.
#[test]
fn uniform_window_allows_the_margin() {
    let mut bus = text_scene();
    for x in 0..32 {
        set_pixel4(&mut bus, VRAM, 1, x % 8, 0, 1);
    }
    for tx in 0..32 {
        map_entry(&mut bus, 31, tx, 0, 0x0001);
    }
    reg(&mut bus, 0x008, 0x1F01);
    reg(&mut bus, 0x000, 0x2100);
    reg(&mut bus, 0x040, 0x00F0); // covers the whole native row
    reg(&mut bus, 0x044, 0x00A0);
    reg(&mut bus, 0x048, 0x0001);
    reg(&mut bus, 0x04A, 0x0001);

    let left = 8u16;
    let line = render_wide(&mut bus, 0, left, 16);
    for hw in [0usize, 100, 239] {
        assert_eq!(line[usize::from(left) + hw], RED, "hardware {hw}");
    }
    assert_eq!(
        line[usize::from(left) + 240],
        RED,
        "uniform control: margin composites like the native row"
    );
}

/// T-12: blending uses the same stack in the margin as in the native area.
#[test]
fn blend_applies_in_the_margin() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0300); // BG0 + BG1
    solid4(&mut bus, VRAM, 3, 1);
    solid4(&mut bus, VRAM, 4, 3);
    for tx in 0..32 {
        map_entry(&mut bus, 31, tx, 0, 3);
        map_entry(&mut bus, 30, tx, 0, 4);
    }
    reg(&mut bus, 0x008, 0x1F01); // BG0 prio 1
    reg(&mut bus, 0x00A, 0x1E00); // BG1 prio 0
    reg(&mut bus, 0x050, 0x0143); // alpha blend; BG0+BG1 first target, BG0 second
    reg(&mut bus, 0x052, 0x0808); // EVA = EVB = 8

    let left = 16u16;
    let line = render_wide(&mut bus, 0, left, 16);
    // BG1 (blue) over BG0 (red) at EVA = EVB = 8 gives 0x3C0F. Asserting the
    // exact value proves the margin went through the same blend stack as the
    // native row rather than showing a raw layer or the backdrop.
    assert_eq!(line[usize::from(left)], 0x3C0F, "native row blended");
    assert_eq!(
        line[usize::from(left) + 240],
        0x3C0F,
        "margin blended identically"
    );
}

// --------------------------- SPEC-069 T-20..T-22: cross-frame differential
//
// A tiny synthetic ROM drives the real CPU/bus/PPU pipeline, so the
// differential covers the whole path (not just render_line) without depending
// on a game image. The program writes a colour to BG palette and flips a
// scanline counter through VRAM each frame.

/// Minimal ARM ROM: infinite loop that touches IO/VRAM so frames advance.
fn differential_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x1000];
    // Header: entry branch placeholder is not needed; Gba starts at 0x0800_0000.
    let mut pc = 0usize;
    let mut emit = |rom: &mut Vec<u8>, word: u32| {
        rom[pc..pc + 4].copy_from_slice(&word.to_le_bytes());
        pc += 4;
    };
    // r0 = 0x04000000 (IO)
    emit(&mut rom, 0xE3A0_0304); // MOV r0, #0x04000000
    // Loop: r1 = 0x05000000 + (counter & 0x1F) * 2
    emit(&mut rom, 0xE3A0_1405); // MOV r1, #0x05000000
    emit(&mut rom, 0xE282_2001); // ADD r2, r2, #1
    emit(&mut rom, 0xE202_301F); // AND r3, r2, #0x1F
    emit(&mut rom, 0xE081_3083); // ADD r3, r1, r3, LSL #1
    emit(&mut rom, 0xE1D3_40B0); // LDRH r4, [r3]
    emit(&mut rom, 0xE284_4001); // ADD r4, r4, #1
    emit(&mut rom, 0xE1C3_40B0); // STRH r4, [r3]
    // Loop back: B .-32 (relative to PC+8)
    emit(&mut rom, 0xEAFF_FFF7);
    rom
}

/// Run `frames` frames, optionally applying the view, and hash every frame.
fn run_frames(
    rom: &[u8],
    frames: u32,
    view_at: &dyn Fn(u32) -> Option<crate::ppu::View>,
) -> Vec<(u64, u64)> {
    let mut gba = crate::Gba::new(crate::GbaConfig {
        rom,
        bios: None,
        backup: None,
        save: None,
    });
    let mut out = Vec::new();
    for f in 0..frames {
        if let Some(view) = view_at(f) {
            gba.set_view(view).unwrap();
        }
        gba.run_frame().unwrap();
        out.push((gba.frame_hash(), gba.instructions));
    }
    out
}

/// T-20: with the same inputs, the canonical hash of every frame is identical
/// whether the wide surface is off, on, or nominally present-but-zero.
#[test]
fn frame_hashes_match_with_view_on_and_off() {
    let rom = differential_rom();
    let baseline = run_frames(&rom, 12, &|_| None);
    let wide = run_frames(&rom, 12, &|_| {
        Some(crate::ppu::View {
            left: 24,
            right: 24,
        })
    });
    let asymmetric = run_frames(&rom, 12, &|_| Some(crate::ppu::View { left: 32, right: 8 }));
    assert_eq!(baseline.len(), 12);
    assert!(
        baseline
            .iter()
            .map(|f| f.0)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1,
        "the synthetic ROM must produce evolving frames, otherwise the \
         differential compares identical images to themselves"
    );
    for f in 0..baseline.len() {
        assert_eq!(baseline[f].0, wide[f].0, "frame {f}: hash must not move");
        assert_eq!(baseline[f].0, asymmetric[f].0, "frame {f}: asymmetric view");
    }
}

/// T-22: the view changes neither retired instructions nor guest cycles.
#[test]
fn view_does_not_change_instruction_or_cycle_counts() {
    let rom = differential_rom();
    let baseline = run_frames(&rom, 8, &|_| None);
    let wide = run_frames(&rom, 8, &|_| {
        Some(crate::ppu::View {
            left: 48,
            right: 48,
        })
    });
    for f in 0..baseline.len() {
        assert_eq!(
            baseline[f].1, wide[f].1,
            "frame {f}: instructions retired must be identical"
        );
    }
}

/// T-21: toggling the view mid-run never diverges from the baseline.
#[test]
fn alternating_view_mid_run_stays_identical() {
    let rom = differential_rom();
    let baseline = run_frames(&rom, 16, &|_| None);
    let toggled = run_frames(&rom, 16, &|f| {
        (f % 3 != 0).then_some(crate::ppu::View {
            left: 16,
            right: 32,
        })
    });
    for f in 0..baseline.len() {
        assert_eq!(baseline[f].0, toggled[f].0, "frame {f}: alternating view");
        assert_eq!(baseline[f].1, toggled[f].1, "frame {f}: instructions");
    }
}

/// T-20b: the wide surface actually has content and the canonical crop of it
/// equals the canonical framebuffer, frame by frame.
#[test]
fn wide_surface_crop_equals_canonical_each_frame() {
    let rom = differential_rom();
    let mut gba = crate::Gba::new(crate::GbaConfig {
        rom: &rom,
        bios: None,
        backup: None,
        save: None,
    });
    let view = crate::ppu::View {
        left: 24,
        right: 24,
    };
    gba.set_view(view).unwrap();
    for f in 0..8u32 {
        gba.run_frame().unwrap();
        let wide = gba.view_framebuffer();
        assert_eq!(wide.len(), view.width() * crate::SCREEN_HEIGHT);
        let canonical = gba.framebuffer();
        for y in 0..160usize {
            let row = &wide[y * view.width()..(y + 1) * view.width()];
            assert_eq!(
                &row[24..264],
                &canonical[y * 240..(y + 1) * 240],
                "frame {f} row {y}: canonical crop must equal the framebuffer"
            );
        }
    }
}

// -------------------- SPEC-069 WS-30: affine is the one genuine framework win
//
// An affine BG map is up to 1024x1024 texels, far wider than the 240-column
// window, so widening shows real map content with no guest change. This pins
// that claim.

#[test]
fn affine_map_1024_shows_new_content_in_the_margin() {
    let mut bus = scene();
    // Mode 1, BG2 affine. Size 3 = 1024x1024, screen block 16, char block 0.
    reg(&mut bus, 0x000, 0x0401);
    reg(&mut bus, 0x00C, 0xD000);
    // Identity mapping (one texel per pixel) and a latched reference at 0.
    reg(&mut bus, 0x020, 0x0100);
    reg(&mut bus, 0x024, 0x0100);
    bus.ppu.vblank_latch(&bus.io);

    // Three distinct 8bpp tiles placed far apart in the map: texel 0, 240, 480.
    for (tx, tile) in [(0u32, 1u32), (30, 2), (60, 3)] {
        for row in 0..8u32 {
            for col in 0..8u32 {
                bus.poke8(VRAM + tile * 64 + row * 8 + col, tile as u8);
            }
        }
        bus.poke8(VRAM + 16 * 0x800 + tx, tile as u8);
    }
    bus.poke16(PAL + 2, RED);
    bus.poke16(PAL + 4, GREEN);
    bus.poke16(PAL + 6, BLUE);

    // The view must be active BEFORE the line renders, since it is latched at
    // render time. Compare native output with an offset surface: the same
    // hardware columns must be sampled, which is what proves the extrapolation
    // (and that the margin is map content, not a repeat of the ring).
    bus.ppu
        .set_view(crate::ppu::View {
            left: 0,
            right: 140,
        })
        .unwrap();
    bus.ppu.vcount = 0;
    bus.ppu.render_line(&bus.io);
    let plain = bus.ppu.view_line().to_vec();
    assert_eq!(plain[0], RED, "texel 0 is tile 1");
    assert_eq!(plain.len(), 380);

    let shifted = render_wide(&mut bus, 0, 16, 124);
    assert_eq!(shifted.len(), 380);
    assert_eq!(
        &shifted[16..16 + 240],
        &plain[..240],
        "offset surface samples the same hardware columns"
    );
    assert_eq!(
        shifted[16], RED,
        "hardware column 0 lands at output column 16"
    );
    assert_eq!(
        shifted[16 + 240],
        plain[240],
        "margin continues the same sampling"
    );
}

/// WS-30b: the mechanism behind WS-30, stated directly.
///
/// A 1024-wide affine map holds content at texel 296 that a 240-column window
/// can never reach. One render with the view active shows both facts at once:
/// the canonical crop has no green, and the margin does.
#[test]
fn affine_map_beyond_240_is_reachable_only_through_the_view() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0401); // mode 1, BG2 affine on
    reg(&mut bus, 0x00C, 0xD000); // size 3 = 1024x1024
    reg(&mut bus, 0x020, 0x0100); // PA = 1.0
    reg(&mut bus, 0x026, 0x0100); // PD = 1.0 (PC stays 0: no shear)
    bus.ppu.vblank_latch(&bus.io);

    // Map cell 37 (texels 296..303) points at tile 3, a solid palette index 3.
    // Every other cell points at tile 0, which is palette index 0.
    for row in 0..8u32 {
        for col in 0..8u32 {
            bus.poke8(VRAM + 3 * 64 + row * 8 + col, 3);
        }
    }
    bus.poke8(VRAM + 16 * 0x800 + 37, 3);
    bus.poke16(PAL + 6, GREEN);

    let line = render_wide(&mut bus, 0, 0, 140);
    assert_eq!(line.len(), 380);
    // The surface beyond the native window samples the affine map; only 240
    // columns of it are visible, so texel 296 is margin-only content.
    assert!(
        !line[..240].contains(&GREEN),
        "a 240-column window stops at texel 239"
    );

    // The margin can, because the map really holds that content.
    assert_eq!(line[296], GREEN, "texel 296 is authored map content");
    assert_eq!(line[303], GREEN, "the tile is 8 texels wide");
    assert_eq!(line[304], BD, "the following cell points at tile 0");
}

// ------------------- SPEC-070 DIAG-70-01..04: margin classification
//
// The classification must be structural (which layer supplied the column and
// where the sample fell), never a colour comparison. These tests pin that.

#[test]
fn text_map_256_marks_the_wrapped_margin_as_ring() {
    // Use the proven text fixture: screen block 31, tile 1.
    let mut bus = text_scene();
    // Every cell of the 256-wide map points at tile 1, and tile 1 is filled so
    // the layer covers all 256 columns instead of leaving transparent gaps.
    for ty in 0..32u32 {
        for tx in 0..32u32 {
            map_entry(&mut bus, 31, tx, ty, 0x0001);
        }
    }
    for row in 0..8u32 {
        for col in 0..8u32 {
            set_pixel4(&mut bus, VRAM, 1, col, row, 1);
        }
    }
    let line = render_wide(&mut bus, 0, 40, 40);
    assert_eq!(line.len(), 320);
    let kinds = bus.ppu.margin_kinds().to_vec();

    // left margin: hw -40..-17 wraps to map columns 216..239, which the native
    // row also samples -> ring; hw -16..-1 wraps to the map's hidden tail
    // 240..255, which the native row cannot reach -> authored.
    assert!(
        kinds[..24].iter().all(|k| *k == MarginKind::Ring),
        "left margin repeats map columns 216..239"
    );
    assert!(
        kinds[24..40].iter().all(|k| *k == MarginKind::Authored),
        "map columns 240..255 are unreachable natively"
    );
    // right margin: hw 240..255 is the same hidden tail -> authored,
    // hw 256..279 wraps to map columns 0..23 -> ring.
    assert!(
        kinds[280..296].iter().all(|k| *k == MarginKind::Authored),
        "map columns 240..255 are unreachable natively"
    );
    assert!(
        kinds[296..].iter().all(|k| *k == MarginKind::Ring),
        "hw 256..279 wraps to the row start"
    );
    // The canonical crop is never classified.
    assert!(kinds[40..280].iter().all(|k| *k == MarginKind::Native));
}

#[test]
fn uncovered_margin_is_backdrop() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0100); // mode 0, BG0 on
    // An empty map: every texel is index 0, so no layer covers anything.
    let line = render_wide(&mut bus, 0, 40, 40);
    assert_eq!(line.len(), 320);
    let kinds = bus.ppu.margin_kinds().to_vec();
    assert!(
        kinds[..40].iter().all(|k| *k == MarginKind::Backdrop),
        "empty map => backdrop"
    );
    assert!(kinds[280..].iter().all(|k| *k == MarginKind::Backdrop));
}

#[test]
fn affine_map_beyond_256_marks_the_margin_as_authored() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0401); // mode 1, BG2 affine
    reg(&mut bus, 0x00C, 0xD000); // 1024x1024
    reg(&mut bus, 0x020, 0x0100);
    reg(&mut bus, 0x026, 0x0100);
    bus.ppu.vblank_latch(&bus.io);
    // Fill the map and one tile with a non-zero index.
    for cell in 0..0x1000u32 {
        bus.poke8(VRAM + 16 * 0x800 + cell, 3);
    }
    for i in 0..64u32 {
        bus.poke8(VRAM + 3 * 64 + i, 3);
    }
    bus.poke16(PAL + 6, GREEN);
    let _ = render_wide(&mut bus, 0, 40, 40);
    let kinds = bus.ppu.margin_kinds().to_vec();
    // With the reference at texel 0, hw < 0 samples negative texels, which are
    // outside the map: that is backdrop, not content (WS-69-12).
    assert!(
        kinds[..40].iter().all(|k| *k == MarginKind::Backdrop),
        "negative texels are outside the affine map"
    );
    // hw >= 256 is inside the 1024-wide map, so it is authored content and
    // never classified as a ring.
    assert!(
        kinds[280..].iter().all(|k| *k == MarginKind::Authored),
        "a 1024-wide affine map has real content past column 256"
    );
}

#[test]
fn wrapped_affine_margin_repeating_the_native_row_is_ring() {
    let mut bus = affine_scene();
    reg(&mut bus, 0x020, 0x0100); // PA = 1.0
    reg(&mut bus, 0x00C, 0x3004); // wrap on, 128x128 map
    // Cover the whole map so the affine layer wins every margin column.
    for cell in 0..256u32 {
        bus.poke8(VRAM + 16 * 0x800 + cell, 1);
    }
    let _ = render_wide(&mut bus, 0, 40, 40);
    let kinds = bus.ppu.margin_kinds().to_vec();
    // The 128-wide wrapped map repeats every 128 columns, so all margin
    // samples land on texels the native row already samples.
    assert!(
        kinds[..40].iter().all(|k| *k == MarginKind::Ring),
        "a wrapped 128 map repeats the native row on both sides"
    );
    assert!(kinds[280..].iter().all(|k| *k == MarginKind::Ring));
    assert!(kinds[40..280].iter().all(|k| *k == MarginKind::Native));
}

#[test]
fn wrapped_affine_margin_splits_ring_from_authored_at_the_map_tail() {
    let mut bus = affine_scene();
    reg(&mut bus, 0x020, 0x0100); // PA = 1.0
    reg(&mut bus, 0x00C, 0x7004); // wrap on; size 1 => 256x256 map
    for cell in 0..1024u32 {
        bus.poke8(VRAM + 16 * 0x800 + cell, 1);
    }
    let _ = render_wide(&mut bus, 0, 40, 40);
    let kinds = bus.ppu.margin_kinds().to_vec();
    // Same structure as the text-256 case: texels 216..239 repeat the native
    // row (ring), texels 240..255 are reachable only through the margin
    // (authored), and past 256 the wrap repeats again (ring).
    assert!(kinds[..24].iter().all(|k| *k == MarginKind::Ring));
    assert!(kinds[24..40].iter().all(|k| *k == MarginKind::Authored));
    assert!(kinds[280..296].iter().all(|k| *k == MarginKind::Authored));
    assert!(kinds[296..].iter().all(|k| *k == MarginKind::Ring));
}

#[test]
fn forced_blank_reports_backdrop_margins_not_stale_kinds() {
    let mut bus = text_scene();
    for ty in 0..32u32 {
        for tx in 0..32u32 {
            map_entry(&mut bus, 31, tx, ty, 0x0001);
        }
    }
    for row in 0..8u32 {
        for col in 0..8u32 {
            set_pixel4(&mut bus, VRAM, 1, col, row, 1);
        }
    }
    let _ = render_wide(&mut bus, 0, 40, 40);
    reg(&mut bus, 0x000, 0x0180); // forced blank
    let line = render_wide(&mut bus, 0, 40, 40);
    assert!(line.iter().all(|&p| p == 0x7FFF), "blank line is white");
    let kinds = bus.ppu.margin_kinds().to_vec();
    assert!(
        kinds[..40].iter().all(|k| *k == MarginKind::Backdrop),
        "margin diagnostics must describe the blank line, not the previous one"
    );
    assert!(kinds[280..].iter().all(|k| *k == MarginKind::Backdrop));
    assert!(kinds[40..280].iter().all(|k| *k == MarginKind::Native));
}

#[test]
fn verdict_requires_a_majority_of_authored() {
    let mut c = MarginCounts::default();
    c.backdrop = 10;
    assert_eq!(c.verdict(), MarginKind::Backdrop);
    c.ring = 20;
    assert_eq!(c.verdict(), MarginKind::Ring);
    c.authored = 5;
    assert_eq!(
        c.verdict(),
        MarginKind::Ring,
        "authored must be the majority"
    );
    c.authored = 30;
    assert_eq!(c.verdict(), MarginKind::Authored);
}

#[test]
fn native_view_has_no_margin_kinds() {
    let mut bus = scene();
    reg(&mut bus, 0x000, 0x0100);
    bus.ppu.vcount = 0;
    bus.ppu.render_line(&bus.io);
    assert!(
        bus.ppu
            .margin_kinds()
            .iter()
            .all(|k| *k == MarginKind::Native),
        "with no view every column is native"
    );
}
