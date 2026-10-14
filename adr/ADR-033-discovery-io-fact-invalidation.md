# ADR-033 — Preservar fatos I/O em stores fora da região I/O

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

A análise estática de LeafGreen observou uma store para Palette RAM em B3B8, corrigida em `96687d8`, e depois uma STRH para `0x04000204` (WAITCNT) em B3C4. WAITCNT é MMIO não modelado pelo runtime atual, então o clear conservador apaga IE/IF/IME. A fonte mGBA fixada separa a escrita WAITCNT dos latches: ela altera wait states, prefetch e ciclos ativos, mas não grava IE/IF/IME diretamente.

O `AbstractState` recebe finite sets de endereços. O fluxo atual retorna no primeiro endereço não-RAM; isso pode deixar de considerar outra alternativa de store que também atinja um latch. Portanto, a correção precisa reunir os efeitos de todas as alternativas, e não somente mudar um limite.

## Decisão

Conforme [SPEC-048](../specs/SPEC-048-discovery-io-fact-invalidation.md), classificar `[0x04000000, 0x05000000)` como região de decode 0x04 / possível I/O, distinguindo-a da janela ordinária de registradores de 0x400 bytes. Classificar o intervalo depois de aplicar a normalização de alinhamento da instrução. Endereços comprovadamente fora dessa região preservam IE/IF/IME, embora continuem limpando `memory_words` quando AMEM-04 assim exige. Para bytes de IE/IF/IME, invalidar os latches sobrepostos. Para o halfword exato WAITCNT `[0x04000204,0x04000206)`, preservar apenas os fatos abstratos IE/IF/IME; o efeito temporal do registrador permanece fora do modelo. Se qualquer byte de uma alternativa pertencer a outra região MMIO não modelada, invalidar todos. O join é por latch e `Unknown` em qualquer alternativa domina. EA desconhecido, overflow e alternativas não limitadas continuam invalidando todos. A transferência deve visitar cada alternativa e combinar efeitos sem depender da ordem.

Esta decisão altera somente a precisão da descoberta estática no perfil atual sem eventos. Não faz store em Palette RAM ou I/O não modelado ser compilável. Se IRQ, DMA, timers ou fontes de eventos forem adicionados, rever as regras de preservação antes de propagar fatos através desses efeitos.

## Alternativas

- Limpar todos os fatos I/O para qualquer store não modelado: simples, mas funde o domínio de RAM com o de I/O e torna Unknown um latch não endereçado.
- Manter os latches para todo store fora dos três endereços conhecidos: rejeitada; endereços em `0x04xxxxxx` ainda podem conter outros efeitos não modelados.
- Preservar os latches após qualquer acesso `0x04xxxxxx` que não atinja IE/IF/IME: rejeitada; WAITCNT é uma exceção documentada por endereço, não uma regra geral para MMIO desconhecido.
- Tratar apenas `[0x04000000,0x04000400)` como possível I/O: rejeitada por subestimar offsets especiais dentro da região `0x04xxxxxx`, incluindo `0x04000800` na referência fixada.

## Consequências e validação pendente

A decisão recupera a prova de IF zero em estados posteriores a stores comprovados em Palette RAM e WAITCNT. A sombra de RAM permanece conservadora; lowering e runtime permanecem inalterados. `cargo test --locked --workspace --jobs 10` passou com 64 testes em `gba-ir` e 101 em `gba-recomp`; o binário strict-AOT foi reconstruído e a descoberta estática pós-WAITCNT foi registrada em [KNO-004](../knowledge/KNO-004-rom-intake.md). A análise ainda termina no BX de LeafGreen e não é execução guest. O runtime ainda retorna fault de região para WAITCNT.

## Referências

- [SPEC-048](../specs/SPEC-048-discovery-io-fact-invalidation.md).
- [SPEC-034](../specs/SPEC-034-abstract-memory-discovery.md), AMEM-04.
- [SPEC-045](../specs/SPEC-045-thumb-ie-ime-latches.md), IOM-06/07/08.
- [SRC-41](../knowledge/KNO-001-evidence.md).
- [SRC-42](../knowledge/KNO-001-evidence.md).
- [KNO-004](../knowledge/KNO-004-rom-intake.md).

