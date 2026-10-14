# ADR-038 — ASR Thumb imediato como operação IR específica

- Estado: Proposed
- Data: 2026-10-14
- Supersede: —

## Contexto

Após SPEC-052, a discovery O2 da LeafGreen parou em `0x17C1` em `0x08000BEE`. SRC-39 confirma pelas fontes fixadas a inferência Thumb Format 1 `ASRS r1,r0,#31`. O projeto já implementa LSL e LSR imediatos, mas ASR usa preenchimento de sinal, carry próprio e o caso `imm5==0` significa shift 32.

## Decisão proposta

Adotar [SPEC-053](../specs/SPEC-053-thumb-asr-immediate.md) e adicionar `ThumbOperation::AsrImmediate { rd, rs, amount }`. Emitir ASR aritmético em LLVM; para `amount==0`, produzir explicitamente zero ou `0xFFFFFFFF` conforme o sinal e usar esse sinal como carry. Atualizar N/Z/C e preservar V e os demais bits. Propagar valores limitados em discovery, sem inferir flags. Não ampliar a aceitação para outros shifts.

Manter ABI/runtime, estado de máquina e `GBA_IR_VERSION` inalterados. O gate continua limitado ao orçamento detectado de dez slots do Mac mini M4.

## Alternativas

- Generalizar shifts LSL/LSR/ASR em uma operação nesta tranche: rejeitada por misturar contratos preexistentes e ampliar formatos não necessários ao frontier.
- Reutilizar LSR e corrigir o resultado depois: rejeitada porque a semântica do sinal, resultado e carry é distinta.
- Delegar a helper/interpreter: rejeitada por violar strict-AOT e não trazer benefício para uma operação puramente CPU.

## Consequências

ASR imediato poderá ser emitido no subset documentado, inclusive o encoding observado. O caso zero terá tratamento explícito que evita shift LLVM por 32. O avanço de discovery LeafGreen, caso ocorra, permanece evidência estática; não demonstra caminho guest, emissão/link do jogo ou compatibilidade.

## Critérios para revisitar

Revisitar uma IR genérica de shifts quando houver contrato conjunto para todas as formas imediato/registrador, incluindo regras ARMv4T de amount zero e carry, com fixtures de borda.

## Referências

- [SPEC-053](../specs/SPEC-053-thumb-asr-immediate.md).
- [SPEC-024 — Thumb LSL imediato](../specs/SPEC-024-thumb-lsl-immediate.md) e [SPEC-041 — Thumb LSR imediato](../specs/SPEC-041-thumb-lsr-immediate.md).
- [SRC-39](../knowledge/KNO-001-evidence.md).
- [KNO-004 — identidade e tentativa LeafGreen](../knowledge/KNO-004-rom-intake.md).
