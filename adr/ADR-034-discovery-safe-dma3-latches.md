# ADR-034 — Reconhecer somente transferências DMA3 imediatas e sem IRQ na discovery

- Estado: Proposed
- Data: 2026-10-14
- Supersede: —

## Contexto

Depois de corrigir invalidações em Palette RAM e WAITCNT, o trace interno da discovery perde IE/IF/IME nas stores da ROM para DMA3. O valor gravado e os endereços são singletons no estado do analisador. A configuração observada representa uma cópia de ROM para IWRAM, 32 bits, imediata e sem IRQ do canal. Stores genéricas em MMIO devem continuar invalidando os latches.

O runner atual declara `io_initialization: "all-zero"` e `interrupt_model: "none"`. O runtime não implementa DMA e não tem scheduler de eventos. Portanto, uma extensão limitada só pode aumentar precisão da descoberta estática; não cria semântica de execução nem suporte para o título.

## Decisão proposta

Adotar [SPEC-049](../specs/SPEC-049-discovery-safe-dma3-latches.md): carregar em cada estado completo uma prova path-sensitive de que os canais DMA permanecem ociosos, a configuração DMA3 conhecida e sua origem/destino. Inicializar a prova somente pela entrada all-zero explícita. Reconhecer os stores Thumb word imediatos em SAD, DAD e CNT quando endereço/valor e canal desabilitado forem provados. Para habilitar, exigir a configuração imediata de 32 bits, incremento, one-shot, sem DRQ/IRQ, fonte integralmente dentro da ROM carregada e destino dentro de IWRAM. Qualquer alternativa, formato diferente ou canal incerto volta à invalidação conservadora.

Em toda store DMA, limpar a sombra abstrata de RAM. No caso aceito, não propagar conteúdo copiado. Tratar a transação como concluída antes da próxima instrução guest no modelo estático, mas não atribuir ciclos ou eventos. SWI #1 pode restaurar o fato ocioso somente quando o flag DMA é singleton e está definido; flags ambíguos perdem a prova.

## Alternativas

- Invalidar em toda store DMA: conserva segurança, mas mantém um falso Unknown numa configuração comprovada sem efeito direto em IE/IF/IME no modelo sem eventos.
- Preservar latches para todo DMA3 ROM→RAM: rejeitada; ignora width, timing, endereço, count, IRQ e outros canais.
- Reconhecer somente PCs/encodings ou hash da LeafGreen: rejeitada; amarra semântica a uma ROM e não valida a transação.
- Implementar runtime DMA nesta tranche: rejeitada; exige estado, ABI, acesso à ROM, scheduling, memória e testes de barramento que não são necessários para a prova estática atual. Continua sendo trabalho obrigatório para qualquer pacote que alcance essa sequência.

## Consequências

Discovery pode avançar na rota estática específica que originou a investigação, se a evidência anterior estiver correta. O estado DMA aumenta o custo por estado, mas não paraleliza uma análise de ROM. A fonte mGBA mostra timing/CPU halt e IRQ condicional; esses eventos permanecem fora do perfil estático. O runtime local segue strict-AOT e faults explícitos, sem fallback.

A aceitação exige testes negativos para stores parciais, outros canais, control inesperado, fonte/destino inválidos, overflow e DMA não ocioso; teste positivo O0/O2 e nova compilação estática de LeafGreen com hash fixo. O resultado da compilação não será descrito como execução do jogo.

## Referências

- [SPEC-049](../specs/SPEC-049-discovery-safe-dma3-latches.md).
- [SPEC-048](../specs/SPEC-048-discovery-io-fact-invalidation.md).
- [SPEC-045](../specs/SPEC-045-thumb-ie-ime-latches.md).
- [SPEC-046](../specs/SPEC-046-strict-aot-entry-state.md).
- [SRC-43](../knowledge/KNO-001-evidence.md).
- [KNO-004](../knowledge/KNO-004-rom-intake.md).
