# SPEC-030 — ORRS Thumb Format 4 por registrador em AOT estrito

Estado do documento: Accepted. Progresso da implementação: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

Após SPEC-029, a descoberta estática da ROM fixada em KNO-004 parou em `0x0800099E`, raw `0x4318`. Os campos inferem Thumb Format 4 `ORRS r0,r3`; SRC-31 registra observações nos commits fixados das referências. Não foram observados valores de registradores, flags ou execução dinâmica.

Esta tranche aceita somente a suboperação ORR de registrador do Format 4 Thumb, com `Rd` e `Rs` em r0–r7. Implementa `Rd := old(Rd) OR old(Rs)`, atualização de N/Z e preservação de C/V. Não amplia suporte para os outros opcodes Format 4.

## Fora de escopo

AND, EOR, shifts por registrador, ADC, SBC, ROR, TST, NEG, CMP, CMN, MUL, BIC e MVN Format 4; registradores high ou PC; outros formatos ALU Thumb; instruções predicadas; flags derivadas de operações não ORR; e qualquer fallback interpretado/JIT.

## Termos e invariantes

- `Rd` é `bits[2:0]`; `Rs` é `bits[5:3]`.
- O bit S é implícito nesta instrução Thumb e atualiza apenas N/Z. O shifter associado é LSL #0, cuja saída de carry preserva C.
- Quando `Rd==Rs`, ambos operandos são lidos do estado anterior à instrução.
- Cada passo aceito avança PC Thumb em dois bytes e soma uma unidade abstrata a `guest_cycle`; isso não representa ciclos físicos.
- O estado guest usa inteiros de 32 bits. LLVM e estruturas host não fazem parte da ABI GBA.

## Requisitos normativos

- TORR-01: o decoder MUST aceitar exatamente a suboperação quando `(encoding & 0xFFC0) == 0x4300`, extrair `Rd=encoding&7` e `Rs=(encoding>>3)&7`, e produzir `ThumbOperation::OrrRegister`. Os demais formatos e suboperações ALU Thumb Format 4 MUST continuar rejeitados.
- TORR-02: para cada instrução aceita, a GBA IR MUST conservar PC, encoding, `Rd` e `Rs`. O verificador MUST conferir alinhamento Thumb, campos contra o encoding, faixa r0–r7 e presença do sucessor sequencial em `PC+2`; deve rejeitar campos adulterados ou operação usada fora de Thumb.
- TORR-03: a semântica MUST calcular `result = old(Rd) | old(Rs)` em u32, escrever somente `Rd`, definir N pelo bit 31 do resultado e Z quando resultado for zero. Deve preservar C, V e todos os demais bits CPSR, sem writeback em outro registrador. Alias lê os dois operandos antes de gravar.
- TORR-04: LLVM MUST emitir OR nativo e atualizar N/Z preservando C/V e os bits restantes de CPSR. A operação é strict-AOT, não chama helper runtime e não pode recorrer a interpretador, JIT ou fallback.
- TORR-05: descoberta MUST avançar para `PC+2`, preservar todos os registradores abstratos exceto `Rd` e propagar OR sobre conjuntos de constantes conhecidos. Se qualquer operando for desconhecido ou o conjunto resultante exceder o limite de 16 constantes de `AbstractValue`, `Rd` deve virar desconhecido sem truncamento ou escolha heurística. Fluxos de dispatch devem continuar limitados às regras vigentes de fechamento estático.
- TORR-06: cada instrução aceita MUST avançar `guest_cycle` em uma unidade abstrata. A atualização não descreve timing físico, waitstates ou sincronização com periféricos.
- TORR-07: a compilação de qualquer instrução alcançável fora do subset contratado MUST falhar antes de publicar pacote parcial. Nenhum caminho strict-AOT pode cair silenciosamente em interpretação ou JIT.

## Interfaces, entradas e saídas

Adicionar variante específica `ThumbOperation::OrrRegister { rd, rs }`. A execução da IR e o backend LLVM usam somente esse contrato; não introduzir enum genérico para todos os opcodes Format 4 nem helper externo. A descoberta aplica operação OR a pares de valores abstratos, preservando o limite já definido em `AbstractValue`.

## Erros, limites e observabilidade

Formato ou opcode não contratado resulta em `unsupported_thumb_instruction` no PC correspondente. Encoding inconsistente com a operação verificada resulta em erro de verificação da IR. O resultado observável é composto por Rd, CPSR, PC Thumb e `guest_cycle`; nenhuma memória é acessada ou alterada.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| `0x4318` | Decodifica ORRS `r0,r3` |
| `Rd`/`Rs` em r0–r7 | O decoder extrai as 64 combinações de registradores da suboperação ORR |
| `Rd==Rs` | Resultado usa o valor anterior do registrador e mantém estado coerente |
| Resultado zero, positivo e com bit 31 | N/Z refletem o resultado |
| C/V e CPSR não relacionado às flags | C/V e os outros bits CPSR são preservados |
| Discovery com constantes | Propaga o OR exato e fecha sucessor Thumb `PC+2` |
| Discovery com desconhecido ou expansão >16 | Somente Rd vira desconhecido; não há truncamento de conjuntos |
| Sequência ORR seguida de BX | Fecha somente alvos permitidos pelas regras de BX existentes |
| O0/O2 | Executáveis strict-AOT produzem o mesmo estado; manifesto mantém o contrato AOT |
| Outros opcodes Format 4 e formato vizinho | AND/EOR/MVN e encodings vizinhos seguem rejeitados antes de publicar pacote |

Testes de decoder/verificador MUST cobrir a máscara, as combinações de campos, PC desalinhado, sucessor ausente, operação em modo ARM e campos divergentes do encoding. Testes da IR e LLVM MUST cobrir resultados zero/positivo/sinalizado, os quatro pares C/V, preservação dos bits restantes de CPSR, alias e avanço de PC/ciclo. Testes de discovery MUST cobrir constantes, desconhecido, cardinalidade acima de 16 e o fluxo ORR→BX. O gate estritamente AOT MUST executar fixtures O0/O2 e verificar os negativos sem pacote parcial. Após os gates, repetir compilação estática de LeafGreen e registrar somente a nova fronteira realmente alcançada.

## Questões abertas

Depois desta suboperação, a próxima instrução alcançável será descoberta novamente a partir da ROM fixada. A spec não presume qual será nem declara cobertura das demais suboperações Format 4.

## Referências e evidências

- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa a identidade da ROM e as fronteiras obtidas por descoberta estática.
- [KNO-001 / SRC-31](../knowledge/KNO-001-evidence.md) registra fontes nos commits fixados de mGBA e gba-recomp. Essas fontes não validam o código local nem fornecem observação dinâmica da ROM.
- [SPEC-002](SPEC-002-armv4t-semantics.md) define o contrato geral de CPU; [SPEC-018](SPEC-018-arm-thumb-branch-exchange.md) delimita o fechamento de destinos BX.
