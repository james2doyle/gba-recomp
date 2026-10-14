# SPEC-048 — Invalidação de fatos I/O por stores na descoberta

Estado do documento: Draft. A invalidação de stores fora de I/O foi implementada em `96687d8`; o refinamento estático de WAITCNT foi implementado em `696b031`. A revisão documental continua pendente. Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

A descoberta mantém uma sombra abstrata de `IE`, `IF` e `IME`, inicialmente zero no runner atual, conforme [SPEC-045](SPEC-045-thumb-ie-ime-latches.md). A invalidação de stores para Palette RAM em B3B8 foi corrigida em `96687d8`; o trace estático então identifica em B3C4 uma STRH com EA singleton `0x04000204` (WAITCNT), que ainda apaga todos os fatos por ser MMIO não modelado. Os endereços e valores são inferidos pela ROM de SHA-256 fixado em [KNO-004](../knowledge/KNO-004-rom-intake.md), e o trace observa estado interno do compilador, não execução guest. A fonte mGBA separa WAITCNT dos latches IE/IF/IME: sua escrita altera wait states, prefetch e ciclos ativos, mas não grava esses latches diretamente.

O contrato vale somente para a descoberta atual: runner com `interrupt_model: "none"`, sem fontes locais de eventos, DMA ou IRQ. Reconhecer WAITCNT aqui significa somente que esse endereço não altera os fatos abstratos de IE/IF/IME nesse modelo; não modela seus efeitos de timing. O lowering AOT atual pode emitir a STRH, mas o runtime ainda rejeita WAITCNT com fault de região. Este refinamento não amplia suporte de runtime a WAITCNT, Palette RAM, outros MMIO ou mirrors; não há fallback interpretativo/JIT.

Nesta spec, “região de decode 0x04” ou “possível I/O” significa o intervalo `[0x04000000, 0x05000000)` identificado pelo byte mais significativo. A janela ordinária de registradores descrita por `GBA_SIZE_IO` tem 0x400 bytes; offsets além dela, inclusive `0x04000800`, são tratados apenas como endereços que podem ter efeitos I/O não modelados, não como registradores válidos ou suportados.

## Requisitos normativos

