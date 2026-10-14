# SPEC-031 — Thumb Format 3 imediato em AOT estrito

Estado do documento: Accepted. Implementada no commit `2020649`; evidência: [gate GBA-020](../knowledge/evidence/gba020-thumb-format-3-macos-arm64.json). Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-030, a descoberta estática da ROM fixada em KNO-004 parou em `0x080009A2`, raw `0x3201`. Os campos inferem Thumb Format 3 `ADDS r2,#1`; SRC-22 registra observações de fonte para os quatro subopcodes `MOVS/CMP/ADD/SUB`. Esta inferência não contém valores de registradores, flags ou execução dinâmica.

Esta tranche completa o grupo Thumb Format 3 imediato: `MOVS Rd,#imm8`, `CMP Rd,#imm8`, `ADDS Rd,#imm8` e `SUBS Rd,#imm8`. `MOVS` já existe sob SPEC-021 e deve permanecer semanticamente compatível; a nova spec amplia o decoder e acrescenta CMP/ADD/SUB. Não cobre outros formatos ALU Thumb nem qualquer acesso à memória.

## Requisitos normativos

- TFI3-01: o decoder MUST aceitar somente o grupo `(encoding & 0xE000) == 0x2000`; `opcode = bits[12:11]`, `Rd = bits[10:8]` (r0–r7) e `imm8 = bits[7:0]` unsigned. `opcode` 00/01/10/11 seleciona MOVS/CMP/ADDS/SUBS. Codificações fora do Format 3 permanecem sujeitas aos contratos e rejeições existentes.
- TFI3-02: MOVS MUST escrever `zero_extend(imm8)` em Rd, definir N=0 e Z quando `imm8==0`, preservar C/V e todos os bits CPSR fora de N/Z. Não altera memória ou modo Thumb.
- TFI3-03: CMP MUST definir `imm = zero_extend(imm8)`, calcular `result = old(Rd) - imm` modulo 2^32 e atualizar N/Z/C/V sem gravar nenhum registrador. N recebe `result[31]`; Z recebe 1 exatamente quando `result==0`. C=1 indica ausência de borrow (`old(Rd) >= imm`); V é `((old(Rd) ^ imm) & (old(Rd) ^ result) & 0x80000000) != 0`.
- TFI3-04: ADDS/SUBS MUST definir `imm = zero_extend(imm8)`, ler `old(Rd)`, calcular soma/subtração modular de 32 bits, gravar o resultado em Rd e atualizar N/Z/C/V. N recebe `result[31]`; Z recebe 1 exatamente quando `result==0`. Para ADDS, C é carry unsigned da soma de 33 bits e V é `(~(old(Rd) ^ imm) & (old(Rd) ^ result) & 0x80000000) != 0`. Para SUBS, C é ausência de borrow (`old(Rd) >= imm`) e V usa a fórmula de subtração de TFI3-03. CPSR.C anterior não é carry-in.
- TFI3-05: todas as formas MUST preservar bits CPSR fora das flags que a operação altera: MOVS altera somente N/Z; CMP/ADDS/SUBS alteram NZCV. Modo, T, máscaras e bits restantes permanecem inalterados. PC avança `+2` e `guest_cycle` `+1` unidade abstrata, sem alegação de timing físico.
- TFI3-06: a GBA IR MUST manter PC, encoding, operação, Rd e imm8. O verificador MUST validar alinhamento Thumb, subopcode, operação, Rd e imm8 contra o encoding em cada variante do Format 3, incluindo o `MovImmediate` existente; também MUST validar faixa r0–r7 e sucessor Thumb em `PC+2`. Encoding incompatível com a variante ou com seus campos, campos fora de faixa e sucessor ausente MUST ser rejeitados. A execução da IR MUST rejeitar estado fora do modo Thumb sem alteração parcial.
- TFI3-07: LLVM MUST emitir operações nativas para as quatro formas em strict-AOT; não pode haver chamada ao interpretador, JIT ou fallback. `GBA_IR_VERSION` e ABI não mudam.
- TFI3-08: descoberta MUST propagar MOVS como constante imediata. ADDS/SUBS MUST transformar cada constante conhecida de Rd com a operação modular; desconhecido invalida apenas Rd. CMP MUST preservar todos os registradores abstratos. As flags abstratas e fechamento condicional de branches não são ampliados nesta tranche.
- TFI3-09: a compilação MUST falhar antes de publicar pacote parcial se alcançar uma instrução ainda não suportada, sem fallback implícito.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x2000`, `0x2FFF`, `0x3201`, `0x3F80` | MOVS r0,#0; CMP r7,#255; ADDS r2,#1; SUBS r7,#128 |
| MOVS #0/#255, C=V=1 | N/Z corretos; C/V e bits restantes preservados |
| CMP Rd,#same | Rd inalterado; resultado zero, Z=1, C=1, N=V=0 |
| CMP `0,#1` | Rd inalterado; N=1, Z=0, C=0, V=0 |
| CMP `0x80000000,#1` | Rd inalterado; resultado de comparação `0x7FFFFFFF`; V=1, C=1 |
| ADDS `0xFFFFFFFF,#1` | Rd=0; N=0,Z=1,C=1,V=0 |
| ADDS `0x7FFFFFFF,#1` | Rd=`0x80000000`; N=1,Z=0,C=0,V=1 |
| SUBS `0,#1` | Rd=`0xFFFFFFFF`; N=1,Z=0,C=0,V=0 |
| SUBS `0x80000000,#1` | Rd=`0x7FFFFFFF`; N=0,Z=0,C=1,V=1 |
| PC/ciclo e CPSR | PC+2; ciclo abstrato +1; preservar bits não afetados e modo Thumb |
| Discovery | MOVS propaga imediato; ADDS/SUBS propagam constantes; desconhecido invalida Rd; CMP não altera registradores |
| O0/O2 | As quatro formas produzem estados observáveis esperados e manifesto strict-AOT |
| Opcode vizinho ainda não aceito | Falha no PC de origem e sem pacote parcial |

