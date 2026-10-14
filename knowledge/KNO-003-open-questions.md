# KNO-003 — Perguntas abertas

| Questão | Evidência necessária | Impacto/etapa |
|---|---|---|
| Qual primeira ROM homebrew e cenário de referência? | Resolvido como smoke test: Tiny Counter tem fonte, hash e build reproduzível; falta fixture interativa com memória/periféricos e saídas A/V | GBA-020/030/040 |
| Emissão, verificação e link da fixture funcionam com a toolchain fixada nos dois hosts? | macOS arm64 nativo passou; falta relatório Linux x86-64 com hashes, linker/sysroot, objeto e execução | GBA-010 |
| Qual precisão de bus/prefetch/pipeline basta para cada título? | Suites e traces, depois hardware quando houver divergência | GBA-030/040 |
| Quais overlays/SMC escapam ao AOT? | Cobertura observada e writes por origem | GBA-025; unknown REA |
| Qual cobertura AOT e quais recursos de hardware LeafGreen exige? | Matriz por etapa em [KNO-005](KNO-005-isa-coverage.md); após SPEC-052, a tentativa O2 passou estaticamente por NEGS e parou em `0x17C1` em `0x08000BEE`, inferido via SRC-39 como ASRS `r1,r0,#31`. Continua antes de LLVM/link, sem pacote ou execução; não prova quais caminhos comerciais executam nem quais periféricos usam | GBA-090 |
| O fechamento do BX r0 em `0x080009BE` cobre todos os caminhos da ROM? | A sombra limitada da SPEC-034 recuperou o LR salvo e aquela descoberta estática passou além do BX; isso não cobre todos os caminhos. A fronteira atual consta em KNO-004 e continua sendo somente análise estática | GBA-025 |
| A análise LeafGreen prova o loop LDMIA/STMIA em `0x080006A8`? | LDMIA/STMIA têm contratos, lowering e fixtures próprios; a tentativa pós-SPEC-045 terminou em `0x08000B6A`, mas não prova que o loop foi alcançado, convergiu ou executaria em runtime | GBA-025/030 |
| Quais SWIs são necessários à primeira fixture? | Grafo + execução controlada | GBA-045 |
| Qual adaptador desktop e biblioteca de menu? | Slice acessível teclado/gamepad e comportamento sem GPU/áudio | GBA-070 |
| Qual fase limita o tempo de compilação de uma ROM? | A última LeafGreen falha durante discovery em 0,61 s, antes de LLVM/link; paralelizar o gate sintético não acelera essa falha. O gate usa até 10 workers × uma thread LLD e o lote novo NEGS levou 2,444 s, mas não há baseline nem medição de saturação sustentada. Medir leitura/hash, discovery, validação IR, construção/otimização/emissão LLVM e link quando uma ROM avançar mais. Registrar também updates, estados únicos, pico da fila e RSS. Os 0,61 s não são benchmark de throughput | GBA-050 |
| Como reduzir tempo de compilação por ROM sem alterar semântica? | Instrumentar tempo/RSS por fase primeiro; se LLVM dominar em ROM que chegue à emissão, avaliar shards determinísticos de código AOT. Discovery tem fila e admissão serial com budgets que dependem da ordem; mais workers no gate sintético não aceleram essa fase | GBA-050 |
| Por que BX r0 continuava desconhecido em LeafGreen após SPEC-048? | A correção limitada de discovery SPEC-049 foi implementada no commit `75aaf8d`; a compilação estática passou dessa fronteira, depois do B Thumb em `0x08000AE6`, e avançou até `0x4002` em `0x08000BE4`. A semântica de runtime DMA não foi adicionada. Evidência em KNO-004. | GBA-025/030 |
| Qual o próximo incremento para a discovery LeafGreen? | SPEC-052 implementou NEGS e a compilação passou por `0x4248` em `0x08000BEA`; a próxima fronteira é raw `0x17C1` em `0x08000BEE`, inferido via SRC-39 como ASRS `r1,r0,#31`. SPEC-053/ADR-038 estão em Draft/Proposed antes da implementação; manter evidência estática separada de execução dinâmica. | GBA-025 |
| Qual orçamento de tamanho e memória por ROM/target? | Medidas por shard/target e orçamento de memória | GBA-050 |
| Quando suportar link, e-Reader, sensores e cartuchos especiais? | Corpus selecionado e especificações incrementais | GBA-100 |

Essas lacunas não impedem os contratos iniciais; impedem declarar emissão/link verificados, desempenho ou jogos suportados sem prova.

As versões Rust/LLVM/binding e os targets iniciais foram escolhidos no [ADR-004](../adr/ADR-004-toolchain-baseline.md), com versões e hashes em [toolchain.lock.toml](../toolchain.lock.toml). O gate GBA-010 passou em macOS arm64 e segue aberto para Linux x86-64. A biblioteca de frontend continua em aberto até GBA-070.
