# SPEC-051 — ANDS Thumb Format 4 por registrador em AOT estrito

Estado do documento: Draft. Especificação escrita antes da implementação. Progresso: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

Após SPEC-050, a compilação estática O2 da ROM LeafGreen fixada em [KNO-004](../knowledge/KNO-004-rom-intake.md) passou pela fronteira `0xE00A` em `0x08000AE6` e parou no halfword `0x4002` em `0x08000BE4`. A leitura dos campos como `ANDS r2,r0` é uma inferência registrada por SRC-45, corroborada por fontes fixadas de mGBA e gba-recomp. O log demonstra apenas descoberta estática; não contém registradores guest observados nem execução do título.

Esta tranche acrescenta somente a suboperação AND do Thumb Format 4, com `Rd` e `Rs` limitados a r0–r7, ao decoder, GBA IR, discovery e LLVM strict-AOT. Ela calcula `Rd := old(Rd) AND old(Rs)`, atualiza N/Z e preserva C/V. Não altera ABI de runtime, estado de máquina ou a versão da GBA IR.

## Fora de escopo

EOR, shifts por registrador, ADC, SBC, ROR, TST, NEG, CMP, CMN, MUL, BIC e MVN do Format 4; demais formatos ALU; registradores high ou PC; tracking geral de flags em discovery; timing físico; fallback interpretativo ou JIT.

## Termos e invariantes

- `Rd = encoding & 7`; `Rs = (encoding >> 3) & 7`.
- ANDS Thumb Format 4 tem suboperação zero e encoding `(encoding & 0xFFC0) == 0x4000`; a atualização de flags é implícita.
- Operandos lidos com `Rd == Rs` vêm do estado anterior à instrução.
- Um passo aceito avança o PC guest em dois bytes e `guest_cycle` em uma unidade abstrata; isso não modela waitstates ou prefetch físico.
- O estado guest usa valores u32. Endereços guest e registradores guest nunca são ponteiros host.

## Requisitos normativos

- TAN-01: decoder MUST aceitar somente `(encoding & 0xFFC0) == 0x4000`, extrair `Rd` e `Rs` dos três bits baixos de seus campos e rejeitar PC Thumb desalinhado. Outras suboperações Format 4 continuam rejeitadas, exceto ORR já especificado em SPEC-030.
- TAN-02: `ThumbOperation::AndRegister { rd, rs }` MUST conservar PC/encoding e somente registradores r0–r7. O verificador MUST conferir a suboperação e os campos contra o encoding e exigir a instrução sequencial em `PC+2`; campos adulterados, modo/operação incompatível ou sucessor ausente MUST falhar.
- TAN-03: semântica guest MUST ler ambos os valores antigos, calcular `result = old(Rd) & old(Rs)`, escrever somente `Rd`, atualizar N pelo bit 31 e Z quando `result == 0`, preservar C/V e todos os outros bits de CPSR e deixar memória/estado de hardware intactos. Alias é permitido.
- TAN-04: discovery MUST propagar `Rd` por AND exato quando os operandos forem constantes limitadas dentro do domínio abstrato existente. Se algum operando for desconhecido ou o produto distinto exceder 16 valores, somente `Rd` torna-se Unknown; não pode truncar nem escolher valor heurístico. Discovery não acompanha N/Z produzidos pelo AND e MUST preservar os dois sucessores de um branch condicional subsequente, salvo outra prova independente válida para aquele branch. A operação não pode deixar uma prova `pending_compare` anterior ser aplicada além da instrução comparativa adjacente.
- TAN-05: LLVM MUST emitir AND nativo e atualizar somente N/Z, preservando C/V e os bits restantes de CPSR. A execução é strict-AOT, sem helper de memória, interpretação, JIT ou fallback.
- TAN-06: cada ANDS aceito MUST avançar `guest_cycle` em uma unidade abstrata. Isso não reivindica timing de hardware.
- TAN-07: `GBA_IR_VERSION`, ABI/runtime e versão do estado de máquina MUST permanecer inalterados. Formatos/suboperações alcançáveis fora do contrato MUST falhar antes de publicar pacote parcial.

## Interfaces, entradas e saídas

Adicionar uma variante específica `ThumbOperation::AndRegister { rd, rs }`, sem enum genérico para ALU Format 4 e sem nova interface de runtime. A discovery usa as operações de conjunto limitado existentes para calcular AND e mantém o teto de 16 constantes por valor.

## Erros, limites e observabilidade

Encoding fora da suboperação aceita resulta em `unsupported_thumb_instruction` no PC guest. IR inconsistente falha no verificador. Resultados observáveis são `Rd`, flags N/Z, preservação de C/V e demais CPSR, PC e `guest_cycle`. Discovery sem valores suficientes continua conservadora e mantém edges condicionais indeterminados.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| `0x4000`–`0x403F` | Decoder cobre as 64 combinações de r0–r7 como ANDS |
| Suboperações vizinhas do Format 4 | Continuam rejeitadas, exceto ORR já coberto por SPEC-030 |
| IR com encoding/campos divergentes ou sem `PC+2` | Verificador rejeita |
| Resultado zero, positivo e com bit 31 | N/Z correspondem ao resultado e destino correto é gravado |
| C/V e bits não relacionados de CPSR | Preservados nos casos zero/sinalizado e em alias `Rd == Rs` |
| Discovery com operandos conhecidos | Calcula todos os resultados AND dentro do limite de 16 |
| Discovery com Unknown ou expansão acima do limite | Só `Rd` torna-se Unknown sem truncamento |
| ANDS seguido por branch condicional | Discovery não presume N/Z; ambos edges são mantidos quando a condição não está provada independentemente |
| Fixtures strict-AOT O0/O2 | Executam ANDS e branch condicional com resultados/flags observáveis e manifesto strict-AOT |
| Alvo alcançável fora do subset | Falha sem pacote parcial |
| LeafGreen fixada | Registra a próxima fronteira estática ou emissão realmente alcançada; não afirma execução do título |

Executar testes de decoder/verificador, semântica da IR, discovery, LLVM, fixtures O0/O2 e rejeições; rodar `cargo test --locked --workspace --jobs 10` e `scripts/verify-gba020.sh` respeitando o orçamento de CPU do host. Repetir a tentativa LeafGreen O2 usando a ROM de KNO-004 e salvar log/hash/resultado.

Fixtures sintéticas verificam somente os estados que configuram. Avançar discovery na ROM comercial não demonstra caminho dinâmico, emissão, link ou compatibilidade do jogo.

## Questões abertas

Depois de ANDS, a próxima instrução alcançável precisa ser descoberta novamente. Esta tranche não presume que o próximo limite seja uma instrução ou estado de hardware específico.

## Referências e evidências

- [SPEC-002](SPEC-002-armv4t-semantics.md), [SPEC-004](SPEC-004-ir-llvm.md) e [SPEC-030](SPEC-030-thumb-orr-register.md).
- [SRC-45](../knowledge/KNO-001-evidence.md) registra as fontes upstream e limites de inferência.
- [SPEC-050](SPEC-050-thumb-unconditional-branch.md) e [ADR-036](../adr/ADR-036-thumb-and-register.md).
- [Tentativa LeafGreen após Thumb B](../knowledge/evidence/leafgreen-after-thumb-b-20261014-O2.json).