- IINV-01: a classificação conservadora de endereços de I/O da descoberta MUST tratar a região guest cujo byte mais significativo é `0x04`, isto é, `[0x04000000, 0x05000000)`, como região que pode conter efeitos I/O. A região de `0x05xxxxxx` é Palette RAM e não deve ser confundida com I/O. Isso não declara suporte aos registradores I/O fora de IE/IF/IME nem afirma alias/mirror fora das regiões documentadas.
- IINV-02: para store cujo conjunto de endereços efetivos é finito, não vazio e limitado por `MAX_CONSTANT_TARGETS` (atualmente 16), cada EA MUST primeiro ser normalizado com a máscara de alinhamento já aplicada pelo decodificador; a classificação usa esse endereço efetivo normalizado, não o EA bruto. Cada intervalo semiaberto `[EA normalizado, EA normalizado + largura)` de 1, 2 ou 4 bytes MUST ser avaliado com soma verificada. Se o intervalo não intersecta `[0x04000000, 0x05000000)`, essa alternativa MUST preservar os fatos `IE`, `IF` e `IME`. O resultado da transferência MUST incluir todas as alternativas e independer da ordem em que são percorridas.
- IINV-03: para uma alternativa cujo intervalo intersecta a região de decode 0x04, a descoberta MUST calcular os bytes dessa interseção. Latch rastreado ocupa todo o seu halfword: IE `[0x04000200, 0x04000202)`, IF `[0x04000202, 0x04000204)` e IME `[0x04000208, 0x0400020A)`. WAITCNT ocupa `[0x04000204, 0x04000206)` e, no modelo estático atual sem eventos, é conhecido por não gravar IE/IF/IME. Se todo byte da interseção estiver coberto pela união dos intervalos dos três latches ou WAITCNT, marcar `Unknown` cada latch rastreado intersectado; bytes de WAITCNT preservam IE/IF/IME. Isso inclui escrita de byte em qualquer lado de latch e intervalos que cubram integralmente mais de um latch, como um word em `0x04000200`, que cobre IE e IF. A exceção de WAITCNT não significa que o registrador seja inerte: seus efeitos de timing continuam fora deste modelo.
- IINV-04: se algum byte da interseção com a região de decode 0x04 não estiver coberto pelos intervalos IE/IF/IME ou WAITCNT definidos em IINV-03, a descoberta MUST limpar conservadoramente todos os fatos I/O, mesmo que o mesmo store também sobreponha um latch rastreado. Isso inclui intervalos parciais que atravessam um latch e bytes MMIO não rastreados, como `[0x040001FE, 0x04000202)`, um word em `0x04000204` que também alcança `0x04000206–0x04000207`, e stores em offsets não modelados, inclusive `0x04000800`. Esses offsets podem ter efeitos desconhecidos e continuam fora do suporte de runtime. A regra não substitui a transferência especial de STRH IE/IME sob IF zero de IOM-07.
- IINV-05: os efeitos sobre as alternativas finitas são combinados por latch: o estado final de cada latch é o join dos estados após cada alternativa, e `Unknown` em qualquer ramo domina um valor constante. Assim, qualquer alternativa que alcance bytes I/O não modelados invalida todos os latches; caso contrário, a união dos latches sobrepostos torna-se `Unknown`, bytes WAITCNT não adicionam invalidações e alternativas comprovadamente fora da região 0x04 preservam os fatos. Quando o EA é `Unknown`, vazio, tem mais alternativas que o limite, a largura não é 1/2/4, a normalização não pode ser representada ou qualquer intervalo transborda o espaço u32, a descoberta MUST limpar todos os fatos I/O. Nenhuma alternativa possível pode ser ignorada por retorno antecipado durante a iteração. Stores de palavra desalinhados continuam conservadores para a sombra de RAM conforme AMEM-04; sua invalidação de I/O considera o intervalo produzido pelo EA efetivo entregue a esta transferência.
- IINV-06: a invalidação de `memory_words` MUST continuar independente e compatível com AMEM-04: store fora de EWRAM/IWRAM suportadas limpa a sombra de RAM como antes. Preservar IE/IF/IME para store comprovadamente fora da região `0x04xxxxxx` não preserva fatos de RAM nem autoriza propagar conteúdo de Palette RAM.
- IINV-07: esta mudança MUST afetar somente fatos usados pela descoberta estática. Não altera semântica guest, helper/runtime, ABI, ciclos, manifesto ou decisão strict-AOT; não adiciona fallback interpretativo/JIT.
- IINV-08: se o modelo de eventos, IRQ, DMA ou efeitos assíncronos do runner mudar, a regra de preservação MUST ser revista antes de aceitar essa extensão. Esta spec não afirma que uma escrita fora de `0x04xxxxxx` nunca possa correlacionar-se a evento em hardware real.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| STRH em `0x05000000` com latches zero e sombra de RAM presente | Latches IE/IF/IME permanecem zero; sombra de RAM é limpa conforme AMEM-04 |
| Store conhecido em EWRAM/IWRAM | Fatos de RAM sobrepostos são removidos; latches permanecem |
| Store em I/O sem efeito conhecido nos fatos IE/IF/IME, inclusive offset `0x04000800` | Todos os fatos de I/O tornam-se Unknown |
| Store de byte em `0x04000204` ou `0x04000205`, ou halfword em `0x04000204` | IE/IF/IME são preservados; a sombra de RAM continua independente e conservadora |
| Alternativas `{WAITCNT, IE}` | IE torna-se Unknown; IF e IME permanecem constantes |
| Word em `0x04000204` (WAITCNT e bytes adjacentes não modelados) | Todos os fatos de I/O tornam-se Unknown |
| Store cujo intervalo sobrepõe IE, IF, IME, ou mais de um deles | Só os latches sobrepostos tornam-se Unknown |
| Store que sobrepõe latch rastreado e bytes de I/O não rastreados | Todos os fatos de I/O tornam-se Unknown |
| Store word em `0x04000208` (IME e bytes adjacentes não rastreados) | Todos os fatos de I/O tornam-se Unknown |
| Store de byte em cada byte de IE, IF ou IME | O halfword inteiro sobreposto torna-se Unknown |
| Endereço em `0x04FFFFFF` com largura 2 ou 4 | Todos os fatos de I/O tornam-se Unknown por tocar byte não modelado em 0x04 |
| STRH bruto em `IE + 1`, normalizado para IE | Só IE torna-se Unknown |
| STRH bruto em `0x04000207`, normalizado para `0x04000206` | Todos os fatos de I/O tornam-se Unknown por tocar bytes não modelados |
| Endereço em `0x05000000` ou `0x05000002` | Preserva IE/IF/IME; ainda invalida a sombra RAM conforme AMEM-04 |
| Conjunto finito com alvos fora de I/O e em I/O | Resultado inclui todas as alternativas e independe da ordem |
| Alternativas `{IE, IME}` ou `{Palette RAM, IE}` | Invalida IE/IME, respectivamente; preserva latches não atingidos |
| Alternativas `{Palette RAM, MMIO não rastreado}` em qualquer ordem | Todos os fatos de I/O tornam-se Unknown |
| Exatamente 16 alternativas, incluindo alvos em IE e IME | IE/IME tornam-se Unknown, IF permanece constante |
| EA Unknown, conjunto vazio, 17 alternativas, largura inválida ou overflow | Fatos de I/O são todos invalidados |
| STRH IE/IME com IF provado zero | Mantém a regra IOM-07; fixture verifica sombra preservada |
| STRH IE/IME com IF Unknown/não zero | Mantém invalidação conservadora de sombra de AMEM/IOM-07 |
| LeafGreen, SHA-256 de KNO-004, O2 | Registrar se a rota B50→B58 chega ao BX em B6A com destino finito; qualquer nova fronteira ou pacote deve ser registrado sem afirmar execução da ROM |

## Fontes e relações

- [SPEC-034 — memória abstrata](SPEC-034-abstract-memory-discovery.md), AMEM-04.
- [SPEC-045 — latches IE/IF/IME](SPEC-045-thumb-ie-ime-latches.md), IOM-06/07/08.
- [ADR-033 — fatos I/O para stores fora da região I/O](../adr/ADR-033-discovery-io-fact-invalidation.md).
- [SRC-41 — semântica de memória e latches de referência](../knowledge/KNO-001-evidence.md).
- [SRC-42 — semântica de WAITCNT na referência](../knowledge/KNO-001-evidence.md).
- [Evidência estática LeafGreen](../knowledge/evidence/leafgreen-bx-r0-audit-20261014.json).
- mGBA commit `143adf5aa2a60c6036810b0cd72445e13d74b0e6`: `src/gba/io.c:510-525` separa a escrita WAITCNT dos handlers de IE/IF/IME; `src/gba/memory.c:1686-1725` mostra que WAITCNT altera tabelas de wait states, prefetch e ciclos ativos. Isso sustenta somente a preservação dos fatos de latch na descoberta sem eventos; não valida o runtime local, o timing de hardware ou suporte para executar essa escrita.
- A referência mGBA é observação de fonte, não reutilização de código nem prova independente de hardware.

