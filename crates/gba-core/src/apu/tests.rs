//! SPEC-059 acceptance: frame sequencer timing, master enable, wave RAM
//! banks, FIFOs fed by timers and DMA, and the mixer.

use super::*;
use crate::bus::tests::{run_until, test_bus};

const SEQ: u64 = FRAME_SEQ_CYCLES;

fn apu_on() -> Apu {
    let mut a = Apu::new();
    a.reset_clock(0);
    a.write8(0x84, 0x80);
    a
}

#[test]
fn envelope_steps_at_64hz() {
    let mut a = apu_on();
    a.write8(0x62, 0x00);
    a.write8(0x63, 0xF1); // volume 15, decrease, step 1
    a.write8(0x64, 0x00);
    a.write8(0x65, 0x80); // trigger
    assert!(a.ch1.on);
    a.run_until(8 * SEQ - 1);
    assert_eq!(a.ch1.env.volume, 15);
    a.run_until(8 * SEQ);
    assert_eq!(a.ch1.env.volume, 14);
    a.run_until(16 * SEQ);
    assert_eq!(a.ch1.env.volume, 13);

    // Increase with step 2: one change every 2/64 s.
    a.write8(0x69, 0x0A); // volume 0, increase, step 2
    a.write8(0x6D, 0x80);
    assert!(a.ch2.on, "DAC on when increase is set");
    a.run_until(24 * SEQ);
    assert_eq!(a.ch2.env.volume, 0);
    a.run_until(32 * SEQ);
    assert_eq!(a.ch2.env.volume, 1);
    // Step 0 freezes the envelope.
    a.write8(0x79, 0xF0);
    a.write8(0x7D, 0x80);
    a.run_until(64 * SEQ);
    assert_eq!(a.ch4.env.volume, 15);
}

#[test]
fn length_counter_at_256hz() {
    let mut a = apu_on();
    a.write8(0x68, 62); // length 2
    a.write8(0x69, 0xF0);
    a.write8(0x6C, 0x00);
    a.write8(0x6D, 0xC0); // trigger + length enable
    // Length clocks on sequencer steps 0, 2, 4, 6.
    a.run_until(3 * SEQ - 1);
    assert!(a.ch2.on);
    assert_eq!(a.read16(0x84) & 0x02, 0x02);
    a.run_until(3 * SEQ);
    assert!(!a.ch2.on);
    assert_eq!(a.read16(0x84) & 0x02, 0);

    // Without length enable the channel keeps playing.
    a.write8(0x68, 62);
    a.write8(0x6D, 0x80);
    a.run_until(20 * SEQ);
    assert!(a.ch2.on);
}

#[test]
fn sweep_at_128hz_with_overflow() {
    let mut a = apu_on();
    a.write8(0x60, 0x11); // period 1, increase, shift 1
    a.write8(0x63, 0xF0);
    a.write8(0x64, 0x00);
    a.write8(0x65, 0x84); // trigger, freq 0x400
    a.run_until(3 * SEQ - 1);
    assert_eq!(a.ch1.freq, 0x400);
    a.run_until(3 * SEQ); // step 2
    assert_eq!(a.ch1.freq, 0x600);
    assert!(a.ch1.on);
    a.run_until(7 * SEQ); // step 6: 0x600 + 0x300 > 0x7FF
    assert!(!a.ch1.on);
}

#[test]
fn sweep_decrease() {
    let mut a = apu_on();
    a.write8(0x60, 0x1A); // period 1, decrease, shift 2
    a.write8(0x63, 0xF0);
    a.write8(0x65, 0x84);
    a.run_until(3 * SEQ);
    assert_eq!(a.ch1.freq, 0x300);
    // Shift 0 never changes the frequency (it would otherwise drop to 0).
    let mut a = apu_on();
    a.write8(0x60, 0x18);
    a.write8(0x63, 0xF0);
    a.write8(0x65, 0x84);
    a.run_until(8 * SEQ);
    assert_eq!(a.ch1.freq, 0x400);
    assert!(a.ch1.on);
}

