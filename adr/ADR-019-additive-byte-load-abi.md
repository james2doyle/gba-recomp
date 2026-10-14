# ADR-019 — Helper aditivo de load byte no ABI v3

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

A SPEC-029 define Thumb LDRB imediato. `GbaExitV3` é reservado ao status e faults; alterar seu layout para transportar um valor carregado quebraria o contrato versionado. O ABI expõe EWRAM/IWRAM físicas e não fornece um barramento geral.

## Decisão

Adicionar o símbolo `gba_load8_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t guest_ea, uint8_t *host_value_out)` ao runtime v3. O EA permanece um inteiro guest e é validado antes do acesso. `host_value_out` é um ponteiro host para temporário local de um byte gerado pelo LLVM; não é endereço guest. O helper lê exatamente um byte nas faixas físicas EWRAM/IWRAM e escreve no temporário somente após validar contexto, memória, saída e região. O compilador faz zero-extension e atualiza `Rd` apenas no caminho de sucesso.

Em falha, o helper retorna `GBA_EXIT_FAULT_V2`, informa PC e EA original quando contexto e estrutura de saída permitem, e usa os códigos de falha existentes. `rt == NULL` mantém PC sentinela zero. Se `out == NULL` ou `host_value_out == NULL`, retorna fault sem modificar `out`, o temporário ou o estado guest. O helper não atualiza CPU ou ciclos.

A adição não altera layout ou versão dos ABIs v3/v4, memória v1 ou manifesto. O caminho AOT valida ABI v4 na entrada de bloco. Strict-AOT não cai em interpretação ou JIT.

## Alternativas

- Acrescentar campo de resultado a `GbaExitV3` quebraria seu layout e misturaria valor de load com relatório de saída/fault.
- Retornar status e byte empacotados em `uint64_t` cria um protocolo especial diferente dos demais helpers.
- Converter EA guest em ponteiro host pode acessar memória host indevida e viola a separação do runtime.
- Aceitar mirrors, MMIO, vídeo, ROM ou outras regiões excederia o subset de memória contratado.

## Consequências

Pacotes com LDRB Thumb imediato linkam o novo helper. O byte lido só afeta o estado guest após sucesso; endereços fora do escopo produzem fault explícito. O helper não promete timing físico nem equivalência de bus geral.

## Referências

[SPEC-029](../specs/SPEC-029-thumb-ldrb-immediate.md), [SPEC-028](../specs/SPEC-028-thumb-strb-immediate.md), [ADR-009](ADR-009-versioned-bus-context.md), [SRC-30/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
