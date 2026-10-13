# ADR-011 — Fechar destinos BX para dispatch AOT por modo

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Depois do MOV `LSL #0`, a descoberta LeafGreen parou em `0x08000230`, `0xE12FFF11`, provável `BX r1`. A ABI v3 já carrega CPSR e PC, mas o dispatcher atual só aceita instruções ARM. Um branch exchange pode selecionar ARM ou Thumb e um endereço que não esteja no programa emitido.

## Decisão

Implementar BX ARM/Thumb com semântica de T/PC explícita, descobrir destinos por propagação conservadora de constantes e emitir switches independentes por modo. A compilação falha se não puder provar um conjunto finito de destinos dentro da ROM e de código suportado. Em execução, um destino fora do conjunto compilado retorna `missing_code`.

## Consequências

A primeira passagem pelo modo Thumb terá apenas BX. Não muda ABI v3 e não adiciona fallback. A análise é um limite conservador, não prova que todo destino de uma ROM foi enumerado; cada destino ainda precisa de decoder, IR e lowering AOT próprios.

## Critérios para revisitar

Expandir para instruções Thumb conforme surgirem no grafo AOT, com requisitos separados de flags, PC, memória e fixtures. Se destinos BX dependerem de valores de memória, SPEC-003/005 deve definir a fonte de dados e seus efeitos antes de relaxar a análise.

## Referências

[SPEC-018](../specs/SPEC-018-arm-thumb-branch-exchange.md), [SPEC-002](../specs/SPEC-002-armv4t-semantics.md), [SRC-19/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
