# ADR-021 — Paralelismo limitado no gate GBA-020

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

O Mac mini M4 disponível para este projeto tem 10 CPUs lógicas. Cargo já usa 10 jobs e o link do próprio compilador usa 10 threads LLD. O gate GBA-020 compila muitos pacotes de fixture em sequência; há grupos independentes que podem ocupar CPUs durante esse trecho, mas cada processo também otimiza LLVM e inicia um link.

## Decisão

Grupos independentes de Thumb LSL, ADD/SUB, Format 3, LDR word e LDRH podem usar o scheduler limitado especificado em [SPEC-043](../specs/SPEC-043-bounded-fixture-parallelism.md). O build do binário do recompilador termina antes dos workers e cada grupo é aguardado antes do próximo. A extensão posterior do pool para STRB/LDRB está registrada em [ADR-032](ADR-032-parallel-thumb-byte-fixtures.md); o teto de CPU detectado e o tratamento dos links seriais estão em [SPEC-047](../specs/SPEC-047-logical-cpu-budget.md).

`GBA020_JOBS` aceita 1, 2, 4, 5, 7, 8, 9 ou 10 workers. Os defaults são, respectivamente, 10, 4, 2, 2, 1, 1, 1 e 1 thread LLD por pacote. `GBA020_LINK_THREADS` pode substituir esse valor, desde que o produto de workers e threads não passe de 10. O gate valida de 1 a 10 threads para cada link. LSL/LSR, ADD/SUB, Thumb Format 3 (dez fixtures), LDR word (nove modos), LDRH (dez modos, incluindo a codificação `0x8821` observada estaticamente em LeafGreen) e, por ADR-032, o grupo conjunto STRB/LDRB (dez modos) usam estes workers. Os demais grupos e ROMs comerciais permanecem seriais.

## Alternativas

- Aumentar Cargo acima de 10 não aumenta a capacidade do host disponível e pode elevar a contenção.
- Rodar todas as 51 compilações em paralelo multiplica memória e, sem limitar LLD por processo, cria dezenas de threads concorrentes.
- Paralelizar LeafGreen antes de medir RSS aumenta o risco de pressão de memória no Mac mini.

## Consequências

O pool usa paths/logs separados por fixture e não muda semântica guest ou manifesto. Em três execuções alternadas no mesmo host e com o binário já compilado, [scripts/verify-gba020-lsl.sh](../scripts/verify-gba020-lsl.sh) mediu medianas de 1,68 s com quatro workers × duas threads, 1,77 s com cinco × duas e 1,48 s com sete × uma. Sete workers × uma thread reduziram o wall time em 11,9% contra quatro × duas e elevaram a razão de CPU acumulada por wall time de 1,49 para 1,91 CPUs. O maior RSS reportado por `time -lp` foi cerca de 39–41 MB por comando; essa medição não soma o RSS simultâneo de todos os workers. Os vetores completos e limitações estão em [gba020-thumb-ldmia-macos-arm64.json](../knowledge/evidence/gba020-thumb-ldmia-macos-arm64.json).

O gate completo passou com o padrão de sete workers × uma thread LLD. A rodada fria incluiu 73 s para compilar o binário e não é comparável ao gate aquecido. Nenhuma alegação de aceleração da compilação de uma ROM comercial é feita.

### Reavaliação — 2026-10-14

O gate completo após SPEC-036 terminou em 128,60 s com média agregada de 3,94 CPUs, RSS máximo de 2,21 GB e zero swaps; uma amostra durante link atingiu cerca de 8,4 CPUs. Isso mede a suíte inteira, que ainda tem fases seriais. Um probe alternado de três compilações O2 Tiny Counter com `GBA_RECOMP_LINK_THREADS=1` e três com `=10` ficou entre 0,21 e 0,27 s por comando; o fixture tem 256 bytes e seis instruções AOT e não mostrou ganho confiável. O número de jobs Cargo permanece dez, que já é o teto do Mac mini M4. A busca do CFG e a construção do módulo LLVM de uma ROM são seriais; threads LLD só atuam no estágio final. Portanto, não aumentamos o fan-out do gate sem uma medição representativa. Para paralelizar uma única ROM será necessário um desenho separado e um pacote grande que permita medir CPU, tempo e RSS. Evidência e limitações estão no [registro SPEC-036](../knowledge/evidence/gba020-thumb-stmia-macos-arm64.json).

