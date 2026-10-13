# SPEC-014 — Loads literais PC-relativas em ROM

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Permitir que o decoder AOT compile `LDR` de palavra imediata com base PC quando o endereço efetivo aponta para uma palavra alinhada e imutável dentro da ROM. A descoberta lê o valor pela abstração `RomImage`; o lowering o emite como constante. Isso não implementa bus, memória guest geral, RAM, MMIO ou comportamento de cargas dinâmicas.

## Requisitos normativos

- LIT-01: aceitar somente `LDR` ARM de palavra, offset imediato, pré-indexado, sem writeback, base `r15`, destino `r0-r14` e condição ARM definida. Formas por registrador, byte, post-index, writeback e outras bases continuam não suportadas neste incremento.
- LIT-02: calcular endereço guest efetivo com base `PC+8` e deslocamento `imm12` com sinal de U. A conta é checked; endereço não alinhado ou fora da ROM retorna erro antes de publicar pacote.
- LIT-03: descoberta lê exatamente quatro bytes por `RomImage::read_arm_word` e fixa o valor guest `u32` na instrução IR. LLVM só usa esse inteiro como constante; nunca converte endereço guest em ponteiro host nem emite leitura de memória host.
- LIT-04: quando condição falha, destino permanece inalterado, mas PC avança quatro bytes e `guest_cycle` avança uma unidade abstrata. Quando passa, destino recebe o valor literal e os mesmos PC/ciclo avançam.
- LIT-05: somente bytes da imagem ROM identificada pelo SHA-256 podem ser embutidos por esse caso. Dados em EWRAM/IWRAM/VRAM/MMIO, ROM banked e código self-modifying permanecem fora do contrato.

## Critérios de aceite

| Caso | Resultado esperado |
|---|---|
| Fixture com literal ROM conhecido | O0/O2 carregam o mesmo valor e produzem o mesmo PC/ciclo |
| Condição falsa | Destino não muda; PC/ciclo avançam |
| `LDR` por PC com endereço fora da ROM | Erro com PC guest; nenhum pacote parcial |
| Endereço efetivo desalinhado | Erro antes de emissão |
| Outra forma de `LDR`/`STR` | `unsupported_instruction`; sem fallback |
| Endereço guest | Permanece inteiro em diagnóstico e IR; nenhum ponteiro guest/host é misturado |

## Evidência e limites

A palavra `0xE59FD028` foi observada como primeiro limite do decoder no compile da ROM local, em `0x0800020C`. Sua interpretação como `LDR` PC-relativa é inferência pelos campos ARM. O endereço calculado `0x0800023C` só se torna evidência de execução após a fixture e o lowering passarem. mGBA consulta `memory.load32` para `LDR`; isso reforça que o caso estático aqui é uma otimização restrita a bytes ROM, não um substituto do bus geral. Fonte e revisão estão em KNO-001 SRC-12.
