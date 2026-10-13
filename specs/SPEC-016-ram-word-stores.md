# SPEC-016 — STR ARM de palavra para RAM física

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Adicionar o primeiro acesso dinâmico à memória guest: `STR` ARM de palavra por offset imediato para endereços físicos de EWRAM ou IWRAM. O acesso passa por helper do ABI GBA v3; o endereço guest continua `u32`. Este incremento não implementa um bus GBA geral.

## Requisitos normativos

- STR-01: aceitar somente `STR` ARM word, offset imediato, pre-indexed, sem writeback, `Rn` e `Rd` em r0-r14, condição definida. Loads, bytes, registrador, post-index, writeback e PC como base/destino continuam recusados.
- STR-02: calcular endereço efetivo em runtime como `Rn ± imm12` em aritmética guest módulo 2^32. Quando a condição falha, não chamar um acesso com efeito; preservar registradores e CPSR, avançar PC quatro bytes e somar um ciclo abstrato.
- STR-03: ABI v3 aditivo MUST preservar `GbaRuntimeV2` e aninhá-lo em `GbaRuntimeV3 { cpu, memory, abi_version }`. `GbaMemoryV1` contém bytes EWRAM de 256 KiB e IWRAM de 32 KiB. Layouts e versões são fixos e verificados em Rust, C e LLVM.
- STR-04: `gba_store32_v3` MUST validar versão, ponteiro de memória, alinhamento de quatro bytes e faixa física antes de escrever. Faixas iniciais: `[0x02000000,0x02040000)` e `[0x03000000,0x03008000)`. Armazenamento é little-endian. Nenhum endereço guest é convertido em ponteiro host.
- STR-05: acesso fora dessas faixas ou desalinhado retorna `fault` com PC, endereço guest e código de falha; memória, registradores, PC e ciclo permanecem inalterados. Não há fallback, interpretação ou JIT.
- STR-06: o runner headless inicializa RAM a zero para resultado reprodutível. Isso define a fixture, não comportamento de power-on do hardware. Writes em RAM têm um ciclo abstrato no incremento atual; waitstates e eventos pertencem à SPEC-005.
- STR-07: mirrors, ROM, BIOS, MMIO, palette, VRAM, OAM, backup, leituras, RMW, DMA, invalidação de código e timing de bus continuam não suportados neste contrato.

## Layout ABI v3

`GbaRuntimeV3` usa `GbaRuntimeV2` no offset 0 (size 192), ponteiro para `GbaMemoryV1` no offset 192 e versão v3 no offset 200; size 208/alignment 8 em targets de 64 bits. `GbaExitV3` contém `reason`, `next_pc`, `fault_address` e `fault_code` em offsets 0/4/8/12, size 16/alignment 4. A função emitida é `gba_block_v3(GbaRuntimeV3*, GbaExitV3*)`.

`GbaMemoryV1` tem EWRAM `[0x02000000,0x02040000)` seguida por IWRAM `[0x03000000,0x03008000)`, total `0x48000` bytes e alinhamento 1. O pointer de host no contexto aponta para essas regiões do runtime; o endereço guest permanece inteiro e é validado antes de indexar os arrays.

## Critérios de aceite

| Caso | Resultado esperado |
|---|---|
| `STR r0,[r1]` para EWRAM base | Valor little-endian armazenado e observado pelo runner |
| `STR` para IWRAM física | Valor armazenado na faixa IWRAM |
| Condição falsa com endereço sem suporte | Nenhum acesso nem falha; PC/ciclo avançam |
| Região fora de EWRAM/IWRAM | `fault` com PC/endereço; estado e RAM sem mutação |
| Endereço desalinhado ou ABI inválida | Falha atômica antes da escrita |
| O0/O2 | Mesmo estado e mesmas escritas da fixture |
| Família fora do escopo | `unsupported_instruction` no compile; sem pacote parcial |

## Evidência e limites

A compilação de LeafGreen alcançou `0x08000224`, encoding `0xE5810000`, após os incrementos anteriores. A leitura como `STR r0,[r1]` é inferência pelos campos ARM. SRC-14 observa que mGBA despacha writes por região e aplica máscaras de tamanho às RAM; nenhuma semântica desse incremento foi validada em hardware.
