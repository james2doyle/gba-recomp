# ADR-007 — Fixar cargas literais PC-relativas da ROM

- Estado: Proposed
- Data: 2026-10-13
- Supersede: —

## Contexto

A ROM LeafGreen avança do primeiro `MSR CPSR` para `0xE59FD028`. Resolver esse acesso como leitura dinâmica exigiria bus, estado de memória e semântica de carga GBA, ainda não implementados. O formato é uma carga por PC de uma palavra ROM constante, se a forma e o endereço forem validados.

## Decisão proposta

Adicionar um único caso AOT para `LDR` de palavra PC-relativa, immediate pre-index sem writeback e destino que não seja PC. A descoberta calcula `PC+8 ± imm12`, confere alinhamento e limites, lê quatro bytes da `RomImage` associada ao hash e coloca o valor inteiro na GBA IR. LLVM emite esse valor como constante. Toda forma dinâmica ou fora da ROM continua recusada.

## Alternativas

- Adicionar um mapa de memória host ao runtime sem bus GBA criaria ponteiros incompatíveis com endereços guest e faria `LDR` parecer suportado além do contrato.
- Recusar a forma deixaria o boot parado mesmo quando o endereço lido é comprovadamente parte da ROM imutável.
- Embutir bytes arbitrários sem domínio/alinhamento associaria semântica host a endereços guest sem validação.

## Consequências

O valor compilado depende de SHA-256 e bytes da ROM de entrada, que o manifesto já registra. Nenhuma ROM é copiada ao pacote. Literal load não implementa RAM, MMIO, open bus, timing real nem atualização de código dinâmico.

## Critérios para revisitar

Substituir ou complementar esse lowering quando SPEC-005 e o bus da SPEC-002 cobrirem leitura guest. O caso PC-ROM deve continuar equivalente ao caminho de memória validado para ROM.

## Referências

[SPEC-014](../specs/SPEC-014-rom-literal-loads.md), [SPEC-002](../specs/SPEC-002-armv4t-semantics.md), [SRC-12/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
