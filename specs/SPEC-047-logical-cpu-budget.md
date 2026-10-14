# SPEC-047 — Orçamento de CPUs lógicas para o gate GBA-020

Estado do documento: Accepted. Implementação: concluída e verificada em Darwin/arm64; validação de produto Linux segue pendente. Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

O host de referência atual é um Mac mini M4 com dez CPUs lógicas. O Cargo tem dez jobs configurados, e o gate GBA-020 agenda grupos de fixtures independentes com workers e threads LLD limitados por [SPEC-043](SPEC-043-bounded-fixture-parallelism.md) e [ADR-021](../adr/ADR-021-bounded-gba020-gate-parallelism.md). O procedimento deve usar a capacidade lógica detectada no host, inclusive quando ela for menor que dez, e deve limitar toda fase configurada ao mesmo orçamento.

Esta spec define a detecção e o limite de CPU do gate, além de permitir que as cinco fixtures Thumb STRB e as cinco LDRB passem a um único lote de até dez modos. Cada worker compila O0/O2, executa os pacotes e valida os estados/manifestos existentes. No host de dez CPUs, o lote pode usar dez workers × uma thread LLD, sem alterar semântica guest. A spec não afirma saturação de CPU nem ganho de desempenho; isso exige evidência medida separadamente.

O script principal reconhece Darwin/arm64 e Linux/x86_64 como ambientes para este gate. O detector não declara suporte de produto nesses destinos. A verificação de emissão, link, execução e adaptadores continua sendo governada por SPEC-010 e pelo roadmap.

## Fora de escopo

- Paralelizar descoberta de CFG, emissão LLVM ou fases de uma ROM individual.
- Executar grupos distintos simultaneamente, ou sobrepor Cargo e workers de fixture.
- Aumentar budgets de estados/ISA, alterar semântica guest, toolchain, manifesto ou formato do pacote.
- Impor afinidade, reserva exclusiva de CPU, ou limite sobre processos externos ao gate e paralelismo interno não configurado de outras ferramentas.
- Prometer throughput ou redução de tempo sem medições comparáveis.
- Declarar suporte Linux de produto apenas porque o detector ou o plano do gate foi validado.

## Vocabulário e modelo

- **CPUs lógicas detectadas**: quantidade positiva reportada pelo detector aplicável ao host antes de Cargo, workers, clang ou LLD.
- **Teto do projeto**: dez CPUs lógicas para este gate, mesmo se o host informar mais.
- **Orçamento automático**: `min(10, CPUs lógicas detectadas)`; se a detecção falhar, fallback seguro de um slot.
- **Orçamento efetivo**: orçamento automático, ou valor menor escolhido por `GBA020_CPU_BUDGET`.
- **Jobs Cargo**: valor efetivo de `--jobs` enviado a cada invocação Cargo do procedimento.
- **Threads LLD do Cargo**: máximo de threads do link do binário `gba-recomp`, configurado via `GBA_RECOMP_CARGO_LINK_THREADS` no build script.
- **Workers de fixture**: limite de workers simultâneos aplicado a cada grupo independente.
- **Slots de grupo**: workers simultâneos multiplicados pelas threads LLD configuradas para cada compilação do grupo.
- **Threads LLD serial**: teto explícito para cada compilação que não roda dentro de um grupo paralelo.

O orçamento limita paralelismo configurado pelo gate em Cargo, workers e LLD. Não é uma reserva de CPU do sistema operacional e não mede uso real.

## Requisitos normativos

