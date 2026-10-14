# SPEC-042 — Thumb LDRH imediato Format 10 em AOT estrito

Estado do documento: Accepted. Implementação e evidência: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-041, a descoberta estática da LeafGreen fixada em KNO-004 avançou até `0x08000B84`, halfword `0x8821`, e parou com `unsupported_thumb_instruction`. Os campos inferem Thumb Format 10 `LDRH r1,[r4,#0]`; a inferência não fornece o valor guest de r4 nem a região dinâmica lida.

Adicionar LDRH imediato Thumb T1 a strict-AOT. Leituras são aceitas somente em EWRAM/IWRAM físicas; outras regiões causam fault explícito. O helper novo mantém endereço guest como inteiro e não amplia bus, MMIO ou fallback.

## Fora de escopo

- STRH, já coberto pela SPEC-027; LDRH por registrador, LDRSH e LDRH PC/SP-relative.
- ROM dinâmica, BIOS, mirrors, MMIO, palette RAM, VRAM, OAM, backup e bus geral.
- Timing físico, interpretação/JIT, fallback ou declaração de suporte a LeafGreen.

## Invariantes

- O LDRH imediato pertence a Format 10 com `(encoding & 0xF800) == 0x8800`; `Rd=bits[2:0]`, `Rb=bits[5:3]`, `imm5=bits[10:6]`, todos r0–r7. `EA=old_Rb.wrapping_add(2*imm5)` em u32.
- A halfword lida começa em `aligned_EA=EA&!1`, é little-endian e deve caber integralmente na RAM física. O valor de 16 bits é zero-estendido para u32. Para `EA&1==1`, o valor u32 é rotacionado à direita por 8, conforme o comportamento observado nos dois cores fixados; para EA par, não há rotação.
- O helper valida ABI/runtime/memória/região e escreve `host_value_out` somente após leitura válida. Fault preserva o temporário e o estado guest e reporta o EA original, inclusive quando ímpar ou quando a soma modular resultou em zero.
- No sucesso, LDRH grava apenas Rd, preserva CPSR e demais registradores, avança Thumb PC em dois bytes e soma um ciclo abstrato. `Rd==Rb` usa o valor antigo do base.
- Discovery torna Rd Unknown e preserva os outros registradores e a sombra de memória. Single-step sem contexto de memória retorna `MemoryRuntimeRequired`.

## Requisitos normativos

- TLH-01: decoder e verificador MUST aceitar somente encoding Format 10 LDRH imediato (`0x8800` sob máscara `0xF800`), conferir os três campos e rejeitar famílias adjacentes ainda não cobertas.
- TLH-02: `ThumbOperation::LoadHalfwordImmediate { rd, rb, imm5 }` MUST calcular `old_Rb + 2*imm5` com wrap modular de 32 bits; não altera flags e não faz writeback.
- TLH-03: `gba_load16_v3(rt,out,guest_ea,host_value_out)` MUST receber EA guest u32 e saída host `uint32_t*`; ponteiro host nunca representa memória guest. Deve ler uma halfword little-endian em `EA&!1` somente se integralmente contida nas faixas físicas `[0x02000000,0x02040000)` ou `[0x03000000,0x03008000)`, aplicar a rotação de 32 bits especificada e informar o EA original em `fault_address`. Nunca converter EA guest em ponteiro host.
- TLH-04: se `out==NULL` ou `host_value_out==NULL`, o helper MUST retornar `GBA_EXIT_FAULT_V2` sem escrever em qualquer ponteiro válido; em particular, `host_value_out==NULL` MUST preservar `out` byte a byte. Falhas por runtime/ABI, memória nula ou região inválida MUST usar fault codes existentes; a saída temporária, RAM e CPU permanecem inalteradas. Para falha reportável, se `rt==NULL`, `next_pc=0`; caso contrário, recebe o PC guest atual.
- TLH-05: lowering LLVM MUST capturar Rb antes da chamada ao helper, e só gravar Rd/PC+2/ciclo+1 depois de sucesso. Fault termina sem commit parcial. O guard de overflow de ciclo antecede qualquer efeito.
- TLH-06: discovery MUST invalidar somente Rd e manter os fatos não afetados. Single-step sem memória MUST retornar `MemoryRuntimeRequired`; não há execução interpretada ou JIT.
- TLH-07: ABI v3/v4, layouts, GBA IR v2 e manifesto permanecem inalterados; nenhuma região fora do contrato pode ser lida ou tratada como RAM.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x8821` | Decodifica `LDRH r1,[r4,#0]` |
| Imediatos 0 e 31 | EA usa escala 2 e offsets 0/62 |
| EA par e ímpar | Bytes `33 44` dão `0x00004433` no EA par e `0x33000044` no EA ímpar, por rotação u32 |
| `Rd==Rb` | EA usa o base pré-instrução |
| EWRAM/IWRAM, primeira e última halfword | Leitura válida; `0x0203FFFF` e `0x03007FFF` alinham para a última halfword inteira |
| Próximo endereço após cada RAM | `0x02040000` e `0x03008000` faultam sem atravessar a região |
| EA wrapping para zero | Fault reporta `0x00000000`, sem alterar Rd/PC/flags/ciclo |
| MMIO, ROM, BIOS, mirror e fora de RAM | Fault explícito; saída temporária e estado guest preservados |
| Runtime, ABI, memória, saída nulos/inválidos | Código de fault contratual; saída nula não muta `GbaExit`, e saída guest nula preserva `GbaExit` byte a byte |
| O0/O2 strict-AOT | Mesmo resultado; PC/ciclo avançam somente após sucesso |
| LDRH por registrador (`0x5A49`) e LDRSH | Rejeição antes de publicar pacote parcial |
| LeafGreen fixada | Nova tentativa registra apenas fronteira estática e não afirma execução/compatibilidade |

## Questões abertas

- Qual região e conteúdo guest r4 têm na execução real do caminho de LeafGreen que alcança `0x08000B84`?
- Qual contrato de bus/eventos é necessário para as próximas leituras fora de RAM física?

## Fontes e referências

- [KNO-001 / SRC-40](../knowledge/KNO-001-evidence.md) registra mGBA e gba-recomp nos commits fixados e limitações da semântica observada.
- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa identidade, fronteira estática, hashes e ausência de execução comercial.
- [SPEC-027](SPEC-027-thumb-strh-immediate.md), [SPEC-034](SPEC-034-abstract-memory-discovery.md), [SPEC-038](SPEC-038-discovery-state-budget.md) e [SPEC-040](SPEC-040-thumb-ldr-word-immediate.md).
- [ADR-029](../adr/ADR-029-thumb-ldrh-immediate.md) decide o helper halfword aditivo e os limites de RAM.
