# ADR-021 — Paralelismo limitado no gate GBA-020

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

O Mac mini M4 disponível para este projeto tem 10 CPUs lógicas. Cargo já usa 10 jobs e o link do próprio compilador usa 10 threads LLD. O gate GBA-020 compila muitos pacotes de fixture em sequência; há grupos independentes que podem ocupar CPUs durante esse trecho, mas cada processo também otimiza LLVM e inicia um link.

## Decisão

Grupos independentes de Thumb LSL, ADD/SUB, Format 3, LDR word e LDRH podem usar o scheduler limitado especificado em [SPEC-043](../specs/SPEC-043-bounded-fixture-parallelism.md). O build do binário do recompilador termina antes dos workers e cada grupo é aguardado antes do próximo.

`GBA020_JOBS` aceita 1, 2, 4, 5, 7, 8, 9 ou 10 workers. Os defaults são, respectivamente, 10, 4, 2, 2, 1, 1, 1 e 1 thread LLD por pacote. `GBA020_LINK_THREADS` pode substituir esse valor, desde que o produto de workers e threads não passe de 10. O gate valida de 1 a 10 threads para cada link. LSL/LSR, ADD/SUB, Thumb Format 3 (dez fixtures), LDR word (nove modos) e LDRH (dez modos, incluindo a codificação `0x8821` observada estaticamente em LeafGreen) usam estes workers; os outros grupos e ROMs comerciais permanecem seriais.

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

SPEC-043 acrescenta os dez modos Thumb Format 3, nove modos LDR word e dez modos LDRH ao scheduler e inclui a fixture `0x8821` exata observada em LeafGreen no lote LDRH. A configuração padrão permite dez workers de fixture, cada um com uma thread LLD; os grupos continuam sequenciais entre si. O resultado de throughput e a pressão de memória agregada permanecem pendentes de medição reproduzível com o gate atualizado.

## Critérios para revisitar

- Mediana serial versus pool deixa de melhorar ou aumenta excessivamente o RSS.
- Evidência de que outros grupos independentes têm footprint de memória aceitável.
- O número de CPUs lógicas ou a toolchain LLVM do host mudar.

## Referências

[ROADMAP](../ROADMAP.md), [README](../README.md), [gate GBA-020](../scripts/verify-gba020.sh), `crates/gba-recomp/src/main.rs`.