#[test]
fn master_enable_gates_and_clears_psg_registers() {
    let mut a = Apu::new();
    a.reset_clock(0);
    a.write8(0x63, 0xF0);
    assert_eq!(a.read16(0x62), 0, "PSG registers locked while master off");
    // SOUNDCNT_H stays writable with master off.
    a.write8(0x82, 0x02);
    assert_eq!(a.read16(0x82), 0x0002);
    a.write8(0x84, 0x80);
    a.write8(0x63, 0xF0);
    a.write8(0x80, 0x77);
    assert_eq!(a.read16(0x62), 0xF000);
    a.write8(0x65, 0x80);
    assert!(a.ch1.on);
    a.write8(0x84, 0x00);
    assert!(!a.ch1.on);
    assert_eq!(a.read16(0x62), 0);
    assert_eq!(a.read16(0x80), 0);
    assert_eq!(a.read16(0x82), 0x0002, "SOUNDCNT_H survives master off");
    assert_eq!(a.read16(0x88), 0x0200, "SOUNDBIAS survives master off");
    // Output is silent while off.
    a.run_until(512 * 4);
    assert!(a.samples.iter().all(|&s| s == 0));
}

#[test]
fn wave_ram_cpu_access_uses_the_other_bank() {
    let mut a = apu_on();
    a.write8(0x70, 0x00); // playing bank 0 → CPU sees bank 1
    a.write8(0x90, 0x12);
    assert_eq!(a.wave_ram[1][0], 0x12);
    assert_eq!(a.read16(0x90), 0x0012);
    a.write8(0x70, 0x40); // playing bank 1 → CPU sees bank 0
    assert_eq!(a.read16(0x90), 0);
    a.write8(0x91, 0x34);
    assert_eq!(a.wave_ram[0][1], 0x34);
    a.write8(0x70, 0x00);
    assert_eq!(a.read16(0x90), 0x0012);
}

#[test]
fn wave_volume_levels() {
    let mut a = apu_on();
    a.write8(0x70, 0x40); // CPU writes bank 0
    a.write8(0x90, 0xF0); // first sample 15
    a.write8(0x70, 0x80); // play bank 0
    for (nr32_hi, expected) in [(0x00u8, 0), (0x20, 15), (0x40, 7), (0x60, 3), (0x80, 11)] {
        a.write8(0x73, nr32_hi);
        a.write8(0x75, 0x80);
        assert!(a.ch3.on);
        assert_eq!(a.wave_output(0), expected, "NR32 high {nr32_hi:#x}");
    }
    // Stopping playback (bit 7 of SOUND3CNT_L) silences the channel.
    a.write8(0x70, 0x00);
    assert_eq!(a.wave_output(0), 0);
}

#[test]
fn canonical_sample_rate() {
    let mut a = apu_on();
    a.run_until(512 * 100);
    assert_eq!(a.samples.len(), 200, "stereo pair every 512 cycles");
}

#[test]
fn mixer_routes_fifos_and_clamps() {
    let mut a = apu_on();
    a.write8(0x82, 0x04); // FIFO A 100%
    a.write8(0x83, 0x01); // FIFO A right, timer 0
    a.fifo_push(0, 64);
    a.timer_overflow(0, 0);
    a.run_until(512);
    assert_eq!(a.samples, [0, 8192]);
    // 50% volume.
    a.samples.clear();
    a.write8(0x82, 0x00);
    a.run_until(1024);
    assert_eq!(a.samples, [0, 4096]);
    // Both FIFOs at full scale on both sides saturate the 10-bit DAC.
    a.samples.clear();
    a.write8(0x82, 0x0C);
    a.write8(0x83, 0x33);
    a.fifo_push(0, 127);
    a.fifo_push(1, 127);
    a.timer_overflow(0, 1024);
    a.run_until(1536);
    assert_eq!(a.samples, [511 * 32, 511 * 32]);
}

#[test]
fn mixer_routes_psg_channels() {
    let mut a = apu_on();
    a.write8(0x80, 0x07); // right volume 7
    a.write8(0x81, 0x02); // channel 2 → right only
    a.write8(0x82, 0x02); // PSG 100%
    a.write8(0x68, 0x80); // duty 50%
    a.write8(0x69, 0xF0);
    a.write8(0x6D, 0x80);
    a.run_until(512);
    assert_eq!(a.samples, [0, 15 * 8 * 2 * 32]);
}

