//! SPEC-058 acceptance: small synthetic scenes written through the bus,
//! checked pixel by pixel after rendering one scanline.

use crate::bus::Bus;
use crate::bus::tests::test_bus;

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
