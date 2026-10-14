# ADR-025 — Budget aumentado de estados completos

- Estado: Accepted
- Data: 2026-10-14
- Supersede: budget por PC definido em ADR-024/SPEC-037

## Contexto

SPEC-037 limita discovery a 64 estados completos por `(modo, PC)` para proteger memória e custo de análise sem descartar correlações. Na LeafGreen fixada em KNO-004, o loop Thumb em `0x08000996` possui pelo menos 96 estados distintos do contador: `r2` percorre `0..95` no cabeçalho. A compilação falha no 65º estado antes de atingir o fixed point, embora o branch adjacente `CMP r2,#95; BLE` prove uma saída finita.

## Decisão

Conforme SPEC-038, elevar somente o budget por PC de 64 para 128. Preservar worklist exato, deduplicação por igualdade, teto global de 100.000 estados, sombra de 64 words, limite de 16 bases ROM e falha explícita acima dos budgets. Não fazer merge nem introduzir fallback.

128 admite o loop observado de 96 estados com margem limitada e mantém uma barreira clara contra crescimento maior. Este é um limite de recurso, não uma afirmação de suporte ao jogo.

## Consequências

Loops finitos com até 128 alternativas completas em uma PC podem convergir, pagando o custo de processar e reter esses estados. Discovery de um ROM continua serial; aumentar workers do gate não acelera essa análise. Uma exigência acima de 128 continuará falhando. Uma futura abstração intervalar/relacional requer especificação independente e prova de preservação das bases ROM e máscaras de branch.

## Critérios para revisitar

- O loop de LeafGreen excede 128 estados em outro PC.
- Evidência de memória/tempo em ROMs reais mostra custo inaceitável.
- Um domínio intervalar/relacional pode manter certificados conservadores de edges e tabelas ROM sem perder correlações.

## Referências

- [SPEC-038 — budget de loops finitos](../specs/SPEC-038-discovery-state-budget.md)
- [ADR-024 — especialização finita LDMIA ROM](ADR-024-finite-rom-ldmia-specialization.md)
- [KNO-004 — ROM LeafGreen fixada](../knowledge/KNO-004-rom-intake.md)
