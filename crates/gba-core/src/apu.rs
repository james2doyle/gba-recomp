//! APU (SPEC-059): four PSG channels, two direct-sound FIFOs and the mixer.
//! Canonical output is stereo i16 at 32768 Hz derived only from guest time
//! (AUD-02).

pub const SAMPLE_RATE: u32 = 32768;
pub const CYCLES_PER_SAMPLE: u64 = 512;
const FRAME_SEQ_CYCLES: u64 = 32768; // 512 Hz

const DUTY: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 0],
];

#[derive(Clone, Copy, Default)]
struct Envelope {
    volume: u8,
    step: u8,
    increase: bool,
    timer: u8,
}

impl Envelope {
    fn load(&mut self, reg: u16) {
        self.volume = (reg >> 12) as u8;
        self.increase = reg & 0x0800 != 0;
        self.step = ((reg >> 8) & 7) as u8;
        self.timer = self.step;
    }
    fn clock(&mut self) {
        if self.step == 0 {
            return;
        }
        if self.timer > 1 {
            self.timer -= 1;
            return;
        }
        self.timer = self.step;
        if self.increase && self.volume < 15 {
            self.volume += 1;
        } else if !self.increase && self.volume > 0 {
            self.volume -= 1;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Square {
    on: bool,
    freq: u16,
    duty: usize,
    phase: usize,
    timer: i64,
    length: u16,
    length_enable: bool,
    env: Envelope,
    // Sweep (channel 1 only).
    sweep_shift: u8,
    sweep_dec: bool,
    sweep_period: u8,
    sweep_timer: u8,
}

impl Square {
    fn period(&self) -> i64 {
        (2048 - i64::from(self.freq)) * 16
    }
    fn advance(&mut self, cycles: i64) {
        if !self.on {
            return;
        }
        self.timer -= cycles;
        let p = self.period();
        while self.timer <= 0 {
            self.timer += p;
            self.phase = (self.phase + 1) & 7;
        }
    }
    fn output(&self) -> i32 {
        if !self.on {
            return 0;
        }
        let high = DUTY[self.duty][self.phase] != 0;
        let v = i32::from(self.env.volume);
        if high { v } else { -v }
    }
    fn clock_length(&mut self) {
        if self.length_enable && self.length > 0 {
            self.length -= 1;
            if self.length == 0 {
                self.on = false;
            }
        }
    }
    fn clock_sweep(&mut self) {
        if self.sweep_period == 0 {
            return;
        }
        if self.sweep_timer > 1 {
            self.sweep_timer -= 1;
            return;
        }
        self.sweep_timer = self.sweep_period;
        let delta = self.freq >> self.sweep_shift;
        if self.sweep_dec {
            self.freq = self.freq.saturating_sub(delta);
        } else {
            let new = u32::from(self.freq) + u32::from(delta);
            if new > 2047 {
                self.on = false;
            } else if self.sweep_shift != 0 {
                self.freq = new as u16;
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Wave {
    on: bool,
    playing: bool,
    freq: u16,
    position: usize,
    timer: i64,
    length: u16,
    length_enable: bool,
    volume: u8,
    force75: bool,
    two_banks: bool,
    bank: usize,
}

#[derive(Clone, Copy, Default)]
struct Noise {
    on: bool,
    lfsr: u16,
    narrow: bool,
    divisor: u8,
    shift: u8,
    timer: i64,
    length: u16,
    length_enable: bool,
    env: Envelope,
}

impl Noise {
    fn period(&self) -> i64 {
        let r = if self.divisor == 0 {
            16
        } else {
            32 * i64::from(self.divisor)
        };
        r << (u32::from(self.shift) + 1)
    }
}

#[derive(Clone, Default)]
struct Fifo {
    data: [i8; 32],
    read: usize,
    len: usize,
    current: i8,
}

impl Fifo {
    fn push(&mut self, v: i8) {
        if self.len < 32 {
            self.data[(self.read + self.len) & 31] = v;
            self.len += 1;
        }
    }
    fn pop(&mut self) {
        if self.len > 0 {
            self.current = self.data[self.read];
            self.read = (self.read + 1) & 31;
            self.len -= 1;
        }
    }
    fn reset(&mut self) {
        self.read = 0;
        self.len = 0;
        self.current = 0;
    }
}

pub struct Apu {
    regs: [u8; 0x50],
    wave_ram: [[u8; 16]; 2],
    ch1: Square,
    ch2: Square,
    ch3: Wave,
    ch4: Noise,
    fifo: [Fifo; 2],
    next_sample: u64,
    last_sample_cycle: u64,
    frame_seq: u64,
    frame_step: u8,
    /// Interleaved stereo samples produced since the last drain.
    pub samples: Vec<i16>,
    pub fifo_underruns: u64,
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

impl Apu {
    pub fn new() -> Self {
        Self {
            regs: [0; 0x50],
            wave_ram: [[0; 16]; 2],
            ch1: Square::default(),
            ch2: Square::default(),
            ch3: Wave::default(),
            ch4: Noise::default(),
            fifo: [Fifo::default(), Fifo::default()],
            next_sample: CYCLES_PER_SAMPLE,
            last_sample_cycle: 0,
            frame_seq: FRAME_SEQ_CYCLES,
            frame_step: 0,
            samples: Vec::with_capacity(2048),
            fifo_underruns: 0,
        }
    }

    pub fn reset_clock(&mut self, now: u64) {
        self.next_sample = now + CYCLES_PER_SAMPLE;
        self.last_sample_cycle = now;
        self.frame_seq = now + FRAME_SEQ_CYCLES;
        // SOUNDBIAS default.
        self.regs[0x28] = 0x00;
        self.regs[0x29] = 0x02;
    }

    fn reg16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.regs[offset], self.regs[offset + 1]])
    }

    fn master_on(&self) -> bool {
        self.regs[0x24] & 0x80 != 0
    }

    pub fn next_sample_at(&self) -> u64 {
        self.next_sample.min(self.frame_seq)
    }

    pub fn read16(&self, offset: u32) -> u16 {
        let o = (offset - 0x60) as usize;
        match offset {
            0x060 => self.reg16(o) & 0x007F,
            0x062 | 0x068 => self.reg16(o) & 0xFFC0,
            0x064 | 0x06C | 0x074 | 0x07C => self.reg16(o) & 0x4000,
            0x070 => self.reg16(o) & 0x00E0,
            0x072 => self.reg16(o) & 0xE000,
            0x078 => self.reg16(o) & 0xFF00,
            0x080 => self.reg16(o) & 0xFF77,
            0x082 => self.reg16(o) & 0x770F,
            0x084 => {
                let mut v = u16::from(self.regs[0x24] & 0x80);
                v |= u16::from(self.ch1.on)
                    | u16::from(self.ch2.on) << 1
                    | u16::from(self.ch3.on) << 2
                    | u16::from(self.ch4.on) << 3;
                v
            }
            0x088 => self.reg16(o),
            0x090..=0x09E => {
                let bank = if self.ch3.bank == 0 { 1 } else { 0 };
                let i = (offset - 0x90) as usize;
                u16::from_le_bytes([self.wave_ram[bank][i], self.wave_ram[bank][i + 1]])
            }
            _ => 0,
        }
    }

    pub fn write8(&mut self, offset: u32, value: u8) {
        if !(0x60..0xB0).contains(&offset) {
            return;
        }
        if (0xA0..0xA8).contains(&offset) {
            self.fifo_push(usize::from(offset >= 0xA4), value as i8);
            return;
        }
        if (0x90..0xA0).contains(&offset) {
            let bank = if self.ch3.bank == 0 { 1 } else { 0 };
            self.wave_ram[bank][(offset - 0x90) as usize] = value;
            return;
        }
        let o = (offset - 0x60) as usize;
        if offset < 0x84 && !self.master_on() && offset != 0x82 && offset != 0x83 {
            return;
        }
        self.regs[o] = value;
        match offset {
            0x060 => {
                self.ch1.sweep_shift = value & 7;
                self.ch1.sweep_dec = value & 8 != 0;
                self.ch1.sweep_period = (value >> 4) & 7;
            }
            0x062 | 0x063 => {
                let r = self.reg16(0x02);
                self.ch1.duty = ((r >> 6) & 3) as usize;
                self.ch1.length = 64 - (r & 0x3F);
                if offset == 0x063 {
                    self.ch1.env.load(r);
                    if r & 0xF800 == 0 {
                        self.ch1.on = false;
                    }
                }
            }
            0x064 | 0x065 => {
                let r = self.reg16(0x04);
                self.ch1.freq = r & 0x7FF;
                self.ch1.length_enable = r & 0x4000 != 0;
                if offset == 0x065 && r & 0x8000 != 0 {
                    let env = self.reg16(0x02);
                    self.ch1.env.load(env);
                    self.ch1.on = env & 0xF800 != 0;
                    self.ch1.timer = self.ch1.period();
                    self.ch1.sweep_timer = self.ch1.sweep_period;
                    if self.ch1.length == 0 {
                        self.ch1.length = 64;
                    }
                }
            }
            0x068 | 0x069 => {
                let r = self.reg16(0x08);
                self.ch2.duty = ((r >> 6) & 3) as usize;
                self.ch2.length = 64 - (r & 0x3F);
                if offset == 0x069 {
                    self.ch2.env.load(r);
                    if r & 0xF800 == 0 {
                        self.ch2.on = false;
                    }
                }
            }
            0x06C | 0x06D => {
                let r = self.reg16(0x0C);
                self.ch2.freq = r & 0x7FF;
                self.ch2.length_enable = r & 0x4000 != 0;
                if offset == 0x06D && r & 0x8000 != 0 {
                    let env = self.reg16(0x08);
                    self.ch2.env.load(env);
                    self.ch2.on = env & 0xF800 != 0;
                    self.ch2.timer = self.ch2.period();
                    if self.ch2.length == 0 {
                        self.ch2.length = 64;
                    }
                }
            }
            0x070 => {
                self.ch3.two_banks = value & 0x20 != 0;
                self.ch3.bank = usize::from(value & 0x40 != 0);
                self.ch3.playing = value & 0x80 != 0;
                if !self.ch3.playing {
                    self.ch3.on = false;
                }
            }
            0x072 | 0x073 => {
                let r = self.reg16(0x12);
                self.ch3.length = 256 - (r & 0xFF);
                self.ch3.volume = ((r >> 13) & 3) as u8;
                self.ch3.force75 = r & 0x8000 != 0;
            }
            0x074 | 0x075 => {
                let r = self.reg16(0x14);
                self.ch3.freq = r & 0x7FF;
                self.ch3.length_enable = r & 0x4000 != 0;
                if offset == 0x075 && r & 0x8000 != 0 && self.ch3.playing {
                    self.ch3.on = true;
                    self.ch3.position = 0;
                    self.ch3.timer = (2048 - i64::from(self.ch3.freq)) * 8;
                    if self.ch3.length == 0 {
                        self.ch3.length = 256;
                    }
                }
            }
            0x078 | 0x079 => {
                let r = self.reg16(0x18);
                self.ch4.length = 64 - (r & 0x3F);
                if offset == 0x079 {
                    self.ch4.env.load(r);
                    if r & 0xF800 == 0 {
                        self.ch4.on = false;
                    }
                }
            }
            0x07C | 0x07D => {
                let r = self.reg16(0x1C);
                self.ch4.divisor = (r & 7) as u8;
                self.ch4.narrow = r & 8 != 0;
                self.ch4.shift = ((r >> 4) & 0xF) as u8;
                self.ch4.length_enable = r & 0x4000 != 0;
                if offset == 0x07D && r & 0x8000 != 0 {
                    let env = self.reg16(0x18);
                    self.ch4.env.load(env);
                    self.ch4.on = env & 0xF800 != 0;
                    self.ch4.lfsr = if self.ch4.narrow { 0x40 } else { 0x4000 };
                    self.ch4.timer = self.ch4.period();
                    if self.ch4.length == 0 {
                        self.ch4.length = 64;
                    }
                }
            }
            0x083 => {
                if value & 0x08 != 0 {
                    self.fifo[0].reset();
                }
                if value & 0x80 != 0 {
                    self.fifo[1].reset();
                }
                self.regs[o] &= 0x77;
            }
            0x084 if value & 0x80 == 0 => {
                // Master off clears PSG registers and channels.
                for r in &mut self.regs[0x00..0x24] {
                    *r = 0;
                }
                self.ch1 = Square::default();
                self.ch2 = Square::default();
                self.ch3 = Wave::default();
                self.ch4 = Noise::default();
            }
            _ => {}
        }
    }

    pub fn fifo_push(&mut self, channel: usize, value: i8) {
        self.fifo[channel].push(value);
    }

    /// Timer overflow from timer `timer` (0/1). Returns a bitmask of FIFOs
    /// that need a DMA refill.
    pub fn timer_overflow(&mut self, timer: usize, at: u64) -> u8 {
        self.run_until(at);
        let cnt_h = self.reg16(0x22);
        let mut requests = 0;
        for channel in 0..2 {
            let selected = ((cnt_h >> (10 + channel * 4)) & 1) as usize;
            if selected != timer {
                continue;
            }
            if self.fifo[channel].len == 0 {
                self.fifo_underruns += 1;
            }
            self.fifo[channel].pop();
            if self.fifo[channel].len <= 16 {
                requests |= 1 << channel;
            }
        }
        requests
    }

    fn clock_frame_sequencer(&mut self) {
        let step = self.frame_step;
        self.frame_step = (step + 1) & 7;
        if step & 1 == 0 {
            self.ch1.clock_length();
            self.ch2.clock_length();
            if self.ch3.length_enable && self.ch3.length > 0 {
                self.ch3.length -= 1;
                if self.ch3.length == 0 {
                    self.ch3.on = false;
                }
            }
            if self.ch4.length_enable && self.ch4.length > 0 {
                self.ch4.length -= 1;
                if self.ch4.length == 0 {
                    self.ch4.on = false;
                }
            }
        }
        if step == 2 || step == 6 {
            self.ch1.clock_sweep();
        }
        if step == 7 {
            self.ch1.env.clock();
            self.ch2.env.clock();
            self.ch4.env.clock();
        }
    }

    fn wave_output(&mut self, cycles: i64) -> i32 {
        let ch = &mut self.ch3;
        if !ch.on {
            return 0;
        }
        let period = (2048 - i64::from(ch.freq)) * 8;
        ch.timer -= cycles;
        let total = if ch.two_banks { 64 } else { 32 };
        while ch.timer <= 0 {
            ch.timer += period;
            ch.position = (ch.position + 1) % total;
            if ch.two_banks && ch.position == 0 {
                ch.bank ^= 1;
            }
            if ch.two_banks && ch.position == 32 {
                ch.bank ^= 1;
            }
        }
        let pos = ch.position % 32;
        let byte = self.wave_ram[ch.bank][pos / 2];
        let sample = i32::from(if pos & 1 == 0 { byte >> 4 } else { byte & 0xF });
        let centered = sample * 2 - 15;
        if ch.force75 {
            centered * 3 / 4
        } else {
            match ch.volume {
                0 => 0,
                1 => centered,
                2 => centered / 2,
                _ => centered / 4,
            }
        }
    }

    fn noise_output(&mut self, cycles: i64) -> i32 {
        let ch = &mut self.ch4;
        if !ch.on {
            return 0;
        }
        ch.timer -= cycles;
        let period = ch.period();
        while ch.timer <= 0 {
            ch.timer += period;
            let carry = ch.lfsr & 1;
            ch.lfsr >>= 1;
            if carry != 0 {
                ch.lfsr ^= if ch.narrow { 0x60 } else { 0x6000 };
            }
        }
        let v = i32::from(ch.env.volume);
        if ch.lfsr & 1 != 0 { v } else { -v }
    }

    fn mix_sample(&mut self, cycles: i64) -> (i16, i16) {
        self.ch1.advance(cycles);
        self.ch2.advance(cycles);
        let c1 = self.ch1.output();
        let c2 = self.ch2.output();
        let c3 = self.wave_output(cycles);
        let c4 = self.noise_output(cycles);
        if !self.master_on() {
            return (0, 0);
        }
        let cnt_l = self.reg16(0x20);
        let cnt_h = self.reg16(0x22);
        let psg = [c1, c2, c3, c4];
        let mut left = 0i32;
        let mut right = 0i32;
        for (i, v) in psg.iter().enumerate() {
            if cnt_l & (0x100 << i) != 0 {
                right += v;
            }
            if cnt_l & (0x1000 << i) != 0 {
                left += v;
            }
        }
        let vol_r = i32::from(cnt_l & 7) + 1;
        let vol_l = i32::from((cnt_l >> 4) & 7) + 1;
        let psg_shift = match cnt_h & 3 {
            0 => 2,
            1 => 1,
            _ => 0,
        };
        // PSG: up to 4 channels × ±15 × 8 → scale to the 10-bit DAC range.
        left = ((left * vol_l) >> psg_shift) * 2;
        right = ((right * vol_r) >> psg_shift) * 2;
        for channel in 0..2 {
            let full = cnt_h & (4 << channel) != 0;
            let s = i32::from(self.fifo[channel].current) * if full { 4 } else { 2 };
            if cnt_h & (0x100 << (channel * 4)) != 0 {
                right += s;
            }
            if cnt_h & (0x200 << (channel * 4)) != 0 {
                left += s;
            }
        }
        let bias = i32::from(self.reg16(0x28) & 0x3FE);
        let clamp = |v: i32| ((v + bias).clamp(0, 0x3FF) - bias) * 32;
        (clamp(left) as i16, clamp(right) as i16)
    }

    pub fn run_until(&mut self, now: u64) {
        loop {
            let next = self.next_sample.min(self.frame_seq);
            if next > now {
                break;
            }
            if self.frame_seq <= self.next_sample {
                self.frame_seq += FRAME_SEQ_CYCLES;
                self.clock_frame_sequencer();
                continue;
            }
            let elapsed = (self.next_sample - self.last_sample_cycle) as i64;
            self.last_sample_cycle = self.next_sample;
            self.next_sample += CYCLES_PER_SAMPLE;
            let (l, r) = self.mix_sample(elapsed);
            if self.samples.len() < 65536 {
                self.samples.push(l);
                self.samples.push(r);
            }
        }
    }
}
