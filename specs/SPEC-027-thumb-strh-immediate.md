# SPEC-027 — STRH Thumb imediato Format 10 em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Contexto e limites

Após SPEC-026, a descoberta estática da ROM fixada em KNO-004 parou em `0x080003B8`, raw `0x8008`. Os campos inferem Thumb Format 10 `STRH r0,[r1,#0]`; SRC-28 registra as observações de fonte. A classe decorre do encoding; não há valor de `r1` observado por execução.

Esta tranche aceita somente Thumb `STRH` com offset imediato para EWRAM/IWRAM físicas. Não cria um barramento GBA geral e não aceita mirrors, ROM, BIOS, MMIO, palette RAM, VRAM, OAM ou backup.

## Requisitos

- TSH-01: o decoder MUST aceitar somente `(encoding & 0xF800) == 0x8000`. Deve extrair `imm5=bits[10:6]`, `Rb=bits[5:3]` e `Rd=bits[2:0]`, todos em r0–r7. O offset é `imm5*2` (0–62 bytes). `LDRH`, `STRH` com offset por registrador e os demais formatos permanecem rejeitados.
- TSH-02: o endereço efetivo guest MUST ser `EA = wrapping_add(Rb, imm5*2)` em aritmética u32. O endereço halfword normalizado é `aligned_ea = EA & !1`; portanto um EA ímpar alinha para baixo, sem fault de alinhamento e sem rotação do valor. Essa regra vale somente para o novo helper halfword e não muda o alinhamento de quatro bytes exigido por `gba_store32_v3`.
- TSH-03: o acesso MUST gravar os 16 bits baixos de `Rd` em little-endian nos dois bytes iniciados em `aligned_ea`. Não deve escrever bytes adjacentes, alterar `Rb`/`Rd`, atualizar flags, nem executar writeback.
- TSH-04: a função `gba_store16_v3` MUST receber o contexto guest versionado e endereço guest como inteiros. Ela deve validar ABI/contexto e, depois da normalização, aceitar somente halfwords completos dentro das faixas físicas `[0x02000000,0x02040000)` ou `[0x03000000,0x03008000)`. `0x0203FFFF` alinha para `0x0203FFFE` e `0x03007FFF` alinha para `0x03007FFE`; ambos são válidos. `0x02040000` e `0x03008000` continuam fora das respectivas regiões. Nunca converter endereço guest em ponteiro host.
- TSH-05: uma falha MUST retornar `GBA_EXIT_FAULT_V2`; quando `out != NULL` e `rt != NULL`, `GbaExitV3.next_pc` MUST informar `rt->cpu.r[15]`, que no caminho AOT é o PC da instrução guest que faultou. Para `rt == NULL`, `next_pc` MUST ser zero como sentinela porque a assinatura não recebe PC separado. Em todos os faults com saída, `fault_address` MUST conter o EA guest original, antes da normalização. Os códigos são `GBA_MEMORY_FAULT_RUNTIME_V3` para `rt == NULL`, `GBA_MEMORY_FAULT_ABI_V3` para versão ABI v3 ou CPU v2 inválida, `GBA_MEMORY_FAULT_NULL_MEMORY_V3` para memória nula e `GBA_MEMORY_FAULT_REGION_V3` quando o halfword normalizado está fora das duas RAMs físicas. Se `out == NULL`, o helper apenas retorna `GBA_EXIT_FAULT_V2`, sem escrita e sem relatório em `GbaExitV3`. Falhas não alteram RAM, registradores, PC ou ciclo. A validação de ambos os bytes ocorre antes de qualquer escrita parcial. Endereço ímpar não é, por si só, falha.
- TSH-06: após store bem-sucedido, PC Thumb avança dois bytes e `guest_cycle` soma uma unidade abstrata; CPSR e demais registradores permanecem iguais. Isto não declara ciclos físicos, waitstates ou sincronização com eventos.
- TSH-07: o LLVM MUST emitir chamada ao helper de store halfword no runtime GBA. O helper é uma extensão aditiva do ABI v3 existente; layouts e versões de `GbaRuntimeV3`, `GbaRuntimeV4`, `GbaMemoryV1` e manifestos não mudam. O runtime v4 continua validado na entrada do bloco.
- TSH-08: a descoberta MUST manter o fallthrough Thumb em `PC+2` sem alterar registradores abstratos ou fingir que a escrita dinâmica modificou ROM/código. Execução é strict-AOT; sem interpretação, JIT ou fallback.
- TSH-09: erros de runtime fora das duas faixas físicas são faults explícitos. Mirrors, outras regiões, leituras, RMW, DMA, invalidação de código, timing de bus e demais loads/stores continuam fora deste contrato.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x8008` | Decodifica `STRH r0,[r1,#0]` |
| Offset imediato 0 e 31 | Endereços base e base+62, sem writeback |
| `Rd=0xAABBCCDD` | Grava bytes `DD CC`; bytes vizinhos preservados |
| EWRAM e IWRAM físicas | Store observável em ambas as regiões |
| EA par e ímpar | EA ímpar grava em `EA & !1`, sem rotação nem fault de alinhamento |
| Último byte ímpar de EWRAM/IWRAM | `0x0203FFFF→0x0203FFFE` e `0x03007FFF→0x03007FFE`; grava atomicamente |
| Alias `Rd==Rb` | EA e valor armazenado vêm do mesmo valor pré-instrução; não há writeback |
| Fora de região, mirror, ROM ou MMIO | Fault com EA original; estado e memória sem mutação |
| Runtime/ABI/memória inválidos | Respectivamente `RUNTIME` (PC zero sentinela), `ABI`, `NULL_MEMORY`; fault atômico |
| `out==NULL` | Retorna fault; sem escrita e sem relatório de saída |
| Estado CPU | Só PC+2 e ciclo abstrato +1 mudam; CPSR, base e demais registradores preservados |
| O0/O2 | Mesmo estado guest e bytes de RAM, manifesto strict-AOT |
| `LDRH`, offset por registrador e outras famílias | Falham antes de publicar pacote parcial |

## Fixtures e verificação

O teste C de `gba_store16_v3` MUST cobrir EWRAM/IWRAM, truncamento, little-endian, sentinelas adjacentes, EA ímpar, último halfword válido nas duas RAMs, limites, regiões não cobertas, cada código de fault, `out==NULL` e falha atômica reportando o EA original e PC da instrução. Fixtures Thumb strict-AOT O0/O2 MUST conferir stores em ambas as regiões, offsets 0/62, alinhamento par/ímpar, alias `Rd==Rb`, PC, ciclo, registradores e CPSR. Casos negativos MUST incluir `LDRH` e store por registrador, sem pacote parcial. O gate deve validar ABI v4/strict-AOT nos manifestos e executar os pacotes.

Após os gates, repetir a compilação estática de LeafGreen e registrar somente o limite realmente observado. Avançar a descoberta não prova que o valor de `r1` do jogo pertença às duas regiões aceitas, nem que LeafGreen execute esse store em runtime.

## Evidência e referências

- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa a ROM, hash, PC e halfword observado.
- [KNO-001 / SRC-28](../knowledge/KNO-001-evidence.md) registra mGBA e gba-recomp nos commits fixados. As fontes corroboram a escala, alinhamento descendente e store de 16 bits; não validam a implementação local nem hardware real.
- [SPEC-016](SPEC-016-ram-word-stores.md) e [ADR-009](../adr/ADR-009-versioned-bus-context.md) fixam o limite de RAM física e a separação entre endereço guest e ponteiro host.
- [ADR-017](../adr/ADR-017-additive-halfword-store-abi.md) registra o helper aditivo sem alteração de layout.
