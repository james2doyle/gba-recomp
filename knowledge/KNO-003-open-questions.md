# KNO-003 — Perguntas abertas

| Questão | Evidência necessária | Impacto/etapa |
|---|---|---|
| Qual primeira ROM homebrew e cenário de referência? | Fonte, build reproduzível, hash e input | GBA-020 |
| Emissão, verificação e link da fixture funcionam com a toolchain fixada nos dois hosts? | macOS arm64 nativo passou; falta relatório Linux x86-64 com hashes, linker/sysroot, objeto e execução | GBA-010 |
| Qual precisão de bus/prefetch/pipeline basta para cada título? | Suites e traces, depois hardware quando houver divergência | GBA-030/040 |
| Quais overlays/SMC escapam ao AOT? | Cobertura observada e writes por origem | GBA-025; unknown REA |
| Quais SWIs são necessários à primeira fixture? | Grafo + execução controlada | GBA-045 |
| Qual adaptador desktop e biblioteca de menu? | Slice acessível teclado/gamepad e comportamento sem GPU/áudio | GBA-070 |
| Limites de compilação e tamanho por ROM? | Medidas por shard/target e orçamento de memória | GBA-050 |
| Quando suportar link, e-Reader, sensores e cartuchos especiais? | Corpus selecionado e especificações incrementais | GBA-100 |

Essas lacunas não impedem os contratos iniciais; impedem declarar emissão/link verificados, desempenho ou jogos suportados sem prova.

As versões Rust/LLVM/binding e os targets iniciais foram escolhidos no [ADR-004](../adr/ADR-004-toolchain-baseline.md), com versões e hashes em [toolchain.lock.toml](../toolchain.lock.toml). O gate GBA-010 passou em macOS arm64 e segue aberto para Linux x86-64. A biblioteca de frontend continua em aberto até GBA-070.
