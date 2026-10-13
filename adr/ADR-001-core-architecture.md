# ADR-001 — LLVM direto e runtime de hardware independente

- Estado: Proposed
- Data: 2026-10-13

## Contexto

A referência local emite C e carrega biblioteca nativa. O pedido requer pipeline LLVM portátil. O GBA não tem uma camada de SO que absorva hardware: CPU recompilada continua acessando MMIO e memória com efeitos temporais.

## Decisão proposta

Usar Rust, decoder ARMv4T próprio, GBA IR com efeitos explícitos, lowering direto para LLVM, ABI C e runtime próprio com adaptadores host. Usar dispatcher de blocos como baseline; encadeamento e chamadas diretas são otimizações posteriores. PPU por software é referência inicial.

## Alternativas

C gerado reaproveitaria a estratégia vendor, mas adicionaria uma linguagem intermediária ao contrato desejado. Traduzir ARM diretamente para LLVM sem GBA IR reduziria componentes, mas dificultaria auditoria de timing, flags e efeitos. Embutir mGBA inteiro aceleraria compatibilidade, porém acoplaria scheduler/estado e política de licenças; o projeto começa usando-o como referência e futuro oráculo externo.

## Consequências

Há custo real de implementar hardware e validar semântica; LLVM não resolve isso. Otimizações precisam preservar eventos guest. A API C reduz acoplamento, mas exige versionamento e cuidado com layout por destino.

## Revisão

Revisitar após slice end-to-end, medindo custo de helpers, tempo de compilação, tamanho do executável e fidelidade. Reutilização de módulos vendor exige decisão explícita com proveniência.

Referências: [KNO-001](../knowledge/KNO-001-evidence.md), [SPEC-004](../specs/SPEC-004-ir-llvm.md).
