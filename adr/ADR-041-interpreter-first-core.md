# ADR-041 — Núcleo GBA interpretado como base do runtime

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

O objetivo do ROADMAP é recompilar jogos de ponta a ponta (G4). Até a SPEC-054 o produto avançou uma instrução por spec seguindo a fronteira de descoberta estática da LeafGreen; o runtime C atual (`runtime_v2/v3/hle_v1`) cobre só transfers físicos, latches IE/IF/IME e SWI #1. Jogos comerciais exigem CPU ARM7TDMI inteira, bus com mirrors e MMIO, scheduler, IRQ, DMA, timers, PPU, APU, BIOS e backup. Sem esse runtime nenhum caminho, nativo ou interpretado, executa um jogo. A SPEC-055 já define o modo `aot-compatible`, mas depende de um interpretador local completo que não existe.

## Decisão

1. Criar o crate Rust `gba-core`, sem dependências externas, contendo CPU interpretada ARM7TDMI completa, bus, scheduler, IRQ, DMA, timers, PPU, APU, BIOS HLE, backup e RTC. Ele é o `gba-runtime`/`gba-compat` previsto em `docs/architecture.md`.
2. Ordem de entrega: CPU (SPEC-056) → bus/scheduler/IRQ/DMA/timers (SPEC-057) → PPU (SPEC-058) → APU (SPEC-059) → BIOS HLE/backup/RTC (SPEC-060) → runner headless e frontend desktop (SPEC-061) → integração `aot-compatible` com blocos nativos (SPEC-062).
3. O núcleo interpretado é o executor completo e a referência interna. Blocos LLVM passam a ser aceleração medida sobre o mesmo estado (SPEC-062); `strict-aot` permanece padrão e inalterado.
4. mGBA fixado em `vendor-ref/mgba` é oráculo externo para diferencial (VAL-03), executado como processo separado. Nenhum código mGBA ou vendor é copiado; a semântica vem de documentação pública (GBATEK/ARM ARM) e testes próprios.

## Alternativas

- Continuar strict-AOT incremental: centenas de specs antes de qualquer frame, e o hardware ainda faltaria.
- Embutir mGBA: rápido, mas acopla licença MPL-2.0, scheduler e estado alheios ao contrato da ABI.
- Reusar o interpretador do vendor `gba-recomp`: exige análise de licença por arquivo e adaptação; ADR-040 já rejeitou nesta etapa.

## Consequências

Trabalho grande e mensurável por subsistema. O caminho até um jogo rodar passa a não depender de cobertura AOT. Pacotes que usam o núcleo são `aot-compatible` e reportam instruções interpretadas; nenhum é rotulado strict-AOT. O runtime C atual continua servindo o modo strict-AOT até a SPEC-062 decidir sua convergência.

## Critérios para revisitar

Se o diferencial contra mGBA revelar divergências sistêmicas de timing, ou se o custo de integrar blocos nativos ao estado Rust inviabilizar a ABI C, revisitar o item 3.

## Referências

[ADR-001](ADR-001-core-architecture.md), [ADR-040](ADR-040-interpreter-assisted-compatibility.md), [SPEC-002](../specs/SPEC-002-armv4t-semantics.md), [SPEC-005](../specs/SPEC-005-runtime-timing.md), [SPEC-055](../specs/SPEC-055-interpreter-assisted-compatibility.md), [SPEC-056](../specs/SPEC-056-armv4t-interpreter.md).
