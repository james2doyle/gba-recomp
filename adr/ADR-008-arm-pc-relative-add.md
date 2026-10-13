# ADR-008 — Compilar ADD imediato relativo ao PC

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Após resolver `MSR CPSR` e cargas literais, a compilação do LeafGreen avançou até `0x08000220`, encoding `0xE28F0020`. Decodificar a forma como `ADD r0, pc, #0x20` é inferência pelos campos ARM. O próximo incremento precisa preservar o PC visível do pipeline sem implementar outras formas de data processing.

## Decisão proposta

Aceitar apenas `ADD Rd, PC, #imm` ARM imediato, sem atualização de flags e com destino diferente de PC. O operando PC é `guest_pc+8`; o imediato usa rotação ARM e a soma usa aritmética guest módulo 2^32. A condição ARM continua controlando somente a escrita do destino.

## Alternativas

- Ler o PC de controle diretamente como operando produziria endereço deslocado em quatro bytes.
- Habilitar genericamente data processing também exporia shifts, carry, flags e escritas em PC sem contratos separados.
- Tratar o resultado como ponteiro host confundiria endereços guest com endereços do processo.

## Consequências

É uma operação de cálculo de registrador, não uma carga de memória. CPSR permanece inalterado; condição falsa preserva o destino; PC de controle e ciclo seguem o contrato atual da IR. Outros usos de PC permanecem fora do suporte.

## Critérios para revisitar

Expandir quando uma nova spec cobrir outra operação, forma de shifter ou escrita de PC com casos próprios e fixture diferencial.

## Referências

[SPEC-015](../specs/SPEC-015-arm-pc-relative-add.md), [SPEC-002](../specs/SPEC-002-armv4t-semantics.md), [SRC-13/KNO-001](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