- CPU-01 — Antes de iniciar Cargo, gerar fixtures, iniciar workers, ou chamar clang/LLD, o gate MUST detectar as CPUs lógicas e validar todas as opções de orçamento e paralelismo.
- CPU-02 — Em Darwin, o detector MUST usar `sysctl -n hw.logicalcpu`. Em Linux, MUST usar `nproc`, que informa CPUs disponíveis ao processo. Estes detectores servem apenas aos hosts reconhecidos pelo gate. Se o detector aplicável estiver indisponível, falhar ou retornar valor inválido, o gate MUST aplicar CPU-03; não pode substituir `nproc` por um total do host que ignore afinidade visível.
- CPU-03 — O detector MUST aceitar somente um inteiro decimal positivo, sem sinal ou sufixo. Se a detecção falhar ou retornar saída inválida, o gate MUST definir orçamento automático de um slot e registrar `detector=fallback-one`; não pode inferir um orçamento maior.
- CPU-04 — O orçamento automático MUST ser o menor entre dez e as CPUs lógicas detectadas. Um host que informa mais de dez permanece limitado a dez.
- CPU-05 — Se `GBA020_CPU_BUDGET` estiver definido, MUST ser um inteiro decimal positivo, não vazio, e menor ou igual ao orçamento automático. Pode reduzir o orçamento; não pode elevá-lo. Valores vazios, inválidos, zero, negativos ou acima do automático MUST falhar antes de qualquer build, geração de fixture, worker, clang ou LLD.
- CPU-06 — Cada comando Cargo executado pelo procedimento do gate MUST receber explicitamente `--jobs <orçamento efetivo>`. Configuração local e variáveis Cargo não podem aumentar o valor passado pelo gate. O build do binário único `gba-recomp` MUST receber `GBA_RECOMP_CARGO_LINK_THREADS=<orçamento efetivo>` e selecionar somente `--bin gba-recomp`; Cargo termina essa fase antes de iniciar workers de fixture.
- CPU-07 — O build Cargo MUST terminar antes do começo dos workers. Cargo e workers não podem executar simultaneamente como fases do gate.
- CPU-08 — `GBA020_JOBS` MUST pertencer ao conjunto aceito por SPEC-043 e não exceder o orçamento efetivo. `GBA020_LINK_THREADS` MUST ser um inteiro positivo não superior ao orçamento efetivo. `GBA020_SERIAL_LINK_THREADS`, quando definido, MUST ser um inteiro positivo não superior ao orçamento efetivo. Opções inválidas MUST falhar antes de Cargo.
- CPU-09 — Antes de Cargo, o gate MUST validar `threads LLD do Cargo <= orçamento efetivo` e `GBA020_JOBS × GBA020_LINK_THREADS <= orçamento efetivo`, ainda que o grupo tenha menos casos que workers disponíveis. O build seleciona um único binário, cujo link recebe o orçamento efetivo inteiro. Nenhum grupo pode exceder seu produto nem compartilhar saídas de workers. Os grupos são executados sequencialmente.
- CPU-10 — Toda compilação fora dos grupos paralelos MUST receber explicitamente `GBA_RECOMP_LINK_THREADS=GBA020_SERIAL_LINK_THREADS`. O padrão serial MUST ser o orçamento efetivo. Um default implícito do compilador não pode ultrapassar esse teto.
- CPU-11 — Sem overrides no Mac mini M4 de dez CPUs lógicas, o padrão MUST ser Cargo 10, LLD do binário Cargo 10, workers 10 × LLD de grupo 1 e LLD serial 10. O máximo configurado por fase MUST permanecer em dez slots.
- CPU-12 — O grupo Thumb STRB/LDRB MUST agendar os cinco modos STRB e cinco modos LDRB como dez tarefas independentes, no máximo `GBA020_JOBS` tarefas simultâneas. No caso sem overrides e com orçamento efetivo dez, deve usar dez workers × uma thread LLD. Nos demais casos, deve usar `GBA020_JOBS` e `GBA020_LINK_THREADS` validados pelas regras gerais; com `GBA020_JOBS=1`, executa serialmente. Esta regra não muda os defaults definidos pela ADR-021 quando o usuário escolhe um número menor de workers.
- CPU-13 — O gate MUST imprimir antes do trabalho uma linha legível por máquina com campos `os`, `arch`, `detector`, `detected_cpus`, `automatic_budget`, `budget_override`, `effective_budget`, `cargo_jobs`, `cargo_link_threads`, `fixture_workers`, `group_link_threads`, `serial_link_threads`, `max_phase_slots` e `fallback`. `max_phase_slots` MUST ser o máximo entre jobs Cargo, threads LLD do binário Cargo, slots de grupo ou LLD serial, pois as fases não se sobrepõem.
- CPU-14 — O registro de slots configurados MUST ser distinto de medição. Não pode ser descrito como CPUs ocupadas, uso sustentado ou saturação. Claims de throughput exigem wall/user/system, repetições e metodologia registrados em evidência separada.
- CPU-15 — Os mesmos estados guest, manifestos, resultados de fixture e decisões de rejeição MUST ser produzidos com qualquer orçamento válido; o orçamento muda somente o paralelismo do host.
- CPU-16 — O helper `verify-gba020-lsl.sh` MUST aceitar exatamente `<jobs> <link-threads> <effective-budget>`. Tanto em chamada direta quanto pelo script principal, deve recalcular o limite detectado e aplicar `GBA020_CPU_BUDGET`, quando definido. O terceiro argumento MUST ser igual ao orçamento efetivo recalculado; divergência, produto acima do orçamento, jobs não aceitos por SPEC-043 ou argumento inválido MUST falhar antes de gerar fixture ou compilar.

