# SPEC-052 — NEGS Thumb Format 4 por registrador em AOT estrito

Estado do documento: Accepted. Implementada no commit `bb94888cb7bc6dc6778dd43be804a86501155e95`. Testes, gate, hashes e nova fronteira estática: [evidência estruturada](../knowledge/evidence/gba020-thumb-negs-macos-arm64.json). Progresso: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

Após SPEC-051, a discovery estática O2 da ROM LeafGreen fixada em [KNO-004](../knowledge/KNO-004-rom-intake.md) parou no halfword raw `0x4248` em `0x08000BEA`. SRC-46 confirma, pelas revisões fixadas de mGBA e gba-recomp, que os campos inferem Thumb Format 4 `NEGS r0,r1`, equivalente a `RSBS r0,r1,#0`. Isso é classificação de encoding, não observação de registradores ou execução guest.

Esta tranche acrescenta somente a suboperação NEG do Thumb Format 4, com `Rd` e `Rs` limitados a r0–r7, ao decoder, GBA IR, discovery e LLVM strict-AOT. Ela calcula `Rd := 0 - old(Rs)` em aritmética u32, grava Rd e substitui NZCV. Não altera ABI de runtime, estado de máquina ou a versão da GBA IR.

## Fora de escopo

AND/EOR/LSL/LSR/ASR/ADC/SBC/ROR/TST/CMP/CMN/ORR/MUL/BIC/MVN além dos subsets já implementados; high registers ou PC; tracking geral de flags em discovery; timing físico; fallback interpretativo ou JIT.

## Termos e invariantes

- `Rd = encoding & 7`; `Rs = (encoding >> 3) & 7`.
- NEGS Thumb Format 4 tem suboperação `1001` e encoding `(encoding & 0xFFC0) == 0x4240`.
- `Rd == Rs` é permitido; o operando é lido antes de gravar o destino.
- `result = 0u32.wrapping_sub(old(Rs))`.
- N é o bit 31 do resultado; Z é 1 quando o resultado é zero; C é 1 quando não há borrow (equivalente a `old(Rs)==0`); V é 1 somente quando `old(Rs)==0x80000000`.
- A operação substitui NZCV e preserva todos os outros bits do CPSR.
- Um passo aceito avança o PC guest em dois bytes e `guest_cycle` em uma unidade abstrata; isso não modela waitstates ou prefetch físico.
- Registradores e endereços guest são inteiros, nunca ponteiros host.

## Requisitos normativos

- TNG-01: decoder MUST aceitar somente `(encoding & 0xFFC0) == 0x4240`, extrair Rd/Rs dos campos low-register e rejeitar PC Thumb desalinhado. Outras suboperações Format 4 continuam rejeitadas, exceto ANDS e ORRS especificados separadamente.
- TNG-02: `ThumbOperation::NegateRegister { rd, rs }` MUST conservar PC/encoding e somente registradores r0–r7. O verificador MUST conferir suboperação/campos contra o encoding e exigir a instrução sequencial em `PC+2`; campos adulterados, modo/operação incompatível ou sucessor ausente MUST falhar.
- TNG-03: semântica guest MUST ler o valor antigo de Rs, calcular subtração u32 modular `0 - old(Rs)`, gravar somente Rd, substituir N/Z/C/V pelas regras acima, preservar os outros bits de CPSR e deixar memória/estado de hardware intactos. Alias é permitido.
- TNG-04: discovery MUST propagar `Rd` pela subtração zero menos cada constante conhecida de Rs, se o conjunto de resultados estiver dentro do domínio limitado existente. Se Rs for desconhecido ou a expansão distinta exceder 16 valores, somente Rd torna-se Unknown, sem truncar ou escolher valor heurístico. Discovery não acompanha NZCV de NEG e MUST preservar ambos sucessores de branch condicional subsequente quando as flags não tiverem outra prova independente. NEG não pode deixar uma prova `pending_compare` anterior valer para além desta instrução.
- TNG-05: LLVM MUST emitir aritmética nativa e implementar NZCV conforme o contrato, preservando o restante do CPSR. A execução é strict-AOT, sem helper de memória, interpretação, JIT ou fallback.
- TNG-06: cada NEGS aceito MUST avançar `guest_cycle` em uma unidade abstrata; não reivindica timing físico.
- TNG-07: `GBA_IR_VERSION`, ABI/runtime e versão do estado de máquina MUST permanecer inalterados. Formatos/suboperações alcançáveis fora do contrato MUST falhar antes de publicar pacote parcial.

