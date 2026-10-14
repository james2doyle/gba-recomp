# ADR-037 — Implementar NEGS Thumb Format 4 como operação específica

- Estado: Proposed
- Data: 2026-10-14
- Supersede: —

## Contexto

Após SPEC-051, a discovery O2 da LeafGreen fixada em KNO-004 parou em `0x4248` no PC `0x08000BEA`. Seus campos inferem `NEGS r0,r1`; SRC-46 registra a confirmação do decode e da semântica pelas revisões fixadas de mGBA e gba-recomp. A ROM não forneceu valores guest observados, execução, pacote ou decompilação.

O projeto já tem ALU aditiva/subtrativa e operações Thumb Format 4 ANDS/ORRS, mas NEGS atualiza NZCV de forma diferente das operações lógicas e não pode ser modelada como AND/OR ou SUB imediato existente.

## Decisão proposta

Adotar [SPEC-052](../specs/SPEC-052-thumb-negate-register.md) e adicionar uma variante `NegateRegister` dedicada. Emitir `0 - Rs` e recalcular NZCV exatamente; propagar somente valores de registradores dentro do domínio limitado e manter discovery de flags conservadora. Não ampliar automaticamente a aceitação para as demais suboperações Format 4.

Manter ABI/runtime, estado de máquina e `GBA_IR_VERSION` inalterados. Respeitar o limite detectado de CPU do host: dez slots no Mac mini M4, sem permitir orçamento superior.

## Alternativas

- Generalizar todos os Format 4: rejeitada por incluir semânticas de shifts, carry, multiplicação e flags que ainda não têm contratos próprios.
- Reutilizar ORRS/ANDS: rejeitada porque NEGS grava C/V e usa subtração com zero.
- Usar fallback de interpretação: rejeitada por violar strict-AOT.
- Paralelizar discovery sem prova de determinismo: rejeitada porque a ordem da worklist interage com budgets e limites de alternativas.

## Consequências

NEGS poderá ser decodificada e emitida em AOT estrito para os campos documentados. Isso deve avançar o frontier LeafGreen se esse caminho estático permanecer o mesmo; não prova que valores ou flags guest são os esperados em runtime e não fecha suporte do jogo. O gate pode usar dez slots disponíveis para testes/fixtures independentes; a compilação de uma ROM ainda fica limitada pela discovery serial.

## Critérios para revisitar

Revisitar tracking de flags na discovery somente com contrato separado, invariantes de convergence e testes de edges verdadeiros/falsos. Cada suboperação Format 4 restante requer critérios próprios.

## Referências

- [SPEC-052](../specs/SPEC-052-thumb-negate-register.md).
- [SPEC-051 — ANDS Thumb Format 4](../specs/SPEC-051-thumb-and-register.md).
- [SRC-46](../knowledge/KNO-001-evidence.md).
- [KNO-004 — identidade e tentativa LeafGreen](../knowledge/KNO-004-rom-intake.md).
