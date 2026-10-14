# SPEC-035 — Thumb LDMIA de registradores baixos em AOT estrito

Estado do documento: Accepted. Implementação pendente; o progresso está no [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

O encoding `0xCB01` em `0x080006A8` é inferido como Thumb `LDMIA r3!, {r0}` e bloqueia a descoberta estática atual de LeafGreen. Esta tranche acrescenta a forma Thumb T1 `(encoding & 0xF800) == 0xC800`, lista não vazia de r0–r7, descoberta, execução por ABI v3 e lowering LLVM strict-AOT. Uma leitura com base comprovadamente singleton dentro da imagem ROM imutável pode ser materializada como constantes AOT; leituras dinâmicas usam o helper de RAM física.

O contrato melhora uma família de instrução, mas não implementa um bus geral nem comprova a execução do jogo comercial.

## Fora de escopo

- Thumb STMIA e formatos ARM LDM/STM.
- Lista vazia: o ARM Architecture Reference Manual a classifica como UNPREDICTABLE; o decoder local deve rejeitá-la, sem atribuir a ela a semântica legada observada nos emuladores.
- POP com PC, transferências a partir de ROM com base dinâmica ou múltiplas bases abstratas, mirrors, BIOS, I/O, VRAM, waitstates, exceção Data Abort arquitetural e timing real.
- Execução, compatibilidade ou suporte declarado para LeafGreen.

## Termos e invariantes

- T1: instrução Thumb LDMIA `0xC800..=0xCFFF`; `Rb=(encoding>>8)&7`, `rlist=encoding&0xFF`.
- `EA_i = (old_Rb.wrapping_add(4*i)) & !3`: endereço guest de cada word, em ordem crescente dos registradores selecionados. A normalização para baixo segue o comportamento observado nos dois cores de referência fixados; não deve ser extrapolada a outros loads.
- `writeback = old_Rb.wrapping_add(4*popcount(rlist))`, preservando os dois bits baixos de `old_Rb`, somente quando Rb não pertence à lista.
- As únicas regiões dinâmicas suportadas são EWRAM física `[0x02000000,0x02040000)` e IWRAM física `[0x03000000,0x03008000)`. Endereços guest permanecem inteiros e nunca são convertidos em ponteiros host.
- Um ROM fold é uma constante derivada exclusivamente dos bytes da imagem imutável identificada pelo SHA-256 do intake. Exige valor singleton provado para Rb em todos os predecessores analisados e todas as words dentro da imagem.
- O ciclo local continua sendo `guest_cycle + 1` por instrução; não representa ciclos reais de ARM7TDMI/GBA.

## Requisitos normativos

- LDM-01: o decoder MUST aceitar somente o grupo T1 com lista não vazia. MUST extrair Rb e a lista dos campos do encoding. MUST rejeitar STMIA, lista vazia e formatos adjacentes sem pacote parcial.
- LDM-02: o verificador GBA IR MUST conferir formato, Rb, lista e correspondência dos campos com o encoding. MUST exigir successor Thumb `PC+2`. Uma operação ROM resolvida MUST ainda identificar a base guest singleton, lista e palavras materializadas.
- LDM-03: para cada transferência, o registrador selecionado de menor número recebe a word do menor endereço efetivo; words subsequentes usam o próximo endereço alinhado crescente. O LDMIA MUST preservar CPSR e registradores não selecionados. Se Rb estiver na lista, a word carregada para Rb prevalece e não há writeback separado; caso contrário, Rb recebe `old_Rb + 4*popcount(rlist)` com wrap u32.
- LDM-04: descoberta MUST propagar valores provados pela sombra de memória da SPEC-034 e por ROM imutável somente para uma base singleton. Slots sem fato ficam `Unknown`. Base na lista recebe a abstração da word correspondente; base fora da lista recebe o writeback abstrato. LDMIA é somente leitura e MUST preservar os fatos da sombra.
- LDM-05: quando Rb é singleton e as words endereçadas pertencem à ROM fixada, discovery MUST materializar as words no GBA IR e a emissão AOT MUST gravá-las nos destinos, sem incluir ROM ou ponteiro host no pacote. Bases dinâmicas/múltiplas não podem presumir bytes ROM.
- LDM-06: o helper aditivo `gba_ldmia_thumb_v3(rt,out,base_register,register_list)` MUST validar ponteiros, ABI v3/CPU v2, base r0–r7, lista não vazia e todos os endereços antes de alterar CPU. MUST restringir o intervalo a uma única região física EWRAM ou IWRAM. Deve ler as words para temporários locais e só então gravar os destinos e, quando aplicável, writeback.
- LDM-07: o helper MUST alinhar cada endereço word para baixo conforme LDM-03. Região não suportada, faixa que cruza limite, ABI inválida ou argumento inválido produz fault explícito. Em fault, registradores, PC, CPSR, ciclo e memória guest permanecem inalterados. Esta política de fault local não afirma fidelidade de Data Abort.
- LDM-08: o lowering LLVM MUST chamar o helper dinâmico uma vez e só avançar PC para `PC+2` e ciclo em 1 após sucesso. O ciclo é pré-validado contra overflow antes da operação. O caminho ROM materializado tem os mesmos efeitos guest sem chamada ao helper.
- LDM-09: strict-AOT MUST reportar explicitamente falhas de compilação ou runtime do subset. Não pode recorrer implicitamente a intérprete ou JIT. O executor single-step sem contexto de memória MUST retornar `MemoryRuntimeRequired` para a forma dinâmica.

## Interfaces, entradas e saídas

- IR Thumb dinâmica: `LoadMultipleIncrementAfter { base_register, register_list }`.
- IR ROM resolvida: `LoadRomMultipleIncrementAfter { base_register, register_list, base_address, values_by_register }`, com words indexadas por r0–r7 e campos não selecionados ignorados.
- ABI C: símbolo novo aditivo `gba_ldmia_thumb_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t, uint32_t)`, sem mudança dos layouts ou das versões ABI existentes.
- Falhas do helper seguem `GbaExitV3` e os códigos existentes de faults de memória/registrador; não alteram estado guest.

## Erros, limites e observabilidade

O helper dinâmico não acessa ROM. Bases guest fora das duas faixas físicas falham explicitamente em runtime. A ROM só é suportada quando discovery prova base singleton e cada word está no intervalo da imagem imutável. Base singleton dentro do espaço ROM cuja faixa exceda o arquivo deve falhar na compilação, sem pacote parcial. Compilação estática e valores propagados não são evidência de execução da ROM.

O resultado de LeafGreen em `0x080006A8` deve ser registrado como descoberta estática com hash. O breakpoint no processo do recompilador fornece apenas estado abstrato interno; bytes ROM são observação do artefato. O word que LDMIA carregaria e o alvo subsequente são inferências condicionadas à execução da instrução. REA não abriu `.gba` e não forneceu Evidence ID de análise/decompilação.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| Decoder `0xCB01` | Rb=3, lista `{r0}`; formatos STM e lista vazia rejeitados |
| Helper EWRAM/IWRAM | Ordem crescente, little-endian, base fora da lista com writeback e base na lista sem writeback |
| Base incluída no meio da lista | Word do slot correspondente prevalece sobre writeback |
| Base desalinhada | Cada EA alinha para baixo e writeback preserva os bits baixos |
| Limite de faixa/fault | Intervalo inteiro validado antes de commit; nenhuma mutação guest em fault |
| Discovery com RAM abstrata | Propaga fatos conhecidos, deixa slots desconhecidos como Unknown, preserva a sombra |
| ROM singleton | Palavras fixadas materializadas; ROM, código objeto e LLVM não entram no pacote |
| Folha Green SHA-256 fixado | Descoberta passa `0xCB01`, registra estado e a próxima fronteira sem alegar execução |
| O0/O2 strict-AOT | Estado/PC/ciclo iguais ao esperado; rejeições não deixam pacote parcial |

## Questões abertas

- Quais instruções, memória mapeada e entradas dinâmicas permanecem depois da nova fronteira LeafGreen?
- Quando SPEC-005 modelará custos reais e eventos para transferências múltiplas?
- Quando haverá um oráculo runtime REA ou teste diferencial acessível para esse `.gba`?

## Referências e evidências

- [SPEC-005 — timing e bus](SPEC-005-runtime-timing.md), [SPEC-016 — STR word](SPEC-016-ram-word-stores.md), [SPEC-029 — LDRB](SPEC-029-thumb-ldrb-immediate.md), [SPEC-033 — POP](SPEC-033-thumb-pop.md), [SPEC-034 — memória abstrata](SPEC-034-abstract-memory-discovery.md).
- ARM Architecture Reference Manual DDI 0100I, Thumb A7.1.27, [cópia hospedada pela TI](https://e2e.ti.com/cfs-file/__key/communityserver-discussions-components-files/1023/ARM-Architecture.pdf); ARM7TDMI TRM DDI 0029G, [manual oficial de interface de memória](https://documentation-service.arm.com/static/5e8e1323fd977155116a3129).
- Observações dos vendors nos commits fixados estão em [KNO-001 SRC-35](../knowledge/KNO-001-evidence.md); limites do artefato e REA estão em [KNO-004](../knowledge/KNO-004-rom-intake.md).
