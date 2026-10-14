# ADR-033 — Preservar fatos I/O em stores fora da região I/O

- Estado: Proposed
- Data: 2026-10-14
- Supersede: —

## Contexto

A análise de LeafGreen observa stores com EA `0x05000000` em B3B8/B3C4. Hoje `invalidate_store` limpa `memory_words` para stores fora de EWRAM/IWRAM e chama uma rotina que também limpa todos os latches I/O sempre que o EA não cabe na janela `[0x04000000,0x04000400)`. A fonte mGBA fixada separa o byte superior `0x04` (I/O) de `0x05` (Palette RAM). O clear indiscriminado transforma IE/IF/IME, inicialmente zero, em Unknown sem que o endereço alcance a região I/O.

O `AbstractState` recebe finite sets de endereços. O fluxo atual retorna no primeiro endereço não-RAM; isso pode deixar de considerar outra alternativa de store que também atinja um latch. Portanto, a correção precisa reunir os efeitos de todas as alternativas, e não somente mudar um limite.

## Decisão proposta

Conforme [SPEC-048](../specs/SPEC-048-discovery-io-fact-invalidation.md), classificar `[0x04000000, 0x05000000)` como região de decode 0x04 / possível I/O, distinguindo-a da janela ordinária de registradores de 0x400 bytes. Classificar o intervalo depois de aplicar a normalização de alinhamento da instrução. Endereços comprovadamente fora dessa região preservam IE/IF/IME, embora continuem limpando `memory_words` quando AMEM-04 assim exige. Quando todo byte da interseção de um store com a região de decode estiver coberto por IE/IF/IME, invalidar os latches sobrepostos; se houver qualquer byte de I/O não rastreado, invalidar todos. O join é por latch e `Unknown` em qualquer alternativa domina. EA desconhecido, overflow e alternativas não limitadas continuam invalidando todos. A transferência deve visitar cada alternativa e combinar efeitos sem depender da ordem.

Esta decisão altera somente a precisão da descoberta estática no perfil atual sem eventos. Não faz store em Palette RAM ou I/O não modelado ser compilável. Se IRQ, DMA, timers ou fontes de eventos forem adicionados, rever as regras de preservação antes de propagar fatos através desses efeitos.

## Alternativas

- Limpar todos os fatos I/O para qualquer store não modelado: simples, mas funde o domínio de RAM com o de I/O e torna Unknown um latch não endereçado.
- Manter os latches para todo store fora dos três endereços conhecidos: rejeitada; endereços em `0x04xxxxxx` ainda podem conter outros efeitos não modelados.
- Tratar apenas `[0x04000000,0x04000400)` como possível I/O: rejeitada por subestimar offsets especiais dentro da região `0x04xxxxxx`, incluindo `0x04000800` na referência fixada.

## Consequências e validação pendente

A decisão pode recuperar a prova de IF zero em estados posteriores a stores comprovados em Palette RAM. A sombra de RAM permanece conservadora; lowering e runtime permanecem inalterados. A aceitação depende de testes unitários para endereços singulares e alternativos, fronteiras/overflow, e de uma nova análise estática da ROM fixada. Nenhum resultado deve ser descrito como execução guest.

## Referências

- [SPEC-048](../specs/SPEC-048-discovery-io-fact-invalidation.md).
- [SPEC-034](../specs/SPEC-034-abstract-memory-discovery.md), AMEM-04.
- [SPEC-045](../specs/SPEC-045-thumb-ie-ime-latches.md), IOM-06/07/08.
- [SRC-41](../knowledge/KNO-001-evidence.md).
- [KNO-004](../knowledge/KNO-004-rom-intake.md).

