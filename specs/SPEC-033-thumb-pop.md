# SPEC-033 — POP Thumb de registradores baixos em AOT estrito

Estado do documento: Accepted. Implementação pendente; progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-032, a descoberta estática da ROM LeafGreen fixada em KNO-004 parou em `0x080009B6`, raw `0xBC08`. Os campos do formato Thumb POP inferem `POP {r3}`: leitura da palavra em SP para r3 e incremento de SP em quatro bytes. SRC-34 registra observações nos commits fixados de gba-recomp e mGBA. Isso é classificação estática; não há valor de SP, conteúdo de memória ou execução dinâmica observado.

Esta tranche aceita POP Thumb com uma lista não vazia somente de r0–r7. O bit que seleciona PC permanece rejeitado. O acesso deve estar alinhado e inteiramente contido em EWRAM ou IWRAM físicas. Não cobre mirrors, MMIO, outras regiões, POP com PC, demais transferências múltiplas nem bus/timing geral.

## Requisitos normativos

- TPOP-01: o decoder MUST reconhecer o grupo POP quando `(encoding & 0xFE00) == 0xBC00`. Só aceita `encoding[8] == 0` e lista baixa `encoding[7:0] != 0`. Se o bit 8 selecionar PC, a lista for vazia ou o encoding não corresponder ao grupo, strict-AOT MUST rejeitar a instrução sem publicar pacote.
- TPOP-02: a IR MUST representar o formato e a lista baixa preservando PC e encoding. O verificador MUST validar máscara, bit de PC, lista não vazia e correspondência integral dos campos com o encoding; também MUST exigir PC Thumb alinhado e sucessor em `guest_pc.wrapping_add(2)`. POP com PC não pode ser representado por esta variante.
- TPOP-03: a semântica é `LDMIA SP!, {rlist}`. A partir do SP de entrada, cada registrador selecionado em ordem crescente r0–r7 recebe uma palavra little-endian no endereço corrente; cada palavra avança o endereço em quatro bytes. Após todas as leituras válidas, SP recebe `old_sp + 4 * popcount(rlist)` em aritmética u32. PC avança dois bytes e o ciclo abstrato soma uma unidade. CPSR, modo, registradores não listados e memória permanecem iguais.
- TPOP-04: este recorte strict-AOT MUST aceitar somente SP múltiplo de quatro e o intervalo completo `[SP, SP + 4 * popcount(rlist))` dentro de uma única faixa física EWRAM `[0x02000000,0x02040000)` ou IWRAM `[0x03000000,0x03008000)`. O alinhamento e a faixa são limites do contrato local v1; não declaram que POP desalinhado seja arquiteturalmente indefinido ou um fault geral do ARMv4T. Leitura é little-endian; o helper converte cada endereço guest aceito em um offset validado de `GbaMemoryV1` e não o converte diretamente em ponteiro host. Não são aceitos mirrors nem regiões adicionais.
- TPOP-05: o helper aditivo `gba_pop_thumb_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t register_list)` MUST validar saída, runtime, versão ABI v3/CPU v2, memória, lista, alinhamento e região antes de qualquer mutação CPU. Deve ler todos os valores para temporários locais e só então gravar r0–r7 selecionados e SP. Em fault, nenhum registrador, PC, CPSR, ciclo ou byte de memória muda. Com `out != NULL`, `GbaExitV3.next_pc` recebe o PC guest atual, ou zero se `rt == NULL`; sucesso escreve `reason=GBA_EXIT_CONTINUE_V2`, `fault_address=0`, `fault_code=GBA_MEMORY_FAULT_NONE_V3`. Fault escreve `reason=GBA_EXIT_FAULT_V2` e: `RUNTIME_V3`/endereço zero para runtime nulo; `ABI_V3`/SP guest para versões ABI incompatíveis; `NULL_MEMORY_V3`/SP para memória nula; `REGISTER_LIST_V3`/SP para lista vazia ou bits fora de r0–r7; `ALIGNMENT_V3`/SP para SP desalinhado; `REGION_V3`/primeiro EA fora da faixa física para intervalo inválido. `out == NULL` retorna fault sem mutação ou relatório.
- TPOP-06: a ABI LLVM MUST chamar o helper uma vez, propagar fault sem executar o commit da instrução, e em sucesso avançar PC/ciclo pelos caminhos existentes. A operação MUST ser nativa strict-AOT: sem interpretação, JIT ou fallback. `GBA_IR_VERSION` e layouts dos ABIs v1–v4 permanecem inalterados.
- TPOP-07: discovery MUST invalidar como desconhecidos os valores abstratos dos registradores selecionados e preservar os demais registradores. Para cada valor de SP conhecido, MUST propagar `SP.wrapping_add(4 * popcount(rlist))`; `Unknown` MUST permanecer `Unknown`. MUST enfileirar somente `PC+2`; não há branch neste formato.
- TPOP-08: qualquer falha de cobertura ou de validação do encoding MUST ocorrer antes da publicação do pacote, sem artefato parcial.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0xBC08` | Decode como POP `{r3}` |
| Lista `{r0,r2,r7}` | Leituras em SP, SP+4, SP+8 para r0, r2, r7; SP final SP+12 |
| Lista baixa não vazia | Decoder e verificador preservam a lista; discovery invalida somente os destinos selecionados |
| Lista vazia | Rejeição strict-AOT antes do pacote |
| Bit 8 seleciona PC (`0xBDxx`) | Rejeição strict-AOT antes do pacote |
| EWRAM/IWRAM | Palavras little-endian são lidas nas duas regiões físicas |
| Última faixa válida | Palavra final em `0x0203FFFC` ou `0x03007FFC` é aceita quando o intervalo termina no limite |
| Intervalo além do limite ou região não coberta | Fault antes de qualquer gravação nos registradores |
| SP desalinhado | Fault de alinhamento; CPU sem mutação |
| Falha tardia no intervalo | Nenhum destino parcial é escrito; fault aponta primeiro EA fora da RAM |
| Estado de CPU | Só destinos selecionados, SP, PC+2 e ciclo+1 mudam; CPSR e demais destinos preservados |
| Discovery | Registradores carregados ficam desconhecidos; cada SP constante soma o tamanho com wrapping; SP desconhecido continua desconhecido |
| O0/O2 | Manifestos strict-AOT e estados guest esperados para POP `{r3}` e lista múltipla |
| POP com PC/encoding vizinho | Rejeitado sem pacote parcial |

## Fixtures e verificações

Adicionar testes de decoder/verificador para lista vazia, `{r0}`, `{r3}`, lista cheia, bit PC, encoding vizinho, PC desalinhado, sucessor ausente e campos adulterados. O teste C da ABI deve cobrir EWRAM/IWRAM, little-endian, múltiplos destinos, SP final, alinhamento, limites exatos, região não coberta, cada fault, `out == NULL`, ABI inválida e atomicidade quando o intervalo cruza uma fronteira. Discovery deve testar invalidação apenas dos registradores lidos e SP constante/desconhecido. O gate GBA-020 MUST emitir e executar fixtures O0/O2 com leitura de memória conhecida e validar manifestos strict-AOT; instruções POP com PC e lista vazia devem falhar sem pacote parcial. Após o gate, repetir a descoberta estática LeafGreen e registrar somente a nova fronteira alcançada.

Fixtures sintéticas confirmam somente os casos e valores iniciais declarados. Não demonstram valor de SP ou conteúdo de stack durante a execução de LeafGreen.

## Relação com specs e referências

SPEC-033 complementa [SPEC-019](SPEC-019-thumb-push.md): ambas cobrem transferências Thumb da stack, mas PUSH usa decrement-before e POP increment-after. A decisão de ABI e atomicidade está em [ADR-020](../adr/ADR-020-additive-thumb-pop-abi.md). SRC-34 em [KNO-001](../knowledge/KNO-001-evidence.md) registra as fontes fixadas; [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa a ROM e sua fronteira estática. As fontes não demonstram valores dinâmicos, execução do jogo ou correção em hardware.
