# SPEC-029 — LDRB Thumb imediato Format 9 em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Contexto e limites

Após SPEC-028, a descoberta estática da ROM fixada em KNO-004 parou em `0x0800099C`, raw `0x7808`. Os campos inferem Thumb Format 9 `LDRB r0,[r1,#0]`; SRC-30 registra as observações das fontes fixadas. Não há valores de registrador, endereço efetivo ou byte lido observados em execução.

Esta tranche aceita somente Thumb `LDRB` imediato para EWRAM/IWRAM físicas. O resultado é o byte zero-extendido a 32 bits. Não cria um barramento GBA geral e não aceita mirrors, ROM, BIOS, MMIO, palette RAM, VRAM, OAM ou backup.

## Requisitos

- TLB-01: o decoder MUST aceitar somente `(encoding & 0xF800) == 0x7800`. Deve extrair `imm5=bits[10:6]`, `Rb=bits[5:3]` e `Rd=bits[2:0]`, todos em r0–r7. O offset byte é `imm5` (0–31), sem escala. `STRB`, `LDRB` por registrador, halfword loads e os demais formatos permanecem fora deste contrato.
- TLB-02: o endereço efetivo guest MUST ser `EA = wrapping_add(Rb, imm5)` em aritmética u32. Não há alinhamento ou normalização; o byte lido está exatamente em EA.
- TLB-03: uma leitura bem-sucedida MUST obter um byte e zero-extendê-lo para `Rd`. Não deve alterar flags, CPSR ou executar writeback. Se `Rd==Rb`, EA vem do valor de Rb anterior à instrução.
- TLB-04: `gba_load8_v3(rt,out,guest_ea,host_value_out)` MUST receber o contexto guest versionado, EA guest como inteiro e um ponteiro host para um temporário de saída de um byte. O ponteiro de saída não representa memória guest. Deve validar ABI/contexto, memória e saída antes de ler; deve aceitar somente bytes nas faixas físicas `[0x02000000,0x02040000)` ou `[0x03000000,0x03008000)`. Os limites iniciais e finais são válidos; `0x02040000` e `0x03008000` não são. Nunca converter endereço guest em ponteiro host.
- TLB-05: o helper MUST escrever o temporário somente após todas as validações e leitura bem-sucedida. Falha MUST retornar `GBA_EXIT_FAULT_V2`; com `out != NULL` e `rt != NULL`, `GbaExitV3.next_pc` informa `rt->cpu.r[15]`, que no caminho AOT é o PC da instrução faultante. Para `rt == NULL`, `next_pc` é zero como sentinela. Em faults reportados, `fault_address` contém o EA original. Usar `GBA_MEMORY_FAULT_RUNTIME_V3`, `GBA_MEMORY_FAULT_ABI_V3`, `GBA_MEMORY_FAULT_NULL_MEMORY_V3` e `GBA_MEMORY_FAULT_REGION_V3` para runtime nulo, ABI/contexto inválido, memória nula e EA fora das RAMs. Se `out==NULL` ou `host_value_out==NULL`, retorna fault sem modificar `out` nem qualquer outro valor. Falhas reportadas não alteram temporário, RAM, registradores, PC ou ciclo.
- TLB-06: após load bem-sucedido, o LLVM grava o valor zero-extendido em `Rd`, avança PC Thumb em dois bytes e soma uma unidade abstrata a `guest_cycle`. CPSR e os demais registradores permanecem iguais. Isto não declara ciclos físicos, waitstates ou sincronização com eventos.
- TLB-07: LLVM MUST chamar `gba_load8_v3` no runtime GBA, transmitindo EA como inteiro guest de 32 bits e resultado por armazenamento temporário host de um byte. O helper é extensão aditiva do ABI v3; layouts e versões de `GbaRuntimeV3`, `GbaRuntimeV4`, `GbaMemoryV1` e manifestos não mudam. Runtime v4 segue validado na entrada do bloco.
- TLB-08: a descoberta MUST manter o fallthrough Thumb em `PC+2`, marcar `Rd` como desconhecido e preservar os demais registradores abstratos. Não deve dobrar a leitura para constante nem inferir efeitos de código escrito em memória. Execução é strict-AOT, sem interpretação, JIT ou fallback.
- TLB-09: faults fora das duas faixas físicas são explícitos. Mirrors, outras regiões, stores, RMW, DMA, invalidação de código, timing de bus e demais loads/stores continuam fora deste contrato.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x7808` | Decodifica `LDRB r0,[r1,#0]` |
| Offset imediato 0 e 31 | EA base e base+31, sem escala ou writeback |
| Bytes `0x00`, `0x7F`, `0x80`, `0xFF` | Valores `0x00000000`, `0x0000007F`, `0x00000080`, `0x000000FF` em Rd |
| EWRAM/IWRAM físicas | Carrega o byte correto de ambas as regiões |
| EA par/ímpar | Lê exatamente EA, sem alinhamento |
| Primeiro/último byte de cada RAM | Válidos; endereço exclusivo seguinte produz fault |
| Alias `Rd==Rb` | EA deriva do valor pré-instrução; Rd recebe o byte lido |
| Wraparound u32 | Adição do offset imediato tem wrapping definido; fault registra EA resultante |
| Mirror, ROM, MMIO e fora de faixa | Fault com EA original, estado inalterado e temporário sentinela preservado |
| Runtime/ABI/memória inválidos | Códigos existentes e falha atômica |
| `out==NULL` ou saída host nula | Retorna fault; deixa `out` e todos os outros valores intactos |
| Estado CPU | Só Rd, PC+2 e ciclo abstrato +1 mudam; CPSR e outros registradores preservados |
| O0/O2 | Mesmo estado guest, manifesto strict-AOT |
| LDRB por registrador e formatos vizinhos | Falham antes de publicar pacote parcial |

## Fixtures e verificação

O teste C de `gba_load8_v3` MUST cobrir EWRAM/IWRAM, zero-extension, bytes inicial/final, endereço além das faixas, mirror/ROM/MMIO, runtime/ABI/memória/saída inválidos, PC/EA, temporário sentinela e ausência de mutação em falha. Fixtures strict-AOT O0/O2 MUST cobrir EWRAM/IWRAM, offsets 0/31, EA par/ímpar, alias `Rd==Rb`, wrapping u32, PC/ciclo/registradores/flags e manifesto. Casos negativos MUST incluir LDRB por registrador e famílias adjacentes, sem pacote parcial.

Após os gates, repetir a compilação estática de LeafGreen e registrar somente o limite realmente observado. A nova descoberta não prova valores de registradores, byte lido, alcance dinâmico ou execução do título.

## Evidência e referências

- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa identidade da ROM e fronteiras observadas.
- [KNO-001 / SRC-30](../knowledge/KNO-001-evidence.md) registra as observações de fonte nos commits fixados. Elas corroboram formato e load sem sinal; não validam código local nem hardware real.
- [SPEC-028](SPEC-028-thumb-strb-immediate.md), [SPEC-016](SPEC-016-ram-word-stores.md) e [ADR-009](../adr/ADR-009-versioned-bus-context.md) limitam a memória a RAM física e mantêm endereços guest separados de ponteiros host.
- [ADR-019](../adr/ADR-019-additive-byte-load-abi.md) registra o símbolo aditivo e o temporário host sem alterar layout ou versão ABI.
