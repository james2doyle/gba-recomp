# ADR-017 — Helper aditivo de store halfword no ABI v3

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

O limite estático de LeafGreen em `0x080003B8` contém `0x8008`, inferido como `STRH r0,[r1,#0]`. O runtime possui `GbaMemoryV1` com EWRAM/IWRAM e stores word por `gba_store32_v3`, mas a função de palavra exige alinhamento de quatro bytes e escreve quatro bytes. A SPEC-016 proíbe tratar o mapa físico parcial como bus completo.

## Decisão

Adicionar o símbolo `gba_store16_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t guest_ea, uint32_t value)` como extensão aditiva do ABI v3, sem campos novos ou mudança de layout/versão em ABI v3, ABI v4 ou memória. O helper trunca para `u16`, normaliza o endereço como `guest_ea & ~1`, valida o halfword completo somente em EWRAM/IWRAM físicas e grava little-endian. Em fault, retorna `GBA_EXIT_FAULT_V2`; quando `rt != NULL`, informa o PC guest corrente e o endereço guest original e usa os códigos existentes de ABI, memória nula ou região. Para `rt == NULL`, usa PC zero como sentinela, preserva `guest_ea` em `fault_address` e usa `GBA_MEMORY_FAULT_RUNTIME_V3`, porque a assinatura não recebe o PC em separado. Com `out == NULL`, retorna fault sem escrita e sem relatório. O helper não altera CPU nem memória ao falhar. O código gerado continua passando endereço guest inteiro; nenhum ponteiro guest vira ponteiro host.

## Alternativas

- Reusar `gba_store32_v3` escreveria largura e bytes incorretos e aplicaria alinhamento inadequado.
- Aumentar ou alterar `GbaRuntimeV3/V4` não é necessário: o estado e a memória existentes bastam.
- Um cast de endereço guest para ponteiro host viola a separação guest/host e permite acesso host inválido.
- Aceitar mirrors ou MMIO extrapolaria SPEC-016 e esconderia regras ainda não modeladas do bus.

## Consequências

Pacotes que emitem STRH precisam linkar a nova função do runtime v3. Versões e layouts já publicados permanecem iguais. Endereços fora das duas RAMs físicas falham explicitamente; não há fallback para interpretação/JIT. Alinhamento descendente para halfword segue as fontes fixadas, aplica-se somente a `gba_store16_v3` e não é alegação de conformance em silício.

## Referências

[SPEC-027](../specs/SPEC-027-thumb-strh-immediate.md), [SPEC-016](../specs/SPEC-016-ram-word-stores.md), [ADR-009](ADR-009-versioned-bus-context.md), [SRC-28/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
