# ADR-036 — Implementar ANDS Thumb Format 4 como operação específica

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

Depois da implementação do branch Thumb Format 18, a discovery O2 da LeafGreen fixada em KNO-004 passou de `0xE00A` em `0x08000AE6` e encontrou `0x4002` em `0x08000BE4`. Os campos inferem `ANDS r2,r0`; mGBA e gba-recomp nas revisões registradas em SRC-45 corroboram o decode e os efeitos de N/Z. Não houve execução guest ou decompilação da ROM.

O produto já suporta ORR Format 4, operações lógicas com propagação abstrata limitada e lowering LLVM que altera N/Z. As demais suboperações Format 4 continuam rejeitadas.

## Decisão proposta

Adotar [SPEC-051](../specs/SPEC-051-thumb-and-register.md) e adicionar uma variante `AndRegister` dedicada. Reutilizar o domínio existente de AND abstrato para propagar valores sem ultrapassar 16 alternativas; manter a discovery de flags conservadora e manter branches condicionais com ambos os edges quando não houver prova independente. Emitir o AND diretamente em LLVM e preservar C/V e os demais campos CPSR. A implementação foi concluída no commit `d26cc8d2c8817bfc517fc9bc36c9ee74acb514f8`; as fixtures O0/O2, testes workspace e nova tentativa estática constam no [relatório de evidência](../knowledge/evidence/gba020-thumb-ands-macos-arm64.json).

Manter ABI/runtime, estado de máquina e `GBA_IR_VERSION` inalterados. Não ampliar automaticamente a cobertura para outra operação Format 4, ainda que a infraestrutura seja parecida.

## Alternativas

- Generalizar todas as operações Format 4 nesta tranche: rejeitada por expandir a superfície sem fixtures semânticas equivalentes.
- Representar toda ALU lógica por uma operação parametrizada: rejeitada; mistura formatos e efeitos de flags antes de haver contratos próprios.
- Usar o intérprete do vendor ou fallback em runtime: rejeitada por violar strict-AOT e a ABI GBA local.
- Não avançar a partir do erro LeafGreen: mantém a lacuna mesmo com uma operação pequena que pode ser coberta com fixtures isoladas.

## Consequências

ANDS poderá ser compilado em AOT estrito para seus formatos alcançáveis. A descoberta acompanha valores de registradores, mas não flags lógicas; por isso não elimina edges de branch com base em N/Z de ANDS. A medida não prova runtime DMA, hardware geral, emissão/link da ROM LeafGreen ou suporte ao título.

## Critérios para revisitar

Revisitar o limite do domínio abstrato se fixtures reais excederem 16 valores de modo recorrente; revisitar tracking de flags somente com contrato separado, invariantes de convergence e testes de edges falsos/verdadeiros. Cada suboperação Format 4 restante exige critérios e evidências próprios.

## Referências

- [SPEC-051](../specs/SPEC-051-thumb-and-register.md).
- [SPEC-030 — ORR Thumb Format 4](../specs/SPEC-030-thumb-orr-register.md).
- [SRC-45](../knowledge/KNO-001-evidence.md).
- [KNO-004 — identidade e tentativa LeafGreen](../knowledge/KNO-004-rom-intake.md).
