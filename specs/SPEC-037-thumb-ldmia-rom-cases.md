# SPEC-037 — Thumb LDMIA de ROM com conjunto finito de bases

Estado do documento: Accepted. Implementação pendente. Progresso no [ROADMAP](../ROADMAP.md); decisão arquitetural em [ADR-024](../adr/ADR-024-finite-rom-ldmia-specialization.md).

## Problema e objetivo

Depois da SPEC-036, a descoberta da LeafGreen volta ao Thumb `LDMIA r3!,{r0}` em `0x080006A8`. O estado abstrato observado no primeiro merge contém duas bases ROM; a análise atual não refina o backedge a partir de `CMP r1,#0; BGE` e não pode materializar um único valor ROM.

Esta tranche permite LDMIA ROM quando a análise prova um conjunto finito de no máximo 16 bases guest e especializa cada base em código AOT. Para fechar loops pequenos sem supor um caminho, a descoberta particiona valores somente quando uma comparação Thumb `CMP` imediata é seguida diretamente por branch condicional. Isso é refinamento de alcançabilidade; não altera flags guest nem a condição avaliada em runtime.

Não implementa bus ROM dinâmico, não inclui a ROM inteira no pacote e não declara compatibilidade ou execução de LeafGreen.

## Fora de escopo

- Bases ROM desconhecidas, conjuntos que excedam 16 valores ou conjuntos que misturem ROM e regiões não ROM para a mesma instrução.
- Acesso runtime dinâmico a ROM, cópia da imagem completa, ponteiros guest/host em ABI ou helper interpretativo.
- Refinamento geral de NZCV, branches ARM condicionais ou CMP separado do branch por outra instrução.
- Conjuntos ROM acima do limite, LDMIA fora de ROM/EWRAM/IWRAM físicas, outros modos de transferência, timing GBA e fidelidade de Data Abort.
- Execução, compatibilidade ou suporte declarado para a LeafGreen.

## Termos e invariantes

- `base_set`: conjunto de valores u32 para Rb no ponto de LDMIA após o fixed point da descoberta, agregado das alternativas de estado completas.
- `case`: par formado pelo valor guest exato de Rb e as words lidas para os registradores selecionados. Endereços efetivos usam `EA_i=(base+4*i) & !3`, como SPEC-035.
- `CMP imediato + branch direto`: `CompareImmediate` em `pc-2`, seguido em `pc` por `ConditionalBranch`, sem instrução entre eles. O fato abstrato identifica PC, registrador e imediato da comparação e só sobrevive até esse branch.
- A descoberta guarda alternativas de estados completos por `(modo, PC)` desde a semente. Alternativas idênticas são deduplicadas; nunca há merge componente a componente de registradores ou memória. Qualquer conjunto finito de constantes dentro de um estado é expandido em alternativas singleton antes de entrar no worklist.
- O limite de precisão é 64 alternativas completas por `(modo, PC)` e 100.000 estados completos únicos no worklist global; exceder qualquer budget rejeita a análise sem merge ou fallback. Cada estado mantém no máximo 64 palavras de sombra, limitando o tamanho de cada fato retido. Esses budgets são independentes do limite de 16 bases ROM por tabela.
- Para `CMP`, `result = lhs.wrapping_sub(immediate)`, `N=result[31]`, `Z=(result==0)`, `C=(lhs>=immediate)`, `V=(((lhs^immediate)&(lhs^result)&0x80000000)!=0)`. As condições 0..13 usam as fórmulas ARM usuais (`EQ:Z`, `NE:!Z`, `CS:C`, `CC:!C`, `MI:N`, `PL:!N`, `VS:V`, `VC:!V`, `HI:C&&!Z`, `LS:!C||Z`, `GE:N==V`, `LT:N!=V`, `GT:!Z&&N==V`, `LE:Z||N!=V`).
- Cada alternativa é refinada separadamente: condições ARM 0..13 são avaliadas sobre o NZCV que Thumb CMP imediato produziria para cada valor conhecido. Estado sem CMP adjacente correspondente ou com operando desconhecido segue ambos os sucessores. O código AOT ainda calcula a condição com CPSR guest em runtime.
- A descoberta mantém, por PC de LDMIA, se algum estado observado já continha base ROM. Após o fixed point, se esse PC também recebeu qualquer base `Unknown` ou não ROM, a compilação rejeita, independentemente da ordem do worklist. `Unknown` sem candidato ROM conserva o helper dinâmico com validação runtime de região da SPEC-035; não declara ROM suportada.
- O programa GBA IR passa à versão 2 porque acrescenta tabelas de especialização e máscara de sucessores condicionais provados.

## Requisitos normativos