#[test]
fn fifo_reset_bits_empty_the_queue() {
    let mut a = apu_on();
    for v in 0..10 {
        a.fifo_push(0, v);
        a.fifo_push(1, v);
    }
    a.write8(0x83, 0x08);
    assert_eq!((a.fifo[0].len, a.fifo[1].len), (0, 10));
    a.write8(0x83, 0x80);
    assert_eq!(a.fifo[1].len, 0);
    assert_eq!(a.read16(0x82) & 0x8800, 0, "reset bits read as 0");
}

// ------------------------------------------------------------ via the bus

const IO: u32 = 0x0400_0000;

#[test]
fn fifo_pops_on_selected_timer_overflow() {
    let mut bus = test_bus();
    bus.poke16(IO + 0x84, 0x0080);
    bus.poke16(IO + 0x82, 0x4B04); // FIFO A 100%, L+R, timer 0, reset; B on timer 1
    bus.poke32(IO + 0xA0, 0x0403_0201);
    assert_eq!(bus.apu.fifo[0].len, 4);
    bus.poke16(IO + 0x100, 0xFF00);
    bus.poke16(IO + 0x102, 0x0080);
    run_until(&mut bus, 2 + 256);
    assert_eq!((bus.apu.fifo[0].current, bus.apu.fifo[0].len), (1, 3));
    run_until(&mut bus, 2 + 256 * 4);
    assert_eq!((bus.apu.fifo[0].current, bus.apu.fifo[0].len), (4, 0));
    assert_eq!(bus.apu.fifo_underruns, 0);
    run_until(&mut bus, 2 + 256 * 5);
    assert_eq!(bus.apu.fifo_underruns, 1);
    assert_eq!(
        bus.apu.fifo[0].current, 4,
        "underrun repeats the last sample"
    );
}

#[test]
fn fifo_refilled_by_special_dma() {
    let mut bus = test_bus();
    for i in 0..64u32 {
        bus.poke8(0x0200_0000 + i, i as u8);
    }
    bus.poke16(IO + 0x84, 0x0080);
    // FIFO A on timer 0, FIFO B on timer 1.
    bus.poke16(IO + 0x82, 0x4B0C | 0x3000);
    // DMA1 → FIFO A, DMA2 → FIFO B: special timing, repeat, 32-bit, dst fixed.
    for (ch, fifo) in [(1u32, 0xA0u32), (2, 0xA4)] {
        let base = IO + 0xB0 + 12 * ch;
        bus.poke32(base, 0x0200_0000);
        bus.poke32(base + 4, IO + fifo);
        bus.poke16(base + 8, 4);
        bus.poke16(base + 10, 0xB640);
    }
    assert!(!bus.dma_pending());
    bus.poke16(IO + 0x100, 0xFF00);
    bus.poke16(IO + 0x102, 0x0080);
    run_until(&mut bus, 2 + 256);
    assert_eq!(bus.apu.fifo_underruns, 1, "first pop found the FIFO empty");
    assert_eq!(bus.apu.fifo[0].len, 16, "DMA pushed 4 words");
    assert_eq!(bus.dma[1].src, 0x0200_0010);
    assert_eq!(bus.dma[1].dst, IO + 0xA0, "destination stays fixed");
    run_until(&mut bus, 2 + 256 * 2);
    assert_eq!((bus.apu.fifo[0].current, bus.apu.fifo[0].len), (0, 31));
    run_until(&mut bus, 2 + 256 * 3);
    assert_eq!((bus.apu.fifo[0].current, bus.apu.fifo[0].len), (1, 30));
    assert_eq!(bus.dma[1].src, 0x0200_0020, "no request above 16 bytes");
    assert_ne!(bus.dma[1].control & 0x8000, 0);
    // FIFO B's timer never ran: DMA2 untouched.
    assert_eq!(bus.apu.fifo[1].len, 0);
    assert_eq!(bus.dma[2].src, 0x0200_0000);
}
