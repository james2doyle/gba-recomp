# KNO-003 — Perguntas abertas

| Questão | Evidência necessária | Impacto/etapa |
|---|---|---|
| Qual primeira ROM homebrew e cenário de referência? | Resolvido como smoke test: Tiny Counter tem fonte, hash e build reproduzível; falta fixture interativa com memória/periféricos e saídas A/V | GBA-020/030/040 |
| Emissão, verificação e link da fixture funcionam com a toolchain fixada nos dois hosts? | macOS arm64 nativo passou; falta relatório Linux x86-64 com hashes, linker/sysroot, objeto e execução | GBA-010 |
| Qual precisão de bus/prefetch/pipeline basta para cada título? | Suites e traces, depois hardware quando houver divergência | GBA-030/040 |
| Quais overlays/SMC escapam ao AOT? | Cobertura observada e writes por origem | GBA-025; unknown REA |
| Qual cobertura AOT e quais recursos de hardware LeafGreen exige? | Matriz por etapa em [KNO-005](KNO-005-isa-coverage.md); a tentativa O2 após SPEC-045 termina em `bx_target_unresolved` no Thumb `BX r0` em `0x08000B6A`, sem pacote; isso não prova quais caminhos comerciais executam nem quais periféricos usam | GBA-090 |
| O fechamento do BX r0 em `0x080009BE` cobre todos os caminhos da ROM? | A sombra limitada da SPEC-034 recuperou o LR salvo e a descoberta estática avançou além do BX nesse compile; repetir a análise após as próximas instruções e registrar qualquer caminho que permaneça desconhecido | GBA-025 |
| A análise LeafGreen prova o loop LDMIA/STMIA em `0x080006A8`? | LDMIA/STMIA têm contratos, lowering e fixtures próprios; a falha mais recente em `0x08000B6A` não prova que o loop foi alcançado, convergiu ou executaria em runtime | GBA-025/030 |
| Quais SWIs são necessários à primeira fixture? | Grafo + execução controlada | GBA-045 |
| Qual adaptador desktop e biblioteca de menu? | Slice acessível teclado/gamepad e comportamento sem GPU/áudio | GBA-070 |
| Qual fase limita o tempo de compilação de uma ROM? | LeafGreen ainda falha em discovery em 0,61 s, antes da emissão/link; o gate sintético levou 140,59 s wall e 497,28 s user com 10 workers configurados, média agregada derivada de 3,67 CPUs lógicas. Não houve telemetria por processo nem perfil por fase. Medir leitura/hash, discovery, validação IR, LLVM e link em ROM que avance mais | GBA-050 |
| Como reduzir tempo de compilação por ROM sem alterar semântica? | Instrumentar tempo/RSS por fase primeiro; se LLVM dominar em ROM que chegue à emissão, avaliar shards determinísticos de código AOT. Discovery tem fila e admissão serial com budgets que dependem da ordem | GBA-050 |
| Por que BX r0 continua desconhecido em LeafGreen após SPEC-045? | O novo compile termina em 0x08000B6A, mas não registra r0/SP/sombra por predecessor. Instrumentar os estados nos dois sucessores de 0x08000B50 e em 0x08000B64–0x08000B6A antes de atribuir a causa; alias da STRB de 0x08000B64 com LR salvo é hipótese condicional ao SP, não observação | GBA-025 |
| Qual orçamento de tamanho e memória por ROM/target? | Medidas por shard/target e orçamento de memória | GBA-050 |
| Quando suportar link, e-Reader, sensores e cartuchos especiais? | Corpus selecionado e especificações incrementais | GBA-100 |

Essas lacunas não impedem os contratos iniciais; impedem declarar emissão/link verificados, desempenho ou jogos suportados sem prova.

As versões Rust/LLVM/binding e os targets iniciais foram escolhidos no [ADR-004](../adr/ADR-004-toolchain-baseline.md), com versões e hashes em [toolchain.lock.toml](../toolchain.lock.toml). O gate GBA-010 passou em macOS arm64 e segue aberto para Linux x86-64. A biblioteca de frontend continua em aberto até GBA-070.