- LDMR-01: o decoder MUST preservar o encoding Thumb T1 LDMIA e os requisitos de lista não vazia da SPEC-035. O verificador MUST conferir base/lista e rejeitar índices de tabela fora do intervalo.
- LDMR-02: a descoberta MUST conservar o caminho singleton da SPEC-035. Se `base_set` contém de 2 a 16 valores e todos estão na janela ROM, MUST criar uma tabela ordenada por `base_address`, com entradas únicas. Cada word selecionada MUST ser lida da imagem intake cujo SHA-256 está fixado; qualquer word fora dos bytes da imagem MUST rejeitar a compilação sem publicar pacote.
- LDMR-03: cada tabela MUST carregar exatamente uma entrada para cada base possível do fixed point. A IR MUST vincular a operação a um table ID e conferir que o número de casos é 2..=16, as bases são únicas/ordenadas e as palavras não selecionadas estão zeradas. A tabela contém somente words necessárias à instrução, nunca bytes ou ponteiros para a ROM completa.
- LDMR-04: se `base_set` contém somente EWRAM/IWRAM físicas, a forma dinâmica da SPEC-035 permanece permitida. Um conjunto contendo bases ROM e não ROM MUST ser rejeitado explicitamente. Endereços guest nunca são ponteiros host.
- LDMR-05: a descoberta MUST conservar estados completos disjuntivos desde a semente, antes de qualquer merge que possa perder correlação entre registradores, flags pendentes e sombra de memória. Instrução ARM predicada sem refinamento de NZCV MUST enviar estado executado e estado não executado como alternativas separadas. Igualdade MUST incluir modo/PC, registradores, memória e fato CMP. Conjuntos finitos dentro de registradores ou memória MUST ser expandidos em alternativas singleton antes do enqueue. Estados idênticos podem ser deduplicados. Ao exceder 64 alternativas completas em qualquer `(modo, PC)` ou 100.000 estados globais, discovery MUST rejeitar com diagnóstico de budget, sem merge, `Unknown` ou fallback.
- LDMR-06: a descoberta MUST acompanhar um fato `CompareImmediate` somente até a instrução Thumb imediatamente seguinte. O fato inclui PC, registrador e imediato. Para cada alternativa separadamente, se o sucessor é branch condicional e o operando comparado possui valor conhecido, MUST calcular `result=lhs.wrapping_sub(immediate)`, `N=result[31]`, `Z=(result==0)`, `C=(lhs>=immediate)` e `V=(((lhs^immediate)&(lhs^result)&0x80000000)!=0)`, e aplicar a condição ARM 0..13 exata. MUST enfileirar apenas o edge tomado ou fallthrough correspondente. Alternativa sem o fato adjacente correspondente ou com operando `Unknown` MUST enfileirar ambos os edges. Comparação não adjacente, outros produtores de flags e análise ARM MUST manter sucessores conservadores. O teste do branch no lowering MUST continuar usando CPSR guest em runtime.
- LDMR-07: para cada PC de LDMIA, a descoberta MUST lembrar sticky se qualquer alternativa observada continha base ROM. Depois do fixed point, se tal PC tiver qualquer alternativa com base `Unknown` ou não ROM, a compilação MUST falhar, mesmo que essa alternativa tenha chegado antes da primeira base ROM. `Unknown` sem candidato ROM segue a forma dinâmica SPEC-035, cujo helper valida EWRAM/IWRAM e gera fault explícito para ROM ou outra região; isso não é suporte AOT a ROM desconhecida.
- LDMR-08: o IR MUST registrar máscara não vazia de sucessores `taken`/`fallthrough`, resolvida somente depois da convergência do fixed point como união dos edges alcançados por todas as alternativas. Bit 0 é taken e bit 1 é fallthrough; outros bits MUST ser rejeitados. O verificador MUST conferir cada sucessor marcado. A máscara é um certificado interno confiável da descoberta, que não aceita IR externo/serializado; o lowering MUST continuar avaliando CPSR guest em runtime e uma rota não marcada inesperada termina no dispatcher AOT ausente. Testes MUST conferir a máscara após convergência, alternativas com CMP e ingresso alternativo sem CMP.
- LDMR-09: para a operação multi-base, o lowering LLVM MUST despachar pelo valor runtime guest de Rb para a constante AOT correspondente. Cada caso MUST associar em ordem crescente as words aos registradores selecionados, usando endereços alinhados da SPEC-035. Em caso válido, MUST preservar CPSR, atualizar os registradores da lista segundo a tabela e aplicar writeback `old_Rb + 4*popcount(list)` quando Rb não pertence à lista; quando pertence, a word da tabela prevalece. PC avança a `PC+2` e ciclo local em 1, igual à SPEC-035.
- LDMR-10: base runtime não presente na tabela MUST retornar `EXIT_FAULT` no PC da instrução, reportar a base em `fault_address` e `GBA_MEMORY_FAULT_ROM_SPECIALIZATION_V3` em `fault_code`, sem alterar registradores, PC, CPSR ou ciclo. A falha é explícita; não há fallback para helper de RAM, intérprete ou JIT.
- LDMR-11: se um PC candidato a ROM tiver alternativa `Unknown`/não-ROM, exceder 16 bases ROM únicas, ou qualquer word sair da imagem, strict-AOT MUST falhar antes da publicação do pacote. Para cada alternativa de entrada com base ROM singleton, a descoberta MUST calcular a saída correspondente dessa base sem mesclar com outras alternativas; a tabela MUST agregar separadamente os pares base→words e ser materializada somente depois da convergência. Não pode apagar a evidência de ROM e reclassificar silenciosamente como RAM dinâmica.
- LDMR-12: o manifesto MUST declarar GBA IR versão 2. O pacote final MUST incluir somente código host, manifesto e dados auxiliares existentes; as tabelas contêm apenas words lidas, sem a imagem `.gba`, LLVM ou toolchain.

