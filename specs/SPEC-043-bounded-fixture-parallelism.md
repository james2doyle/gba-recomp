# SPEC-043 — Paralelismo limitado de fixtures independentes do GBA-020

Estado do documento: Accepted. Implementação no commit `a27e95e1ccf276f471586a220a8b39eab23f6807`; evidência: [gate GBA-020 e medição A/B](../knowledge/evidence/gba020-thumb-strict-aot-parallelism-macos-arm64.json). Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

O gate GBA-020 emite e linka muitos pacotes de fixture pequenos. Alguns grupos contêm programas isolados, com ROM, pacote, manifesto e log próprios, mas são verificados em sequência. O host medido tem dez CPUs lógicas. O build do recompilador ocorre antes dos grupos de fixtures.

Permitir que grupos explicitamente listados usem workers limitados, cada um compilando um modo completo em O0 e O2. O padrão pode ocupar até dez workers com uma thread LLD por compilação. A execução das fixtures e a verificação de manifestos permanecem dentro do worker de cada modo. Os grupos iniciais são Thumb Format 3, LDR word imediato e LDRH imediato.

## Fora de escopo

- Paralelizar fases dentro de uma ROM, descoberta do CFG ou emissão de um único módulo LLVM.
- Paralelizar grupos diferentes ao mesmo tempo ou sobrepor o build do Cargo aos workers de fixture.
- Executar ROM comercial ou elevar budgets de estado/ISA em nome de throughput.
- Alterar semântica guest, manifesto, toolchain ou o contrato strict-AOT.
- Afirmar aceleração de compilação de uma ROM comercial a partir do throughput de fixtures.

## Invariantes

- Os workers do grupo são independentes: cada modo escreve ROM, pacotes O0/O2 e log com nomes exclusivos.
- `workers * GBA020_LINK_THREADS <= 10`; cada processo de compilação recebe explicitamente o limite de threads LLD escolhido.
- O gate compila o binário Cargo antes de iniciar qualquer worker. Cada grupo espera todas as tarefas antes de iniciar o seguinte.
- Uma falha em qualquer worker faz o gate falhar e preserva no output os logs dos modos do lote, para diagnóstico.
- `GBA020_JOBS=1` continua permitindo execução serial. Modos fora dos grupos declarados continuam seriais.
- A configuração de jobs afeta somente o host de build e não pode alterar os resultados guest.

## Requisitos normativos

- BFP-01: o scheduler MUST aceitar somente `GBA020_JOBS=1,2,4,5,7,8,9,10` e rejeitar produto workers×threads LLD acima de dez antes de compilar.
- BFP-02: os lotes Thumb Format 3, LDR word e LDRH MUST respeitar o limite configurado, aguardar cada lote e não compartilhar caminhos de saída entre workers.
- BFP-03: cada worker MUST validar resultados O0/O2, manifestos strict-AOT e estados/faults definidos para seu modo; rejeições que permaneçam seriais MUST continuar verificadas pelo gate.
- BFP-04: falha de um worker MUST retornar status não zero do gate e imprimir os logs do lote, sem ser convertida em sucesso.
- BFP-05: modo serial e modo paralelo MUST produzir os mesmos estados guest e decisões de rejeição para os mesmos binários e fixtures.
- BFP-06: o gate MUST emitir a duração wall de cada grupo agendado e o registro de desempenho MUST identificar host, configuração, duração wall/user/sys, número de casos e hashes dos scripts/logs. RSS de `time` não pode ser descrito como soma dos workers.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| Configuração serial | `GBA020_JOBS=1` passa o lote e verifica os mesmos resultados |
| Configuração padrão | Até dez workers × uma thread LLD passam sem ultrapassar o teto de dez slots |
| Erro de configuração | Produto acima de dez é rejeitado antes de iniciar Cargo |
| Falha em worker | O lote retorna erro e imprime o log da fixture com falha |
| Concorrência | Nenhum grupo excede workers configurados; grupos não se sobrepõem |
| LDR word e LDRH | Sucessos e faults O0/O2 conservam estado ou endereço/fault esperados |
| Throughput | Comparação serial/paralela mede apenas o grupo sintético e registra hashes/limitações |

## Fontes e referências

- [ADR-021 — paralelismo limitado](../adr/ADR-021-bounded-gba020-gate-parallelism.md).
- [Gate GBA-020](../scripts/verify-gba020.sh).
- [ROADMAP](../ROADMAP.md), [KNO-001](../knowledge/KNO-001-evidence.md).
