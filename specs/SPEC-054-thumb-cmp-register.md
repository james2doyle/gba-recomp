# SPEC-054 — CMP Thumb Format 4 por registrador em AOT estrito

Estado do documento: Accepted. Implementação `e8d855c7186667b0a23693a39150bcb8bfe25adc`; progresso e evidência: [ROADMAP](../ROADMAP.md) e [relatório macOS arm64](../knowledge/evidence/gba020-thumb-cmp-register-macos-arm64.json).

## Problema, objetivos e escopo

Após SPEC-053, a discovery estática O2 da LeafGreen fixada em [KNO-004](../knowledge/KNO-004-rom-intake.md) parou no halfword `0x428A`, em `0x08000C00`. SRC-47 classifica o encoding como Thumb Format 4 `CMP r2,r1`, com base nos campos e nas fontes fixadas de mGBA e gba-recomp. Isso não fornece valores observados dos registradores nem demonstra execução da ROM.

Acrescentar somente CMP entre registradores baixos do Thumb Format 4 ao decoder, GBA IR, verificador, discovery, stepper de referência e LLVM strict-AOT. CMP calcula `old(Rd)-old(Rs)`, atualiza NZCV e não grava registrador. A discovery pode usar o resultado da comparação somente se a instrução condicional vier imediatamente após CMP e ambos operandos forem conhecidos.

## Fora de escopo

TST, CMN, EOR e outras suboperações Format 4; CMP high-register e demais formatos; tracking geral de flags; mudança de ABI/runtime ou `GBA_IR_VERSION`; timing físico; interpretação, JIT ou fallback.

## Termos e invariantes

- `Rd = encoding & 7`; `Rs = (encoding >> 3) & 7`.
- CMP Format 4 é exatamente `(encoding & 0xFFC0) == 0x4280`, com `Rd` e `Rs` em r0–r7. O encoding `0x428A` é `CMP r2,r1`.
- `result = old(Rd).wrapping_sub(old(Rs))`; N é o bit 31 do resultado, Z indica resultado zero, C indica ausência de borrow (`old(Rd) >= old(Rs)`) e V é `((old(Rd) ^ old(Rs)) & (old(Rd) ^ result) & 0x80000000) != 0`.
- CMP não altera nenhum registrador geral nem memória. Substitui NZCV e preserva os demais bits do CPSR.
- Um passo aceito avança o PC Thumb em dois bytes e soma um ciclo guest abstrato; isso não modela waitstates ou prefetch.
- `Rd == Rs` é permitido: ambos valores são lidos antes de qualquer atualização observável.
- Valores e endereços guest permanecem inteiros e não se tornam ponteiros host.

## Requisitos normativos

- TCR-01: decoder MUST aceitar somente `(encoding & 0xFFC0) == 0x4280`, extrair os campos baixos `Rd` e `Rs` e rejeitar PC Thumb ímpar. Suboperações vizinhas não implementadas MUST continuar rejeitadas.
- TCR-02: `ThumbOperation::CompareRegister { rd, rs }` MUST conservar PC e encoding. O verificador MUST limitar ambos operandos a r0–r7, conferir máscara/campos e exigir sucessor sequencial em `PC+2`; inconsistências MUST falhar.
- TCR-03: o stepper de referência MUST calcular `old(Rd)-old(Rs)` em u32 modular, atualizar NZCV pelas regras acima, preservar todos os outros bits CPSR e todos os registradores, avançar PC em dois bytes e não alterar memória.
- TCR-04: discovery MUST conservar uma prova CMP-register somente para a próxima instrução sequencial Thumb. Um branch condicional adjacente pode ser refinado quando os valores de ambos registradores forem conhecidos no estado completo atual. Se `Rd == Rs`, a comparação é sempre zero e pode ser refinada mesmo que o registrador seja Unknown. Nos demais casos, se algum valor for Unknown, MUST manter ambos edges. A prova MUST ser descartada em qualquer instrução interveniente ou mudança de modo. Comparações de conjuntos finitos MUST considerar todas as combinações possíveis sem excluir caminhos.
- TCR-05: LLVM MUST emitir subtração e flags em código nativo, sem writeback de Rd, preservando os bits CPSR não pertencentes a NZCV. A operação MUST avançar PC/ciclo e permanecer strict-AOT sem helper, interpretação, JIT ou fallback.
- TCR-06: `GBA_IR_VERSION`, ABI/runtime e versão do estado da máquina MUST permanecer inalterados. Qualquer instrução alcançável fora do subset MUST falhar antes de publicar pacote parcial.

## Interfaces, entradas e saídas

