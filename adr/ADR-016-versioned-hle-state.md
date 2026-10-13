# ADR-016 — ABI v4 para estado de HLE RegisterRamReset

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

LeafGreen alcançou estaticamente Thumb 0xDF01, inferido como SWI #1. A ABI v3 carrega apenas EWRAM/IWRAM e não tem espaço versionado para palette RAM, VRAM, OAM, IO ou wave RAM. RegisterRamReset altera essas áreas conforme a máscara em r0. Alterar GbaMemoryV1 ou GbaRuntimeV3 quebraria contratos existentes.

## Decisão

Adicionar ABI v4 como wrapper aditivo: primeiro membro GbaRuntimeV3, ponteiro explícito para GbaMachineStateV1 e versão v4. O estado de máquina inicia com GbaMemoryV1 como primeiro membro e inclui as demais regiões necessárias à HLE. O ponteiro legado GbaRuntimeV3.memory aponta para esse membro inicial; runtime e helper validam igualdade e ambas as versões.

Emitir gba_block_v4 para pacotes novos. Helpers de store/PUSH existentes continuam recebendo o prefixo v3 e ABI v2 da CPU. A HLE usa um helper v4 versionado, linkado estaticamente; nenhuma função host é chamada por endereços guest e nenhuma semântica cai para interpretação/JIT.

## Alternativas consideradas

- Ampliar GbaMemoryV1 ou GbaRuntimeV3: rejeitada por alterar tamanho/offsets congelados.
- Recuperar o estado estendido por cast de GbaMemoryV1*: rejeitada porque não valida alocação nem versão do contexto.
- Limpar só EWRAM/IWRAM e reportar sucesso: rejeitada porque omite efeitos visíveis de RegisterRamReset.
- Tratar qualquer SWI como NOP ou fallback: rejeitada por produzir semântica guest incorreta.

## Consequências

ABI v1–v3 continuam compatíveis e seus helpers podem ser reaproveitados pelo prefixo. Pacotes novos declaram ABI v4 e versão HLE no manifesto. A extensão guarda as áreas e latches tocados por SWI #1; não implementa ainda o bus nem a execução de vídeo/áudio/periféricos.

## Referências

[SPEC-023](../specs/SPEC-023-thumb-swi-register-ram-reset.md), [SPEC-004](../specs/SPEC-004-ir-llvm.md), [SPEC-007](../specs/SPEC-007-bios-cart-state.md), [SRC-24/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
