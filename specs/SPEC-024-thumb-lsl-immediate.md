# SPEC-024 — Thumb LSL imediato em AOT estrito

Estado do documento: Draft. Implementação e progresso ficam no [ROADMAP](../ROADMAP.md).

## Contexto e limite

Na ROM fixada por KNO-004, a compilação estática do commit local `aab31ae` parou em `0x080003B2`, raw `0x04C9`, após o caminho que retorna do SWI #1. A leitura dos campos classifica esse halfword como Thumb Format 1 `LSLS r1,r1,#19`; a semântica é corroborada por SRC-25. Isso é descoberta AOT sobre bytes da ROM, não trace de runtime nem execução do jogo. Este contrato implementa somente LSL imediato Thumb; não libera as formas LSR/ASR nem shift por registrador.

## Requisitos

- TSLI-01: o decoder MUST aceitar como LSL imediato somente encodings com `(encoding & 0xF800) == 0`. MUST extrair `imm5=bits[10:6]`, `Rs=bits[5:3]` e `Rd=bits[2:0]`. No raw `0x04C9`, os campos são `imm5=19`, `Rs=r1`, `Rd=r1`.
- TSLI-02: o resultado MUST ser `Rd = (Rs << imm5) mod 2^32`, lendo o valor antigo de `Rs`; alias `Rd==Rs` é válido.
- TSLI-03: N e Z MUST refletir o resultado. Para `imm5` entre 1 e 31, C MUST receber o bit `32-imm5` do valor antigo de `Rs`. Para `imm5=0`, C MUST permanecer inalterado. V MUST permanecer inalterado. CPSR.T, modo, bits de interrupção e demais registradores MUST permanecer inalterados.
- TSLI-04: a instrução MUST avançar PC em 2 bytes e incrementar `guest_cycle` em uma unidade abstrata. Isso não afirma equivalência de timing físico.
- TSLI-05: a GBA IR MUST representar a operação como `ThumbOperation::LslImmediate { rd, rs, amount }`, preservar guest PC e encoding e verificá-los contra os campos originais. A IR é reconstruída de ROM em cada compilação e não é persistida em cache; `GBA_IR_VERSION` permanece 1 enquanto operações existentes mantiverem a mesma semântica.
- TSLI-06: o lowerer MUST emitir código nativo estritamente AOT. Nenhum caminho pode cair em interpretação, JIT ou fallback; um shift LLVM de 32 bits MUST tratar `imm5=0` explicitamente.
- TSLI-07: LSR/ASR imediato, shifts por registrador e outras famílias Thumb MUST continuar rejeitados durante descoberta, sem publicar pacote parcial.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `imm5=0`, C inicialmente 0 e 1 | Resultado idêntico ao valor de entrada; N/Z calculados; C preservado em ambos os estados |
| `imm5=1` | Resultado deslocado; C recebe o bit 31 antigo; N/Z corretos |
| `imm5=19`, `r1=0x00001000` | `r1=0x80000000`; N=1, Z=0, C=0; V preservado; PC+2 e CPSR.T ativo |
| `imm5=31`, `r1=2` | `r1=0`; N=0, Z=1, C=1; V preservado |
| `Rd==Rs` | Leitura usa o valor antigo e commit produz resultado correto |
| O0 e O2 | Mesmo estado guest observável |
| LSR/ASR imediato ou shift por registrador | Falha strict-AOT identificada no PC; nenhum executável ou pacote parcial |

## Fixture e verificação

Adicionar modos sintéticos à fixture Tiny Counter. Preparar valor e flags por instruções já suportadas, entrar em Thumb, testar os valores de borda acima e observar resultado, CPSR, PC e ciclo no runner. O gate GBA-020 MUST cobrir O0/O2 e confirmar que os negativos não deixam pacote parcial. As fixtures são programas sintéticos e não afirmam equivalência diferencial com hardware.

## Evidência e referências

KNO-004 fixa a ROM em SHA-256 `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825` e registra a tentativa estática no commit `aab31ae`. SRC-25 reúne observações de fonte de mGBA (`143adf5aa2a60c6036810b0cd72445e13d74b0e6`) e gba-recomp (`de4edf59b872d887046d6a3b005e2df551b6d44c`). Elas corroboram decode e flags, mas não provam semântica em hardware nem execução da ROM. O runtime geral, ciclos de barramento, demais shifts e o próximo limite de descoberta permanecem fora deste contrato.