## Interfaces

- IR de caso: `ThumbOperation::LoadRomMultipleIncrementAfterCases { base_register, register_list, table_id }`.
- Side table: `ArmProgram.thumb_ldmia_rom_tables: Vec<ThumbLdmiaRomTable>`; cada tabela possui casos `{ base_address, values_by_register[8] }` ordenados. Uma operação singleton continua usando `LoadRomMultipleIncrementAfter` da SPEC-035.
- `ThumbOperation::ConditionalBranch` passa a carregar `reachable_successors`, calculado pela união dos sucessores estáticos alcançados.
- ABI C/Rust v3 acrescenta apenas o código de saída `GBA_MEMORY_FAULT_ROM_SPECIALIZATION_V3 = 7`; layouts e versões ABI não mudam.
- GBA IR: versão 2.

## Erros e observabilidade

Diagnósticos de compile MUST identificar PC, registrador/base set quando disponível e causa: conjunto não finito, mais de 16 casos, região mista ou word ROM fora da imagem. Runtime fault informa PC/base e código 7. Rejeições de compile não deixam diretório de pacote parcial.

Para LeafGreen, hash `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825`. Bytes em `0x080006A8` (`01 cb 01 c2 01 39 00 29 fa da`) são observação do artefato; decodificação como LDMIA/STMIA/SUBS/CMP/BGE é inferência. `r3={0x081E9F74,0x081E9F78}`, `r1={12,13}` e `r2={0x03003540,0x03003544}` são estados abstratos de uma revisita instrumentada, não registradores guest em execução. Valores posteriores do loop são inferência de análise, não trace.

## Critérios de aceite e matriz

| Caso | Resultado verificável |
|---|---|
| CMP imediato seguido de BEQ/BGE | Particiona cada alternativa para zeros, positivos, negativos, carry e overflow; runtime usa CPSR real |
| CMP com `Unknown`, branch sem CMP adjacente ou outro produtor de flags | Essa alternativa mantém ambos sucessores |
| Entrada alternativa ao branch sem o mesmo fato CMP | Essa alternativa mantém ambos sucessores; outras podem continuar refinadas |
| Branch loop com contador/base correlacionados | Estados completos disjuntivos mantêm pares; fixed point converge e produz até 16 bases; LeafGreen ultrapassa `0x080006A8` na descoberta estática ou registra a próxima fronteira |
| LDMIA ROM com 2..16 bases | IR valida tabela; O0/O2 dispatcham cada base à word correta |
| Base ausente em runtime | EXIT_FAULT com PC, endereço e código 7; estado guest e ciclo inalterados |
| 65+ alternativas completas por PC, 17+ bases ROM, loop não limitado após candidato ROM ou `Unknown` | Erro de compile, sem pacote parcial e sem fallback |
| Mistura ROM/RAM ou word truncada pela imagem | Erro explícito de compile sem pacote |
| Singleton ROM e RAM dinâmica existentes | Regressão dos comportamentos SPEC-035 O0/O2 |
| Conteúdo do pacote | Não contém ROM inteira, biblioteca LLVM ou toolchain |
| Integração | `cargo test --locked --workspace --jobs 10`, fmt, verificadores C/LLVM, gate GBA-020, LeafGreen compile attempt e hashes registrados |

## Questões abertas

- Depois desta fronteira, qual instrução ou efeito de hardware bloqueia a descoberta de LeafGreen?
- O limite de 16 casos continua adequado quando um ROM grande compilar por completo?
- Uma futura análise generalizada de NZCV deve substituir o refinamento local de CMP/branch?

## Referências

- [SPEC-003 — descoberta](SPEC-003-discovery-overlays.md), [SPEC-004 — IR/LLVM](SPEC-004-ir-llvm.md), [SPEC-031 — Thumb Format 3](SPEC-031-thumb-immediate-format-3.md), [SPEC-032 — branch condicional Thumb](SPEC-032-thumb-conditional-branch.md), [SPEC-035 — Thumb LDMIA](SPEC-035-thumb-ldmia.md), [SPEC-036 — Thumb STMIA](SPEC-036-thumb-stmia.md).
- [ADR-022 — LDMIA ABI](../adr/ADR-022-additive-thumb-ldmia-abi.md), [ADR-024 — especialização finita](../adr/ADR-024-finite-rom-ldmia-specialization.md), [KNO-004 — intake LeafGreen](../knowledge/KNO-004-rom-intake.md).
- SHA-256 e delimitações da ROM estão em KNO-004; inventário ISA e referências vendor fixadas estão em [KNO-005](../knowledge/KNO-005-isa-coverage.md) e [KNO-001](../knowledge/KNO-001-evidence.md).
