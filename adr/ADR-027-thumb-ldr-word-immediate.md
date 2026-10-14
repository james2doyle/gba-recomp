# ADR-027 — Helper aditivo para LDR word Thumb

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

SPEC-040 adiciona Thumb LDR word imediato dinâmico. O runtime já tem `gba_load8_v3` e `load_le32` interno, mas não uma API que leia word guest. A fronteira LeafGreen em `0x080006BC` é inferida como `LDR r0,[r0,#8]`; o endereço calculado `0x040000DC` também é inferido e cai no registrador MMIO `DMA3CNT_LO`.

## Decisão

Adicionar `gba_load32_v3` como helper ABI v3 aditivo, recebendo endereço guest `uint32_t` e escrevendo uma saída host temporária `uint32_t*` somente após leitura válida. Validar uma word little-endian em EWRAM/IWRAM físicas no endereço alinhado para baixo e aplicar ROR por `8*(EA&3)` ao resultado. Fault reporta o EA original e não modifica temporário ou estado guest.

O decoder/IR/lowering implementa somente Thumb Format 9 `0x6800`. A descoberta torna Rd desconhecido. O helper não lê MMIO, ROM, mirrors nem outras regiões; `0x040000DC` falha explicitamente em runtime. Não há fallback.

## Alternativas

- Reutilizar o caminho do LDMIA: rejeitada, pois LDMIA é uma operação múltipla com contrato próprio e não aplica a rotação escalar definida para LDR word não alinhado.
- Reutilizar `gba_load8_v3` para compor quatro leituras: rejeitada, pois faria chamadas múltiplas e não representaria uma leitura/validação única da word alinhada.
- Converter EA guest em ponteiro host: rejeitada, pois contorna o mapeamento e a validação de região.
- Tratar MMIO como RAM: rejeitada; DMA exige contrato separado de barramento e efeitos de dispositivo.

## Consequências

O helper amplia a ABI por símbolo sem alterar layouts ou a versão ABI v3, GBA IR v2 ou manifesto. A operação suporta rotação de unaligned loads somente quando a word alinhada está integralmente em RAM física. Em LeafGreen, a compilação pode avançar, mas executar o acesso inferido a DMA3 continua resultando em fault; esse incremento não declara suporte ao jogo.

## Critérios para revisitar

- Um cenário exigir leitura dinâmica de MMIO ou espelhamento de regiões: definir contrato de bus antes de ampliar este helper.
- Evidência de hardware/oráculo divergir da rotação ou das bordas de região especificadas.

## Referências

- [SPEC-040 — Thumb LDR word imediato](../specs/SPEC-040-thumb-ldr-word-immediate.md)
- [SPEC-029 — Thumb LDRB imediato](../specs/SPEC-029-thumb-ldrb-immediate.md)
- [SPEC-035 — Thumb LDMIA](../specs/SPEC-035-thumb-ldmia.md)
- [KNO-004 — LeafGreen fixada](../knowledge/KNO-004-rom-intake.md)