Após SPEC-037, os oito casos independentes Thumb ADD/SUB também usam o pool, com padrão oito workers × uma thread LLD; o limite de dez slots ainda permite trocar configuração sem oversubscription deliberado. O gate completo passou em 48,85 s em uma execução aquecida, mas o conjunto de fixtures mudou em relação à medição anterior de 128,60 s, portanto esses wall times não formam um A/B. Com relink frio do binário antes do gate, a execução atual passou em 130,22 s, user 488,33 s, sys 15,26 s, RSS máximo de 2.083.323.904 bytes reportado e zero swaps; a amostra fria inclui o link de Cargo e não é comparável à quente. No grupo ADD/SUB, os vetores brutos foram: 7 workers × 1 thread, wall 2,36/2,04/2,07 s, user 2,23/2,23/2,24 s, sys 1,20/0,98/0,99 s e RSS 40.419.328/40.615.936/40.517.632 bytes; 1 worker × 10 threads, wall 4,24/4,23/4,22 s, user 1,86/1,86/1,86 s, sys 0,80/0,78/0,78 s e RSS 38.584.320/38.600.704/38.617.088 bytes. As medianas foram 2,07 s e 4,23 s (51% menos wall time no primeiro caso). Isso mostra throughput melhor ao compilar fixtures independentes, não aceleração de uma ROM comercial. `time -lp` RSS não soma workers simultâneos; não houve cap rígido do scheduler nem captura de percentual de CPU. O harness temporário foi gerado extraindo `verify_thumb_addsub()` e as oito chamadas desse script; o wrapper completo, ordem exata das seis amostras e hash do harness não persistiram. Assim, os vetores são evidência histórica com reprodutibilidade limitada. Os oito workers foram escolhidos porque o grupo tem oito casos; não há alegação de que oito seja mais rápido que sete. Os dados e limitações estão no [relatório SPEC-037](../knowledge/evidence/gba020-thumb-ldmia-rom-cases-macos-arm64.json).

SPEC-043 agenda dez modos Thumb Format 3, nove modos LDR word, dez modos LDRH (incluindo a fixture `0x8821` inferida dos bytes de LeafGreen), os grupos existentes de LSL/LSR e ADD/SUB e, por ADR-032, o lote STRB/LDRB de dez modos. Os grupos seguem sequenciais. Em uma amostra pareada por configuração com o mesmo código e binário compilado, o gate completo passou em 81,51 s com 1 worker × 10 threads LLD e em 46,62 s com 10 workers × 1 thread LLD: 42,8% menos wall no segundo caso. A soma das durações dos cinco grupos cronometrados foi 34,63 s contra 10,26 s (3,38×). Cada grupo inclui gerar fixtures, compilar, executar e checar manifesto. O resultado é uma única amostra por configuração, não uma mediana; `time -lp` reporta RSS máximo do processo invocador, não a soma dos workers simultâneos; swaps foram zero e não foi provada saturação contínua dos dez CPUs. A evidência com comandos, resultados, logs e hashes está em [gba020-thumb-strict-aot-parallelism-macos-arm64.json](../knowledge/evidence/gba020-thumb-strict-aot-parallelism-macos-arm64.json). A busca do CFG e a emissão LLVM de uma ROM comercial continuam seriais; essa medição não demonstra ganho para LeafGreen.

### Reavaliação — SPEC-047 e STRB/LDRB

O gate passou com orçamento detectado de dez: Cargo recebe `--jobs 10` e o único binário `gba-recomp` liga com LLD 10; workers de fixtures usam no máximo dez × LLD 1; links seriais recebem LLD 10. Os seis grupos correm em sequência e `max_phase_slots=10`. A SPEC-047 registra a detecção com `sysctl`/`nproc`, limite redutível e falha antecipada de configurações fora do orçamento. Três rodadas alternadas do grupo sintético STRB/LDRB, no mesmo binário e com verificações de estado/manifesto O0/O2, deram medianas de 5,5067 s com 1×10 e 2,1146 s com 10×1, redução de 61,6% no wall do grupo. O gate completo aquecido passou em 44,55 s; sua execução serial de equivalência passou em 82,61 s, uma amostra por configuração e sem comparação de throughput do gate completo. Os dados de grupo, logs seriais/paralelos, hashes e limitações estão em [gba020-cpu-budget-accelerated-gate.json](../knowledge/evidence/gba020-cpu-budget-accelerated-gate.json). Não houve claim de aceleração para ROM comercial: LeafGreen falha durante discovery e discovery/emissão de uma ROM permanecem seriais.

## Critérios para revisitar

- Mediana serial versus pool deixa de melhorar ou aumenta excessivamente o RSS.
- Evidência de que outros grupos independentes têm footprint de memória aceitável.
- O número de CPUs lógicas ou a toolchain LLVM do host mudar.

## Referências

[ROADMAP](../ROADMAP.md), [README](../README.md), [gate GBA-020](../scripts/verify-gba020.sh), `crates/gba-recomp/src/main.rs`.
