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
| Qual fase limita o tempo de compilação de uma ROM? | LeafGreen ainda falha em discovery em 0,61 s, antes da emissão/link; paralelizar o gate sintético não acelera essa falha. Medir leitura/hash, discovery, validação IR, LLVM e link em uma ROM que avance mais. A evidência recente do gate mede apenas suas fixtures | GBA-050 |
| Como reduzir tempo de compilação por ROM sem alterar semântica? | Instrumentar tempo/RSS por fase primeiro; se LLVM dominar em ROM que chegue à emissão, avaliar shards determinísticos de código AOT. Discovery tem fila e admissão serial com budgets que dependem da ordem | GBA-050 |
| Por que BX r0 continua desconhecido em LeafGreen após SPEC-048? | Respondido pelo trace pós-WAITCNT: a primeira invalidação restante ocorre no store de DMA3 SAD em B6B4 (`0x040000D4`). O trace seguinte mostra DAD=`0x03003580` e word count/control=`0x84000200`, mas isso é estado interno de discovery. SPEC-049 define uma exceção limitada para transação imediata ROM→IWRAM sem IRQ no modelo `interrupt_model:none`; falta implementar, testar e repetir a análise. DMA real, eventos e runtime continuam sem suporte. | GBA-025/030 |
| Qual orçamento de tamanho e memória por ROM/target? | Medidas por shard/target e orçamento de memória | GBA-050 |
| Quando suportar link, e-Reader, sensores e cartuchos especiais? | Corpus selecionado e especificações incrementais | GBA-100 |

Essas lacunas não impedem os contratos iniciais; impedem declarar emissão/link verificados, desempenho ou jogos suportados sem prova.

As versões Rust/LLVM/binding e os targets iniciais foram escolhidos no [ADR-004](../adr/ADR-004-toolchain-baseline.md), com versões e hashes em [toolchain.lock.toml](../toolchain.lock.toml). O gate GBA-010 passou em macOS arm64 e segue aberto para Linux x86-64. A biblioteca de frontend continua em aberto até GBA-070.
