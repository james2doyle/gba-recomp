# ADR-012 — PUSH Thumb usa helper de bloco na ABI GBA v3

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Depois de adicionar BX ARM/Thumb, a descoberta estática LeafGreen chegou a `0x080003A4`, encoding Thumb `0xB5F0`, provavelmente `PUSH {r4-r7,lr}`. A operação decrementa SP e escreve várias palavras por um só efeito guest. Chamar repetidamente a helper de `STR` pode deixar escrita parcial se um endereço posterior sair da RAM física aceita.

## Decisão

Emitir uma chamada à helper `gba_push_thumb_v3` da ABI GBA v3. A helper recebe a máscara guest de registradores, valida o intervalo inteiro em EWRAM/IWRAM físicas antes de alterar memória, escreve os valores em ordem crescente de registrador e atualiza SP somente após validar e armazenar todos. A função é uma extensão aditiva: layouts e versões de structs v1–v3 permanecem fixos. O código gerado continua usando endereços guest inteiros e a ABI GBA para todo acesso à memória.

## Consequências

Falhas de range, alinhamento, runtime ou ABI são reportadas por `GbaExitV3` sem mutação parcial do stack ou do estado CPU. Lista vazia e demais formatos de transferência múltipla são recusados. POP e outras transferências continuam exigindo contratos próprios; isso não implementa o bus geral.

## Critérios para revisitar

Substituir a helper específica por uma ABI de blocos mais geral quando houver contratos para LDM/STM ARM e POP Thumb, preservando validação prévia de intervalo e sem expor ponteiros host na ABI guest.

## Referências

[SPEC-019](../specs/SPEC-019-thumb-push.md), [SPEC-016](../specs/SPEC-016-ram-word-stores.md), [ADR-009](ADR-009-versioned-bus-context.md), [SRC-20/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
