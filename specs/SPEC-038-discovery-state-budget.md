# SPEC-038 — Orçamento de estados disjuntivos para loops finitos

Estado do documento: Accepted. Complementa o budget da SPEC-037 sem alterar o domínio abstrato. Implementação e evidência: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

A descoberta da LeafGreen de SHA-256 `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825` chega a um loop Thumb em `0x08000996`. A instrumentação local observou 64 estados exatos com `r2=0..63`; o próximo estado tem `r2=64`. Os halfwords no loop são `ADDS r2,#1` (`0x080009A2`), `CMP r2,#95` (`0x080009A4`) e `BLE 0x08000996` (`0x080009A6`). A rejeição atual decorre do budget de 64 alternativas por PC antes de a análise poder alcançar o fixed point desse loop finito.

Esta tranche eleva somente o limite de estados completos disjuntivos por `(modo, PC)` a 128. O limite global permanece 100.000 estados, a sombra de memória permanece em 64 words e a especialização LDMIA ROM permanece limitada a 16 bases. A mudança retém igualdade exata e erros explícitos acima dos limites.

## Fora de escopo

- Juntar estados, apagar correlações, introduzir intervalos/strides ou converter estados em `Unknown` para evitar o budget.
- Aumentar o limite global de estados, o limite da sombra de memória ou o limite de casos ROM.
- Paralelizar discovery de um único ROM, alterar semântica guest ou habilitar interpretação/JIT.
- Declarar que LeafGreen compila, executa ou é compatível antes de completar gates independentes.

## Termos e invariantes

- `complete_state`: tupla exata de modo/PC, registradores, sombra de memória e fato CMP pendente, conforme SPEC-037.
- `per_pc_budget`: máximo de complete states únicos em um `(modo, PC)`.
- `global_budget`: máximo de complete states únicos em todo o worklist.
- Estados iguais continuam deduplicados; estados diferentes nunca se subsumem.

## Requisitos normativos

- DSB-01: Discovery MUST reter estados completos disjuntivos e MUST deduplicar somente estados iguais. O limite por `(modo, PC)` MUST ser 128. Ao tentar inserir o 129º estado distinto, MUST falhar com diagnóstico `discovery_state_alternative_limit` que informe PC, modo e limite, sem merge ou fallback.
- DSB-02: O limite global MUST permanecer 100.000 estados únicos. Excedê-lo MUST falhar com `discovery_global_state_limit`. Os limites de 64 palavras de sombra e 16 bases ROM MUST permanecer inalterados.
- DSB-03: Uma fixture Thumb inicializa `r2=0`, incrementa `r2`, compara imediatamente com 95 e usa BLE para retornar ao cabeçalho. Discovery MUST convergir, registrar exatamente os valores de `r2=0..95` no cabeçalho, registrar ambos os sucessores do branch, e descobrir o caminho de saída para `r2=96`.
- DSB-04: Uma fixture com 129 estados completos distintos no mesmo PC MUST falhar no 129º estado e conservar os 128 estados já admitidos; a mensagem MUST citar o budget 128.
- DSB-05: Os resultados MUST ser determinísticos independente da ordem de inserção. A comparação CMP/branch continua sendo refinada por alternativa conforme SPEC-037, e o lowering AOT MUST continuar verificando CPSR guest em runtime.
- DSB-06: Nenhuma mudança de budget pode mudar a versão da ABI/IR, adicionar fallback interpretado/JIT ou publicar pacote parcial após falha.

## Interfaces, entradas e saídas

Não há alteração de IR, ABI, pacote ou interface de linha de comando. Apenas o budget interno de discovery muda de 64 para 128 alternativas completas por `(modo, PC)`.

## Erros, limites e observabilidade

O diagnóstico de limite permanece `discovery_state_alternative_limit at 0x{pc}: more than 128 complete states at {mode} PC`. O budget global de 100.000 permanece independente. Cada compilação deve registrar tempo, pico de memória quando disponível, próximo limite estático e hash da ROM.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| Loop Thumb de 96 valores | Fixed point alcançado; `r2=0..95` no cabeçalho; branch com taken e fallthrough; saída descoberta |
| 129 estados no mesmo PC | Rejeição explícita no limite 128, sem inserir o 129º |
| Budget global atingido | Regressão da rejeição em 100.000 estados |
| Branch sem CMP adjacente/operando desconhecido | Ambos os sucessores continuam conservadores |
| LeafGreen com hash fixado | Registrar somente a nova fronteira e métricas; sucesso estático não declara execução/compatibilidade |
| Workspace e gate | Testes Rust, fmt, `git diff --check` e gate GBA-020 conforme ambiente macOS arm64 |

## Questões abertas

- Qual é a próxima instrução ou efeito de runtime que limita a compilação estática após `0x08000996`?
- Medições de ROMs maiores justificam uma análise de intervalos/strides ou mais paralelismo no pipeline?

## Referências e evidências

- [SPEC-037 — estados exatos, branches CMP e LDMIA ROM](SPEC-037-thumb-ldmia-rom-cases.md)
- [ADR-025 — budget aumentado de estados completos](../adr/ADR-025-discovery-state-budget.md)
- [KNO-004 — intake e tentativas de LeafGreen](../knowledge/KNO-004-rom-intake.md)