## Fixtures e verificações

Adicionar testes de decoder/verificador para os quatro subopcodes, todos os registradores Rd, imediatos 0/255, encoding inconsistente, PC desalinhado e sucessor ausente. Os testes de execução da IR MUST cobrir vetores de overflow/carry/borrow, CMP sem writeback, MOVS preservando C/V, CPSR fora das flags, PC/ciclo e erro atômico para CPSR.T=0. Discovery MUST verificar propagação e invalidação. O gate GBA-020 MUST compilar/rodar fixtures O0/O2 para os quatro subopcodes, validar manifesto strict-AOT e manter rejeições sem pacote parcial para instruções vizinhas não aceitas. Após o gate, repetir compilação estática de LeafGreen e registrar somente a nova fronteira realmente alcançada.

As fixtures sintéticas validam os casos contratuais; não provam equivalência em hardware nem execução de LeafGreen.

## Relação com SPEC-021 e referências

SPEC-031 amplia o escopo de decode que SPEC-021 havia limitado a MOVS. TFI3-01 substitui a restrição de escopo e a exigência de rejeitar CMP/ADD/SUB em TMOVI-01 e TMOVI-05. TFI3-09 mantém a exigência de falha estrita, sem pacote parcial, para qualquer instrução ainda não suportada. TMOVI-02 e os critérios de TMOVI-03/04 referentes a MOVS continuam vigentes. A forma `ThumbOperation::MovImmediate` e sua semântica aceita permanecem compatíveis. SRC-32 em [KNO-001](../knowledge/KNO-001-evidence.md) fixa as observações de fonte nos commits mGBA e gba-recomp. [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa ROM, hash e fronteira estática. As fontes não validam a implementação local, hardware ou execução dinâmica do jogo.