Adicionar a variante específica `ThumbOperation::CompareRegister { rd, rs }`; não criar uma ALU genérica. Ampliar o certificado interno de comparação para representar RHS imediato ou registrador, mantendo o refinamento existente de CMP imediato. Não adicionar campos à ABI de runtime.

## Erros, limites e observabilidade

Encoding fora do subset resulta em `unsupported_thumb_instruction` com PC guest. IR inconsistente falha no verificador. Resultados observáveis: preservação de todos os GPRs, NZCV, demais bits CPSR, PC e ciclo guest. Sem valores abstratos conhecidos, discovery não escolhe um sucessor com base em CMP.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| `0x4280`–`0x42BF` | Os 64 encodings decodificam com os campos corretos; `0x428A` resulta em `rd=2, rs=1` |
| `0x4200`, `0x4240`, `0x42C0` e vizinhos | TST/CMN seguem rejeitados, NEGS mantém seu contrato e nenhuma suboperação extra passa |
| PC ímpar, campos/encoding divergentes, registrador inválido ou ausência de `PC+2` | Decode/verificador rejeitam |
| `5-5`, `0-1`, `0x80000000-1`, `0x7FFFFFFF-0xFFFFFFFF` | NZCV respectivamente `0x60000000`, `0x80000000`, `0x30000000`, `0x90000000` |
| `Rd == Rs` e bits CPSR não relacionados | NZCV indica zero/sem borrow/sem overflow; todos GPRs e bits CPSR não relacionados permanecem intactos |
| CMP register seguido diretamente por Bcond | Discovery escolhe edge exato para pares constantes, inclusive EQ/NE e condições assinadas/unsigned |
| CMP `Rd,Rd` seguido por Bcond com `Rd` Unknown | Discovery prova igualdade sem tratar as duas leituras do mesmo registrador como independentes |
| Um registrador Unknown ou instrução entre CMP e Bcond | Ambos edges permanecem; prova CMP não atravessa a instrução intermediária |
| Fixtures strict-AOT O0/O2 | Executam CMP sem writeback e branches adjacentes com ambos resultados; manifesto identifica strict-AOT |
| Instrução alcançável fora do subset | Falha antes de publicar pacote parcial |
| LeafGreen fixada | Registrar a nova fronteira estática ou uma etapa posterior realmente alcançada; não inferir execução do título |

Executar testes de decoder/verificador, stepper, discovery e LLVM; fixtures O0/O2 e rejeições; `cargo test --locked --workspace --jobs 10` e `scripts/verify-gba020.sh` no limite de CPU configurado. Repetir discovery estática O2 da ROM fixada e guardar log, hash e resultado.

Fixtures cobrem somente os estados configurados. Avanço da discovery não prova que um branch é tomado em runtime, que o jogo foi emitido/linkado ou que é compatível.

## Resultado da implementação

O CMP register foi implementado no decoder, GBA IR/verificador, executor de referência, discovery e lowering LLVM strict-AOT. `cargo test --locked --workspace --jobs 10 --quiet` passou com 76 testes em `gba-ir` e 121 em `gba-recomp`. `scripts/verify-gba020.sh` passou em Darwin/arm64; as três fixtures O0/O2 produziram estados iguais, com 11 instruções AOT cada, e CMN `0x42D1` foi rejeitado sem pacote parcial. ABI v4 e IR v2 permaneceram inalteradas.

A nova tentativa LeafGreen O2 parou em `bx_target_unresolved` para `BX r0` (`0x4700`) em `0x080006E4`, antes de emissão LLVM/link; não gerou pacote e não executou a ROM. Trata-se de discovery estática e não prova que o CMP ou esse BX sejam tomados em runtime. Logs, hashes, estados e limites estão no [relatório](../knowledge/evidence/gba020-thumb-cmp-register-macos-arm64.json).

## Questões abertas

Esta tranche não implementa as demais suboperações do Format 4, tracking geral de flags ou o restante do hardware necessário para LeafGreen.

## Referências e evidências

- [SPEC-002](SPEC-002-armv4t-semantics.md), [SPEC-004](SPEC-004-ir-llvm.md), [SPEC-032](SPEC-032-thumb-conditional-branch.md), [SPEC-051](SPEC-051-thumb-and-register.md) e [SPEC-053](SPEC-053-thumb-asr-immediate.md).
- [SRC-47](../knowledge/KNO-001-evidence.md) registra revisão upstream, campos inferidos e limites da evidência.
- [KNO-006](../knowledge/KNO-006-vendor-gba-recomp.md) descreve a cobertura vendor e os limites do build híbrido.
- [ADR-039](../adr/ADR-039-thumb-cmp-register.md).
