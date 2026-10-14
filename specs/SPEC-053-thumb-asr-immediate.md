# SPEC-053 — ASR Thumb imediato Format 1 em AOT estrito

Estado do documento: Draft. Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-052, a discovery estática O2 da ROM LeafGreen fixada em [KNO-004](../knowledge/KNO-004-rom-intake.md) chegou ao halfword raw `0x17C1` em `0x08000BEE` e parou com `unsupported_thumb_instruction`. SRC-39 infere Thumb Format 1 `ASRS r1,r0,#31`, corroborado pelos decoders/handlers nas revisões fixadas de mGBA e gba-recomp. Não há valores de `r0`/flags observados nem execução guest.

Adicionar somente ASR imediato Thumb T1 ao decoder, GBA IR, descoberta, interpretador de referência e LLVM strict-AOT. A operação é de CPU, sem acesso a memória guest ou dependência de ABI/runtime auxiliar.

## Fora de escopo

- ASR por registrador, shifts restantes, outras operações Format 1 e demais suboperações não implementadas.
- Mudança da ABI, estado de runtime, GBA IR v2, manifesto ou contrato de timing físico.
- Interpretação/JIT/fallback e alegações de suporte ou compatibilidade com LeafGreen.

## Invariantes

- Thumb Format 1 ASR: `(encoding & 0xF800) == 0x1000`; `amount=bits[10:6]`, `Rs=bits[5:3]`, `Rd=bits[2:0]`; todos os registradores são r0–r7.
- Para `amount` entre 1 e 31, `result = (int32_t)old(Rs) >> amount`; `C = old(Rs)[amount-1]`.
- `amount==0` codifica ASR de 32: resultado é `0xFFFFFFFF` se o bit 31 antigo de Rs for 1, senão zero; C recebe o bit 31 antigo de Rs.
- A operação grava Rd usando o valor anterior de Rs, atualiza N/Z/C a partir do resultado e carry definidos, preserva V e todos os demais bits CPSR, avança o PC Thumb em dois bytes e soma um ciclo abstrato. `Rd==Rs` é permitido.
- Discovery aplica o shift aritmético exato aos valores constantes finitos de Rs, mantém Unknown como Unknown e não acompanha as flags produzidas por ASR. Branch condicional após ASR conserva ambos edges quando não houver outra prova independente.
- Endereços guest e registradores guest permanecem inteiros; LLVM e APIs host não entram na ABI GBA.

## Requisitos normativos

- TAS-01: decoder MUST aceitar somente `(encoding & 0xF800)==0x1000`, extrair `Rd`, `Rs` e `amount` e rejeitar PC Thumb desalinhado. Outros formatos e encodings vizinhos continuam rejeitados.
- TAS-02: `ThumbOperation::AsrImmediate { rd, rs, amount }` MUST conservar PC/encoding; o verificador MUST conferir máscara e campos, limitar registradores a r0–r7, amount a 0–31 e exigir sucessor sequencial `PC+2`.
- TAS-03: single-step MUST implementar a semântica de resultado/carry para amount 0–31, atualizar somente N/Z/C e os registradores definidos, preservar V/controle e ler a fonte antes de gravar Rd.
- TAS-04: discovery MUST propagar cada valor conhecido por ASR exato, sem inventar valores quando a fonte é Unknown; deve invalidar prova de compare anterior e conservar ambos edges condicionais quando ASR produz as flags relevantes.
- TAS-05: LLVM MUST emitir o shift nativo e N/Z/C corretos. O amount zero precisa evitar shift de largura igual a 32 e qualquer valor LLVM poison/indefinido. O lowering MUST preservar V/controle, avançar PC/ciclo e permanecer strict-AOT sem helper ou fallback.
- TAS-06: `GBA_IR_VERSION`, ABI/runtime e versão do estado da máquina MUST permanecer inalterados.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| Todas as combinações `(Rd,Rs,amount)` de `0x1000`–`0x17FF` em PC Thumb alinhado | 2.048 encodings decodificam com os campos corretos |
| PC Thumb ímpar e encodings vizinhos | PC ímpar rejeita; LSL/LSR mantêm contratos existentes e `0x1800+` permanece classificado por seus próprios formatos |
| Amount 0 | Resultado all-zero/all-one pelo sinal da fonte e C igual ao sinal original |
| Amounts 1 e 16 | ASR aritmético e carry refletem corretamente fontes com bit de saída 0 e 1 |
| Amount 31 com fontes `0`, `0x40000000`, `0x80000000`, `0xC0000000` | Testa os quatro pares independentes de sinal e carry; resultado/N/Z/C corretos |
| Amount 0, fontes com bit 31 0/1 | Shift 32, preenchimento de sinal e C do sinal corretos |
| C inicial 0/1, V inicial 0/1, fontes zero/positivas/negativas, `Rd==Rs` | C novo substitui o anterior; V e CPSR não relacionado preservados; alias lê o valor antigo |
| IR com mask/campos divergentes, registrador inválido ou sem `PC+2` | Verificador rejeita |
| Discovery com valores finitos, Unknown e CMP→ASR→Bcond | Resultados exatos; prova CMP anterior descartada e máscara do branch conserva ambos sucessores (`0b11`) |
| Fixtures strict-AOT O0/O2 | Executam e observam branches baseados em N/Z/C após ASR (com amount 0 e 31 e ambos sinais); estados iguais e manifestos strict-AOT; sem helper/runtime novo |
| Alvo alcançável fora do subset | Falha antes de publicar pacote parcial |
| LeafGreen fixada | Registrar nova fronteira estática ou emissão realmente alcançada, sem alegar execução do título |

Executar testes de decoder/verificador, semântica IR, discovery, LLVM e fixtures O0/O2; rodar suíte workspace e `scripts/verify-gba020.sh` usando o teto de 10 slots do Mac mini M4. Repetir discovery estática LeafGreen após os gates.

Fixtures demonstram somente os estados configurados. Uma fronteira estática posterior não é trace dinâmico nem prova de caminho tomado.

## Questões abertas

A discovery não interpreta flags de shifts como prova para branches. O ASR por registrador, a família de shifts completa e a adequação do runner para LeafGreen exigem contratos separados.

## Referências e evidências

- [SPEC-002](SPEC-002-armv4t-semantics.md), [SPEC-004](SPEC-004-ir-llvm.md), [SPEC-024](SPEC-024-thumb-lsl-immediate.md) e [SPEC-041](SPEC-041-thumb-lsr-immediate.md).
- [SRC-39](../knowledge/KNO-001-evidence.md) registra as revisões fixadas e os limites da inferência `ASRS r1,r0,#31`.
- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa a identidade/hash da ROM e a tentativa após NEGS.
- [ADR-038](../adr/ADR-038-thumb-asr-immediate.md).
