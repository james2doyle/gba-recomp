# ADR-021 — Paralelismo limitado no gate GBA-020

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

O Mac mini M4 disponível para este projeto tem 10 CPUs lógicas. Cargo já usa 10 jobs e o link do próprio compilador usa 10 threads LLD. O gate GBA-020 compila muitos pacotes de fixture em sequência; há grupos independentes que podem ocupar CPUs durante esse trecho, mas cada processo também otimiza LLVM e inicia um link.

## Decisão

O gate começa com dois workers para os sete casos independentes de Thumb LSL e configura cada link final LLD com quatro threads. Isso limita cada pacote worker a uma otimização LLVM e quatro threads LLD. O build do binário do recompilador continua serial em relação ao gate.

O usuário pode selecionar execução serial com `GBA020_JOBS=1` (10 threads LLD por pacote) ou o pool padrão com `GBA020_JOBS=2` (4 threads LLD por pacote). `GBA020_LINK_THREADS` pode substituir a contagem LLD em ambos os modos; o CLI valida o intervalo de 1 a 10. Outros grupos do gate e ROMs comerciais permanecem seriais até medição individual de memória e throughput.

## Alternativas

- Aumentar Cargo acima de 10 não aumenta a capacidade do host disponível e pode elevar a contenção.
- Rodar todas as 51 compilações em paralelo multiplica memória e, sem limitar LLD por processo, cria dezenas de threads concorrentes.
- Paralelizar LeafGreen antes de medir RSS aumenta o risco de pressão de memória no Mac mini.

## Consequências

O pool é pequeno, limitado, desativável e usa paths/logs separados por fixture. Nenhuma semântica guest ou manifesto muda. A medição deve comparar o tempo da fase de fixtures em execução serial e com o pool no mesmo commit, cache quente e três execuções, registrando wall time, CPU e RSS. Não declarar ganho antes desse benchmark.

## Critérios para revisitar

- Mediana serial versus pool deixa de melhorar ou aumenta excessivamente o RSS.
- Evidência de que outros grupos independentes têm footprint de memória aceitável.
- O número de CPUs lógicas ou a toolchain LLVM do host mudar.

## Referências

[ROADMAP](../ROADMAP.md), [README](../README.md), [gate GBA-020](../scripts/verify-gba020.sh), `crates/gba-recomp/src/main.rs`.
