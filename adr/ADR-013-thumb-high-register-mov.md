# ADR-013 — MOV high-register Thumb permanece sem flags e sem escrita de PC

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Depois de suportar `PUSH {r4-r7,lr}`, a descoberta LeafGreen parou em `0x080003A6`, encoding Thumb `0x4647`, provavelmente `MOV r7,r8`. O formato compartilha a família de `ADD`, `CMP` e `BX`, mas essas operações têm efeitos distintos em flags ou fluxo.

## Decisão

Implementar somente `MOV Rd,Rm` high-register, sem flags e com destinos r0–r14. Aceitar `Rm=PC` usando `guest_pc+4`; recusar destino PC e manter `ADD`, `CMP` e outros formatos sem suporte explícito. Preservar os registradores high no estado e no lowering AOT.

## Consequências

A instrução pode copiar entre bancos de baixa/alta faixa e alimentar branches indiretos estáticos. O limite no destino evita tratar escrita de PC como branch sem uma semântica mode-aware própria.

## Critérios para revisitar

Adicionar `MOV PC,Rm`, `ADD`, `CMP` ou outras operações high-register somente com efeitos de PC/flags e fixtures definidos em contratos separados ou extensão verificável desta spec.

## Referências

[SPEC-020](../specs/SPEC-020-thumb-high-mov.md), [SPEC-018](../specs/SPEC-018-arm-thumb-branch-exchange.md), [SRC-21/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
