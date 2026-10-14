# ADR-021 — Paralelismo limitado no gate GBA-020

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

O Mac mini M4 disponível para este projeto tem 10 CPUs lógicas. Cargo já usa 10 jobs e o link do próprio compilador usa 10 threads LLD. O gate GBA-020 compila muitos pacotes de fixture em sequência; há grupos independentes que podem ocupar CPUs durante esse trecho, mas cada processo também otimiza LLVM e inicia um link.

## Decisão

O gate usa sete workers para os sete casos independentes de Thumb LSL e configura cada link final LLD com uma thread. O grupo tem no máximo sete links simultâneos. O build do binário do recompilador continua serial em relação ao gate.

`GBA020_JOBS` aceita 1, 2, 4, 5 ou 7 workers. Os defaults são, respectivamente, 10, 5, 2, 2 e 1 thread LLD por pacote. `GBA020_LINK_THREADS` pode substituir esse valor, desde que o produto de workers e threads não passe de 10. O CLI valida de 1 a 10 threads para cada link. Outros grupos do gate e ROMs comerciais permanecem seriais até medição individual de memória e throughput.

## Alternativas

- Aumentar Cargo acima de 10 não aumenta a capacidade do host disponível e pode elevar a contenção.
- Rodar todas as 51 compilações em paralelo multiplica memória e, sem limitar LLD por processo, cria dezenas de threads concorrentes.
- Paralelizar LeafGreen antes de medir RSS aumenta o risco de pressão de memória no Mac mini.

## Consequências

O pool usa paths/logs separados por fixture e não muda semântica guest ou manifesto. Em três execuções alternadas no mesmo host e com o binário já compilado, [scripts/verify-gba020-lsl.sh](../scripts/verify-gba020-lsl.sh) mediu medianas de 1,68 s com quatro workers × duas threads, 1,77 s com cinco × duas e 1,48 s com sete × uma. Sete workers × uma thread reduziram o wall time em 11,9% contra quatro × duas e elevaram a razão de CPU acumulada por wall time de 1,49 para 1,91 CPUs. O maior RSS reportado por `time -lp` foi cerca de 39–41 MB por comando; essa medição não soma o RSS simultâneo de todos os workers. Os vetores completos e limitações estão em [gba020-thumb-ldmia-macos-arm64.json](../knowledge/evidence/gba020-thumb-ldmia-macos-arm64.json).

O gate completo passou com o padrão de sete workers × uma thread LLD. A rodada fria incluiu 73 s para compilar o binário e não é comparável ao gate aquecido. Nenhuma alegação de aceleração da compilação de uma ROM comercial é feita.

## Critérios para revisitar

- Mediana serial versus pool deixa de melhorar ou aumenta excessivamente o RSS.
- Evidência de que outros grupos independentes têm footprint de memória aceitável.
- O número de CPUs lógicas ou a toolchain LLVM do host mudar.

## Referências

[ROADMAP](../ROADMAP.md), [README](../README.md), [gate GBA-020](../scripts/verify-gba020.sh), `crates/gba-recomp/src/main.rs`.
