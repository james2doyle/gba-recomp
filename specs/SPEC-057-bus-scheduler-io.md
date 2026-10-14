# SPEC-057 — Bus, scheduler, IRQ, DMA e timers do núcleo `gba-core`

Estado do documento: Accepted em 2026-10-14. Progresso: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

Implementar no `gba-core` o contrato da [SPEC-005](SPEC-005-runtime-timing.md) necessário para executar jogos: mapa de memória completo com mirrors e widths, MMIO, waitstates, scheduler em ciclos, interrupções, HALT, quatro canais DMA e quatro timers. Vídeo, áudio, BIOS e backup consomem esta base (SPEC-058–060).

## Fora de escopo

Prefetch buffer exato do Game Pak (modelado como custo agregado, ver BUS-57-04), serial/link (SIO responde desconectado), latência sub-instrução de IRQ.

## Requisitos normativos

- BUS-57-01: regiões e mirrors: BIOS 16 KiB (leitura fora da BIOS com PC fora dela devolve o último opcode lido da BIOS); EWRAM 256 KiB espelhada a cada 0x40000 em 0x02xxxxxx; IWRAM 32 KiB espelhada a cada 0x8000; I/O 0x04000000–0x040003FE (sem mirror, exceto 0x04xx0800 de MEMCNT); Palette 1 KiB espelhada a cada 0x400; VRAM 96 KiB em janela de 128 KiB (0x18000–0x1FFFF espelha 0x10000–0x17FFF); OAM 1 KiB espelhada; ROM em 0x08/0x0A/0x0C com espelho de tamanho; SRAM/Flash em 0x0E–0x0F com barramento de 8 bits.
- BUS-57-02: writes de byte em Palette e VRAM BG MUST duplicar o byte no halfword; writes de byte em OAM e VRAM OBJ MUST ser ignoradas. Leitura não mapeada MUST devolver open bus (último valor do prefetch da CPU).
- BUS-57-03: acessos halfword/word MUST alinhar o endereço; a rotação de LDR fica na CPU (SPEC-056).
- BUS-57-04: custo por acesso: BIOS/IWRAM/I/O/OAM 1 ciclo; Palette/VRAM 1 (16 bits) ou 2 (32 bits); EWRAM 3 (16 bits) ou 6 (32 bits); Game Pak conforme WAITCNT (N = 4/3/2/8, S por waitstate 0/1/2), 32 bits = N+S; SRAM conforme WAITCNT. Prefetch habilitado MAY reduzir acessos sequenciais de código da ROM a 1 ciclo; a escolha MUST constar do manifesto.
- SCH-57-01: o scheduler MUST manter um relógio `u64` monotônico e eventos ordenados por (ciclo, prioridade fixa documentada: PPU, timers, DMA, APU). Antes de cada passo de CPU, eventos com ciclo ≤ agora são processados; HALT avança direto ao próximo evento.
- IRQ-57-01: IE (0x200), IF (0x202, write-one-to-clear) e IME (0x208) controlam a linha IRQ; fontes: VBlank, HBlank, VCount, Timer0–3, Serial, DMA0–3, Keypad, GamePak. A linha é `IME & (IE & IF) != 0`.
- IRQ-57-02: HALTCNT (0x301) MUST parar a CPU até `IE & IF != 0` (independente de IME); STOP MUST parar até keypad/serial/gamepak IRQ.
- DMA-57-01: quatro canais com SAD/DAD/CNT_L/CNT_H; start imediato, VBlank, HBlank (não em VBlank), especial (DMA1/2 FIFO de som, DMA3 captura de vídeo reportada como capability ausente); repeat, controle de endereço incremento/decremento/fixo/reload; largura 16/32; count 0 = 0x4000 (0x10000 no DMA3); máscaras de endereço por canal; IRQ ao fim; prioridade 0 > 3. DMA de FIFO transfere 4 words com destino fixo.
- DMA-57-02: transferências MUST passar pelo mesmo bus da CPU (MMIO, waitstates); custo 2N + 2(n−1)S + 2I e a CPU fica parada durante a transferência.
- TIM-57-01: quatro timers com reload, prescaler 1/64/256/1024, cascade (timers 1–3), IRQ em overflow e notificação ao APU para FIFOs (timers 0/1). Leituras de contador MUST refletir o ciclo corrente sem depender de eventos por incremento.
- KEY-57-01: KEYINPUT (0x130) ativo em 0, KEYCNT com IRQ por OR/AND. Entrada vem do host por frame (SPEC-061).
- IO-57-01: registradores sem modelo MUST ler como valor armazenado (ou 0 se write-only), e o primeiro acesso a cada registrador não modelado MUST ser contado em `unsupported_hardware` com o endereço.

## Interfaces

`Gba::new(rom, bios_policy, backup)`, `Gba::run_frame(keys) -> FrameResult`, `Gba::step()`. Estado inteiro serializável sem ponteiros host (base para STATE-01).

## Critérios de aceite e matriz de testes

Mirrors de cada região; byte write em Palette/VRAM/OAM; open bus; WAITCNT alterado em execução; IF write-one-to-clear; HALT com IRQ pendente e com IME=0; DMA imediato/VBlank/HBlank, repeat, reload, count 0, decremento; timers cascade e overflow simultâneos; leitura de contador entre overflows. Diferencial com mGBA de ciclos por frame em ROMs de teste quando disponível.

## Verificação

Testes unitários em `crates/gba-core/src/bus/tests.rs` (35 testes, `cargo test -p gba-core`, 2026-10-14) cobrem: mirrors de EWRAM/IWRAM/Palette/VRAM (janela de 128 KiB)/OAM/ROM (0x08/0x0A/0x0C e leitura além do fim = endereço/2)/SRAM; BIOS protegida devolvendo o último opcode; open bus (BIOS acima de 0x4000, 0x01xxxxxx, 0x10+, I/O acima de 0x400, fetch Thumb replicado); duplicação de byte em Palette/VRAM BG e descarte em OAM/VRAM OBJ (modos texto e bitmap); alinhamento halfword/word; custos por região, WAITCNT alterado em execução e prefetch; IF write-one-to-clear (halfword e byte), linha IRQ, writes de byte em IE/IME/WAITCNT/DMA/timers; HALTCNT/STOP; DMA imediato/VBlank/HBlank (só linhas visíveis), repeat, reload, recarga de count, count 0 (0x4000/0x10000), 0x4000 explícito, incremento/decremento/fixo, 16/32 bits, IRQ ao fim, prioridade 0 > 3 e custo; timers com prescalers, reload, IRQ, leituras entre overflows, cascade, overflows simultâneos, parada/reinício e TM0 ignorando cascade; DISPSTAT/VCOUNT; KEYCNT OR/AND; contagem de MMIO não modelado. HALT com IME=0 e o caminho do stub IRQ são testados em `gba/tests.rs`. O diferencial de ciclos contra mGBA continua pendente. Bugs corrigidos ao escrever os testes: [KNO-007](../knowledge/KNO-007-interpreted-core.md#bugs-corrigidos-pelos-testes-unitários).

## Referências

[SPEC-005](SPEC-005-runtime-timing.md), [SPEC-056](SPEC-056-armv4t-interpreter.md), [ADR-041](../adr/ADR-041-interpreter-first-core.md), GBATEK.
