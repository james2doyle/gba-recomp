# ADR-014 — MOVS Thumb imediato atualiza somente N/Z

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Após implementar `PUSH` e MOV high-register Thumb, a compilação estática de LeafGreen avançou até `0x080003AA`, encoding `0x20FF`, provável `MOVS r0,#0xFF`. O formato compartilha o grupo Thumb de MOV/CMP/ADD/SUB com imediato, mas somente MOV entra nesta tranche.

## Decisão

Implementar `MOVS Rd,#imm8` como operação Thumb própria. A escrita atualiza N e Z pelo resultado zero-estendido e preserva C/V, os demais bits de CPSR e o modo. Aceitar apenas registradores r0–r7 e o subopcode MOV (`encoding & 0xF800 == 0x2000`). As outras operações do grupo permanecem recusadas.

## Consequências

O caso imediato passa por decoder, IR, propagação de constantes e lowering AOT explícitos, sem interpretação/JIT. A semântica não é generalizada automaticamente para as outras operações da família ALU; elas precisam de contratos para flags e resultados próprios.

## Critérios para revisitar

Ampliar o grupo Format 3 somente com uma spec e fixtures que cubram resultados e flags de cada opcode. Revisitar a representação comum de ALU Thumb quando mais operações estiverem implementadas.

## Referências

[SPEC-021](../specs/SPEC-021-thumb-movs-immediate.md), [SRC-22/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