## Política de defaults

- Sem `GBA020_CPU_BUDGET`, usar o orçamento automático. Com override válido, usar o override como orçamento efetivo.
- `cargo build --bin gba-recomp` usa sempre `--jobs` igual ao orçamento efetivo e compila apenas o binário do gate; o `.cargo/config.toml` não substitui esse argumento. O gate passa `GBA_RECOMP_CARGO_LINK_THREADS` igual ao orçamento efetivo para que o único link configurado use toda a capacidade disponível. Cargo termina antes dos workers de fixture. Fora do gate, o build script mantém o padrão de dez threads LLD ou valida o override de 1 a 10.
- Sem `GBA020_JOBS`, escolher o maior valor aceito por SPEC-043 que não exceda o orçamento efetivo.
- Sem `GBA020_LINK_THREADS`, calcular `min(default_ADR-021, floor(orçamento efetivo / GBA020_JOBS))`, onde a tabela ADR-021 é 1→10, 2→4, 4→2, 5→2, 7–10→1. Exemplo: orçamento 3 e dois workers resultam em uma thread LLD. Um override explícito que faça o produto exceder o orçamento MUST falhar, sem redução silenciosa. Quando jobs atingirem o orçamento, usar uma thread LLD por worker.
- Sem `GBA020_SERIAL_LINK_THREADS`, usar o orçamento efetivo para links seriais.
- Overrides de jobs, LLD por worker e LLD serial podem reduzir paralelismo. Nenhuma combinação aceita pode superar o orçamento efetivo.

## Matriz de aceite

| Host/configuração | Resultado verificável |
|---|---|
| Darwin/arm64, dez CPUs lógicas, sem override | Detecta dez; Cargo jobs 10 e binário link LLD 10; grupos até 10×1; link serial 10; máximo de fase 10 |
| Linux/x86_64, oito CPUs utilizáveis por `nproc` | Orçamento automático/efetivo oito; nenhum limite de fase excede oito |
| Detector informa mais de dez | Orçamento automático limitado a dez |
| Detector informa menos de dez | Cargo, workers e LLD limitados ao orçamento detectado |
| `GBA020_CPU_BUDGET=4` em host com dez CPUs | Orçamento efetivo quatro e todos os produtos limitados a quatro |
| Override vazio, inválido, zero, negativo ou acima do automático | Falha antes de Cargo, fixture, worker, clang ou LLD |
| Detecção indisponível/inválida | Fallback um registrado; override acima de um rejeitado |
| Produto `GBA020_JOBS × GBA020_LINK_THREADS` acima do orçamento | Falha antes de Cargo ou worker |
| Cada invocação Cargo | Contém `--jobs` explícito igual ao orçamento efetivo e build do binário único com threads LLD iguais ao orçamento |
| Serial e paralelo | Mesmo estado guest e decisões de rejeição |
| Grupo STRB/LDRB | Dez modos distintos passam em O0/O2, com saídas independentes e teto agregado respeitado |
| Helper LSL chamado diretamente | Detecta host, valida orçamento, workers e LLD antes de gerar fixture ou compilar |

