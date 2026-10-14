# SPEC-040 — Thumb LDR word imediato Format 9 em AOT estrito

Estado do documento: Accepted. Implementação e evidência: [ROADMAP](../ROADMAP.md).

## Contexto, objetivo e limite

Após SPEC-038/039, a compilação estática O2 da ROM LeafGreen fixada em KNO-004 alcançou `0x080006BC`, raw `0x6880`, e parou com `unsupported_thumb_instruction`. Os campos inferem Thumb Format 9 `LDR r0,[r0,#8]`. A base `0x040000D4` e o EA `0x040000DC` são inferidos pela propagação estática de LDR literal em `0x080006B2`; no mGBA fixado, o offset `0xDC` é nomeado `DMA3CNT_LO`. Nenhum desses valores veio de execução guest.

Esta tranche adiciona LDR word imediato Thumb T1 strict-AOT e leitura dinâmica somente de EWRAM/IWRAM físicas. Ela permite emitir o opcode; não modela MMIO e portanto o acesso inferido da LeafGreen deve continuar falhando explicitamente em runtime. O avanço de compile não equivale a compatibilidade ou execução do jogo.

## Fora de escopo

- LDR word por registrador (`0x5xxx`), SP-relative (`0x9xxx`), ROM dinâmica, BIOS, mirrors, MMIO, DMA, palette RAM, VRAM, OAM, backup ou bus geral.
- Alteração da ABI/layout do runtime, do GBA IR v2 ou do manifesto.
- Interpretação, JIT ou fallback em AOT estrito.

## Invariantes

- Format 9 LDR: `(encoding & 0xF800) == 0x6800`; `Rd=bits[2:0]`, `Rb=bits[5:3]`, `imm5=bits[10:6]`, todos r0–r7. `0x6800` corresponde a `LDR r0,[r0,#0]` e pertence ao formato aceito.
- `EA = old_Rb.wrapping_add(imm5 * 4)` em u32. O endereço guest continua inteiro até o helper.
- O valor carregado é a word little-endian iniciada em `aligned_EA = EA & !3`, rotacionada à direita por `8 * (EA & 3)`. O helper valida a word alinhada nas faixas físicas `[0x02000000,0x02040000)` e `[0x03000000,0x03008000)`, mas informa o EA original em fault. Assim, um EA não alinhado dentro de uma RAM física segue a regra de rotação e não gera fault de alinhamento.
- `Rd==Rb` calcula EA do valor antigo de Rb. A instrução não faz writeback nem altera CPSR; no sucesso, só Rd, PC+2 e o ciclo abstrato +1 mudam.
- Nenhum efeito guest é commitado se o helper falhar. O valor temporário de saída do host só é escrito depois de leitura válida.

## Requisitos normativos

- TLW-01: decoder e verificador MUST aceitar somente o formato `0x6800` com campos consistentes com o encoding e registradores baixos. Famílias vizinhas não cobertas MUST continuar rejeitadas.
- TLW-02: `ThumbOperation::LoadWordImmediate { rd, rb, imm5 }` MUST usar wrapping u32 para `old_Rb + 4*imm5`, preservar o valor antigo de base quando `Rd==Rb` e não alterar flags ou fazer writeback.
- TLW-03: o helper aditivo `gba_load32_v3(rt,out,guest_ea,host_value_out)` MUST receber EA como u32 guest e uma saída temporária host `uint32_t*`; esse ponteiro nunca representa memória guest. Ele MUST ler somente uma word inteiramente contida em EWRAM/IWRAM físicas, usando `aligned_EA=EA&!3` e rotacionando a word lida por `8*(EA&3)`. A validação usa o intervalo alinhado; faults preservam e reportam o EA original.
- TLW-04: se `out==NULL` ou `host_value_out==NULL`, o helper MUST retornar fault sem escrever em qualquer dos ponteiros. Com ponteiros válidos, runtime nulo, ABI inválida, memória nula ou região inválida MUST retornar os códigos de fault existentes. Toda falha MUST deixar o temporário, RAM e estado CPU intactos; com `rt!=NULL`, o registro de fault informa o PC guest atual, e com `out!=NULL` informa o EA guest original. Para `rt==NULL`, `next_pc` é zero.
- TLW-05: o lowering LLVM MUST capturar Rb antes da chamada, calcular EA em i32 guest, chamar `gba_load32_v3` e só após sucesso gravar o valor em Rd, PC+2 e ciclo+1. Falha encerra com o estado guest inalterado. O guard de overflow de ciclo existente permanece antes de qualquer efeito.
- TLW-06: discovery MUST marcar Rd como desconhecido e preservar outros registradores e a sombra de memória. Não deve dobrar leituras dinâmicas para ROM/MMIO ou inferir conteúdo de RAM mutável.
- TLW-07: interpretação single-step sem contexto de memória MUST retornar `MemoryRuntimeRequired`. O manifesto permanece `strict-aot`. ABI v3/v4 e GBA IR v2/layouts MUST permanecer inalterados; nenhuma região não suportada pode ser ignorada ou cair em fallback.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x6880` | Decodifica `LDR r0,[r0,#8]` |
| `0x6800`, offsets 0 e 31 | Decodifica; EA usa escala de 4 e offsets 0/124 |
| `Rd==Rb` | Endereço usa base pré-instrução; Rd recebe dado lido |
| EA alinhado e `EA&3` igual a 1, 2 ou 3 | Word em `EA&!3` rotacionada à direita por 8/16/24 bits |
| Primeira e última word de EWRAM/IWRAM | Carga válida; endereço fora das regiões faulta |
| EA no último byte de cada RAM | Word alinhada final é lida e rotacionada; nenhuma leitura cruza a alocação |
| Wrap u32 | EA modular é mantido; região inválida gera fault com o EA resultante |
| ROM, MMIO (incluindo `0x040000DC`), mirror ou fora de RAM | Fault explícito com PC/EA e sem mutação de Rd, temporário ou RAM |
| Fixture de MMIO `0x040000DC`, O0/O2 | Execução da instrução emitida faulta; `Rd`, PC, flags e ciclo permanecem nos valores anteriores |
| Runtime, ABI, memória ou saída inválidos | Fault explícito, temporário sentinela preservado e sem mutação guest |
| O0/O2 strict-AOT | Fixtures EWRAM/IWRAM produzem mesmo estado; PC/ciclo avançam apenas após sucesso |
| `0x5800` e `0x9800` | LDR por registrador e LDR SP-relative continuam rejeitados sem pacote parcial |

As fixtures são sintéticas. A leitura inferida em `0x040000DC` permanece fora do subset e deve faultar; a compilação estática da ROM depois de adicionar esta instrução servirá apenas para identificar a próxima fronteira.

## Fontes e referências

- [KNO-001 / SRC-37 e SRC-38](../knowledge/KNO-001-evidence.md) fixa observações de fonte para STR/LDR word, seus formatos e rotação de loads desalinhados.
- [KNO-004](../knowledge/KNO-004-rom-intake.md) registra a fronteira estática LeafGreen, hashes, comandos e limites.
- [SPEC-016](SPEC-016-ram-word-stores.md), [SPEC-029](SPEC-029-thumb-ldrb-immediate.md), [SPEC-034](SPEC-034-abstract-memory-discovery.md), [SPEC-038](SPEC-038-discovery-state-budget.md) e [SPEC-039](SPEC-039-thumb-str-word-immediate.md).
- [ADR-027](../adr/ADR-027-thumb-ldr-word-immediate.md) registra a decisão do helper de leitura word aditivo.
