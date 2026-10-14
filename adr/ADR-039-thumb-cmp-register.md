# ADR-039 — CMP Thumb Format 4 como operação IR específica

- Estado: Proposed
- Data: 2026-10-14
- Supersede: —

## Contexto

A descoberta O2 local da LeafGreen fixada em KNO-004 passou por ASR imediato e parou no raw `0x428A` em `0x08000C00`. SRC-47 identifica o encoding como `CMP r2,r1` pelas fontes upstream fixadas. O vendor já decodifica e emite essa comparação, embora seu build híbrido não demonstre full recomp; consulte KNO-006. A evidência de fonte confirma semântica, mas não valores guest nem execução isolada dessa instrução.

## Decisão proposta

Adotar [SPEC-054](../specs/SPEC-054-thumb-cmp-register.md) e adicionar `ThumbOperation::CompareRegister { rd, rs }` apenas para a máscara `(encoding & 0xFFC0) == 0x4280`. Reusar o lowering nativo de subtração sem writeback. Generalizar o certificado interno `pending_compare` para RHS imediato ou registrador, permitindo refinar somente o branch Thumb adjacente quando os dois valores forem conhecidos; manter os dois edges para estados desconhecidos. `CMP Rd,Rd` é sempre zero, mesmo quando o valor do registrador é desconhecido.

Preservar ABI/runtime, estado da máquina e `GBA_IR_VERSION`. Não copiar implementação do vendor nem modificar `vendor-ref/`.

## Alternativas

- Aceitar todo Thumb Format 4 em uma tranche: rejeitada porque TST, CMN e demais suboperações têm contratos ainda ausentes.
- Emitir CMP como SUB com writeback descartado por convenção: rejeitada porque o tipo IR deve tornar explícita a ausência de destino e o verificador precisa validar a semântica.
- Não refinar discovery após CMP register: rejeitada porque a prova existente de CMP imediato suporta comparação exata adjacente e os estados completos já guardam valores concretos por registrador; a versão register preserva ambos edges para Unknown, salvo o caso idêntico `Rd == Rs`, que prova igualdade sem conhecer o valor.
- Usar interpretação ou JIT: rejeitada pelo contrato strict-AOT.

## Consequências

O encoding `0x428A` poderá ser aceito localmente se suas instruções posteriores também pertencerem ao subset. Isso pode mover a fronteira de discovery de LeafGreen, mas não prova execução, emissão ou pacote do jogo. Estados com operandos desconhecidos conservam os dois edges, exceto comparações de um registrador consigo mesmo; operações entre CMP e branch invalidam a prova.

## Critérios para revisitar

Revisitar o certificado se a discovery passar a acompanhar flags geradas por operações além de CMP imediatamente anterior a branch. Uma IR ALU genérica requer contratos para todas as suboperações e seus efeitos de flags/writeback.

## Referências

- [SPEC-054](../specs/SPEC-054-thumb-cmp-register.md).
- [SRC-47](../knowledge/KNO-001-evidence.md).
- [SPEC-037 — refinamento de CMP imediato e branch adjacente](../specs/SPEC-037-thumb-ldmia-rom-cases.md).
- [KNO-006 — audit vendor gba-recomp](../knowledge/KNO-006-vendor-gba-recomp.md).
