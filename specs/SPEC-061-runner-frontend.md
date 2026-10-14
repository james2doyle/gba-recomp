# SPEC-061 — Runner headless e frontend desktop do núcleo

Estado do documento: Accepted em 2026-10-14. O runner headless (`gba-run`) está implementado. FE-61-01 está implementado em `gba-desktop` (SDL3 3.4 via FFI mínima escrita à mão, sem dependências novas): janela com escala inteira (`--scale`, padrão 3x) letterbox 3:2 e filtro nearest, áudio S16 estéreo 32768 Hz com fila limitada a ~100 ms (contadores de underrun/overrun; sem dispositivo a emulação continua, AUD-03), teclado/gamepad com hotplug, ritmo pelo relógio monotônico a ~59,7275 Hz com fast-forward só de apresentação (Tab), save atômico quando sujo (saída e a cada ~10 s) e fault com saída não zero. Verificado: build, clippy, testes unitários e smoke `--frames 300` da LeafGreen com `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy` (saída 0) e sem driver de áudio; janela real, gamepad físico e qualidade de áudio ainda não foram verificados manualmente. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Executar o núcleo sobre ROMs reais de forma reproduzível (VAL-01) e, depois, de forma jogável com vídeo, áudio e controles (G2).

## Requisitos normativos

- RUN-61-01: `gba-run --rom R --frames N [--input FRAME:LEN:KEYS]… [--save F] [--backup K] [--bios B] [--ppm P] [--ppm-every N] [--hashes]` MUST ser determinístico para as mesmas entradas e reportar JSON com modo (`interpreted`), frames, instruções interpretadas, ciclos guest, hash do framebuffer, SWIs, MMIO sem modelo, fault e tempo host.
- RUN-61-02: fault de CPU ou SWI não suportado MUST encerrar com código de saída diferente de zero e o fault no relatório.
- RUN-61-03: o save MUST ser persistido atomicamente apenas se sujo (SAVE-60-01).
- FE-61-01: o frontend desktop MUST apresentar o framebuffer com escala inteira, tocar o áudio canônico com buffer limitado, mapear teclado/gamepad para KEYINPUT e regular o ritmo pelo tempo guest (~59,73 Hz), sem que preferências host alterem a simulação (VID-03/04, AUD-02).

## Limites

Rótulo do runner: `interpreted`. Não é pacote recompilado; pacotes `aot-compatible` com blocos nativos são da SPEC-062.

## Referências

[SPEC-009](SPEC-009-settings-menu.md), [SPEC-011](SPEC-011-verification.md), [SPEC-057](SPEC-057-bus-scheduler-io.md).
