# ADR-015 — BL Thumb permanece em duas instruções com alvo estático

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Depois de implementar MOVS Thumb imediato, a compilação estática de LeafGreen chegou a `0x080003AC`, encoding `0xF1E3`; o halfword seguinte é `0xFC0E`. A chamada Thumb usa o LR em duas etapas e retorna ao par de instruções seguinte, normalmente por `BX LR`.

## Decisão

Representar o prefixo e o sufixo BL como instruções Thumb AOT separadas. O prefixo calcula e grava o LR parcial; a descoberta valida o LR ao chegar ao sufixo, calcula o alvo e fecha o destino na ROM. O sufixo grava o LR de retorno com bit Thumb e despacha ao destino já validado. `BX LR` segue os registradores efetivamente analisados e os estados são mesclados no continuation; não inferir clobbers de uma ABI de chamadas externa.

## Consequências

O relatório conserva dois endereços guest e dois passos abstratos por chamada. Sufixos sem prefixo adjacente, estados incompatíveis e alvos fora da ROM falham antes da emissão do pacote. Não há chamadas dinâmicas, BLX ou fallback implícito.

## Critérios para revisitar

Adicionar B/BL Thumb curto, BLX ou chamadas por código gerado em RAM com contratos próprios. Introduzir resumos interprocedurais apenas se forem provados contra o fluxo concreto do guest, sem substituir a semântica arquitetural do LR.

## Referências

[SPEC-022](../specs/SPEC-022-thumb-bl-pair.md), [SRC-23/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md), [SPEC-018](../specs/SPEC-018-arm-thumb-branch-exchange.md).
