# ADR-029 — Helper halfword para Thumb LDRH imediato

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

SPEC-042 adiciona LDRH imediato Thumb Format 10, cuja fronteira estática LeafGreen observada é `0x8821` em `0x08000B84`. O runtime já tem helper de load byte e word e helper de store halfword, mas não uma API para leitura halfword.

As referências mGBA e gba-recomp fixadas calculam `base + 2*imm5`, leem a halfword alinhada para baixo e, para EA ímpar, rotacionam à direita o valor zero-estendido em 8 bits no domínio u32. Essa regra local é observação de cores de referência, não promessa de hardware independente.

## Decisão

Adicionar `gba_load16_v3(rt,out,guest_ea,host_value_out)` como símbolo aditivo ABI v3. O helper valida a halfword alinhada em EWRAM/IWRAM físicas, carrega bytes little-endian, aplica `rotate_right(8*(guest_ea&1))` em u32 e só escreve a saída temporária após sucesso. Fault reportável preenche `out` com os dados de fault, informa o EA guest original e mantém `host_value_out`, CPU e RAM intactos. Se qualquer ponteiro de saída for nulo, retorna fault sem modificar o ponteiro válido.

O decoder, IR, descoberta e LLVM aceitam somente Thumb LDRH imediato Format 10. `Rd==Rb` usa snapshot do valor antigo; a operação não altera flags nem writeback. LDRH por registrador, LDRSH e outras regiões continuam rejeitados ou faultam de modo explícito, sem fallback.

## Alternativas

- Compor a leitura com duas chamadas a `gba_load8_v3`: rejeitada, porque fragmenta uma leitura halfword e não centraliza validação/atomicidade do intervalo.
- Usar `gba_load32_v3`: rejeitada, porque o helper tem validação de quatro bytes e uma regra de rotação distinta.
- Tratar halfwords ímpares como fault de alinhamento ou simplesmente alinhar sem rotação: rejeitada nesta tranche, pois diverge dos dois cores fixados para LDRH GBA.
- Mapear endereço guest para ponteiro host ou aceitar MMIO genérico: rejeitados por contornar a validação de regiões e exigir contrato de bus não existente.

## Consequências

O ABI v3 recebe um símbolo adicional, sem mudança de layout/versão nem de manifesto ou GBA IR v2. O contrato de RAM permanece físico e limitado a EWRAM/IWRAM. A próxima compilação estática da LeafGreen pode alcançar outra fronteira, mas não prova região dinâmica, execução, compatibilidade ou suporte do jogo.

## Critérios para revisitar

- Evidência independente divergir da rotação odd-EA observada nos cores fixados.
- A leitura necessária estiver fora de EWRAM/IWRAM e exigir definir bus/MMIO.
- Medições ou um teste diferencial de hardware mostrarem semântica diferente.

## Referências

- [SPEC-042 — Thumb LDRH imediato](../specs/SPEC-042-thumb-ldrh-immediate.md)
- [ADR-017 — helper halfword aditivo para STRH](ADR-017-additive-halfword-store-abi.md)
- [KNO-001 / SRC-40](../knowledge/KNO-001-evidence.md)
- [KNO-004 — ROM LeafGreen fixada](../knowledge/KNO-004-rom-intake.md)
