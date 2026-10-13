# ADR-009 — Introduzir bus guest versionado para STR em RAM

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Depois de habilitar o ADD PC-relativo, o compile da ROM local alcançou `0xE5810000` em `0x08000224`. Sua leitura como `STR r0,[r1]` é inferência pelos campos ARM. O runtime v2 não contém bus ou memória mutável, e endereços guest não podem ser reinterpretados como ponteiros host.

## Decisão proposta

Adicionar ABI v3 preservando `GbaRuntimeV2`, com um contexto `GbaMemoryV1` separado. O primeiro helper aceita somente writes alinhados de palavra em EWRAM/IWRAM físicas. Ele valida faixa e ABI antes da escrita e retorna falha com endereço guest para todas as outras regiões. Sem interpretação/JIT.

## Alternativas

- Cast de `r1` para ponteiro host violaria a separação guest/host e permitiria corrupção do processo.
- Acrescentar RAM ao layout v2 quebraria o contrato congelado e mudaria a ABI existente.
- Aceitar todos os endereços com memória linear esconderia MMIO, VRAM, ROM e regras de backup.
- Esperar a implementação integral de bus/DMA/timing deixaria sem especificação a primeira escrita requerida pelo caminho AOT.

## Consequências

O contrato inicia GBA-030 e mantém strict AOT: armazenamento não coberto produz fault explícito. RAM zero é estado determinístico da fixture, não alegação de power-on real. Bus, mirrors, timing, eventos e periféricos exigem incrementos independentes.

## Critérios para revisitar

Estender o bus somente por novas especificações com faixa, alinhamento, efeitos observáveis e testes de falha atômica; nunca mapear guest inteiro diretamente para host.

## Referências

[SPEC-016](../specs/SPEC-016-ram-word-stores.md), [SPEC-004](../specs/SPEC-004-ir-llvm.md), [SPEC-005](../specs/SPEC-005-runtime-timing.md), [SRC-14/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
