# SPEC-048 — Invalidação de fatos I/O por stores na descoberta

Estado do documento: Draft. Implementação pendente. Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

A descoberta mantém uma sombra abstrata de `IE`, `IF` e `IME`, inicialmente zero no runner atual, conforme [SPEC-045](SPEC-045-thumb-ie-ime-latches.md). A descoberta corrigida de LeafGreen registra `STRH` em B3B8 e B3C4 com EA singleton `0x05000000`, inferida da ROM de SHA-256 fixado em [KNO-004](../knowledge/KNO-004-rom-intake.md). O código chama a invalidação de store não modelado e apaga todos os fatos I/O por o endereço estar abaixo/fora da janela de 1 KiB examinada. A referência mGBA fixada separa a região de I/O `0x04xxxxxx` da região Palette RAM `0x05xxxxxx`; nenhum desses acessos à Palette RAM é um acesso aos latches IE/IF/IME.

Isto é evidência do estado abstrato do compilador, não execução guest. O contrato vale somente para a descoberta atual: runner com `interrupt_model: "none"`, sem fontes locais de eventos, DMA ou IRQ. Ele não amplia o lowering AOT nem suporte de runtime a Palette RAM, outros MMIO ou mirrors; um store ainda não coberto pelo runtime deve continuar faultando conforme os contratos existentes.

Nesta spec, “região de decode 0x04” ou “possível I/O” significa o intervalo `[0x04000000, 0x05000000)` identificado pelo byte mais significativo. A janela ordinária de registradores descrita por `GBA_SIZE_IO` tem 0x400 bytes; offsets além dela, inclusive `0x04000800`, são tratados apenas como endereços que podem ter efeitos I/O não modelados, não como registradores válidos ou suportados.

## Requisitos normativos

