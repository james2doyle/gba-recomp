# SPEC-044 — Thumb STRH para IME em AOT estrito

Estado do documento: Draft. Implementação: não iniciada. Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

A tentativa estática O2 da LeafGreen fixada em [KNO-004](../knowledge/KNO-004-rom-intake.md) encontra `STRH` com endereço guest inferido `0x04000208` e valor abstrato zero em `0x08000B58`. `0x04000208` é o registrador IME no mapa I/O. A instrução ocorre entre um `PUSH` rastreado e `POP {r0}`; a política atual da sombra abstrata limpa todas as palavras porque o destino não é RAM física, deixando o `BX r0` em `0x08000B6A` sem destino provado. Endereço, valor e sequência são resultados da análise estática local; não são observações de execução do jogo.

Adicionar suporte estritamente limitado a uma escrita halfword no IME por Thumb STRH imediato. A escrita zero atualiza o latch IME versionado já presente em `GbaMachineStateV1`. O analisador pode preservar fatos de stack somente quando cada alternativa tem endereço exatamente `0x04000208` e valor exatamente zero. A ABI v3 e o helper RAM v3 permanecem congelados; a operação usa helper v4 aditivo.

## Fora de escopo

- Leitura de MMIO, byte/word access em I/O, qualquer registrador I/O além de IME e mirrors de I/O.
- Entrega de IRQ, scheduler, DMA, timers, PPU/APU, sincronização de eventos ou ciclos físicos.
- IME habilitado (`bit 0 = 1`), que requer integração com o controlador de interrupções.
- Preservar a sombra em outros stores fora de EWRAM/IWRAM, mesmo quando o endereço parece distante da stack.
- Alegar que a ROM compila, executa, é compatível, ou reproduz comportamento de hardware por ter passado este slice.

## Requisitos normativos

- IME-01: `gba_store16_v4(GbaRuntimeV4*, GbaExitV3*, uint32_t guest_ea, uint32_t value)` MUST validar runtime, CPU, ABI, máquina, versão do estado e a identidade `base.memory == &machine.memory` antes de qualquer efeito.
- IME-02: para EA halfword-alinhado que pertença a EWRAM/IWRAM físicas, o helper v4 MUST delegar a `gba_store16_v3` e conservar seus faults e efeitos. Nenhum endereço guest pode virar ponteiro host sem validação de região.
- IME-03: para `guest_ea & ~1 == 0x04000208`, o helper MUST aceitar somente valores cujo bit 0 seja zero. Em sucesso, deve gravar zero nos dois bytes do latch `machine.io[0x208..0x20A]`. Valor com bit 0 ativo MUST retornar fault `GBA_MEMORY_FAULT_IO_EFFECT_V4` (8), sem mutação de memória, latch ou CPU.
- IME-04: qualquer outro destino fora das RAMs físicas MUST retornar fault explícito de região e preservar todo o estado guest. O fault deve registrar o EA original, inclusive se ímpar.
- IME-05: a escrita STRH em `0x04000208` com `Rd` singleton zero MUST ser representada no lowering AOT por `gba_store16_v4`. A operação conserva snapshot de base/valor, flags e writeback; em sucesso avança PC/ciclo conforme o contrato STRH vigente. O helper v3 continua disponível para consumidores existentes.
- IME-06: discovery MUST preservar a sombra de memória SPEC-034 somente para um store STRH cuja abstração do EA seja `{0x04000208}` e cujo valor escrito seja `{0}`. Outros valores, outros endereços fora da RAM suportada, alternativas múltiplas que não satisfaçam o predicado ou endereço/valor Unknown MUST limpar a sombra conforme a política anterior.
- IME-07: essa preservação representa somente os efeitos explícitos modelados pelo runtime deste slice. Não modela DMA já ativo, IRQ entre instruções nem eventos assíncronos; não pode ser usada como alegação de precisão de hardware nem como razão para preservar fatos após outras escritas I/O.
- IME-08: novas falhas de acesso não podem cair em interpretação/JIT nem deixar pacote parcial. O manifesto permanece strict-AOT e não altera ABI v4, layout de estado, versão de HLE ou GBA IR.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| STRH para IME com valor zero | Latch vira zero, estado CPU avança, sem fault |
| STRH para IME com bit 0 ativo | Fault code 8, latch/CPU/RAM intactos |
| Runtime, máquina ou versão inválida | Fault antes de qualquer mutação |
| STRH em EWRAM/IWRAM via helper v4 | Mesmo resultado/fault do helper v3 |
| Outro destino I/O ou região não suportada | Fault de região, sem mutação |
| Discovery de STRH `{IME},{0}` entre PUSH e POP | Fatos de stack ficam disponíveis e o valor de POP é propagado |
| Discovery para IME com valor desconhecido/não zero | Sombra limpa; BX sem destino continua rejeitado |
| Fixture strict-AOT O0/O2 | Estado e decisão de fault idênticos; manifesto identifica strict-AOT |
| LeafGreen com SHA-256 fixado | Registrar somente a próxima fronteira estática ou pacote realmente criado; não inferir execução |

## Fontes e referências

- [SPEC-027 — Thumb STRH imediato](SPEC-027-thumb-strh-immediate.md).
- [SPEC-034 — memória abstrata de discovery](SPEC-034-abstract-memory-discovery.md).
- [SPEC-023 — estado de máquina ABI v4](SPEC-023-thumb-swi-register-ram-reset.md).
- [KNO-001 / SRC-41](../knowledge/KNO-001-evidence.md).
- [KNO-004 — tentativa LeafGreen](../knowledge/KNO-004-rom-intake.md).
