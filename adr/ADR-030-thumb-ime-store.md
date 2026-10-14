# ADR-030 — Helper v4 restrito para escrita Thumb no IME

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

A descoberta estática mais recente da ROM LeafGreen chega a `BX r0` em `0x08000B6A`. No caminho fallthrough, um Thumb STRH em `0x08000B58` grava o valor abstrato zero no endereço inferido `0x04000208` (IME). O runtime já contém `GbaMachineStateV1.io` e HLE de SWI #1 atualiza IME para zero, mas as operações normais de memória v3 aceitam somente EWRAM/IWRAM. A sombra da SPEC-034 limpa todos os slots para um store fora dessas regiões e perde o LR salvo.

## Decisão

Implementar [SPEC-044](../specs/SPEC-044-thumb-ime-store.md): adicionar `gba_store16_v4` para delegar stores em RAM física ao helper v3 e atualizar somente o latch IME quando o EA alinhado é `0x04000208` e o bit 0 do valor está zero. Bit 0 ativo recebe fault explícito até existir entrega de IRQ. Discovery preserva fatos de stack somente quando endereço e valor são singletons exatamente `0x04000208` e zero. Qualquer outro acesso não coberto conserva a política conservadora atual.

## Alternativas

- Ampliar genericamente `gba_store16_v3` para MMIO: rejeitada, pois o prefixo v3 não contém ponteiro para o estado de máquina.
- Ignorar o store I/O no lowering: rejeitada, pois produziria sucesso silencioso com semântica guest ausente.
- Preservar a sombra para todo store fora de RAM: rejeitada, pois MMIO inclui DMA, IRQ e periféricos com efeitos ainda não modelados.
- Implementar todo o bus, scheduler e IRQ nesta tranche: rejeitada por misturar diversos contratos e impedir evidência isolada do primeiro registrador.

## Consequências e limites

O helper v4 adiciona uma capacidade sem alterar layouts ABI, versões, manifesto ou GBA IR. O subset pode compilar e executar a escrita de IME zero; outras escritas I/O continuam faultando. O estado de máquina mantém o latch, mas este slice não entrega IRQ nem avança periféricos, e não modela DMA preexistente ou interferência assíncrona. A passagem do `BX` estático não demonstra execução ou compatibilidade de LeafGreen. O próximo trabalho de GBA-030 precisa completar leitura/escrita do bus e integração de IRQ/DMA/timers com eventos antes de qualificar cenários de jogo.

## Referências

- [SPEC-044](../specs/SPEC-044-thumb-ime-store.md)
- [SPEC-023](../specs/SPEC-023-thumb-swi-register-ram-reset.md)
- [SPEC-034](../specs/SPEC-034-abstract-memory-discovery.md)
- [SRC-41](../knowledge/KNO-001-evidence.md)
- [ROADMAP](../ROADMAP.md)
