# SPEC-024 — Thumb LSL imediato em AOT estrito

Estado do documento: Draft. Implementação e progresso ficam no [ROADMAP](../ROADMAP.md).

## Contexto e limite

Na ROM fixada por KNO-004, a compilação estática do commit local `aab31ae` parou em `0x080003B2`, raw `0x04C9`, após o caminho que retorna do SWI #1. A leitura dos campos classifica esse halfword como Thumb Format 1 `LSLS r1,r1,#19`; a semântica é corroborada por SRC-25. Isso é descoberta AOT sobre bytes da ROM, não trace de runtime nem execução do jogo. Este contrato implementa somente LSL imediato Thumb; não libera as formas LSR/ASR nem shift por registrador.

## Requisitos

- TSLI-01: o decoder MUST aceitar como LSL imediato somente encodings com `(encoding & 0xF800) == 0`. MUST extrair `imm5=bits[10:6]`, `Rs=bits[5:3]` e `Rd=bits[2:0]`. No raw `0x04C9`, os campos são `imm5=19`, `Rs=r1`, `Rd=r1`.
- TSLI-02: o resultado MUST ser `Rd = (Rs << imm5) mod 2^32`, lendo o valor antigo de `Rs`; alias `Rd==Rs` é válido.
- TSLI-03: N e Z MUST refletir o resultado. Para `imm5` entre 1 e 31, C MUST receber o bit `32-imm5` do valor antigo de `Rs`. Para `imm5=0`, C MUST permanecer inalterado. V MUST permanecer inalterado. CPSR.T, modo, bits de interrupção e demais registradores MUST permanecer inalterados.
- TSLI-04: a instrução MUST avançar PC em 2 bytes e incrementar `guest_cycle` em uma unidade abstrata. Isso não afirma equivalência de timing físico.
- TSLI-05: a GBA IR MUST representar a operação como `ThumbOperation::LslImmediate { rd, rs, amount }`, preservar guest PC e encoding e rejeitar instrução com PC desalinhado ou campos da operação divergentes do encoding. A versão identifica o contrato em memória e o valor publicado no manifesto; esta adição é compatível porque não muda a representação nem a semântica das operações existentes, então `GBA_IR_VERSION` permanece 1. Se uma futura alteração incompatível ou formato persistido for introduzido, sua compatibilidade MUST ser versionada.
- TSLI-06: o lowerer MUST emitir código nativo estritamente AOT. Nenhum caminho pode cair em interpretação, JIT ou fallback; um shift LLVM de 32 bits MUST tratar `imm5=0` explicitamente.
- TSLI-07: LSR/ASR imediato, shifts por registrador e outras famílias Thumb MUST continuar rejeitados durante descoberta, sem publicar pacote parcial.
- TSLI-08: a descoberta MUST propagar `Rd=(Rs << imm5) mod 2^32` quando Rs tem valor constante, e invalidar a constante de Rd quando Rs é desconhecido. Com `Rd==Rs`, a transferência MUST ler o valor de entrada antes de atualizar o destino, para que desvios indiretos seguintes continuem fechando estaticamente.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `imm5=0`, `r1=0x80000000`, C/V iniciais 0 | `r1` preservado; N=1, Z=0, C=0, V=0 |
| `imm5=0`, `r1=0`, C/V iniciais 1 | `r1=0`; N=0, Z=1, C=1, V=1 |
| `imm5=1`, `r1=0x80000001`, C inicial 0 | `r1=2`; N=0, Z=0, C=1; V preservado |
| `imm5=1`, `r1=0x40000000` | `r1=0x80000000`; N=1, Z=0, C=0 |
| `imm5=19`, `r1=0x00001000` | `r1=0x80000000`; N=1, Z=0, C=0; V preservado; PC+2 e CPSR.T ativo |
| `imm5=19`, `r1=0x00002000` | `r1=0`; N=0, Z=1, C=1 |
| `imm5=31`, `r1=2` | Produz zero; N=0, Z=1, C=1 |
| `imm5=31`, `r1=1` | Produz `0x80000000`; N=1, Z=0, C=0 |
| `Rd!=Rs` | Destino recebe o shift e registrador fonte mantém o valor antigo |
| IR com amount/Rs/Rd divergente do encoding | Verificador rejeita a operação com erro de encoding |
| Descoberta com Rs constante/desconhecido | Propaga o resultado constante ou invalida Rd, respectivamente |
| O0 e O2 | Mesmo estado guest observável |
| LSR/ASR imediato ou shift por registrador | Falha strict-AOT identificada no PC; nenhum executável ou pacote parcial |

## Fixture e verificação

Adicionar modos sintéticos à fixture Tiny Counter. Preparar valor e flags por instruções já suportadas, entrar em Thumb, testar os valores de borda acima e observar resultado, CPSR, PC e ciclo no runner. Testes de IR MUST cobrir campo adulterado, alinhamento e propagação constante; a suíte de interpreter MUST cobrir todos os vetores de flags. O gate GBA-020 MUST cobrir O0/O2 e confirmar que os negativos não deixam pacote parcial. Depois das fixtures, repetir a compilação estática LeafGreen de KNO-004 e registrar somente o próximo limite efetivamente alcançado. As fixtures são programas sintéticos e não afirmam equivalência diferencial com hardware.

## Evidência e referências

KNO-004 fixa a ROM em SHA-256 `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825` e registra a tentativa estática no commit `aab31ae`. SRC-25 reúne observações de fonte de mGBA (`143adf5aa2a60c6036810b0cd72445e13d74b0e6`) e gba-recomp (`de4edf59b872d887046d6a3b005e2df551b6d44c`). Elas corroboram decode e flags, mas não provam semântica em hardware nem execução da ROM. O runtime geral, ciclos de barramento, demais shifts e o próximo limite de descoberta permanecem fora deste contrato.
