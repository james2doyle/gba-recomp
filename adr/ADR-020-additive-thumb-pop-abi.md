# ADR-020 — Helper aditivo de POP Thumb na ABI v3

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

SPEC-033 acrescenta POP Thumb com registradores r0–r7. O runtime já oferece `gba_push_thumb_v3`, que executa uma operação de stack em uma única chamada, mas não tem load word. A ABI v3 não deve alterar layout para representar a operação ou seu resultado.

## Decisão

Adicionar o símbolo `gba_pop_thumb_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t register_list)`. O helper aceita apenas lista não vazia de r0–r7, valida SP alinhado e todo o intervalo em uma única faixa física EWRAM/IWRAM, carrega os valores little-endian em temporários locais e então grava os registradores e SP. Exigir SP alinhado é uma restrição explícita do recorte local v1, não uma afirmação sobre a semântica arquitetural de POP desalinhado. A implementação valida todas as condições antes de mutar CPU e não escreve na memória guest. Em sucesso, o relatório recebe reason CONTINUE, PC guest atual, endereço zero e fault NONE. Em falha, o fault code mapeia 1:1 para runtime nulo, ABI incompatível, memória nula, lista inválida, alinhamento ou região; `fault_address` é zero para runtime nulo, SP para ABI/memória/lista/alinhamento e primeiro EA fora da faixa para região.

Faults preservam r0–r15, CPSR, ciclos e memória. `GbaExitV3` reporta o PC corrente, o primeiro endereço guest inválido e um dos códigos existentes. `rt == NULL` mantém o PC sentinela zero. `out == NULL` retorna fault sem alteração. A assinatura é aditiva; layouts ABI v1–v4, `GBA_IR_VERSION` e manifestos não mudam. O lowering LLVM faz uma chamada e só avança PC/ciclo se o helper retornar sucesso.

## Alternativas

- Ler uma palavra por helper e fazer múltiplas chamadas LLVM exige guardar todos os resultados temporários gerados e coordenar atomicidade entre chamadas.
- Alterar `GbaExitV3` para carregar valores quebraria o layout e misturaria dados da operação com status/fault.
- Converter endereços guest em ponteiros host viola a separação de memória guest e host.
- Aceitar POP com PC exigiria contrato para carregar PC indiretamente, aplicar as regras de alinhamento de destino Thumb e fazer dispatch AOT pelo par `(modo, PC)`. POP com PC permanece em Thumb; não implica troca de modo.

## Consequências

O caminho AOT de POP fica concentrado em uma chamada ABI, com commits de registradores e SP atômicos para o recorte físico suportado. O helper é específico a esta operação; futuros loads word precisarão de contrato próprio. Mirrors, MMIO, POP com PC e timing geral continuam fora deste incremento.

## Critérios para revisitar

- Implementar um load word genérico com semântica de bus compatível para mais formatos de leitura.
- Acrescentar POP com PC depois de definir alinhamento Thumb, dispatch dinâmico e fechamento AOT do destino.
- Ampliar as faixas de memória após existir bus GBA versionado.

## Referências

[SPEC-033](../specs/SPEC-033-thumb-pop.md), [SPEC-019](../specs/SPEC-019-thumb-push.md), [ADR-009](ADR-009-versioned-bus-context.md), [SRC-34/KNO-001](../knowledge/KNO-001-evidence.md).