- IINV-01: a classificação conservadora de endereços de I/O da descoberta MUST tratar a região guest cujo byte mais significativo é `0x04`, isto é, `[0x04000000, 0x05000000)`, como região que pode conter efeitos I/O. A região de `0x05xxxxxx` é Palette RAM e não deve ser confundida com I/O. Isso não declara suporte aos registradores I/O fora de IE/IF/IME nem afirma alias/mirror fora das regiões documentadas.
- IINV-02: para store cujo conjunto de endereços efetivos é finito, não vazio e limitado por `MAX_CONSTANT_TARGETS`, cada endereço MUST ser normalizado com a máscara de alinhamento já aplicada pelo decodificador e cada intervalo semiaberto `[EA, EA + largura)` de 1, 2 ou 4 bytes MUST ser avaliado com soma verificada. Se o intervalo não intersecta `[0x04000000, 0x05000000)`, essa alternativa MUST preservar os fatos `IE`, `IF` e `IME`. O resultado da transferência MUST incluir todas as alternativas e independer da ordem em que são percorridas.
- IINV-03: para uma alternativa cujo intervalo intersecta a região de decode 0x04, a descoberta MUST calcular os bytes dessa interseção. Latch rastreado ocupa todo o seu halfword: IE `[0x04000200, 0x04000202)`, IF `[0x04000202, 0x04000204)` e IME `[0x04000208, 0x0400020A)`. Se todo byte da interseção estiver coberto pela união desses intervalos, marcar `Unknown` cada latch intersectado. Isso inclui escrita de byte em qualquer lado do latch e intervalos alinhados que cubram integralmente mais de um latch, como um word em `0x04000200`, que cobre IE e IF.
- IINV-04: se algum byte da interseção com a região de decode 0x04 não estiver coberto pelos intervalos IE/IF/IME, a descoberta MUST limpar conservadoramente todos os fatos I/O, mesmo que o mesmo store também sobreponha um latch rastreado. Isso inclui intervalos parciais que atravessam um latch e bytes MMIO não rastreados, como `[0x040001FE, 0x04000202)`, e stores em offsets não modelados, inclusive `0x04000800`. Esses offsets podem ter efeitos desconhecidos e continuam fora do suporte de runtime. A regra não substitui a transferência especial de STRH IE/IME sob IF zero de IOM-07.
- IINV-05: os efeitos sobre as alternativas finitas são combinados conservadoramente: qualquer alternativa que alcance bytes I/O não modelados invalida todos os latches; caso contrário, a união dos latches sobrepostos torna-se `Unknown`, e alternativas comprovadamente fora da região 0x04 não adicionam invalidações. Quando o EA é `Unknown`, vazio, tem mais alternativas que o limite, a largura não é 1/2/4 ou qualquer intervalo transborda o espaço u32, a descoberta MUST limpar todos os fatos I/O. Nenhuma alternativa possível pode ser ignorada por retorno antecipado durante a iteração.
- IINV-06: a invalidação de `memory_words` MUST continuar independente e compatível com AMEM-04: store fora de EWRAM/IWRAM suportadas limpa a sombra de RAM como antes. Preservar IE/IF/IME para store comprovadamente fora da região `0x04xxxxxx` não preserva fatos de RAM nem autoriza propagar conteúdo de Palette RAM.
- IINV-07: esta mudança MUST afetar somente fatos usados pela descoberta estática. Não altera semântica guest, helper/runtime, ABI, ciclos, manifesto ou decisão strict-AOT; não adiciona fallback interpretativo/JIT.
- IINV-08: se o modelo de eventos, IRQ, DMA ou efeitos assíncronos do runner mudar, a regra de preservação MUST ser revista antes de aceitar essa extensão. Esta spec não afirma que uma escrita fora de `0x04xxxxxx` nunca possa correlacionar-se a evento em hardware real.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| STRH em `0x05000000` com latches zero e sombra de RAM presente | Latches IE/IF/IME permanecem zero; sombra de RAM é limpa conforme AMEM-04 |
| Store conhecido em EWRAM/IWRAM | Fatos de RAM sobrepostos são removidos; latches permanecem |
| Store em I/O não rastreado, inclusive offset `0x04000800` | Todos os fatos de I/O tornam-se Unknown |
| Store cujo intervalo sobrepõe IE, IF, IME, ou mais de um deles | Só os latches sobrepostos tornam-se Unknown |
| Store que sobrepõe latch rastreado e bytes de I/O não rastreados | Todos os fatos de I/O tornam-se Unknown |
| Store de byte em cada byte de IE, IF ou IME | O halfword inteiro sobreposto torna-se Unknown |
| Endereço em `0x04FFFFFF` com largura 2 ou 4 | Todos os fatos de I/O tornam-se Unknown por tocar byte não modelado em 0x04 |
| Endereço em `0x05000000` ou `0x05000002` | Preserva IE/IF/IME; ainda invalida a sombra RAM conforme AMEM-04 |
| Conjunto finito com alvos fora de I/O e em I/O | Resultado inclui todas as alternativas e independe da ordem |
| Alternativas `{IE, IME}` ou `{Palette RAM, IE}` | Invalida IE/IME, respectivamente; preserva latches não atingidos |
| EA Unknown, conjunto vazio/acima do limite, ou overflow | Fatos de I/O são todos invalidados |
| STRH IE/IME com IF provado zero | Mantém a regra IOM-07; fixture verifica sombra preservada |
| STRH IE/IME com IF Unknown/não zero | Mantém invalidação conservadora de sombra de AMEM/IOM-07 |
| LeafGreen, SHA-256 de KNO-004, O2 | Registrar se a rota B50→B58 chega ao BX em B6A com destino finito; qualquer nova fronteira ou pacote deve ser registrado sem afirmar execução da ROM |

## Fontes e relações

- [SPEC-034 — memória abstrata](SPEC-034-abstract-memory-discovery.md), AMEM-04.
- [SPEC-045 — latches IE/IF/IME](SPEC-045-thumb-ie-ime-latches.md), IOM-06/07/08.
- [ADR-033 — fatos I/O para stores fora da região I/O](../adr/ADR-033-discovery-io-fact-invalidation.md).
- [SRC-41 — semântica de memória e latches de referência](../knowledge/KNO-001-evidence.md).
- [Evidência estática LeafGreen](../knowledge/evidence/leafgreen-bx-r0-audit-20261014.json).
- mGBA commit `143adf5aa2a60c6036810b0cd72445e13d74b0e6`: `include/mgba/internal/gba/memory.h:21-73,82` declara regiões, tamanhos e `BASE_OFFSET=24`; `include/mgba/internal/gba/io.h:159-163` registra offsets WAITCNT estendidos `0x800`/`0x802`; `src/gba/memory.c:541-568,656-684,831-900` despacha regiões `0x04` (I/O) e `0x05` (Palette RAM) separadamente. A referência é observação de fonte, não reutilização de código nem prova independente de hardware.

