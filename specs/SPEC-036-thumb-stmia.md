# SPEC-036 — Thumb STMIA de registradores baixos em AOT estrito

Estado do documento: Accepted. Implementação pendente; o progresso está no [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

A descoberta estática da LeafGreen no SHA-256 registrado em [KNO-004](../knowledge/KNO-004-rom-intake.md) atravessa o LDMIA em `0x080006A8` e para no raw `0xC201` em `0x080006AA`. Pelos campos, `0xC201` infere Thumb `STMIA r2!,{r0}`. Esta tranche adiciona somente STMIA T1 para r0–r7 ao decoder, GBA IR, descoberta, helper ABI v3 e lowering LLVM strict-AOT.

O helper oferece RAM física EWRAM/IWRAM. Não modela o bus geral, Data Abort arquitetural, outras regiões, mirrors ou timing físico. Strict-AOT continua sem fallback implícito para interpretação/JIT.

## Fora de escopo

- Thumb LDMIA adicional, STMIA ARM e demais formatos ARM LDM/STM.
- Lista vazia ou base na lista com um registrador menor selecionado: a ARM classifica esses casos como UNPREDICTABLE; o decoder deve rejeitá-los sem pacote parcial.
- Stores para ROM, BIOS, mirrors, I/O, palette, VRAM, OAM ou save hardware.
- Atomicidade como propriedade de hardware em Data Abort. A atomicidade do helper é política local para faults de endereços fora do subset.
- Bus, waitstates e ciclos físicos ARM7TDMI/GBA.

## Termos e invariantes

- T1: instrução Thumb `0xC000..=0xC7FF`, `Rb=(encoding>>8)&7`, `rlist=encoding&0xFF`.
- Lista: máscara não vazia dos registradores r0–r7. Se Rb estiver na lista, Rb deve ser o registrador selecionado de menor número; caso contrário, a instrução é imprevisível e rejeitada.
- `old_Rb` é capturado antes da operação. Para cada registrador selecionado em ordem crescente, `EA_i = (old_Rb.wrapping_add(4*i)) & !3`; a word desse registrador é gravada little-endian nesse endereço.
- Writeback sempre ocorre: `Rb = old_Rb.wrapping_add(4*popcount(rlist))`, preservando os dois bits baixos originais. Se Rb estiver na lista como o menor selecionado, a memória recebe `old_Rb`; o valor final de Rb continua sendo o writeback.
- STMIA preserva CPSR e todos os registradores fora de Rb. Não altera flags. Sucesso avança o PC Thumb para `PC+2`.
- A sombra da SPEC-034 pode registrar palavras da operação somente quando Rb é singleton e todo o intervalo alinhado fica em uma região física suportada; os valores-fonte vêm do estado anterior ao writeback. Caso contrário, os fatos de memória são limpos conservadoramente.
- O ciclo local segue `guest_cycle + 1` por instrução; não é timing físico.

## Requisitos normativos

- STM-01: o decoder MUST aceitar somente T1 (`encoding & 0xF800 == 0xC000`) com lista não vazia. MUST rejeitar lista vazia, formato adjacente e Rb listado depois de outro registrador selecionado, por ser UNPREDICTABLE. O verificador MUST conferir encoding, Rb/lista, validade de r0–r7 e successor Thumb `PC+2`.
- STM-02: o verificador GBA IR MUST garantir que o Rb/lista da operação correspondem ao encoding. Formas imprevisíveis não podem ser construídas ou verificadas como programa válido.
- STM-03: a semântica MUST gravar os valores de entrada dos registradores selecionados em ordem crescente nos EAs alinhados para baixo definidos acima. Deve capturar os valores-fonte antes de writeback; se Rb for o menor membro, o valor armazenado é `old_Rb`. Após as gravações, Rb recebe writeback com wrap u32. CPSR e outros registradores permanecem intactos.
- STM-04: a descoberta MUST aplicar writeback abstrato em Rb para cada estado finito e MUST preservar demais registradores. Para base singleton e intervalo em EWRAM/IWRAM físicas, deve invalidar fatos sobrepostos e registrar os valores abstratos de entrada escritos pela STMIA na sombra esparsa da SPEC-034. Slot cuja origem seja Unknown permanece ausente. Base Unknown/múltipla, wrap ou intervalo não suportado MUST limpar todos os fatos de memória.
- STM-05: a ABI adiciona `gba_stmia_thumb_v3(rt,out,base_register,register_list)` sem alterar layouts nem versões existentes. O helper MUST detectar ponteiros nulos de `rt`, `out` e `rt->memory`, validar ABI v3/CPU v2, Rb r0–r7, lista não vazia, regra previsível de base na lista e todas as words antes de alterar memória guest ou CPU. O chamador deve passar ponteiros não nulos válidos; a ABI C não valida ponteiros arbitrariamente inválidos. Deve restringir a transferência contígua alinhada a uma única região física EWRAM ou IWRAM.
- STM-06: o helper MUST alinhar cada endereço efetivo para baixo; writeback usa o valor de base não alinhado e preserva seus bits baixos. Deve capturar os valores-fonte dos registradores antes de commit, gravar little-endian em ordem crescente e só então fazer writeback. Um fault local preserva memória guest e todos os registradores; PC/CPSR/ciclo não são alterados pelo helper. Esta política não afirma atomicidade de Data Abort em hardware.
- STM-07: região não suportada, faixa que cruza o limite, wrapping de endereço, ABI inválida, argumento inválido ou saída/runtime nulos produz fault explícito. `fault_address` identifica o primeiro endereço efetivo inválido, alinhado para baixo quando aplicável. Sem saída de helper válida, retorna fault sem mutação guest.
- STM-08: o lowering LLVM MUST validar overflow de ciclo antes da chamada, chamar o helper uma vez e só avançar PC para `PC+2` e ciclo em 1 após sucesso. Falha retorna o estado do helper sem executar writeback no lowering.
- STM-09: strict-AOT MUST rejeitar compilação de encoding imprevisível sem publicar pacote parcial e MUST não usar intérprete/JIT. O executor single-step sem contexto de memória deve retornar `MemoryRuntimeRequired` para STMIA.

## Interfaces, entradas e saídas

- IR Thumb: `StoreMultipleIncrementAfter { base_register, register_list }`.
- ABI C aditiva: `gba_stmia_thumb_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t base_register, uint32_t register_list)`.
- `GbaExitV3` informa sucesso ou fault conforme os códigos já existentes; não recebe novo campo nem altera layout.
- O helper grava apenas nos arrays host EWRAM/IWRAM identificados pelos endereços guest numéricos. Endereço guest nunca é convertido diretamente em ponteiro host.

## Erros, limites e observabilidade

As words são validadas antes do commit, em uma única região física. A lista vazia e base listada sem ser o menor membro são rejeições de compilação por imprevisibilidade arquitetural, não faults de runtime. Endereço fora do subset compila apenas se os demais requisitos AOT forem satisfeitos e falha explicitamente na execução; não há fallback.

A evidência de LeafGreen consiste em bytes do artefato e estado abstrato do processo compilador. `r0=0x08000821` e `r2=0x03003540` não são valores observados em execução guest. Compilar além de `0xC201` não prova que o jogo tome o caminho, que o store seja executado ou que LeafGreen seja compatível.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| Decoder `0xC201` | Rb=2, lista `{r0}`; STMIA é aceito |
| Encoding `0xC000` | Lista vazia rejeitada sem pacote parcial |
| Encoding com Rb depois de membro menor | Rejeição explícita como imprevisível |
| Helper, lista múltipla | Registradores e palavras em endereços crescentes e ordem little-endian |
| Rb fora da lista | Writeback `old_Rb + 4*popcount` com wrap |
| Rb como menor membro | Word gravada recebe base original; base final recebe writeback |
| Base desalinhada | EAs alinham para baixo, writeback preserva bits baixos |
| Faixa inválida/fault | Rejeição atômica local; nenhum byte/reg guest muda |
| `rt`, `out` ou memória nulos | Fault explícito e nenhum estado guest alterado; ponteiros não nulos devem ser válidos pelo contrato do chamador |
| Discovery singleton | Base recebe writeback; fatos da sombra são substituídos pelos valores-fonte conhecidos |
| Discovery incerto/fora de RAM | Sombra é limpa sem inventar constantes |
| Fixture LeafGreen | Descoberta passa `0xC201` e registra a fronteira seguinte, ainda como compilação estática |
| O0/O2 strict-AOT | Mesmo PC, CPSR, ciclo, registradores e RAM; manifestos strict-AOT; vizinhos imprevisíveis não geram pacote |

## Questões abertas

- Quais formatos e regiões são necessários após a nova fronteira LeafGreen?
- Como SPEC-005 incorporará custos de barramento e aborts de transferências múltiplas?
- Quando haverá um oráculo runtime REA ou diferencial para a ROM `.gba`?

## Referências e evidências

- [SPEC-005 — timing e bus](SPEC-005-runtime-timing.md), [SPEC-016 — STR word](SPEC-016-ram-word-stores.md), [SPEC-034 — memória abstrata](SPEC-034-abstract-memory-discovery.md), [KNO-004 — intake](../knowledge/KNO-004-rom-intake.md).
- ARM Architecture Reference Manual DDI 0100I, Thumb A7.1.57 e Data Abort A2.6.6, [manual oficial ARM](https://documentation-service.arm.com/static/5f8dacc8f86e16515cdb865a); ARM7TDMI TRM DDI 0029G §§2.8.6 e 6.10, [manual oficial](https://documentation-service.arm.com/static/5e8e1323fd977155116a3129).
- Fontes e limites em [KNO-001 SRC-36](../knowledge/KNO-001-evidence.md); código vendor é somente leitura e permanece fixado nos commits registrados.
