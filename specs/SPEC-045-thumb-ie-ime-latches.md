# SPEC-045 — Loads e stores Thumb halfword para latches IE/IF/IME

Estado do documento: Draft. Implementação pendente. Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-044, a análise estática da LeafGreen identifica em `0x08000B54` uma leitura de IME, seguida por stores para IME em `0x08000B58` e `0x08000B62`, além de um store para IE em `0x08000B60`. Os endereços são inferidos de encodings e literais da ROM cujo SHA-256 está fixado em [KNO-004](../knowledge/KNO-004-rom-intake.md); não são observações de execução. A leitura de IME devolve o valor anterior ao clear, portanto o restore posterior pode conter zero ou um.

O pacote atual inicializa `GbaMachineStateV1` inteira com zero ([runner em `main.rs`](../crates/gba-recomp/src/main.rs)). O runtime ainda não gera eventos de vídeo, timer, DMA ou outras fontes de IRQ; nesse modelo fechado, IF permanece zero, e `IE & IF` não agenda IRQ no mGBA fixado. Isso permite modelar os latches IE/IF/IME sem afirmar entrega de IRQ ou compatibilidade com hardware. Quando fontes de eventos forem implementadas, o contrato precisa ser revisto antes que fatos de stack atravessem novas fronteiras assíncronas.

Adicionar leitura Thumb LDRH dos latches IE, IF e IME, e escrita Thumb STRH dos latches IE e IME, estritamente em AOT. Preservar os helpers existentes `gba_load16_v3` e `gba_store16_v4`; os novos efeitos usam helpers aditivos com estado ABI v4. O pacote declara `interrupt_model: "none"` e `io_initialization: "all-zero"`.

## Fora de escopo

- Entrega, agendamento, entrada, despacho ou retorno de IRQ.
- Fontes de IF, timers, DMA, PPU, APU, serial, input ou atualização assíncrona de qualquer latch.
- Escritas em IF, acesso byte/word a I/O, mirrors de I/O ou outros registradores.
- Ciclos ARM7TDMI, WAITCNT, stalls ou sincronização de eventos.
- Alegar compatibilidade de LeafGreen ou de hardware por compilar uma sequência sintética.

## Requisitos normativos

- IOM-01: `gba_load16_v4(GbaRuntimeV4*, GbaExitV3*, uint32_t guest_ea, uint32_t *host_value_out)` MUST validar todos os ponteiros, versões ABI, estado de máquina e identidade `base.memory == &machine.memory` antes de mutação observável. Loads em EWRAM/IWRAM MUST delegar a `gba_load16_v3` e conservar os efeitos/faults já definidos.
- IOM-02: para EA halfword-alinhado `0x04000200` (IE), `0x04000202` (IF) ou `0x04000208` (IME), `gba_load16_v4` MUST devolver o latch little-endian correspondente. EA ímpar MUST usar a mesma regra de alinhamento/rotação do helper LDRH físico vigente. Outros destinos fora da RAM física MUST faultar explicitamente sem alterar o temporário de saída.
- IOM-03: `gba_store16_v5(GbaRuntimeV4*, GbaExitV3*, uint32_t guest_ea, uint32_t value)` MUST delegar stores em EWRAM/IWRAM ao helper v3. Para IE ou IME, MUST validar IF==0 antes de qualquer efeito; caso contrário, MUST retornar `GBA_MEMORY_FAULT_IO_EFFECT_V4` (8), preservando RAM, latches e CPU. IE conserva os 16 bits baixos; IME conserva somente bit 0. Escrita em IF e qualquer outra escrita I/O MUST faultar sem mutação.
- IOM-04: qualquer fault de load/store MUST preservar o temporário de saída, RAM, latches, registradores, PC e ciclos; o relatório MUST manter o EA guest original, inclusive quando ímpar. Nenhum endereço guest pode ser convertido em ponteiro host antes da validação da região.
- IOM-05: lowering Thumb LDRH MUST usar `gba_load16_v4`; lowering Thumb STRH MUST usar `gba_store16_v5`. A operação conserva snapshots de base/valor, flags e writeback definidos pelas SPEC-027/042 e avança PC/ciclo somente após sucesso. Não há fallback para interpretação ou JIT.
- IOM-06: discovery MUST iniciar IE, IF e IME em zero conforme o runner do pacote. LDRH para esses endereços MUST propagar o valor abstrato do latch; stores IE/IME MUST atualizar o latch abstrato com a truncagem/máscara correspondente. SWI #1 MUST preservar ou zerar os fatos de IO conforme seus efeitos; uma chamada ou store não modelado MUST invalidar fatos que possam ter alterado os latches.
- IOM-07: discovery MAY preservar sombra de RAM através de STRH para IE/IME somente quando cada alternativa prova endereço exato e IF singleton zero. Se IF for desconhecido ou não zero, a sombra MUST ser invalidada. Isso vale apenas para o runner sem fontes de evento e não afirma que a sombra sobreviva a DMA ou IRQ de hardware.
- IOM-08: manifesto strict-AOT MUST declarar `interrupt_model: "none"` e `io_initialization: "all-zero"`. Código gerado MUST faultar explicitamente em acesso I/O não coberto; compilação continua sem interpretação/JIT e falhas não podem deixar pacote parcial.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| LDRH IE/IF/IME | Latch correto em `rd`; RAM, PC e ciclos sem efeito adicional |
| LDRH em RAM física | Mesmo resultado do helper v3 |
| IE store com IF zero | Latch IE recebe `value & 0xFFFF` |
| IME store com IF zero | Latch IME recebe `value & 1`, inclusive restore do valor lido antes |
| IE/IME store com IF não zero | Fault 8 atômico; nenhum estado muda |
| IF store ou outro MMIO | Fault explícito e atômico |
| EA ímpar | Alinhamento/rotação e EA de fault conformes ao contrato LDRH/STRH |
| Discovery IE/IME entre PUSH e POP com IF zero | Sombra preservada; stores não alteram memória RAM |
| Discovery com IF desconhecido | Sombra limpa antes de continuar |
| Fixture da sequência B48–B6A em O0/O2 | Executa strict-AOT até BX/retorno e observa IE/IF/IME; manifesta o perfil sem IRQ |
| LeafGreen com SHA-256 fixado | Registrar próxima fronteira estática e eventual pacote separadamente; isso não é execução do jogo |

## Fontes e referências

- [SPEC-027 — Thumb STRH imediato](SPEC-027-thumb-strh-immediate.md), [SPEC-042 — Thumb LDRH imediato](SPEC-042-thumb-ldrh-immediate.md) e [SPEC-044 — store IME zero](SPEC-044-thumb-ime-store.md).
- [SPEC-005 — timing e IRQ](SPEC-005-runtime-timing.md).
- [KNO-001 / SRC-41](../knowledge/KNO-001-evidence.md) e [KNO-004 — tentativa LeafGreen](../knowledge/KNO-004-rom-intake.md).
- mGBA fixado em `143adf5aa2a60c6036810b0cd72445e13d74b0e6`: `src/gba/io.c` (latches IE/IF/IME e IRQ test) e `src/gba/gba.c` (agendamento de IRQ somente quando `IE & IF != 0`).
