# ADR-018 — Helper aditivo de store byte no ABI v3

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

A SPEC-028 define Thumb STRB imediato com escritas limitadas às duas RAMs físicas modeladas. `gba_store16_v3` e `gba_store32_v3` não podem representar corretamente uma escrita de byte: o primeiro alinha e escreve dois bytes, o segundo quatro bytes alinhados.

## Decisão

Adicionar o símbolo `gba_store8_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t guest_ea, uint32_t value)` ao runtime v3. Ele grava `value & 0xFF` no endereço guest exato somente dentro de EWRAM/IWRAM físicas, sem alinhamento. Em falha, retorna `GBA_EXIT_FAULT_V2`, informa PC e EA original quando a estrutura de saída/contexto permitem, e usa os códigos de falha existentes. Falhas são atômicas. O endereço permanece inteiro guest; nenhum ponteiro guest é convertido em ponteiro host.

A adição não altera layout ou versão do ABI v3/v4, memória v1 ou manifesto. O caminho AOT valida ABI v4 na entrada de bloco. Strict-AOT não cai em interpretação ou JIT.

## Alternativas

- Reusar `gba_store16_v3` ou `gba_store32_v3` grava largura/alinhamento incorretos.
- Mudar layouts ou versões ABI não é necessário; o helper recebe o estado existente.
- Aceitar mirrors, MMIO, VRAM, OAM ou outras regiões excederia o subset de barramento contratado.
- Converter EA guest em ponteiro host viola a separação do runtime e pode acessar memória host indevida.

## Consequências

Pacotes com STRB Thumb imediato linkam o novo helper. O store afeta somente um byte nas duas RAMs físicas, e endereços fora do escopo produzem fault explícito sem mutação. O helper não promete timing físico nem equivalência de bus geral.

## Referências

[SPEC-028](../specs/SPEC-028-thumb-strb-immediate.md), [SPEC-027](../specs/SPEC-027-thumb-strh-immediate.md), [SPEC-016](../specs/SPEC-016-ram-word-stores.md), [ADR-009](ADR-009-versioned-bus-context.md), [SRC-29/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
