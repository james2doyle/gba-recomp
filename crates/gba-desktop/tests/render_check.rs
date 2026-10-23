//! Visual check: draw the menu overlay and save it as a PNG so the real
//! rendered text (accents included) can be inspected (SPEC-069).

use gba_desktop::sdl;

#[test]
#[ignore = "writes a PNG; run with --ignored under SDL_VIDEODRIVER=dummy"]
fn render_menu_overlay_to_png() {
    let ctx = sdl::Context::init().expect("SDL init");
    let mut video = ctx
        .create_video("render check", 240, 160, 1)
        .expect("create video");
    let frame = vec![0u32; 240 * 160];
    let rows = [
        ("Vídeo", ""),
        ("Escala", "3x"),
        ("Tela cheia", "Não"),
        ("Filtro", "Nítido"),
        ("Áudio", ""),
        ("Latência", "60 ms"),
        ("Idioma", "Português (Brasil)"),
    ];
    let (rw, rh, rgba) = video
        .render_overlay(&frame, |p| {
            p.fill(0.0, 0.0, 240.0, 160.0, [0, 0, 24, 200]);
            p.text(6.0, 4.0, "Configurações", [255, 220, 120]);
            for (i, (label, value)) in rows.iter().enumerate() {
                let y = 20.0 + i as f32 * 12.0;
                p.text(8.0, y, label, [255, 255, 255]);
                if !value.is_empty() {
                    let w = p.text_width(value);
                    p.text(236.0 - w, y, value, [170, 220, 255]);
                }
            }
        })
        .expect("render");
    // Minimal PNG writer: no dependency, RGBA8, one IDAT.
    let png = png_rgba(&rgba, rw, rh);
    std::fs::write("/tmp/menu_render.png", png).expect("write png");
    eprintln!("wrote /tmp/menu_render.png ({rw}x{rh})");
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let mut with_kind = kind.to_vec();
    with_kind.extend_from_slice(body);
    out.extend_from_slice(&with_kind);
    out.extend_from_slice(&crc32(&with_kind).to_be_bytes());
}

fn png_rgba(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + h as usize);
    for y in 0..h {
        raw.push(0); // filter: none
        let at = (y * w * 4) as usize;
        raw.extend_from_slice(&rgba[at..at + (w * 4) as usize]);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    // zlib stored blocks (no compression).
    let mut z = vec![0x78, 0x01];
    for (i, part) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(if last { 1 } else { 0 });
        let len = part.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(part);
    }
    let mut adler_a = 1u32;
    let mut adler_b = 0u32;
    for &byte in &raw {
        adler_a = (adler_a + u32::from(byte)) % 65521;
        adler_b = (adler_b + adler_a) % 65521;
    }
    z.extend_from_slice(&((adler_b << 16) | adler_a).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
