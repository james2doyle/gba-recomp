# SPEC-028 — STRB Thumb imediato Format 9 em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Contexto e limites

Após SPEC-027, a descoberta estática da ROM fixada em KNO-004 parou em `0x08000998`, raw `0x7005`. Os campos inferem Thumb Format 9 `STRB r5,[r0,#0]`; SRC-29 registra a leitura dos decoders e caminhos de memória das referências. Não há valores de registrador ou endereço efetivo observados em execução.

Esta tranche aceita somente Thumb `STRB` imediato para EWRAM/IWRAM físicas. Não cria um barramento GBA geral e não aceita mirrors, ROM, BIOS, MMIO, palette RAM, VRAM, OAM ou backup.

## Requisitos

- TSB-01: o decoder MUST aceitar somente `(encoding & 0xF800) == 0x7000`. Deve extrair `imm5=bits[10:6]`, `Rb=bits[5:3]` e `Rd=bits[2:0]`, todos em r0–r7. O offset byte é `imm5` (0–31), sem escala. `STRB` com offset por registrador e os demais formatos permanecem rejeitados; LDRB imediato é tratado separadamente pela [SPEC-029](SPEC-029-thumb-ldrb-immediate.md).
- TSB-02: o endereço efetivo guest MUST ser `EA = wrapping_add(Rb, imm5)` em aritmética u32. Não há alinhamento ou normalização; o endereço do byte acessado é exatamente EA.
- TSB-03: o acesso MUST gravar `Rd & 0xFF` em exatamente um byte. Não deve alterar bytes adjacentes, `Rb`/`Rd`, flags ou executar writeback. Se `Rd==Rb`, endereço e valor vêm do registrador guest anterior à instrução.
- TSB-04: `gba_store8_v3` MUST receber o contexto guest versionado e endereço guest como inteiros. Deve validar ABI/contexto e aceitar somente bytes nas faixas físicas `[0x02000000,0x02040000)` ou `[0x03000000,0x03008000)`. O primeiro e o último byte de cada faixa são válidos; `0x02040000` e `0x03008000` não são. Nunca converter endereço guest em ponteiro host.
- TSB-05: uma falha MUST retornar `GBA_EXIT_FAULT_V2`; com `out != NULL` e `rt != NULL`, `GbaExitV3.next_pc` informa `rt->cpu.r[15]`, que no caminho AOT é o PC da instrução faultante. Para `rt == NULL`, `next_pc` é zero como sentinela. Em todos os faults com saída, `fault_address` contém o EA original. Usar `GBA_MEMORY_FAULT_RUNTIME_V3`, `GBA_MEMORY_FAULT_ABI_V3`, `GBA_MEMORY_FAULT_NULL_MEMORY_V3` e `GBA_MEMORY_FAULT_REGION_V3` para runtime nulo, ABI/contexto inválido, memória nula e EA fora das RAMs, respectivamente. Se `out == NULL`, retornar fault sem relatório ou escrita. Falhas não alteram RAM, registradores, PC ou ciclo.
- TSB-06: após store bem-sucedido, PC Thumb avança dois bytes e `guest_cycle` soma uma unidade abstrata; CPSR e demais registradores permanecem iguais. Isto não declara ciclos físicos, waitstates ou sincronização com eventos.
- TSB-07: LLVM MUST chamar `gba_store8_v3` no runtime GBA e transmitir o EA como inteiro guest de 32 bits. O helper é extensão aditiva do ABI v3; layouts e versões de `GbaRuntimeV3`, `GbaRuntimeV4`, `GbaMemoryV1` e manifestos não mudam. O runtime v4 segue validado na entrada do bloco.
- TSB-08: a descoberta MUST manter o fallthrough Thumb em `PC+2`, sem alterar registradores abstratos ou inferir efeitos de código escrito em memória. Execução é strict-AOT, sem interpretação, JIT ou fallback.
- TSB-09: faults fora das duas faixas físicas são explícitos. Mirrors, outras regiões, leitura, RMW, DMA, invalidação de código, timing de bus e demais loads/stores continuam fora deste contrato.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x7005` | Decodifica `STRB r5,[r0,#0]` |
| Offset imediato 0 e 31 | Endereços base e base+31, sem escala nem writeback |
| `Rd=0xAABBCCDD` | Grava somente `0xDD`; bytes vizinhos preservados |
| EWRAM/IWRAM | Bytes observáveis em ambas as regiões |
| EA par/ímpar | Ambos gravam exatamente no EA, sem alinhamento |
| Último byte de cada RAM | `0x0203FFFF` e `0x03007FFF` válidos; próximo byte faulta |
| Alias `Rd==Rb` | EA e byte gravado derivam do valor pré-instrução |
| Wraparound u32 | Adição do offset imediato tem wrapping definido |
| Mirror, ROM, MMIO e fora de faixa | Fault com EA original e estado/memória sem mutação |
| Runtime/ABI/memória inválidos | Códigos existentes de fault e falha atômica |
| `out==NULL` | Retorna fault sem escrita/relatório |
| Estado CPU | Só PC+2 e ciclo abstrato +1 mudam |
| O0/O2 | Estado guest igual, manifesto strict-AOT |
| STRB por registrador | Falha antes de publicar pacote parcial |

## Fixtures e verificação

O teste C de `gba_store8_v3` MUST cobrir as duas RAMs, truncamento para byte, sentinelas adjacentes, bytes inicial/final, endereço além das faixas, mirror/ROM/MMIO, runtime/ABI/memória/output inválidos, PC/EA e falha atômica. Fixtures strict-AOT O0/O2 MUST cobrir EWRAM/IWRAM, offsets 0/31, EA par/ímpar, alias `Rd==Rb`, wrapping u32, PC/ciclo/registradores/flags e manifesto. Casos negativos MUST incluir store por registrador e demais formatos fora desta spec, sem pacote parcial. A aceitação de LDRB imediato tem critérios próprios na SPEC-029.

Após os gates, repetir a compilação estática de LeafGreen e registrar somente o limite observado. A nova descoberta não prova valores de registradores, destino da escrita ou execução dinâmica do título.

## Evidência e referências

- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa identidade da ROM e fronteiras observadas.
- [KNO-001 / SRC-29](../knowledge/KNO-001-evidence.md) registra as observações de fonte nos commits fixados. Elas corroboram formato e operação de byte; não validam código local nem hardware real.
- [SPEC-016](SPEC-016-ram-word-stores.md), [SPEC-027](SPEC-027-thumb-strh-immediate.md) e [ADR-009](../adr/ADR-009-versioned-bus-context.md) limitam memória a RAM física e mantêm endereços guest separados de ponteiros host.
- [ADR-018](../adr/ADR-018-additive-byte-store-abi.md) registra o símbolo aditivo sem mudança de layout ou versão.
