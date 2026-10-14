# ADR-028 — LSR imediato Thumb em operação IR própria

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

SPEC-041 adiciona o próximo encoding estático da LeafGreen, `0x0C00` em `0x08000B80`, inferido como `LSR r0,r0,#16`. O projeto já implementa LSL imediato, mas LSR tem regra distinta para `imm5==0` e para o carry de saída.

## Decisão

Adicionar `ThumbOperation::LsrImmediate { rd, rs, amount }` com decodificação Format 1 `0x0800`. Para amount zero, modelar shift lógico de 32: resultado zero e carry do bit 31 antigo da fonte. Para 1–31, deslocar à direita e levar ao carry o bit `amount-1`. Atualizar N/Z/C, preservando V e demais flags/controle. Propagar exatos estados constantes na descoberta e baixar a operação para LLVM nativo strict-AOT.

## Alternativas

- Reutilizar `LslImmediate` ou renomear como shift genérico nesta tranche: rejeitada para manter a alteração pequena e evitar reescrever o contrato LSL existente.
- Tratar `imm5==0` como no-op por coincidência com uma operação LLVM de shift 0: rejeitada porque ARM define esse encoding como deslocamento de 32 para LSR.
- Interpretação/JIT ou helper runtime: rejeitados porque a semântica é puramente CPU e o modo continua strict-AOT.

## Consequências

Não há mudança de ABI, estado, IR versionada ou manifesto. ASR e shifts por registrador permanecem fora do subset. Fixtures O0/O2 verificam carry, flags preservadas, alias e amount zero.

## Critérios para revisitar

- A próxima fronteira exigir combinação mais ampla de shifts: avaliar uma representação genérica de shift com semântica ARMv4T completamente especificada.
- Evidência de referência/oráculo divergir da regra para shift zero ou carry.

## Referências

- [SPEC-041 — Thumb LSR imediato](../specs/SPEC-041-thumb-lsr-immediate.md)
- [SPEC-024 — Thumb LSL imediato](../specs/SPEC-024-thumb-lsl-immediate.md)
- [KNO-004 — LeafGreen fixada](../knowledge/KNO-004-rom-intake.md)