## Verificações e evidência requeridas para implementação

- Verificar parser/detector e seleção de orçamento com hosts simulados de 1, 2, 4, 8, 10 e mais de 10 CPUs, além de fallback; não depender do número físico do host.
- Verificar rejeição antecipada de overrides inválidos e combinações jobs×LLD acima do limite, sem subprocessos de build/fixture/link.
- Executar o gate em Darwin/arm64 e registrar linha de orçamento; isso não declara suporte de produto Linux.
- Confirmar todos os grupos e compilações seriais usam limites explícitos de LLD, e cada Cargo usa `--jobs` explícito; o build do binário único aplica o orçamento efetivo como limite LLD.
- Confirmar STRB/LDRB serial e paralelo dão os mesmos estados guest, manifestos e rejeições, sem pacote parcial e sem colisão de caminho.
- Confirmar chamada direta do helper LSL não pode exceder o detector nem o override de CPU.
- Se comparar throughput, fixar binário, source, fixtures e host, alternar baseline e candidato em ao menos três rodadas, registrar wall/user/system e os hashes. RSS reportado por um processo não pode ser chamado de soma dos workers.
- Registrar comandos, hashes do script e resultados em evidência antes de mudar esta spec para Accepted. Execução de gate em Linux permanece sujeita aos critérios de suporte de plataforma do roadmap.

## Registro de aceite — 2026-10-14

O detector foi simulado para 1, 2, 4, 8, 10, 16 CPUs, oito CPUs Linux e fallback inválido; os orçamentos corresponderam ao contrato. Override normalizado `0004` foi aceito; override vazio, zero, negativo, acima do automático e texto foram rejeitados. Os gates completo paralelo e serial passaram em Darwin/arm64 com os mesmos estados O0/O2 e decisões de rejeição. Uma chamada do helper LSL com budget divergente e a combinação antecipada `GBA020_CPU_BUDGET=4 GBA020_JOBS=5` foram rejeitadas antes de compilar. Três rodadas alternadas mediram o grupo sintético STRB/LDRB em 5,5067 s de mediana serial contra 2,1146 s paralelo. A evidência preserva logs integrais, harness, hashes, tempos e limites em [gba020-cpu-budget-accelerated-gate.json](../knowledge/evidence/gba020-cpu-budget-accelerated-gate.json). A matriz Linux foi simulada no detector; não significa execução do gate nem suporte de produto Linux.

## Referências

- [SPEC-043 — paralelismo limitado de fixtures](SPEC-043-bounded-fixture-parallelism.md).
- [ADR-021 — paralelismo limitado do gate](../adr/ADR-021-bounded-gba020-gate-parallelism.md).
- [ADR-032 — lote de fixtures Thumb STRB/LDRB](../adr/ADR-032-parallel-thumb-byte-fixtures.md).
- [README](../README.md), [ROADMAP](../ROADMAP.md) e [gate GBA-020](../scripts/verify-gba020.sh).
- [Evidência SPEC-038–043](../knowledge/evidence/gba020-thumb-strict-aot-parallelism-macos-arm64.json) e [evidência SPEC-045](../knowledge/evidence/gba020-thumb-ie-ime-latches-macos-arm64.json).
