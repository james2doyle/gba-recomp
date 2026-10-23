//! Renders the real localized menu and saves a PNG for visual inspection.

use gba_desktop::menu::{Menu, Page};
use gba_desktop::settings::Settings;
use gba_desktop::sdl;

#[test]
#[ignore = "writes a PNG; run with --ignored under SDL_VIDEODRIVER=offscreen"]
fn render_localized_menu() {
    let ctx = sdl::Context::init().expect("SDL init");
    let mut video = ctx.create_video("menu", 240, 160, 1).expect("video");
    let mut menu = Menu::new(Settings::default());
    menu.open_at(Page::Main, &Settings::default());
    let rows = menu.rows();
    let title = menu.title();
    let frame = vec![0u32; 240 * 160];
    let (w, h, rgba) = video
        .render_overlay(&frame, |p| {
            p.fill(0.0, 0.0, 240.0, 160.0, [0, 0, 24, 200]);
            p.text(6.0, 4.0, &title, [255, 220, 120]);
            for (i, row) in rows.iter().enumerate() {
                let y = 16.0 + i as f32 * 12.0;
                if i == 8 {
                    p.fill(2.0, y, 236.0, 12.0, [80, 80, 160, 255]);
                }
                p.text(8.0, y, &row.label, [255, 255, 255]);
                if !row.value.is_empty() {
                    let tw = p.text_width(&row.value);
                    p.text(236.0 - tw, y, &row.value, [170, 220, 255]);
                }
            }
            p.fill(0.0, 148.0, 240.0, 12.0, [0, 0, 0, 220]);
            p.text(4.0, 148.5, &gba_i18n::tr!("menu.hint"), [200, 200, 200]);
        })
        .expect("render");
    std::fs::write("/tmp/menu_i18n.png", png_rgba(&rgba, w, h)).expect("write");
    eprintln!("wrote /tmp/menu_i18n.png {w}x{h} rows={}", rows.len());
}

fn crc32(d: &[u8]) -> u32 {
    let mut t = [0u32; 256];
    for (i, s) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *s = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in d {
        c = t[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn chunk(o: &mut Vec<u8>, k: &[u8; 4], b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_be_bytes());
    let mut kk = k.to_vec();
    kk.extend_from_slice(b);
    o.extend_from_slice(&kk);
    o.extend_from_slice(&crc32(&kk).to_be_bytes());
}

fn png_rgba(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        let at = (y * w * 4) as usize;
        raw.extend_from_slice(&rgba[at..at + (w * 4) as usize]);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ih = Vec::new();
    ih.extend_from_slice(&w.to_be_bytes());
    ih.extend_from_slice(&h.to_be_bytes());
    ih.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ih);
    let mut z = vec![0x78, 0x01];
    for (i, part) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(if last { 1 } else { 0 });
        let n = part.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(part);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
