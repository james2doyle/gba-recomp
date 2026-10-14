# SPEC-061 — Runner headless e frontend desktop do núcleo

Estado do documento: Accepted em 2026-10-14. O runner headless (`gba-run`) está implementado; o frontend desktop é a próxima etapa. Progresso: [ROADMAP](../ROADMAP.md).

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
