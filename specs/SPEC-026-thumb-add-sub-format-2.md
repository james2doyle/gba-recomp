# SPEC-026 — ADD/SUB Thumb Format 2 em AOT estrito

Estado do documento: Draft. Implementação e progresso ficam no [ROADMAP](../ROADMAP.md).

## Contexto e limites

Após a implementação de SPEC-025, a descoberta estática da ROM de KNO-004 alcançou o halfword raw `0x1C10` em `0x080003B6` e parou no decoder. Pelos campos, ele é Thumb Format 2 `ADDS r0,r2,#0`. SRC-27 registra as observações nas referências fixadas. Isso é uma inferência de encoding sobre a ROM, não um trace dinâmico ou execução do jogo.

O contrato abrange as quatro formas do formato: ADD/SUB com `imm3` e ADD/SUB com registrador. Não inclui `ADC`/`SBC`, outros formatos ALU Thumb, operandos de PC, escritas de PC ou qualquer acesso à memória.

## Requisitos

- TAS2-01: o decoder MUST aceitar somente halfwords com `(encoding & 0xF800) == 0x1800`. MUST extrair `I=bit[10]`, `Op=bit[9]`, `operand=bits[8:6]`, `Rn=bits[5:3]`, `Rd=bits[2:0]`. `I=1` torna `operand` um `imm3` unsigned em 0–7; `I=0` torna o campo `Rm`, inclusive `Rm=r0` quando o campo é zero. `Op=0` é ADD e `Op=1` é SUB. Os registradores são sempre r0–r7.
- TAS2-02: as operações MUST ler os valores de entrada de `Rn` e, quando `I=0`, `Rm` antes de escrever `Rd`, permitindo aliasing entre destino e fonte. O resultado MUST ser modular em 32 bits: ADD calcula `x+y`; SUB calcula `x-y`. O CPSR.C anterior não é carry-in.
- TAS2-03: toda forma MUST atualizar N/Z/C/V. N é o bit 31 do resultado e Z indica zero. Para ADD, C é carry unsigned de 33 bits; para SUB, C é 1 quando `x >= y` (ausência de borrow). V MUST refletir overflow signed: `(~(x^y) & (x^result) & 0x80000000) != 0` em ADD e `((x^y) & (x^result) & 0x80000000) != 0` em SUB. Imediatos zero não preservam C/V: ADD `#0` produz C=0,V=0; SUB `#0` produz C=1,V=0.
- TAS2-04: bits de CPSR fora de NZCV MUST permanecer inalterados, inclusive T, modo e máscaras. Rd recebe o resultado; registradores não destinados permanecem inalterados. PC avança dois bytes e `guest_cycle` aumenta uma unidade abstrata, sem alegação de timing físico.
- TAS2-05: GBA IR MUST preservar PC, encoding e operandos/formato. O verificador MUST rejeitar PC Thumb ímpar, registradores inválidos, qualquer campo divergente do encoding ou falta de sucessor Thumb em `PC+2`. A operação MUST ser lowering native strict-AOT; não pode recorrer a interpretação, JIT ou fallback.
- TAS2-06: descoberta MUST propagar o valor constante de Rd quando Rn e, se aplicável, Rm forem conhecidos; se uma entrada for desconhecida, MUST invalidar a constante de Rd. O cálculo lê os valores anteriores à gravação de Rd, para que `0x1C10` seguido por `BX r0` feche um alvo somente quando ele cumprir as regras existentes de faixa, alinhamento e modo de [SPEC-018](SPEC-018-arm-thumb-branch-exchange.md).
- TAS2-07: outros formatos Thumb ainda sem contrato MUST continuar rejeitados antes de publicar pacote parcial.
- TAS2-08: a adição não altera formato persistido nem semântica incompatível existente; `GBA_IR_VERSION` permanece 1.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x1C10` | Decodifica `ADDS r0,r2,#0` (`I=1`, `Op=0`, `imm3=0`) |
| ADD `0xFFFFFFFF + 1` | Resultado 0; N=0,Z=1,C=1,V=0 |
| ADD `0x7FFFFFFF + 1` | Resultado `0x80000000`; N=1,Z=0,C=0,V=1 |
| SUB `0 - 1` | Resultado `0xFFFFFFFF`; N=1,Z=0,C=0,V=0 |
| SUB `0x80000000 - 1` | Resultado `0x7FFFFFFF`; N=0,Z=0,C=1,V=1 |
| ADD immediate `#0` e SUB immediate `#0` | N/Z vêm do resultado; C/V tornam-se 0/0 e 1/0 respectivamente, mesmo se flags iniciais forem 1 |
| Operando immediate `#7` e registrador `r0` | Decodificados distintamente; o campo zero com `I=0` lê o valor de r0 |
| Alias `Rd==Rn` e `Rd==Rm` | Resultado usa ambos os valores antigos das fontes |
| CPSR | Apenas NZCV mudam; T, modo, masks e bits restantes preservados |
| PC/ciclo | PC+2 e ciclo abstrato +1 |
| Constantes e BX | Propagação fecha um alvo estático válido pelas regras de SPEC-018; teste com `0x1C10` seguido de `BX r0` |
| O0/O2 | Estado observável equivalente e manifesto strict-AOT |
| Família fora do contrato | Falha no PC de origem, sem executável ou pacote parcial |

## Fixture e verificação

Adicionar fixtures sintéticas para as quatro combinações `I`/`Op`, incluindo `imm3=0` e `imm3=7`, formas por registrador, aliasing e os vetores de carry/borrow/overflow acima. Os testes do interpreter MUST validar todos os quatro bits NZCV de cada vetor e a preservação dos outros bits de CPSR. Testes do verificador MUST adulterar campos da IR e encodings, testar PC Thumb ímpar e ausência de sucessor em `PC+2`; a descoberta MUST exercitar propagação constante e invalidação. O gate GBA-020 MUST emitir e executar as fixtures aplicáveis em O0/O2, validar manifestos strict-AOT e preservar as rejeições sem pacote parcial. Uma fixture com `0x1C10` seguido de `BX r0` confirma fechamento apenas para alvo válido pelas regras de SPEC-018. Após o gate, repetir compilação estática LeafGreen e registrar somente o limite realmente observado.

Fixtures sintéticas verificam o subset descrito; não provam equivalência com hardware nem execução da ROM comercial.

## Evidência e referências

- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa ROM, SHA-256, compilação estática e limite observado.
- [KNO-001 / SRC-27](../knowledge/KNO-001-evidence.md) registra fonte de mGBA e gba-recomp nos commits fixados.
- [SPEC-002](SPEC-002-armv4t-semantics.md) é o contrato geral da CPU; esta spec delimita somente Thumb Format 2.

As fontes corroboram a classificação do formato e regras de execução; não validam a implementação local, hardware real ou execução de LeafGreen.