## Interfaces, entradas e saídas

Adicionar `ThumbOperation::NegateRegister { rd, rs }`, sem generalizar o enum ALU Format 4 e sem criar interface de runtime. Discovery usa operações do domínio limitado existente; máximo de 16 constantes por valor.

## Erros, limites e observabilidade

Encoding fora da suboperação aceita resulta em `unsupported_thumb_instruction` no PC guest. IR inconsistente falha no verificador. Resultados observáveis são Rd, NZCV, preservação dos demais bits CPSR, PC e `guest_cycle`. Discovery sem valor conhecido de Rs mantém o valor destino desconhecido e não escolhe edge de branch com base em flags de NEG.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| `0x4240`–`0x427F` | Decoder cobre as 64 combinações de r0–r7 como NEGS |
| Suboperações Format 4 vizinhas | Continuam rejeitadas, exceto ANDS/ORRS já cobertos |
| IR com encoding/campos divergentes ou sem `PC+2` | Verificador rejeita |
| Rs=0 | Resultado zero, Z/C=1, N/V=0 |
| Rs=1, `0x7FFFFFFF`, `0x80000000`, `0xFFFFFFFF` | Resultado e NZCV correspondem à subtração; Rs=`0xFFFFFFFF` produz 1 com N/Z/C/V zerados; V só no caso `0x80000000` |
| `Rd==Rs` e bits CPSR não relacionados | Operando antigo é usado e bits fora NZCV são preservados |
| Discovery com constantes limitadas | Propaga exatamente `0-Rs` |
| Discovery com Unknown ou expansão acima do limite | Só Rd torna-se Unknown, sem truncamento |
| CMP → NEGS → branch condicional | Discovery invalida a prova CMP não adjacente, não presume NZCV de NEG e conserva ambos edges |
| Fixtures strict-AOT O0/O2 | Verificam zero, não zero, sinal/overflow, branch e manifesto strict-AOT |
| Alvo alcançável fora do subset | Falha sem pacote parcial |
| LeafGreen fixada | Registra a nova fronteira estática ou a emissão realmente alcançada, sem afirmar execução do título |

Executar testes de decoder/verificador, semântica da IR, discovery, LLVM, fixtures O0/O2 e rejeições; rodar `cargo test --locked --workspace --jobs 10` e `scripts/verify-gba020.sh` sob o orçamento do host. Repetir a tentativa LeafGreen O2 para a ROM de KNO-004 e salvar log/hash/resultado.

Fixtures verificam somente os estados configurados. Avançar discovery de uma ROM comercial não prova seus caminhos dinâmicos, emissão, link ou compatibilidade do jogo.

## Questões abertas

Depois de NEGS, a próxima fronteira precisa ser descoberta novamente. Esta tranche não presume o opcode ou o estado de hardware seguinte.

## Referências e evidências

- [SPEC-002](SPEC-002-armv4t-semantics.md), [SPEC-004](SPEC-004-ir-llvm.md), [SPEC-030](SPEC-030-thumb-orr-register.md) e [SPEC-051](SPEC-051-thumb-and-register.md).
- [SRC-46](../knowledge/KNO-001-evidence.md) registra as revisões e limites da inferência.
- [ADR-037](../adr/ADR-037-thumb-negate-register.md).
- [Tentativa LeafGreen após ANDS](../knowledge/evidence/gba020-thumb-ands-macos-arm64.json).
