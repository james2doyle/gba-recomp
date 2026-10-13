# ADR-010 — Compilar MOV ARM entre registradores sem shift

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Após a primeira escrita física de RAM, a descoberta LeafGreen parou em `0x0800022C`, encoding `0xE1A0E00F`. A leitura como `MOV LR,PC` é inferência pelos campos ARM. O próximo incremento deve cobrir este caso sem habilitar genericamente o shifter ou as demais operações de data processing.

## Decisão

Aceitar somente `MOV Rd,Rm` ARM com `LSL #0`, `S=0`, `Rd` diferente de PC e `Rn=0`. Um `Rm=PC` lê `guest_pc+8`. Preservar a predicação ARM, CPSR, PC de controle e ciclo conforme SPEC-017.

## Consequências

A forma é uma cópia de valor guest, sem leitura de memória, acesso de bus ou escrita de PC. Outros shifts, `MOVS` e destino PC ficam recusados até haver contratos e fixtures próprios. A semântica foi consultada nas referências fixadas, mas esta decisão não usa código copiado nem declara compatibilidade do jogo.

## Critérios para revisitar

Revisitar quando uma forma de shifter, atualização de flags ou escrita de PC alcançada pela ROM tiver casos próprios no mapa KNO-005 e uma spec incremental.

## Referências

[SPEC-017](../specs/SPEC-017-arm-mov-register.md), [SPEC-002](../specs/SPEC-002-armv4t-semantics.md), [SRC-18/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
